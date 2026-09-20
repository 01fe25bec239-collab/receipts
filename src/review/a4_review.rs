//! Review-owned, in-process A4Review with optional reviewed_at evidence.
//! Reproduction check timestamps remain deferred; no wire completeness is claimed.
//! Physical authority: BUILD-A1-ADR-A4REVIEW-OPTIONAL-REVIEWED-AT-PHYSICAL-V1-001.
//! Composes the unchanged non-temporal core with an optional validated timestamp.
//! No execution, clock, defaults, temporal policy, or wire serialization.

use crate::{A4ReviewNonTemporalCore, ReviewDateTimeV1};

/// Immutable composition preserving caller-supplied values exactly.
/// `None` means the optional property is absent; `Some` contains a validated
/// date-time. There is no null carrier. The explicit option has no default.
///
/// ```compile_fail
/// # use receipts_review_integration::{A4Review, A4ReviewNonTemporalCore};
/// # fn forbidden(core: A4ReviewNonTemporalCore) {
/// A4Review::new(core);
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{A4Review, A4ReviewNonTemporalCore};
/// # fn forbidden(core: A4ReviewNonTemporalCore) {
/// A4Review::new(core, Some(None));
/// # }
/// ```
/// Raw strings cannot bypass the canonical date-time validation boundary.
///
/// ```compile_fail
/// # use receipts_review_integration::{A4Review, A4ReviewNonTemporalCore};
/// # fn forbidden(core: A4ReviewNonTemporalCore) {
/// A4Review::new(core, Some(String::from("invalid")));
/// # }
/// ```
/// Fields are private and accessors expose shared references only.
///
/// ```compile_fail
/// # use receipts_review_integration::{A4Review, A4ReviewNonTemporalCore};
/// # fn forbidden(core: A4ReviewNonTemporalCore) {
/// let _ = A4Review { core, reviewed_at: None };
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{A4Review, A4ReviewNonTemporalCore};
/// # fn forbidden(value: &mut A4Review, core: A4ReviewNonTemporalCore) {
/// value.core = core;
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A4Review) {
/// value.reviewed_at = None;
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{A4Review, ReviewDateTimeV1};
/// # fn forbidden(value: &mut A4Review) {
/// let _: Option<&mut ReviewDateTimeV1> = value.reviewed_at();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{A4Review, A4ReviewNonTemporalCore};
/// # fn forbidden(value: &mut A4Review) {
/// let _: &mut A4ReviewNonTemporalCore = value.core();
/// # }
/// ```
/// Timestamp internals remain private and immutable through the wrapper.
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A4Review) {
/// value.reviewed_at().unwrap().0.clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A4Review) {
/// let _: &mut str = value.reviewed_at().unwrap().as_str();
/// # }
/// ```
/// Reproduction check timestamps remain deferred.
///
/// ```compile_fail
/// # fn forbidden(check: &receipts_review_integration::A4ReviewReproductionCheck) {
/// check.started_at();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(check: &receipts_review_integration::A4ReviewReproductionCheck) {
/// check.finished_at();
/// # }
/// ```
/// No comparison, ordering, arithmetic, or default timestamp is provided.
///
/// ```compile_fail
/// # use receipts_review_integration::A4Review;
/// # fn forbidden(a: A4Review, b: A4Review) {
/// let _ = a == b;
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::A4Review;
/// # fn forbidden(a: A4Review, b: A4Review) {
/// let _ = a < b;
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::A4Review;
/// # fn forbidden(a: A4Review, b: A4Review) {
/// let _ = a - b;
/// # }
/// ```
/// ```compile_fail
/// let _ = receipts_review_integration::A4Review::default();
/// ```
#[derive(Debug, Clone)]
pub struct A4Review {
    core: A4ReviewNonTemporalCore,
    reviewed_at: Option<ReviewDateTimeV1>,
}

impl A4Review {
    /// Stores the core and explicit optional timestamp without inference.
    pub fn new(core: A4ReviewNonTemporalCore, reviewed_at: Option<ReviewDateTimeV1>) -> Self {
        Self { core, reviewed_at }
    }

    pub fn core(&self) -> &A4ReviewNonTemporalCore {
        &self.core
    }

    pub fn reviewed_at(&self) -> Option<&ReviewDateTimeV1> {
        self.reviewed_at.as_ref()
    }
}
