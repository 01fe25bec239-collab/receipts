//! Review Integration: closed vocabularies and structured physical data.

pub mod a4_review_vocabulary;

pub use a4_review_vocabulary::{
    A4ReviewDimension, A4ReviewDimensionAssessment, A4ReviewFindingCategory,
    A4ReviewFindingConfidence, A4ReviewFindingSeverity, A4ReviewFindingSource,
    A4ReviewRecommendedAction, A4ReviewVerdict,
};

#[cfg(test)]
mod a4_review_vocabulary_tests;

pub mod a4_review_structured_core;
pub use a4_review_structured_core::{
    A4ReviewConstructionError, A4ReviewDimensionReview, A4ReviewFinding, A4ReviewNonTemporalCore,
    A4ReviewReproduction, A4ReviewReproductionCheck, A4ReviewReproductionCheckResult,
    A4ReviewReproductionLimitation, A4ReviewReviewer,
};

#[cfg(test)]
mod a4_review_structured_core_tests;

pub mod review_capsule_structured_core;
pub use review_capsule_structured_core::{
    ReviewCapsuleCheck, ReviewCapsuleCheckResult, ReviewCapsuleConstructionError,
    ReviewCapsuleCriterion, ReviewCapsuleCriterionKind, ReviewCapsuleNonTemporalCore,
    ReviewCapsuleReviewScope, ReviewCapsuleSeverityPolicy,
};

#[cfg(test)]
mod review_capsule_structured_core_tests;

pub mod a3_handoff_structured_core;
pub use a3_handoff_structured_core::{
    A3HandoffBlocker, A3HandoffCheck, A3HandoffCheckResult, A3HandoffConstructionError,
    A3HandoffContractConsumed, A3HandoffEvidenceLabel, A3HandoffImplementer,
    A3HandoffLabeledEvidence, A3HandoffNonTemporalCore,
};

#[cfg(test)]
mod a3_handoff_structured_core_tests;

pub mod repair_cycle_control;
pub use repair_cycle_control::{
    DEFAULT_MAX_REPAIR_ATTEMPTS, DeterministicRepairCycleControlCore, RepairCycleControlError,
    RepairCycleControlResult, RepairCycleDisposition,
};

#[cfg(test)]
mod repair_cycle_control_tests;

pub mod exact_sha_acceptance_gate;
pub use exact_sha_acceptance_gate::{
    AcceptanceEvidenceRecord, DependencyShaFreshnessLink, ExactShaAcceptanceGate,
    ExactShaAcceptanceGateError, ExactShaAcceptanceGateInput, ExactShaAcceptanceGatePass,
};

#[cfg(test)]
mod exact_sha_acceptance_gate_tests;

pub mod assurance_profile;
pub use assurance_profile::{
    AssuranceProfile, AssuranceProfileRequirements, AssuranceReviewerQualityFloor,
    AssuranceTaskCategory, DistinctProviderPolicy, SecurityPipelinePolicy,
    default_for_task_category, is_assurance_profile_blocking_floor,
};

#[cfg(test)]
mod assurance_profile_tests;

pub mod review_request_structured_core;
pub use review_request_structured_core::{
    ReviewRequestAttemptNumber, ReviewRequestConstructionError, ReviewRequestNonNegativeInteger,
    ReviewRequestNonTemporalCore, ReviewRequestPolicy, ReviewRequestReviewerFloor,
};

#[cfg(test)]
mod review_request_structured_core_tests;

pub mod integration_decision_structured_core;
pub use integration_decision_structured_core::{
    IntegrationDecisionCheck, IntegrationDecisionCheckResult, IntegrationDecisionConstructionError,
    IntegrationDecisionNonTemporalCore, IntegrationDecisionNullableString,
    IntegrationDecisionOutcome, IntegrationDecisionProvenance,
};

#[cfg(test)]
mod integration_decision_structured_core_tests;

pub mod integration_request_structured_core;
pub use integration_request_structured_core::{
    IntegrationRequestA4Verdict, IntegrationRequestAttestations,
    IntegrationRequestConstructionError, IntegrationRequestGateLevel,
    IntegrationRequestNonTemporalCore, IntegrationRequestOpenFinding,
    IntegrationRequestPositiveInteger, IntegrationRequestPostMergeCheck,
    IntegrationRequestSignedInteger, IntegrationRequestTask,
};

#[cfg(test)]
mod integration_request_structured_core_tests;

pub mod safety_interruption_structured_core;
pub use safety_interruption_structured_core::{
    SafetyInterruptionConstructionError, SafetyInterruptionDetectionConfidence,
    SafetyInterruptionNonTemporalCore, SafetyInterruptionState, SafetyInterruptionTerminalOutcome,
};

#[cfg(test)]
mod safety_interruption_structured_core_tests;

pub mod date_time;
pub use date_time::{ReviewDateTimeError, ReviewDateTimeV1};

#[cfg(test)]
mod date_time_tests;

pub mod safety_interruption;
pub use safety_interruption::SafetyInterruption;

#[cfg(test)]
mod safety_interruption_tests;

pub mod review_request;
pub use review_request::ReviewRequest;

#[cfg(test)]
mod review_request_tests;

pub mod integration_decision;
pub use integration_decision::IntegrationDecision;

#[cfg(test)]
mod integration_decision_tests;

pub mod a4_review;
pub use a4_review::A4Review;

#[cfg(test)]
mod a4_review_tests;
