//! Complete Review-owned, in-process ReviewRequest machine-shape data.
//! Physical authority: BUILD-A1-ADR-REVIEWREQUEST-OPTIONAL-REQUESTED-AT-PHYSICAL-V1-001.
//! Composes the unchanged non-temporal core with an optional validated timestamp.
//! No execution, clock, defaults, temporal policy, or wire serialization.

use crate::{ReviewDateTimeV1, ReviewRequestNonTemporalCore};

/// Immutable composition preserving caller-supplied values exactly.
/// `None` means the optional property is absent; `Some` contains a validated
/// date-time. There is no null carrier. The explicit option has no default.
///
/// ```compile_fail
/// # use receipts_review_integration::{ReviewRequest, ReviewRequestNonTemporalCore};
/// # fn forbidden(core: ReviewRequestNonTemporalCore) {
/// ReviewRequest::new(core);
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{ReviewRequest, ReviewRequestNonTemporalCore};
/// # fn forbidden(core: ReviewRequestNonTemporalCore) {
/// ReviewRequest::new(core, Some(None));
/// # }
/// ```
/// Raw strings cannot bypass the canonical date-time validation boundary.
///
/// ```compile_fail
/// # use receipts_review_integration::{ReviewRequest, ReviewRequestNonTemporalCore};
/// # fn forbidden(core: ReviewRequestNonTemporalCore) {
/// ReviewRequest::new(core, Some(String::from("invalid")));
/// # }
/// ```
/// Fields are private and accessors expose shared references only.
///
/// ```compile_fail
/// # use receipts_review_integration::{ReviewRequest, ReviewRequestNonTemporalCore};
/// # fn forbidden(core: ReviewRequestNonTemporalCore) {
/// let _ = ReviewRequest { core, requested_at: None };
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{ReviewRequest, ReviewRequestNonTemporalCore};
/// # fn forbidden(value: &mut ReviewRequest, core: ReviewRequestNonTemporalCore) {
/// value.core = core;
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequest) {
/// value.requested_at = None;
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{ReviewRequest, ReviewDateTimeV1};
/// # fn forbidden(value: &mut ReviewRequest) {
/// let _: Option<&mut ReviewDateTimeV1> = value.requested_at();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::{ReviewRequest, ReviewRequestNonTemporalCore};
/// # fn forbidden(value: &mut ReviewRequest) {
/// let _: &mut ReviewRequestNonTemporalCore = value.core();
/// # }
/// ```
/// No comparison, ordering, arithmetic, or default timestamp is provided.
///
/// ```compile_fail
/// # use receipts_review_integration::ReviewRequest;
/// # fn forbidden(a: ReviewRequest, b: ReviewRequest) {
/// let _ = a == b;
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::ReviewRequest;
/// # fn forbidden(a: ReviewRequest, b: ReviewRequest) {
/// let _ = a < b;
/// # }
/// ```
/// ```compile_fail
/// # use receipts_review_integration::ReviewRequest;
/// # fn forbidden(a: ReviewRequest, b: ReviewRequest) {
/// let _ = a - b;
/// # }
/// ```
/// ```compile_fail
/// let _ = receipts_review_integration::ReviewRequest::default();
/// ```
#[derive(Debug, Clone)]
pub struct ReviewRequest {
    core: ReviewRequestNonTemporalCore,
    requested_at: Option<ReviewDateTimeV1>,
}

impl ReviewRequest {
    /// Stores the core and explicit optional timestamp without inference.
    pub fn new(core: ReviewRequestNonTemporalCore, requested_at: Option<ReviewDateTimeV1>) -> Self {
        Self { core, requested_at }
    }

    pub fn core(&self) -> &ReviewRequestNonTemporalCore {
        &self.core
    }

    pub fn requested_at(&self) -> Option<&ReviewDateTimeV1> {
        self.requested_at.as_ref()
    }
}
