//! Privileged Linux product checks. `#[ignore]`d: they run ONLY inside the
//! hosted disposable-VM diagnostic (`.github/scripts/we-w1-completion-linux`),
//! as root, with `RECEIPTS_WE_HOSTED_CGROUP` naming a delegated cgroup v2
//! directory. Never run them with sudo on a workstation.
//!
//! The probes run inside the real boundary (fresh worker identity, or the
//! confined controller identity) and print `key=bool` receipts; the parent test
//! asserts every expected receipt and the parent-side custody, channel,
//! lifecycle and teardown properties.
//!
//! Probes observe their own confinement through system calls
//! (`getresuid`/`getresgid`/`getgroups`/`capget`/`prctl`/`fcntl`/`getrlimit`),
//! never `/proc`: the controller's Landlock plan grants no `/proc` access and
//! production access is not widened for the probe.

use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::execution::{LiveProcessTerminalCause, ProcessRunRequest, ProcessTimeoutPolicy};
use crate::test_support::{TempDir, git};
use crate::{
    ControllerOperation, ControllerRequest, ControllerResponse, ProtectedLaunchConfig,
    ProtectedLaunchError, ProtectedLauncher, WorkspaceError, WorkspaceProvisionRequest,
    WorkspaceState, WorkspaceTeardownRequest,
};

const PROBE_DIR: &str = "RECEIPTS_PROBE";
const CONTROLLER_ID: u32 = 61001;
const WORKER_GID: u32 = 61002;
const WORKER_RANGE: std::ops::RangeInclusive<u32> = 62000..=62063;
const FILTER_MARKER: &str = "FILTER_RAN";
const CUSTODY_MARKER: &str = "receipts-custody.state";

unsafe extern "C" {
    fn syscall(number: i64, ...) -> i64;
}
const SYS_GETRLIMIT: i64 = 97;
const SYS_FCNTL: i64 = 72;
const SYS_GETGROUPS: i64 = 115;
const SYS_GETRESUID: i64 = 118;
const SYS_GETRESGID: i64 = 120;
const SYS_SETFSUID: i64 = 122;
const SYS_SETFSGID: i64 = 123;
const SYS_CAPGET: i64 = 125;
const SYS_PRCTL: i64 = 157;
const PR_GET_SECCOMP: i64 = 21;
const PR_CAPBSET_READ: i64 = 23;
const PR_GET_NO_NEW_PRIVS: i64 = 39;
const PR_CAP_AMBIENT: i64 = 47;
const PR_CAP_AMBIENT_IS_SET: i64 = 1;
const F_GETFD: i64 = 1;
const RLIMIT_NOFILE: i64 = 7;
const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
const EBADF: i32 = 9;
const SYS_PROCESS_VM_READV: i64 = 310;
const SYS_OPEN_BY_HANDLE_AT: i64 = 304;
const SYS_SETNS: i64 = 308;
const SYS_UNSHARE: i64 = 272;
const SYS_PIDFD_GETFD: i64 = 438;
const EPERM: i32 = 1;

fn hosted_cgroup() -> PathBuf {
    PathBuf::from(std::env::var_os("RECEIPTS_WE_HOSTED_CGROUP").expect("hosted diagnostic only"))
}

