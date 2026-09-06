//! In-process composition under
//! USER-ADR-DISPATCH-ADMISSION-ALLOW-REQUIRES-ALL-SIX-PASS-001.
//! No execution, reference resolution, policy evaluation, or I/O.

use super::{
    DispatchAdmissionAxisResult, DispatchAdmissionDecisionCore,
    DispatchAdmissionEntitlementAxisResult, DispatchAdmissionFailingAxis, DispatchAdmissionOutcome,
    DispatchAdmissionProviderAuthAxisResult, DispatchAdmissionProviderAvailabilityAxisResult,
    DispatchAdmissionProviderPolicyAxisResult, DispatchAdmissionQualityFloorAxisResult,
    DispatchAdmissionSafetyAxisResult,
};

/// Exactly six mandatory, explicitly supplied axis records. No defaults.
///
/// ```compile_fail
/// use receipts_orchestration::scheduler::DispatchAdmissionAxisResults;
/// let axes = DispatchAdmissionAxisResults {};
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionAxisResults {
    pub entitlement: DispatchAdmissionEntitlementAxisResult,
    pub provider_auth: DispatchAdmissionProviderAuthAxisResult,
    pub provider_policy: DispatchAdmissionProviderPolicyAxisResult,
    pub provider_availability: DispatchAdmissionProviderAvailabilityAxisResult,
    pub safety: DispatchAdmissionSafetyAxisResult,
    pub quality_floor: DispatchAdmissionQualityFloorAxisResult,
}

impl DispatchAdmissionAxisResults {
    fn results(&self) -> [(DispatchAdmissionFailingAxis, DispatchAdmissionAxisResult); 6] {
        use DispatchAdmissionFailingAxis as Axis;
        [
            (Axis::Entitlement, self.entitlement.result()),
            (Axis::ProviderAuth, self.provider_auth.result()),
            (Axis::ProviderPolicy, self.provider_policy.result()),
            (
                Axis::ProviderAvailability,
                self.provider_availability.result(),
            ),
            (Axis::Safety, self.safety.result()),
            (Axis::QualityFloor, self.quality_floor.result()),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchAdmissionDecisionNonTemporalCoreError {
    AllowRequiresAllSixPass,
    FailingAxisMustReferenceFail {
        failing_axis: DispatchAdmissionFailingAxis,
    },
}

/// Validated NON-TEMPORAL CORE, not the complete `DispatchAdmissionDecision`.
///
/// `decided_at` is deferred: DEFERRED_NO_AUTHORIZED_TEMPORAL_TYPE.
/// Existing core fields (including optional provider/runtime strings) remain
/// authoritative and unchanged. Evidence is stored exactly; this boundary
/// validates explicit results, never infers them from opaque references or
/// policy/auth status. Status-to-result evaluation belongs to the evidence
/// producer. UNKNOWN/NEEDS_REVIEW never supply an implicit PASS.
///
/// ALLOW requires six PASS results. DENY requires the supplied failing axis to
/// name a FAIL; other axes may have any result. No failure precedence exists.
///
/// The validated aggregate exposes no mutable access or unchecked constructor.
///
/// ```compile_fail
/// use receipts_orchestration::scheduler::{
///     DispatchAdmissionDecisionNonTemporalCore, DispatchAdmissionProviderAuthAxisResult,
/// };
/// fn mutate(decision: &mut DispatchAdmissionDecisionNonTemporalCore,
///           replacement: DispatchAdmissionProviderAuthAxisResult) {
///     decision.axis_results().provider_auth = replacement;
/// }
/// ```
///
/// ```compile_fail
/// use receipts_orchestration::scheduler::DispatchAdmissionDecisionNonTemporalCore;
/// fn timestamp(decision: &DispatchAdmissionDecisionNonTemporalCore) {
///     decision.decided_at(); // Intentionally unavailable on a non-temporal core.
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionDecisionNonTemporalCore {
    core: DispatchAdmissionDecisionCore,
    axis_results: DispatchAdmissionAxisResults,
}

impl DispatchAdmissionDecisionNonTemporalCore {
    /// Takes an already validated core and all six records by value, preserving
    /// the core's identifier and outcome/failing-axis invariants.
    pub fn try_new(
        core: DispatchAdmissionDecisionCore,
        axis_results: DispatchAdmissionAxisResults,
    ) -> Result<Self, DispatchAdmissionDecisionNonTemporalCoreError> {
        let results = axis_results.results();
        match core.outcome() {
            DispatchAdmissionOutcome::Allow => {
                if !results
                    .iter()
                    .all(|(_, result)| *result == DispatchAdmissionAxisResult::Pass)
                {
                    return Err(
                        DispatchAdmissionDecisionNonTemporalCoreError::AllowRequiresAllSixPass,
                    );
                }
            }
            DispatchAdmissionOutcome::Deny => {
                if !results.contains(&(core.failing_axis(), DispatchAdmissionAxisResult::Fail)) {
                    return Err(
                        DispatchAdmissionDecisionNonTemporalCoreError::FailingAxisMustReferenceFail {
                            failing_axis: core.failing_axis(),
                        },
                    );
                }
            }
        }
        Ok(Self { core, axis_results })
    }

    pub fn core(&self) -> &DispatchAdmissionDecisionCore {
        &self.core
    }

    pub fn axis_results(&self) -> &DispatchAdmissionAxisResults {
        &self.axis_results
    }
}
