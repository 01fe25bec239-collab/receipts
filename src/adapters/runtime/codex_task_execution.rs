//! One bounded `codex exec` worker-task execution primitive.
//!
//! This is the first real Codex worker-task execution primitive. It runs
//! exactly one official Codex child through the Workspace-owned process
//! runner:
//!
//! ```text
//! <absolute codex path> exec --json --sandbox <read-only|workspace-write> <PROMPT>
//! ```
//!
//! Frozen properties of this slice:
//!
//! * **Workspace-owned process boundary.** Execution goes through
//!   [`run_with_timeout_and_capture`](receipts_workspace_execution::execution::run_with_timeout_and_capture)
//!   only. There is no direct child spawning here, no command-string
//!   assembly, and no environment handling: the Workspace runner remains
//!   authoritative for absolute-executable validation, working-directory
//!   containment, the empty inherited environment, timeouts, termination,
//!   and bounded capture.
//! * **Prompt is data.** The caller-supplied prompt travels as exactly one
//!   argv element, verbatim. It is never trimmed, normalized, split,
//!   interpolated, or interpreted; flag-looking prompt text cannot alter the
//!   sandbox mode, the timeout, the working directory, or the executable.
//!   Only the true empty string is rejected; whitespace-only prompts are
//!   valid and preserved exactly.
//! * **Exactly two sandbox modes.** [`CodexTaskSandboxMode`] exposes
//!   [`CodexTaskSandboxMode::ReadOnly`] and
//!   [`CodexTaskSandboxMode::WorkspaceWrite`] only. The provider's unsafe
//!   unrestricted sandbox mode is deliberately unrepresentable: no enum
//!   variant maps to it, no string-based escape hatch exists, and no silent
//!   fallback to another mode ever happens.
//! * **Raw output bytes.** Standard output and standard error are preserved
//!   as exact raw bytes (head followed by tail) with no decoding and no
//!   inserted separators. No JSONL event parsing, no final-message
//!   extraction, and no semantic success inference happen here.
//! * **Fail-closed capture.** A timed-out run, a truncated stream, or a
//!   completed run without a numeric exit status never yields a result.
//!   An ordinary completed non-zero exit is normal process truth and is
//!   preserved in the result; later binding slices decide its semantics.
//!
//! Deliberately excluded here (later binding slices): `RuntimeAdapter`
//! implementation, failure-class mapping, attempt events, result
//! collection, cancellation, resume, model discovery, model routing, and
//! credential handling of any kind.

use std::{error::Error, ffi::OsString, fmt, path::Path, path::PathBuf};

use receipts_workspace_execution::execution::{
    CapturedProcessRun, ExecutionError, ProcessRunRequest, ProcessTermination,
    ProcessTimeoutPolicy, run_with_timeout_and_capture,
};

/// The narrow Runtime-owned sandbox vocabulary for one Codex task.
///
/// Exactly two modes are representable. There is no variant for the
/// provider's unsafe unrestricted sandbox mode and no string-based
/// constructor, so that mode cannot be reached through this API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexTaskSandboxMode {
    /// Maps to the provider's read-only sandbox CLI value.
    ReadOnly,
    /// Maps to the provider's workspace-write sandbox CLI value.
    WorkspaceWrite,
}

impl CodexTaskSandboxMode {
    /// The exact CLI value placed after `--sandbox` for this mode.
    pub fn cli_value(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
        }
    }
}

/// The fully explicit request to execute one bounded Codex task.
///
/// Every field is required: there are no hidden defaults for the
/// executable, the workspace root, the working directory, the timeout
/// policy, the sandbox mode, or the prompt.
#[derive(Debug, Clone)]
pub struct CodexTaskExecutionRequest {
    absolute_codex_path: PathBuf,
    workspace_root: PathBuf,
    cwd: PathBuf,
    timeout_policy: ProcessTimeoutPolicy,
    sandbox_mode: CodexTaskSandboxMode,
    prompt: String,
}

