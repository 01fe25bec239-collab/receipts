//! Host Integration foundation: the host-neutral [`HostAdapter`]
//! translation boundary and the pure host detection/selection policy.
//!
//! This crate implements the interface and bounded read-only observation slice of `M-HOST-1`: the frozen
//! architectural boundary between the Receipts core and external hosts
//! (Claude Code, Codex, headless runners). An adapter is a translation
//! boundary and nothing else. It contains no orchestration, routing, state,
//! review, workspace, or runtime-worker logic, and it never gains authority
//! merely because an interface method exists.
//!
//! The [`host_detection`] module adds the pure resolution policy over
//! caller-supplied host-presence facts: explicit override first, then
//! single-host automatic detection, Headless as the non-host fallback, and
//! a typed failure for ambiguous Claude + Codex detection. It observes
//! nothing itself.
//!
//! Boundary rules honored by this crate:
//!
//! * bounded source-specific JSON/TOML decoding with the authorized serde,
//!   serde_json, and toml substrate;
//! * read-only Host capability observation through fixed configuration paths
//!   and the composition's probe port; no shell and no settings writes;
//! * [`claude`] binds the frozen `HostAdapter` placeholders for Claude Code
//!   and maps bounded, untrusted hook stdin to frozen normalized events;
//! * [`host_event_spool`] is the only write path: owner-checked append-only
//!   `0600` lines under `<project>/.receipts/inbox/`. It never opens State;
//! * [`wire`] holds the strict `HostCapabilityReport`/`NormalizedHostEvent`
//!   JSON codecs;
//! * no installation, hook mutation, trust approval, credential authority,
//!   networking, process launch, or runtime worker execution;
//! * [`render_graph_snapshot`] formats supplied snapshot evidence only;
//! * no authoritative state read or write path exists here.

pub mod adapter;
pub mod claude;
pub mod graph_snapshot_renderer;
pub mod host_capability_consistency;
pub mod host_capability_freshness_policy;
pub mod host_capability_freshness_vocabulary;
pub mod host_capability_inactive_reason_policy;
pub mod host_capability_mode_override;
pub mod host_capability_mode_selection;
pub mod host_capability_native_prerequisite;
pub mod host_capability_observation;
pub mod host_capability_report;
pub mod host_capability_report_core;
pub mod host_capability_report_refresh;
pub mod host_capability_report_selection_composition;
pub mod host_capability_report_vocabulary;
pub mod host_capability_selected_mode_consistency;
pub mod host_detection;
pub mod host_event_spool;
pub mod host_id;
pub mod host_probe_port;
pub mod host_session_activation;
pub mod normalized_host_event;
pub mod normalized_host_event_emission;
pub mod normalized_host_event_source_class;
pub mod normalized_host_event_source_policy;
pub mod wire;

#[cfg(test)]
mod adapter_tests;

#[cfg(test)]
mod conformance_tests;

#[cfg(test)]
mod graph_snapshot_renderer_tests;

#[cfg(test)]
mod host_capability_consistency_tests;

#[cfg(test)]
mod host_capability_inactive_reason_policy_tests;

#[cfg(test)]
mod host_capability_freshness_policy_tests;

#[cfg(test)]
mod host_capability_freshness_vocabulary_tests;

#[cfg(test)]
mod host_capability_mode_override_tests;

#[cfg(test)]
mod host_capability_mode_selection_tests;

#[cfg(test)]
mod host_capability_native_prerequisite_tests;

#[cfg(test)]
mod host_capability_report_tests;

#[cfg(test)]
mod host_capability_report_core_tests;

#[cfg(test)]
mod host_capability_report_selection_composition_tests;

#[cfg(test)]
mod host_capability_report_vocabulary_tests;

#[cfg(test)]
mod host_capability_selected_mode_consistency_tests;

#[cfg(test)]
mod host_detection_tests;

#[cfg(test)]
mod host_id_tests;

#[cfg(test)]
mod normalized_host_event_tests;

