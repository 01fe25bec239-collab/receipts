//! Evidence-bound WorkspaceHandle lifecycle transitions (WE-W1-3).
//!
//! Every transition here is checked by
//! [`WorkspaceState::may_transition_to`] and requires evidence this crate
//! observed itself: an attempt actually started in the handle's worktree, a
//! checkpoint actually captured from it, or a collected attempt failure whose
//! process cleanup is unknown. A state string, a decoded wire record or a
//! caller-built checkpoint value cannot move a handle. Recovery execution
//! (`ORPHANED` → `RECOVERED`) belongs to a later wave and has no public
//! producer here.
//!
//! Lifecycle state is not a protection claim: an `ACTIVE` handle started with
//! [`start_workspace_attempt`] runs under the ordinary argv-only runner, which
//! is NOT a sandbox. Protected launches live in [`crate::protected`].

use std::fmt;

use crate::execution::{
    ExecutionError, LiveProcessAttempt, LiveProcessAttemptError, LiveProcessStartError,
    ProcessRunRequest, ProcessTimeoutPolicy, start_live_process_attempt,
};
use crate::{
    WorkspaceCheckpointEvidenceCaptureError, WorkspaceCheckpointEvidenceCaptureRequest,
    WorkspaceCheckpointExecutedCheckCore, WorkspaceCheckpointKind, WorkspaceCheckpointRef,
    WorkspaceCheckpointTemporalCaptureCore, WorkspaceDateTimeV1, WorkspaceError, WorkspaceHandle,
    WorkspaceState, capture_workspace_checkpoint_temporal_evidence,
};

/// A lifecycle operation was refused or its evidence could not be produced.
#[derive(Debug)]
#[non_exhaustive]
pub enum WorkspaceLifecycleError {
    /// The checked oracle refuses this transition.
    Transition(WorkspaceError),
    /// The attempt's workspace root is not the handle's canonical worktree.
    AttemptOutsideWorkspace,
    /// The attempt did not start; the handle is unchanged.
    Start(LiveProcessStartError),
    /// Capture failed; the handle is unchanged.
    Capture(WorkspaceCheckpointEvidenceCaptureError),
    /// The supplied failure proves known cleanup, so it is not orphan evidence.
    NotOrphanEvidence,
}

impl fmt::Display for WorkspaceLifecycleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transition(error) => error.fmt(f),
            Self::AttemptOutsideWorkspace => {
                f.write_str("attempt workspace root is not the handle's worktree")
            }
            Self::Start(error) => write!(f, "attempt start failed: {error}"),
            Self::Capture(error) => write!(f, "checkpoint capture failed: {error}"),
            Self::NotOrphanEvidence => {
                f.write_str("attempt failure does not show unknown process cleanup")
            }
        }
    }
}

impl std::error::Error for WorkspaceLifecycleError {}

fn checked(handle: &WorkspaceHandle, to: WorkspaceState) -> Result<(), WorkspaceLifecycleError> {
    if handle.state().may_transition_to(to) {
        Ok(())
    } else {
        Err(WorkspaceLifecycleError::Transition(
            WorkspaceError::IllegalStateTransition {
                from: handle.state().as_str(),
                to: to.as_str(),
            },
        ))
    }
}

/// Starts one live attempt whose workspace root is exactly the handle's
/// canonical worktree and returns the `ACTIVE` handle only after the start
/// was observed. On any refusal the input handle is unchanged and no attempt
/// exists.
pub fn start_workspace_attempt(
    handle: &WorkspaceHandle,
    request: &ProcessRunRequest,
    policy: &ProcessTimeoutPolicy,
) -> Result<(WorkspaceHandle, WorkspaceAttempt), WorkspaceLifecycleError> {
    checked(handle, WorkspaceState::Active)?;
    let canonical = |path: &std::path::Path| std::fs::canonicalize(path).ok();
    match (
        canonical(request.workspace_root()),
        canonical(handle.worktree_path()),
    ) {
        (Some(root), Some(worktree)) if root == worktree => {}
        _ => return Err(WorkspaceLifecycleError::AttemptOutsideWorkspace),
    }
    let attempt =
        start_live_process_attempt(request, policy).map_err(WorkspaceLifecycleError::Start)?;
    let active = handle
        .transitioned(WorkspaceState::Active, None)
        .expect("transition was checked before start");
    Ok((active.clone(), WorkspaceAttempt { active, attempt }))
}

/// Caller-supplied inert checkpoint metadata; Git state is always observed.
#[derive(Debug)]
pub struct WorkspaceCheckpointLifecycleInput {
    pub checkpoint_id: String,
    pub attempt_id: Option<String>,
    pub kind: WorkspaceCheckpointKind,
    pub dirty_diff_ref: Option<WorkspaceCheckpointRef>,
    pub executed_checks: Vec<WorkspaceCheckpointExecutedCheckCore>,
    pub captured_at: WorkspaceDateTimeV1,
}

