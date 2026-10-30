use std::time::Duration;

use super::*;
use crate::execution::{LiveProcessTerminalCause, ProcessRunRequest, ProcessTimeoutPolicy};
use crate::test_support::TestRepo;
use crate::{WorkspaceProvisionRequest, WorkspaceTeardownRequest};

fn provisioned(repo: &TestRepo, name: &str) -> WorkspaceHandle {
    WorkspaceProvisionRequest::new(
        repo.path(),
        &format!("ws-{name}"),
        Some("task-1"),
        &format!("task/{name}"),
        repo.path().join(name),
        &repo.head_sha(),
    )
    .unwrap()
    .provision()
    .unwrap()
}

fn git_version_request(root: &std::path::Path) -> ProcessRunRequest {
    let git = crate::git::resolved_git_executable().unwrap();
    ProcessRunRequest::new(git, ["--version"], root, root).unwrap()
}

fn policy() -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(Duration::from_secs(30), Duration::from_millis(200)).unwrap()
}

fn input(id: &str) -> WorkspaceCheckpointLifecycleInput {
    WorkspaceCheckpointLifecycleInput {
        checkpoint_id: id.into(),
        attempt_id: Some("attempt-1".into()),
        kind: WorkspaceCheckpointKind::Progress,
        dirty_diff_ref: None,
        executed_checks: Vec::new(),
        captured_at: WorkspaceDateTimeV1::try_new("2026-10-30T00:00:00Z").unwrap(),
    }
}

#[test]
fn transition_oracle_covers_all_36_pairs_exactly() {
    use WorkspaceState::*;
    let legal = [
        (Provisioned, Active),
        (Checkpointed, Active),
        (Recovered, Active),
        (Active, Checkpointed),
        (Provisioned, Orphaned),
        (Active, Orphaned),
        (Checkpointed, Orphaned),
        (Orphaned, Recovered),
        (Provisioned, TornDown),
        (Checkpointed, TornDown),
        (Recovered, TornDown),
    ];
    let mut count = 0;
    for from in WorkspaceState::ALL {
        for to in WorkspaceState::ALL {
            count += 1;
            assert_eq!(
                from.may_transition_to(to),
                legal.contains(&(from, to)),
                "{} -> {}",
                from.as_str(),
                to.as_str()
            );
        }
    }
    assert_eq!(count, 36);
    for state in WorkspaceState::ALL {
        assert!(!state.may_transition_to(state), "no self-transition");
        assert!(!TornDown.may_transition_to(state), "TORN_DOWN is terminal");
    }
}

#[test]
fn start_checkpoint_reactivate_and_teardown_follow_observed_evidence() {
    let repo = TestRepo::new("life-full");
    let handle = provisioned(&repo, "wt");

    // Capturing before an observed start is illegal and changes nothing.
    let refused = checkpoint_workspace(&handle, input("cp-0")).unwrap_err();
    assert!(matches!(refused, WorkspaceLifecycleError::Transition(_)));

    let (active, attempt) = start_workspace_attempt(
        &handle,
        &git_version_request(handle.worktree_path()),
        &policy(),
    )
    .unwrap();
    assert_eq!(active.state(), WorkspaceState::Active);
    assert_eq!(handle.state(), WorkspaceState::Provisioned);
    let outcome = attempt.wait_collect().unwrap();
    assert_eq!(
        outcome.terminal_cause(),
        LiveProcessTerminalCause::Completed
    );

    // ACTIVE cannot be torn down: no verified safe cleanup boundary exists.
    let error = WorkspaceTeardownRequest::new(repo.path())
        .teardown(&active)
        .unwrap_err();
    assert!(matches!(
        error,
        WorkspaceError::TeardownUnsupportedState { state: "ACTIVE" }
    ));

    std::fs::write(active.worktree_path().join("new.txt"), "x").unwrap();
    crate::test_support::git(active.worktree_path(), &["add", "new.txt"]);
    crate::test_support::git(active.worktree_path(), &["commit", "-m", "work"]);
    let committed = crate::test_support::stdout_trimmed(&crate::test_support::git(
        active.worktree_path(),
        &["rev-parse", "HEAD"],
    ));

    let (checkpointed, checkpoint) = checkpoint_workspace(&active, input("cp-1")).unwrap();
    assert_eq!(checkpointed.state(), WorkspaceState::Checkpointed);
    assert_eq!(checkpoint.core().head_sha().as_str(), committed);
    assert_eq!(checkpointed.head_sha().unwrap().as_str(), committed);
    assert_eq!(checkpoint.core().workspace_id(), handle.workspace_id());
    assert_eq!(checkpoint.core().task_id(), handle.task_id());
    assert_eq!(checkpointed.base_sha(), handle.base_sha());

    // Subsequent admitted work returns to ACTIVE.
    let (again, attempt) = start_workspace_attempt(
        &checkpointed,
        &git_version_request(handle.worktree_path()),
        &policy(),
    )
    .unwrap();
    assert_eq!(again.state(), WorkspaceState::Active);
    attempt.wait_collect().unwrap();

    // Non-force teardown from CHECKPOINTED verifies the checkpointed head.
    let torn = WorkspaceTeardownRequest::new(repo.path())
        .teardown(&checkpointed)
        .unwrap();
    assert_eq!(torn.state(), WorkspaceState::TornDown);
    assert_eq!(torn.head_sha().unwrap().as_str(), committed);
}

#[test]
fn attempt_outside_the_handle_worktree_is_refused_before_start() {
    let repo = TestRepo::new("life-outside");
    let handle = provisioned(&repo, "wt");
    let error =
        start_workspace_attempt(&handle, &git_version_request(repo.path()), &policy()).unwrap_err();
    assert!(matches!(
        error,
        WorkspaceLifecycleError::AttemptOutsideWorkspace
    ));
}

#[test]
fn orphaning_requires_unknown_cleanup_evidence() {
    let repo = TestRepo::new("life-orphan");
    let handle = provisioned(&repo, "wt");
    let known = LiveProcessAttemptError::AlreadyCollectedOrCollecting;
    assert!(matches!(
        orphan_workspace(&handle, &known),
        Err(WorkspaceLifecycleError::NotOrphanEvidence)
    ));
    let orphaned = orphan_workspace(&handle, &LiveProcessAttemptError::ControllerFailed).unwrap();
    assert_eq!(orphaned.state(), WorkspaceState::Orphaned);
    // No public route recovers or tears down an orphan in this wave.
    assert!(matches!(
        WorkspaceTeardownRequest::new(repo.path()).teardown(&orphaned),
        Err(WorkspaceError::TeardownUnsupportedState { state: "ORPHANED" })
    ));
    assert!(matches!(
        start_workspace_attempt(
            &orphaned,
            &git_version_request(handle.worktree_path()),
            &policy()
        ),
        Err(WorkspaceLifecycleError::Transition(_))
    ));
}
