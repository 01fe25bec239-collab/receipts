use std::{error::Error, fmt, path::Path};

use receipts_workspace_execution::execution::{
    CapturedProcessRun, ExecutionError, ProcessRunRequest, ProcessTermination,
    ProcessTimeoutPolicy, run_with_timeout_and_capture,
};

use super::codex_probe::{
    CODEX_EXEC_HELP_PROBE, CODEX_VERSION_PROBE, CodexCapabilityProbeReport, CodexProbeChannel,
    CodexProbeError, CodexProbeKind, CodexProbeObservation, parse_codex_probe,
};

#[derive(Debug)]
pub enum CodexProbeExecutionError {
    WorkspaceExecution {
        probe: CodexProbeKind,
        source: ExecutionError,
    },
    TimedOut {
        probe: CodexProbeKind,
        termination: ProcessTermination,
    },
    TruncatedStream {
        probe: CodexProbeKind,
        channel: CodexProbeChannel,
    },
    Parse(CodexProbeError),
}

impl fmt::Display for CodexProbeExecutionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkspaceExecution { probe, source } => write!(
                formatter,
                "codex capability probe {probe:?} could not be executed: {source}"
            ),
            Self::TimedOut { probe, termination } => write!(
                formatter,
                "codex capability probe {probe:?} timed out: {termination:?}"
            ),
            Self::TruncatedStream { probe, channel } => write!(
                formatter,
                "codex capability probe {probe:?} capture was truncated on {channel:?}"
            ),
            Self::Parse(error) => write!(formatter, "codex capability probe failed: {error}"),
        }
    }
}

impl Error for CodexProbeExecutionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::WorkspaceExecution { source, .. } => Some(source),
            Self::Parse(error) => Some(error),
            Self::TimedOut { .. } | Self::TruncatedStream { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProbeCaptureSnapshot {
    pub(crate) termination: ProcessTermination,
    pub(crate) exit_code: Option<i32>,
    pub(crate) stdout_head: Vec<u8>,
    pub(crate) stdout_tail: Vec<u8>,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_head: Vec<u8>,
    pub(crate) stderr_tail: Vec<u8>,
    pub(crate) stderr_truncated: bool,
}

impl ProbeCaptureSnapshot {
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

pub fn execute_codex_capability_probe(
    absolute_codex_path: &Path,
    workspace_root: &Path,
    cwd: &Path,
    timeout_policy: &ProcessTimeoutPolicy,
) -> Result<CodexCapabilityProbeReport, CodexProbeExecutionError> {
    execute_with_runner(
        absolute_codex_path,
        workspace_root,
        cwd,
        timeout_policy,
        real_runner,
    )
}

pub(crate) fn execute_with_runner(
    absolute_codex_path: &Path,
    workspace_root: &Path,
    cwd: &Path,
    timeout_policy: &ProcessTimeoutPolicy,
    mut run_one: impl FnMut(
        CodexProbeKind,
        &ProcessRunRequest,
        &ProcessTimeoutPolicy,
    ) -> Result<ProbeCaptureSnapshot, CodexProbeExecutionError>,
) -> Result<CodexCapabilityProbeReport, CodexProbeExecutionError> {
    let version_request = ProcessRunRequest::new(
        absolute_codex_path,
        CODEX_VERSION_PROBE.args.iter().copied(),
        workspace_root,
        cwd,
    )
    .map_err(|source| CodexProbeExecutionError::WorkspaceExecution {
        probe: CodexProbeKind::Version,
        source,
    })?;
    let version_snapshot = run_one(CodexProbeKind::Version, &version_request, timeout_policy)?;
    let (version_stdout, version_stderr) =
        snapshot_to_observation_bytes(&version_snapshot, CodexProbeKind::Version)?;

    let help_request = ProcessRunRequest::new(
        absolute_codex_path,
        CODEX_EXEC_HELP_PROBE.args.iter().copied(),
        workspace_root,
        cwd,
    )
    .map_err(|source| CodexProbeExecutionError::WorkspaceExecution {
        probe: CodexProbeKind::ExecHelp,
        source,
    })?;
    let help_snapshot = run_one(CodexProbeKind::ExecHelp, &help_request, timeout_policy)?;
    let (help_stdout, help_stderr) =
        snapshot_to_observation_bytes(&help_snapshot, CodexProbeKind::ExecHelp)?;

    let version_observation = CodexProbeObservation {
        stdout: &version_stdout,
        stderr: &version_stderr,
        exit_code: version_snapshot.exit_code,
        capture_complete: true,
    };
    let help_observation = CodexProbeObservation {
        stdout: &help_stdout,
        stderr: &help_stderr,
        exit_code: help_snapshot.exit_code,
        capture_complete: true,
    };
    parse_codex_probe(version_observation, help_observation)
        .map_err(CodexProbeExecutionError::Parse)
}

fn real_runner(
    probe: CodexProbeKind,
    request: &ProcessRunRequest,
    timeout_policy: &ProcessTimeoutPolicy,
) -> Result<ProbeCaptureSnapshot, CodexProbeExecutionError> {
    let captured = run_with_timeout_and_capture(request, timeout_policy)
        .map_err(|source| CodexProbeExecutionError::WorkspaceExecution { probe, source })?;
    Ok(ProbeCaptureSnapshot::from_captured(&captured))
}

fn snapshot_to_observation_bytes(
    snapshot: &ProbeCaptureSnapshot,
    probe: CodexProbeKind,
) -> Result<(Vec<u8>, Vec<u8>), CodexProbeExecutionError> {
    if snapshot.termination.is_timed_out() {
        return Err(CodexProbeExecutionError::TimedOut {
            probe,
            termination: snapshot.termination,
        });
    }
    if snapshot.stdout_truncated {
        return Err(CodexProbeExecutionError::TruncatedStream {
            probe,
            channel: CodexProbeChannel::Stdout,
        });
    }
    if snapshot.stderr_truncated {
        return Err(CodexProbeExecutionError::TruncatedStream {
            probe,
            channel: CodexProbeChannel::Stderr,
        });
    }
    let mut stdout = Vec::with_capacity(snapshot.stdout_head.len() + snapshot.stdout_tail.len());
    stdout.extend_from_slice(&snapshot.stdout_head);
    stdout.extend_from_slice(&snapshot.stdout_tail);
    let mut stderr = Vec::with_capacity(snapshot.stderr_head.len() + snapshot.stderr_tail.len());
    stderr.extend_from_slice(&snapshot.stderr_head);
    stderr.extend_from_slice(&snapshot.stderr_tail);
    Ok((stdout, stderr))
}
