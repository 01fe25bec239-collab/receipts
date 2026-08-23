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
//! termination (`SIGTERM` semantics), a bounded termination grace, then a
//! forced kill only if the child is still running, followed by a verified
//! final reap. A timed-out process can therefore never be silently
//! reported as an ordinary successful completion; the returned
//! [`ProcessRunOutcome`] carries the typed
//! [`ProcessTermination`] classification.
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
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Child, ExitStatus};
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::time::{Duration, Instant};

use crate::execution::error::ExecutionError;
use crate::execution::outcome::ProcessRunOutcome;
#[cfg(unix)]
use crate::execution::outcome::ProcessTermination;
use crate::execution::request::ProcessRunRequest;
#[cfg(unix)]
use crate::execution::timeout::ProcessTimeoutPolicy;
#[cfg(unix)]
use crate::execution::unix_signal::{SignalDelivery, deliver_sigterm};

/// Common local shell basenames rejected at executable validation.
const SHELL_BASENAMES: [&str; 6] = ["sh", "bash", "zsh", "fish", "dash", "ksh"];

/// Private bounded-polling cadence for child-state observation. An
/// implementation detail of the timed path: monitoring sleeps at most this
/// long between observations and never beyond the remaining deadline, so
/// the runner never busy-spins and never overshoots a deadline by more
/// than one short poll.
#[cfg(unix)]
const POLL_INTERVAL: Duration = Duration::from_millis(10);

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
/// null stdio — so no less-safe duplicate launcher exists. The only
/// difference is lifecycle control after spawn:
///
/// 1. monitor the child with the monotonic clock (`Instant`) using a
///    small bounded polling cadence that never sleeps past the deadline;
/// 2. if the child exits before the deadline, return an ordinary
///    [`ProcessTermination::Completed`] outcome — non-zero exits
///    included;
/// 3. at the deadline, observe once more so an already-finished child is
///    never needlessly terminated;
/// 4. deliver graceful termination (`SIGTERM` semantics on Unix);
/// 5. wait at most the policy's termination grace for the child to exit,
///    reaping it if it does;
/// 6. force kill only a child still running after grace, then reap it.
///
/// A timed-out outcome always reports `success() == false` plus the exact
/// [`ProcessTermination`] classification, so a timed-out process can never
/// be silently reported as an ordinary successful completion. Every
/// control-boundary failure — graceful delivery, forced kill, final reap —
/// fails closed as its own typed error after best-effort cleanup; cleanup
/// success never masks the original failure.
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
    command.args(request.arguments());

    let mut child = command.spawn().map_err(spawn_failed)?;

    match await_child_until(&mut child, deadline)? {
        Awaited::Exited(status) => Ok(ProcessRunOutcome::new(status.success(), status.code())),
        Awaited::DeadlineReached => enforce_timeout(child, policy),
    }
}

/// Fails closed on platforms without the graceful-termination guarantee.
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