#[cfg(test)]
mod normalized_host_event_emission_tests;

#[cfg(test)]
mod normalized_host_event_source_class_tests;

#[cfg(test)]
mod normalized_host_event_source_policy_tests;

pub use adapter::HostAdapter;
pub use claude::ClaudeHostAdapter;
pub use graph_snapshot_renderer::render_graph_snapshot;
pub use host_capability_consistency::{
    HostCapabilityConsistencyError, HostCapabilityConsistencyInputs,
    validate_complete_probe_consistency,
};
pub use host_capability_freshness_policy::{
    HostCapabilityFreshnessDisposition, freshness_disposition, is_embedded_eligible,
};
pub use host_capability_freshness_vocabulary::{
    HostCapabilityReprobeTrigger, HostCapabilityValidityInput,
};
pub use host_capability_inactive_reason_policy::{
    HostCapabilityInactiveReasonInputs, native_path_inactive_reason,
};
pub use host_capability_mode_override::{
    HostCapabilityModeOverride, HostCapabilityModeOverrideError,
};
pub use host_capability_mode_selection::{
    HostCapabilityModeSelectionError, HostCapabilityModeSelectionIndeterminacy,
    HostCapabilityModeSelectionInputs, HostCapabilityModeSelectionOutcome,
    select_host_capability_mode,
};
pub use host_capability_native_prerequisite::{
    HostCapabilityNativePrerequisiteInputs, HostCapabilityNativePrerequisiteState,
    assess_native_path_prerequisites,
};
pub use host_capability_report::HostCapabilityReport;
pub use host_capability_report_core::{
    HostCapabilityReportNonTemporalCore, HostCapabilityReportNonTemporalCoreError,
    HostCapabilityReportNonTemporalCoreInputs,
};
pub use host_capability_report_refresh::{
    HostCapabilityReportRefreshOutcome, HostCapabilityReportRefreshRequest,
    refresh_host_capability_report,
};
pub use host_capability_report_selection_composition::{
    HostCapabilityReportSelectionCompositionError, HostCapabilityReportSelectionCompositionInputs,
    HostCapabilityReportSelectionCompositionOutcome, compose_host_capability_report_selection,
};
pub use host_capability_report_vocabulary::{
    HostCapabilityEvidenceLabel, HostCapabilityHookCoverageClass, HostCapabilityInactiveReason,
    HostCapabilityModeOverrideSource, HostCapabilityProbeStatus, HostCapabilitySelectedMode,
    HostCapabilityStaleReason,
};
pub use host_capability_selected_mode_consistency::{
    HostCapabilitySelectedModeConsistencyError, HostCapabilitySelectedModeConsistencyInputs,
    validate_selected_mode_consistency,
};
pub use host_detection::{HostDetectionError, HostDetectionSignals, resolve_host};
pub use host_event_spool::{HostEventSpool, HostEventSpoolError};
pub use host_id::HostId;
pub use normalized_host_event::{
    NormalizedHostEvent, NormalizedHostEventConfidence, NormalizedHostEventError,
    NormalizedHostEventHost, NormalizedHostEventId, NormalizedHostEventInputs,
    NormalizedHostEventRawRef, NormalizedHostEventRawRefType, NormalizedHostEventType,
};
pub use normalized_host_event_emission::{
    NormalizedHostEventEmissionSourceError, emit_validated_normalized_host_event,
    validate_normalized_host_event_source,
};
pub use normalized_host_event_source_class::NormalizedHostEventSourceClass;
pub use normalized_host_event_source_policy::source_class_allowed;

pub use host_probe_port::{
    HostCapabilityCacheEntryV1, HostExecutableIdentityV1, HostObservationFingerprintV1,
    HostProbeCommandV1, HostProbeProcessPort,
};
pub use host_session_activation::{
    HostSessionActivationError, HostSessionActivationOutcome, HostSessionActivationRequest,
    prepare_host_session,
};
