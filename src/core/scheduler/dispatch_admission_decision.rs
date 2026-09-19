use super::DispatchAdmissionDecisionNonTemporalCore;
use crate::orchestration::OrchestrationDateTimeV1;

/// Complete dispatch-admission data with a caller-supplied decision time.
/// The validated core and timestamp are stored directly, without evaluation.
///
/// The timestamp cannot be omitted or supplied as `None`:
///
/// ```compile_fail
/// use receipts_orchestration::scheduler::{
///     DispatchAdmissionDecision, DispatchAdmissionDecisionNonTemporalCore,
/// };
/// fn missing(core: DispatchAdmissionDecisionNonTemporalCore) {
///     DispatchAdmissionDecision::new(core);
/// }
/// ```
///
/// ```compile_fail
/// use receipts_orchestration::scheduler::{
///     DispatchAdmissionDecision, DispatchAdmissionDecisionNonTemporalCore,
/// };
/// fn optional(core: DispatchAdmissionDecisionNonTemporalCore) {
///     DispatchAdmissionDecision::new(core, None);
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionDecision {
    core: DispatchAdmissionDecisionNonTemporalCore,
    decided_at: OrchestrationDateTimeV1,
}

impl DispatchAdmissionDecision {
    pub fn new(
        core: DispatchAdmissionDecisionNonTemporalCore,
        decided_at: OrchestrationDateTimeV1,
    ) -> Self {
        Self { core, decided_at }
    }

    pub fn core(&self) -> &DispatchAdmissionDecisionNonTemporalCore {
        &self.core
    }

    pub fn decided_at(&self) -> &OrchestrationDateTimeV1 {
        &self.decided_at
    }
}
