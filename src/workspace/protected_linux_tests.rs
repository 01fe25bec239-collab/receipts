//! Privileged Linux product checks. `#[ignore]`d: they run ONLY inside the
//! hosted disposable-VM diagnostic (`.github/scripts/we-w1-completion-linux`),
//! as root, with `RECEIPTS_WE_HOSTED_CGROUP` naming a delegated cgroup v2
//! directory. Never run them with sudo on a workstation.

use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::execution::{LiveProcessTerminalCause, ProcessRunRequest, ProcessTimeoutPolicy};
use crate::test_support::{TempDir, git};
use crate::{
    ControllerOperation, ControllerRequest, ControllerResponse, ProtectedLaunchConfig,
    ProtectedLauncher, WorkspaceProvisionRequest,
};

const PROBE_DIR: &str = "RECEIPTS_PROBE";

fn hosted_cgroup() -> PathBuf {
    PathBuf::from(std::env::var_os("RECEIPTS_WE_HOSTED_CGROUP").expect("hosted diagnostic only"))
}

fn config() -> ProtectedLaunchConfig {
    ProtectedLaunchConfig::new(61001, 61001, 61002, 62000..=62063, hosted_cgroup()).unwrap()
}

fn probe_request(worktree: &Path, executable: &Path, name: &str) -> ProcessRunRequest {
    ProcessRunRequest::new(
        executable,
        [
            format!("protected_linux_tests::{name}"),
            "--exact".into(),
            "--ignored".into(),
            "--nocapture".into(),
            "--test-threads=1".into(),
        ],
        worktree,
        worktree,
    )
    .unwrap()
}

fn policy() -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(Duration::from_secs(60), Duration::from_secs(1)).unwrap()
}

fn fixture() -> (TempDir, PathBuf, crate::WorkspaceHandle) {
    let dir = TempDir::new("we-hosted");
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let repo = dir.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "user.name", "fixture"]);
    git(&repo, &["config", "user.email", "fixture@receipts.invalid"]);
    std::fs::write(repo.join("tracked.txt"), "original\n").unwrap();
    git(&repo, &["add", "tracked.txt"]);
    git(&repo, &["commit", "-q", "-m", "base"]);
    let base = crate::test_support::stdout_trimmed(&git(&repo, &["rev-parse", "HEAD"]));
    let handle = WorkspaceProvisionRequest::new(
        &repo,
        "ws-hosted",
        Some("task-hosted"),
        "task/hosted",
        dir.path().join("wt"),
        &base,
    )
    .unwrap()
    .provision()
    .unwrap();
    (dir, repo, handle)
}

/// Worker-side probe (runs as the fresh worker UID inside the boundary).
#[test]
#[ignore = "hosted worker probe"]
fn worker_probe() {
    let Ok(worktree) = std::env::current_dir() else {
        return;
    };
    if !worktree.join(".git").is_file() {
        return; // Only meaningful inside a protected worktree.
    }
    let mut report = Vec::new();
    let mut record = |name: &str, ok: bool| report.push(format!("{name}={ok}"));
    record(
        "uid_nonroot",
        std::fs::metadata("/proc/self").unwrap().uid() != 0,
    );
    let gitdir = std::fs::read_to_string(worktree.join(".git")).unwrap();
    let common = PathBuf::from(gitdir.trim().trim_start_matches("gitdir: "))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf();
    record(
        "config_write_denied",
        std::fs::OpenOptions::new()
            .append(true)
            .open(common.join("config"))
            .is_err(),
    );
    record(
        "pointer_unlink_denied",
        std::fs::remove_file(worktree.join(".git")).is_err(),
    );
    record(
        "pointer_rename_denied",
        std::fs::rename(worktree.join(".git"), worktree.join("moved")).is_err(),
    );
    record(
        "hooks_create_denied",
        std::fs::write(common.join("hooks/post-commit"), "x").is_err(),
    );
    record(
        "payload_edit",
        std::fs::write(worktree.join("tracked.txt"), "edited\n").is_ok(),
    );
    let private = worktree.join("private.txt");
    let wrote = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&private)
        .and_then(|mut file| file.write_all(b"worker secret\n"))
        .is_ok();
    record("private_payload", wrote);
    // Escape the process group; only cgroup containment can reach this.
    let escapee = std::process::Command::new("/usr/bin/sleep")
        .arg("300")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn();
    if let Ok(child) = &escapee {
        std::fs::write(worktree.join("escapee.pid"), child.id().to_string()).unwrap();
    }
    record("escapee_spawned", escapee.is_ok());
    unsafe extern "C" {
        fn unshare(flags: i32) -> i32;
    }
    record("unshare_denied", unsafe { unshare(0x1000_0000) } != 0);
    println!("PROBE {}", report.join(" "));
    std::mem::forget(escapee);
}

/// Controller-identity probe (CAP_DAC_READ_SEARCH + Landlock + seccomp).
#[test]
#[ignore = "hosted controller probe"]
fn controller_probe() {
    let Ok(worktree) = std::env::current_dir() else {
        return;
    };
    let read = |path: &Path| std::fs::read(path).is_ok();
    println!(
        "CONTROLLER private_read={} sentinel_denied={} sentinel_visible={}",
        read(&worktree.join("private.txt")),
        !read(&worktree.join("../sentinel")),
        worktree.join("../sentinel").exists(),
    );
}

