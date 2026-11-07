//! Runtime composition of the Workspace protected-launch boundary.
//!
//! Protection is requested only by trusted composition holding an admitted
//! [`ProtectedLauncher`]; it is never inferred from a CLI sandbox or
//! permission flag. A protected request uses Workspace admission and launch
//! exclusively and never falls back to the ordinary runner. Workspace alone
//! owns custody, worker creation, deadlines, collection, cancellation, cgroup
//! cleanup and release. Runtime adds no timeout loop, signalling or privilege.
//! Ordinary attempts remain available and are labeled non-authoritative:
//! they hold no custody and expose no controller channel.
use std::path::PathBuf;

use receipts_workspace_execution::execution::{
    ExecutionError, LiveProcessAttempt, LiveProcessAttemptError, LiveProcessCancelAcceptance,
    LiveProcessOutcome, LiveProcessOutput, LiveProcessStartError, LiveProcessTerminalCause,
    ProcessRunRequest, ProcessTimeoutPolicy, start_live_process_attempt,
};
use receipts_workspace_execution::{
    ControllerChannel, ProtectedAttempt, ProtectedLaunchError, ProtectedLauncher, WorkspaceHandle,
};

use crate::{FailureClass, RawFailure};

/// Trusted request for a Workspace-protected launch: an already admitted
/// launcher and the repository whose worktree the workspace handle names.
/// Debug omits the repository path.
#[derive(Clone)]
pub struct ProtectedLaunch {
    launcher: ProtectedLauncher,
    repository_root: PathBuf,
}
impl ProtectedLaunch {
    pub fn new(launcher: ProtectedLauncher, repository_root: impl Into<PathBuf>) -> Self {
        Self {
            launcher,
            repository_root: repository_root.into(),
        }
    }
}
impl std::fmt::Debug for ProtectedLaunch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtectedLaunch").finish_non_exhaustive()
    }
}

/// One live process: ordinary (non-authoritative) or Workspace-protected.
pub(crate) enum LiveProcess {
    Ordinary(LiveProcessAttempt),
    Protected {
        attempt: Box<ProtectedAttempt>,
        read_only: bool,
    },
}
impl LiveProcess {
    pub(crate) fn ordinary(
        request: &ProcessRunRequest,
        timeout: &ProcessTimeoutPolicy,
    ) -> Result<Self, LiveProcessStartError> {
        start_live_process_attempt(request, timeout).map(Self::Ordinary)
    }

    /// Admission then launch of the same request bytes the ordinary path
    /// would run. Every refusal returns before or without an ordinary start.
    pub(crate) fn protected(
        request: &ProcessRunRequest,
        timeout: &ProcessTimeoutPolicy,
        launch: &ProtectedLaunch,
        workspace: &WorkspaceHandle,
        read_only: bool,
    ) -> Result<Self, FailureClass> {
        let admission = launch
            .launcher
            .admit(workspace, &launch.repository_root)
            .map_err(|e| classify_protected(&e))?;
        let attempt = launch
            .launcher
            .launch(admission, request, timeout)
            .map_err(|e| classify_protected(&e))?;
        Ok(Self::Protected {
            attempt: Box::new(attempt),
            read_only,
        })
    }

    pub(crate) fn snapshot_output(&self) -> Result<LiveProcessOutput, ExecutionError> {
        match self {
            Self::Ordinary(a) => a.snapshot_output(),
            Self::Protected { attempt, .. } => attempt.attempt().snapshot_output(),
        }
    }
    pub(crate) fn cancel(&self) -> LiveProcessCancelAcceptance {
        match self {
            Self::Ordinary(a) => a.cancel(),
            Self::Protected { attempt, .. } => attempt.attempt().cancel(),
        }
    }
    pub(crate) fn terminal_cause(&self) -> Option<LiveProcessTerminalCause> {
        match self {
            Self::Ordinary(a) => a.terminal_cause(),
            Self::Protected { attempt, .. } => attempt.attempt().terminal_cause(),
        }
    }
    /// Protected success is also Workspace's collection/cgroup-cleanup proof.
    pub(crate) fn wait_collect(&self) -> Result<LiveProcessOutcome, LiveProcessAttemptError> {
        match self {
            Self::Ordinary(a) => a.wait_collect(),
            Self::Protected { attempt, .. } => attempt.attempt().wait_collect(),
        }
    }
    pub(crate) fn protected_attempt(&self) -> bool {
        matches!(self, Self::Protected { .. })
    }
    /// The role-appropriate typed channel: query-only reviewer for read-only
    /// attempts, worker otherwise. Ordinary attempts have none.
    pub(crate) fn controller_channel(&self) -> Option<&ControllerChannel> {
        match self {
            Self::Ordinary(_) => None,
            Self::Protected { attempt, read_only } => Some(if *read_only {
                attempt.reviewer_channel()
            } else {
                attempt.worker_channel()
            }),
        }
    }
    /// Workspace refuses unless collection proved cleanup; refusal retains
    /// custody. `None`: an ordinary attempt holds no custody.
    pub(crate) fn release_custody(&self) -> Option<Result<(), ProtectedLaunchError>> {
        match self {
            Self::Ordinary(_) => None,
            Self::Protected { attempt, .. } => Some(attempt.release_custody()),
        }
    }
}

/// Refusals before any worker could exist are `PolicyBlocked`: the mandatory
/// protection could not be provided. Start keeps the typed live-start
/// projection; unproven cleanup and anything unrecognized stay `Unknown`.
fn classify_protected(error: &ProtectedLaunchError) -> FailureClass {
    match error {
        ProtectedLaunchError::Unsupported(_)
        | ProtectedLaunchError::InvalidConfig(_)
        | ProtectedLaunchError::Custody(_)
        | ProtectedLaunchError::Binding(_)
        | ProtectedLaunchError::IdentityExhausted
        | ProtectedLaunchError::Workspace(_) => FailureClass::PolicyBlocked,
        ProtectedLaunchError::Start(start) => RawFailure::from(start).classify_failure(),
        _ => FailureClass::Unknown,
    }
}

#[cfg(test)]
pub(crate) fn classify_protected_for_test(error: &ProtectedLaunchError) -> FailureClass {
    classify_protected(error)
}
