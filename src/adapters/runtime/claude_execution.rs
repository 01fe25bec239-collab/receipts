//! Native Claude **worker** foundation, not a RuntimeAdapter or host integration.
//! Callers must establish authorization and admission before execution. Technical
//! auth is independent of eligibility, entitlement and routing. In particular this
//! module enables no subscription external-worker route and does not resolve Q-V13-04.
//!
//! Contract reconciled 2026-09-11 with Claude Code 2.1.246 and official sources:
//! <https://code.claude.com/docs/en/cli-reference>,
//! <https://code.claude.com/docs/en/headless>, and
//! <https://code.claude.com/docs/en/permissions>.
//! The official Python SDK's `_internal/client.py` supplies the string user
//! envelope (including empty strings); `_internal/transport/subprocess_cli.py`
//! builds stream-json with `--verbose`. Source:
//! <https://github.com/anthropics/claude-agent-sdk-python/tree/main/src/claude_agent_sdk/_internal>.
//! `--verbose` follows both that transport and the official headless example.
//! No bidirectional hooks/permission callback or SDK control channel is implemented.
//!
//! Workspace owns all byte delivery, process control and cleanup. Its empty
//! environment remains unchanged; this foundation makes no claim that a native
//! installation's credentials or configured model will be available in that environment.
use crate::claude_stream_json::{contiguous_stdout, protocol};
use crate::{ClaudeStreamJson, ClaudeStreamJsonError, FailureClass, RuntimeAuthStatus};
use receipts_workspace_execution::execution::{
    CapturedProcessRun, ExecutionError, LiveProcessAttempt, LiveProcessAttemptError,
    LiveProcessCancelAcceptance, LiveProcessOutcome, LiveProcessOutput, LiveProcessStartError,
    LiveProcessTerminalCause, ProcessRunRequest, ProcessStdin, ProcessTermination,
    ProcessTimeoutPolicy, run_with_timeout, run_with_timeout_and_capture,
    start_live_process_attempt,
};
use std::{error::Error, fmt, mem::discriminant, path::PathBuf};

/// Claude permission behavior only. Neither mode provides OS confinement or
/// Workspace sandboxing. Installed Claude settings/policies still apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudePermissionMode {
    /// Exploration under Claude's plan permissions; not a security sandbox.
    Plan,
    /// Allows edits according to Claude's permission system, not unrestricted OS
    /// permission. Other actions remain subject to Claude's permission rules.
    AcceptEdits,
}
impl ClaudePermissionMode {
    fn argument(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::AcceptEdits => "acceptEdits",
        }
    }
}

/// Explicit execution inputs; never PATH-searched. No model binding or provider
/// session identity. The caller prompt remains unmodified data, including empty
/// and whitespace-only strings. Debug is omitted to avoid formatting prompts.
pub struct ClaudeTaskExecutionRequest {
    executable: PathBuf,
    workspace_root: PathBuf,
    cwd: PathBuf,
    timeout_policy: ProcessTimeoutPolicy,
    permission_mode: ClaudePermissionMode,
    prompt: String,
}
impl ClaudeTaskExecutionRequest {
    pub fn new(
        executable: impl Into<PathBuf>,
        workspace_root: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
        timeout_policy: ProcessTimeoutPolicy,
        permission_mode: ClaudePermissionMode,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            executable: executable.into(),
            workspace_root: workspace_root.into(),
            cwd: cwd.into(),
            timeout_policy,
            permission_mode,
            prompt: prompt.into(),
        }
    }
    pub fn timeout_policy(&self) -> &ProcessTimeoutPolicy {
        &self.timeout_policy
    }
    pub fn permission_mode(&self) -> ClaudePermissionMode {
        self.permission_mode
    }
}

/// Typed Workspace request/execution error. Explicit access to the underlying
/// error is possible, but formatting/source chains never reveal its paths/prose.
pub struct ClaudeTaskExecutionError(pub ExecutionError);
impl fmt::Debug for ClaudeTaskExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ClaudeTaskExecutionError")
            .field(&discriminant(&self.0))
            .finish()
    }
}
impl fmt::Display for ClaudeTaskExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl Error for ClaudeTaskExecutionError {}
impl From<ExecutionError> for ClaudeTaskExecutionError {
    fn from(error: ExecutionError) -> Self {
        Self(error)
    }
}

