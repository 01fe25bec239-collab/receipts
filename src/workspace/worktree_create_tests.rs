use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;
use crate::WorkspaceState;
use crate::test_support::{TestRepo, branch_exists, git, worktree_registered};

fn task() -> Option<WorktreeCreateTask> {
    Some(WorktreeCreateTask {
        workspace_id: "ws-1".into(),
        task_id: "task-1".into(),
    })
}

fn request(
    repo: &TestRepo,
    name: &str,
    base: &str,
    task: Option<WorktreeCreateTask>,
) -> WorktreeCreateRequest {
    WorktreeCreateRequest::new(
        repo.path(),
        repo.path().join(name),
        &format!("task/{name}"),
        base,
        task,
    )
    .unwrap()
}

#[test]
fn idempotent_second_call_same_path_without_reset_or_new_handle() {
    let repo = TestRepo::new("wc-idem");
    let base = repo.head_sha();
    let first = request(&repo, "wt", &base, task()).create().unwrap();
    assert_eq!(first.mode(), WorktreeCreateMode::TaskProvisioned);
    assert!(first.path().is_absolute());
    assert_eq!(
        first.path(),
        std::fs::canonicalize(repo.path().join("wt")).unwrap()
    );
    assert_eq!(first.handle().unwrap().state(), WorkspaceState::Provisioned);

    // Edited and committed work is preserved; HEAD is reported as it is.
    std::fs::write(first.path().join("work.txt"), "w").unwrap();
    git(first.path(), &["add", "work.txt"]);
    git(first.path(), &["commit", "-m", "work"]);
    std::fs::write(first.path().join("dirty.txt"), "d").unwrap();
    let head = crate::test_support::stdout_trimmed(&git(first.path(), &["rev-parse", "HEAD"]));

    let second = request(&repo, "wt", &base, task()).create().unwrap();
    assert_eq!(second.mode(), WorktreeCreateMode::TaskAttached);
    assert_eq!(second.path(), first.path());
    assert_eq!(second.head_sha().as_str(), head);
    assert!(!second.clean());
    assert!(
        second.handle().is_none(),
        "no fabricated PROVISIONED handle"
    );
    assert!(first.path().join("dirty.txt").exists());
}

#[test]
fn mismatched_branch_base_or_unregistered_target_refuses() {
    let repo = TestRepo::new("wc-mismatch");
    let base = repo.head_sha();
    request(&repo, "wt", &base, task()).create().unwrap();
    // Same path, different branch.
    let other_branch = WorktreeCreateRequest::new(
        repo.path(),
        repo.path().join("wt"),
        "task/other",
        &base,
        task(),
    )
    .unwrap();
    assert!(matches!(
        other_branch.create(),
        Err(WorkspaceError::WorktreeCreateConflict { .. })
    ));
    assert!(!branch_exists(&repo, "task/other"));
    // Same branch and path, base not in its history.
    let unrelated = repo.commit_file("elsewhere", "x");
    assert!(matches!(
        request(&repo, "wt", &unrelated, task()).create(),
        Err(WorkspaceError::WorktreeCreateConflict { .. })
    ));
    // Same branch requested at a different path refuses (branch exists).
    assert!(matches!(
        WorktreeCreateRequest::new(
            repo.path(),
            repo.path().join("elsewhere-wt"),
            "task/wt",
            &base,
            task()
        )
        .unwrap()
        .create(),
        Err(WorkspaceError::BranchAlreadyExists { .. })
    ));
    // Existing unregistered content is never adopted.
    std::fs::create_dir(repo.path().join("occupied")).unwrap();
    std::fs::write(repo.path().join("occupied/file"), "keep").unwrap();
    assert!(request(&repo, "occupied", &base, task()).create().is_err());
    assert_eq!(
        std::fs::read_to_string(repo.path().join("occupied/file")).unwrap(),
        "keep"
    );
}

#[test]
fn no_task_context_falls_back_to_plain_worktree() {
    let repo = TestRepo::new("wc-plain");
    let base = repo.head_sha();
    let created = request(&repo, "plain", &base, None).create().unwrap();
    assert_eq!(created.mode(), WorktreeCreateMode::PlainCreated);
    assert!(
        created.handle().is_none(),
        "no task admission without context"
    );
    assert!(worktree_registered(&repo, created.path()));
    assert_eq!(created.head_sha().as_str(), base);
    let again = request(&repo, "plain", &base, None).create().unwrap();
    assert_eq!(again.mode(), WorktreeCreateMode::PlainAttached);
    assert_eq!(again.path(), created.path());
}