impl CodexTaskExecutionRequest {
    /// Assembles an explicit task-execution request.
    ///
    /// The prompt is stored exactly as supplied; emptiness is checked (and
    /// rejected) at execution time so the request itself stays a plain
    /// carrier of caller intent.
    pub fn new(
        absolute_codex_path: impl Into<PathBuf>,
        workspace_root: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
        timeout_policy: ProcessTimeoutPolicy,
        sandbox_mode: CodexTaskSandboxMode,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            absolute_codex_path: absolute_codex_path.into(),
            workspace_root: workspace_root.into(),
            cwd: cwd.into(),
            timeout_policy,
            sandbox_mode,
            prompt: prompt.into(),
        }
    }

    /// The caller-supplied absolute path of the official Codex executable.
    pub fn absolute_codex_path(&self) -> &Path {
        &self.absolute_codex_path
    }

    /// The caller-supplied absolute workspace root.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// The caller-supplied absolute child working directory.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// The caller-supplied explicit timeout policy.
    pub fn timeout_policy(&self) -> &ProcessTimeoutPolicy {
        &self.timeout_policy
    }

    /// The caller-selected sandbox mode.
    pub fn sandbox_mode(&self) -> CodexTaskSandboxMode {
        self.sandbox_mode
    }

    /// The caller-supplied prompt, exactly as given.
    pub fn prompt(&self) -> &str {
        &self.prompt
    }
}

/// The typed result of one completed, fully captured Codex task process.
///
/// A non-zero exit code is ordinary process truth preserved here, never a
/// semantic verdict: this slice performs no agent-result interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexTaskExecutionResult {
    exit_code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    sandbox_mode: CodexTaskSandboxMode,
}

impl CodexTaskExecutionResult {
    /// The child's numeric exit status, zero or otherwise.
    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    /// The exact complete standard-output bytes.
    pub fn stdout(&self) -> &[u8] {
        &self.stdout
    }

    /// The exact complete standard-error bytes.
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    /// The sandbox mode the child was actually launched with.
    pub fn sandbox_mode(&self) -> CodexTaskSandboxMode {
        self.sandbox_mode
    }
}

/// Which captured task stream a truncation error refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexTaskOutputChannel {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// Typed failures of one bounded Codex task execution.
#[derive(Debug)]
pub enum CodexTaskExecutionError {
    /// The prompt was the true empty string. Nothing was launched.
    EmptyPrompt,
    /// Building the process request or running the child through the
    /// Workspace runner failed. The underlying Workspace error is retained.
    WorkspaceExecution {
        /// The underlying Workspace-owned failure.
        source: ExecutionError,
    },
    /// The run deadline expired. No result exists.
    TimedOut {
        /// How the timed-out attempt actually ended.
        termination: ProcessTermination,
    },
    /// One stream exceeded the bounded capture limit, so its middle was
    /// discarded. Partial output is never reported as a result.
    TruncatedStream {
        /// Which stream lost its middle.
        channel: CodexTaskOutputChannel,
    },
    /// An ordinarily completed run reported no numeric exit status.
    /// Nothing is fabricated.
    MissingExitStatus,
}

impl fmt::Display for CodexTaskExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPrompt => write!(
                formatter,
                "codex task prompt is empty; refusing to launch a worker task with no instructions"
            ),
            Self::WorkspaceExecution { source } => {
                write!(formatter, "codex task could not be executed: {source}")
            }
            Self::TimedOut { termination } => {
                write!(formatter, "codex task timed out: {termination:?}")
            }
            Self::TruncatedStream { channel } => write!(
                formatter,
                "codex task capture was truncated on {channel:?}; refusing partial output"
            ),
            Self::MissingExitStatus => write!(
                formatter,
                "codex task completed without a numeric exit status; refusing to fabricate one"
            ),
        }
    }
}

impl Error for CodexTaskExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::WorkspaceExecution { source } => Some(source),
            Self::EmptyPrompt
            | Self::TimedOut { .. }
            | Self::TruncatedStream { .. }
            | Self::MissingExitStatus => None,
        }
    }
}

/// Private Runtime-local snapshot of the Workspace-owned captured view.
///
/// Exists only so the execution seam is deterministically testable without
/// touching Workspace-owned constructors. Never exported.
#[derive(Debug, Clone)]
pub(crate) struct TaskCaptureSnapshot {
    pub(crate) termination: ProcessTermination,
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout_head: Vec<u8>,
    pub(crate) stdout_tail: Vec<u8>,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_head: Vec<u8>,
    pub(crate) stderr_tail: Vec<u8>,
    pub(crate) stderr_truncated: bool,
}

