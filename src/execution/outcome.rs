//! Typed exit-status metadata returned by one completed process run.

/// Exit-status metadata for one completed child process.
///
/// This is the entire result payload of this foundation slice: a success
/// flag and, when the platform reported one, the child's numeric exit
/// code. A non-zero child exit is normal runner output carried here — it
/// is never converted into an
/// [`ExecutionError`](crate::execution::ExecutionError),
/// which is reserved for validation and spawn/wait failures.
///
/// `exit_code` is `None` when the child terminated without a normal
/// numeric exit code on the platform (for example termination by signal).
///
/// No stdout/stderr bytes exist anywhere in this type: output capture
/// belongs to later runner slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessRunOutcome {
    success: bool,
    exit_code: Option<i32>,
}

impl ProcessRunOutcome {
    /// Assembles outcome metadata from the completed child's exit status.
    pub(crate) fn new(success: bool, exit_code: Option<i32>) -> Self {
        Self { success, exit_code }
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
