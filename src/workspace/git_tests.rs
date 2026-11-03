//! WE-W1-1 hardening tests. Hostile "programs" are symlinks to `touch`, so a
//! hook/fsmonitor that runs leaves files named after its arguments; no shell
//! script or shell string exists in these fixtures.

use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use crate::error::WorkspaceError;
use crate::provision::WorkspaceProvisionRequest;
use crate::test_support::{TestRepo, git};

fn touch() -> &'static Path {
    for candidate in ["/usr/bin/touch", "/bin/touch"] {
        if Path::new(candidate).is_file() {
            return Path::new(candidate);
        }
    }
    panic!("touch is required for hook fixtures")
}

fn install_hook(repo: &TestRepo, name: &str) {
    let hooks = repo.path().join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::os::unix::fs::symlink(touch(), hooks.join(name)).unwrap();
}

fn request(repo: &TestRepo, name: &str) -> WorkspaceProvisionRequest {
    WorkspaceProvisionRequest::new(
        repo.path(),
        &format!("ws-{name}"),
        None,
        &format!("task/{name}"),
        repo.path().join(name),
        &repo.head_sha(),
    )
    .unwrap()
}

#[test]
fn repo_hook_does_not_run_on_provision() {
    let repo = TestRepo::new("git-hook");
    install_hook(&repo, "post-checkout");
    // Positive control: ordinary Git really runs this hook (touch creates a
    // file named "1", post-checkout's branch-checkout flag).
    git(
        repo.path(),
        &["worktree", "add", "control", "-b", "control"],
    );
    assert!(
        repo.path().join("control/1").exists(),
        "control hook must run"
    );

    let handle = request(&repo, "hardened").provision().unwrap();
    assert!(!handle.worktree_path().join("1").exists(), "hook ran");
}

#[test]
fn fsmonitor_config_not_executed_on_status() {
    let repo = TestRepo::new("git-fsmonitor");
    let monitor = repo.root.path().join("monitor");
    std::os::unix::fs::symlink(touch(), &monitor).unwrap();
    git(
        repo.path(),
        &["config", "core.fsmonitor", monitor.to_str().unwrap()],
    );
    // Positive control: plain status invokes the configured monitor, which
    // touches a file named after its protocol version ("2" or "1").
    git(repo.path(), &["status", "--porcelain"]);
    let ran = |dir: &Path| dir.join("2").exists() || dir.join("1").exists();
    assert!(ran(repo.path()), "control fsmonitor must run");
    for name in ["1", "2"] {
        let _ = std::fs::remove_file(repo.path().join(name));
    }
    let _ = std::fs::remove_dir_all(repo.path().join(".git/fsmonitor--daemon"));

    let status = crate::git::capture(
        repo.path(),
        "hardened status",
        &[OsStr::new("status"), OsStr::new("--porcelain")],
    )
    .unwrap();
    assert!(status.success);
    assert!(!ran(repo.path()), "hardened status ran fsmonitor");
}

#[test]
fn filter_driver_repo_refused() {
    let repo = TestRepo::new("git-filter");
    git(
        repo.path(),
        &["config", "filter.lfs.clean", "/nonexistent/clean"],
    );
    let error = request(&repo, "filtered").provision().unwrap_err();
    assert!(
        matches!(&error, WorkspaceError::RepositoryConfigRefused { key } if key == "filter.lfs.clean"),
        "{error:?}"
    );
    assert!(!repo.path().join("filtered").exists());
    assert!(!crate::test_support::branch_exists(&repo, "task/filtered"));
}

#[test]
fn configuration_include_repo_refused() {
    let repo = TestRepo::new("git-include");
    let outside = repo.root.path().join("outside.gitconfig");
    std::fs::write(&outside, "[user]\n\tname = outside\n").unwrap();
    git(
        repo.path(),
        &["config", "include.path", outside.to_str().unwrap()],
    );
    let error = request(&repo, "included").provision().unwrap_err();
    assert!(
        matches!(&error, WorkspaceError::RepositoryConfigRefused { key } if key == "include.path"),
        "{error:?}"
    );
}

#[test]
fn git_call_times_out_typed() {
    let repo = TestRepo::new("git-timeout");
    let _guard = crate::git::override_git_timeout_for_test(Duration::from_nanos(1));
    let error = crate::git::capture(repo.path(), "slow", &[OsStr::new("version")]).unwrap_err();
    assert!(
        matches!(
            error,
            WorkspaceError::GitTimedOut {
                operation: "slow",
                ..
            }
        ),
        "{error:?}"
    );
}

