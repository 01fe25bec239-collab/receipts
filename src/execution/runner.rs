//! The argv-only local process-runner foundation.
//!
//! [`run`] executes exactly one program described by a
//! [`ProcessRunRequest`]: one explicit absolute executable path, arguments
//! traveling as discrete argv values, and a requested child working
//! directory that is proven — by canonical filesystem resolution, never by
//! lexical string comparison — to lie at or inside the canonical workspace
//! root.
//!
//! [`run_with_timeout`] executes the same validated request under an
//! explicitly supplied orchestrator-owned [`ProcessTimeoutPolicy`]: the
//! identical spawn boundary and validations are reused (there is no less-
//! safe duplicate launcher), monitoring uses the monotonic clock, and an
//! expired deadline triggers the frozen lifecycle sequence — graceful
//! termination (`SIGTERM`) of the attempt-owned process group, a bounded
//! termination grace, then a forced `SIGKILL` of that group only if any
//! attempt-owned member survives it — followed by a verified final reap
//! and a bounded proof that no attempt-owned descendant remains. A
//! timed-out process can therefore never be silently reported as an
//! ordinary successful completion, and a timed-out attempt can never leave
//! orphaned descendants behind; the returned [`ProcessRunOutcome`] carries
//! the typed [`ProcessTermination`] classification.
//!
//! Frozen security properties of this slice:
//!
//! * **Absolute executable only.** The caller supplies an absolute path;
//!   there is no `PATH` lookup anywhere in this foundation, and
//!   unqualified names are rejected before the filesystem is touched.
//! * **Validated canonical executable.** The supplied path must resolve to
//!   an existing regular file carrying executable permission bits (on
//!   platforms exposing them), and it is the canonical (`realpath`) form —
//!   with symlink indirection removed — that the child actually executes.
//!   Recognized common shell basenames (`sh`, `bash`, `zsh`, `fish`,
//!   `dash`, `ksh`) are rejected both before and after resolution, so this
//!   runner cannot be used as a shell-command escape hatch such as
//!   `/bin/sh -c ...`. This is a bounded rejection list of common local
//!   shells, not a global shell database.
//! * **Realpath workspace boundary.** Both the workspace root and the
//!   requested working directory are canonicalized; membership requires
//!   component-wise containment of the resolved locations. A symlink
//!   leading outside the workspace or a `..` chain resolving past the root
//!   is refused; a sibling directory whose name merely shares a textual
//!   prefix with the root is not mistaken for containment. Any failed
//!   canonicalization fails closed.
//! * **Empty child environment.** Command construction starts from
//!   [`Command::env_clear`] and adds nothing back: the child inherits no
//!   parent variable — not `PATH`, `HOME`, credentials, proxies, or any
//!   other ambient state. Because the executable is an absolute validated
//!   path, no environment entry is required to locate it, and none is
//!   required by the spawn mechanism itself on supported platforms.
//! * **Non-interactive silent child.** stdin is null (the child observes
//!   immediate EOF), and stdout/stderr are null: this slice returns exit
//!   metadata only and deliberately implements no output capture.
//! * **Attempt-owned process group (timed path).** Each timed execution
//!   attempt is spawned as the leader of its own dedicated process group
//!   (`setpgid` semantics applied inside the child before `exec`, so
//!   descendants inherit membership). Timeout termination targets exactly
//!   that group and nothing else: pid 0, group 0, the `kill(-1)`
//!   broadcast, out-of-range identifiers, and the caller's own process
//!   group are all refused fail-closed before any signal is sent.
//!
//! The runner waits normally for the child. A child that exits non-zero is
//! a successful runner invocation reported as typed [`ProcessRunOutcome`]
//! metadata, never an error; [`ExecutionError`] is reserved for validation
//! failures plus the typed spawn/wait/control boundaries. Output capture,
//! digests, checkpoints, and recovery remain later runner slices.
//!
//! The workspace boundary provides workspace isolation only. It is NOT a
//! security sandbox: nothing here prevents a contained child from reaching
//! other filesystem locations, the network, other processes, or host
//! credentials. True isolation belongs to the runtime/host sandbox layer.

use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Child, ExitStatus};
use std::process::{Command, Stdio};
// Deadline arithmetic is platform-neutral: the typed overflow check is
// shared by every target so its fail-closed contract stays inspectable.
use std::time::{Duration, Instant};

use crate::execution::capture::{BoundedStreamRetention, CaptureFault};
#[cfg(unix)]
use crate::execution::capture::{CapturedProcessRun, CapturedStream, drain_to_eof};
use crate::execution::error::ExecutionError;
use crate::execution::outcome::ProcessRunOutcome;
#[cfg(unix)]
use crate::execution::outcome::ProcessTermination;
use crate::execution::request::ProcessRunRequest;
use crate::execution::timeout::ProcessTimeoutPolicy;
#[cfg(unix)]
use crate::execution::unix_signal::{
    GroupPresence, GroupQuiescence, GroupSignalDelivery, LeaderState, OwnedProcessGroup,
    deliver_group_sigcont, deliver_group_sigkill, deliver_group_sigstop, deliver_group_sigterm,
    group_presence, group_quiescence, observe_leader_without_reaping, timeout_platform_supported,
};

/// Common local shell basenames rejected at executable validation.
const SHELL_BASENAMES: [&str; 6] = ["sh", "bash", "zsh", "fish", "dash", "ksh"];

/// Private bounded-polling cadence for child-state observation. An
/// implementation detail of the timed path: monitoring sleeps at most this
/// long between observations and never beyond the remaining deadline, so
/// the runner never busy-spins and never overshoots a deadline by more
/// than one short poll.
#[cfg(unix)]
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Strictly bounded window for verifying, after a forced group `SIGKILL`,
/// that no attempt-owned member survives. Generous relative to SIGKILL
/// semantics, yet never unbounded: cleanup waits cannot extend forever.
#[cfg(unix)]
const GROUP_TEARDOWN_VERIFY_WINDOW: Duration = Duration::from_secs(5);

/// Strictly bounded window for best-effort direct-child reaping inside
/// typed-failure cleanup paths.
#[cfg(unix)]
const CLEANUP_REAP_WINDOW: Duration = Duration::from_secs(2);

/// Private fail-closed window for proving that post-grace `SIGSTOP` has
/// reached a stable process-group fixed point.
#[cfg(unix)]
const GROUP_QUIESCENCE_VERIFY_WINDOW: Duration = Duration::from_secs(1);

/// Bounded post-cleanup proof that capture readers reached EOF. Group
/// teardown closes every attempt-owned writer, so this is verification, not
/// a second user-visible run timeout.
#[cfg(unix)]
const CAPTURE_READER_VERIFY_WINDOW: Duration = Duration::from_secs(2);

#[cfg(all(test, unix))]
thread_local! {
    static CAPTURE_TEST_FAULTS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}

#[cfg(all(test, unix))]
thread_local! {
    static FORCE_UNSUPPORTED_TIMEOUT_PLATFORM: std::cell::Cell<bool> = const {
        std::cell::Cell::new(false)
    };
}

#[cfg(all(test, unix))]
pub(crate) struct UnsupportedTimeoutPlatformGuard(bool);

#[cfg(all(test, unix))]
impl Drop for UnsupportedTimeoutPlatformGuard {
    fn drop(&mut self) {
        FORCE_UNSUPPORTED_TIMEOUT_PLATFORM.set(self.0);
    }
}

#[cfg(all(test, unix))]
pub(crate) fn inject_unsupported_timeout_platform() -> UnsupportedTimeoutPlatformGuard {
    UnsupportedTimeoutPlatformGuard(FORCE_UNSUPPORTED_TIMEOUT_PLATFORM.replace(true))
}

