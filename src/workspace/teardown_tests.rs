//! Deterministic teardown tests for the Workspace-Execution foundation.
//!
//! Each test provisions through the real provisioning API into its own
//! throwaway Git repository (see [`crate::test_support`]), drives the real
//! teardown API, and verifies outcomes externally by observing Git and the
//! filesystem directly rather than trusting returned handles alone. All
//! fixture invocation is explicit argv with repository-local identities;
//! every operation stays purely local.

use std::path::{Path, PathBuf};

use crate::error::WorkspaceError;
use crate::handle::{CommitSha, WorkspaceHandle, WorkspaceIsolation, WorkspaceState};
use crate::provision::WorkspaceProvisionRequest;
use crate::remote_publish_policy::WorkspaceRemotePublishPolicy;
use crate::teardown::WorkspaceTeardownRequest;
use crate::test_support::{
    TestRepo, branch_exists, git, porcelain_status, stdout_trimmed, worktree_list_porcelain,
    worktree_meta_dir, worktree_registered,
};

/// Provisions one worktree through the real API and returns the handle.
fn provision_worktree(
    repo: &TestRepo,
    workspace_id: &str,
    branch: &str,
    worktree_name: &str,
) -> WorkspaceHandle {
    let base_sha = repo.head_sha();
    WorkspaceProvisionRequest::new(
        repo.path(),
        workspace_id,
        Some("task-teardown"),
        branch,
        repo.path().join(worktree_name),
        &base_sha,
    )
    .expect("teardown-fixture request must pass structural validation")
    .provision()
    .expect("teardown-fixture provisioning must succeed")
}

fn teardown_request(repo: &TestRepo) -> WorkspaceTeardownRequest {
    WorkspaceTeardownRequest::new(repo.path())
}

fn retained_branch_target(repo: &TestRepo, branch: &str) -> String {
    stdout_trimmed(&git(
        repo.path(),
        &[
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}^{{commit}}"),
        ],
    ))
}

/// Commits one additional file inside a linked worktree, advancing that
/// worktree's HEAD beyond the provisioned base while leaving the tree
/// clean.
fn commit_in_worktree(worktree: &Path, relative: &str, content: &str) {
    std::fs::write(worktree.join(relative), content).expect("write worktree file");
    git(worktree, &["add", relative]);
    git(
        worktree,
        &[
            "commit",
            "--quiet",
            "--message",
            &format!("advance {relative}"),
        ],
    );
}

// T1 — successful teardown removes the registered worktree checkout while
// returning a TORN_DOWN handle that preserves every immutable identity
// field and the verified evidence.
#[test]
fn td01_successful_teardown_returns_torn_down_and_verifies_externally() {
    let repo = TestRepo::new("wtd-t01");
    repo.commit_file("notes.txt", "seeded content");
    let base_sha = repo.head_sha();
    let handle = provision_worktree(&repo, "ws-teardown-ok", "task/teardown-me", "the worktree");
    let worktree_path = repo.path().join("the worktree");

    assert!(worktree_path.exists());
    assert!(worktree_registered(&repo, &worktree_path));

    let torn = teardown_request(&repo)
        .teardown(&handle)
        .expect("teardown of a pristine provisioned worktree must succeed");

    assert_eq!(torn.state(), WorkspaceState::TornDown);
    assert_eq!(torn.state().as_str(), "TORN_DOWN");
    assert_eq!(torn.workspace_id(), "ws-teardown-ok");
    assert_eq!(torn.task_id(), Some("task-teardown"));
    assert_eq!(torn.branch(), "task/teardown-me");
    assert_eq!(torn.worktree_path(), worktree_path);
    assert_eq!(torn.base_sha().as_str(), base_sha);
    assert_eq!(
        torn.head_sha().map(CommitSha::as_str),
        Some(base_sha.as_str()),
        "the final head must be the exact verified pre-removal commit"
    );
    assert_eq!(torn.isolation(), WorkspaceIsolation::WorkspaceIsolation);
    assert_eq!(handle.remote_publish_policy(), None);
    assert_eq!(torn.remote_publish_policy(), None);

    // External verification: registration gone, checkout removed, branch
    // retained at the exact original commit.
    assert!(
        !worktree_registered(&repo, &worktree_path),
        "the removed checkout must no longer be registered"
    );
    assert!(
        !worktree_path.exists(),
        "the removed checkout directory must be gone"
    );
    assert!(
        branch_exists(&repo, "task/teardown-me"),
        "the task branch must remain"
    );
    assert_eq!(
        retained_branch_target(&repo, "task/teardown-me"),
        base_sha,
        "the retained branch must still point at the exact original base"
    );
}