/// Captures a checkpoint from the `ACTIVE` handle's own worktree, binding the
/// handle's workspace/task/base identity, and returns the `CHECKPOINTED`
/// handle whose head is the captured head. Capture failure leaves the handle
/// unchanged.
pub fn checkpoint_workspace(
    handle: &WorkspaceHandle,
    input: WorkspaceCheckpointLifecycleInput,
) -> Result<(WorkspaceHandle, WorkspaceCheckpointTemporalCaptureCore), WorkspaceLifecycleError> {
    checked(handle, WorkspaceState::Checkpointed)?;
    let request = WorkspaceCheckpointEvidenceCaptureRequest {
        directory: handle.worktree_path(),
        checkpoint_id: input.checkpoint_id,
        workspace_id: handle.workspace_id().to_owned(),
        task_id: handle.task_id().map(str::to_owned),
        attempt_id: input.attempt_id,
        kind: input.kind,
        base_sha: Some(handle.base_sha().as_str()),
        dirty_diff_ref: input.dirty_diff_ref,
        executed_checks: input.executed_checks,
    };
    let checkpoint = capture_workspace_checkpoint_temporal_evidence(request, input.captured_at)
        .map_err(WorkspaceLifecycleError::Capture)?;
    let next = handle
        .transitioned(
            WorkspaceState::Checkpointed,
            Some(checkpoint.core().head_sha().clone()),
        )
        .expect("transition was checked before capture");
    Ok((next, checkpoint))
}

/// Owns the attempt and its observed ACTIVE handle. Collection cannot be bypassed.
#[derive(Debug)]
pub struct WorkspaceAttempt {
    active: WorkspaceHandle,
    attempt: LiveProcessAttempt,
}

/// An actual owning collection failure. Fields are private; diagnostics alone
/// cannot construct lifecycle authority.
#[derive(Debug)]
pub struct WorkspaceCollectionFailure {
    error: LiveProcessAttemptError,
    orphan: Option<Box<WorkspaceHandle>>,
}
impl WorkspaceCollectionFailure {
    pub fn error(&self) -> &LiveProcessAttemptError {
        &self.error
    }
}
impl fmt::Display for WorkspaceCollectionFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for WorkspaceCollectionFailure {}

impl WorkspaceAttempt {
    pub fn snapshot_output(&self) -> Result<crate::execution::LiveProcessOutput, ExecutionError> {
        self.attempt.snapshot_output()
    }
    pub fn cancel(&self) -> crate::execution::LiveProcessCancelAcceptance {
        self.attempt.cancel()
    }
    pub fn terminal_cause(&self) -> Option<crate::execution::LiveProcessTerminalCause> {
        self.attempt.terminal_cause()
    }
    pub fn wait_collect(
        &self,
    ) -> Result<crate::execution::LiveProcessOutcome, WorkspaceCollectionFailure> {
        self.attempt.wait_collect().map_err(|error| {
            let orphan = cleanup_unknown(&error).then(|| Box::new(self.active.clone()));
            WorkspaceCollectionFailure { error, orphan }
        })
    }
}

/// Uses only the handle retained by the failed owning collection. No caller
/// handle, path or identifier can substitute for it.
///
/// ```compile_fail
/// use receipts_workspace_execution::{orphan_workspace, WorkspaceHandle};
/// use receipts_workspace_execution::execution::LiveProcessAttemptError;
/// fn fabricate(handle: &WorkspaceHandle) {
///     orphan_workspace(handle, &LiveProcessAttemptError::ControllerFailed);
/// }
/// ```
pub fn orphan_workspace(
    failure: WorkspaceCollectionFailure,
) -> Result<WorkspaceHandle, WorkspaceLifecycleError> {
    let handle = failure
        .orphan
        .ok_or(WorkspaceLifecycleError::NotOrphanEvidence)?;
    checked(&handle, WorkspaceState::Orphaned)?;
    handle
        .transitioned(WorkspaceState::Orphaned, None)
        .map_err(WorkspaceLifecycleError::Transition)
}

fn cleanup_unknown(failure: &LiveProcessAttemptError) -> bool {
    match failure {
        LiveProcessAttemptError::ControllerFailed => true,
        LiveProcessAttemptError::Execution(error) => matches!(
            error,
            ExecutionError::ProcessGroupControlFailed { .. }
                | ExecutionError::ProcessGroupOwnershipFailed { .. }
                | ExecutionError::ForceKillFailed { .. }
                | ExecutionError::GracefulTerminationFailed { .. }
                | ExecutionError::TimeoutGraceWaitFailed { .. }
                | ExecutionError::TimeoutFinalWaitFailed { .. }
                | ExecutionError::ProcessWaitFailed { .. }
        ),
        _ => false,
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
