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
//!   and an enumerated non-worker CLI probe; no shell or filesystem writes;
//! * no installation, hook mutation, trust approval, credential authority,
//!   networking, rendering, event normalization, or runtime worker execution;
//! * adjacent frozen contracts (`InstallPlan`, `CoreHandle`,
//!   `NormalizedHostEvent`, `CoreView`, `UserPrompt`, `UserResponse`,
//!   `HostCapabilityReport`, shutdown reason) remain externally owned and
//!   appear here only as unbound associated-type placeholders;
//! * no authoritative state read or write path exists here.

pub mod adapter;
pub mod host_capability_consistency;
pub mod host_capability_freshness_policy;
pub mod host_capability_freshness_vocabulary;
pub mod host_capability_inactive_reason_policy;
pub mod host_capability_mode_override;
pub mod host_capability_mode_selection;
pub mod host_capability_native_prerequisite;
pub mod host_capability_observation;
pub mod host_capability_report_core;
pub mod host_capability_report_refresh;
pub mod host_capability_report_selection_composition;
pub mod host_capability_report_vocabulary;
pub mod host_capability_selected_mode_consistency;
pub mod host_detection;
pub mod host_id;
pub mod host_session_activation;
pub mod normalized_host_event;
pub mod normalized_host_event_source_class;

#[cfg(test)]
mod adapter_tests;

#[cfg(test)]
mod conformance_tests;

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
mod normalized_host_event_source_class_tests;

pub use adapter::HostAdapter;
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
pub use host_id::HostId;
pub use normalized_host_event::{NormalizedHostEventConfidence, NormalizedHostEventType};
pub use normalized_host_event_source_class::NormalizedHostEventSourceClass;

pub use host_session_activation::{
    HostSessionActivationError, HostSessionActivationOutcome, HostSessionActivationRequest,
    prepare_host_session,
};