#[cfg(unix)]
fn ensure_timeout_platform_supported() -> Result<(), ExecutionError> {
    #[cfg(test)]
    if FORCE_UNSUPPORTED_TIMEOUT_PLATFORM.get() {
        return Err(ExecutionError::UnsupportedTimeoutPlatform);
    }
    if timeout_platform_supported() {
        Ok(())
    } else {
        Err(ExecutionError::UnsupportedTimeoutPlatform)
    }
}

#[cfg(all(test, unix))]
pub(crate) const CAPTURE_TEST_FAIL_FORCE_KILL: u8 = 1;
#[cfg(all(test, unix))]
pub(crate) const CAPTURE_TEST_FAIL_WAIT: u8 = 2;
#[cfg(all(test, unix))]
pub(crate) const CAPTURE_TEST_FAIL_STDOUT_READ: u8 = 4;
#[cfg(all(test, unix))]
pub(crate) const CAPTURE_TEST_FAIL_SIGCONT: u8 = 8;
#[cfg(all(test, unix))]
pub(crate) const CAPTURE_TEST_FORCE_COMPLETION_BARRIER: u8 = 16;

#[cfg(all(test, unix))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TimeoutLifecycleEvent {
    GroupSigterm,
    GroupSigstop,
    GroupQuiescent,
    GroupSigcont,
    GroupSigkill,
    LeaderReaped,
    GroupEmptyVerified,
}