// T2 — teardown never deletes the local branch: observe Git directly
// before and after instead of trusting the returned handle.
#[test]
fn td02_teardown_never_deletes_the_local_branch() {
    let repo = TestRepo::new("wtd-t02");
    repo.commit_file("tracked.txt", "content");
    let handle = provision_worktree(&repo, "ws-retention", "task/retained", "the worktree");

    assert!(branch_exists(&repo, "task/retained"));
    let before = retained_branch_target(&repo, "task/retained");

    teardown_request(&repo)
        .teardown(&handle)
        .expect("teardown must succeed");

    assert!(branch_exists(&repo, "task/retained"));
    assert_eq!(
        retained_branch_target(&repo, "task/retained"),
        before,
        "the branch must survive teardown unchanged"
    );
}

// T3 — a modified tracked file refuses teardown; the worktree, its
// registration, the branch, and the modification itself all stay intact.
#[test]
fn td03_dirty_tracked_worktree_refuses_teardown() {
    let repo = TestRepo::new("wtd-t03");
    repo.commit_file("notes.txt", "original content");
    let handle = provision_worktree(
        &repo,
        "ws-dirty-tracked",
        "task/dirty-tracked",
        "the worktree",
    );
    let worktree_path = repo.path().join("the worktree");

    std::fs::write(worktree_path.join("notes.txt"), "locally modified").expect("modify tracked");

    let error = teardown_request(&repo)
        .teardown(&handle)
        .expect_err("a dirty worktree must refuse teardown");
    assert!(
        matches!(error, WorkspaceError::TeardownWorktreeDirty { .. }),
        "unexpected error: {error:?}"
    );

    assert!(worktree_path.exists());
    assert!(worktree_registered(&repo, &worktree_path));
    assert!(branch_exists(&repo, "task/dirty-tracked"));
    assert_eq!(
        std::fs::read_to_string(worktree_path.join("notes.txt")).expect("read back"),
        "locally modified",
        "the modification must remain untouched"
    );
}

// T4 — an untracked file refuses teardown exactly like any other dirt.
#[test]
fn td04_untracked_file_refuses_teardown() {
    let repo = TestRepo::new("wtd-t04");
    repo.commit_file("notes.txt", "seeded");
    let handle = provision_worktree(&repo, "ws-untracked", "task/untracked", "the worktree");
    let worktree_path = repo.path().join("the worktree");

    std::fs::create_dir_all(worktree_path.join("scratch")).expect("create untracked directory");
    std::fs::write(
        worktree_path.join("scratch").join("evidence.txt"),
        "partial",
    )
    .expect("write untracked file");

    let error = teardown_request(&repo)
        .teardown(&handle)
        .expect_err("an untracked file must refuse teardown");
    assert!(
        matches!(error, WorkspaceError::TeardownWorktreeDirty { .. }),
        "unexpected error: {error:?}"
    );

    assert!(worktree_path.exists());
    assert!(worktree_registered(&repo, &worktree_path));
    assert!(branch_exists(&repo, "task/untracked"));
    assert_eq!(
        std::fs::read_to_string(worktree_path.join("scratch/evidence.txt")).expect("read back"),
        "partial",
        "untracked evidence must remain untouched"
    );
}

