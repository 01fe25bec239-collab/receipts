//! Tests for the bounded timeout/terminate/grace/kill lifecycle.
//!
//! Every behavioral proof runs a real child through the public bounded
//! runner. Controlled long-running children are this very test binary
//! acting as ignored probe helpers, coordinated through local filesystem
//! markers (a pid file written before a ready file) — no shells, no output
//! capture, no signaling of arbitrary pids. Liveness/reaping evidence uses
//! the zero-signal `kill(pid, 0)` probe against pids the runner-owned
//! child itself recorded, never against unrelated processes.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::runner::{
    TimeoutLifecycleEvent, checked_deadline, force_kill_failed_with_cleanup,
    grace_wait_failed_with_cleanup, inject_unsupported_timeout_platform,
    take_timeout_lifecycle_events,
};
use crate::execution::unix_signal::{
    GroupPresence, GroupSignalDelivery, LeaderState, OwnedProcessGroup, TimeoutSignalClass,
    caller_process_group, classify_group_kill_result, classify_group_presence,
    deliver_group_sigkill, observe_leader_without_reaping, process_alive, recorded_group_is_empty,
    timeout_platform_supported, timeout_signal_numbers_for,
};
use crate::execution::unix_signal::{
    GroupQuiescence, PidGroupObservation, classify_pid_group_observation, group_quiescence,
    inaccessible_pid_quiescence,
};
use crate::execution::{
    ExecutionError, ProcessRunOutcome, ProcessRunRequest, ProcessTermination, ProcessTimeoutPolicy,
    run, run_with_timeout, run_with_timeout_and_capture,
};

/// How long every controlled probe child would keep running if the
/// timeout machinery failed to act. Assertions compare elapsed wall time
/// against generous bounds far below this value.
const PROBE_SLEEP: Duration = Duration::from_secs(60);

/// Generous upper bound for any bounded run in this suite: far above what
/// deadline + grace + scheduling overhead could ever need on a loaded CI
/// host, yet far below [`PROBE_SLEEP`], so an unbounded leak could never
/// pass.
const BOUNDED_RUN_UPPER_BOUND: Duration = Duration::from_secs(20);

/// Independent safety ceiling for the deliberately hostile churn helper.
/// The substantive test needs less than two seconds through quiescence;
/// five seconds preserves that pressure while bounding a broken runner.
const CHURN_SAFETY_WINDOW: Duration = Duration::from_secs(5);
const CHURN_SETTLE_WINDOW: Duration = Duration::from_secs(1);
const CHURN_START_TOKEN: &[u8] = b"receipts-churn-start-v1";

/// A temporary directory removed on drop.
struct TempDir {
    root: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "receipts-timeout-test-{tag}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temporary test directory creation");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn first_existing(candidates: &[&str]) -> PathBuf {
    candidates
        .iter()
        .map(Path::new)
        .find(|candidate| candidate.is_file())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| panic!("expected one of {candidates:?} to exist on this platform"))
}

fn bounded_request(
    executable: impl Into<PathBuf>,
    arguments: &[&str],
    workspace_root: &Path,
    cwd: &Path,
) -> ProcessRunRequest {
    ProcessRunRequest::new(executable, arguments.iter().copied(), workspace_root, cwd)
        .expect("structural request construction")
}

fn policy(run_timeout: Duration, termination_grace: Duration) -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(run_timeout, termination_grace)
        .expect("valid timeout policy for test")
}

/// Runs one of this binary's ignored probe helpers through the public
/// bounded runner with the given policy.
fn run_probe_bounded(
    probe_name: &str,
    workspace_root: &Path,
    run_timeout: Duration,
    termination_grace: Duration,
) -> (ProcessRunOutcome, Duration) {
    let started = Instant::now();
    let outcome = run_with_timeout(
        &bounded_request(
            std::env::current_exe().expect("current test executable path"),
            &[probe_name, "--ignored"],
            workspace_root,
            workspace_root,
        ),
        &policy(run_timeout, termination_grace),
    )
    .expect("bounded run should produce typed metadata, not an error");
    (outcome, started.elapsed())
}

