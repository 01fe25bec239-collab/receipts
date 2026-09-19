mod availability_state;
mod availability_state_core;
mod availability_vocabulary;
mod candidate_eligibility;
mod candidate_identity_constraint_composition;
mod candidate_identity_constraints;
mod candidate_set;
mod quota_state;

#[cfg(test)]
mod quota_state_tests;

pub use quota_state::{QuotaScope, QuotaStateRequiredCore};

#[cfg(test)]
mod candidate_eligibility_tests;
#[cfg(test)]
mod candidate_identity_constraint_composition_tests;
#[cfg(test)]
mod candidate_identity_constraints_tests;
#[cfg(test)]
mod candidate_set_tests;

pub use candidate_eligibility::{
    CandidateEligibilityOutcome, CandidateEligibilityRejection, evaluate_candidate_eligibility,
};
pub use candidate_identity_constraint_composition::{
    RegistryCandidateIdentityConstraintAssessment, evaluate_registry_candidate_identity_constraints,
};
pub use candidate_identity_constraints::{
    CandidateIdentityConstraintOutcome, CandidateIdentityConstraintRejection,
    evaluate_candidate_identity_constraints,
};
pub use candidate_set::{RegistryCandidateIdentity, enumerate_registry_candidates};

#[cfg(test)]
mod availability_state_core_tests;
#[cfg(test)]
mod availability_state_tests;
#[cfg(test)]
mod availability_vocabulary_tests;

pub use availability_state::AvailabilityState;
pub use availability_state_core::{AvailabilityStateCoreError, AvailabilityStateNonTemporalCore};
pub use availability_vocabulary::{AvailabilitySignalSource, AvailabilityStateKind};

mod routing_request;
#[cfg(test)]
mod routing_request_tests;

mod routing_decision;
#[cfg(test)]
mod routing_decision_tests;

pub use routing_decision::{
    AlternativeCandidate, AlternativeCandidateError, CapabilityEvidenceNonTemporalCore,
    DecisionConfidence, EstimatedCostClass, EvidenceConfidence, EvidenceSourceRef,
    EvidenceSourceRefError, EvidenceSourceRefType, RegistryFreshness, RoutingDecisionCoreError,
    RoutingDecisionMode, RoutingDecisionNonTemporalCore, RoutingDecisionOutcome,
    RoutingScoreComponents, RoutingScoreComponentsError, SelectionReason,
};

pub use routing_request::{
    RoutingPriority, RoutingQualityFloor, RoutingRequestConstraints,
    RoutingRequestConstraintsError, RoutingRequestCoreError, RoutingRequestNonTemporalCore,
    RoutingRequestRole, RoutingTaskClass,
};

pub mod policy_eligibility;

#[path = "../intelligence/mod.rs"]
pub mod intelligence;
#[path = "../registry/mod.rs"]
pub mod registry;