// T5 — cleanliness alone proves nothing: a worktree whose HEAD advanced
// past the handle's verified evidence refuses teardown even though it is
// perfectly clean, and stays fully intact.
#[test]
fn td05_stale_head_refuses_teardown() {
    let repo = TestRepo::new("wtd-t05");
    repo.commit_file("notes.txt", "seeded");
    let base_sha = repo.head_sha();
    let handle = provision_worktree(&repo, "ws-stale", "task/stale-head", "the worktree");
    let worktree_path = repo.path().join("the worktree");

    commit_in_worktree(&worktree_path, "advanced.txt", "committed locally");
    assert_eq!(
        porcelain_status(&worktree_path),
        "",
        "fixture precondition: the advanced worktree must be clean"
    );

    let error = teardown_request(&repo)
        .teardown(&handle)
        .expect_err("a stale-evidence worktree must refuse teardown");
    assert!(
        matches!(
            &error,
            WorkspaceError::TeardownHeadMismatch { expected, .. } if *expected == base_sha
        ),
        "unexpected error: {error:?}"
    );

    assert!(worktree_path.exists());
    assert!(worktree_registered(&repo, &worktree_path));
    assert!(branch_exists(&repo, "task/stale-head"));
}

// T6 — a checkout that is no longer on the handle's expected branch
// refuses teardown before any destructive action.
#[test]
fn td06_branch_identity_mismatch_refuses_teardown() {
    let repo = TestRepo::new("wtd-t06");
    repo.commit_file("notes.txt", "seeded");
    let handle = provision_worktree(&repo, "ws-detached", "task/on-branch", "the worktree");
    let worktree_path = repo.path().join("the worktree");

    git(&worktree_path, &["checkout", "--quiet", "--detach", "HEAD"]);

    let error = teardown_request(&repo)
        .teardown(&handle)
        .expect_err("a detached checkout must refuse teardown");
    assert!(
        matches!(
            &error,
            WorkspaceError::TeardownBranchMismatch { expected_branch, .. }
                if *expected_branch == "task/on-branch"
        ),
        "unexpected error: {error:?}"
    );

    assert!(worktree_path.exists());
    assert!(worktree_registered(&repo, &worktree_path));
    assert!(branch_exists(&repo, "task/on-branch"));
}

// T7 — a valid handle whose Git-worktree registration was externally
// altered away fails closed with a typed error, never a TORN_DOWN result.
#[test]
fn td07_unregistered_worktree_fails_closed() {
    let repo = TestRepo::new("wtd-t07");
    repo.commit_file("notes.txt", "seeded");
    let handle = provision_worktree(
        &repo,
        "ws-unregistered",
        "task/unregistered",
        "the worktree",
    );
    let worktree_path = repo.path().join("the worktree");

    // Externally corrupt the fixture: delete the administrative directory
    // that registers the linked worktree, leaving the checkout itself in
    // place but unknown to Git.
    let meta = worktree_meta_dir(&repo, &worktree_path)
        .expect("the provisioned worktree must have a registration entry");
    std::fs::remove_dir_all(&meta).expect("remove fixture registration");

    let result = teardown_request(&repo).teardown(&handle);
    let error = result.expect_err("an unregistered worktree must fail closed");
    assert!(
        matches!(error, WorkspaceError::TeardownWorktreeNotRegistered { .. }),
        "unexpected error: {error:?}"
    );

    assert!(
        worktree_path.exists(),
        "the checkout must remain available for recovery"
    );
    assert!(branch_exists(&repo, "task/unregistered"));
}

// T8 — paths containing spaces work end-to-end through argv-only
// provisioning and teardown.
#[test]
fn td08_spaced_paths_teardown_end_to_end() {
    let repo = TestRepo::new_nested("wtd spaced root", "repo dir");
    repo.commit_file("notes.txt", "seeded");
    let base_sha = repo.head_sha();

    let worktree_path = repo.path().join("task worktree with spaces");
    let request = WorkspaceProvisionRequest::new(
        repo.path(),
        "ws spaced id",
        Some("task spaced 42"),
        "task/spaced-teardown",
        worktree_path.clone(),
        &base_sha,
    )
    .expect("spaced inputs must pass structural validation");
    let handle = request
        .provision()
        .expect("spaced provisioning must succeed");

    let torn = WorkspaceTeardownRequest::new(repo.path())
        .teardown(&handle)
        .expect("spaced teardown must succeed");

    assert_eq!(torn.state(), WorkspaceState::TornDown);
    assert!(!worktree_registered(&repo, &worktree_path));
    assert!(!worktree_path.exists());
    assert!(branch_exists(&repo, "task/spaced-teardown"));
    assert_eq!(
        retained_branch_target(&repo, "task/spaced-teardown"),
        base_sha
    );
}

