//! Owned scheduler namespace.
//!
//! Dispatch-admission evidence and validated non-temporal composition.
//! No scheduler loop, policy engine, or operational dispatch.

pub mod dispatch_admission_axis_references;
pub mod dispatch_admission_core;
pub mod dispatch_admission_decision_non_temporal_core;
pub mod dispatch_admission_provider_auth_axis;
pub mod dispatch_admission_provider_policy_axis;
pub mod dispatch_admission_vocabulary;

pub use dispatch_admission_axis_references::{
    DispatchAdmissionAxisReferenceError, DispatchAdmissionEntitlementAxisResult,
    DispatchAdmissionProviderAvailabilityAxisResult, DispatchAdmissionQualityFloorAxisResult,
    DispatchAdmissionSafetyAxisResult,
};
pub use dispatch_admission_core::{
    DispatchAdmissionDecisionCore, DispatchAdmissionDecisionCoreError,
};
pub use dispatch_admission_decision_non_temporal_core::{
    DispatchAdmissionAxisResults, DispatchAdmissionDecisionNonTemporalCore,
    DispatchAdmissionDecisionNonTemporalCoreError,
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

#[cfg(test)]
mod dispatch_admission_axis_references_tests;
#[cfg(test)]
mod dispatch_admission_core_tests;
#[cfg(test)]
mod dispatch_admission_decision_non_temporal_core_tests;
#[cfg(test)]
mod dispatch_admission_provider_auth_axis_tests;
#[cfg(test)]
mod dispatch_admission_vocabulary_tests;