#[cfg(all(test, unix))]
thread_local! {
    static TIMEOUT_LIFECYCLE_EVENTS: std::cell::RefCell<Vec<TimeoutLifecycleEvent>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

#[cfg(all(test, unix))]
pub(crate) fn take_timeout_lifecycle_events() -> Vec<TimeoutLifecycleEvent> {
    TIMEOUT_LIFECYCLE_EVENTS.with(|events| std::mem::take(&mut *events.borrow_mut()))
}

#[cfg(all(test, unix))]
fn record_timeout_lifecycle_event(event: TimeoutLifecycleEvent) {
    TIMEOUT_LIFECYCLE_EVENTS.with(|events| events.borrow_mut().push(event));
}

/// Thread-local fault injection keeps parallel tests isolated and never
/// changes the public API or production process-control path.
#[cfg(all(test, unix))]
pub(crate) struct CaptureTestFaultGuard(u8);

#[cfg(all(test, unix))]
impl Drop for CaptureTestFaultGuard {
    fn drop(&mut self) {
        CAPTURE_TEST_FAULTS.set(self.0);
    }
}

#[cfg(all(test, unix))]
pub(crate) fn inject_capture_test_faults(faults: u8) -> CaptureTestFaultGuard {
    CaptureTestFaultGuard(CAPTURE_TEST_FAULTS.replace(faults))
}

/// Runs one validated process request to completion and returns its exit
/// metadata.
///
/// Validation order is fail-closed before any child exists: shell-basename
/// rejection, existence, canonicalization, regular-file and executable-
/// permission checks on the executable; then canonicalization and
/// directory/containment checks establishing the realpath workspace
/// boundary for the working directory. Only after every check passes does
/// construction of the child command begin, using the canonical executable
/// path and the canonical working directory.
pub fn run(request: &ProcessRunRequest) -> Result<ProcessRunOutcome, ExecutionError> {
    let executable = validated_executable(request.executable())?;
    let cwd = validated_workspace_cwd(request.workspace_root(), request.cwd())?;

    let mut command = prepared_command(&executable, &cwd);
    // Arguments travel verbatim as individual argv values; nothing joins,
    // splits, quotes, or interprets them because no shell exists here.
    command.args(request.arguments());

    let mut child = command.spawn().map_err(spawn_failed)?;
    let status = child.wait().map_err(wait_failed)?;

    Ok(ProcessRunOutcome::new(status.success(), status.code()))
}

/// Runs one validated process request under an explicitly supplied
/// orchestrator-owned timeout policy and returns typed lifecycle metadata.
///
/// The bounded path reuses exactly the same validation foundation as the
/// unbounded [`run`] — absolute canonical executable, common-shell
/// rejection, realpath workspace containment, empty child environment,
/// null stdio — so no less-safe duplicate launcher exists. The timed child
/// is additionally spawned as the leader of its own dedicated process
/// group (`setpgid` semantics applied inside the child before `exec`),
/// which the child's own descendants then inherit, making process-group
/// membership exactly the attempt-ownership boundary. The only other
/// difference is lifecycle control after spawn:
///
/// 1. monitor the child with the monotonic clock (`Instant`) using a
///    small bounded polling cadence that never sleeps past the deadline;
/// 2. if the child exits before the deadline, return an ordinary
///    [`ProcessTermination::Completed`] outcome — non-zero exits
///    included;
/// 3. at the deadline, observe once more so an already-finished child is
///    never needlessly terminated;
/// 4. deliver graceful termination (`SIGTERM`) to the attempt-owned
///    process group;
/// 5. wait at most the policy's termination grace, observing direct-child
///    exit without reaping its process-group leader and separately checking
///    for live descendants;
/// 6. if any attempt-owned member survives past grace, force kill the
///    owned process group (`SIGKILL`) while the leader remains unreaped;
///    only after all group signaling is finished, reap the direct child and
///    verify within a strictly bounded window that no member remains;
/// 7. classify truthfully: [`ProcessTermination::TimedOutForceKilled`]
///    whenever any attempt-owned member required the force step,
///    regardless of how the direct child itself exited.
///
/// A timed-out outcome always reports `success() == false` plus the exact
/// [`ProcessTermination`] classification, so a timed-out process can never
/// be silently reported as an ordinary successful completion. Every
/// control-boundary failure — ownership proof, graceful delivery, forced
/// kill, final reap — fails closed as its own typed error after bounded
/// best-effort cleanup; cleanup success never masks the original failure.
///
/// On platforms where graceful termination cannot be implemented with the
/// same guarantees, this fails closed with
/// [`ExecutionError::UnsupportedTimeoutPlatform`] before any child exists
/// rather than degrading graceful termination into an immediate force
/// kill.
#[cfg(unix)]
pub fn run_with_timeout(
    request: &ProcessRunRequest,
    policy: &ProcessTimeoutPolicy,
) -> Result<ProcessRunOutcome, ExecutionError> {
    ensure_timeout_platform_supported()?;

    // Capability and deadline arithmetic both fail closed before any
    // filesystem or process resource is touched.
    let deadline = validated_run_deadline(policy)?;

    let executable = validated_executable(request.executable())?;
    let cwd = validated_workspace_cwd(request.workspace_root(), request.cwd())?;

    let mut command = prepared_command(&executable, &cwd);
    // Attempt-owned process group: created by the platform exec wrapper
    // inside the child before ordinary execution begins, so every
    // descendant the child spawns inherits membership naturally. Value 0
    // means "the child leads a fresh group whose pgid equals its pid".
    command.process_group(0);
    command.args(request.arguments());

    let mut child = command.spawn().map_err(spawn_failed)?;

    // Fail-closed ownership proof: timeout enforcement may target only a
    // group the safety guards certify as this invocation's dedicated
    // group — never zero, one/broadcast, out-of-range values, or the
    // caller's own process group.
    let group = match OwnedProcessGroup::from_child_pid(child.id()) {
        Some(group) => group,
        None => return Err(process_group_ownership_failed(child)),
    };

    match await_child_until(&mut child, deadline)? {
        Awaited::Exited(status) => Ok(ProcessRunOutcome::new(status.success(), status.code())),
        Awaited::DeadlineReached => enforce_timeout(child, group, policy),
    }
}

/// Fails closed on platforms without the graceful-termination guarantee.
/// The policy type itself is platform-neutral (pure validated durations),
/// so this boundary is a compile-checked signature, not a behavior claim:
/// no child is ever spawned and no graceful-termination semantics are
/// pretended on unsupported platforms.
#[cfg(not(unix))]
pub fn run_with_timeout(
    request: &ProcessRunRequest,
    policy: &ProcessTimeoutPolicy,
) -> Result<ProcessRunOutcome, ExecutionError> {
    let _ = (request, policy);
    Err(ExecutionError::UnsupportedTimeoutPlatform)
}

/// Names of the two independently captured streams. Used only to keep the
/// failing pipe identifiable in typed errors; the two streams never share
/// buffers, counters, or limits.
#[cfg(unix)]
const STDOUT: &str = "stdout";
#[cfg(unix)]
const STDERR: &str = "stderr";

/// Runs one validated process request under an explicitly supplied
/// orchestrator-owned timeout policy, capturing stdout and stderr
/// separately under a bounded `HEAD_TAIL` retention contract.
///
/// This is the same bounded runner as [`run_with_timeout`] — identical
/// validation, identical dedicated-process-group ownership, identical
/// terminate → bounded-grace → force-kill → verify → reap lifecycle, all
/// reused rather than reimplemented — plus separate bounded capture of the
/// two child pipes. The unbounded [`run`] and the uncaptured
/// [`run_with_timeout`] keep their accepted null-stdio behavior untouched;
/// capture is opt-in through this API only.
///
/// Ordering, in full:
///
/// 1. verify timeout-platform capability and compute the monotonic deadline
///    (fails closed on unsupported targets or overflow);
/// 2. validate the request exactly as the uncaptured paths do;
/// 3. allocate both bounded retention buffers **before** the child exists,
///    so a machine that cannot honor the retention bound fails closed with
///    nothing spawned;
/// 4. prepare the child with stdin null and both output streams piped;
/// 5. spawn as the leader of its own dedicated process group;
/// 6. take both pipe handles immediately and start one dedicated reader
///    thread per stream, so the two pipes drain **concurrently** and
///    neither can block the child by filling while the other is read;
/// 7. preserve the unreaped direct child while waiting for both readers to
///    reach EOF under the original run deadline;
/// 8. after both readers finish, non-reapingly observe the leader and prove
///    the owned group has no live descendants under that same deadline;
/// 9. if any completion condition misses the deadline, run the accepted owned-group
///    timeout lifecycle, then verify reader completion within a private
///    bounded cleanup window;
/// 10. assemble the typed capture result.
///
/// Each reader keeps consuming past the retention limit through EOF: the
/// limit bounds retained memory, never bytes taken from the pipe. Bytes a
/// child emits during graceful termination, and up to a forced kill, are
/// ordinary stream bytes; capture is never frozen at the instant a timeout
/// is detected.
///
/// Failure precedence is deterministic. A timeout/process-control failure
/// is always the primary error — it is never hidden behind a secondary
/// capture failure. When process cleanup succeeded but a stream could not
/// be drained to EOF, the typed capture error is returned instead of an
/// `Ok` carrying incomplete metadata: `total_bytes` is never reported for a
/// stream that was not fully drained.
///
/// A child exiting non-zero is still an ordinary completed invocation: it
/// returns `Ok` with `success() == false` plus both captured streams.
#[cfg(unix)]
pub fn run_with_timeout_and_capture(
    request: &ProcessRunRequest,
    policy: &ProcessTimeoutPolicy,
) -> Result<CapturedProcessRun, ExecutionError> {
    ensure_timeout_platform_supported()?;

    let deadline = validated_run_deadline(policy)?;

    let executable = validated_executable(request.executable())?;
    let cwd = validated_workspace_cwd(request.workspace_root(), request.cwd())?;

    // Bounded retention is established before anything is spawned: if the
    // bound cannot be honored, no child ever exists to clean up.
    let stdout_retention = frozen_retention(STDOUT)?;
    let stderr_retention = frozen_retention(STDERR)?;

    let mut command = prepared_command(&executable, &cwd);
    // stdin stays null (immediate EOF); only the two output streams change
    // from the accepted null shape, and they stay separate pipes — stderr
    // is never redirected into stdout.
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.process_group(0);
    command.args(request.arguments());

    let mut child = command.spawn().map_err(spawn_failed)?;

    let group = match OwnedProcessGroup::from_child_pid(child.id()) {
        Some(group) => group,
        None => return Err(process_group_ownership_failed(child)),
    };

    // Both pipe handles are taken promptly, before any waiting happens.
    let Some(child_stdout) = child.stdout.take() else {
        return Err(capture_failure_after_spawn(
            child,
            group,
            ExecutionError::CaptureStreamUnavailable {
                stream: STDOUT,
                detail: "the spawned child exposed no stdout pipe".to_string(),
            },
        ));
    };
    let Some(child_stderr) = child.stderr.take() else {
        return Err(capture_failure_after_spawn(
            child,
            group,
            ExecutionError::CaptureStreamUnavailable {
                stream: STDERR,
                detail: "the spawned child exposed no stderr pipe".to_string(),
            },
        ));
    };

    let stdout_reader = match spawn_reader(STDOUT, child_stdout, stdout_retention) {
        Ok(handle) => handle,
        Err(error) => {
            return Err(capture_failure_after_spawn(
                child,
                group,
                ExecutionError::CaptureReaderStartFailed {
                    stream: STDOUT,
                    detail: error.to_string(),
                },
            ));
        }
    };
    let stderr_reader = match spawn_reader(STDERR, child_stderr, stderr_retention) {
        Ok(handle) => handle,
        Err(error) => {
            // The stdout reader owns the other pipe; group cleanup below
            // closes its writers, so it ends on its own and is detached
            // rather than joined while the primary failure is reported.
            drop(stdout_reader);
            return Err(capture_failure_after_spawn(
                child,
                group,
                ExecutionError::CaptureReaderStartFailed {
                    stream: STDERR,
                    detail: error.to_string(),
                },
            ));
        }
    };

    let mut stdout_reader = Some(stdout_reader);
    let mut stderr_reader = Some(stderr_reader);
    let mut stdout = None;
    let mut stderr = None;

    // Reader EOF is part of captured-run completion. Do not call try_wait
    // here: it reaps an exited leader and would leave only a stale numeric
    // PGID if an inherited writer kept either reader pending.
    loop {
        if stdout.is_none() {
            match take_finished_reader(STDOUT, &mut stdout_reader) {
                Ok(captured) => stdout = captured,
                Err(error) => {
                    return Err(capture_failure_after_spawn(child, group, error));
                }
            }
        }
        if stderr.is_none() {
            match take_finished_reader(STDERR, &mut stderr_reader) {
                Ok(captured) => stderr = captured,
                Err(error) => {
                    return Err(capture_failure_after_spawn(child, group, error));
                }
            }
        }
        if stdout.is_some() && stderr.is_some() {
            break;
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            let outcome = enforce_timeout(child, group, policy)?;
            let (stdout, stderr) = finish_readers_bounded(
                stdout_reader,
                stderr_reader,
                stdout,
                stderr,
                CAPTURE_READER_VERIFY_WINDOW,
            )?;
            return Ok(CapturedProcessRun::new(outcome, stdout, stderr));
        };
        if remaining.is_zero() {
            let outcome = enforce_timeout(child, group, policy)?;
            let (stdout, stderr) = finish_readers_bounded(
                stdout_reader,
                stderr_reader,
                stdout,
                stderr,
                CAPTURE_READER_VERIFY_WINDOW,
            )?;
            return Ok(CapturedProcessRun::new(outcome, stdout, stderr));
        }
        std::thread::sleep(POLL_INTERVAL.min(remaining));
    }

    // EOF alone is not child completion: a child may close both streams and
    // continue running. Observe it only now, under the original deadline.
    let outcome = match await_captured_child_until(&mut child, group, deadline) {
        Ok(Awaited::Exited(status)) => ProcessRunOutcome::new(status.success(), status.code()),
        Ok(Awaited::DeadlineReached) => enforce_timeout(child, group, policy)?,
        Err(error) => return Err(capture_failure_after_spawn(child, group, error)),
    };

    let stdout = stdout.expect("both captures were proven complete");
    let stderr = stderr.expect("both captures were proven complete");

    Ok(CapturedProcessRun::new(outcome, stdout, stderr))
}

/// Fails closed when bounded output capture is unavailable on this
/// platform, before any child exists.
#[cfg(not(unix))]
pub fn run_with_timeout_and_capture(
    request: &ProcessRunRequest,
    policy: &ProcessTimeoutPolicy,
) -> Result<crate::execution::capture::CapturedProcessRun, ExecutionError> {
    let _ = (request, policy);
    Err(ExecutionError::UnsupportedTimeoutPlatform)
}

/// Allocates one stream's frozen retention shape, mapping a reservation
/// failure into the typed fail-closed error.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn frozen_retention(
    stream: &'static str,
) -> Result<BoundedStreamRetention, ExecutionError> {
    BoundedStreamRetention::with_frozen_limits()
        .map_err(|error| retention_allocation_failed(stream, error))
}