// T9 — no force-removal or shell-driven cleanup path may exist anywhere in
// this slice's sources, and dirtiness can never be bypassed behaviorally.
#[test]
fn td09_no_force_removal_path_exists() {
    // Behavioral half: a dirty worktree cannot be removed, so no caller
    // input can steer teardown past its own cleanliness verification.
    let repo = TestRepo::new("wtd-t09");
    repo.commit_file("notes.txt", "seeded");
    let handle = provision_worktree(&repo, "ws-no-force", "task/no-force", "the worktree");
    std::fs::write(repo.path().join("the worktree").join("notes.txt"), "dirty")
        .expect("dirty the worktree");
    let error = teardown_request(&repo)
        .teardown(&handle)
        .expect_err("dirty state must never be bypassed");
    assert!(matches!(
        error,
        WorkspaceError::TeardownWorktreeDirty { .. }
    ));
    assert!(repo.path().join("the worktree").exists());

    // Static half: the teardown-owned sources contain no force flags, no
    // shell invocations, and no destructive cleanup commands. The terms
    // below are assembled from fragments precisely so this scanner never
    // contains any of them literally itself.
    let source_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/workspace");
    const GUARDED_SOURCES: [&str; 3] = ["teardown.rs", "teardown_tests.rs", "test_support.rs"];
    let forbidden_terms: Vec<String> = [
        ("--", "force"),
        ("\"-", "f\""),
        ("sh ", "-c"),
        ("bash ", "-c"),
        ("zsh ", "-c"),
        ("system", "("),
        ("git ", "clean"),
        ("reset ", "--hard"),
        ("branch -", "D"),
        ("branch -", "d"),
    ]
    .into_iter()
    .map(|(prefix, suffix)| format!("{prefix}{suffix}"))
    .collect();
    for name in GUARDED_SOURCES {
        let content = std::fs::read_to_string(source_dir.join(name))
            .unwrap_or_else(|error| panic!("guarded source {name} must be readable: {error}"));
        for term in &forbidden_terms {
            assert!(
                !content.contains(term.as_str()),
                "{name} must not contain {term:?}: teardown owns no force, shell, or destructive-cleanup path"
            );
        }
    }
}

// §8 — teardown is deliberately not idempotent: a TORN_DOWN handle is
// rejected as an unsupported state instead of silently succeeding twice.
#[test]
fn td10_second_teardown_of_torn_down_handle_is_rejected() {
    let repo = TestRepo::new("wtd-t10");
    repo.commit_file("notes.txt", "seeded");
    let handle = provision_worktree(&repo, "ws-twice", "task/torn-once", "the worktree");
    let torn = teardown_request(&repo)
        .teardown(&handle)
        .expect("first teardown must succeed");
    assert_eq!(torn.state(), WorkspaceState::TornDown);

    let error = teardown_request(&repo)
        .teardown(&torn)
        .expect_err("tearing down an already-torn-down handle must fail closed");
    assert!(
        matches!(
            error,
            WorkspaceError::TeardownUnsupportedState { state } if state == "TORN_DOWN"
        ),
        "unexpected error: {error:?}"
    );
    assert!(
        branch_exists(&repo, "task/torn-once"),
        "nothing about a rejected request may disturb retained evidence"
    );
}

// A handle whose checkout path does not exist cannot have its identity
// proven; teardown fails closed before inspecting anything.
#[test]
fn td11_missing_worktree_checkout_fails_closed() {
    let repo = TestRepo::new("wtd-t11");
    let base_sha = repo.head_sha();
    let phantom = repo.path().join("never-provisioned worktree");
    let handle = WorkspaceHandle::provisioned(
        "ws-phantom".to_string(),
        None,
        "task/phantom".to_string(),
        phantom.clone().into_boxed_path(),
        CommitSha::parse(&base_sha).expect("fixture SHA shape"),
        None,
    );

    let error = teardown_request(&repo)
        .teardown(&handle)
        .expect_err("a nonexistent checkout must fail closed");
    assert!(
        matches!(error, WorkspaceError::WorktreeUnresolvable { .. }),
        "unexpected error: {error:?}"
    );
}