fn config() -> ProtectedLaunchConfig {
    ProtectedLaunchConfig::new(
        CONTROLLER_ID,
        CONTROLLER_ID,
        WORKER_GID,
        WORKER_RANGE,
        hosted_cgroup(),
    )
    .unwrap()
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

// ---- probe-side helpers (run inside the boundary) ----

/// Raw system call; `None` when it fails (errno is then read by the caller).
fn call(number: i64, a: i64, b: i64, c: i64) -> Option<i64> {
    // SAFETY: every call below passes integers or pointers to live local
    // buffers of the size the kernel documents for that call.
    let result = unsafe { syscall(number, a, b, c, 0i64, 0i64, 0i64) };
    (result >= 0).then_some(result)
}

/// Real, effective, saved and filesystem IDs of one kind.
fn id_quad(real_effective_saved: i64, fs_setter: i64) -> Option<[u32; 4]> {
    let mut ids = [u32::MAX; 3];
    let base = ids.as_mut_ptr();
    // SAFETY: three adjacent u32 slots, as `getres[ug]id` requires.
    let ok = unsafe {
        syscall(
            real_effective_saved,
            base as i64,
            base.wrapping_add(1) as i64,
            base.wrapping_add(2) as i64,
            0i64,
            0i64,
            0i64,
        )
    } == 0;
    // `setfs[ug]id(-1)` is rejected without effect and returns the CURRENT
    // filesystem ID.
    let fs = call(fs_setter, -1, 0, 0)? as u32;
    ok.then_some([ids[0], ids[1], ids[2], fs])
}

/// All four IDs are one value inside `allowed`.
fn single_id(quad: Option<[u32; 4]>, allowed: std::ops::RangeInclusive<u32>) -> bool {
    quad.is_some_and(|ids| ids.iter().all(|id| *id == ids[0]) && allowed.contains(&ids[0]))
}

/// `(effective, permitted, inheritable)` capability sets of this thread.
fn capability_sets() -> Option<[u64; 3]> {
    let header: [u32; 2] = [LINUX_CAPABILITY_VERSION_3, 0];
    let mut data = [0u32; 6]; // two (effective, permitted, inheritable) words
    call(
        SYS_CAPGET,
        header.as_ptr() as i64,
        data.as_mut_ptr() as i64,
        0,
    )?;
    let set = |slot: usize| u64::from(data[slot]) | (u64::from(data[3 + slot]) << 32);
    Some([set(0), set(1), set(2)])
}

/// Bit set of capabilities for which `query(cap)` reports 1, over every
/// capability the kernel knows (it answers `EINVAL` beyond the last one). An
/// unanswered first query means the set is unobservable, never "empty".
fn capability_mask(query: impl Fn(i64) -> Option<i64>) -> Option<u64> {
    let mut mask = 0u64;
    for cap in 0..64 {
        match query(cap) {
            Some(1) => mask |= 1 << cap,
            Some(_) => {}
            None if cap == 0 => return None,
            None => break,
        }
    }
    Some(mask)
}

/// Exact capability sets, no supplementary groups, no-new-privs, seccomp filter.
fn identity_receipt(uid: std::ops::RangeInclusive<u32>, gid: u32, caps: &str) -> String {
    let expected = u64::from_str_radix(caps, 16).ok();
    let bounding = capability_mask(|cap| call(SYS_PRCTL, PR_CAPBSET_READ, cap, 0));
    let ambient =
        capability_mask(|cap| call(SYS_PRCTL, PR_CAP_AMBIENT, PR_CAP_AMBIENT_IS_SET, cap));
    let [effective, permitted, inheritable] = capability_sets().unwrap_or([u64::MAX; 3]);
    let caps_exact = [
        Some(effective),
        Some(permitted),
        Some(inheritable),
        bounding,
        ambient,
    ]
    .iter()
    .all(|set| *set == expected);
    format!(
        "uid_exact={} gid_exact={} no_groups={} caps_exact={} nnp={} seccomp_filter={}",
        single_id(id_quad(SYS_GETRESUID, SYS_SETFSUID), uid),
        single_id(id_quad(SYS_GETRESGID, SYS_SETFSGID), gid..=gid),
        call(SYS_GETGROUPS, 0, 0, 0) == Some(0),
        caps_exact,
        call(SYS_PRCTL, PR_GET_NO_NEW_PRIVS, 0, 0) == Some(1),
        call(SYS_PRCTL, PR_GET_SECCOMP, 0, 0) == Some(2),
    )
}

/// Only stdio is open: every other descriptor number up to the process limit
/// answers `EBADF`, so no leaked metadata, cgroup, namespace or privileged
/// descriptor survives. Observed by number, without `/proc`.
fn fds_sealed() -> bool {
    use std::os::fd::FromRawFd;
    let mut limit = [0u64; 2];
    if call(SYS_GETRLIMIT, RLIMIT_NOFILE, limit.as_mut_ptr() as i64, 0).is_none() {
        return false;
    }
    let bound = limit[0].min(1 << 16) as i64;
    let closed = |fd: i64| {
        call(SYS_FCNTL, fd, F_GETFD, 0).is_none()
            && std::io::Error::last_os_error().raw_os_error() == Some(EBADF)
    };
    let stdio_ok = (0..3).all(|fd| {
        // SAFETY: descriptors 0..=2 are open; the temporary `File` is never
        // dropped, so it cannot close them.
        let file = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
        file.metadata().is_ok_and(|metadata| {
            let kind = metadata.file_type();
            use std::os::unix::fs::FileTypeExt;
            kind.is_fifo() || kind.is_char_device()
        })
    });
    stdio_ok && (3..bound).all(closed)
}

/// The filtered syscall returns EPERM although unfiltered it would succeed or
/// fail differently for these arguments.
fn filtered(number: i64, a: i64, b: i64, c: i64) -> bool {
    // SAFETY: every probed call takes only integer arguments that the kernel
    // validates; a rejected call has no side effect, and the unfiltered
    // variants used here (`unshare(0)`, `process_vm_readv` with zero vectors)
    // are harmless.
    let result = unsafe { syscall(number, a, b, c, 0i64, 0i64, 0i64) };
    result == -1 && std::io::Error::last_os_error().raw_os_error() == Some(EPERM)
}

fn cgroup_lines(probe: &Path) -> Vec<PathBuf> {
    std::fs::read_to_string(probe.join("cgroups.txt"))
        .unwrap()
        .lines()
        .map(PathBuf::from)
        .collect()
}

/// Migration into another cgroup needs write access to its `cgroup.procs`.
fn cgroup_migration_denied(paths: &[PathBuf]) -> bool {
    paths.iter().all(|cgroup| {
        std::fs::OpenOptions::new()
            .write(true)
            .open(cgroup.join("cgroup.procs"))
            .is_err_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
    })
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
        id_quad(SYS_GETRESUID, SYS_SETFSUID).is_some_and(|ids| ids.iter().all(|id| *id != 0)),
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
        "metadata_replace_denied",
        std::fs::rename(worktree.join("tracked.txt"), common.join("config")).is_err(),
    );
    record(
        "proc_root_metadata_denied",
        std::fs::OpenOptions::new()
            .append(true)
            .open(format!(
                "/proc/self/root{}",
                common.join("config").display()
            ))
            .is_err(),
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

    // Exact identity and sealed descriptors.
    println!(
        "IDENTITY {}",
        identity_receipt(WORKER_RANGE, WORKER_GID, "0")
    );
    record("fds_sealed", fds_sealed());
    // Namespace, handle and descriptor-stealing syscalls are filtered.
    record("unshare_filtered", filtered(SYS_UNSHARE, 0, 0, 0));
    record("setns_filtered", filtered(SYS_SETNS, -1, 0, 0));
    record("pidfd_getfd_filtered", filtered(SYS_PIDFD_GETFD, -1, -1, 0));
    record(
        "process_vm_filtered",
        filtered(SYS_PROCESS_VM_READV, 0, 0, 0),
    );
    // cgroup escape: root, sibling and own cgroup cannot be written.
    record(
        "cgroup_migration_denied",
        cgroup_migration_denied(&cgroup_lines(&worktree.join(PROBE_DIR))),
    );
    // A worker-owned embedded repository whose config names a clean filter. It
    // must never run as the controller (the controller refuses to stage it; the
    // distinct owner would also stop Git from entering it).
    //
    // Phases keep worker preparation apart from controller execution:
    //  1. PREPARE: initialize and commit the repository BEFORE any filter exists,
    //     so preparing it cannot run (and cannot create) the marker.
    //  2. ARM: install the malicious filter only afterwards.
    //  3. CONTROL: modify a tracked file and `git add` it as the worker; that
    //     attempted unsafe Git path really runs the filter, proving the marker
    //     mechanism is live in this boundary. The marker is then removed, so a
    //     marker seen later can only come from a later execution.
    let evil = worktree.join("evil");
    let run_git = |args: &[&str]| {
        std::process::Command::new("/usr/bin/git")
            .env_clear()
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("HOME", "/nonexistent")
            .args(["-C", evil.to_str().unwrap()])
            .args(args)
            .output()
            .is_ok_and(|output| output.status.success())
    };
    let marker = worktree.join(FILTER_MARKER);
    let prepared = std::fs::create_dir(&evil).is_ok()
        && run_git(&["init", "-q", "-b", "main"])
        && std::fs::write(evil.join("f"), "data").is_ok()
        && run_git(&["add", "-A"])
        && run_git(&[
            "-c",
            "user.name=w",
            "-c",
            "user.email=w@w.invalid",
            "commit",
            "-q",
            "-m",
            "w",
        ]);
    record("embedded_repo_prepared", prepared);
    record("filter_absent_after_preparation", !marker.exists());
    let filter = format!("sh -c 'touch {}; cat'", marker.display());
    let armed = std::fs::write(evil.join(".gitattributes"), "* filter=x\n").is_ok()
        && run_git(&["config", "filter.x.clean", &filter]);
    record("embedded_filter_armed", armed);
    record("filter_absent_after_arming", !marker.exists());
    let touched = std::fs::write(evil.join("f"), "changed").is_ok() && run_git(&["add", "f"]);
    record("filter_control_fires", touched && marker.exists());
    record(
        "marker_reset_for_controller_phase",
        std::fs::remove_file(&marker).is_ok() && !marker.exists(),
    );
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
    let sentinel = worktree.join("../sentinel");
    let outside_by_proc = PathBuf::from(format!(
        "/proc/self/root{}",
        std::fs::canonicalize(&sentinel)
            .unwrap_or_else(|_| sentinel.clone())
            .display()
    ));
    let probe = worktree.join(PROBE_DIR);
    // Migration targets are the delegated root and the sibling only (the
    // controller's own per-call cgroup name is unknown to the probe).
    let migration_targets: Vec<PathBuf> = cgroup_lines(&probe).into_iter().take(2).collect();
    println!(
        "CONTROLLER private_read={} sentinel_denied={} sentinel_visible={} proc_root_outside_denied={}",
        read(&worktree.join("private.txt")),
        !read(&sentinel),
        sentinel.exists(),
        !read(&outside_by_proc),
    );
    println!(
        "CONTROLLER_IDENTITY {}",
        identity_receipt(CONTROLLER_ID..=CONTROLLER_ID, CONTROLLER_ID, "4")
    );
    println!(
        "CONTROLLER_ESCAPE fds_sealed={} open_by_handle_filtered={} setns_filtered={} unshare_filtered={} pidfd_getfd_filtered={} cgroup_migration_denied={}",
        fds_sealed(),
        filtered(SYS_OPEN_BY_HANDLE_AT, -1, 0, 0),
        filtered(SYS_SETNS, -1, 0, 0),
        filtered(SYS_UNSHARE, 0, 0, 0),
        filtered(SYS_PIDFD_GETFD, -1, -1, 0),
        cgroup_migration_denied(&migration_targets),
    );
}

fn assert_receipts(stdout: &str, expected: &[&str]) {
    for check in expected {
        assert!(stdout.contains(check), "missing {check} in:\n{stdout}");
    }
}

/// Exact custody after admission: owners, modes, ancestors and payload.
fn assert_custody(common: &Path, worktree: &Path, worker_uid: u32) {
    let mut pending = vec![common.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(
            (metadata.uid(), metadata.gid()),
            (CONTROLLER_ID, CONTROLLER_ID),
            "{}",
            path.display()
        );
        assert_eq!(metadata.mode() & 0o022, 0, "{}", path.display());
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).unwrap() {
                pending.push(entry.unwrap().path());
            }
        }
    }
    let root = std::fs::symlink_metadata(worktree).unwrap();
    assert_eq!(
        (root.uid(), root.gid(), root.mode() & 0o7777),
        (CONTROLLER_ID, WORKER_GID, 0o1770)
    );
    let pointer = std::fs::symlink_metadata(worktree.join(".git")).unwrap();
    assert_eq!((pointer.uid(), pointer.mode() & 0o022), (CONTROLLER_ID, 0));
    let payload = std::fs::symlink_metadata(worktree.join("tracked.txt")).unwrap();
    assert_eq!((payload.uid(), payload.gid()), (worker_uid, WORKER_GID));
    crate::protected::verify_custody(common, worktree, CONTROLLER_ID, WORKER_GID)
        .expect("ancestors and custody verify");
}