/// Maps a retention reservation failure into its typed variant. Extracted
/// so the mapping can be exercised directly without exhausting real memory.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn retention_allocation_failed(
    stream: &'static str,
    error: std::collections::TryReserveError,
) -> ExecutionError {
    ExecutionError::CaptureRetentionAllocationFailed {
        detail: format!("{stream} retention buffers could not be reserved: {error}"),
    }
}

/// Maps one stream's drain fault into its typed, stream-identified error.
/// Stream identity is never collapsed away: a stdout failure and a stderr
/// failure stay distinguishable.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn capture_fault_failed(stream: &'static str, fault: CaptureFault) -> ExecutionError {
    match fault {
        CaptureFault::Read(error) => ExecutionError::CaptureReadFailed {
            stream,
            detail: error.to_string(),
        },
        CaptureFault::TotalByteOverflow { counted, chunk } => {
            ExecutionError::CaptureTotalByteOverflow {
                stream,
                detail: format!(
                    "{counted} bytes already counted plus a {chunk}-byte chunk exceeds what u64 \
                     can represent; refusing to wrap or saturate the reported total"
                ),
            }
        }
    }
}

/// One stream's dedicated reader thread result.
#[cfg(unix)]
type ReaderHandle = std::thread::JoinHandle<Result<BoundedStreamRetention, CaptureFault>>;

/// Starts one dedicated reader thread. Both streams get their own, so the
/// two pipes are drained concurrently and neither can deadlock the child by
/// filling while the other is being read.
#[cfg(unix)]
fn spawn_reader(
    stream: &'static str,
    source: impl std::io::Read + Send + 'static,
    mut retention: BoundedStreamRetention,
) -> std::io::Result<ReaderHandle> {
    #[cfg(test)]
    if stream == STDOUT && take_capture_test_fault(CAPTURE_TEST_FAIL_STDOUT_READ) {
        return std::thread::Builder::new()
            .name(format!("receipts-capture-{stream}"))
            .spawn(move || {
                drop((source, retention));
                Err(CaptureFault::Read(std::io::Error::other(
                    "injected stdout capture read failure",
                )))
            });
    }
    std::thread::Builder::new()
        .name(format!("receipts-capture-{stream}"))
        .spawn(move || drain_to_eof(source, &mut retention).map(|()| retention))
}

/// Joins one reader and converts its result into the immutable captured
/// stream. A drain failure or an abnormal reader end fails closed: no
/// `total_bytes` is ever reported for a stream that did not reach EOF.
#[cfg(unix)]
fn join_reader(
    stream: &'static str,
    handle: ReaderHandle,
) -> Result<CapturedStream, ExecutionError> {
    match handle.join() {
        Ok(Ok(retention)) => Ok(retention.finish()),
        Ok(Err(fault)) => Err(capture_fault_failed(stream, fault)),
        Err(panic) => Err(ExecutionError::CaptureReaderFailed {
            stream,
            detail: reader_panic_detail(panic.as_ref()),
        }),
    }
}

/// Renders whatever a panicking reader carried, without ever unwinding
/// again while assembling the failure.
#[cfg(unix)]
fn reader_panic_detail(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        format!("the reader thread panicked: {message}")
    } else if let Some(message) = payload.downcast_ref::<String>() {
        format!("the reader thread panicked: {message}")
    } else {
        "the reader thread panicked with a non-string payload".to_string()
    }
}

/// Takes and joins a reader only after `is_finished` proves joining cannot
/// block. `None` means the reader is still draining.
#[cfg(unix)]
fn take_finished_reader(
    stream: &'static str,
    handle: &mut Option<ReaderHandle>,
) -> Result<Option<CapturedStream>, ExecutionError> {
    if !handle.as_ref().is_some_and(ReaderHandle::is_finished) {
        return Ok(None);
    }
    join_reader(stream, handle.take().expect("finished reader is present")).map(Some)
}

/// After successful group cleanup, proves both readers reached EOF within a
/// private bounded verification window and joins only proven-finished
/// handles. No path can block indefinitely on `JoinHandle::join`.
#[cfg(unix)]
fn finish_readers_bounded(
    mut stdout_reader: Option<ReaderHandle>,
    mut stderr_reader: Option<ReaderHandle>,
    mut stdout: Option<CapturedStream>,
    mut stderr: Option<CapturedStream>,
    window: Duration,
) -> Result<(CapturedStream, CapturedStream), ExecutionError> {
    let deadline = checked_deadline(Instant::now(), window, "capture reader verification")?;
    loop {
        if stdout.is_none() {
            stdout = take_finished_reader(STDOUT, &mut stdout_reader)?;
        }
        if stderr.is_none() {
            stderr = take_finished_reader(STDERR, &mut stderr_reader)?;
        }
        if stdout.is_some() && stderr.is_some() {
            return Ok((
                stdout.take().expect("stdout capture is complete"),
                stderr.take().expect("stderr capture is complete"),
            ));
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Err(reader_completion_timed_out(
                if stdout.is_none() { STDOUT } else { STDERR },
                window,
            ));
        };
        if remaining.is_zero() {
            return Err(reader_completion_timed_out(
                if stdout.is_none() { STDOUT } else { STDERR },
                window,
            ));
        }
        std::thread::sleep(POLL_INTERVAL.min(remaining));
    }
}

#[cfg(unix)]
fn reader_completion_timed_out(stream: &'static str, window: Duration) -> ExecutionError {
    ExecutionError::CaptureReaderFailed {
        stream,
        detail: format!(
            "the reader did not reach EOF within the bounded {window:?} post-cleanup verification window"
        ),
    }
}

#[cfg(all(test, unix))]
pub(crate) fn bounded_reader_completion_failure_for_test(window: Duration) -> ExecutionError {
    let stdout_retention = frozen_retention(STDOUT).expect("test stdout retention");
    let stderr_retention = frozen_retention(STDERR).expect("test stderr retention");
    let (release, blocked) = std::sync::mpsc::sync_channel::<()>(0);
    let stdout_reader = std::thread::spawn(move || {
        let _ = blocked.recv();
        Ok(stdout_retention)
    });
    let stderr_reader = std::thread::spawn(move || Ok(stderr_retention));
    let error =
        finish_readers_bounded(Some(stdout_reader), Some(stderr_reader), None, None, window)
            .expect_err("blocked reader must exceed verification window");
    drop(release);
    error
}

/// Central failure precedence for every post-spawn capture/setup/observation
/// error: process containment failure wins; otherwise the original capture
/// error is returned unchanged.
#[cfg(unix)]
fn capture_failure_after_spawn(
    child: Child,
    group: OwnedProcessGroup,
    primary: ExecutionError,
) -> ExecutionError {
    match cleanup_owned_attempt(child, group) {
        Ok(()) => primary,
        Err(control_error) => control_error,
    }
}