#[test]
fn create_never_fetches_from_a_configured_remote() {
    let repo = TestRepo::new("wc-nofetch");
    // A remote whose URL would fail loudly if contacted.
    git(
        repo.path(),
        &["remote", "add", "origin", "/nonexistent/remote.git"],
    );
    let base = repo.head_sha();
    request(&repo, "wt", &base, task()).create().unwrap();
    let refs =
        crate::test_support::stdout_trimmed(&git(repo.path(), &["for-each-ref", "refs/remotes"]));
    assert!(refs.is_empty());
}

#[test]
fn completes_under_500ms_on_small_repo() {
    let repo = TestRepo::new("wc-fast");
    let base = repo.head_sha();
    let started = Instant::now();
    request(&repo, "wt", &base, task()).create().unwrap();
    let elapsed = started.elapsed();
    // Recorded for evidence; the bound applies to a quiet host.
    eprintln!("worktree_create_elapsed_ms={}", elapsed.as_millis());
    assert!(elapsed < Duration::from_millis(500), "{elapsed:?}");
}

fn admin_file(repo: &TestRepo, path: &Path) -> PathBuf {
    let root = std::fs::canonicalize(repo.path()).unwrap();
    crate::worktree_binding::admin_dir(&root, &std::fs::canonicalize(path).unwrap())
        .unwrap()
        .join(crate::worktree_binding::BINDING_FILE)
}

fn expect_conflict(result: Result<WorktreeCreateResponse, WorkspaceError>, needle: &str) {
    match result {
        Err(WorkspaceError::WorktreeCreateConflict { detail }) => {
            assert!(detail.contains(needle), "{detail}")
        }
        other => panic!("expected conflict containing {needle:?}: {other:?}"),
    }
}

/// A different base that is still an ancestor of the branch used to receive
/// `TaskAttached`; the recorded ORIGINAL base must match exactly.
#[test]
fn changed_ancestral_base_never_attaches() {
    let repo = TestRepo::new("wc-ancestor");
    let older = repo.head_sha();
    let newer = repo.commit_file("later", "x");
    let created = request(&repo, "wt", &newer, task()).create().unwrap();
    assert_eq!(created.mode(), WorktreeCreateMode::TaskProvisioned);
    // `older` is an ancestor of the branch, but it was not the original base.
    expect_conflict(request(&repo, "wt", &older, task()).create(), "base");
    // The exact original base still attaches.
    let again = request(&repo, "wt", &newer, task()).create().unwrap();
    assert_eq!(again.mode(), WorktreeCreateMode::TaskAttached);
    assert!(again.handle().is_none());
}

#[test]
fn different_task_or_workspace_never_attaches_and_nothing_changes() {
    let repo = TestRepo::new("wc-identity");
    let base = repo.head_sha();
    let created = request(&repo, "wt", &base, task()).create().unwrap();
    std::fs::write(created.path().join("keep.txt"), "edited").unwrap();
    let file = admin_file(&repo, created.path());
    let record = std::fs::read(&file).unwrap();
    let head = created.head_sha().clone();
    let other_task = Some(WorktreeCreateTask {
        workspace_id: "ws-1".into(),
        task_id: "task-2".into(),
    });
    let other_workspace = Some(WorktreeCreateTask {
        workspace_id: "ws-2".into(),
        task_id: "task-1".into(),
    });
    expect_conflict(request(&repo, "wt", &base, other_task).create(), "task");
    expect_conflict(
        request(&repo, "wt", &base, other_workspace).create(),
        "workspace",
    );
    assert_eq!(std::fs::read(&file).unwrap(), record, "binding untouched");
    assert_eq!(
        std::fs::read_to_string(created.path().join("keep.txt")).unwrap(),
        "edited"
    );
    let still = request(&repo, "wt", &base, task()).create().unwrap();
    assert_eq!(still.head_sha(), &head);
}