#[test]
#[ignore = "privileged hosted diagnostic"]
fn hosted_protected_worker_controller_and_cleanup() {
    let (dir, repo, handle) = fixture();
    let launcher = ProtectedLauncher::new(config()).expect("hosted admission must succeed");
    let admission = launcher.admit(&handle, &repo).expect("custody");
    let config_before = std::fs::read(repo.join(".git/config")).unwrap();
    let worktree = handle.worktree_path().to_path_buf();
    let common = std::fs::canonicalize(repo.join(".git")).unwrap();
    assert_custody(&common, &worktree, admission.worker_uid_for_test());

    let probe = worktree.join(PROBE_DIR);
    std::fs::create_dir(&probe).unwrap();
    let executable = probe.join("probe");
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    // Cgroup paths the probes must be unable to migrate into: the delegated
    // root, a real sibling, and the worker's own cgroup.
    let sibling = hosted_cgroup().join("receipts-sibling");
    std::fs::create_dir(&sibling).unwrap();
    let worker_cgroup = hosted_cgroup().join(format!("receipts-{}-worker", admission.attempt_id()));
    let lines = [hosted_cgroup(), sibling.clone(), worker_cgroup];
    std::fs::write(
        probe.join("cgroups.txt"),
        lines
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();

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
    assert_receipts(
        &stdout,
        &[
            "uid_nonroot=true",
            "config_write_denied=true",
            "pointer_unlink_denied=true",
            "pointer_rename_denied=true",
            "hooks_create_denied=true",
            "metadata_replace_denied=true",
            "proc_root_metadata_denied=true",
            "payload_edit=true",
            "private_payload=true",
            "escapee_spawned=true",
            "fds_sealed=true",
            "unshare_filtered=true",
            "setns_filtered=true",
            "pidfd_getfd_filtered=true",
            "process_vm_filtered=true",
            "cgroup_migration_denied=true",
            "embedded_repo_prepared=true",
            "filter_absent_after_preparation=true",
            "embedded_filter_armed=true",
            "filter_absent_after_arming=true",
            "filter_control_fires=true",
            "marker_reset_for_controller_phase=true",
            "uid_exact=true gid_exact=true no_groups=true caps_exact=true nnp=true seccomp_filter=true",
        ],
    );
    // Baseline before ANY controller operation: the worker phase ended with the
    // marker reset, so a marker later is attributable to controller execution.
    assert!(
        !worktree.join(FILTER_MARKER).exists(),
        "filter marker present at the controller baseline"
    );
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
        CONTROLLER_ID
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
    let send = |channel: &crate::ControllerChannel, sequence, operation| {
        let credentials = channel.credentials();
        channel.handle(ControllerRequest {
            attempt_id: credentials.attempt_id,
            task_id: credentials.task_id,
            token: credentials.token,
            sequence,
            operation,
        })
    };
    assert_eq!(credentials.task_id, "task-hosted");

    // Reviewer: query-only on the enforced channel, mutations refused.
    let reviewer = protected.reviewer_channel();
    assert!(send(reviewer, 0, ControllerOperation::Status).is_ok());
    // A role refusal does not consume the sequence, so both use number 1.
    for operation in [
        ControllerOperation::Stage {
            paths: vec!["tracked.txt".into()],
        },
        ControllerOperation::Commit {
            message: "reviewer".into(),
        },
    ] {
        assert!(matches!(
            send(reviewer, 1, operation),
            Err(ProtectedLaunchError::Binding(
                "reviewer channel is query-only"
            ))
        ));
    }

    // The worker-planted embedded repository is never staged, so its filter
    // can never run as the controller.
    assert!(matches!(
        send(
            worker,
            0,
            ControllerOperation::Stage {
                paths: vec!["evil".into()]
            }
        ),
        Err(ProtectedLaunchError::Binding(
            "embedded repositories cannot be staged"
        ))
    ));
    send(
        worker,
        1,
        ControllerOperation::Stage {
            paths: vec!["tracked.txt".into(), "private.txt".into()],
        },
    )
    .expect("controller stages worker payload");
    let ControllerResponse::Committed { commit } = send(
        worker,
        2,
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
    let ControllerResponse::Checkpointed {
        handle: checkpointed,
        checkpoint,
    } = send(
        worker,
        3,
        ControllerOperation::Checkpoint {
            checkpoint_id: "hosted".into(),
            kind: crate::WorkspaceCheckpointKind::Progress,
            captured_at: crate::WorkspaceDateTimeV1::try_new("2026-10-30T00:00:00Z").unwrap(),
        },
    )
    .expect("checkpoint")
    else {
        panic!("checkpoint")
    };
    assert_eq!(checkpoint.core().head_sha(), &commit);
    assert_eq!(checkpointed.state(), WorkspaceState::Checkpointed);
    assert_eq!(checkpointed.head_sha(), Some(&commit));
    assert!(
        !worktree.join(FILTER_MARKER).exists(),
        "a worker-supplied filter ran as the controller (marker absent at the baseline)"
    );

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
    assert_receipts(
        &stdout,
        &[
            "private_read=true",
            "sentinel_denied=true",
            "sentinel_visible=true",
            "proc_root_outside_denied=true",
            "uid_exact=true gid_exact=true no_groups=true caps_exact=true nnp=true seccomp_filter=true",
            "fds_sealed=true",
            "open_by_handle_filtered=true",
            "setns_filtered=true",
            "unshare_filtered=true",
            "pidfd_getfd_filtered=true",
            "cgroup_migration_denied=true",
        ],
    );

    // Custody returns only after proven cleanup; channels then refuse.
    assert!(common.join(CUSTODY_MARKER).exists(), "reservation record");
    protected.release_custody().expect("custody release");
    assert!(
        !common.join(CUSTODY_MARKER).exists(),
        "complete release deletes the reservation record"
    );
    let restored = std::fs::symlink_metadata(&common).unwrap();
    assert_eq!(
        restored.uid(),
        0,
        "metadata returns to the root-owned fixture"
    );
    assert!(matches!(
        send(worker, 4, ControllerOperation::Status),
        Err(ProtectedLaunchError::Binding("custody was released"))
    ));

    // Non-force teardown: dirty payload (probe files, the embedded repository)
    // refuses and survives; once removed, teardown succeeds and the task
    // branch keeps its commit.
    let teardown = WorkspaceTeardownRequest::new(&repo);
    assert!(matches!(
        teardown.teardown(&checkpointed),
        Err(WorkspaceError::TeardownWorktreeDirty { .. })
    ));
    assert!(worktree.join("evil").exists() && probe.exists());
    std::fs::remove_dir_all(&probe).unwrap();
    std::fs::remove_dir_all(worktree.join("evil")).unwrap();
    std::fs::remove_file(worktree.join("escapee.pid")).unwrap();
    let torn = teardown
        .teardown(&checkpointed)
        .expect("non-force teardown");
    assert_eq!(torn.state(), WorkspaceState::TornDown);
    assert!(!worktree.exists());
    let tip =
        crate::test_support::stdout_trimmed(&git(&repo, &["rev-parse", "refs/heads/task/hosted"]));
    assert_eq!(tip, commit.as_str());

    std::fs::remove_dir(&sibling).unwrap();
    let leftovers: Vec<_> = std::fs::read_dir(hosted_cgroup())
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("receipts-"))
        .collect();
    assert!(leftovers.is_empty(), "owned cgroups remain: {leftovers:?}");
}

fn owner_mode(path: &Path) -> (u32, u32, u32) {
    let metadata = std::fs::symlink_metadata(path).unwrap();
    (metadata.uid(), metadata.gid(), metadata.mode() & 0o7777)
}

/// F2/F4 in the real boundary (root, real chown): one custody owner per Git
/// common directory across launcher instances, linked worktrees and aliases,
/// refusals that change nothing (including external symlink and hardlink
/// targets), and safe re-admission after a complete release.
#[test]
#[ignore = "privileged hosted diagnostic"]
fn hosted_exclusive_custody_and_linked_target_refusals() {
    let (dir, repo, first_handle) = fixture();
    let base = crate::test_support::stdout_trimmed(&git(&repo, &["rev-parse", "HEAD"]));
    let second_handle = WorkspaceProvisionRequest::new(
        &repo,
        "ws-second",
        Some("task-second"),
        "task/second",
        dir.path().join("wt2"),
        &base,
    )
    .unwrap()
    .provision()
    .unwrap();
    let launcher_a = ProtectedLauncher::new(config()).expect("hosted admission A");
    let launcher_b = ProtectedLauncher::new(config()).expect("hosted admission B");
    let common = std::fs::canonicalize(repo.join(".git")).unwrap();
    let first_root = first_handle.worktree_path().to_path_buf();
    let second_root = std::fs::canonicalize(second_handle.worktree_path()).unwrap();
    let marker = common.join(CUSTODY_MARKER);
    let second_before = owner_mode(&second_root);

    let admission = launcher_a
        .admit(&first_handle, &repo)
        .expect("first custody");
    let record = std::fs::read(&marker).unwrap();
    let common_after_first = owner_mode(&common);
    assert_eq!(common_after_first.0, CONTROLLER_ID);
    // Same handle, another linked worktree, another launcher instance and the
    // worktree spelled as the repository root all meet the same owner.
    for (launcher, handle, root) in [
        (&launcher_a, &first_handle, repo.clone()),
        (&launcher_b, &first_handle, repo.clone()),
        (&launcher_a, &second_handle, repo.clone()),
        (&launcher_b, &second_handle, repo.clone()),
        (&launcher_b, &second_handle, first_root.clone()),
    ] {
        match launcher.admit(handle, &root) {
            Err(ProtectedLaunchError::Custody(detail)) => {
                assert!(detail.contains(admission.attempt_id()), "{detail}");
            }
            other => panic!("second owner must refuse: {other:?}"),
        }
    }
    assert_eq!(std::fs::read(&marker).unwrap(), record);
    assert_eq!(owner_mode(&common), common_after_first);
    assert_eq!(
        owner_mode(&second_root),
        second_before,
        "loser changed ownership"
    );

    // Complete lifecycle, then safe re-admission by the other launcher.
    let request = ProcessRunRequest::new(
        "/usr/bin/true",
        Vec::<String>::new(),
        &first_root,
        &first_root,
    )
    .unwrap();
    let protected = launcher_a
        .launch(admission, &request, &policy())
        .expect("launch");
    protected.attempt().wait_collect().expect("collected");
    protected.release_custody().expect("complete release");
    assert!(!marker.exists(), "record removed by the complete release");
    assert_eq!(
        owner_mode(&common).0,
        0,
        "metadata back to its original owner"
    );
    drop(protected);
    let second = launcher_b
        .admit(&second_handle, &repo)
        .expect("re-admission after a complete release");
    assert!(marker.exists());
    drop(second); // unused admission: custody is returned
    assert!(!marker.exists());
    assert_eq!(owner_mode(&second_root), second_before);

    // External symlink pointer: Git's own prechecks accept it; custody refuses
    // before it changes the external target, the common directory or the
    // worktree.
    let pointer = second_root.join(".git");
    let pointer_text = std::fs::read(&pointer).unwrap();
    let external = dir.path().join("external-pointer");
    std::fs::write(&external, &pointer_text).unwrap();
    std::fs::set_permissions(&external, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::remove_file(&pointer).unwrap();
    std::os::unix::fs::symlink(&external, &pointer).unwrap();
    let state = || {
        (
            owner_mode(&common),
            owner_mode(&second_root),
            owner_mode(&external),
        )
    };
    let before = state();
    match launcher_b.admit(&second_handle, &repo) {
        Err(ProtectedLaunchError::Custody(detail)) => {
            assert!(detail.contains(".git pointer is a symlink"), "{detail}");
        }
        other => panic!("symlink pointer must refuse: {other:?}"),
    }
    assert_eq!(state(), before, "external pointer target or roots changed");
    assert!(!marker.exists());
    std::fs::remove_file(&pointer).unwrap();
    std::fs::write(&pointer, &pointer_text).unwrap();

    // Hardlink alias in the payload: the shared inode keeps its external owner
    // and mode.
    let shared = dir.path().join("external-alias");
    std::fs::write(&shared, "shared").unwrap();
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::hard_link(&shared, second_root.join("alias")).unwrap();
    let before = (
        owner_mode(&common),
        owner_mode(&second_root),
        owner_mode(&shared),
    );
    match launcher_b.admit(&second_handle, &repo) {
        Err(ProtectedLaunchError::Custody(detail)) => {
            assert!(detail.contains("hard links"), "{detail}");
        }
        other => panic!("hardlink alias must refuse: {other:?}"),
    }
    assert_eq!(
        (
            owner_mode(&common),
            owner_mode(&second_root),
            owner_mode(&shared)
        ),
        before
    );
    assert!(!marker.exists());
}
