use crate::{CodexProbeExecutionError, FailureClass};

/// Classifies only exact failure evidence from one bounded Codex probe execution.
pub fn classify_codex_probe_execution_error(error: &CodexProbeExecutionError) -> FailureClass {
    match error {
        CodexProbeExecutionError::TimedOut { .. } => FailureClass::Timeout,
        CodexProbeExecutionError::WorkspaceExecution { .. }
        | CodexProbeExecutionError::TruncatedStream { .. }
        | CodexProbeExecutionError::Parse(_) => FailureClass::Unknown,
    }
}
