//! Physical composition only; this crate owns none of the composed capsule semantics.

mod capsule_family;

pub use capsule_family::RuntimeCapsuleFamily;

#[cfg(test)]
mod capsule_family_tests;

// Canonical owner types only; construction and validation stay with their owners.
pub use receipts_orchestration::capsules::{
    AssuranceProfileSelector, CostPolicy, Criterion, CriterionKind, QualityFloor, Ref, RefType,
    RepairAttemptNumberV1, RepairCapsule, RepairCapsuleFindingV1, RepairContextEpochV1,
    RepairFindingCategoryV1, RepairFindingSeverityV1, TaskCapsule, TaskType,
    VerificationPlanEntryV1,
};
pub use receipts_review_integration::{
    ReviewCapsuleCriterion, ReviewCapsuleCriterionKind, ReviewCapsuleNonTemporalCore,
    ReviewCapsuleReviewScope, ReviewCapsuleSeverityPolicy,
};

pub use receipts_orchestration::graph::CapabilityName;