/// The sole worker argv/encoder/builder, used by both one-shot and live starts.
/// JSON Value serialization is infallible; canonical Workspace validation runs
/// on the serialized bytes *after* the transport newline is appended.
pub(crate) fn build_claude_task_request(
    request: &ClaudeTaskExecutionRequest,
) -> Result<ProcessRunRequest, ClaudeTaskExecutionError> {
    let mut encoded = serde_json::json!({
        "type": "user", "session_id": "",
        "message": { "role": "user", "content": request.prompt },
        "parent_tool_use_id": null
    })
    .to_string()
    .into_bytes();
    encoded.push(b'\n');
    let stdin = ProcessStdin::bytes(encoded)?;
    Ok(ProcessRunRequest::new(
        &request.executable,
        [
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--no-session-persistence",
            "--permission-mode",
            request.permission_mode.argument(),
            "--verbose",
        ],
        &request.workspace_root,
        &request.cwd,
    )?
    .with_stdin(stdin))
}

/// Bounded process truth and separately available protocol. Even a timed-out,
/// nonzero, malformed or truncated run retains both captured streams and cleanup.
pub struct ClaudeTaskExecutionResult {
    process: CapturedProcessRun,
    stdout: Option<Vec<u8>>,
}
impl ClaudeTaskExecutionResult {
    pub fn process(&self) -> &CapturedProcessRun {
        &self.process
    }
    pub fn protocol(&self) -> Result<ClaudeStreamJson<'_>, ClaudeStreamJsonError> {
        protocol(&self.stdout)
    }
}
pub fn execute_claude_task_once(
    request: &ClaudeTaskExecutionRequest,
) -> Result<ClaudeTaskExecutionResult, ClaudeTaskExecutionError> {
    let process = run_with_timeout_and_capture(
        &build_claude_task_request(request)?,
        request.timeout_policy(),
    )?;
    Ok(ClaudeTaskExecutionResult {
        stdout: contiguous_stdout(process.stdout()),
        process,
    })
}

/// Request validation and Workspace live **start** errors, never collection errors.
/// TerminalBeforeReady preserves the original Workspace lifecycle/capture evidence.
#[derive(Debug)]
pub enum ClaudeLiveStartError {
    Request(ClaudeTaskExecutionError),
    Workspace(LiveProcessStartError),
}
impl fmt::Display for ClaudeLiveStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl Error for ClaudeLiveStartError {}

/// One bounded snapshot, not a lifecycle verdict. A partial JSON record may be
/// incomplete now and parseable later; no history is accumulated by Runtime.
pub struct ClaudeLiveOutputSnapshot {
    output: LiveProcessOutput,
    stdout: Option<Vec<u8>>,
}
impl ClaudeLiveOutputSnapshot {
    pub fn output(&self) -> &LiveProcessOutput {
        &self.output
    }
    pub fn protocol(&self) -> Result<ClaudeStreamJson<'_>, ClaudeStreamJsonError> {
        protocol(&self.stdout)
    }
}
/// Collected Workspace outcome. Completed/nonzero stays Completed/nonzero, and
/// process success never becomes provider success. Forced kill is separate evidence.
pub struct ClaudeLiveOutcome {
    process: LiveProcessOutcome,
    stdout: Option<Vec<u8>>,
}
impl ClaudeLiveOutcome {
    pub fn process(&self) -> &LiveProcessOutcome {
        &self.process
    }
    pub fn protocol(&self) -> Result<ClaudeStreamJson<'_>, ClaudeStreamJsonError> {
        protocol(&self.stdout)
    }
}
/// Thin non-cloneable composition. Workspace Drop owns cleanup before collection.
pub struct ClaudeLiveAttempt {
    attempt: LiveProcessAttempt,
}
impl ClaudeLiveAttempt {
    pub fn snapshot_output(&self) -> Result<ClaudeLiveOutputSnapshot, ExecutionError> {
        let output = self.attempt.snapshot_output()?;
        Ok(ClaudeLiveOutputSnapshot {
            stdout: contiguous_stdout(output.stdout()),
            output,
        })
    }
    pub fn cancel(&self) -> LiveProcessCancelAcceptance {
        self.attempt.cancel()
    }
    pub fn terminal_cause(&self) -> Option<LiveProcessTerminalCause> {
        self.attempt.terminal_cause()
    }
    pub fn wait_collect(&self) -> Result<ClaudeLiveOutcome, LiveProcessAttemptError> {
        let process = self.attempt.wait_collect()?;
        Ok(ClaudeLiveOutcome {
            stdout: contiguous_stdout(process.stdout()),
            process,
        })
    }
}
pub fn start_claude_live_attempt(
    request: &ClaudeTaskExecutionRequest,
) -> Result<ClaudeLiveAttempt, ClaudeLiveStartError> {
    let process = build_claude_task_request(request).map_err(ClaudeLiveStartError::Request)?;
    let attempt = start_live_process_attempt(&process, request.timeout_policy())
        .map_err(ClaudeLiveStartError::Workspace)?;
    Ok(ClaudeLiveAttempt { attempt })
}

