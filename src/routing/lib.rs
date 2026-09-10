mod availability_state_core;
mod availability_vocabulary;

#[cfg(test)]
mod availability_state_core_tests;
#[cfg(test)]
mod availability_vocabulary_tests;

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
