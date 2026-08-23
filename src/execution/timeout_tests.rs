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

use super::runner::checked_deadline;
use crate::execution::unix_signal::unix::{SignalDelivery, classify_kill_result, process_alive};
use crate::execution::{
    ExecutionError, ProcessRunOutcome, ProcessRunRequest, ProcessTermination, ProcessTimeoutPolicy,
    run, run_with_timeout,
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

/// Reads the runner-owned probe child's recorded pid.
fn recorded_pid(workspace_root: &Path) -> u32 {
    await_marker(workspace_root, "pid");
    fs::read_to_string(workspace_root.join("pid"))
        .expect("pid marker readable")
        .trim()
        .parse()
        .expect("recorded pid parses")
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

// --- Policy validation (T10 / T11) ---------------------------------------

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
fn to14_control_boundary_errors_are_typed_distinct_and_fail_closed() {
    let boom = || std::io::Error::other("boom");

    let grace_wait = super::runner::grace_wait_failed(boom());
    let final_wait = super::runner::final_wait_failed(boom());
    let force_none = super::runner::force_kill_failed(boom(), None);
    let force_observe = super::runner::force_kill_failed(boom(), Some(boom()));

    assert!(matches!(
        grace_wait,
        ExecutionError::TimeoutGraceWaitFailed { .. }
    ));
    assert!(matches!(
        final_wait,
        ExecutionError::TimeoutFinalWaitFailed { .. }
    ));
    assert!(matches!(force_none, ExecutionError::ForceKillFailed { .. }));
    assert!(matches!(
        force_observe,
        ExecutionError::ForceKillFailed { .. }
    ));

    // Every boundary renders distinctly, and the follow-up observation
    // failure is preserved rather than silently dropped.
    let rendered = [
        grace_wait.to_string(),
        final_wait.to_string(),
        force_none.to_string(),
        force_observe.to_string(),
    ];
    for (index, message) in rendered.iter().enumerate() {
        assert!(!message.is_empty());
        assert!(
            rendered[..index].iter().all(|earlier| earlier != message),
            "distinct boundaries must render distinctly"
        );
    }
    assert!(rendered[3].contains("observation"));
    assert!(!rendered[2].contains("also failed"));
}

#[test]
fn to15_sigterm_delivery_classification_table() {
    // Pure errno table: no process is ever signaled by this test.
    assert_eq!(
        classify_kill_result(0, None, 4242),
        SignalDelivery::Delivered
    );
    assert_eq!(
        classify_kill_result(-1, Some(3), 4242),
        SignalDelivery::AlreadyExited
    );
    let failed = classify_kill_result(-1, Some(1), 4242);
    assert!(matches!(failed, SignalDelivery::Failed { .. }));
    let failed_other = classify_kill_result(-1, None, 4242);
    assert!(matches!(failed_other, SignalDelivery::Failed { .. }));
    match (failed, failed_other) {
        (SignalDelivery::Failed { detail: first }, SignalDelivery::Failed { detail: second }) => {
            assert_ne!(first, second);
            assert!(first.contains("4242"));
        }
        _ => unreachable!("both classified as failures above"),
    }
}
