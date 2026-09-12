//! Review Integration: closed vocabularies and a non-temporal structured core.

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