#[test]
fn task_and_plain_modes_are_never_promoted_or_mixed() {
    let repo = TestRepo::new("wc-modes");
    let base = repo.head_sha();
    // Plain worktree: a task request cannot attach and creates no marker.
    let plain = request(&repo, "plain", &base, None).create().unwrap();
    expect_conflict(
        request(&repo, "plain", &base, task()).create(),
        "no task binding",
    );
    let plain_marker = crate::worktree_binding::admin_dir(
        &std::fs::canonicalize(repo.path()).unwrap(),
        plain.path(),
    )
    .unwrap()
    .join(crate::worktree_binding::BINDING_FILE);
    assert!(!plain_marker.exists(), "no promotion to task context");
    assert_eq!(
        request(&repo, "plain", &base, None)
            .create()
            .unwrap()
            .mode(),
        WorktreeCreateMode::PlainAttached
    );
    // Task worktree: a no-task request cannot attach it.
    request(&repo, "tasked", &base, task()).create().unwrap();
    expect_conflict(
        request(&repo, "tasked", &base, None).create(),
        "bound to a task",
    );
}

#[test]
fn missing_or_corrupt_binding_refuses_attach_and_is_not_repaired() {
    let repo = TestRepo::new("wc-corrupt");
    let base = repo.head_sha();
    let created = request(&repo, "wt", &base, task()).create().unwrap();
    let file = admin_file(&repo, created.path());
    let good = std::fs::read(&file).unwrap();

    let mut corrupt = good.clone();
    let at = corrupt.len() - 3;
    corrupt[at] = if corrupt[at] == b'0' { b'1' } else { b'0' };
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::write(&file, &corrupt).unwrap();
    expect_conflict(request(&repo, "wt", &base, task()).create(), "binding");
    assert_eq!(std::fs::read(&file).unwrap(), corrupt, "not repaired");

    std::fs::remove_file(&file).unwrap();
    expect_conflict(request(&repo, "wt", &base, task()).create(), "missing");
    assert!(!file.exists(), "not recreated from ancestry");
    assert!(created.path().exists() && branch_exists(&repo, "task/wt"));
}

#[test]
fn clean_rollback_after_a_binding_failure_lets_the_same_identity_retry() {
    let repo = TestRepo::new("wc-bind-rollback");
    let base = repo.head_sha();
    crate::provision::PROVISION_FAULT.set(Some(crate::provision::ProvisionFault::Binding));
    let error = request(&repo, "wt", &base, task()).create().unwrap_err();
    assert!(
        matches!(error, WorkspaceError::GitCommandFailed { .. }),
        "{error:?}"
    );
    assert!(!branch_exists(&repo, "task/wt") && !repo.path().join("wt").exists());
    let created = request(&repo, "wt", &base, task()).create().unwrap();
    assert_eq!(created.mode(), WorktreeCreateMode::TaskProvisioned);
    assert!(admin_file(&repo, created.path()).is_file());
}

/// A real binding-write failure whose rollback cannot complete is reported as
/// rollback-incomplete; the retained worktree can never be task-attached.
#[test]
fn interrupted_creation_is_reported_incomplete_and_never_attaches() {
    let repo = TestRepo::new("wc-bind-blocked");
    let base = repo.head_sha();
    crate::provision::PROVISION_FAULT.set(Some(crate::provision::ProvisionFault::BindingBlocked));
    let error = request(&repo, "wt", &base, task()).create().unwrap_err();
    let admin = {
        let root = std::fs::canonicalize(repo.path()).unwrap();
        // Git may already have removed the clean checkout itself; the
        // registration is found by its recorded path.
        crate::worktree_binding::admin_dir(&root, &root.join("wt")).unwrap()
    };
    // Restore access so fixtures can be removed, whatever happens below.
    let restore = || {
        let _ = std::fs::set_permissions(&admin, std::fs::Permissions::from_mode(0o700));
    };
    let attach = request(&repo, "wt", &base, task()).create();
    restore();
    match error {
        WorkspaceError::ProvisionRollbackIncomplete { cause, detail } => {
            assert!(
                matches!(*cause, WorkspaceError::WorktreeBindingFailed { .. }),
                "{cause:?}"
            );
            assert!(detail.contains("retained"), "{detail}");
        }
        other => panic!("expected incomplete rollback, got {other:?}"),
    }
    // Attributable resources were retained, not deleted and not adopted.
    assert!(branch_exists(&repo, "task/wt") && admin.exists());
    expect_conflict(attach, "missing");
}