#[test]
fn oversize_git_output_bounded_with_typed_error() {
    let repo = TestRepo::new("git-oversize");
    let big = "x".repeat(2 * 1024 * 1024);
    repo.commit_file("big.txt", &big);
    let error = crate::git::capture(
        repo.path(),
        "read big blob",
        &[
            OsStr::new("cat-file"),
            OsStr::new("-p"),
            OsStr::new("HEAD:big.txt"),
        ],
    )
    .unwrap_err();
    assert!(
        matches!(error, WorkspaceError::GitOutputTooLarge { total_bytes, .. } if total_bytes == big.len() as u64),
        "{error:?}"
    );
}

#[test]
fn version_floor_is_parsed_strictly() {
    use crate::git::git_version_at_least as at_least;
    assert_eq!(at_least("git version 2.36.0\n", 2, 36), Some(true));
    assert_eq!(
        at_least("git version 2.54.0 (Apple Git-157)", 2, 36),
        Some(true)
    );
    assert_eq!(at_least("git version 2.35.9", 2, 36), Some(false));
    assert_eq!(at_least("git version 1.99", 2, 36), Some(false));
    assert_eq!(at_least("hub version 9", 2, 36), None);
    assert_eq!(at_least("git version x.y", 2, 36), None);
}

#[test]
fn actual_target_worktree_configuration_refuses_before_content_operations() {
    use crate::{WorkspaceTeardownRequest, WorktreeCreateRequest, WorktreeCreateTask};
    let repo = TestRepo::new("target-config");
    repo.commit_file("f", "original\n");
    let base = repo.head_sha();
    let target = repo.root.path().join("target");
    let create = WorktreeCreateRequest::new(
        repo.path(),
        &target,
        "task/target",
        &base,
        Some(WorktreeCreateTask {
            workspace_id: "ws-target".into(),
            task_id: "task-target".into(),
        }),
    )
    .unwrap();
    let first = create.create().unwrap();
    let handle = first.handle().unwrap();
    git(
        repo.path(),
        &["config", "extensions.worktreeConfig", "true"],
    );
    let marker = repo.root.path().join("FILTER_RAN");
    let command = format!("{} {}", touch().display(), marker.display());
    git(
        &target,
        &["config", "--worktree", "filter.x.clean", &command],
    );
    std::fs::write(target.join(".gitattributes"), "f filter=x\n").unwrap();
    std::fs::write(target.join("f"), "modified\n").unwrap();
    crate::git::refuse_untrusted_repository_config(repo.path(), "root control").unwrap();
    assert!(matches!(
        crate::git::refuse_untrusted_repository_config(&target, "target control"),
        Err(WorkspaceError::RepositoryConfigRefused { .. })
    ));
    assert!(!marker.exists());
    // A real unsafe status executes the configured filter; reset only this
    // attributed positive control before exercising the product refusals.
    git(&target, &["status", "--porcelain"]);
    assert!(marker.exists());
    std::fs::remove_file(&marker).unwrap();
    assert!(matches!(
        create.create(),
        Err(WorkspaceError::RepositoryConfigRefused { .. })
    ));
    assert!(!marker.exists(), "attach executed the target filter");
    assert!(matches!(
        WorkspaceTeardownRequest::new(repo.path()).teardown(handle),
        Err(WorkspaceError::RepositoryConfigRefused { .. })
    ));
    assert!(!marker.exists(), "teardown executed the target filter");
    assert!(target.exists());
    let launcher = crate::ProtectedLauncher::unenforced_for_test(
        crate::ProtectedLaunchConfig::new(
            61001,
            61001,
            61002,
            62000..=62009,
            "/sys/fs/cgroup/receipts",
        )
        .unwrap(),
    );
    assert!(matches!(
        launcher.admit(handle, repo.path()),
        Err(crate::ProtectedLaunchError::Workspace(
            WorkspaceError::RepositoryConfigRefused { .. }
        ))
    ));
    assert!(!marker.exists(), "admission executed the target filter");
    assert!(matches!(
        crate::checkpoint_evidence_capture::observe_state(&target),
        Err(crate::WorkspaceCheckpointEvidenceCaptureError::UnsupportedConfiguredFilters)
    ));
    assert!(!marker.exists(), "checkpoint executed the target filter");
    git(
        &target,
        &["config", "--worktree", "--unset", "filter.x.clean"],
    );
    git(
        &target,
        &[
            "config",
            "--worktree",
            "include.path",
            "/nonexistent/unsafe-config",
        ],
    );
    assert!(crate::checkpoint_evidence_capture::observe_state(&target).is_err());
    assert!(!marker.exists());
    let config_path = crate::test_support::stdout_trimmed(&git(
        &target,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "config.worktree",
        ],
    ));
    std::fs::write(config_path, "[invalid\n").unwrap();
    assert!(matches!(
        crate::git::refuse_untrusted_repository_config(&target, "malformed target"),
        Err(WorkspaceError::GitCommandFailed { .. })
    ));
    assert!(!marker.exists());
}
