//! Canonical shared Review data: one type and validator implementation for the
//! ReviewCapsule closure. receipts-review-integration re-exports every item at
//! its original root and module paths; codecs and controllers stay there.

pub mod date_time;
pub use date_time::{ReviewDateTimeError, ReviewDateTimeV1};

pub mod review_request_integer;
pub use review_request_integer::{ReviewRequestConstructionError, ReviewRequestNonNegativeInteger};

pub mod review_capsule_structured_core;
pub use review_capsule_structured_core::{
    ReviewCapsuleCheck, ReviewCapsuleCheckResult, ReviewCapsuleConstructionError,
    ReviewCapsuleCriterion, ReviewCapsuleCriterionKind, ReviewCapsuleNonTemporalCore,
    ReviewCapsuleReviewScope, ReviewCapsuleSeverityPolicy,
};
