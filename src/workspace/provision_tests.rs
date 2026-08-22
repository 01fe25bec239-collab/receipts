//! Deterministic provisioning tests for the Workspace-Execution foundation.
//!
//! Every test builds its own throwaway Git repository under the system
//! temporary directory (never inside this repository), configures a local
//! Git identity so nothing depends on any developer's global identity, and
//! cleans up on drop. All Git invocation — production and fixture alike —
//! goes through explicit argv via [`std::process::Command`]; no shell
//! strings exist anywhere. No network remote is ever configured or
//! contacted: every operation is purely local.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::WorkspaceError;
use crate::handle::{CommitSha, WorkspaceHandle, WorkspaceIsolation, WorkspaceState};
use crate::provision::WorkspaceProvisionRequest;

/// A temporary directory removed on drop.
struct TempDir {
    root: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "receipts-workspace-test-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("temporary test directory creation");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A throwaway local Git repository with one seeded empty commit.
struct TestRepo {
    root: TempDir,
}

impl TestRepo {
    fn new(tag: &str) -> Self {
        let root = TempDir::new(tag);
        git(
            root.path(),
            &["init", "--quiet", "--initial-branch", "main"],
        );
        git(
            root.path(),
            &["config", "user.name", "Receipts Workspace Tests"],
        );
        git(
            root.path(),
            &["config", "user.email", "workspace-tests@receipts.invalid"],
        );
        git(root.path(), &["config", "commit.gpgsign", "false"]);
        git(
            root.path(),
            &["commit", "--quiet", "--allow-empty", "--message", "seed"],
        );
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn head_sha(&self) -> String {
        stdout_trimmed(&git(self.path(), &["rev-parse", "HEAD"]))
    }
}

fn dev_null() -> &'static Path {
    Path::new("/dev/null")
}

/// Runs one fixture Git command with explicit argv and hermetic config.
fn git(directory: &Path, args: &[&str]) -> Output {
    let output = git_raw(directory, args);
    assert!(
        output.status.success(),
        "fixture git command {args:?} failed: {}",
        stderr_text(&output)
    );
    output
}

fn git_raw(directory: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(directory)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", dev_null())
        .env("GIT_CONFIG_SYSTEM", dev_null())
        .args(args)
        .output()
        .expect("git executable must be available for provisioning tests")
}

fn stdout_trimmed(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn stderr_text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

fn valid_request(
    repo: &TestRepo,
    workspace_id: &str,
    branch: &str,
    worktree_name: &str,
    base_sha: &str,
) -> WorkspaceProvisionRequest {
    WorkspaceProvisionRequest::new(
        repo.path(),
        workspace_id,
        Some("task-001"),
        branch,
        repo.path().join(worktree_name),
        base_sha,
    )
    .expect("test request must pass structural validation")
}

fn branch_exists(repo: &TestRepo, branch: &str) -> bool {
    git_raw(
        repo.path(),
        &["rev-parse", "--verify", &format!("refs/heads/{branch}")],
    )
    .status
    .success()
}

fn worktree_head(repo: &TestRepo, worktree_name: &str) -> String {
    stdout_trimmed(&git(
        &repo.path().join(worktree_name),
        &["rev-parse", "HEAD"],
    ))
}

fn porcelain_status(worktree: &Path) -> String {
    stdout_trimmed(&git_raw(worktree, &["status", "--porcelain"]))
}

// T1 — a malformed or non-exact base SHA is rejected before any Git
// command runs, and nothing is created.
#[test]
fn t01_malformed_base_sha_is_rejected() {
    let repo = TestRepo::new("wt-t01");
    let malformed = [
        "",
        "0123456789abcdef0123456789abcdef0123456",
        "0123456789abcdef0123456789abcdef012345678",
        "0123456789ABCDEF0123456789ABCDEF01234567",
        "g123456789abcdef0123456789abcdef01234567",
        "0123456789abcdef0123456789abcdef0123456!",
    ];
    for base_sha in malformed {
        let error = WorkspaceProvisionRequest::new(
            repo.path(),
            "ws",
            None,
            "task/branch",
            repo.path().join("the worktree"),
            base_sha,
        )
        .expect_err("malformed base SHA must be rejected");
        assert!(
            matches!(error, WorkspaceError::CommitShaInvalid { .. }),
            "unexpected error for {base_sha:?}: {error:?}"
        );
        assert!(
            !repo.path().join("the worktree").exists(),
            "no worktree may exist after a rejected request"
        );
    }
}

// T2 — a syntactically valid but nonexistent commit SHA is rejected by
// Git-level verification, with no branch and no worktree created.
#[test]
fn t02_nonexistent_base_commit_is_rejected() {
    let repo = TestRepo::new("wt-t02");
    let unknown_base = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4";
    let request = valid_request(
        &repo,
        "ws-missing-base",
        "task/missing",
        "the worktree",
        unknown_base,
    );
    let error = request
        .provision()
        .expect_err("a nonexistent base commit must fail verification");
    assert!(
        matches!(error, WorkspaceError::BaseCommitVerificationFailed { .. }),
        "unexpected error: {error:?}"
    );
    assert!(!repo.path().join("the worktree").exists());
    assert!(!branch_exists(&repo, "task/missing"));
}

// T3 — successful provisioning creates the requested branch from the
// exact requested commit.
#[test]
fn t03_branch_is_created_from_exact_base_commit() {
    let repo = TestRepo::new("wt-t03");
    let base_sha = repo.head_sha();
    let request = valid_request(
        &repo,
        "ws-branch",
        "task/exact-base",
        "the worktree",
        &base_sha,
    );
    request.provision().expect("provisioning must succeed");
    let branch_target = stdout_trimmed(&git(
        repo.path(),
        &[
            "rev-parse",
            "--verify",
            "refs/heads/task/exact-base^{commit}",
        ],
    ));
    assert_eq!(
        branch_target, base_sha,
        "branch must point at the exact base commit"
    );
}

// T4 — the created worktree HEAD equals the requested base SHA.
#[test]
fn t04_worktree_head_equals_base_sha() {
    let repo = TestRepo::new("wt-t04");
    let base_sha = repo.head_sha();
    let request = valid_request(
        &repo,
        "ws-head",
        "task/head-check",
        "the worktree",
        &base_sha,
    );
    request.provision().expect("provisioning must succeed");
    assert_eq!(worktree_head(&repo, "the worktree"), base_sha);
}

// T5 — the newly provisioned worktree is clean per Git porcelain status.
#[test]
fn t05_newly_provisioned_worktree_is_clean() {
    let repo = TestRepo::new("wt-t05");
    let base_sha = repo.head_sha();
    let request = valid_request(
        &repo,
        "ws-clean",
        "task/clean-check",
        "the worktree",
        &base_sha,
    );
    request.provision().expect("provisioning must succeed");
    assert_eq!(
        porcelain_status(&repo.path().join("the worktree")),
        "",
        "a freshly provisioned worktree must report no porcelain status"
    );
}

// T6 — successful provisioning returns a handle in the frozen PROVISIONED
// state carrying the frozen fields of this slice.
#[test]
fn t06_handle_is_provisioned_with_expected_fields() {
    let repo = TestRepo::new("wt-t06");
    let base_sha = repo.head_sha();
    let request = valid_request(&repo, "ws-handle", "task/handle", "the worktree", &base_sha);
    let handle: WorkspaceHandle = request.provision().expect("provisioning must succeed");
    assert_eq!(handle.workspace_id(), "ws-handle");
    assert_eq!(handle.task_id(), Some("task-001"));
    assert_eq!(handle.branch(), "task/handle");
    assert_eq!(handle.worktree_path(), repo.path().join("the worktree"));
    assert_eq!(handle.base_sha().as_str(), base_sha);
    assert_eq!(
        handle.head_sha().map(CommitSha::as_str),
        Some(base_sha.as_str()),
        "the verified head must equal the requested base"
    );
    assert_eq!(handle.state(), WorkspaceState::Provisioned);
    assert_eq!(handle.state().as_str(), "PROVISIONED");
    assert_eq!(handle.isolation(), WorkspaceIsolation::WorkspaceIsolation);
    assert_eq!(handle.isolation().as_str(), "WORKSPACE_ISOLATION");
}

// T7 — a Git command failure (duplicate branch creation refused by Git) is
// surfaced as the typed Git-command failure.
#[test]
fn t07_duplicate_branch_git_failure_is_surfaced() {
    let repo = TestRepo::new("wt-t07");
    let base_sha = repo.head_sha();
    let first = valid_request(
        &repo,
        "ws-first",
        "task/shared",
        "first worktree",
        &base_sha,
    );
    first.provision().expect("first provisioning must succeed");

    let second = valid_request(
        &repo,
        "ws-second",
        "task/shared",
        "second worktree",
        &base_sha,
    );
    let error = second
        .provision()
        .expect_err("creating an existing branch must fail at the Git level");
    assert!(
        matches!(
            error,
            WorkspaceError::GitCommandFailed {
                operation: "create branch",
                ..
            }
        ),
        "unexpected error: {error:?}"
    );
    assert!(
        !repo.path().join("second worktree").exists(),
        "no worktree may appear when branch creation failed"
    );
}

// T8 — a later-step Git failure can never result in a successful handle.
#[test]
fn t08_git_failure_cannot_produce_a_workspace_handle() {
    let repo = TestRepo::new("wt-t08");
    let base_sha = repo.head_sha();
    std::fs::write(repo.path().join("blocker"), "not a directory").expect("write blocker file");

    // The parent of the requested worktree path is an ordinary file, which
    // passes structural validation but makes `git worktree add` fail.
    let request = WorkspaceProvisionRequest::new(
        repo.path(),
        "ws-blocked",
        None,
        "task/blocked",
        repo.path().join("blocker").join("nested worktree"),
        &base_sha,
    )
    .expect("request must pass structural validation");
    let error = request
        .provision()
        .expect_err("worktree creation behind a file must fail");
    assert!(
        matches!(
            error,
            WorkspaceError::GitCommandFailed {
                operation: "add worktree",
                ..
            }
        ),
        "unexpected error: {error:?}"
    );
    // The branch may already exist as a partial artifact of the failed
    // flow, but no handle could have been constructed from it.
    assert!(branch_exists(&repo, "task/blocked"));
    assert!(
        !repo.path().join("blocker").join("nested worktree").exists(),
        "no handle-producing worktree may exist"
    );
}

// T9 — a valid path containing spaces works end-to-end through
// argv-safe invocation where Git permits it.
#[test]
fn t09_worktree_path_with_spaces_is_argv_safe() {
    let repo = TestRepo::new("wt spaced root");
    let base_sha = repo.head_sha();
    let request = WorkspaceProvisionRequest::new(
        repo.path(),
        "ws spaced",
        Some("task spaced 42"),
        "task/spaced-path",
        repo.path().join("task worktree with spaces"),
        &base_sha,
    )
    .expect("spaced inputs must pass structural validation");
    let handle = request.provision().expect("provisioning must succeed");
    assert_eq!(
        handle.worktree_path(),
        repo.path().join("task worktree with spaces")
    );
    assert_eq!(worktree_head(&repo, "task worktree with spaces"), base_sha);
    assert_eq!(
        porcelain_status(&repo.path().join("task worktree with spaces")),
        ""
    );
}

// T10 — structurally invalid branch names are rejected before any Git
// command runs.
#[test]
fn t10_invalid_branch_names_are_rejected() {
    let repo = TestRepo::new("wt-t10");
    let base_sha = repo.head_sha();
    let invalid = [
        "",
        "-leading-dash",
        "has space",
        "a..b",
        "ends.lock",
        "ends.",
        "ends/",
        "@{shorthand",
        "double//slash",
        "caret^name",
        "tilde~name",
        "colon:name",
    ];
    for branch in invalid {
        let error = WorkspaceProvisionRequest::new(
            repo.path(),
            "ws-branch-validation",
            None,
            branch,
            repo.path().join("the worktree"),
            &base_sha,
        )
        .expect_err("invalid branch names must be rejected");
        assert!(
            matches!(error, WorkspaceError::BranchNameInvalid { .. }),
            "unexpected error for {branch:?}: {error:?}"
        );
        assert!(
            !repo.path().join("the worktree").exists(),
            "no worktree may exist after a rejected branch name"
        );
    }
}

// T11 — relative worktree paths are rejected before any Git command runs.
#[test]
fn t11_relative_worktree_path_is_rejected() {
    let repo = TestRepo::new("wt-t11");
    let base_sha = repo.head_sha();
    let error = WorkspaceProvisionRequest::new(
        repo.path(),
        "ws-relative",
        None,
        "task/relative",
        "relative/the worktree",
        &base_sha,
    )
    .expect_err("relative worktree paths must be rejected");
    assert!(
        matches!(error, WorkspaceError::InvalidWorktreePath { .. }),
        "unexpected error: {error:?}"
    );
}

// T12 — worktree paths colliding with existing filesystem content are
// rejected before any Git command runs.
#[test]
fn t12_existing_content_at_worktree_path_is_rejected() {
    let repo = TestRepo::new("wt-t12");
    let base_sha = repo.head_sha();

    std::fs::write(repo.path().join("occupied-file"), "content").expect("write occupied file");
    let file_error = WorkspaceProvisionRequest::new(
        repo.path(),
        "ws-file-collision",
        None,
        "task/file-collision",
        repo.path().join("occupied-file"),
        &base_sha,
    )
    .expect_err("an existing file at the worktree path must be rejected");
    assert!(matches!(
        file_error,
        WorkspaceError::InvalidWorktreePath { .. }
    ));

    let populated = repo.path().join("populated-dir");
    std::fs::create_dir_all(&populated).expect("create populated directory");
    std::fs::write(populated.join("resident.txt"), "content").expect("write resident file");
    let dir_error = WorkspaceProvisionRequest::new(
        repo.path(),
        "ws-dir-collision",
        None,
        "task/dir-collision",
        populated.clone(),
        &base_sha,
    )
    .expect_err("a populated directory at the worktree path must be rejected");
    assert!(matches!(
        dir_error,
        WorkspaceError::InvalidWorktreePath { .. }
    ));
}
