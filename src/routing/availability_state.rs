use crate::AvailabilityStateNonTemporalCore;
use crate::policy_eligibility::ModelRoutingDateTimeV1;

/// Complete in-process availability storage with a caller-supplied observation timestamp.
/// Preserves both values unchanged; no temporal policy or wire codec.
///
/// Both the non-temporal core and the validated timestamp are required.
///
/// ```compile_fail
/// # use receipts_model_routing::{AvailabilityState, AvailabilityStateNonTemporalCore};
/// # fn forbidden(core: AvailabilityStateNonTemporalCore) {
/// AvailabilityState::new(core);
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::AvailabilityState;
/// # use receipts_model_routing::policy_eligibility::ModelRoutingDateTimeV1;
/// # fn forbidden(observed_at: ModelRoutingDateTimeV1) {
/// AvailabilityState::new(observed_at);
/// # }
/// ```
/// Fields are private and accessors expose only shared references.
///
/// ```compile_fail
/// # use receipts_model_routing::{AvailabilityState, AvailabilityStateNonTemporalCore};
/// # use receipts_model_routing::policy_eligibility::ModelRoutingDateTimeV1;
/// # fn forbidden(core: AvailabilityStateNonTemporalCore, observed_at: ModelRoutingDateTimeV1) {
/// let _ = AvailabilityState { core, observed_at };
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::{AvailabilityState, AvailabilityStateNonTemporalCore};
/// # fn forbidden(value: &mut AvailabilityState) {
/// let _: &mut AvailabilityStateNonTemporalCore = value.core();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::AvailabilityState;
/// # use receipts_model_routing::policy_eligibility::ModelRoutingDateTimeV1;
/// # fn forbidden(value: &mut AvailabilityState) {
/// let _: &mut ModelRoutingDateTimeV1 = value.observed_at();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailabilityState {
    core: AvailabilityStateNonTemporalCore,
    observed_at: ModelRoutingDateTimeV1,
}

impl AvailabilityState {
    pub fn new(
        core: AvailabilityStateNonTemporalCore,
        observed_at: ModelRoutingDateTimeV1,
    ) -> Self {
        Self { core, observed_at }
    }

    pub fn core(&self) -> &AvailabilityStateNonTemporalCore {
        &self.core
    }

    pub fn observed_at(&self) -> &ModelRoutingDateTimeV1 {
        &self.observed_at
    }
}
