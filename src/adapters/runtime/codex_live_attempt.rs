//! Post-admission Codex lifecycle composition, not a RuntimeAdapter.
//! Workspace alone owns execution, terminal arbitration, timeout and Drop cleanup.
//! No routing, provider policy, credentials or product verdict is implemented.

use std::{error::Error, fmt};

use receipts_workspace_execution::execution::{
    CapturedStream, ExecutionError, LiveProcessAttempt, LiveProcessAttemptError,
    LiveProcessCancelAcceptance, LiveProcessOutcome, LiveProcessOutput, LiveProcessTerminalCause,
    start_live_process_attempt,
};

use crate::codex_jsonl::interpret_stdout;
use crate::codex_task_execution::build_codex_task_request;
use crate::{
    CodexJsonlError, CodexJsonlProtocol, CodexTaskExecutionError, CodexTaskExecutionRequest,
    CodexTaskSandboxMode,
};

/// Start failure only; no lifecycle or protocol verdict is inferred.
#[derive(Debug)]
pub enum CodexLiveStartError {
    /// Shared Codex request validation failed before launch.
    Request(CodexTaskExecutionError),
    /// The authoritative Workspace live start failed.
    Workspace(LiveProcessAttemptError),
}
impl fmt::Display for CodexLiveStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(e) => e.fmt(f),
            Self::Workspace(e) => e.fmt(f),
        }
    }
}
impl Error for CodexLiveStartError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(match self {
            Self::Request(e) => e,
            Self::Workspace(e) => e,
        })
    }
}

/// Fail-closed protocol availability, independent of lifecycle and FailureClass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexLiveProtocolError {
    /// Missing middle bytes prohibit interpreting retained head/tail as JSONL.
    StdoutTruncated,
    /// Current stdout is not a complete valid protocol stream. A later snapshot
    /// may be parseable; this does not mean the process failed.
    Jsonl(CodexJsonlError),
}
impl fmt::Display for CodexLiveProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StdoutTruncated => f.write_str("Codex stdout truncated; protocol unavailable"),
            Self::Jsonl(e) => e.fmt(f),
        }
    }
}
impl Error for CodexLiveProtocolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::StdoutTruncated => None,
            Self::Jsonl(e) => Some(e),
        }
    }
}

// At most one capture-limit-sized copy, never an accumulator across snapshots.
// Truncated fragments are never joined, even temporarily.
fn contiguous_stdout(stdout: &CapturedStream) -> Option<Vec<u8>> {
    (!stdout.truncated()).then(|| [stdout.head(), stdout.tail()].concat())
}
fn protocol(stdout: &Option<Vec<u8>>) -> Result<CodexJsonlProtocol<'_>, CodexLiveProtocolError> {
    interpret_stdout(
        stdout
            .as_deref()
            .ok_or(CodexLiveProtocolError::StdoutTruncated)?,
    )
    .map_err(CodexLiveProtocolError::Jsonl)
}

/// Bounded observation, not terminal evidence. No accumulated snapshot history.
/// Payload access is explicit; this type deliberately omits Debug.
pub struct CodexLiveOutputSnapshot {
    output: LiveProcessOutput,
    stdout: Option<Vec<u8>>,
}
impl CodexLiveOutputSnapshot {
    /// Authoritative separate streams, byte counts and truncation facts.
    pub fn output(&self) -> &LiveProcessOutput {
        &self.output
    }
    /// Interpret only this snapshot's stdout with the shared fail-closed parser.
    /// Partial records fail explicitly; stderr can never supply protocol events.
    pub fn protocol(&self) -> Result<CodexJsonlProtocol<'_>, CodexLiveProtocolError> {
        protocol(&self.stdout)
    }
}

/// Collected Workspace evidence plus independently interpretable bounded stdout.
/// Parser failure never erases lifecycle, exit status or cleanup evidence.
/// No Codex task, Receipts task, review or admission success is inferred.
pub struct CodexLiveOutcome {
    process: LiveProcessOutcome,
    stdout: Option<Vec<u8>>,
    sandbox_mode: CodexTaskSandboxMode,
}
impl CodexLiveOutcome {
    /// Authoritative process lifecycle and separate bounded streams. Its
    /// `success()` is Workspace process success only. Forced kill is cleanup
    /// evidence, separate from Completed, Cancelled and TimedOut.
    pub fn process(&self) -> &LiveProcessOutcome {
        &self.process
    }
    /// Exact sandbox mode used in the shared Codex request.
    pub fn sandbox_mode(&self) -> CodexTaskSandboxMode {
        self.sandbox_mode
    }
    /// Interpret stdout only. Lifecycle completion does not imply protocol
    /// completion, and protocol completion does not imply Receipts success.
    pub fn protocol(&self) -> Result<CodexJsonlProtocol<'_>, CodexLiveProtocolError> {
        protocol(&self.stdout)
    }
}

/// Non-cloneable owner of a Workspace live attempt; not a RuntimeAdapter.
/// Dropping it invokes the owned Workspace handle's bounded cleanup directly.
/// Shared-reference methods preserve Workspace concurrent collection/cancel semantics.
pub struct CodexLiveAttempt {
    attempt: LiveProcessAttempt,
    sandbox_mode: CodexTaskSandboxMode,
}
impl CodexLiveAttempt {
    /// Bounded observation while running, terminating, terminal or collected.
    /// Each call stands alone and grants no stronger guarantee than Workspace.
    pub fn snapshot_output(&self) -> Result<CodexLiveOutputSnapshot, ExecutionError> {
        let output = self.attempt.snapshot_output()?;
        Ok(CodexLiveOutputSnapshot {
            stdout: contiguous_stdout(output.stdout()),
            output,
        })
    }
    /// Delegate cancellation exactly; only Workspace decides which claim wins.
    pub fn cancel(&self) -> LiveProcessCancelAcceptance {
        self.attempt.cancel()
    }
    /// Observe Workspace's winning cause. This alone does not prove cleanup.
    pub fn terminal_cause(&self) -> Option<LiveProcessTerminalCause> {
        self.attempt.terminal_cause()
    }
    /// Collect once through Workspace, preserving its typed control errors.
    /// Protocol errors are separately available on the returned outcome.
    pub fn wait_collect(&self) -> Result<CodexLiveOutcome, LiveProcessAttemptError> {
        let process = self.attempt.wait_collect()?;
        Ok(CodexLiveOutcome {
            stdout: contiguous_stdout(process.stdout()),
            process,
            sandbox_mode: self.sandbox_mode,
        })
    }
}

/// Start using the exact one-shot Codex argv builder and explicit timeout policy.
/// Caller authorization/admission must precede this boundary; this function
/// performs no admission, routing or provider-policy decision. Workspace alone
/// validates and owns the OS lifecycle, empty environment and bounded capture.
pub fn start_codex_live_attempt(
    request: &CodexTaskExecutionRequest,
) -> Result<CodexLiveAttempt, CodexLiveStartError> {
    let process = build_codex_task_request(request).map_err(CodexLiveStartError::Request)?;
    let attempt = start_live_process_attempt(&process, request.timeout_policy())
        .map_err(CodexLiveStartError::Workspace)?;
    Ok(CodexLiveAttempt {
        attempt,
        sandbox_mode: request.sandbox_mode(),
    })
}
