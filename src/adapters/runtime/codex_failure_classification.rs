use crate::{CodexTaskExecutionError, CodexTaskExecutionResult, FailureClass};

/// Classifies only exact failure evidence from one bounded Codex task execution.
pub fn classify_codex_task_execution_error(error: &CodexTaskExecutionError) -> FailureClass {
    match error {
        CodexTaskExecutionError::TimedOut { .. } => FailureClass::Timeout,
        CodexTaskExecutionError::EmptyPrompt
        | CodexTaskExecutionError::WorkspaceExecution { .. }
        | CodexTaskExecutionError::TruncatedStream { .. }
        | CodexTaskExecutionError::MissingExitStatus => FailureClass::Unknown,
    }
}

/// Classifies a completed process result without interpreting its output.
pub fn classify_codex_task_execution_result(
    result: &CodexTaskExecutionResult,
) -> Option<FailureClass> {
    (result.exit_code() != 0).then_some(FailureClass::Unknown)
}