/// No auth payload, inferred expiry, or provider eligibility is represented.
#[derive(Debug)]
pub enum ClaudeAuthStatusError {
    Workspace(ClaudeTaskExecutionError),
    TimedOut(ProcessTermination),
}
impl fmt::Display for ClaudeAuthStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl Error for ClaudeAuthStatusError {}

/// Technical observation only: Connected means the native CLI reports logged in.
/// It does NOT mean provider eligible, subscription external-worker allowed,
/// Receipts entitled, routable, dispatch allowed, or Q-V13-04 resolved.
/// Workspace discards stdout/stderr at the OS boundary; auth JSON is never parsed
/// or retained. The official CLI's completed exit contract is 0/1; all others Unknown.
pub fn observe_claude_auth_status(
    executable: impl Into<PathBuf>,
    workspace_root: impl Into<PathBuf>,
    cwd: impl Into<PathBuf>,
    policy: &ProcessTimeoutPolicy,
) -> Result<RuntimeAuthStatus, ClaudeAuthStatusError> {
    let run = || -> Result<_, ClaudeTaskExecutionError> {
        let request = ProcessRunRequest::new(
            executable.into(),
            ["auth", "status"],
            workspace_root.into(),
            cwd.into(),
        )?;
        Ok(run_with_timeout(&request, policy)?)
    };
    let outcome = run().map_err(ClaudeAuthStatusError::Workspace)?;
    if outcome.timed_out() {
        return Err(ClaudeAuthStatusError::TimedOut(outcome.termination()));
    }
    Ok(match outcome.exit_code() {
        Some(0) => RuntimeAuthStatus::Connected,
        Some(1) => RuntimeAuthStatus::AuthRequired,
        _ => RuntimeAuthStatus::Unknown,
    })
}

/// Execution errors alone establish no provider meaning, including errors whose
/// variant names mention timeout cleanup. Only typed terminal timeout is Timeout.
pub fn classify_claude_task_execution_error(_: &ClaudeTaskExecutionError) -> FailureClass {
    FailureClass::Unknown
}
/// None means no process failure observed, never Claude task success. Protocol
/// validity and diagnostic strings are deliberately not classification inputs.
pub fn classify_claude_task_execution_result(
    result: &ClaudeTaskExecutionResult,
) -> Option<FailureClass> {
    let outcome = result.process().outcome();
    if outcome.timed_out() {
        Some(FailureClass::Timeout)
    } else if !outcome.success() {
        Some(FailureClass::Unknown)
    } else {
        None
    }
}
pub fn classify_claude_live_outcome(result: &ClaudeLiveOutcome) -> Option<FailureClass> {
    if result.process().terminal_cause() == LiveProcessTerminalCause::TimedOut {
        Some(FailureClass::Timeout)
    } else if !result.process().success() {
        Some(FailureClass::Unknown)
    } else {
        None
    }
}