#[test]
#[ignore = "privileged hosted diagnostic"]
fn hosted_protected_worker_controller_and_cleanup() {
    let (dir, repo, handle) = fixture();
    let launcher = ProtectedLauncher::new(config()).expect("hosted admission must succeed");
    let admission = launcher.admit(&handle, &repo).expect("custody");
    let config_before = std::fs::read(repo.join(".git/config")).unwrap();

    let worktree = handle.worktree_path().to_path_buf();
    let probe = worktree.join(PROBE_DIR);
    std::fs::create_dir(&probe).unwrap();
    let executable = probe.join("probe");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();

    let protected = launcher
        .launch(
            admission,
            &probe_request(&worktree, &executable, "worker_probe"),
            &policy(),
        )
        .expect("worker launch");
    let outcome = protected
        .attempt()
        .wait_collect()
        .expect("verified cleanup");
    assert_eq!(
        outcome.terminal_cause(),
        LiveProcessTerminalCause::Completed
    );
    let stdout = String::from_utf8_lossy(outcome.stdout().head()).into_owned();
    println!("{stdout}");
    for check in [
        "uid_nonroot=true",
        "config_write_denied=true",
        "pointer_unlink_denied=true",
        "pointer_rename_denied=true",
        "hooks_create_denied=true",
        "payload_edit=true",
        "private_payload=true",
        "escapee_spawned=true",
        "unshare_denied=true",
    ] {
        assert!(stdout.contains(check), "missing {check}");
    }
    let pid = std::fs::read_to_string(worktree.join("escapee.pid")).unwrap();
    let alive = std::fs::read_to_string(format!("/proc/{}/stat", pid.trim()))
        .map(|stat| !stat.contains(") Z "))
        .unwrap_or(false);
    assert!(!alive, "process-group escapee survived cgroup cleanup");
    assert_eq!(
        std::fs::read(repo.join(".git/config")).unwrap(),
        config_before
    );
    assert_eq!(
        std::fs::metadata(worktree.join(".git")).unwrap().uid(),
        61001
    );

    // Controller: read worker 0600 payload without chmod/chown, deny an
    // outside visible sentinel.
    std::fs::write(dir.path().join("sentinel"), "outside").unwrap();
    std::fs::set_permissions(
        dir.path().join("sentinel"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let mode_before = std::fs::metadata(worktree.join("private.txt"))
        .unwrap()
        .mode();
    let worker = protected.worker_channel();
    let credentials = worker.credentials();
    let send = |sequence, operation| {
        worker.handle(ControllerRequest {
            attempt_id: credentials.attempt_id.clone(),
            task_id: credentials.task_id.clone(),
            token: credentials.token.clone(),
            sequence,
            operation,
        })
    };
    send(
        0,
        ControllerOperation::Stage {
            paths: vec!["tracked.txt".into(), "private.txt".into()],
        },
    )
    .expect("controller stages worker payload");
    let ControllerResponse::Committed { commit } = send(
        1,
        ControllerOperation::Commit {
            message: "worker work".into(),
        },
    )
    .expect("commit") else {
        panic!("commit")
    };
    // Root inspects the controller-owned repository explicitly.
    let object = format!("{}:private.txt", commit.as_str());
    let shown = git(&repo, &["-c", "safe.directory=*", "show", &object]);
    assert_eq!(shown.stdout, b"worker secret\n");
    assert_eq!(
        std::fs::metadata(worktree.join("private.txt"))
            .unwrap()
            .mode(),
        mode_before
    );
    let ControllerResponse::Checkpointed(checkpoint) = send(
        2,
        ControllerOperation::Checkpoint {
            checkpoint_id: "hosted".into(),
            kind: crate::WorkspaceCheckpointKind::Progress,
            captured_at: crate::WorkspaceDateTimeV1::try_new("2026-10-30T00:00:00Z").unwrap(),
        },
    )
    .expect("checkpoint") else {
        panic!("checkpoint")
    };
    assert_eq!(checkpoint.core().head_sha(), &commit);

    // Controller identity probe through the same confinement.
    let launch = protected.controller_launch_for_test();
    let attempt = crate::execution::live_attempt_start_customized(
        &probe_request(&worktree, &executable, "controller_probe"),
        &policy(),
        launch.customization().expect("controller customization"),
    )
    .expect("controller probe start");
    let outcome = attempt.wait_collect().expect("controller probe cleanup");
    let stdout = String::from_utf8_lossy(outcome.stdout().head()).into_owned();
    println!("{stdout}");
    for check in [
        "private_read=true",
        "sentinel_denied=true",
        "sentinel_visible=true",
    ] {
        assert!(stdout.contains(check), "missing {check}");
    }
    let leftovers: Vec<_> = std::fs::read_dir(hosted_cgroup())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("receipts-"))
        .collect();
    assert!(leftovers.is_empty(), "owned cgroups remain: {leftovers:?}");
}