/// Bounded, fail-closed cleanup for one still-owned captured attempt.
#[cfg(unix)]
fn cleanup_owned_attempt(mut child: Child, group: OwnedProcessGroup) -> Result<(), ExecutionError> {
    match capture_deliver_group_sigkill(group) {
        GroupSignalDelivery::Delivered | GroupSignalDelivery::GroupAlreadyGone => {}
        GroupSignalDelivery::Failed { detail } => {
            return Err(force_kill_failed_with_cleanup(child, group, detail));
        }
    }
    match bounded_child_reap(&mut child) {
        Ok(Some(_)) => {
            #[cfg(test)]
            record_timeout_lifecycle_event(TimeoutLifecycleEvent::LeaderReaped);
        }
        Ok(None) => {
            return Err(ExecutionError::TimeoutFinalWaitFailed {
                detail: "the direct child remained unreaped beyond the bounded capture-failure cleanup window".to_string(),
            });
        }
        Err(error) => return Err(final_wait_failed(error)),
    }
    if !await_group_empty(group, GROUP_TEARDOWN_VERIFY_WINDOW) {
        return Err(ExecutionError::ForceKillFailed {
            detail: format!(
                "the attempt-owned process group -{} was not proven empty after bounded capture-failure cleanup",
                group.raw()
            ),
        });
    }
    Ok(())
}

/// Internal result of monitoring a child up to a monotonic deadline.
#[cfg(unix)]
enum Awaited {
    /// The child exited on its own before the deadline.
    Exited(ExitStatus),
    /// The deadline expired with the child last observed still running.
    DeadlineReached,
}

/// Monitors `child` until it exits or `deadline` passes, whichever comes
/// first.
///
/// Polling cadence: observe child state, compute the remaining time from
/// the monotonic clock, sleep for the smaller of the private poll
/// interval and the remainder. No busy-spin; no overshoot beyond one short
/// poll. One final observation happens after the loop so a child that has
/// already completed at the boundary is not needlessly terminated.
#[cfg(unix)]
fn await_child_until(child: &mut Child, deadline: Instant) -> Result<Awaited, ExecutionError> {
    loop {
        if let Some(status) = child.try_wait().map_err(wait_failed)? {
            return Ok(Awaited::Exited(status));
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(POLL_INTERVAL.min(remaining));
    }

    if let Some(status) = child.try_wait().map_err(wait_failed)? {
        return Ok(Awaited::Exited(status));
    }
    Ok(Awaited::DeadlineReached)
}

/// Captured-path observation boundary with private deterministic fault
/// injection. Production delegates directly to the shared timed waiter.
#[cfg(unix)]
fn await_captured_child_until(
    child: &mut Child,
    group: OwnedProcessGroup,
    deadline: Instant,
) -> Result<Awaited, ExecutionError> {
    #[cfg(test)]
    if take_capture_test_fault(CAPTURE_TEST_FAIL_WAIT) {
        return Err(wait_failed(std::io::Error::other(
            "injected captured child observation failure",
        )));
    }
    let leader = child.id();
    loop {
        if observe_leader_without_reaping(leader).map_err(wait_failed)? == LeaderState::Exited {
            #[cfg(test)]
            let preliminary = if take_capture_test_fault(CAPTURE_TEST_FORCE_COMPLETION_BARRIER) {
                GroupQuiescence::Empty
            } else {
                group_quiescence(group, Some(leader))
            };
            #[cfg(not(test))]
            let preliminary = group_quiescence(group, Some(leader));
            match preliminary {
                GroupQuiescence::Empty => match quiesce_owned_group(group, leader) {
                    Ok(false) => {
                        if Instant::now() >= deadline {
                            return Ok(Awaited::DeadlineReached);
                        }
                        let status = child.wait().map_err(wait_failed)?;
                        #[cfg(test)]
                        record_timeout_lifecycle_event(TimeoutLifecycleEvent::LeaderReaped);
                        if !await_group_empty(group, GROUP_TEARDOWN_VERIFY_WINDOW) {
                            return Err(ExecutionError::ForceKillFailed {
                                detail: format!(
                                    "the attempt-owned process group -{} was not proven empty after captured completion",
                                    group.raw()
                                ),
                            });
                        }
                        #[cfg(test)]
                        record_timeout_lifecycle_event(TimeoutLifecycleEvent::GroupEmptyVerified);
                        return Ok(Awaited::Exited(status));
                    }
                    Ok(true) => match capture_deliver_group_sigcont(group) {
                        GroupSignalDelivery::Delivered | GroupSignalDelivery::GroupAlreadyGone => {}
                        GroupSignalDelivery::Failed { detail } => {
                            return Err(ExecutionError::ForceKillFailed { detail });
                        }
                    },
                    Err(detail) => return Err(ExecutionError::ForceKillFailed { detail }),
                },
                GroupQuiescence::Mutable | GroupQuiescence::AllStopped(_) => {}
                GroupQuiescence::Unknown { detail } => {
                    return Err(wait_failed(std::io::Error::other(detail)));
                }
            }
        }

        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(Awaited::DeadlineReached);
        };
        if remaining.is_zero() {
            return Ok(Awaited::DeadlineReached);
        }
        std::thread::sleep(POLL_INTERVAL.min(remaining));
    }
}

#[cfg(all(test, unix))]
fn take_capture_test_fault(fault: u8) -> bool {
    CAPTURE_TEST_FAULTS.with(|faults| {
        let current = faults.get();
        if current & fault == 0 {
            false
        } else {
            faults.set(current & !fault);
            true
        }
    })
}

#[cfg(unix)]
fn capture_deliver_group_sigkill(group: OwnedProcessGroup) -> GroupSignalDelivery {
    #[cfg(test)]
    if take_capture_test_fault(CAPTURE_TEST_FAIL_FORCE_KILL) {
        return GroupSignalDelivery::Failed {
            detail: "injected forced SIGKILL delivery failure".to_string(),
        };
    }
    timeout_deliver_group_sigkill(group)
}

#[cfg(unix)]
fn timeout_deliver_group_sigterm(group: OwnedProcessGroup) -> GroupSignalDelivery {
    #[cfg(test)]
    record_timeout_lifecycle_event(TimeoutLifecycleEvent::GroupSigterm);
    deliver_group_sigterm(group)
}

#[cfg(unix)]
fn timeout_deliver_group_sigstop(group: OwnedProcessGroup) -> GroupSignalDelivery {
    #[cfg(test)]
    record_timeout_lifecycle_event(TimeoutLifecycleEvent::GroupSigstop);
    deliver_group_sigstop(group)
}

#[cfg(unix)]
fn capture_deliver_group_sigcont(group: OwnedProcessGroup) -> GroupSignalDelivery {
    #[cfg(test)]
    record_timeout_lifecycle_event(TimeoutLifecycleEvent::GroupSigcont);
    #[cfg(test)]
    if take_capture_test_fault(CAPTURE_TEST_FAIL_SIGCONT) {
        return GroupSignalDelivery::Failed {
            detail: "injected completion-verification SIGCONT delivery failure".to_string(),
        };
    }
    deliver_group_sigcont(group)
}

#[cfg(unix)]
fn timeout_deliver_group_sigkill(group: OwnedProcessGroup) -> GroupSignalDelivery {
    #[cfg(test)]
    record_timeout_lifecycle_event(TimeoutLifecycleEvent::GroupSigkill);
    deliver_group_sigkill(group)
}

