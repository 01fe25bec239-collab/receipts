//! Deterministic provisioning tests for the Workspace-Execution foundation.
//!
//! Every test builds its own throwaway Git repository under the system
//! temporary directory (never inside this repository), configures a local
//! Git identity so nothing depends on any developer's global identity, and
//! cleans up on drop. All Git invocation — production and fixture alike —
//! goes through explicit argv via [`std::process::Command`]; no shell
//! strings exist anywhere. No network remote is ever configured or
//! contacted: every operation is purely local.
//!
//! The throwaway-repository fixtures are shared with the teardown suite in
//! [`crate::test_support`].

use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard};

use crate::error::WorkspaceError;
use crate::handle::{CommitSha, WorkspaceHandle, WorkspaceIsolation, WorkspaceState};
use crate::provision::WorkspaceProvisionRequest;
use crate::test_support::{
    TestRepo, branch_exists, git, porcelain_status, stdout_trimmed, unix_symlink, worktree_head,
};

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

/// Serializes every test that temporarily installs variables into this
/// process's environment, so no two such tests ever mutate concurrently.
static HOSTILE_ENV_LOCK: Mutex<()> = Mutex::new(());

/// Installs hostile variables in this test process's environment while
/// holding [`HOSTILE_ENV_LOCK`], and restores the previous values on drop —
/// including during panic unwinding.
struct HostileEnv {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<OsString>)>,
}

