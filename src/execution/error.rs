//! Fail-closed error surface for the argv-only local process-runner
//! foundation.
//!
//! Every failure mode of validating a run request or executing one child
//! process surfaces as an explicit [`ExecutionError`]. A child that exits
//! non-zero is deliberately NOT an error: it is a successful runner
//! invocation reported as typed exit metadata.

use std::fmt;

/// Errors produced by the local process-runner foundation.
///
/// Validation errors are raised before any child exists; execution errors
/// distinguish the spawn boundary from the wait boundary so callers can
/// tell "the program never started" from "the program started but waiting
/// for it failed".
#[derive(Debug)]
#[non_exhaustive]
pub enum ExecutionError {
    /// The supplied executable path was not syntactically absolute.
    ///
    /// This runner never performs `PATH` lookups, so unqualified names
    /// such as `git` or `python` are rejected before anything touches the
    /// filesystem.
    ExecutablePathNotAbsolute {
        /// The rejected value, preserved exactly for diagnostics.
        value: String,
    },
    /// The supplied workspace root was not syntactically absolute.
    WorkspaceRootNotAbsolute {
        /// The rejected value, preserved exactly for diagnostics.
        value: String,
    },
    /// The requested child working directory was not syntactically
    /// absolute.
    CwdNotAbsolute {
        /// The rejected value, preserved exactly for diagnostics.
        value: String,
    },
    /// The supplied executable path does not exist on the filesystem.
    ExecutableNotFound {
        /// The rejected path and the underlying filesystem failure.
        detail: String,
    },
    /// The executable path could not be resolved to its canonical
    /// (realpath) form, so no validated absolute executable exists.
    ExecutableUnresolvable {
        /// The rejected path and the underlying resolution failure.
        detail: String,
    },
    /// The resolved executable exists but is not a regular file (for
    /// example a directory or a device).
    ExecutableNotRegularFile {
        /// The rejected path and why it is not a regular file.
        detail: String,
    },
    /// The resolved executable is a regular file but carries no
    /// executable permission bits (platforms that expose permission
    /// semantics only).
    ExecutableNotExecutable {
        /// The rejected path and the observed permission state.
        detail: String,
    },
    /// The executable's basename is a recognized common shell interpreter.
    ///
    /// This is a process runner, not a shell runner: rejecting common
    /// shells at validation prevents the foundation from becoming a shell
    /// command-string escape hatch such as `/bin/sh -c ...`.
    ShellExecutableRejected {
        /// The rejected basename.
        name: String,
    },
    /// The workspace root could not be canonicalized to its realpath form,
    /// so the containment boundary cannot be established. Fails closed.
    WorkspaceRootUnresolvable {
        /// The rejected path and the underlying resolution failure.
        detail: String,
    },
    /// The requested child working directory could not be canonicalized to
    /// its realpath form (nonexistent path, dangling link). Fails closed.
    CwdUnresolvable {
        /// The rejected path and the underlying resolution failure.
        detail: String,
    },
    /// The canonicalized requested working directory is not a directory.
    CwdNotADirectory {
        /// The rejected path and what was observed instead.
        detail: String,
    },
    /// The canonicalized requested working directory lies outside the
    /// canonicalized workspace root — including symlink escapes, `..`
    /// chains that resolve past the root, and sibling directories whose
    /// names merely share a textual prefix with the root.
    CwdOutsideWorkspace {
        /// The originally requested working-directory path.
        requested: String,
        /// Where the requested path actually resolves on the filesystem.
        canonical_cwd: String,
        /// The canonical workspace root membership was checked against.
        canonical_workspace_root: String,
    },
    /// The child process could not be spawned.
    ///
    /// Distinct from [`ExecutionError::ProcessWaitFailed`]: the process
    /// never ran, so no exit status exists.
    ProcessSpawnFailed {
        /// Underlying spawn failure detail.
        detail: String,
    },
    /// The child was spawned successfully but waiting for it to complete
    /// failed.
    ProcessWaitFailed {
        /// Underlying wait failure detail.
        detail: String,
    },
    /// A timeout policy value could not satisfy the frozen bounded-run
    /// sequence (for example a zero run timeout or a zero termination
    /// grace). Refused at construction; never silently clamped.
    InvalidTimeoutPolicy {
        /// The rejected policy value and why it cannot be honored.
        detail: String,
    },
    /// Adding a policy interval to the monotonic clock reading cannot be
    /// represented, so no honest deadline exists. Fails closed instead of
    /// wrapping or silently changing the effective policy.
    TimeoutDeadlineOverflow {
        /// Which deadline overflowed and by what interval.
        detail: String,
    },
    /// Bounded execution is not supported on this platform with the same
    /// frozen guarantees. Fails closed before any child exists rather than
    /// silently degrading graceful termination into an immediate force
    /// kill.
    UnsupportedTimeoutPlatform,
    /// Graceful (`SIGTERM`) termination of a timed-out child could not be
    /// delivered through the runner-owned termination boundary.
    ///
    /// Distinct from [`ExecutionError::ForceKillFailed`]: this failure
    /// happened at the graceful step, before any force-kill decision.
    /// Best-effort cleanup evidence is preserved in the detail without
    /// ever converting the outcome into success.
    GracefulTerminationFailed {
        /// Underlying delivery failure plus best-effort cleanup evidence.
        detail: String,
    },
    /// Observing the child during the bounded termination-grace window
    /// failed after graceful termination had already been delivered.
    ///
    /// Distinct from [`ExecutionError::ProcessWaitFailed`]: the child has
    /// been signaled and the lifecycle decision now depends on this
    /// observation.
    TimeoutGraceWaitFailed {
        /// Underlying observation failure detail.
        detail: String,
    },
    /// Forced `SIGKILL` delivery or force-containment of an owned process
    /// group failed. Never reported as successful cleanup or timeout.
    ForceKillFailed {
        /// Underlying delivery failure plus best-effort cleanup evidence.
        detail: String,
    },
    /// The bounded final direct-child reap during timeout or post-spawn
    /// cleanup failed, so no verified final state exists. Fails closed
    /// rather than claiming the runner-owned child was reaped.
    TimeoutFinalWaitFailed {
        /// Underlying final-wait failure detail.
        detail: String,
    },
    /// The spawned attempt could not be given a dedicated, safely
    /// signalable process group owned by this invocation, so the frozen
    /// no-orphan timeout contract could not be guaranteed. Fails closed:
    /// the just-spawned child receives best-effort direct-handle cleanup
    /// and the run reports this error instead of proceeding unowned.
    ProcessGroupOwnershipFailed {
        /// Why ownership could not be established plus best-effort cleanup
        /// evidence.
        detail: String,
    },
    /// Control or observation of an owned process group, or proof of its
    /// containment state, failed and no more specific signal, child-wait,
    /// or ownership error truthfully describes the failed operation.
    ProcessGroupControlFailed {
        /// The failed group-control, state-observation, or containment-proof
        /// operation and its underlying evidence.
        detail: String,
    },
    /// Bounded output retention could not be established, so the frozen
    /// capture bound could not be honored. Raised before any child is
    /// spawned: capture buffers are allocated fallibly up front precisely
    /// so this failure never leaves a live attempt behind.
    CaptureRetentionAllocationFailed {
        /// Which retention buffer could not be reserved and why.
        detail: String,
    },
    /// A captured child was spawned but the expected pipe for one stream
    /// was not present, so that stream could never be drained to EOF.
    ///
    /// Fails closed rather than reporting a complete `total_bytes` for a
    /// stream that was never read.
    CaptureStreamUnavailable {
        /// `"stdout"` or `"stderr"` — never collapsed into one opaque
        /// stream identity, so the failing pipe stays diagnosable.
        stream: &'static str,
        /// Best-effort cleanup evidence for the just-spawned attempt.
        detail: String,
    },
    /// The dedicated reader for one stream could not be started, so
    /// concurrent draining of both pipes could not be guaranteed.
    CaptureReaderStartFailed {
        /// `"stdout"` or `"stderr"`.
        stream: &'static str,
        /// Underlying thread-start failure plus cleanup evidence.
        detail: String,
    },
    /// Reading one stream's pipe failed before EOF was reached.
    CaptureReadFailed {
        /// `"stdout"` or `"stderr"`.
        stream: &'static str,
        /// Underlying read failure detail.
        detail: String,
    },
    /// One stream's dedicated reader ended abnormally (panic or an
    /// unobservable join result), so no verified drained state exists.
    CaptureReaderFailed {
        /// `"stdout"` or `"stderr"`.
        stream: &'static str,
        /// What was observed at the join boundary.
        detail: String,
    },
    /// Counting the bytes drained from one stream would overflow `u64`.
    ///
    /// Refused outright: a wrapped or saturated total would silently
    /// misreport how much the child actually produced.
    CaptureTotalByteOverflow {
        /// `"stdout"` or `"stderr"`.
        stream: &'static str,
        /// The counted total and the chunk that could not be added.
        detail: String,
    },
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionError::ExecutablePathNotAbsolute { value } => write!(
                f,
                "executable path {value:?} is not absolute; this runner requires one explicit absolute executable and performs no PATH lookup"
            ),
            ExecutionError::WorkspaceRootNotAbsolute { value } => write!(
                f,
                "workspace root {value:?} is not absolute; supply an explicit absolute workspace root"
            ),
            ExecutionError::CwdNotAbsolute { value } => write!(
                f,
                "requested working directory {value:?} is not absolute; supply an explicit absolute working directory"
            ),
            ExecutionError::ExecutableNotFound { detail } => {
                write!(f, "executable does not exist: {detail}")
            }
            ExecutionError::ExecutableUnresolvable { detail } => write!(
                f,
                "executable path could not be resolved to its canonical location: {detail}"
            ),
            ExecutionError::ExecutableNotRegularFile { detail } => {
                write!(f, "executable is not a regular file: {detail}")
            }
            ExecutionError::ExecutableNotExecutable { detail } => write!(
                f,
                "executable is a regular file without executable permission bits: {detail}"
            ),
            ExecutionError::ShellExecutableRejected { name } => write!(
                f,
                "executable {name:?} is a recognized shell interpreter; this foundation runs programs through argv, not shells"
            ),
            ExecutionError::WorkspaceRootUnresolvable { detail } => write!(
                f,
                "workspace root could not be resolved to its canonical location: {detail}"
            ),
            ExecutionError::CwdUnresolvable { detail } => write!(
                f,
                "requested working directory could not be resolved to its canonical location: {detail}"
            ),
            ExecutionError::CwdNotADirectory { detail } => {
                write!(
                    f,
                    "requested working directory is not a directory: {detail}"
                )
            }
            ExecutionError::CwdOutsideWorkspace {
                requested,
                canonical_cwd,
                canonical_workspace_root,
            } => write!(
                f,
                "requested working directory {requested:?} resolves to {canonical_cwd:?}, which is outside the workspace root {canonical_workspace_root:?}"
            ),
            ExecutionError::ProcessSpawnFailed { detail } => {
                write!(f, "failed to spawn child process: {detail}")
            }
            ExecutionError::ProcessWaitFailed { detail } => {
                write!(f, "failed while waiting for child process: {detail}")
            }
            ExecutionError::InvalidTimeoutPolicy { detail } => {
                write!(f, "invalid timeout policy: {detail}")
            }
            ExecutionError::TimeoutDeadlineOverflow { detail } => write!(
                f,
                "timeout deadline cannot be represented on the monotonic clock: {detail}"
            ),
            ExecutionError::UnsupportedTimeoutPlatform => write!(
                f,
                "bounded execution with graceful termination is not supported on this platform; \
                 refusing to degrade graceful termination into an immediate force kill"
            ),
            ExecutionError::GracefulTerminationFailed { detail } => {
                write!(
                    f,
                    "graceful termination of the timed-out child failed: {detail}"
                )
            }
            ExecutionError::TimeoutGraceWaitFailed { detail } => write!(
                f,
                "failed while observing the child during the bounded termination grace window: \
                 {detail}"
            ),
            ExecutionError::ForceKillFailed { detail } => {
                write!(
                    f,
                    "forced process-group kill or containment failed: {detail}"
                )
            }
            ExecutionError::TimeoutFinalWaitFailed { detail } => write!(
                f,
                "bounded final direct-child reap failed during timeout or post-spawn cleanup: {detail}"
            ),
            ExecutionError::ProcessGroupOwnershipFailed { detail } => write!(
                f,
                "the spawned attempt could not be given a dedicated, safely signalable process \
                 group owned by this invocation: {detail}"
            ),
            ExecutionError::ProcessGroupControlFailed { detail } => write!(
                f,
                "process-group control, state observation, or containment proof failed: {detail}"
            ),
            ExecutionError::CaptureRetentionAllocationFailed { detail } => write!(
                f,
                "bounded output retention could not be allocated, so the frozen capture bound \
                 could not be honored: {detail}"
            ),
            ExecutionError::CaptureStreamUnavailable { stream, detail } => write!(
                f,
                "the captured child exposed no {stream} pipe, so {stream} could not be drained to \
                 EOF: {detail}"
            ),
            ExecutionError::CaptureReaderStartFailed { stream, detail } => write!(
                f,
                "the dedicated {stream} reader could not be started, so both pipes could not be \
                 drained concurrently: {detail}"
            ),
            ExecutionError::CaptureReadFailed { stream, detail } => {
                write!(f, "reading the child's {stream} pipe failed: {detail}")
            }
            ExecutionError::CaptureReaderFailed { stream, detail } => write!(
                f,
                "the dedicated {stream} reader ended abnormally, so no verified drained state \
                 exists: {detail}"
            ),
            ExecutionError::CaptureTotalByteOverflow { stream, detail } => write!(
                f,
                "counting the bytes drained from {stream} would overflow; refusing to report a \
                 wrapped or saturated total: {detail}"
            ),
        }
    }
}

impl std::error::Error for ExecutionError {}
