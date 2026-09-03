mod adapter;
mod auth_status;
mod codex_probe;
mod failure;

#[cfg(test)]
mod adapter_tests;
#[cfg(test)]
mod codex_probe_tests;
#[cfg(test)]
mod conformance_tests;

pub use adapter::RuntimeAdapter;
pub use auth_status::RuntimeAuthStatus;
pub use codex_probe::{
    CODEX_EXEC_HELP_PROBE, CODEX_VERSION_PROBE, CodexCapability, CodexCapabilityEvidence,
    CodexCapabilityProbeReport, CodexProbeChannel, CodexProbeCommand, CodexProbeError,
    CodexProbeKind, CodexProbeObservation, parse_codex_probe,
};
pub use failure::FailureClass;