/// Implements the frozen terminate → bounded grace → force-kill → final-
/// reap sequence for a child observed still running at the run deadline.
#[cfg(unix)]
fn enforce_timeout(
    mut child: Child,
    policy: &ProcessTimeoutPolicy,
) -> Result<ProcessRunOutcome, ExecutionError> {
    match deliver_sigterm(child.id()) {
        SignalDelivery::Delivered => {}
        SignalDelivery::AlreadyExited => {
            // The child vanished between the last observation and signal
            // delivery. Reap and classify truthfully: the deadline did
            // expire, but no force kill was ever needed.
            let status = child.wait().map_err(final_wait_failed)?;
            return Ok(timed_out_outcome(
                ProcessTermination::TimedOutGracefullyTerminated,
                status,
            ));
        }
        SignalDelivery::Failed { detail } => {
            return Err(graceful_termination_failed_with_cleanup(child, detail));
        }
    }

    let grace_deadline = checked_deadline(
        Instant::now(),
        policy.termination_grace(),
        "termination_grace",
    )?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Ok(timed_out_outcome(
                    ProcessTermination::TimedOutGracefullyTerminated,
                    status,
                ));
            }
            Ok(None) => {}
            Err(error) => return Err(grace_wait_failed(error)),
        }
        let Some(remaining) = grace_deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(POLL_INTERVAL.min(remaining));
    }

    // Final state check before forcing: never force-kill a child that has
    // already exited during the grace window.
    match child.try_wait() {
        Ok(Some(status)) => {
            return Ok(timed_out_outcome(
                ProcessTermination::TimedOutGracefullyTerminated,
                status,
            ));
        }
        Ok(None) => {}
        Err(error) => return Err(grace_wait_failed(error)),
    }

    // Force kill. On Unix this is SIGKILL semantics against the direct
    // runner-owned child.
    if let Err(kill_error) = child.kill() {
        // Distinguish a genuine kill-delivery failure from a child that
        // raced past this exact window and exited on its own. Only a
        // verified exit status may be reported as an outcome; anything
        // else fails closed.
        match child.try_wait() {
            Ok(Some(status)) => {
                // Verified: the child exited concurrently after graceful
                // termination had been delivered — no force kill was
                // actually required.
                return Ok(timed_out_outcome(
                    ProcessTermination::TimedOutGracefullyTerminated,
                    status,
                ));
            }
            Ok(None) => return Err(force_kill_failed(kill_error, None)),
            Err(observe_error) => return Err(force_kill_failed(kill_error, Some(observe_error))),
        }
    }

    // Verified final reap: the timed-out API must not report success while
    // the runner-owned child remains unwaited.
    let status = child.wait().map_err(final_wait_failed)?;
    Ok(timed_out_outcome(
        ProcessTermination::TimedOutForceKilled,
        status,
    ))
}

/// Computes a monotonic deadline with checked arithmetic. If adding the
/// policy interval to the current monotonic reading cannot be represented,
/// there is no honest deadline and the run fails closed instead of
/// wrapping into a shorter or inverted effective policy.
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

/// Best-effort orphan safety after graceful-termination delivery failed:
/// the original failure stays primary, but the runner-owned child is not
/// casually abandoned while cleanup is still possible. Cleanup evidence is
/// appended to the reported detail and never converts the outcome into
/// success.
#[cfg(unix)]
fn graceful_termination_failed_with_cleanup(
    mut child: Child,
    delivery_detail: String,
) -> ExecutionError {
    let mut detail = delivery_detail;
    match child.try_wait() {
        Ok(Some(_)) => {
            detail.push_str("; best-effort cleanup: the child had already exited and was reaped");
        }
        Ok(None) => match child.kill() {
            Ok(()) => match child.wait() {
                Ok(_) => {
                    detail.push_str("; best-effort cleanup: forced kill and reap succeeded");
                }
                Err(reap_error) => detail.push_str(&format!(
                    "; best-effort cleanup reap also failed: {reap_error}"
                )),
            },
            Err(kill_error) => detail.push_str(&format!(
                "; best-effort cleanup force kill also failed: {kill_error}"
            )),
        },
        Err(observe_error) => detail.push_str(&format!(
            "; best-effort cleanup observation also failed: {observe_error}"
        )),
    }
    ExecutionError::GracefulTerminationFailed { detail }
}

/// Maps a forced-kill delivery failure into its typed error, preserving
/// any follow-up observation failure so neither boundary is silently
/// ignored. The child, if any, is left to the caller's knowledge: no
/// verified exit status existed at this boundary.
#[cfg(unix)]
pub(crate) fn force_kill_failed(
    kill_error: std::io::Error,
    observe_error: Option<std::io::Error>,
) -> ExecutionError {
    let mut detail = format!("SIGKILL delivery to the runner-owned child failed: {kill_error}");
    if let Some(observe_error) = observe_error {
        detail.push_str(&format!(
            "; follow-up state observation also failed: {observe_error}"
        ));
    } else {
        detail.push_str("; follow-up state observation found the child still unwaited");
    }
    ExecutionError::ForceKillFailed { detail }
}

/// Maps an observation failure inside the bounded termination-grace window
/// into its typed error — a materially different lifecycle boundary from
/// the plain pre-deadline wait, because graceful termination has already
/// been delivered when this can occur.
#[cfg(unix)]
pub(crate) fn grace_wait_failed(error: std::io::Error) -> ExecutionError {
    ExecutionError::TimeoutGraceWaitFailed {
        detail: error.to_string(),
    }
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
