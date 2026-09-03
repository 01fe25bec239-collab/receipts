mod adapter;
mod auth_status;
mod codex_probe;
mod codex_probe_execution;
mod codex_task_execution;
mod failure;

#[cfg(test)]
mod adapter_tests;
#[cfg(test)]
mod codex_probe_execution_tests;
#[cfg(test)]
mod codex_probe_tests;
#[cfg(test)]
mod codex_task_execution_tests;
#[cfg(test)]
mod conformance_tests;

pub use adapter::RuntimeAdapter;
pub use auth_status::RuntimeAuthStatus;
pub use codex_probe::{
    CODEX_EXEC_HELP_PROBE, CODEX_VERSION_PROBE, CodexCapability, CodexCapabilityEvidence,
    CodexCapabilityProbeReport, CodexProbeChannel, CodexProbeCommand, CodexProbeError,
    CodexProbeKind, CodexProbeObservation, parse_codex_probe,
};
pub use codex_probe_execution::{CodexProbeExecutionError, execute_codex_capability_probe};
pub use codex_task_execution::{
    CodexTaskExecutionError, CodexTaskExecutionRequest, CodexTaskExecutionResult,
    CodexTaskOutputChannel, CodexTaskSandboxMode, execute_codex_task_once,
};
pub use failure::FailureClass;
