//! Vendor-neutral classification of collected Workspace lifecycle evidence.
//! Provider output, prose and exit codes never select a class here; see
//! docs/runtime-adapters/failure-classes.md for every class's evidence basis.

use receipts_workspace_execution::execution::{LiveProcessOutcome, LiveProcessTerminalCause};

use crate::{CancelReason, FailureClass};

/// `accepted_cancel` is the reason of the caller cancellation that Workspace
/// accepted for this attempt, if any. `None` means no process failure was
/// observed, never task success. Cancellation without an accepted
/// `UserRequested` reason stays Unknown.
pub fn classify_live_outcome(
    outcome: &LiveProcessOutcome,
    accepted_cancel: Option<CancelReason>,
) -> Option<FailureClass> {
    match outcome.terminal_cause() {
        LiveProcessTerminalCause::TimedOut => Some(FailureClass::Timeout),
        LiveProcessTerminalCause::Cancelled => Some(match accepted_cancel {
            Some(CancelReason::UserRequested) => FailureClass::UserCancelled,
            None => FailureClass::Unknown,
        }),
        LiveProcessTerminalCause::Completed => {
            (!outcome.success()).then_some(FailureClass::Unknown)
        }
    }
}
