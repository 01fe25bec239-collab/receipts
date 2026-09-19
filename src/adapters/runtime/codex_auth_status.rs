//! Codex technical authentication observation only; no credential acquisition.
//! Connected grants no entitlement, provider eligibility, dispatch admission or
//! subscription-worker authorization. Q-V13-04 remains unresolved / fail-closed.

use std::{error::Error, fmt, mem::discriminant, path::Path};

use receipts_workspace_execution::execution::{
    ExecutionError, ProcessRunRequest, ProcessTermination, ProcessTimeoutPolicy,
    run_with_timeout_and_capture,
};

use crate::{FailureClass, RuntimeAuthStatus};

/// Typed execution evidence, never provider diagnostics or inferred auth truth.
/// Workspace errors remain explicitly accessible; formatting and source chains
/// omit their potentially sensitive paths and prose.
pub enum CodexAuthStatusError {
    Workspace(ExecutionError),
    TimedOut(ProcessTermination),
}

impl fmt::Debug for CodexAuthStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Workspace(error) => f
                .debug_tuple("CodexAuthStatusError::Workspace")
                .field(&discriminant(error))
                .finish(),
            Self::TimedOut(termination) => f
                .debug_tuple("CodexAuthStatusError::TimedOut")
                .field(termination)
                .finish(),
        }
    }
}

impl fmt::Display for CodexAuthStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for CodexAuthStatusError {}

/// Observe `codex login status` through Workspace's hardened execution boundary.
/// Completed exit 0 means Connected; every other or missing status is Unknown.
/// Stdout/stderr are bounded by Workspace and discarded without interpretation,
/// including truncated output. The empty environment and lifecycle stay owned
/// by Workspace; unavailable credential context never triggers setup or recovery.
pub fn observe_codex_auth_status(
    absolute_codex_path: &Path,
    workspace_root: &Path,
    cwd: &Path,
    timeout_policy: &ProcessTimeoutPolicy,
) -> Result<RuntimeAuthStatus, CodexAuthStatusError> {
    let request = ProcessRunRequest::new(
        absolute_codex_path,
        ["login", "status"],
        workspace_root,
        cwd,
    )
    .map_err(CodexAuthStatusError::Workspace)?;
    let captured = run_with_timeout_and_capture(&request, timeout_policy)
        .map_err(CodexAuthStatusError::Workspace)?;
    let outcome = captured.outcome();
    if outcome.timed_out() {
        return Err(CodexAuthStatusError::TimedOut(outcome.termination()));
    }
    Ok(match outcome.exit_code() {
        Some(0) => RuntimeAuthStatus::Connected,
        _ => RuntimeAuthStatus::Unknown,
    })
}

/// Only typed Workspace timeout evidence establishes Timeout; prose never does.
pub fn classify_codex_auth_status_error(error: &CodexAuthStatusError) -> FailureClass {
    match error {
        CodexAuthStatusError::TimedOut(termination) if termination.is_timed_out() => {
            FailureClass::Timeout
        }
        _ => FailureClass::Unknown,
    }
}
