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

use crate::execution::error::ExecutionError;
use crate::execution::outcome::ProcessRunOutcome;
#[cfg(unix)]
use crate::execution::outcome::ProcessTermination;
use crate::execution::request::ProcessRunRequest;
use crate::execution::timeout::ProcessTimeoutPolicy;
#[cfg(unix)]
use crate::execution::unix_signal::{
    GroupPresence, GroupSignalDelivery, OwnedProcessGroup, deliver_group_sigkill,
    deliver_group_sigterm, group_presence,
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
/// 5. wait at most the policy's termination grace for BOTH the direct
///    child to be reaped AND the owned group to become empty — a direct
///    child exit alone never proves cleanup while descendants survive;
/// 6. if any attempt-owned member survives past grace, force kill the
///    owned process group (`SIGKILL`), reap the direct child, and verify
///    within a strictly bounded window that no member remains;
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
    // Deadline arithmetic happens first: it is pure, cheap, and fails
    // closed before any filesystem or process resource is touched.
    let deadline = checked_deadline(Instant::now(), policy.run_timeout(), "run_timeout")?;

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

/// Implements the frozen terminate → bounded grace → force-kill → verify →
/// final-reap sequence for an attempt observed still running at the run
/// deadline. Termination targets the attempt-owned process group, so a
/// direct child that exits while descendants survive never classifies as
/// cleaned up.
#[cfg(unix)]
fn enforce_timeout(
    mut child: Child,
    group: OwnedProcessGroup,
    policy: &ProcessTimeoutPolicy,
) -> Result<ProcessRunOutcome, ExecutionError> {
    // Step 1 — graceful SIGTERM to the whole attempt-owned group.
    match deliver_group_sigterm(group) {
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

    // Step 2 — bounded termination grace: complete only when BOTH the
    // direct child has been reaped AND the attempt-owned group is empty.
    let grace_deadline = checked_deadline(
        Instant::now(),
        policy.termination_grace(),
        "termination_grace",
    )?;
    let mut reaped_status: Option<ExitStatus> = None;
    loop {
        if reaped_status.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => reaped_status = Some(status),
                Ok(None) => {}
                Err(error) => return Err(grace_wait_failed_with_cleanup(child, group, error)),
            }
        }
        if let Some(status) = reaped_status
            && group_presence(group) == GroupPresence::Empty
        {
            // Every attempt-owned member terminated during the
            // graceful interval; nothing required forcing.
            return Ok(timed_out_outcome(
                ProcessTermination::TimedOutGracefullyTerminated,
                status,
            ));
        }
        let Some(remaining) = grace_deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(POLL_INTERVAL.min(remaining));
    }

    // Step 3 — final cleanup-state observation before any force decision:
    // never force when the whole attempt-owned group already terminated.
    if reaped_status.is_none() {
        match child.try_wait() {
            Ok(Some(status)) => reaped_status = Some(status),
            Ok(None) => {}
            Err(error) => return Err(grace_wait_failed_with_cleanup(child, group, error)),
        }
    }
    if group_presence(group) == GroupPresence::Empty {
        // The group emptied at the very end of grace. The direct child's
        // zombie would keep the group occupied, so an unreaped handle here
        // is contradictory; reap it and classify truthfully — no member
        // ever required SIGKILL.
        let status = match reaped_status {
            Some(status) => status,
            None => child.wait().map_err(final_wait_failed)?,
        };
        return Ok(timed_out_outcome(
            ProcessTermination::TimedOutGracefullyTerminated,
            status,
        ));
    }

    // Step 4 — forced kill of the OWNED GROUP (never the caller's group):
    // some attempt-owned member survived graceful termination.
    let forced_kill_delivered = match deliver_group_sigkill(group) {
        GroupSignalDelivery::Delivered => true,
        // Every member vanished between observation and delivery; nothing
        // actually required the force step. Verification and reap below
        // still run before any outcome is produced.
        GroupSignalDelivery::GroupAlreadyGone => false,
        GroupSignalDelivery::Failed { detail } => {
            return Err(force_kill_failed_with_cleanup(child, group, detail));
        }
    };

    // Step 5 — reap the direct child first: its zombie membership would
    // otherwise keep the owned group occupied for verification.
    let status = child.wait().map_err(final_wait_failed)?;

    // Step 6 — bounded verification that no attempt-owned descendant
    // remains. Only after this proof may a timed-out outcome be returned.
    if !await_group_empty(group, GROUP_TEARDOWN_VERIFY_WINDOW) {
        return Err(ExecutionError::ForceKillFailed {
            detail: format!(
                "the attempt-owned process group -{} kept surviving members beyond the bounded \
                 teardown verification window after forced SIGKILL; the direct child was reaped, \
                 but timeout ownership cannot be reported clean",
                group.raw()
            ),
        });
    }

    // Step 7 — classification is group-aware: any member requiring the
    // force step yields TimedOutForceKilled even when the direct child
    // itself exited gracefully.
    Ok(timed_out_outcome(
        if forced_kill_delivered {
            ProcessTermination::TimedOutForceKilled
        } else {
            ProcessTermination::TimedOutGracefullyTerminated
        },
        status,
    ))
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
    match deliver_group_sigkill(group) {
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
    match deliver_group_sigkill(group) {
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
    match deliver_group_sigkill(group) {
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
