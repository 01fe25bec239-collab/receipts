mod attempt_types;
mod claude_adapter;
mod policy;
pub use attempt_types::{ClaudeAttemptEvent, ClaudeEventError, ClaudeEventStream};
pub use claude_adapter::{
    CancelReason, ClaudeAdapter, ClaudeAttemptHandle, ClaudeAttemptResult, RuntimeObservation,
};
pub use policy::RuntimeExecutionPolicy;
mod adapter;
mod attempt;
mod auth_status;
mod claude_execution;
mod claude_probe;
mod claude_probe_execution;
#[cfg(all(test, unix))]
mod claude_probe_execution_tests;
#[cfg(test)]
mod claude_probe_tests;
mod claude_stream_json;
#[cfg(test)]
mod claude_stream_json_tests;
mod codex_auth_status;
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
mod runtime_capabilities;

#[cfg(test)]
mod adapter_tests;
#[cfg(test)]
mod attempt_tests;
#[cfg(all(test, unix))]
mod codex_auth_status_tests;
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
#[cfg(test)]
mod runtime_capabilities_tests;
#[cfg(test)]
mod third_adapter_capabilities_tests;

#[cfg(all(test, unix))]
mod claude_tests;

pub use claude_execution::{
    ClaudeAuthStatusError, ClaudeLiveAttempt, ClaudeLiveOutcome, ClaudeLiveOutputSnapshot,
    ClaudeLiveStartError, ClaudePermissionMode, ClaudeTaskExecutionError,
    ClaudeTaskExecutionRequest, ClaudeTaskExecutionResult, classify_claude_live_outcome,
    classify_claude_task_execution_error, classify_claude_task_execution_result,
    execute_claude_task_once, observe_claude_auth_status, start_claude_live_attempt,
};
pub use claude_stream_json::{
    ClaudeProviderResult, ClaudeResult, ClaudeResultError, ClaudeResultSubtype, ClaudeResultUsage,
    ClaudeStreamJson, ClaudeStreamJsonError, ClaudeStreamJsonRecord,
};

pub use adapter::RuntimeAdapter;
pub use attempt::{AttemptHandle, AttemptId, AttemptIdError, AttemptResult};
pub use failure_classification::classify_live_outcome;
pub use raw_failure::{
    FailureChannel, RawFailure, RawFailureEvidence, RawFailureSource, RuntimeFailureOrigin,
    RuntimeId, RuntimeIdError,
};
mod failure_classification;
#[cfg(all(test, unix))]
mod failure_classification_tests;
/// Type-erased registry; a separate module so its trait methods never shadow
/// `RuntimeAdapter` methods under crate-root glob imports.
pub mod registry;
#[cfg(all(test, unix))]
mod third_adapter_lifecycle_tests;

pub use auth_status::RuntimeAuthStatus;
pub use codex_auth_status::{
    CodexAuthStatusError, classify_codex_auth_status_error, observe_codex_auth_status,
};
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
pub use runtime_capabilities::{
    CapabilityStatus, ExtensionCapabilityReport, ExtensionCapabilityReportError,
    RuntimeCapabilities,
};

pub use claude_probe::{
    ClaudeCapability, ClaudeCapabilityEvidence, ClaudeCapabilityProbeReport, ClaudeProbeChannel,
    ClaudeProbeError, ClaudeProbeKind, ClaudeProbeObservation, parse_claude_probe,
};
pub use claude_probe_execution::{
    ClaudeProbeExecutionError, classify_claude_probe_execution_error,
    execute_claude_capability_probe,
};

#[cfg(all(test, unix))]
mod claude_adapter_tests;

mod codex_adapter;
pub use attempt_types::{CodexAttemptEvent, CodexEventError, CodexEventStream};
pub use codex_adapter::{CodexAdapter, CodexAttemptHandle, CodexAttemptResult};

#[cfg(all(test, unix))]
mod codex_adapter_tests;
