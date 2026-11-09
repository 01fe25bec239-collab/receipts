//! Owned scheduler namespace.
//!
//! Dispatch-admission evidence, the D11 evaluator, the deterministic
//! dispatch scheduler and write-collision resolution. No dispatch loop,
//! routing or execution happens here.

pub mod dispatch_admission_axis_references;
pub mod dispatch_admission_core;
pub mod dispatch_admission_decision;
pub mod dispatch_admission_decision_non_temporal_core;
pub mod dispatch_admission_derivation;
pub mod dispatch_admission_provider_auth_axis;
pub mod dispatch_admission_provider_policy_axis;
pub mod dispatch_admission_vocabulary;
pub mod dispatch_scheduler;
pub mod write_collision;
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
pub use dispatch_scheduler::{
    ConcurrencyLimitsV1, DeferralV1, DispatchBudgetV1, DispatchCandidateV1, DispatchPriorityV1,
    DispatchScheduleV1, InFlightWriterV1, ScheduledCandidateV1, SchedulingContextV1,
    SchedulingDecisionV1, SchedulingRefusalV1, schedule_dispatch_v1,
};
pub use write_collision::{
    CollidingWriterV1, CollisionMutationV1, WorkstreamOwnershipV1, WriteCollisionResolutionErrorV1,
    WriteCollisionResolutionV1, WriteCollisionResolvedV1, resolve_write_collision_v1,
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
mod dispatch_scheduler_tests;
#[cfg(test)]
mod write_collision_tests;
#[cfg(test)]
mod write_scope_port_tests;