/// Proves both public policy intervals representable before any child is
/// spawned. The grace loop itself uses elapsed time, so no fallible deadline
/// arithmetic remains after ownership begins.
#[cfg(unix)]
fn validated_run_deadline(policy: &ProcessTimeoutPolicy) -> Result<Instant, ExecutionError> {
    let now = Instant::now();
    let deadline = checked_deadline(now, policy.run_timeout(), "run_timeout")?;
    checked_deadline(now, policy.termination_grace(), "termination_grace")?;
    Ok(deadline)
}

/// Stops group mutation and confirms the same stopped membership twice,
/// reaffirming `SIGSTOP` between observations. Once an observation contains
/// only stopped members, those members cannot fork; the reaffirmed signal
/// also catches a child created immediately before its parent stopped. The
/// matching second snapshot is therefore a stable ownership-release point.
#[cfg(unix)]
fn quiesce_owned_group(group: OwnedProcessGroup, leader: u32) -> Result<bool, String> {
    match timeout_deliver_group_sigstop(group) {
        GroupSignalDelivery::Delivered | GroupSignalDelivery::GroupAlreadyGone => {}
        GroupSignalDelivery::Failed { detail } => {
            if matches!(
                observe_leader_without_reaping(leader),
                Ok(LeaderState::Exited)
            ) && group_quiescence(group, Some(leader)) == GroupQuiescence::Empty
            {
                #[cfg(test)]
                record_timeout_lifecycle_event(TimeoutLifecycleEvent::GroupQuiescent);
                return Ok(false);
            }
            return Err(detail);
        }
    }

    let started = Instant::now();
    let mut previous = None;
    let mut last_issue = None;
    loop {
        let snapshot = match group_quiescence(group, Some(leader)) {
            GroupQuiescence::Empty => Some(Vec::new()),
            GroupQuiescence::AllStopped(pids) => Some(pids),
            GroupQuiescence::Mutable => {
                previous = None;
                last_issue = Some("a live group member had not stopped".to_string());
                None
            }
            GroupQuiescence::Unknown { detail } => {
                previous = None;
                last_issue = Some(detail);
                None
            }
        };

        if let Some(snapshot) = snapshot {
            if previous.as_ref() == Some(&snapshot) {
                #[cfg(test)]
                record_timeout_lifecycle_event(TimeoutLifecycleEvent::GroupQuiescent);
                return Ok(!snapshot.is_empty());
            }
            previous = Some(snapshot);
        }

        if started.elapsed() >= GROUP_QUIESCENCE_VERIFY_WINDOW {
            return Err(format!(
                "owned process group -{} did not reach a stable stopped state within {:?}: {}",
                group.raw(),
                GROUP_QUIESCENCE_VERIFY_WINDOW,
                last_issue.as_deref().unwrap_or("membership kept changing")
            ));
        }
        match timeout_deliver_group_sigstop(group) {
            GroupSignalDelivery::Delivered | GroupSignalDelivery::GroupAlreadyGone => {}
            GroupSignalDelivery::Failed { detail } => {
                if matches!(
                    observe_leader_without_reaping(leader),
                    Ok(LeaderState::Exited)
                ) && group_quiescence(group, Some(leader)) == GroupQuiescence::Empty
                {
                    #[cfg(test)]
                    record_timeout_lifecycle_event(TimeoutLifecycleEvent::GroupQuiescent);
                    return Ok(false);
                }
                return Err(detail);
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Implements the frozen terminate → bounded grace → force-kill → verify →
/// final-reap sequence for an attempt observed still running at the run
/// deadline. Termination targets the attempt-owned process group, so a
/// direct child that exits while descendants survive never classifies as
/// cleaned up.
#[cfg(unix)]
fn enforce_timeout(
    child: Child,
    group: OwnedProcessGroup,
    policy: &ProcessTimeoutPolicy,
) -> Result<ProcessRunOutcome, ExecutionError> {
    let leader = child.id();
    // Step 1 — graceful SIGTERM to the whole attempt-owned group.
    match timeout_deliver_group_sigterm(group) {
        GroupSignalDelivery::Delivered => {}
        // Unreachable while this handle's unreaped child zombie still
        // belongs to the group; falling through is truthful anyway — the
        // observation loop below reaps and re-checks membership.
        GroupSignalDelivery::GroupAlreadyGone => {}
        GroupSignalDelivery::Failed { detail } => {
            return Err(graceful_termination_failed_with_cleanup(
                child, group, detail,
            ));
        }
    }

    // Step 2 — the complete bounded grace opportunity. Elapsed-duration
    // accounting cannot overflow and performs no fallible arithmetic after
    // spawn. The leader remains unreaped throughout.
    let grace_started = Instant::now();
    loop {
        if let Err(error) = observe_leader_without_reaping(leader) {
            return Err(grace_wait_failed_with_cleanup(child, group, error));
        }
        let elapsed = grace_started.elapsed();
        if elapsed >= policy.termination_grace() {
            break;
        }
        std::thread::sleep(POLL_INTERVAL.min(policy.termination_grace().saturating_sub(elapsed)));
    }

    // Step 3 — freeze ordinary process-tree mutation before the final
    // descendant decision. No pre-quiescence process-table snapshot is
    // authoritative. Any barrier failure is force-cleaned while the leader
    // still anchors ownership.
    let leader_survived = match observe_leader_without_reaping(leader) {
        Ok(LeaderState::Running) => true,
        Ok(LeaderState::Exited) => false,
        Err(error) => return Err(grace_wait_failed_with_cleanup(child, group, error)),
    };
    let has_live_descendants = match quiesce_owned_group(group, leader) {
        Ok(has_live_descendants) => has_live_descendants,
        Err(detail) => return Err(quiescence_failed_with_cleanup(child, group, detail)),
    };
    if !leader_survived && !has_live_descendants {
        return finish_timeout_after_signaling(
            child,
            group,
            ProcessTermination::TimedOutGracefullyTerminated,
        );
    }

    // Step 4 — at least one quiesced attempt-owned process survived grace.
    // Kill the group while the unreaped leader still anchors ownership.
    let termination = match capture_deliver_group_sigkill(group) {
        GroupSignalDelivery::Delivered => ProcessTermination::TimedOutForceKilled,
        // Every member vanished between observation and delivery; nothing
        // actually required the force step. Verification and reap below
        // still run before any outcome is produced.
        GroupSignalDelivery::GroupAlreadyGone => ProcessTermination::TimedOutGracefullyTerminated,
        GroupSignalDelivery::Failed { detail } => {
            return Err(force_kill_failed_with_cleanup(child, group, detail));
        }
    };

    finish_timeout_after_signaling(child, group, termination)
}

/// Reaps only after the state machine has irrevocably finished group
/// signaling, then verifies emptiness without attempting stale-PGID repair.
#[cfg(unix)]
fn finish_timeout_after_signaling(
    mut child: Child,
    group: OwnedProcessGroup,
    termination: ProcessTermination,
) -> Result<ProcessRunOutcome, ExecutionError> {
    let status = child.wait().map_err(final_wait_failed)?;
    #[cfg(test)]
    record_timeout_lifecycle_event(TimeoutLifecycleEvent::LeaderReaped);
    if !await_group_empty(group, GROUP_TEARDOWN_VERIFY_WINDOW) {
        return Err(ExecutionError::ForceKillFailed {
            detail: format!(
                "the attempt-owned process group -{} still had members after the leader was reaped; ownership was released, so no further group signal was attempted",
                group.raw()
            ),
        });
    }
    #[cfg(test)]
    record_timeout_lifecycle_event(TimeoutLifecycleEvent::GroupEmptyVerified);
    Ok(timed_out_outcome(termination, status))
}

/// Polls the zero-signal presence probe until the owned group is empty or
/// the strictly bounded window expires. Returns whether emptiness was
/// proven — unknown states never count as success.
#[cfg(unix)]
fn await_group_empty(group: OwnedProcessGroup, window: Duration) -> bool {
    let Ok(deadline) = checked_deadline(Instant::now(), window, "cleanup verification") else {
        return false;
    };
    loop {
        if group_presence(group) == GroupPresence::Empty {
            return true;
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return false;
        };
        if remaining.is_zero() {
            return false;
        }
        std::thread::sleep(POLL_INTERVAL.min(remaining));
    }
}

/// Bounded best-effort reap of the direct child handle inside cleanup
/// paths. Never blocks indefinitely; returns the verified status when the
/// child exited within the window.
#[cfg(unix)]
fn bounded_child_reap(child: &mut Child) -> std::io::Result<Option<ExitStatus>> {
    let Ok(deadline) = checked_deadline(Instant::now(), CLEANUP_REAP_WINDOW, "cleanup reap") else {
        return Ok(None);
    };
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return Ok(None);
        };
        if remaining.is_zero() {
            return Ok(None);
        }
        std::thread::sleep(POLL_INTERVAL.min(remaining));
    }
}

/// Fails closed when the freshly spawned child does not certify as this
/// invocation's dedicated process-group owner. Direct-handle `kill`/`wait`
/// are used for cleanup because they need no group arithmetic, then the
/// typed ownership failure is returned.
#[cfg(unix)]
fn process_group_ownership_failed(mut child: Child) -> ExecutionError {
    let mut detail = format!(
        "spawned child pid {} did not yield a certifiably distinct attempt-owned process group",
        child.id()
    );
    match child.kill() {
        Ok(()) => match child.wait() {
            Ok(_) => {
                detail.push_str("; best-effort cleanup: direct child killed and reaped");
            }
            Err(reap_error) => {
                detail.push_str(&format!(
                    "; best-effort cleanup reap also failed: {reap_error}"
                ));
            }
        },
        Err(kill_error) => {
            detail.push_str(&format!(
                "; best-effort cleanup force kill also failed: {kill_error}"
            ));
        }
    }
    ExecutionError::ProcessGroupOwnershipFailed { detail }
}

/// Computes a monotonic deadline with checked arithmetic. If adding the
/// policy interval to the current monotonic reading cannot be represented,
/// there is no honest deadline and the run fails closed instead of
/// wrapping into a shorter or inverted effective policy.
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) fn checked_deadline(
    base: Instant,
    interval: Duration,
    label: &'static str,
) -> Result<Instant, ExecutionError> {
    base.checked_add(interval)
        .ok_or_else(|| ExecutionError::TimeoutDeadlineOverflow {
            detail: format!(
                "{label} interval {interval:?} added to the current monotonic reading cannot be \
                 represented; refusing to substitute a wrapped or silently different deadline"
            ),
        })
}

/// Best-effort orphan safety after graceful group delivery failed: the
/// original failure stays primary, but neither the attempt-owned group nor
/// the direct child is casually abandoned while bounded cleanup is still
/// possible. Cleanup evidence is appended to the reported detail and never
/// converts the outcome into success.
#[cfg(unix)]
fn graceful_termination_failed_with_cleanup(
    mut child: Child,
    group: OwnedProcessGroup,
    delivery_detail: String,
) -> ExecutionError {
    let mut detail = delivery_detail;
    match timeout_deliver_group_sigkill(group) {
        GroupSignalDelivery::Delivered => {
            detail.push_str(
                "; best-effort cleanup: forced SIGKILL delivered to the attempt-owned process \
                 group",
            );
        }
        GroupSignalDelivery::GroupAlreadyGone => {
            detail.push_str(
                "; best-effort cleanup: the attempt-owned process group was already empty",
            );
        }
        GroupSignalDelivery::Failed { detail: failed } => {
            detail.push_str(&format!(
                "; best-effort cleanup group SIGKILL also failed: {failed}"
            ));
        }
    }
    match bounded_child_reap(&mut child) {
        Ok(Some(_)) => {
            detail.push_str("; best-effort cleanup: the direct child was reaped");
        }
        Ok(None) => {
            detail.push_str(
                "; best-effort cleanup: the direct child remained unreaped within the bounded \
                 cleanup window",
            );
        }
        Err(reap_error) => {
            detail.push_str(&format!(
                "; best-effort cleanup reap also failed: {reap_error}"
            ));
        }
    }
    ExecutionError::GracefulTerminationFailed { detail }
}

/// Reports a failed quiescence proof only after bounded force cleanup while
/// the unreaped leader still anchors the group.
#[cfg(unix)]
fn quiescence_failed_with_cleanup(
    mut child: Child,
    group: OwnedProcessGroup,
    failure: String,
) -> ExecutionError {
    let mut detail = format!(
        "process-group quiescence failed for owned group -{}: {failure}",
        group.raw()
    );
    match timeout_deliver_group_sigkill(group) {
        GroupSignalDelivery::Delivered => {
            detail.push_str("; forced SIGKILL delivered before ownership release");
        }
        GroupSignalDelivery::GroupAlreadyGone => {
            detail.push_str("; the owned group was already empty");
        }
        GroupSignalDelivery::Failed { detail: failed } => {
            return force_kill_failed_with_cleanup(
                child,
                group,
                format!("{detail}; cleanup SIGKILL also failed: {failed}"),
            );
        }
    }
    match bounded_child_reap(&mut child) {
        Ok(Some(_)) => detail.push_str("; the direct child was reaped"),
        Ok(None) => detail
            .push_str("; the direct child remained unreaped within the bounded cleanup window"),
        Err(error) => detail.push_str(&format!("; cleanup reap also failed: {error}")),
    }
    if await_group_empty(group, GROUP_TEARDOWN_VERIFY_WINDOW) {
        detail.push_str("; the owned group was verified empty");
    } else {
        detail.push_str("; the owned group was not proven empty after bounded cleanup");
    }
    ExecutionError::ForceKillFailed { detail }
}

/// Typed force-kill failure after group `SIGKILL` delivery failed: the
/// original failure stays primary, one bounded best-effort retry plus a
/// direct-child reap are attempted for orphan safety, and presence
/// evidence is preserved. Never converts into a successful timeout
/// outcome.
#[cfg(unix)]
pub(crate) fn force_kill_failed_with_cleanup(
    mut child: Child,
    group: OwnedProcessGroup,
    delivery_detail: String,
) -> ExecutionError {
    let mut detail = format!(
        "SIGKILL delivery to the attempt-owned process group -{} failed: {delivery_detail}",
        group.raw()
    );
    match timeout_deliver_group_sigkill(group) {
        GroupSignalDelivery::Delivered => {
            detail.push_str("; best-effort cleanup: retry SIGKILL reached the owned group");
        }
        GroupSignalDelivery::GroupAlreadyGone => {
            detail.push_str("; best-effort cleanup: the owned group was already empty");
        }
        GroupSignalDelivery::Failed { detail: failed } => {
            detail.push_str(&format!(
                "; best-effort cleanup retry also failed: {failed}"
            ));
        }
    }
    match bounded_child_reap(&mut child) {
        Ok(Some(_)) => {
            detail.push_str("; best-effort cleanup: the direct child was reaped");
        }
        Ok(None) => {
            detail.push_str(
                "; best-effort cleanup: the direct child remained unreaped within the bounded \
                 cleanup window",
            );
        }
        Err(reap_error) => {
            detail.push_str(&format!(
                "; best-effort cleanup reap also failed: {reap_error}"
            ));
        }
    }
    match group_presence(group) {
        GroupPresence::HasMembers => {
            detail.push_str("; follow-up evidence: the owned group still has members");
        }
        GroupPresence::Empty => {
            detail.push_str("; follow-up evidence: the owned group observed empty after failure");
        }
        GroupPresence::Unknown { detail: unknown } => {
            detail.push_str(&format!("; follow-up evidence: {unknown}"));
        }
    }
    ExecutionError::ForceKillFailed { detail }
}

/// Maps an observation failure inside the bounded termination-grace window
/// into its typed error while attempting bounded best-effort cleanup of a
/// potentially live attempt: forced SIGKILL to the owned group, then a
/// bounded direct-child reap. The original grace-wait error is preserved
/// as the primary failure — it is never hidden or replaced, and no
/// successful timeout metadata can ever be produced on this path.
#[cfg(unix)]
pub(crate) fn grace_wait_failed_with_cleanup(
    mut child: Child,
    group: OwnedProcessGroup,
    observe_error: std::io::Error,
) -> ExecutionError {
    let mut detail = format!("grace-window observation failed: {observe_error}");
    match timeout_deliver_group_sigkill(group) {
        GroupSignalDelivery::Delivered => {
            detail.push_str(
                "; best-effort cleanup: forced SIGKILL delivered to the attempt-owned process \
                 group",
            );
        }
        GroupSignalDelivery::GroupAlreadyGone => {
            detail.push_str(
                "; best-effort cleanup: the attempt-owned process group was already empty",
            );
        }
        GroupSignalDelivery::Failed { detail: failed } => {
            detail.push_str(&format!(
                "; best-effort cleanup group SIGKILL also failed: {failed}"
            ));
        }
    }
    match bounded_child_reap(&mut child) {
        Ok(Some(_)) => {
            detail.push_str("; best-effort cleanup: the direct child was reaped");
        }
        Ok(None) => {
            detail.push_str(
                "; best-effort cleanup: the direct child remained unreaped within the bounded \
                 cleanup window",
            );
        }
        Err(reap_error) => {
            detail.push_str(&format!(
                "; best-effort cleanup reap also failed: {reap_error}"
            ));
        }
    }
    ExecutionError::TimeoutGraceWaitFailed { detail }
}

/// Maps a failure to reap the child after forced kill into its typed
/// error, kept distinct from kill-delivery failures so neither boundary is
/// silently ignored.
#[cfg(unix)]
pub(crate) fn final_wait_failed(error: std::io::Error) -> ExecutionError {
    ExecutionError::TimeoutFinalWaitFailed {
        detail: error.to_string(),
    }
}

/// Assembles the typed outcome for one timed-out run from the reaped
/// child's exit status.
#[cfg(unix)]
fn timed_out_outcome(termination: ProcessTermination, status: ExitStatus) -> ProcessRunOutcome {
    ProcessRunOutcome::new_timed_out(termination, status.code())
}

/// Maps an `std::io::Error` from [`Command::spawn`] into the typed
/// spawn-failure variant. Extracted so tests can exercise the mapping
/// directly; see the spawn-failure test for why kernel-side argv overflow
/// is the only race-free way to reach this boundary once pre-validation
/// has closed every validation-shaped failure mode.
pub(crate) fn spawn_failed(error: std::io::Error) -> ExecutionError {
    ExecutionError::ProcessSpawnFailed {
        detail: error.to_string(),
    }
}

/// Maps an `std::io::Error` from [`std::process::Child::wait`] into the
/// typed wait-failure variant, keeping the spawn and wait boundaries
/// distinct.
pub(crate) fn wait_failed(error: std::io::Error) -> ExecutionError {
    ExecutionError::ProcessWaitFailed {
        detail: error.to_string(),
    }
}

/// Validates `supplied` as the explicit absolute executable and returns its
/// canonical form: the exact path handed to [`Command::new`].
///
/// Flow: reject recognized shell basenames lexically, require existence,
/// canonicalize (`realpath`), re-reject shells on the resolved name (which
/// catches symlinks aliased onto a shell), then require a regular file
/// carrying executable permission bits where the platform exposes them.
pub(crate) fn validated_executable(supplied: &Path) -> Result<PathBuf, ExecutionError> {
    reject_shell_basename(supplied)?;

    if let Err(error) = std::fs::metadata(supplied) {
        return Err(if error.kind() == ErrorKind::NotFound {
            ExecutionError::ExecutableNotFound {
                detail: format!("{}: {error}", supplied.display()),
            }
        } else {
            ExecutionError::ExecutableUnresolvable {
                detail: format!("{} could not be inspected: {error}", supplied.display()),
            }
        });
    }

    let canonical = std::fs::canonicalize(supplied).map_err(|error| {
        ExecutionError::ExecutableUnresolvable {
            detail: format!("{}: {error}", supplied.display()),
        }
    })?;

    reject_shell_basename(&canonical)?;

    let metadata =
        std::fs::metadata(&canonical).map_err(|error| ExecutionError::ExecutableUnresolvable {
            detail: format!("{}: {error}", canonical.display()),
        })?;
    if !metadata.is_file() {
        return Err(ExecutionError::ExecutableNotRegularFile {
            detail: format!("{} is not a regular file", canonical.display()),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(ExecutionError::ExecutableNotExecutable {
                detail: format!(
                    "{} carries none of the owner/group/other execute bits",
                    canonical.display()
                ),
            });
        }
    }
    Ok(canonical)
}

fn reject_shell_basename(path: &Path) -> Result<(), ExecutionError> {
    let Some(name) = path.file_name() else {
        return Ok(());
    };
    let name = name.to_string_lossy().to_lowercase();
    if SHELL_BASENAMES.contains(&name.as_str()) {
        Err(ExecutionError::ShellExecutableRejected { name })
    } else {
        Ok(())
    }
}

/// Establishes the realpath workspace boundary for the requested child
/// working directory and returns its canonical form.
///
/// Both paths are canonicalized independently (any failure fails closed),
/// the canonical working directory must be a directory, and membership is
/// decided with component-wise path semantics on the canonical forms:
/// equal to the root or a descendant of it. String-prefix similarity never
/// counts as containment.
pub(crate) fn validated_workspace_cwd(
    workspace_root: &Path,
    requested: &Path,
) -> Result<PathBuf, ExecutionError> {
    let canonical_workspace_root = std::fs::canonicalize(workspace_root).map_err(|error| {
        ExecutionError::WorkspaceRootUnresolvable {
            detail: format!("{}: {error}", workspace_root.display()),
        }
    })?;
    let canonical_cwd =
        std::fs::canonicalize(requested).map_err(|error| ExecutionError::CwdUnresolvable {
            detail: format!("{}: {error}", requested.display()),
        })?;
    if !canonical_cwd.is_dir() {
        return Err(ExecutionError::CwdNotADirectory {
            detail: format!("{} is not a directory", canonical_cwd.display()),
        });
    }
    // `Path::starts_with` compares whole components, so a sibling whose
    // name shares a textual prefix with the root is correctly excluded,
    // while equality and descendant relationships pass.
    if !canonical_cwd.starts_with(&canonical_workspace_root) {
        return Err(ExecutionError::CwdOutsideWorkspace {
            requested: requested.display().to_string(),
            canonical_cwd: canonical_cwd.display().to_string(),
            canonical_workspace_root: canonical_workspace_root.display().to_string(),
        });
    }
    Ok(canonical_cwd)
}

/// Builds the exact production child [`Command`] shape for this foundation.
///
/// The program is the already-validated canonical absolute executable, the
/// working directory is the already-validated canonical location inside the
/// workspace, the environment is cleared and left empty, and all three
/// standard streams are detached: stdin null (immediate EOF for the child),
/// stdout and stderr null (no capture machinery exists in this slice).
/// Nothing here spawns; tests may inspect the configured command.
pub(crate) fn prepared_command(executable: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(executable);
    // Closed minimal environment policy for this foundation: the allowlist
    // is intentionally EMPTY. The child inherits nothing from the parent —
    // not PATH, HOME, USER, SHELL, credential, proxy, or vendor variables.
    // The executable is absolute, so locating it requires no PATH, and the
    // spawn mechanism itself requires no environment entries.
    command.env_clear();
    command.current_dir(cwd);
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command
}