/// Waits until the probe child's marker file exists, failing closed if the
/// child never became ready.
fn await_marker(workspace_root: &Path, marker: &str) {
    let marker_path = workspace_root.join(marker);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !marker_path.exists() {
        assert!(
            Instant::now() < deadline,
            "probe child never wrote its {marker} marker; host is stalled beyond all test budgets"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Reads one recorded marker value (a pid or pgid written by a
/// runner-owned probe child).
fn recorded_value(workspace_root: &Path, marker: &str) -> u32 {
    await_marker(workspace_root, marker);
    fs::read_to_string(workspace_root.join(marker))
        .expect("marker readable")
        .trim()
        .parse()
        .expect("recorded marker value parses")
}

/// Reads the runner-owned probe child's recorded pid.
fn recorded_pid(workspace_root: &Path) -> u32 {
    recorded_value(workspace_root, "pid")
}

/// Spawns one of this binary's ignored probes as a direct no-shell child
/// of the current process and waits until its ready marker exists. The
/// spawned process inherits the caller's process group — exactly how
/// descendants join an attempt-owned group in production. The handle is
/// deliberately never reaped: the descendant must outlive this helper, and
/// the probe parent itself dies before the descendant in every scenario
/// this suite exercises, so the kernel reparents and reaps it.
#[allow(clippy::zombie_processes)]
fn spawn_and_await_descendant_probe(probe_name: &str) {
    let executable = std::env::current_exe().expect("current test executable path");
    let descendant = std::process::Command::new(executable)
        .args([probe_name, "--ignored"])
        .spawn()
        .expect("descendant probe spawn");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !Path::new("descendant-ready").exists() {
        assert!(
            Instant::now() < deadline,
            "descendant probe {probe_name} never became ready"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // Keep the handle alive (without reaping) until this function ends;
    // dropping it does not terminate the descendant.
    let _ = descendant.id();
}

/// Polls until the zero-signal probe reports the recorded pid gone,
/// failing closed if it survives every test budget. Absorbs the short
/// reparenting/reaping window between a process's death and its final
/// disappearance from the process table.
fn await_process_death(pid: u32, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while process_alive(pid) {
        assert!(
            Instant::now() < deadline,
            "{what} ({pid}) is still live or unreaped beyond every test budget"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Polls until the kernel reports the attempt-owned process group empty.
fn await_group_empty(pgid: u32, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if recorded_group_is_empty(pgid) == Some(true) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what}: owned process group -{pgid} still has members beyond every test budget"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// --- Controlled child helpers ------------------------------------------
//
// Never executed by the parent suite; the bounded runner re-invokes this
// same test binary filtered onto exactly one probe. Each probe records its
// pid before publishing the ready marker so the parent can afterwards
// prove what happened to the exact runner-owned process. All three are
// exercised by substantive parent tests below.

/// Default SIGTERM disposition: exits when gracefully terminated.
#[test]
#[ignore]
fn execution_timeout_probe_child_ready_then_sleep_long() {
    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("pid"), std::process::id().to_string()).expect("pid marker");
    fs::write(cwd.join("ready"), b"ready").expect("ready marker");
    std::thread::sleep(PROBE_SLEEP);
}

/// Ignores SIGTERM deliberately, so only the forced-kill step can end it.
#[test]
#[ignore]
fn execution_timeout_probe_child_ignores_sigterm_then_sleep_long() {
    #[cfg(unix)]
    unsafe {
        // Test-side only: install SIG_IGN for SIGTERM via the historic
        // signal(2) entry point. No shell, no dependency; the constant
        // SIG_IGN is fixed by POSIX.
        unsafe extern "C" {
            fn signal(signum: std::os::raw::c_int, handler: usize) -> usize;
        }
        const SIGTERM: std::os::raw::c_int = 15;
        const SIG_IGN: usize = 1;
        signal(SIGTERM, SIG_IGN);
    }

    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("pid"), std::process::id().to_string()).expect("pid marker");
    fs::write(cwd.join("ready"), b"ready").expect("ready marker");
    std::thread::sleep(PROBE_SLEEP);
}

/// Verifies the timed path leaks no ambient environment: any inherited
/// variable makes it exit 13 before its deadline instead of surviving to
/// graceful termination.
#[test]
#[ignore]
fn execution_timeout_probe_child_assert_empty_env_then_ready_and_sleep_long() {
    if std::env::vars_os().next().is_some() {
        std::process::exit(13);
    }
    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("pid"), std::process::id().to_string()).expect("pid marker");
    fs::write(cwd.join("ready"), b"ready").expect("ready marker");
    std::thread::sleep(PROBE_SLEEP);
}

// --- Process-group ownership probes (repair regression) ------------------
//
// The following five ignored probes exist to prove the no-orphan
// invariant: the timed child leads a dedicated process group, descendants
// it creates inherit that group, and timeout termination reaches every
// member. Coordination uses only pid/pgid/ready marker files under the
// attempt's working directory — no sleeps, no shells.

/// Records its pid, its own process group, and readiness before sleeping;
/// lets the parent prove the timed child leads a dedicated group.
#[test]
#[ignore]
fn execution_timeout_probe_child_records_own_process_group_then_sleep_long() {
    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("pid"), std::process::id().to_string()).expect("pid marker");
    fs::write(cwd.join("pgid"), caller_process_group().to_string()).expect("pgid marker");
    fs::write(cwd.join("ready"), b"ready").expect("ready marker");
    std::thread::sleep(PROBE_SLEEP);
}

/// Long-lived descendant with default SIGTERM disposition; records its own
/// pid before publishing its ready marker.
#[test]
#[ignore]
fn execution_timeout_probe_descendant_ready_then_sleep_long() {
    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("descendant-pid"), std::process::id().to_string())
        .expect("descendant pid marker");
    fs::write(cwd.join("descendant-ready"), b"ready").expect("descendant ready marker");
    std::thread::sleep(PROBE_SLEEP);
}

/// Long-lived descendant that ignores SIGTERM deliberately, so only a
/// forced process-group SIGKILL can end it.
#[test]
#[ignore]
fn execution_timeout_probe_descendant_ignores_sigterm_then_sleep_long() {
    #[cfg(unix)]
    unsafe {
        // Test-side only: install SIG_IGN for SIGTERM via the historic
        // signal(2) entry point. No shell, no dependency; the constant
        // SIG_IGN is fixed by POSIX.
        unsafe extern "C" {
            fn signal(signum: std::os::raw::c_int, handler: usize) -> usize;
        }
        const SIGTERM: std::os::raw::c_int = 15;
        const SIG_IGN: usize = 1;
        signal(SIGTERM, SIG_IGN);
    }

    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("descendant-pid"), std::process::id().to_string())
        .expect("descendant pid marker");
    fs::write(cwd.join("descendant-ready"), b"ready").expect("descendant ready marker");
    std::thread::sleep(PROBE_SLEEP);
}

/// Timed parent that spawns one graceful descendant before becoming
/// ready: both belong to the attempt-owned process group.
#[test]
#[ignore]
fn execution_timeout_probe_parent_of_graceful_descendant_then_sleep_long() {
    spawn_and_await_descendant_probe("execution_timeout_probe_descendant_ready_then_sleep_long");
    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("attempt-pid"), std::process::id().to_string()).expect("attempt pid marker");
    fs::write(cwd.join("attempt-pgid"), caller_process_group().to_string())
        .expect("attempt pgid marker");
    fs::write(cwd.join("ready"), b"ready").expect("ready marker");
    std::thread::sleep(PROBE_SLEEP);
}

/// Timed parent that spawns one SIGTERM-ignoring descendant before
/// becoming ready: the direct parent dies from SIGTERM while the
/// descendant survives into the force-kill step.
#[test]
#[ignore]
fn execution_timeout_probe_parent_of_sigterm_ignoring_descendant_then_sleep_long() {
    spawn_and_await_descendant_probe(
        "execution_timeout_probe_descendant_ignores_sigterm_then_sleep_long",
    );
    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("attempt-pid"), std::process::id().to_string()).expect("attempt pid marker");
    fs::write(cwd.join("attempt-pgid"), caller_process_group().to_string())
        .expect("attempt pgid marker");
    fs::write(cwd.join("ready"), b"ready").expect("ready marker");
    std::thread::sleep(PROBE_SLEEP);
}

/// Short-lived churn leaf. Its SIGTERM ignore disposition is inherited from
/// the churn parent across exec, so it can survive into the quiescence race.
#[test]
#[ignore]
fn execution_timeout_probe_churn_leaf() {
    if std::env::var_os("RECEIPTS_RECORD_CHURN_LEAF").is_some() {
        let cwd = std::env::current_dir().expect("churn leaf working directory");
        fs::write(cwd.join("churn-leaf-pid"), std::process::id().to_string())
            .expect("churn leaf pid marker");
        fs::write(
            cwd.join("churn-leaf-pgid"),
            caller_process_group().to_string(),
        )
        .expect("churn leaf pgid marker");
        if std::env::var_os("RECEIPTS_SUPPRESS_CHURN_LEAF_READY").is_none() {
            fs::write(cwd.join("churn-leaf-ready"), b"ready").expect("churn leaf ready marker");
        }
        if std::env::var_os("RECEIPTS_HOLD_RECORDED_CHURN_LEAF").is_some() {
            std::thread::sleep(PROBE_SLEEP);
            return;
        }
    }
    std::thread::sleep(Duration::from_millis(40));
}

/// Repeatedly replaces descendants across the grace boundary. A fork can be
/// in flight when the runner delivers `SIGSTOP`; the production fixed-point
/// barrier must still stop and discover the resulting group member.
#[test]
#[ignore]
fn execution_timeout_probe_descendant_churns_after_sigterm() {
    #[cfg(unix)]
    unsafe {
        unsafe extern "C" {
            fn signal(signum: std::os::raw::c_int, handler: usize) -> usize;
        }
        const SIGTERM: std::os::raw::c_int = 15;
        const SIG_IGN: usize = 1;
        signal(SIGTERM, SIG_IGN);
    }

    let cwd = std::env::current_dir().expect("probe working directory");
    if std::env::var_os("RECEIPTS_CHURN_START_BARRIER").is_some() {
        fs::write(cwd.join("churn-helper-ready"), b"ready").expect("churn helper barrier marker");
        let deadline = Instant::now() + Duration::from_secs(30);
        while !churn_start_released(&cwd.join("churn-helper-start")) {
            if Instant::now() >= deadline {
                std::process::exit(24);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fs::write(cwd.join("churn-pid"), std::process::id().to_string()).expect("churn pid marker");
    fs::write(cwd.join("churn-pgid"), caller_process_group().to_string())
        .expect("churn pgid marker");
    fs::write(cwd.join("descendant-ready"), b"ready").expect("churn ready marker");
    let executable = std::env::current_exe().expect("current test executable path");
    let mut children = Vec::new();
    let deadline = Instant::now() + CHURN_SAFETY_WINDOW;
    while Instant::now() < deadline {
        children.retain_mut(|child: &mut std::process::Child| {
            child.try_wait().expect("churn child observation").is_none()
        });
        let mut command = std::process::Command::new(&executable);
        command
            .args(["execution_timeout_probe_churn_leaf", "--ignored"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if children.is_empty() && !cwd.join("churn-leaf-ready").exists() {
            command.env("RECEIPTS_RECORD_CHURN_LEAF", "1");
        }
        children.push(command.spawn().expect("churn leaf spawn"));
        if std::env::var_os("RECEIPTS_FAIL_CHURN_HELPER_AFTER_LEAF").is_some() {
            await_marker(&cwd, "churn-leaf-ready");
            std::process::exit(23);
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    let settle_deadline = Instant::now() + CHURN_SETTLE_WINDOW;
    while !children.is_empty() && Instant::now() < settle_deadline {
        children.retain_mut(|child| child.try_wait().expect("churn child reap").is_none());
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        children.is_empty(),
        "churn leaves did not settle within the safety window"
    );
}

#[test]
#[ignore]
fn execution_timeout_probe_parent_of_churning_descendant() {
    spawn_and_await_descendant_probe("execution_timeout_probe_descendant_churns_after_sigterm");
    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("attempt-pid"), std::process::id().to_string()).expect("attempt pid marker");
    fs::write(cwd.join("attempt-pgid"), caller_process_group().to_string())
        .expect("attempt pgid marker");
    fs::write(cwd.join("ready"), b"ready").expect("ready marker");
    std::thread::sleep(PROBE_SLEEP);
}

// --- Policy validation (T10 / T11) ---------------------------------------

#[test]
fn unsupported_timeout_platform_refuses_uncaptured_spawn() {
    let workspace = TempDir::new("unsupported-plain");
    let request = bounded_request(
        std::env::current_exe().expect("current test executable path"),
        &[
            "execution_timeout_probe_child_ready_then_sleep_long",
            "--ignored",
        ],
        workspace.path(),
        workspace.path(),
    );
    let _guard = inject_unsupported_timeout_platform();
    let error = run_with_timeout(
        &request,
        &policy(Duration::from_secs(30), Duration::from_secs(1)),
    )
    .expect_err("unsupported timeout capability must fail before spawn");

    assert!(matches!(error, ExecutionError::UnsupportedTimeoutPlatform));
    assert!(!workspace.path().join("ready").exists());
    assert!(!workspace.path().join("pid").exists());
}

#[test]
fn unsupported_timeout_platform_refuses_captured_spawn() {
    let workspace = TempDir::new("unsupported-capture");
    let request = bounded_request(
        std::env::current_exe().expect("current test executable path"),
        &[
            "execution_timeout_probe_child_ready_then_sleep_long",
            "--ignored",
        ],
        workspace.path(),
        workspace.path(),
    );
    let _guard = inject_unsupported_timeout_platform();
    let error = run_with_timeout_and_capture(
        &request,
        &policy(Duration::from_secs(30), Duration::from_secs(1)),
    )
    .expect_err("unsupported capture capability must fail before spawn");

    assert!(matches!(error, ExecutionError::UnsupportedTimeoutPlatform));
    assert!(!workspace.path().join("ready").exists());
    assert!(!workspace.path().join("pid").exists());
}

#[test]
fn timeout_signal_mapping_capability_is_explicit_and_fail_closed() {
    let apple = timeout_signal_numbers_for(TimeoutSignalClass::Apple)
        .expect("supported Apple signal mapping");
    assert_eq!((apple.stop, apple.cont), (17, 19));

    let linux_android = timeout_signal_numbers_for(TimeoutSignalClass::LinuxAndroidCommon)
        .expect("supported Linux/Android signal mapping");
    assert_eq!((linux_android.stop, linux_android.cont), (19, 18));

    assert_eq!(
        timeout_signal_numbers_for(TimeoutSignalClass::Unsupported),
        None
    );
    assert!(timeout_platform_supported());
}

#[test]
fn inaccessible_proc_pid_uses_independent_group_membership_fail_closed() {
    let permission_denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
    let owned_pgid = 4242;

    let unrelated = classify_pid_group_observation(4343, None, owned_pgid);
    assert_eq!(unrelated, PidGroupObservation::Other);
    assert_eq!(
        inaccessible_pid_quiescence(7, &permission_denied, unrelated),
        None
    );

    let owned = classify_pid_group_observation(owned_pgid, None, owned_pgid);
    assert_eq!(owned, PidGroupObservation::Owned);
    assert!(matches!(
        inaccessible_pid_quiescence(8, &permission_denied, owned),
        Some(GroupQuiescence::Unknown { .. })
    ));

    let gone = classify_pid_group_observation(-1, Some(3), owned_pgid);
    assert_eq!(gone, PidGroupObservation::Gone);
    assert_eq!(
        inaccessible_pid_quiescence(9, &permission_denied, gone),
        None
    );

    let ambiguous = classify_pid_group_observation(-1, Some(1), owned_pgid);
    assert!(matches!(ambiguous, PidGroupObservation::Unknown { .. }));
    assert!(matches!(
        inaccessible_pid_quiescence(10, &permission_denied, ambiguous),
        Some(GroupQuiescence::Unknown { .. })
    ));
}

#[test]
fn to01_zero_run_timeout_is_refused_without_spawning() {
    let error = ProcessTimeoutPolicy::new(Duration::ZERO, Duration::from_secs(1))
        .expect_err("zero run timeout must be refused");
    assert!(matches!(error, ExecutionError::InvalidTimeoutPolicy { .. }));
    assert!(error.to_string().contains("run_timeout"));
}

#[test]
fn to02_zero_termination_grace_is_refused_without_spawning() {
    let error = ProcessTimeoutPolicy::new(Duration::from_secs(1), Duration::ZERO)
        .expect_err("zero termination grace must be refused");
    assert!(matches!(error, ExecutionError::InvalidTimeoutPolicy { .. }));
    assert!(error.to_string().contains("termination_grace"));

    // Valid policies expose their immutable intervals unchanged.
    let valid = policy(Duration::from_millis(250), Duration::from_millis(125));
    assert_eq!(valid.run_timeout(), Duration::from_millis(250));
    assert_eq!(valid.termination_grace(), Duration::from_millis(125));
}

// --- Ordinary completion under a generous policy (T1 / T2) ----------------

#[test]
fn to03_normal_success_before_deadline_is_ordinary_completion() {
    let workspace = TempDir::new("to03");
    let outcome = run_with_timeout(
        &bounded_request(
            first_existing(&["/usr/bin/true", "/bin/true"]),
            &[],
            workspace.path(),
            workspace.path(),
        ),
        &policy(Duration::from_secs(30), Duration::from_secs(1)),
    )
    .expect("fast successful child completes ordinarily");

    assert_eq!(outcome.termination(), ProcessTermination::Completed);
    assert!(!outcome.timed_out());
    assert!(!outcome.forced_kill_required());
    assert!(outcome.success());
    assert_eq!(outcome.exit_code(), Some(0));
}

#[test]
fn to04_nonzero_exit_before_deadline_stays_ordinary_not_timed_out() {
    let workspace = TempDir::new("to04");
    let outcome = run_with_timeout(
        &bounded_request(
            first_existing(&["/usr/bin/false", "/bin/false"]),
            &[],
            workspace.path(),
            workspace.path(),
        ),
        &policy(Duration::from_secs(30), Duration::from_secs(1)),
    )
    .expect("fast non-zero child is ordinary runner output");

    assert_eq!(outcome.termination(), ProcessTermination::Completed);
    assert!(!outcome.timed_out());
    assert!(!outcome.forced_kill_required());
    assert!(!outcome.success());
    assert_eq!(outcome.exit_code(), Some(1));
}

// --- Actual timeout: graceful termination (T3 / T4 / T8) ------------------

#[test]
fn to05_deadline_expiry_terminates_gracefully_without_force() {
    let workspace = TempDir::new("to05");
    let run_timeout = Duration::from_millis(600);
    let (outcome, elapsed) = run_probe_bounded(
        "execution_timeout_probe_child_ready_then_sleep_long",
        workspace.path(),
        run_timeout,
        Duration::from_millis(750),
    );

    // Typed timeout classification: never an ordinary completion, never a
    // forced kill, never reported successful.
    assert_eq!(
        outcome.termination(),
        ProcessTermination::TimedOutGracefullyTerminated
    );
    assert!(outcome.timed_out());
    assert!(!outcome.forced_kill_required());
    assert!(!outcome.success());
    // Signal-terminated children carry no numeric exit code, which keeps
    // them unmistakable from an ordinary pre-deadline completion.
    assert_eq!(outcome.exit_code(), None);

    // Monotonic deadline was honored and the total run stayed far below
    // the child's own long duration (T6 evidence for the graceful path).
    assert!(
        elapsed >= run_timeout,
        "runner returned before the configured deadline elapsed: {elapsed:?}"
    );
    assert!(
        elapsed < BOUNDED_RUN_UPPER_BOUND,
        "bounded run approached the child's full duration: {elapsed:?}"
    );
}

// --- Forced kill after ignored graceful termination (T5 / T9) -------------

#[test]
fn to06_sigterm_ignoring_child_is_force_killed_after_bounded_grace() {
    let workspace = TempDir::new("to06");
    let run_timeout = Duration::from_millis(600);
    let grace = Duration::from_millis(600);
    let (outcome, elapsed) = run_probe_bounded(
        "execution_timeout_probe_child_ignores_sigterm_then_sleep_long",
        workspace.path(),
        run_timeout,
        grace,
    );

    assert_eq!(
        outcome.termination(),
        ProcessTermination::TimedOutForceKilled
    );
    assert!(outcome.timed_out());
    assert!(outcome.forced_kill_required());
    // A forced kill must never be confusable with an ordinary non-zero
    // exit: no numeric code exists and success is refused outright.
    assert_eq!(outcome.exit_code(), None);
    assert!(!outcome.success());

    assert!(elapsed >= run_timeout);
    assert!(
        elapsed < BOUNDED_RUN_UPPER_BOUND,
        "forced-kill run approached the child's full duration: {elapsed:?}"
    );

    // The force-killed child was reaped, not merely signaled (see to07).
    let pid = recorded_pid(workspace.path());
    assert_ne!(pid, std::process::id());
    assert!(
        !process_alive(pid),
        "force-killed child {pid} remains live or unreaped after the bounded run returned"
    );
}

// --- Final reap proof (T7) -------------------------------------------------

#[test]
fn to07_gracefully_terminated_child_is_reaped_before_return() {
    let workspace = TempDir::new("to07");
    let (outcome, _) = run_probe_bounded(
        "execution_timeout_probe_child_ready_then_sleep_long",
        workspace.path(),
        Duration::from_millis(500),
        Duration::from_millis(500),
    );
    assert_eq!(
        outcome.termination(),
        ProcessTermination::TimedOutGracefullyTerminated
    );

    let pid = recorded_pid(workspace.path());

    // Control: the probe technique discriminates live processes, and the
    // recorded pid genuinely belonged to another process.
    assert!(process_alive(std::process::id()));
    assert_ne!(pid, std::process::id());

    // A killed-but-unreaped child would remain a zombie of this test
    // process and therefore still answer the zero-signal probe; only a
    // fully reaped child answers "no such process".
    assert!(
        !process_alive(pid),
        "runner-owned child {pid} is still live or an unreaped zombie after the bounded run \
         returned"
    );
}

// --- Empty environment through the timed path (T13) ------------------------

#[test]
fn to08_timed_path_preserves_empty_child_environment() {
    let workspace = TempDir::new("to08");
    let (outcome, _) = run_probe_bounded(
        "execution_timeout_probe_child_assert_empty_env_then_ready_and_sleep_long",
        workspace.path(),
        Duration::from_millis(600),
        Duration::from_millis(500),
    );

    // The probe survives to the deadline (graceful classification) only if
    // it observed an empty environment; any leaked variable would have
    // produced an ordinary exit 13 completion instead.
    assert_eq!(
        outcome.termination(),
        ProcessTermination::TimedOutGracefullyTerminated,
        "timed-path child gained ambient environment or died unexpectedly: {outcome:?}"
    );
    assert!(outcome.timed_out());
    assert!(!outcome.success());
}

// --- Preserved validation boundaries on the timed path (T14 / T15) ---------

#[test]
fn to09_timed_path_preserves_symlink_cwd_escape_rejection() {
    let base = TempDir::new("to09");
    let workspace = base.path().join("ws");
    let outside = base.path().join("outside");
    fs::create_dir_all(&workspace).expect("workspace directory creation");
    fs::create_dir_all(&outside).expect("outside directory creation");
    let link = workspace.join("inside-link");
    std::os::unix::fs::symlink(&outside, &link).expect("test symlink creation");

    let error = run_with_timeout(
        &bounded_request(
            first_existing(&["/usr/bin/true", "/bin/true"]),
            &[],
            &workspace,
            &link,
        ),
        &policy(Duration::from_secs(5), Duration::from_secs(1)),
    )
    .expect_err("symlink escape must fail closed on the timed path too");
    assert!(
        matches!(error, ExecutionError::CwdOutsideWorkspace { .. }),
        "expected CwdOutsideWorkspace, got: {error:?}"
    );
}

#[test]
fn to10_timed_path_preserves_shell_executable_rejection() {
    let workspace = TempDir::new("to10");
    let error = run_with_timeout(
        &bounded_request("/bin/sh", &[], workspace.path(), workspace.path()),
        &policy(Duration::from_secs(5), Duration::from_secs(1)),
    )
    .expect_err("shell executables must stay rejected before timeout machinery");
    assert!(
        matches!(error, ExecutionError::ShellExecutableRejected { .. }),
        "expected ShellExecutableRejected, got: {error:?}"
    );
}

// --- Deadline arithmetic fails closed (T12) --------------------------------

#[test]
fn to11_monotonic_deadline_overflow_fails_closed() {
    // Helper-level deterministic proof: adding an unrepresentable interval
    // to the monotonic clock yields the typed overflow error.
    let error = checked_deadline(Instant::now(), Duration::MAX, "run_timeout")
        .expect_err("Duration::MAX cannot be added to a nonzero monotonic reading");
    assert!(matches!(
        error,
        ExecutionError::TimeoutDeadlineOverflow { .. }
    ));

    // End-to-end: the overflow is detected before any filesystem or
    // process resource is touched, even though the request itself names a
    // nonexistent executable.
    let workspace = TempDir::new("to11");
    let error = run_with_timeout(
        &bounded_request(
            workspace.path().join("never-spawned"),
            &[],
            workspace.path(),
            workspace.path(),
        ),
        &policy(Duration::from_secs(u64::MAX), Duration::from_secs(1)),
    )
    .expect_err("unrepresentable deadline must fail closed");
    assert!(matches!(
        error,
        ExecutionError::TimeoutDeadlineOverflow { .. }
    ));
}

#[test]
fn to11a_extreme_termination_grace_is_rejected_before_spawn() {
    let workspace = TempDir::new("to11a");
    let caller_group = caller_process_group();
    let started = Instant::now();
    let error = run_with_timeout(
        &bounded_request(
            std::env::current_exe().expect("current test executable path"),
            &[
                "execution_timeout_probe_parent_of_sigterm_ignoring_descendant_then_sleep_long",
                "--ignored",
            ],
            workspace.path(),
            workspace.path(),
        ),
        &policy(Duration::from_millis(100), Duration::MAX),
    )
    .expect_err("unrepresentable termination grace must fail before spawn");

    assert!(matches!(
        error,
        ExecutionError::TimeoutDeadlineOverflow { .. }
    ));
    assert!(!workspace.path().join("ready").exists());
    assert!(!workspace.path().join("descendant-pid").exists());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(caller_process_group(), caller_group);
}

// --- No output capture on the timed path (T16) ------------------------------

#[test]
fn to12_timed_outcome_is_pure_exit_metadata_no_capture() {
    let workspace = TempDir::new("to12");
    // The child writes payloads to both stdout and stderr; both streams
    // are null at the boundary and the entire result equals plain exit
    // metadata — nothing else exists on the type to inspect.
    let outcome = run_with_timeout(
        &bounded_request(
            first_existing(&["/bin/echo", "/usr/bin/echo"]),
            &["stdout-payload", "stderr-payload"],
            workspace.path(),
            workspace.path(),
        ),
        &policy(Duration::from_secs(30), Duration::from_secs(1)),
    )
    .expect("echoing child completes ordinarily");
    assert_eq!(outcome, ProcessRunOutcome::new(true, Some(0)));

    // Timed-out outcomes likewise carry no captured payload and are never
    // reported successful.
    let timed = ProcessRunOutcome::new_timed_out(
        ProcessTermination::TimedOutGracefullyTerminated,
        Some(143),
    );
    assert!(!timed.success());
    assert!(timed.timed_out());
    assert!(!timed.forced_kill_required());
    let forced = ProcessRunOutcome::new_timed_out(ProcessTermination::TimedOutForceKilled, None);
    assert!(forced.forced_kill_required());
    assert_ne!(timed, forced);
}

// --- Unbounded run() compatibility (T17) ------------------------------------

#[test]
fn to13_plain_run_has_no_hidden_default_deadline() {
    let workspace = TempDir::new("to13");
    let started = Instant::now();
    let outcome = run(&bounded_request(
        first_existing(&["/bin/sleep", "/usr/bin/sleep"]),
        &["2"],
        workspace.path(),
        workspace.path(),
    ))
    .expect("the accepted unbounded API still runs children without a hidden deadline");
    let elapsed = started.elapsed();

    assert_eq!(outcome.termination(), ProcessTermination::Completed);
    assert!(!outcome.timed_out());
    assert!(outcome.success());
    // The child genuinely ran past any plausible hidden default deadline.
    assert!(elapsed >= Duration::from_secs(2));
    assert_eq!(outcome.exit_code(), Some(0));
}

// --- Error mapping coverage (T18) --------------------------------------------

#[test]
fn to14_control_boundary_errors_are_typed_and_fail_closed() {
    let boom = || std::io::Error::other("boom");

    // Pure typed mappings stay distinct per lifecycle boundary.
    let spawn_wait = super::runner::spawn_failed(boom());
    let plain_wait = super::runner::wait_failed(boom());
    let final_wait = super::runner::final_wait_failed(boom());
    let unsupported = ExecutionError::UnsupportedTimeoutPlatform;

    assert!(matches!(
        spawn_wait,
        ExecutionError::ProcessSpawnFailed { .. }
    ));
    assert!(matches!(
        plain_wait,
        ExecutionError::ProcessWaitFailed { .. }
    ));
    assert!(matches!(
        final_wait,
        ExecutionError::TimeoutFinalWaitFailed { .. }
    ));

    // Every boundary renders distinctly so callers never have to parse
    // ambiguity out of an error string.
    let rendered = [
        spawn_wait.to_string(),
        plain_wait.to_string(),
        final_wait.to_string(),
        unsupported.to_string(),
    ];
    for (index, message) in rendered.iter().enumerate() {
        assert!(!message.is_empty());
        assert!(
            rendered[..index].iter().all(|earlier| earlier != message),
            "distinct boundaries must render distinctly"
        );
    }
}

#[test]
fn to15_group_signal_and_presence_classification_tables() {
    // Pure errno tables: no process is ever signaled by this test.

    // Signal delivery classification against an owned group.
    assert_eq!(
        classify_group_kill_result(0, None, 4242, "graceful SIGTERM"),
        GroupSignalDelivery::Delivered
    );
    // ESRCH: every group member had already exited — never a failure.
    assert_eq!(
        classify_group_kill_result(-1, Some(3), 4242, "graceful SIGTERM"),
        GroupSignalDelivery::GroupAlreadyGone
    );
    let failed_perm = classify_group_kill_result(-1, Some(1), 4242, "forced SIGKILL");
    let failed_other = classify_group_kill_result(-1, None, 4242, "forced SIGKILL");
    assert!(matches!(failed_perm, GroupSignalDelivery::Failed { .. }));
    assert!(matches!(failed_other, GroupSignalDelivery::Failed { .. }));
    match (failed_perm, failed_other) {
        (
            GroupSignalDelivery::Failed { detail: first },
            GroupSignalDelivery::Failed { detail: second },
        ) => {
            assert_ne!(first, second);
            assert!(first.contains("-4242"));
            assert!(first.contains("SIGKILL"));
        }
        _ => unreachable!("both classified as failures above"),
    }

    // Presence probing distinguishes empty from occupied from unknown,
    // and EPERM (exists but not signalable by us) counts as OCCUPIED.
    assert_eq!(
        classify_group_presence(0, None, 4242),
        GroupPresence::HasMembers
    );
    assert_eq!(
        classify_group_presence(-1, Some(3), 4242),
        GroupPresence::Empty
    );
    assert_eq!(
        classify_group_presence(-1, Some(1), 4242),
        GroupPresence::HasMembers
    );
    match classify_group_presence(-1, Some(22), 4242) {
        GroupPresence::Unknown { detail } => {
            assert!(detail.contains("-4242"));
            assert!(detail.contains("22"));
        }
        other => panic!("unexpected errno must classify as unknown, got: {other:?}"),
    }
}

// --- Repair regression matrix ---------------------------------------------
//
// R1/R7: dedicated attempt-owned group, caller untouched.
// R3/R4/R5: descendants cannot survive timeout return; ignoring SIGTERM
// forces group-level SIGKILL and forced classification.
// R6: whole-group graceful termination classifies gracefully.
// R8: PID/PGID safety guards refuse unsafe targets fail-closed.
// R9: grace-wait failures clean up without hiding the primary error.

#[test]
fn tg01_timed_child_leads_dedicated_process_group_distinct_from_caller() {
    let workspace = TempDir::new("tg01");
    let caller_group_before = caller_process_group();
    assert!(caller_group_before > 0, "caller pgid must be positive");

    let run_timeout = Duration::from_millis(500);
    let (outcome, elapsed) = run_probe_bounded(
        "execution_timeout_probe_child_records_own_process_group_then_sleep_long",
        workspace.path(),
        run_timeout,
        Duration::from_millis(750),
    );
    assert_eq!(
        outcome.termination(),
        ProcessTermination::TimedOutGracefullyTerminated
    );

    let child_pid = recorded_value(workspace.path(), "pid");
    let child_pgid = recorded_value(workspace.path(), "pgid");

    // The timed child leads its own fresh group (pgid == its pid)…
    assert_eq!(
        child_pgid, child_pid,
        "the timed child must lead its own process group"
    );
    // …which is demonstrably distinct from the caller's process group.
    let caller_unsigned = u32::try_from(caller_group_before).expect("caller pgid positive");
    assert_ne!(
        child_pgid, caller_unsigned,
        "the attempt-owned group must not be the caller's group"
    );

    assert!(elapsed >= run_timeout);
    // The caller/test harness stays alive with its group untouched.
    assert!(process_alive(std::process::id()));
    assert_eq!(caller_process_group(), caller_group_before);

    // The direct child was fully reaped and its group no longer exists.
    await_process_death(child_pid, "timed probe child");
    await_group_empty(child_pgid, "tg01");
}

#[test]
fn tg02_whole_group_graceful_termination_classifies_gracefully_without_orphans() {
    let workspace = TempDir::new("tg02");
    let caller_group_before = caller_process_group();

    let run_timeout = Duration::from_millis(600);
    let (outcome, elapsed) = run_probe_bounded(
        "execution_timeout_probe_parent_of_graceful_descendant_then_sleep_long",
        workspace.path(),
        run_timeout,
        Duration::from_millis(1000),
    );

    // Parent AND descendant both honored SIGTERM during grace: graceful
    // classification with no force kill anywhere in the attempt tree.
    assert_eq!(
        outcome.termination(),
        ProcessTermination::TimedOutGracefullyTerminated
    );
    assert!(outcome.timed_out());
    assert!(!outcome.forced_kill_required());
    assert!(!outcome.success());
    assert_eq!(outcome.exit_code(), None);
    assert!(elapsed >= run_timeout);
    assert!(
        elapsed < BOUNDED_RUN_UPPER_BOUND,
        "bounded run approached the probes' full duration: {elapsed:?}"
    );

    let attempt_pid = recorded_value(workspace.path(), "attempt-pid");
    let attempt_pgid = recorded_value(workspace.path(), "attempt-pgid");
    let descendant_pid = recorded_value(workspace.path(), "descendant-pid");
    assert_ne!(attempt_pid, std::process::id());

    // Direct child reaped, descendant gone, owned group empty.
    await_process_death(attempt_pid, "timed attempt child");
    await_process_death(descendant_pid, "graceful attempt descendant");
    await_group_empty(attempt_pgid, "tg02");

    // Caller alive; caller group untouched and provably different.
    assert!(process_alive(std::process::id()));
    assert_eq!(caller_process_group(), caller_group_before);
    let caller_unsigned = u32::try_from(caller_group_before).expect("caller pgid positive");
    assert_ne!(attempt_pgid, caller_unsigned);
}

#[test]
fn tg03_sigterm_ignoring_descendant_triggers_group_sigkill_and_forced_classification() {
    let workspace = TempDir::new("tg03");
    let caller_group_before = caller_process_group();

    let run_timeout = Duration::from_millis(600);
    let grace = Duration::from_millis(600);
    let (outcome, elapsed) = run_probe_bounded(
        "execution_timeout_probe_parent_of_sigterm_ignoring_descendant_then_sleep_long",
        workspace.path(),
        run_timeout,
        grace,
    );

    // The DIRECT child exited from the graceful SIGTERM, but the
    // descendant ignored it — the classification must still be forced,
    // because attempt cleanup required the group-level SIGKILL.
    assert_eq!(
        outcome.termination(),
        ProcessTermination::TimedOutForceKilled
    );
    assert!(outcome.timed_out());
    assert!(outcome.forced_kill_required());
    assert!(!outcome.success());
    assert_eq!(outcome.exit_code(), None);
    assert!(elapsed >= run_timeout);
    assert!(
        elapsed < BOUNDED_RUN_UPPER_BOUND,
        "forced group-kill run approached the probes' full duration: {elapsed:?}"
    );

    let attempt_pid = recorded_value(workspace.path(), "attempt-pid");
    let attempt_pgid = recorded_value(workspace.path(), "attempt-pgid");
    let descendant_pid = recorded_value(workspace.path(), "descendant-pid");
    assert_ne!(attempt_pid, std::process::id());

    // No attempt-owned member survives the return: direct child reaped,
    // the SIGTERM-ignoring descendant dead, owned group empty.
    await_process_death(attempt_pid, "timed attempt child");
    await_process_death(descendant_pid, "SIGTERM-ignoring descendant");
    await_group_empty(attempt_pgid, "tg03");

    // Caller alive; caller group untouched and provably different.
    assert!(process_alive(std::process::id()));
    assert_eq!(caller_process_group(), caller_group_before);
    let caller_unsigned = u32::try_from(caller_group_before).expect("caller pgid positive");
    assert_ne!(attempt_pgid, caller_unsigned);
}

#[test]
fn tg03a_churning_descendants_are_quiesced_before_leader_reap() {
    let workspace = TempDir::new("tg03a");
    let caller_group = caller_process_group();
    let _ = take_timeout_lifecycle_events();

    assert!(
        CHURN_SAFETY_WINDOW
            > Duration::from_millis(500) + Duration::from_millis(400) + Duration::from_secs(1),
        "the helper safety ceiling must outlast deadline, grace, and quiescence"
    );

    let (outcome, elapsed) = run_probe_bounded(
        "execution_timeout_probe_parent_of_churning_descendant",
        workspace.path(),
        Duration::from_millis(500),
        Duration::from_millis(400),
    );
    assert_eq!(
        outcome.termination(),
        ProcessTermination::TimedOutForceKilled
    );
    assert!(elapsed < BOUNDED_RUN_UPPER_BOUND);

    let attempt_pgid = recorded_value(workspace.path(), "attempt-pgid");
    await_process_death(
        recorded_value(workspace.path(), "attempt-pid"),
        "churn leader",
    );
    await_process_death(
        recorded_value(workspace.path(), "churn-pid"),
        "churn descendant",
    );
    await_group_empty(attempt_pgid, "tg03a fork race");
    assert_eq!(caller_process_group(), caller_group);

    let events = take_timeout_lifecycle_events();
    let position = |event| {
        events
            .iter()
            .position(|candidate| *candidate == event)
            .unwrap_or_else(|| panic!("missing {event:?} in {events:?}"))
    };
    assert!(
        position(TimeoutLifecycleEvent::GroupSigterm)
            < position(TimeoutLifecycleEvent::GroupSigstop)
    );
    assert!(
        position(TimeoutLifecycleEvent::GroupSigstop)
            < position(TimeoutLifecycleEvent::GroupQuiescent)
    );
    assert!(
        position(TimeoutLifecycleEvent::GroupQuiescent)
            < position(TimeoutLifecycleEvent::GroupSigkill)
    );
    assert!(
        position(TimeoutLifecycleEvent::GroupSigkill)
            < position(TimeoutLifecycleEvent::LeaderReaped)
    );
    assert!(
        position(TimeoutLifecycleEvent::LeaderReaped)
            < position(TimeoutLifecycleEvent::GroupEmptyVerified)
    );
}

#[test]
fn tg03b_churn_helper_has_finite_independent_wall_clock_bound() {
    for _ in 0..5 {
        let workspace = TempDir::new("tg03b");
        let outcome = run_churn_helper(&workspace, false, false, ChurnHelperFault::None)
            .unwrap_or_else(|failure| panic!("bounded churn helper failed: {failure}"));
        assert!(!outcome.timed_out);
        assert!(
            outcome.cleanup.status.success(),
            "bounded churn helper failed: {:?}",
            outcome.cleanup.status
        );
        assert_owned_churn_cleanup(&outcome.cleanup);
        assert!(!outcome.cleanup.group_sigkill_delivered);
        let elapsed = outcome.elapsed;
        assert!(elapsed >= CHURN_SAFETY_WINDOW);
        assert!(elapsed < BOUNDED_RUN_UPPER_BOUND);
    }
}

#[derive(Debug)]
struct ChurnHelperOutcome {
    cleanup: OwnedChurnCleanupEvidence,
    elapsed: Duration,
    timed_out: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChurnHelperFault {
    None,
    Observation,
    Ownership,
    Release,
    Readiness,
}

#[derive(Debug)]
struct OwnedChurnCleanupEvidence {
    status: std::process::ExitStatus,
    group_signaling_complete: bool,
    group_sigkill_delivered: bool,
    future_group_signals: bool,
    helper_reaped: bool,
    group_empty: bool,
    caller_pgid_preserved: bool,
}

#[derive(Debug)]
enum ChurnCleanupEvidence {
    Owned(OwnedChurnCleanupEvidence),
    PreOwnership {
        status: std::process::ExitStatus,
        helper_reaped: bool,
        caller_pgid_preserved: bool,
    },
}

#[derive(Debug)]
struct ChurnHelperFailure {
    primary: String,
    cleanup: Result<ChurnCleanupEvidence, String>,
    helper_pid: u32,
    leaf_alive_before_cleanup: Option<bool>,
}

impl std::fmt::Display for ChurnHelperFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.cleanup {
            Ok(evidence) => write!(
                formatter,
                "primary: {}; cleanup: PASS ({evidence:?})",
                self.primary
            ),
            Err(cleanup) => write!(
                formatter,
                "SAFETY-CRITICAL CLEANUP FAILURE: {cleanup}; primary: {}",
                self.primary
            ),
        }
    }
}

fn assert_owned_churn_cleanup(cleanup: &OwnedChurnCleanupEvidence) {
    assert!(cleanup.group_signaling_complete);
    assert!(!cleanup.future_group_signals);
    assert!(cleanup.helper_reaped);
    assert!(cleanup.group_empty);
    assert!(cleanup.caller_pgid_preserved);
}

fn marker_appears_within(workspace: &Path, marker: &str, window: Duration) -> bool {
    let Some(deadline) = Instant::now().checked_add(window) else {
        return false;
    };
    while !workspace.join(marker).exists() {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    true
}

fn churn_start_released(path: &Path) -> bool {
    matches!(fs::read(path), Ok(contents) if contents == CHURN_START_TOKEN)
}

fn bounded_reap_churn_helper(
    child: &mut std::process::Child,
) -> Result<std::process::ExitStatus, String> {
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(5))
        .ok_or_else(|| "could not represent direct-helper reap deadline".to_string())?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(format!("direct-helper reap failed: {error}")),
        }
        if Instant::now() >= deadline {
            return Err("direct helper remained unreaped beyond cleanup deadline".to_string());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn recorded_group_empties_within(pgid: u32, window: Duration) -> bool {
    let Some(deadline) = Instant::now().checked_add(window) else {
        return false;
    };
    loop {
        if recorded_group_is_empty(pgid) == Some(true) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn finish_owned_churn_helper_cleanup(
    child: &mut std::process::Child,
    group: OwnedProcessGroup,
    caller_group: std::os::raw::c_int,
    group_sigkill_delivered: bool,
    mut failures: Vec<String>,
) -> Result<OwnedChurnCleanupEvidence, String> {
    let group_signaling_complete = failures.is_empty();

    // No group signal may occur below this point: Child::try_wait may reap.
    let status = match bounded_reap_churn_helper(child) {
        Ok(status) => Some(status),
        Err(error) => {
            failures.push(error);
            None
        }
    };
    let group_empty = recorded_group_empties_within(group.raw() as u32, Duration::from_secs(10));
    if !group_empty {
        failures.push(format!(
            "owned process group -{} was not proven empty after direct-helper reap",
            group.raw()
        ));
    }
    let caller_pgid_preserved = caller_process_group() == caller_group;
    if !caller_pgid_preserved {
        failures.push(format!(
            "caller process group changed from {caller_group} to {}",
            caller_process_group()
        ));
    }

    if failures.is_empty() {
        Ok(OwnedChurnCleanupEvidence {
            status: status.expect("successful cleanup includes a reaped status"),
            group_signaling_complete,
            group_sigkill_delivered,
            future_group_signals: false,
            helper_reaped: true,
            group_empty,
            caller_pgid_preserved,
        })
    } else {
        Err(failures.join("; "))
    }
}

/// Force-contains a validated helper-owned group before boundedly reaping its
/// leader. No membership observation may skip this failure-path SIGKILL, and
/// no group signal may occur after this function begins direct-child reaping.
fn cleanup_owned_churn_helper_failure(
    child: &mut std::process::Child,
    group: OwnedProcessGroup,
    caller_group: std::os::raw::c_int,
) -> Result<OwnedChurnCleanupEvidence, String> {
    let mut failures = Vec::new();
    let group_sigkill_delivered = match deliver_group_sigkill(group) {
        GroupSignalDelivery::Delivered | GroupSignalDelivery::GroupAlreadyGone => true,
        GroupSignalDelivery::Failed { detail } => {
            failures.push(format!("initial owned-group SIGKILL failed: {detail}"));
            match deliver_group_sigkill(group) {
                GroupSignalDelivery::Delivered | GroupSignalDelivery::GroupAlreadyGone => true,
                GroupSignalDelivery::Failed { detail } => {
                    failures.push(format!("owned-group SIGKILL retry failed: {detail}"));
                    false
                }
            }
        }
    };

    finish_owned_churn_helper_cleanup(
        child,
        group,
        caller_group,
        group_sigkill_delivered,
        failures,
    )
}

fn cleanup_blocked_churn_helper(
    child: &mut std::process::Child,
    caller_group: std::os::raw::c_int,
) -> Result<ChurnCleanupEvidence, String> {
    let mut failures = Vec::new();
    if let Err(error) = child.kill() {
        failures.push(format!("direct-helper kill failed: {error}"));
    }
    let status = match bounded_reap_churn_helper(child) {
        Ok(status) => Some(status),
        Err(error) => {
            failures.push(error);
            None
        }
    };
    let caller_pgid_preserved = caller_process_group() == caller_group;
    if !caller_pgid_preserved {
        failures.push(format!(
            "caller process group changed from {caller_group} to {}",
            caller_process_group()
        ));
    }
    if failures.is_empty() {
        Ok(ChurnCleanupEvidence::PreOwnership {
            status: status.expect("successful cleanup includes a reaped status"),
            helper_reaped: true,
            caller_pgid_preserved,
        })
    } else {
        Err(failures.join("; "))
    }
}

/// Runs the churn helper in a fresh group. Failure paths contain the complete
/// group before reaping; normal success first proves leader exit and group
/// quiescence while the leader remains waitable.
fn run_churn_helper(
    workspace: &TempDir,
    force_timeout: bool,
    fail_after_leaf: bool,
    fault: ChurnHelperFault,
) -> Result<ChurnHelperOutcome, ChurnHelperFailure> {
    use std::os::unix::process::CommandExt;

    let caller_group = caller_process_group();
    let started = Instant::now();
    let mut command =
        std::process::Command::new(std::env::current_exe().expect("current test executable path"));
    command
        .args([
            "execution_timeout_probe_descendant_churns_after_sigterm",
            "--ignored",
        ])
        .current_dir(workspace.path())
        .env_remove("RECEIPTS_RECORD_CHURN_LEAF")
        .env_remove("RECEIPTS_HOLD_RECORDED_CHURN_LEAF")
        .env_remove("RECEIPTS_FAIL_CHURN_HELPER_AFTER_LEAF")
        .env_remove("RECEIPTS_SUPPRESS_CHURN_LEAF_READY")
        .env("RECEIPTS_CHURN_START_BARRIER", "1");
    command.process_group(0);
    if force_timeout || fail_after_leaf || fault == ChurnHelperFault::Readiness {
        command.env("RECEIPTS_HOLD_RECORDED_CHURN_LEAF", "1");
    }
    if fail_after_leaf {
        command.env("RECEIPTS_FAIL_CHURN_HELPER_AFTER_LEAF", "1");
    }
    if fault == ChurnHelperFault::Readiness {
        command.env("RECEIPTS_SUPPRESS_CHURN_LEAF_READY", "1");
    }

    let mut child = command.spawn().expect("bounded churn helper spawn");
    let helper_pid = child.id();
    if !marker_appears_within(
        workspace.path(),
        "churn-helper-ready",
        Duration::from_secs(5),
    ) {
        return Err(ChurnHelperFailure {
            primary: "churn helper did not reach its pre-leaf ownership barrier".to_string(),
            cleanup: cleanup_blocked_churn_helper(&mut child, caller_group),
            helper_pid,
            leaf_alive_before_cleanup: None,
        });
    }
    let group = if fault == ChurnHelperFault::Ownership {
        None
    } else {
        OwnedProcessGroup::from_child_pid(helper_pid)
    };
    let Some(group) = group else {
        return Err(ChurnHelperFailure {
            primary: "churn helper process-group ownership was not established".to_string(),
            cleanup: cleanup_blocked_churn_helper(&mut child, caller_group),
            helper_pid,
            leaf_alive_before_cleanup: None,
        });
    };
    if group.raw() == caller_group {
        return Err(ChurnHelperFailure {
            primary: "churn helper group unexpectedly matched the caller group".to_string(),
            cleanup: cleanup_blocked_churn_helper(&mut child, caller_group),
            helper_pid,
            leaf_alive_before_cleanup: None,
        });
    }
    if fault == ChurnHelperFault::Release
        && let Err(error) = fs::create_dir(workspace.path().join("churn-helper-start"))
    {
        return Err(ChurnHelperFailure {
            primary: format!("could not prepare release-failure condition: {error}"),
            cleanup: cleanup_owned_churn_helper_failure(&mut child, group, caller_group)
                .map(ChurnCleanupEvidence::Owned),
            helper_pid,
            leaf_alive_before_cleanup: None,
        });
    }
    if let Err(error) = fs::write(
        workspace.path().join("churn-helper-start"),
        CHURN_START_TOKEN,
    ) {
        return Err(ChurnHelperFailure {
            primary: format!("could not release owned churn helper: {error}"),
            cleanup: cleanup_owned_churn_helper_failure(&mut child, group, caller_group)
                .map(ChurnCleanupEvidence::Owned),
            helper_pid,
            leaf_alive_before_cleanup: None,
        });
    }

    let ready = marker_appears_within(workspace.path(), "descendant-ready", Duration::from_secs(5))
        && marker_appears_within(workspace.path(), "churn-leaf-ready", Duration::from_secs(5));
    let deadline = if force_timeout {
        Instant::now()
    } else {
        started + CHURN_SAFETY_WINDOW + CHURN_SETTLE_WINDOW + Duration::from_secs(2)
    };
    let mut primary = (!ready).then(|| "churn helper did not publish leaf readiness".to_string());
    let leaf_alive_before_cleanup = if fault == ChurnHelperFault::Readiness
        && marker_appears_within(workspace.path(), "churn-leaf-pid", Duration::from_secs(1))
    {
        Some(process_alive(recorded_value(
            workspace.path(),
            "churn-leaf-pid",
        )))
    } else {
        None
    };
    let mut observation_fault_pending = fault == ChurnHelperFault::Observation;
    let mut leader_exited = false;
    let timed_out = loop {
        if primary.is_some() {
            break false;
        }
        let observation = if observation_fault_pending {
            observation_fault_pending = false;
            Err(std::io::Error::other(
                "synthetic live-leader observation failure",
            ))
        } else {
            observe_leader_without_reaping(helper_pid)
        };
        match observation {
            Ok(LeaderState::Exited) => {
                leader_exited = true;
                break false;
            }
            Ok(LeaderState::Running) => {}
            Err(error) => {
                primary = Some(format!("leader observation failed: {error}"));
                break false;
            }
        }
        if Instant::now() >= deadline {
            break true;
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    if let Some(primary) = primary {
        let cleanup = cleanup_owned_churn_helper_failure(&mut child, group, caller_group);
        return Err(ChurnHelperFailure {
            primary,
            cleanup: cleanup.map(ChurnCleanupEvidence::Owned),
            helper_pid,
            leaf_alive_before_cleanup,
        });
    }
    let cleanup = if timed_out || fail_after_leaf {
        cleanup_owned_churn_helper_failure(&mut child, group, caller_group)
    } else if leader_exited && group_quiescence(group, Some(helper_pid)) == GroupQuiescence::Empty {
        finish_owned_churn_helper_cleanup(&mut child, group, caller_group, false, Vec::new())
    } else {
        let cleanup = cleanup_owned_churn_helper_failure(&mut child, group, caller_group);
        return Err(ChurnHelperFailure {
            primary: "churn helper descendants were not quiescent after leader exit".to_string(),
            cleanup: cleanup.map(ChurnCleanupEvidence::Owned),
            helper_pid,
            leaf_alive_before_cleanup,
        });
    };
    let cleanup = match cleanup {
        Ok(cleanup) => cleanup,
        Err(detail) => {
            return Err(ChurnHelperFailure {
                primary: "owned churn-helper cleanup failed".to_string(),
                cleanup: Err(detail),
                helper_pid,
                leaf_alive_before_cleanup,
            });
        }
    };

    assert!(
        ready,
        "churn helper did not publish readiness before cleanup"
    );
    let helper_pgid = recorded_value(workspace.path(), "churn-pgid");
    let leaf_pid = recorded_value(workspace.path(), "churn-leaf-pid");
    let leaf_pgid = recorded_value(workspace.path(), "churn-leaf-pgid");
    assert_eq!(helper_pgid, helper_pid);
    assert_eq!(leaf_pgid, helper_pid);
    await_process_death(helper_pid, "direct churn helper");
    await_process_death(leaf_pid, "recorded churn leaf");
    await_group_empty(helper_pgid, "churn helper cleanup");
    assert_eq!(caller_process_group(), caller_group);

    Ok(ChurnHelperOutcome {
        cleanup,
        elapsed: started.elapsed(),
        timed_out,
    })
}

#[test]
fn tg03c_churn_helper_timeout_cleans_complete_owned_group() {
    let workspace = TempDir::new("tg03c");
    let outcome = run_churn_helper(&workspace, true, false, ChurnHelperFault::None)
        .unwrap_or_else(|failure| panic!("forced cleanup failed: {failure}"));
    assert!(outcome.timed_out, "forced timeout must be reported");
    assert_owned_churn_cleanup(&outcome.cleanup);
    assert!(outcome.cleanup.group_sigkill_delivered);
    let report = std::panic::catch_unwind(|| {
        panic!("churn helper exceeded its independent wall-clock bound")
    });
    assert!(report.is_err());
}

#[test]
fn tg03d_churn_helper_non_success_cleans_group_before_panic() {
    for _ in 0..10 {
        let workspace = TempDir::new("tg03d");
        let outcome = run_churn_helper(&workspace, false, true, ChurnHelperFault::None)
            .unwrap_or_else(|failure| panic!("non-success cleanup failed: {failure}"));
        assert!(!outcome.timed_out);
        assert_owned_churn_cleanup(&outcome.cleanup);
        assert!(outcome.cleanup.group_sigkill_delivered);
        assert!(
            !outcome.cleanup.status.success(),
            "forced-failure helper must report non-success after cleanup"
        );
        let report = std::panic::catch_unwind(|| {
            assert!(
                outcome.cleanup.status.success(),
                "bounded churn helper failed: {:?}",
                outcome.cleanup.status
            );
        });
        assert!(report.is_err());
    }
}

#[test]
fn tg03e_observation_failure_cleans_live_churn_group_before_reporting() {
    for _ in 0..10 {
        let workspace = TempDir::new("tg03e");
        let caller_group = caller_process_group();
        let failure = run_churn_helper(&workspace, true, false, ChurnHelperFault::Observation)
            .expect_err("synthetic observation failure must be reported after cleanup");

        assert!(failure.primary.contains("leader observation failed"));
        assert!(
            failure
                .primary
                .contains("synthetic live-leader observation failure")
        );
        let cleanup = match failure.cleanup {
            Ok(ChurnCleanupEvidence::Owned(cleanup)) => cleanup,
            other => panic!("observation failure lacked owned cleanup evidence: {other:?}"),
        };
        assert_owned_churn_cleanup(&cleanup);
        assert!(cleanup.group_sigkill_delivered);

        let leaf_pid = recorded_value(workspace.path(), "churn-leaf-pid");
        let helper_pgid = recorded_value(workspace.path(), "churn-pgid");
        assert_eq!(helper_pgid, failure.helper_pid);
        await_process_death(failure.helper_pid, "observation-failure helper");
        await_process_death(leaf_pid, "observation-failure controlled leaf");
        await_group_empty(helper_pgid, "observation-failure cleanup");
        assert_eq!(caller_process_group(), caller_group);
    }
}

#[test]
fn tg03f_ownership_failure_reaps_blocked_helper_before_leaf_creation() {
    for _ in 0..10 {
        let workspace = TempDir::new("tg03f");
        let caller_group = caller_process_group();
        let failure = run_churn_helper(&workspace, false, false, ChurnHelperFault::Ownership)
            .expect_err("synthetic ownership rejection must be reported after direct cleanup");

        assert!(failure.primary.contains("ownership was not established"));
        match failure.cleanup {
            Ok(ChurnCleanupEvidence::PreOwnership {
                status,
                helper_reaped,
                caller_pgid_preserved,
            }) => {
                assert!(!status.success());
                assert!(helper_reaped);
                assert!(caller_pgid_preserved);
            }
            other => panic!("ownership failure lacked direct cleanup evidence: {other:?}"),
        }
        assert!(!process_alive(failure.helper_pid));
        assert!(!workspace.path().join("churn-pid").exists());
        assert!(!workspace.path().join("churn-leaf-pid").exists());
        assert!(!workspace.path().join("churn-leaf-ready").exists());
        assert_eq!(caller_process_group(), caller_group);
    }
}

#[test]
fn tg03g_churn_start_barrier_requires_exact_token_file() {
    let workspace = TempDir::new("tg03g");
    let start = workspace.path().join("churn-helper-start");

    assert!(!churn_start_released(&start));
    fs::create_dir(&start).expect("directory release impostor");
    assert!(!churn_start_released(&start));
    fs::remove_dir(&start).expect("remove directory release impostor");
    fs::write(&start, b"wrong-token").expect("malformed release token");
    assert!(!churn_start_released(&start));
    fs::write(&start, CHURN_START_TOKEN).expect("valid release token");
    assert!(churn_start_released(&start));
}

#[test]
fn tg03h_release_failure_contains_live_helper_without_false_empty() {
    for _ in 0..10 {
        let workspace = TempDir::new("tg03h");
        let caller_group = caller_process_group();
        let failure = run_churn_helper(&workspace, false, false, ChurnHelperFault::Release)
            .expect_err("directory-at-release-path must fail before contained return");

        assert!(
            failure
                .primary
                .contains("could not release owned churn helper")
        );
        let cleanup = match failure.cleanup {
            Ok(ChurnCleanupEvidence::Owned(cleanup)) => cleanup,
            other => panic!("release failure lacked owned cleanup evidence: {other:?}"),
        };
        assert_owned_churn_cleanup(&cleanup);
        assert!(cleanup.group_sigkill_delivered);
        assert!(!process_alive(failure.helper_pid));
        assert!(!workspace.path().join("churn-pid").exists());
        assert!(!workspace.path().join("churn-leaf-pid").exists());
        await_group_empty(failure.helper_pid, "release-failure cleanup");
        assert_eq!(caller_process_group(), caller_group);
    }
}

#[test]
fn tg03i_post_release_readiness_failure_contains_live_leaf() {
    for _ in 0..10 {
        let workspace = TempDir::new("tg03i");
        let caller_group = caller_process_group();
        let failure = run_churn_helper(&workspace, false, false, ChurnHelperFault::Readiness)
            .expect_err("suppressed post-release readiness must fail after containment");

        assert!(failure.primary.contains("did not publish leaf readiness"));
        assert_eq!(failure.leaf_alive_before_cleanup, Some(true));
        let leaf_pid = recorded_value(workspace.path(), "churn-leaf-pid");
        let helper_pgid = recorded_value(workspace.path(), "churn-pgid");
        assert_eq!(helper_pgid, failure.helper_pid);
        let cleanup = match failure.cleanup {
            Ok(ChurnCleanupEvidence::Owned(cleanup)) => cleanup,
            other => panic!("readiness failure lacked owned cleanup evidence: {other:?}"),
        };
        assert_owned_churn_cleanup(&cleanup);
        assert!(cleanup.group_sigkill_delivered);
        assert!(!process_alive(failure.helper_pid));
        assert!(!process_alive(leaf_pid));
        await_group_empty(helper_pgid, "post-release readiness-failure cleanup");
        assert_eq!(caller_process_group(), caller_group);
    }
}

/// Spawns one controlled `sleep` child in its own fresh process group,
/// mirroring the production spawn shape narrowly, for exercising typed
/// cleanup helpers against real process state.
fn spawn_controlled_group_child() -> (std::process::Child, OwnedProcessGroup, u32) {
    use std::os::unix::process::CommandExt;
    let executable = first_existing(&["/bin/sleep", "/usr/bin/sleep"]);
    let mut command = std::process::Command::new(executable);
    command.arg("30");
    command.process_group(0);
    let child = command.spawn().expect("controlled cleanup-test child");
    let pid = child.id();
    let group =
        OwnedProcessGroup::from_child_pid(pid).expect("fresh child certifies as an owned group");
    (child, group, pid)
}

#[test]
fn tg05a_non_reaping_exit_observation_preserves_the_waitable_leader() {
    use std::os::unix::process::CommandExt;
    let executable = first_existing(&["/usr/bin/true", "/bin/true"]);
    let mut command = std::process::Command::new(executable);
    command.process_group(0);
    let mut child = command.spawn().expect("controlled observation child");
    let pid = child.id();
    let deadline = Instant::now() + Duration::from_secs(5);
    while observe_leader_without_reaping(pid).expect("waitid WNOWAIT observation")
        != LeaderState::Exited
    {
        assert!(Instant::now() < deadline, "controlled child did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        process_alive(pid),
        "WNOWAIT must leave the exited leader waitable and unreaped"
    );
    child.wait().expect("final controlled-child reap");
    assert!(
        !process_alive(pid),
        "Child::wait must perform the final reap"
    );
}

#[test]
fn tg05_grace_wait_failure_cleanup_preserves_primary_error_within_bounded_cleanup() {
    let (child, group, pid) = spawn_controlled_group_child();
    let original = std::io::Error::other("synthetic grace observation failure");

    let error = grace_wait_failed_with_cleanup(child, group, original);

    match &error {
        ExecutionError::TimeoutGraceWaitFailed { detail } => {
            // The primary grace-wait error leads and is never hidden or
            // replaced by cleanup evidence.
            assert!(
                detail.starts_with("grace-window observation failed"),
                "primary error must stay primary: {detail}"
            );
            assert!(detail.contains("synthetic grace observation failure"));
            // Bounded best-effort cleanup was attempted and evidenced;
            // nothing claims successful timeout metadata anywhere.
            assert!(detail.contains("best-effort cleanup"));
            assert!(
                detail.contains("SIGKILL delivered") || detail.contains("already empty"),
                "group cleanup evidence missing: {detail}"
            );
            assert!(
                detail.contains("direct child was reaped"),
                "reap evidence missing: {detail}"
            );
            assert!(!detail.contains("TimedOut"));
        }
        other => panic!("expected TimeoutGraceWaitFailed, got: {other:?}"),
    }

    // The best-effort cleanup really happened: the controlled child was
    // forced down inside its own fresh group and reaped.
    assert_ne!(pid, std::process::id());
    await_process_death(pid, "grace-wait cleanup child");
}

#[test]
fn tg06_force_kill_delivery_failure_preserves_evidence_and_attempts_bounded_cleanup() {
    let (child, group, pid) = spawn_controlled_group_child();

    let error = force_kill_failed_with_cleanup(
        child,
        group,
        "synthetic SIGKILL delivery refusal".to_string(),
    );

    match &error {
        ExecutionError::ForceKillFailed { detail } => {
            // Original delivery failure preserved as primary evidence.
            assert!(
                detail.contains("synthetic SIGKILL delivery refusal"),
                "primary failure must stay primary: {detail}"
            );
            assert!(detail.contains(&format!("-{}", group.raw())));
            // One bounded best-effort retry plus direct-child reap were
            // attempted, and follow-up group evidence is preserved. No Ok
            // outcome can ever be fabricated from this path.
            assert!(detail.contains("retry SIGKILL reached the owned group"));
            assert!(detail.contains("direct child was reaped"));
            assert!(detail.contains("follow-up evidence"));
            assert!(detail.contains("observed empty after failure"));
        }
        other => panic!("expected ForceKillFailed, got: {other:?}"),
    }

    assert_ne!(pid, std::process::id());
    await_process_death(pid, "force-kill-failure cleanup child");
}

#[test]
fn tg04_owned_group_guards_reject_unsafe_targets_fail_closed() {
    // Zero would reach the caller's entire process group under
    // kill(0, ...) / kill(-0, ...): refused outright.
    assert_eq!(OwnedProcessGroup::from_child_pid(0), None);
    // One is the kernel-wide broadcast form under kill(-1, ...): refused.
    assert_eq!(OwnedProcessGroup::from_child_pid(1), None);
    // The caller's live process group must never become a signaling
    // target, however the value arrived.
    let caller = caller_process_group();
    assert!(caller > 0);
    let caller_unsigned = u32::try_from(caller).expect("caller pgid positive");
    assert_eq!(OwnedProcessGroup::from_child_pid(caller_unsigned), None);
    // Values outside pid_t fail closed instead of converting lossily.
    // Negative arbitrary values are unrepresentable at the type boundary
    // (u32 input), closing that door by construction.
    assert_eq!(OwnedProcessGroup::from_child_pid(0x8000_0000), None);
    // A plausible freshly created dedicated group certifies intact and
    // never aliases the caller's group.
    let candidate = caller_unsigned ^ 0x4000_0000;
    assert_ne!(candidate, caller_unsigned);
    assert!(candidate > 1);
    let certified =
        OwnedProcessGroup::from_child_pid(candidate).expect("plausible fresh group certifies");
    assert_eq!(
        certified.raw(),
        i32::try_from(candidate).expect("candidate fits pid_t")
    );

    // The test-only emptiness probe likewise refuses implausible ids
    // rather than guessing about group state.
    assert_eq!(recorded_group_is_empty(0), None);
    assert_eq!(recorded_group_is_empty(u32::MAX), None);
}
