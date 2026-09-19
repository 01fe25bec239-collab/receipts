//! Complete Review-owned, in-process IntegrationDecision data.
//! Composes the unchanged non-temporal core with required caller-supplied evidence.
//! No decision making, check execution, clock, temporal policy, or serialization.

use crate::{IntegrationDecisionNonTemporalCore, ReviewDateTimeV1};

/// Immutable composition preserving the supplied core and timestamp exactly.
/// The timestamp is required and must already satisfy ReviewDateTimeV1 validation.
///
/// ```compile_fail
/// # use receipts_review_integration::{IntegrationDecision, IntegrationDecisionNonTemporalCore};
/// # fn forbidden(core: IntegrationDecisionNonTemporalCore) {
/// IntegrationDecision::new(core);
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{IntegrationDecision, IntegrationDecisionNonTemporalCore};
/// # fn forbidden(core: IntegrationDecisionNonTemporalCore) {
/// IntegrationDecision::new(core, None);
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{IntegrationDecision, IntegrationDecisionNonTemporalCore};
/// # fn forbidden(core: IntegrationDecisionNonTemporalCore) {
/// IntegrationDecision::new(core, String::from("2026-09-08T00:00:00Z"));
/// # }
/// ```
/// Fields are private; both accessors return shared references only.
///
/// ```compile_fail
/// # use receipts_review_integration::{IntegrationDecision, IntegrationDecisionNonTemporalCore, ReviewDateTimeV1};
/// # fn forbidden(core: IntegrationDecisionNonTemporalCore, decided_at: ReviewDateTimeV1) {
/// let _ = IntegrationDecision { core, decided_at };
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{IntegrationDecision, IntegrationDecisionNonTemporalCore};
/// # fn forbidden(value: &mut IntegrationDecision, core: IntegrationDecisionNonTemporalCore) {
/// value.core = core;
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{IntegrationDecision, ReviewDateTimeV1};
/// # fn forbidden(value: &mut IntegrationDecision, timestamp: ReviewDateTimeV1) {
/// value.decided_at = timestamp;
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{IntegrationDecision, ReviewDateTimeV1};
/// # fn forbidden(value: &mut IntegrationDecision) {
/// let _: &mut ReviewDateTimeV1 = value.decided_at();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{IntegrationDecision, IntegrationDecisionNonTemporalCore};
/// # fn forbidden(value: &mut IntegrationDecision) {
/// let _: &mut IntegrationDecisionNonTemporalCore = value.core();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecision) {
/// value.decided_at().0.clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecision) {
/// let _: &mut str = value.decided_at().as_str();
/// # }
/// ```
/// No default or comparison semantics are provided.
///
/// ```compile_fail
/// let _ = receipts_review_integration::IntegrationDecision::default();
/// ```
/// ```compile_fail
/// # use receipts_review_integration::IntegrationDecision;
/// # fn forbidden(a: IntegrationDecision, b: IntegrationDecision) {
/// let _ = a == b;
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::IntegrationDecision;
/// # fn forbidden(a: IntegrationDecision, b: IntegrationDecision) {
/// let _ = a < b;
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct IntegrationDecision {
    core: IntegrationDecisionNonTemporalCore,
    decided_at: ReviewDateTimeV1,
}

impl IntegrationDecision {
    /// Stores already-supplied decision evidence without inference or execution.
    pub fn new(core: IntegrationDecisionNonTemporalCore, decided_at: ReviewDateTimeV1) -> Self {
        Self { core, decided_at }
    }

    pub fn core(&self) -> &IntegrationDecisionNonTemporalCore {
        &self.core
    }

    pub fn decided_at(&self) -> &ReviewDateTimeV1 {
        &self.decided_at
    }
}