impl TaskCaptureSnapshot {
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        termination: ProcessTermination,
        exit_code: Option<i32>,
        stdout_head: Vec<u8>,
        stdout_tail: Vec<u8>,
        stdout_truncated: bool,
        stderr_head: Vec<u8>,
        stderr_tail: Vec<u8>,
        stderr_truncated: bool,
    ) -> Self {
        Self {
            termination,
            exit_code,
            stdout_head,
            stdout_tail,
            stdout_truncated,
            stderr_head,
            stderr_tail,
            stderr_truncated,
        }
    }

    fn from_captured(captured: &CapturedProcessRun) -> Self {
        Self {
            termination: captured.outcome().termination(),
            exit_code: captured.outcome().exit_code(),
            stdout_head: captured.stdout().head().to_vec(),
            stdout_tail: captured.stdout().tail().to_vec(),
            stdout_truncated: captured.stdout().truncated(),
            stderr_head: captured.stderr().head().to_vec(),
            stderr_tail: captured.stderr().tail().to_vec(),
            stderr_truncated: captured.stderr().truncated(),
        }
    }
}

/// Executes exactly one bounded Codex task through the real Workspace runner.
pub fn execute_codex_task_once(
    request: &CodexTaskExecutionRequest,
) -> Result<CodexTaskExecutionResult, CodexTaskExecutionError> {
    execute_with_runner(request, real_runner)
}

/// Shared execution core: production supplies [`real_runner`], tests supply
/// a deterministic fake. Private; not a public process-execution API.
pub(crate) fn execute_with_runner(
    request: &CodexTaskExecutionRequest,
    mut run_one: impl FnMut(
        &ProcessRunRequest,
        &ProcessTimeoutPolicy,
    ) -> Result<TaskCaptureSnapshot, CodexTaskExecutionError>,
) -> Result<CodexTaskExecutionResult, CodexTaskExecutionError> {
    // Only the true empty string is rejected. Whitespace-only prompts are
    // valid data and must reach the child untouched.
    if request.prompt().is_empty() {
        return Err(CodexTaskExecutionError::EmptyPrompt);
    }

    // The prompt becomes exactly one argv element, verbatim: no shell, no
    // splitting, no interpolation, no added flags.
    let argv: Vec<OsString> = vec![
        OsString::from("exec"),
        OsString::from("--json"),
        OsString::from("--sandbox"),
        OsString::from(request.sandbox_mode().cli_value()),
        OsString::from(request.prompt()),
    ];
    let process_request = ProcessRunRequest::new(
        request.absolute_codex_path(),
        argv,
        request.workspace_root(),
        request.cwd(),
    )
    .map_err(|source| CodexTaskExecutionError::WorkspaceExecution { source })?;

    let snapshot = run_one(&process_request, request.timeout_policy())?;

    if snapshot.termination.is_timed_out() {
        return Err(CodexTaskExecutionError::TimedOut {
            termination: snapshot.termination,
        });
    }
    if snapshot.stdout_truncated {
        return Err(CodexTaskExecutionError::TruncatedStream {
            channel: CodexTaskOutputChannel::Stdout,
        });
    }
    if snapshot.stderr_truncated {
        return Err(CodexTaskExecutionError::TruncatedStream {
            channel: CodexTaskOutputChannel::Stderr,
        });
    }
    let Some(exit_code) = snapshot.exit_code else {
        return Err(CodexTaskExecutionError::MissingExitStatus);
    };

    // Non-truncated streams reconstruct exactly as head followed by tail,
    // with nothing synthetic inserted between them.
    let mut stdout = Vec::with_capacity(snapshot.stdout_head.len() + snapshot.stdout_tail.len());
    stdout.extend_from_slice(&snapshot.stdout_head);
    stdout.extend_from_slice(&snapshot.stdout_tail);
    let mut stderr = Vec::with_capacity(snapshot.stderr_head.len() + snapshot.stderr_tail.len());
    stderr.extend_from_slice(&snapshot.stderr_head);
    stderr.extend_from_slice(&snapshot.stderr_tail);

    Ok(CodexTaskExecutionResult {
        exit_code,
        stdout,
        stderr,
        sandbox_mode: request.sandbox_mode(),
    })
}

/// The single production bridge to the Workspace-owned runner. No fake is
/// selectable here and no environment toggle exists.
fn real_runner(
    request: &ProcessRunRequest,
    timeout_policy: &ProcessTimeoutPolicy,
) -> Result<TaskCaptureSnapshot, CodexTaskExecutionError> {
    let captured = run_with_timeout_and_capture(request, timeout_policy)
        .map_err(|source| CodexTaskExecutionError::WorkspaceExecution { source })?;
    Ok(TaskCaptureSnapshot::from_captured(&captured))
}
