//! Typed exit-status metadata returned by one completed process run.

/// How one run's child process actually ended.
///
/// This closed enum is the typed lifecycle classification callers rely on:
/// a timed-out run can never be mistaken for an ordinary completion, and a
/// forced kill can never be mistaken for graceful termination, without
/// interpreting any error string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessTermination {
    /// The child exited on its own before the run deadline expired.
    ///
    /// A non-zero exit is still an ordinary completion — it is normal
    /// runner output, not an error and not a timeout.
    Completed,
    /// The run deadline expired; the attempt-owned process group was asked
    /// to terminate gracefully (`SIGTERM`) and every member — the direct
    /// child and any descendants it spawned — exited within the bounded
    /// termination grace. No force kill was required.
    TimedOutGracefullyTerminated,
    /// The run deadline expired; at least one attempt-owned process — the
    /// direct child or a descendant — was still alive after the bounded
    /// termination grace expired and the whole owned process group had to
    /// be force-killed.
    TimedOutForceKilled,
}

impl ProcessTermination {
    /// Whether this classification means the run deadline expired.
    pub fn is_timed_out(self) -> bool {
        matches!(
            self,
            ProcessTermination::TimedOutGracefullyTerminated
                | ProcessTermination::TimedOutForceKilled
        )
    }

    /// Whether ending this run required a forced kill after the bounded
    /// grace expired.
    pub fn is_forced_kill(self) -> bool {
        matches!(self, ProcessTermination::TimedOutForceKilled)
    }
}

/// Exit-status metadata for one completed child process.
///
/// The result payload of the runner: how the process ended (as
/// [`ProcessTermination`]), a success flag and, when the platform reported
/// one, the child's numeric exit code. A non-zero child exit under ordinary
/// completion is normal runner output carried here — it is never converted
/// into an [`ExecutionError`](crate::execution::ExecutionError), which is
/// reserved for validation and spawn/wait/control failures.
///
/// `success()` is `true` only for an ordinary pre-deadline completion that
/// the platform reports as successful. A timed-out run is never silently
/// reported as successful: both timed-out classifications carry
/// `success() == false` regardless of any exit code the child produced on
/// its way down, so `success() == true` always implies
/// `termination() == ProcessTermination::Completed`.
///
/// `exit_code` is `None` when the child terminated without a normal
/// numeric exit code on the platform — in particular when terminated by a
/// signal, which is the usual shape of both timeout classifications.
///
/// No stdout/stderr bytes exist anywhere in this type: output capture
/// belongs to later runner slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessRunOutcome {
    termination: ProcessTermination,
    success: bool,
    exit_code: Option<i32>,
}

impl ProcessRunOutcome {
    /// Assembles outcome metadata from the completed child's exit status.
    ///
    /// Produces the ordinary [`ProcessTermination::Completed`]
    /// classification used by the unbounded [`run`](crate::run) API.
    pub(crate) fn new(success: bool, exit_code: Option<i32>) -> Self {
        Self {
            termination: ProcessTermination::Completed,
            success,
            exit_code,
        }
    }

    /// Assembles outcome metadata for one timed-out run from the reaped
    /// child's exit status.
    ///
    /// `success` is forced to `false`: a run that had to be terminated by
    /// policy is never reported as an ordinary successful completion.
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn new_timed_out(termination: ProcessTermination, exit_code: Option<i32>) -> Self {
        debug_assert!(termination.is_timed_out());
        Self {
            termination,
            success: false,
            exit_code,
        }
    }

    /// How the child process actually ended.
    pub fn termination(&self) -> ProcessTermination {
        self.termination
    }

    /// Whether the run deadline expired and termination control was used.
    pub fn timed_out(&self) -> bool {
        self.termination.is_timed_out()
    }

    /// Whether ending this run required a forced kill after the bounded
    /// grace expired.
    pub fn forced_kill_required(&self) -> bool {
        self.termination.is_forced_kill()
    }

    /// Whether the child exited successfully.
    pub fn success(&self) -> bool {
        self.success
    }

    /// The child's numeric exit code, if the platform reported one.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
}
