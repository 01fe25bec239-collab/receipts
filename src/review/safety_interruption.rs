//! Complete Review-owned, in-process SafetyInterruption machine-shape data.
//! Composes the unchanged A3-012 core with the required caller-supplied timestamp.
//! No clock, policy, retry, routing, state transitions, or Q-06 tooling behavior.

use crate::{ReviewDateTimeV1, SafetyInterruptionNonTemporalCore};

/// Immutable composition; all non-temporal values are preserved without inference.
/// `HUMAN_REQUIRED` remains caller-supplied data and triggers no action.
///
/// Both the existing core and a validated, non-null timestamp are required.
///
/// ```compile_fail
/// # use receipts_review_integration::{SafetyInterruption, SafetyInterruptionNonTemporalCore};
/// # fn forbidden(core: SafetyInterruptionNonTemporalCore) {
/// SafetyInterruption::new(core);
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{SafetyInterruption, ReviewDateTimeV1};
/// # fn forbidden(observed_at: ReviewDateTimeV1) {
/// SafetyInterruption::new(observed_at);
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{SafetyInterruption, SafetyInterruptionNonTemporalCore};
/// # fn forbidden(core: SafetyInterruptionNonTemporalCore) {
/// SafetyInterruption::new(core, None);
/// # }
/// ```
/// Private fields and shared accessors provide no mutable escape.
///
/// ```compile_fail
/// # use receipts_review_integration::{SafetyInterruption, SafetyInterruptionNonTemporalCore, ReviewDateTimeV1};
/// # fn forbidden(core: SafetyInterruptionNonTemporalCore, observed_at: ReviewDateTimeV1) {
/// let _ = SafetyInterruption { core, observed_at };
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{SafetyInterruption, ReviewDateTimeV1};
/// # fn forbidden(value: &mut SafetyInterruption, observed_at: ReviewDateTimeV1) {
/// value.observed_at = observed_at;
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{SafetyInterruption, ReviewDateTimeV1};
/// # fn forbidden(value: &mut SafetyInterruption) {
/// let _: &mut ReviewDateTimeV1 = value.observed_at();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{SafetyInterruption, SafetyInterruptionNonTemporalCore};
/// # fn forbidden(value: &mut SafetyInterruption) {
/// let _: &mut SafetyInterruptionNonTemporalCore = value.core();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruption) {
/// let _: &mut [receipts_workspace_execution::WorkspaceCheckpointRef] =
///     value.core().preserved_evidence_refs().unwrap();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruption) {
/// value.core().preserved_evidence_refs().unwrap()[0] =
///     value.core().preserved_evidence_refs().unwrap()[1].clone();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyInterruption {
    core: SafetyInterruptionNonTemporalCore,
    observed_at: ReviewDateTimeV1,
}

impl SafetyInterruption {
    pub fn new(core: SafetyInterruptionNonTemporalCore, observed_at: ReviewDateTimeV1) -> Self {
        Self { core, observed_at }
    }

    pub fn core(&self) -> &SafetyInterruptionNonTemporalCore {
        &self.core
    }

    pub fn observed_at(&self) -> &ReviewDateTimeV1 {
        &self.observed_at
    }
}