impl HostileEnv {
    fn install(variables: &[(&'static str, &str)]) -> Self {
        let lock = HOSTILE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let saved = variables
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in variables {
            // SAFETY: all environment mutation in this test binary happens
            // while holding HOSTILE_ENV_LOCK, so no other mutating thread
            // runs concurrently; unrelated parallel tests only read the
            // environment when spawning fixture processes.
            unsafe { std::env::set_var(key, value) };
        }
        Self { _lock: lock, saved }
    }
}

impl Drop for HostileEnv {
    fn drop(&mut self) {
        for (key, original) in std::mem::take(&mut self.saved) {
            match original {
                Some(value) => {
                    // SAFETY: see HostileEnv::install.
                    unsafe { std::env::set_var(key, value) };
                }
                None => {
                    // SAFETY: see HostileEnv::install.
                    unsafe { std::env::remove_var(key) };
                }
            }
        }
    }
}

// T13 — provisioning through a repository root containing canonicalizable
// path indirection executes against the canonical repository; a root that
// cannot be canonicalized fails closed before any Git command runs.
#[test]
fn t13_provisioning_through_path_indirection_targets_canonical_repository() {
    let repo = TestRepo::new_nested("wt-t13", "real-repo");
    let base_sha = repo.head_sha();
    let alias = repo.root.path().join("alias-to-repo");
    unix_symlink(repo.path(), &alias);

    let request = WorkspaceProvisionRequest::new(
        alias.as_path(),
        "ws-canonical-root",
        None,
        "task/canonical-root",
        repo.path().join("the worktree"),
        &base_sha,
    )
    .expect("indirected root must pass structural validation");
    request
        .provision()
        .expect("provisioning through a canonicalizable root must succeed");

    assert!(
        branch_exists(&repo, "task/canonical-root"),
        "branch must exist in the physical repository"
    );
    assert_eq!(worktree_head(&repo, "the worktree"), base_sha);

    let missing_root = repo.root.path().join("does-not-exist");
    let error = WorkspaceProvisionRequest::new(
        missing_root.as_path(),
        "ws-unresolvable-root",
        None,
        "task/unresolvable-root",
        repo.path().join("unreachable worktree"),
        &base_sha,
    )
    .expect("request must pass structural validation")
    .provision()
    .expect_err("an unresolvable repository root must fail closed");
    assert!(
        matches!(error, WorkspaceError::RepositoryRootUnresolvable { .. }),
        "unexpected error: {error:?}"
    );
    assert!(!repo.path().join("unreachable worktree").exists());
}

// T14 — after worktree creation, HEAD/status verification operates from the
// canonicalized created-worktree path, while the frozen handle contract
// still reports the validated absolute path that was requested.
#[test]
fn t14_worktree_verification_operates_from_canonical_worktree_path() {
    let repo = TestRepo::new("wt-t14");
    std::fs::create_dir_all(repo.path().join("direct-parent"))
        .expect("create physical parent directory");
    unix_symlink(
        &repo.path().join("direct-parent"),
        &repo.path().join("linked-parent"),
    );
    let base_sha = repo.head_sha();

    let requested = repo.path().join("linked-parent").join("the worktree");
    let request = WorkspaceProvisionRequest::new(
        repo.path(),
        "ws-canonical-wt",
        None,
        "task/canonical-wt",
        requested.clone(),
        &base_sha,
    )
    .expect("indirected worktree path must pass structural validation");
    let handle = request
        .provision()
        .expect("verification through the canonical worktree path must succeed");

    assert_eq!(
        handle.worktree_path(),
        requested,
        "frozen handle semantics carry the validated requested path"
    );
    assert_eq!(
        stdout_trimmed(&git(
            &repo.path().join("direct-parent/the worktree"),
            &["rev-parse", "HEAD"]
        )),
        base_sha
    );
    assert_eq!(
        porcelain_status(&repo.path().join("direct-parent/the worktree")),
        ""
    );

    let canonical_parent =
        crate::git::subprocess_cwd(&repo.path().join("linked-parent"), "boundary probe")
            .expect("symlinked directory must resolve");
    assert_eq!(
        canonical_parent,
        std::fs::canonicalize(repo.path().join("direct-parent")).expect("physical parent")
    );
}

// T15 — a hostile parent-process GIT_DIR cannot redirect provisioning away
// from the explicitly supplied canonical repository.
#[test]
fn t15_hostile_parent_git_dir_cannot_redirect_provisioning() {
    let repo = TestRepo::new("wt-t15");
    let base_sha = repo.head_sha();
    let request = valid_request(
        &repo,
        "ws-hostile-git-dir",
        "task/hostile-git-dir",
        "the worktree",
        &base_sha,
    );
    let handle;
    {
        let _hostile = HostileEnv::install(&[(
            "GIT_DIR",
            repo.path().join("hostile.git").to_str().expect("utf-8"),
        )]);
        handle = request
            .provision()
            .expect("hostile parent GIT_DIR must not redirect provisioning");
    }
    assert_eq!(handle.base_sha().as_str(), base_sha);
    assert_eq!(handle.branch(), "task/hostile-git-dir");
    assert_eq!(worktree_head(&repo, "the worktree"), base_sha);
}

// T16 — a hostile parent-process GIT_WORK_TREE cannot redirect execution.
#[test]
fn t16_hostile_parent_git_work_tree_cannot_redirect_provisioning() {
    let repo = TestRepo::new("wt-t16");
    let base_sha = repo.head_sha();
    let request = valid_request(
        &repo,
        "ws-hostile-work-tree",
        "task/hostile-work-tree",
        "the worktree",
        &base_sha,
    );
    let handle;
    {
        let _hostile = HostileEnv::install(&[(
            "GIT_WORK_TREE",
            repo.path().join("hostile-tree").to_str().expect("utf-8"),
        )]);
        handle = request
            .provision()
            .expect("hostile parent GIT_WORK_TREE must not redirect provisioning");
    }
    assert_eq!(handle.base_sha().as_str(), base_sha);
    assert_eq!(worktree_head(&repo, "the worktree"), base_sha);
}

// T17 — a hostile parent-process GIT_INDEX_FILE does not affect
// provisioning.
#[test]
fn t17_hostile_parent_git_index_file_does_not_affect_provisioning() {
    let repo = TestRepo::new("wt-t17");
    let base_sha = repo.head_sha();
    let request = valid_request(
        &repo,
        "ws-hostile-index",
        "task/hostile-index",
        "the worktree",
        &base_sha,
    );
    let handle;
    {
        let _hostile = HostileEnv::install(&[(
            "GIT_INDEX_FILE",
            repo.path().join("hostile-index").to_str().expect("utf-8"),
        )]);
        handle = request
            .provision()
            .expect("hostile parent GIT_INDEX_FILE must not affect provisioning");
    }
    assert_eq!(handle.base_sha().as_str(), base_sha);
    assert_eq!(worktree_head(&repo, "the worktree"), base_sha);
}

// T18 — hostile ambient Git configuration environment values are blocked:
// none of them reach the child, and provisioning remains deterministic.
#[test]
fn t18_hostile_git_config_environment_does_not_affect_provisioning() {
    let repo = TestRepo::new("wt-t18");
    let base_sha = repo.head_sha();
    let request = valid_request(
        &repo,
        "ws-hostile-config",
        "task/hostile-config",
        "the worktree",
        &base_sha,
    );
    let handle;
    {
        let _hostile = HostileEnv::install(&[
            ("GIT_CONFIG", "/nonexistent/hostile-config"),
            ("GIT_CONFIG_COUNT", "2"),
            ("GIT_CONFIG_KEY_0", "user.name"),
            ("GIT_CONFIG_VALUE_0", "Hostile Actor"),
            ("GIT_CONFIG_KEY_1", "core.hooksPath"),
            ("GIT_CONFIG_VALUE_1", "/nonexistent/hooks"),
            ("GIT_CONFIG_GLOBAL", "/nonexistent/global-config"),
            ("GIT_CONFIG_SYSTEM", "/nonexistent/system-config"),
            (
                "GIT_CEILING_DIRECTORIES",
                repo.root.path().to_str().expect("utf-8"),
            ),
            (
                "GIT_EXEC_PATH",
                repo.path().join("hostile-exec").to_str().expect("utf-8"),
            ),
        ]);
        handle = request
            .provision()
            .expect("hostile Git config environment must not affect provisioning");
    }
    assert_eq!(handle.base_sha().as_str(), base_sha);
    assert_eq!(worktree_head(&repo, "the worktree"), base_sha);
}

// T19 — production execution resolves one absolute, canonical, executable
// `git` binary (never an unqualified "git" lookup), and the fully prepared
// child command carries exactly the documented allowlisted environment.
#[test]
fn t19_production_git_execution_is_absolute_canonical_and_allowlisted() {
    let repo = TestRepo::new("wt-t19");

    let resolved =
        crate::git::resolved_git_executable().expect("git executable must resolve on this host");
    assert!(resolved.is_absolute(), "resolved git must be absolute");
    assert_eq!(resolved.file_name(), Some(OsStr::new("git")));
    let metadata = std::fs::metadata(resolved).expect("resolved git must exist");
    assert!(metadata.is_file(), "resolved git must be a regular file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            metadata.permissions().mode() & 0o111,
            0,
            "resolved git must be executable"
        );
    }
    assert_eq!(
        resolved,
        std::fs::canonicalize(resolved).expect("resolved git must already be canonical"),
        "resolved git must be free of symlink indirection"
    );

    let command = crate::git::prepared_command(
        repo.path(),
        "boundary inspection",
        &[OsStr::new("status"), OsStr::new("--porcelain")],
    )
    .expect("command preparation must succeed");
    assert_eq!(
        command.get_program(),
        resolved.as_os_str(),
        "child program must be the absolute resolved git binary"
    );
    assert_eq!(
        command.get_current_dir(),
        Some(
            std::fs::canonicalize(repo.path())
                .expect("repository directory")
                .as_path()
        ),
        "child working directory must be canonical"
    );
    let argv: Vec<OsString> = command
        .get_args()
        .map(|argument| argument.to_os_string())
        .collect();
    assert_eq!(
        argv,
        vec![OsString::from("status"), OsString::from("--porcelain")],
        "arguments must travel as distinct verbatim argv entries"
    );

    let child_environment: Vec<(String, String)> = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .map(|(key, value)| (key, value.unwrap_or_default()))
        .collect();
    assert_eq!(
        child_environment.len(),
        2,
        "only the documented allowlist may be present: {child_environment:?}"
    );
    assert!(child_environment.contains(&("GIT_CONFIG_NOSYSTEM".to_string(), "1".to_string())));
    assert!(
        child_environment.contains(&("GIT_CONFIG_GLOBAL".to_string(), "/dev/null".to_string()))
    );
    const FORBIDDEN_KEYS: [&str; 16] = [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_COMMON_DIR",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_KEY_0",
        "GIT_CONFIG_VALUE_0",
        "GIT_CEILING_DIRECTORIES",
        "GIT_EXEC_PATH",
        "GIT_SSH",
        "GIT_SSH_COMMAND",
        "GIT_ASKPASS",
        "GIT_TERMINAL_PROMPT",
    ];
    for key in FORBIDDEN_KEYS {
        assert!(
            !child_environment.iter().any(|(name, _)| name == key),
            "{key} must never reach the child environment"
        );
    }
}

// T21 — the subprocess-CWD boundary fails closed on paths that cannot be
// canonicalized (missing directories, dangling links), never falling back
// to the lexical input.
#[test]
fn t21_subprocess_cwd_boundary_fails_closed_on_unresolvable_paths() {
    let repo = TestRepo::new("wt-t21");

    let canonical =
        crate::git::subprocess_cwd(repo.path(), "boundary probe").expect("existing directory");
    assert_eq!(
        canonical,
        std::fs::canonicalize(repo.path()).expect("canonical form of existing directory")
    );

    let missing = repo.path().join("missing-directory");
    unix_symlink(&missing, &repo.path().join("dangling-link"));
    for unresolvable in [missing, repo.path().join("dangling-link")] {
        let error = crate::git::subprocess_cwd(&unresolvable, "boundary probe")
            .expect_err("unresolvable directories must fail closed");
        assert!(
            matches!(error, WorkspaceError::SubprocessCwdUnresolvable { .. }),
            "unexpected error: {error:?}"
        );
    }
}
