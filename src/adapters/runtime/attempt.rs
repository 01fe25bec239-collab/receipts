//! Runtime-owned identity and Codex attempt composition for later trait binding.
//! These types neither dispatch work nor grant admission, routing or credential authority.

use std::{error::Error, fmt};

use receipts_workspace_execution::execution::{
    LiveProcessCancelAcceptance, LiveProcessTerminalCause,
};

use crate::{CodexLiveAttempt, CodexLiveOutcome, CodexLiveOutputSnapshot, RawFailure};

/// Caller-supplied Runtime attempt identity, not a provider session or process ID.
/// Frozen A3Handoff, ReviewRequest, ReviewCapsule, WorkspaceCheckpoint and
/// SafetyInterruption schemas agree on 1..=200 Unicode characters. No trimming,
/// normalization, generation, uniqueness claim or execution authorization occurs.
/// Retention is at most 800 UTF-8 bytes; Debug deliberately hides the value.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct AttemptId(Box<str>);

impl AttemptId {
    /// Validate the frozen character bound before allocating. Invalid input is
    /// rejected without retaining or formatting it; caller bytes otherwise survive.
    pub fn new(value: &str) -> Result<Self, AttemptIdError> {
        if !(1..=200).contains(&value.chars().take(201).count()) {
            return Err(AttemptIdError);
        }
        Ok(Self(value.into()))
    }

    /// Explicit access to the exact caller value; does not prove global uniqueness.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AttemptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AttemptId([redacted])")
    }
}

/// Payload-free failure of the Runtime identity length check; no authority granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttemptIdError;

impl fmt::Display for AttemptIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("attempt identity must contain 1 through 200 Unicode characters")
    }
}
impl Error for AttemptIdError {}

/// One non-cloneable Runtime handle composing an already-started Codex attempt.
/// Workspace remains the sole lifecycle owner, including cancellation, timeout,
/// single collection and Drop cleanup. Additional storage is one bounded identity.
/// No provider payload is formatted, and no policy or cancellation reason is invented.
///
/// ```compile_fail
/// use receipts_runtime_adapters::AttemptHandle;
/// fn duplicate(handle: &AttemptHandle) -> AttemptHandle { (*handle).clone() }
/// ```
pub struct AttemptHandle {
    id: AttemptId,
    codex: CodexLiveAttempt,
}

impl AttemptHandle {
    /// Attach caller identity to an existing post-admission live attempt, moving
    /// ownership without spawning anything. The caller owns identity assignment.
    pub fn new(id: AttemptId, codex: CodexLiveAttempt) -> Self {
        Self { id, codex }
    }

    /// Bounded identity supplied at composition, unchanged throughout the lifecycle.
    pub fn id(&self) -> &AttemptId {
        &self.id
    }

    /// Delegate a bounded point-in-time observation; never an asynchronous stream.
    /// No history accumulates. Workspace errors are projected without payloads.
    pub fn snapshot_output(&self) -> Result<CodexLiveOutputSnapshot, RawFailure> {
        self.codex
            .snapshot_output()
            .map_err(|e| RawFailure::from(&e))
    }

    /// Delegate the cancellation request; only Workspace decides acceptance.
    /// This exposes mechanics and does not define the deferred CancelReason binding.
    pub fn cancel(&self) -> LiveProcessCancelAcceptance {
        self.codex.cancel()
    }

    /// Observe Workspace's winning cause, not proof of cleanup or task acceptance.
    pub fn terminal_cause(&self) -> Option<LiveProcessTerminalCause> {
        self.codex.terminal_cause()
    }

    /// Collect once via Workspace, retaining identity and all bounded Codex evidence.
    /// Collection failures remain typed and consumed; no retry or terminal decision
    /// is made here. Protocol errors remain independent on the successful outcome.
    pub fn collect_result(&self) -> Result<AttemptResult, RawFailure> {
        let codex = self
            .codex
            .wait_collect()
            .map_err(|e| RawFailure::from(&e))?;
        Ok(AttemptResult {
            id: self.id.clone(),
            codex,
        })
    }
}

/// Runtime result of one collected Codex attempt, never a Receipts task verdict.
/// Moves the accepted Codex outcome without duplicating output. It retains separate
/// Workspace lifecycle, forced-kill, exit-status, bounded stdout/stderr and truncation
/// facts. Protocol interpretation uses the existing parser and may independently fail.
/// No Debug/Display payload surface, acceptance inference or admission authority exists.
/// Storage is the existing live-outcome bound plus at most 800 identity bytes.
pub struct AttemptResult {
    id: AttemptId,
    codex: CodexLiveOutcome,
}

impl AttemptResult {
    /// Exact identity of the handle that collected this result.
    pub fn id(&self) -> &AttemptId {
        &self.id
    }

    /// Existing, unmodified Runtime evidence. `process()` exposes Workspace truth;
    /// `protocol()` interprets stdout only, preserving unknown ordered records or a
    /// payload-free parse error. Neither layer supplies a Receipts semantic verdict.
    pub fn codex(&self) -> &CodexLiveOutcome {
        &self.codex
    }
}
