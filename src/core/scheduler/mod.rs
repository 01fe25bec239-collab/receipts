//! Owned scheduler namespace.
//!
//! Dispatch-admission evidence and validated non-temporal composition.
//! No scheduler loop, policy engine, or operational dispatch.

pub mod dispatch_admission_axis_references;
pub mod dispatch_admission_core;
pub mod dispatch_admission_decision;
pub mod dispatch_admission_decision_non_temporal_core;
pub mod dispatch_admission_derivation;
pub mod dispatch_admission_provider_auth_axis;
pub mod dispatch_admission_provider_policy_axis;
pub mod dispatch_admission_vocabulary;
pub mod write_scope_port;

pub use dispatch_admission_axis_references::{
    DispatchAdmissionAxisReferenceError, DispatchAdmissionEntitlementAxisResult,
    DispatchAdmissionProviderAvailabilityAxisResult, DispatchAdmissionQualityFloorAxisResult,
    DispatchAdmissionSafetyAxisResult,
};
pub use dispatch_admission_core::{
    DispatchAdmissionDecisionCore, DispatchAdmissionDecisionCoreError,
};
pub use dispatch_admission_decision::DispatchAdmissionDecision;
pub use dispatch_admission_decision_non_temporal_core::{
    DispatchAdmissionAxisResults, DispatchAdmissionDecisionNonTemporalCore,
    DispatchAdmissionDecisionNonTemporalCoreError,
};
pub use dispatch_admission_derivation::{
    AdmissionBindingV1, AvailabilityEvidenceV1, AvailabilityObservationV1,
    DispatchAdmissionDerivationV1, DispatchAdmissionRefusalV1, DispatchAdmissionRequestV1,
    EntitlementEvidenceV1, EvidenceFreshnessV1, PRIMARY_FAILURE_PRECEDENCE, ProviderAuthEvidenceV1,
    ProviderConnectionV1, ProviderPolicyEvidenceV1, QualityFloorEvidenceV1, QualityFloorResultV1,
    SafetyEvidenceV1, SafetyStateV1, derive_dispatch_admission,
};
pub use dispatch_admission_provider_auth_axis::DispatchAdmissionProviderAuthAxisResult;
pub use dispatch_admission_provider_policy_axis::{
    DispatchAdmissionProviderPolicyAxisError, DispatchAdmissionProviderPolicyAxisResult,
    DispatchAdmissionProviderPolicyStatus,
};
pub use dispatch_admission_vocabulary::{
    DispatchAdmissionAxisResult, DispatchAdmissionDenialReason, DispatchAdmissionFailingAxis,
    DispatchAdmissionOutcome,
};
pub use write_scope_port::{
    WriteScopeCollisionV1, WriteScopeOverlapV1, WriteScopePort, classify_write_collision,
};

#[cfg(test)]
mod dispatch_admission_axis_references_tests;
#[cfg(test)]
mod dispatch_admission_core_tests;
#[cfg(test)]
mod dispatch_admission_decision_non_temporal_core_tests;
#[cfg(test)]
mod dispatch_admission_decision_tests;
#[cfg(test)]
mod dispatch_admission_derivation_tests;
#[cfg(test)]
mod dispatch_admission_provider_auth_axis_tests;
#[cfg(test)]
mod dispatch_admission_vocabulary_tests;
#[cfg(test)]
mod write_scope_port_tests;
