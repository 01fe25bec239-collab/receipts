//! Workspace owns all process lifecycle, environment clearing and bounded capture.
use std::{error::Error, fmt, path::Path};

use receipts_workspace_execution::execution::{
    CapturedProcessRun, ProcessRunRequest, ProcessTermination, ProcessTimeoutPolicy,
    run_with_timeout_and_capture,
};

use crate::{
    ClaudeCapabilityProbeReport, ClaudeProbeChannel, ClaudeProbeError, ClaudeProbeKind,
    ClaudeProbeObservation, FailureClass, parse_claude_probe,
};

/// Bounded evidence only: even Workspace diagnostic strings are not retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeProbeExecutionError {
    WorkspaceExecution {
        probe: ClaudeProbeKind,
    },
    TimedOut {
        probe: ClaudeProbeKind,
        termination: ProcessTermination,
    },
    TruncatedStream {
        probe: ClaudeProbeKind,
        channel: ClaudeProbeChannel,
    },
    Parse(ClaudeProbeError),
}

impl fmt::Display for ClaudeProbeExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Claude capability probe failed: {self:?}")
    }
}
impl Error for ClaudeProbeExecutionError {}

pub fn classify_claude_probe_execution_error(error: &ClaudeProbeExecutionError) -> FailureClass {
    match error {
        ClaudeProbeExecutionError::TimedOut { termination, .. } if termination.is_timed_out() => {
            FailureClass::Timeout
        }
        _ => FailureClass::Unknown,
    }
}

pub(crate) fn probe_request(
    executable: &Path,
    workspace_root: &Path,
    cwd: &Path,
    probe: ClaudeProbeKind,
) -> Result<ProcessRunRequest, ClaudeProbeExecutionError> {
    let argument = match probe {
        ClaudeProbeKind::Version => "--version",
        ClaudeProbeKind::Help => "--help",
    };
    ProcessRunRequest::new(executable, [argument], workspace_root, cwd)
        .map_err(|_| ClaudeProbeExecutionError::WorkspaceExecution { probe })
}

pub fn execute_claude_capability_probe(
    absolute_claude_path: &Path,
    workspace_root: &Path,
    cwd: &Path,
    timeout_policy: &ProcessTimeoutPolicy,
) -> Result<ClaudeCapabilityProbeReport, ClaudeProbeExecutionError> {
    execute_with_runner(
        absolute_claude_path,
        workspace_root,
        cwd,
        timeout_policy,
        |probe, request, policy| {
            run_with_timeout_and_capture(request, policy)
                .map_err(|_| ClaudeProbeExecutionError::WorkspaceExecution { probe })
        },
    )
}

pub(crate) fn execute_with_runner(
    executable: &Path,
    workspace_root: &Path,
    cwd: &Path,
    policy: &ProcessTimeoutPolicy,
    mut runner: impl FnMut(
        ClaudeProbeKind,
        &ProcessRunRequest,
        &ProcessTimeoutPolicy,
    ) -> Result<CapturedProcessRun, ClaudeProbeExecutionError>,
) -> Result<ClaudeCapabilityProbeReport, ClaudeProbeExecutionError> {
    let mut run = |probe| {
        let request = probe_request(executable, workspace_root, cwd, probe)?;
        let captured = runner(probe, &request, policy)?;
        capture_bytes(&captured, probe).map(|bytes| (captured, bytes))
    };
    let (version, [version_stdout, version_stderr]) = run(ClaudeProbeKind::Version)?;
    let (help, [help_stdout, help_stderr]) = run(ClaudeProbeKind::Help)?;
    parse_claude_probe(
        ClaudeProbeObservation {
            termination: version.outcome().termination(),
            exit_code: version.outcome().exit_code(),
            capture_complete: true,
            stdout: &version_stdout,
            stderr: &version_stderr,
        },
        ClaudeProbeObservation {
            termination: help.outcome().termination(),
            exit_code: help.outcome().exit_code(),
            capture_complete: true,
            stdout: &help_stdout,
            stderr: &help_stderr,
        },
    )
    .map_err(ClaudeProbeExecutionError::Parse)
}

fn capture_bytes(
    captured: &CapturedProcessRun,
    probe: ClaudeProbeKind,
) -> Result<[Vec<u8>; 2], ClaudeProbeExecutionError> {
    let termination = captured.outcome().termination();
    if termination.is_timed_out() {
        return Err(ClaudeProbeExecutionError::TimedOut { probe, termination });
    }
    for (stream, channel) in [
        (captured.stdout(), ClaudeProbeChannel::Stdout),
        (captured.stderr(), ClaudeProbeChannel::Stderr),
    ] {
        if stream.truncated() {
            return Err(ClaudeProbeExecutionError::TruncatedStream { probe, channel });
        }
    }
    Ok(
        [captured.stdout(), captured.stderr()]
            .map(|stream| [stream.head(), stream.tail()].concat()),
    )
}
