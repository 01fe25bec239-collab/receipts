mod adapter;
mod attempt;
mod auth_status;
mod claude_execution;
mod claude_stream_json;
mod codex_failure_classification;
mod codex_jsonl;
mod codex_live_attempt;
mod codex_probe;
mod codex_probe_execution;
mod codex_probe_failure_classification;
mod codex_task_execution;
mod failure;
mod jsonl;
mod raw_failure;

#[cfg(test)]
mod adapter_tests;
#[cfg(test)]
mod attempt_tests;
#[cfg(test)]
mod codex_failure_classification_tests;
#[cfg(test)]
mod codex_jsonl_tests;
#[cfg(all(test, unix))]
mod codex_live_attempt_tests;
#[cfg(test)]
mod codex_probe_execution_tests;
#[cfg(test)]
mod codex_probe_failure_classification_tests;
#[cfg(test)]
mod codex_probe_tests;
#[cfg(test)]
mod codex_task_execution_tests;
#[cfg(test)]
mod conformance_tests;
#[cfg(test)]
mod raw_failure_tests;

#[cfg(all(test, unix))]
mod claude_tests;

pub use claude_execution::{
    ClaudeAuthStatusError, ClaudeLiveAttempt, ClaudeLiveOutcome, ClaudeLiveOutputSnapshot,
    ClaudeLiveStartError, ClaudePermissionMode, ClaudeTaskExecutionError,
    ClaudeTaskExecutionRequest, ClaudeTaskExecutionResult, classify_claude_live_outcome,
    classify_claude_task_execution_error, classify_claude_task_execution_result,
    execute_claude_task_once, observe_claude_auth_status, start_claude_live_attempt,
};
pub use claude_stream_json::{ClaudeStreamJson, ClaudeStreamJsonError, ClaudeStreamJsonRecord};

pub use adapter::RuntimeAdapter;
pub use attempt::{AttemptHandle, AttemptId, AttemptIdError, AttemptResult};
pub use raw_failure::{RawFailure, RawFailureEvidence, RawFailureSource};

/// Codex technical capability binding: the existing probe report, not a mirror.
/// Supported/UNKNOWN evidence remains probe-derived; no auth, policy, entitlement
/// or routing inference is added. Production probe capture bounds retained version
/// evidence; direct caller construction retains the existing report's contract.
/// Probe failure remains `CodexProbeExecutionError`, never a fabricated report.
pub type RuntimeCapabilities = CodexCapabilityProbeReport;
pub use auth_status::RuntimeAuthStatus;
pub use codex_failure_classification::{
    classify_codex_task_execution_error, classify_codex_task_execution_result,
};
pub use codex_jsonl::{
    CodexJsonlError, CodexJsonlErrorKind, CodexJsonlEvent, CodexJsonlEventKind,
    CodexJsonlInterpretation, CodexJsonlProtocol, CodexProtocolTermination, interpret_codex_jsonl,
};
pub use codex_live_attempt::{
    CodexLiveAttempt, CodexLiveOutcome, CodexLiveOutputSnapshot, CodexLiveProtocolError,
    CodexLiveStartError, start_codex_live_attempt,
};
pub use codex_probe::{
    CODEX_EXEC_HELP_PROBE, CODEX_VERSION_PROBE, CodexCapability, CodexCapabilityEvidence,
    CodexCapabilityProbeReport, CodexProbeChannel, CodexProbeCommand, CodexProbeError,
    CodexProbeKind, CodexProbeObservation, parse_codex_probe,
};
pub use codex_probe_execution::{CodexProbeExecutionError, execute_codex_capability_probe};
pub use codex_probe_failure_classification::classify_codex_probe_execution_error;
pub use codex_task_execution::{
    CodexTaskExecutionError, CodexTaskExecutionRequest, CodexTaskExecutionResult,
    CodexTaskOutputChannel, CodexTaskSandboxMode, execute_codex_task_once,
};
pub use failure::FailureClass;