// An unusable repository root fails closed before any Git command runs.
#[test]
fn td12_unresolvable_repository_root_fails_closed() {
    let repo = TestRepo::new("wtd-t12");
    repo.commit_file("notes.txt", "seeded");
    let handle = provision_worktree(&repo, "ws-bad-root", "task/bad-root", "the worktree");

    let missing_root = repo.path().join("does-not-exist");
    let error = WorkspaceTeardownRequest::new(missing_root)
        .teardown(&handle)
        .expect_err("an unresolvable repository root must fail closed");
    assert!(
        matches!(error, WorkspaceError::RepositoryRootUnresolvable { .. }),
        "unexpected error: {error:?}"
    );
    assert!(
        worktree_registered(&repo, &repo.path().join("the worktree")),
        "a rejected request must leave the registration intact"
    );
    let listing = worktree_list_porcelain(&repo);
    assert!(
        listing.contains("worktree "),
        "fixture sanity: registration output must be parseable"
    );
}

#[test]
fn explicit_remote_policies_are_preserved_without_remote_behavior() {
    for policy in [
        WorkspaceRemotePublishPolicy::LocalOnly,
        WorkspaceRemotePublishPolicy::PushOnAccept,
        WorkspaceRemotePublishPolicy::PushAlways,
    ] {
        let repo = TestRepo::new_nested("wtd-policy", "repo");
        let unusable_remote = repo.root.path().join("not-a-git-repository");
        std::fs::write(&unusable_remote, "ordinary local file").expect("create unusable target");
        git(
            repo.path(),
            &[
                "config",
                "remote.origin.url",
                unusable_remote.to_str().expect("fixture path is UTF-8"),
            ],
        );
        let base_sha = repo.head_sha();
        let worktree_path = repo.path().join("policy worktree");
        let request = WorkspaceProvisionRequest::new(
            repo.path(),
            "ws-policy",
            Some("task-policy"),
            "task/policy",
            &worktree_path,
            &base_sha,
        )
        .expect("valid request")
        .with_remote_publish_policy(policy);
        let handle = request.provision().expect("policy must remain data only");
        assert_eq!(handle.remote_publish_policy(), Some(policy));
        assert_eq!(handle.state(), WorkspaceState::Provisioned);
        assert_eq!(handle.workspace_id(), "ws-policy");
        assert_eq!(handle.task_id(), Some("task-policy"));
        assert_eq!(handle.branch(), "task/policy");
        assert_eq!(handle.worktree_path(), worktree_path);
        assert_eq!(handle.base_sha().as_str(), base_sha);
        assert_eq!(handle.head_sha(), Some(handle.base_sha()));
        assert_eq!(handle.isolation(), WorkspaceIsolation::WorkspaceIsolation);
        assert!(worktree_registered(&repo, &worktree_path));
        assert_eq!(porcelain_status(&worktree_path), "");

        let torn = teardown_request(&repo)
            .teardown(&handle)
            .expect("teardown must not execute the remote policy");
        assert_eq!(torn.state(), WorkspaceState::TornDown);
        assert_eq!(torn.remote_publish_policy(), Some(policy));
        assert_eq!(handle.remote_publish_policy(), Some(policy));
        assert_eq!(torn.workspace_id(), handle.workspace_id());
        assert_eq!(torn.task_id(), handle.task_id());
        assert_eq!(torn.branch(), handle.branch());
        assert_eq!(torn.worktree_path(), handle.worktree_path());
        assert_eq!(torn.base_sha(), handle.base_sha());
        assert_eq!(torn.head_sha(), handle.head_sha());
        assert_eq!(torn.isolation(), handle.isolation());
        assert!(!worktree_registered(&repo, &worktree_path));
        assert!(!worktree_path.exists());
        assert_eq!(retained_branch_target(&repo, "task/policy"), base_sha);
        assert!(unusable_remote.is_file());
    }
}
