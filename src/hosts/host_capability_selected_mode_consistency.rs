//! Pure consistency validation of an already caller-selected integration mode.

use crate::{
    HostCapabilityInactiveReason, HostCapabilityModeOverride,
    HostCapabilityNativePrerequisiteState, HostCapabilityProbeStatus, HostCapabilitySelectedMode,
    HostCapabilityStaleReason, is_embedded_eligible,
};

/// Caller-supplied facts and selected mode for consistency validation.
///
/// Native prerequisites are already assessed; their logic is not recomputed.
/// This is not a `HostCapabilityReport` implementation and performs no probing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilitySelectedModeConsistencyInputs {
    pub report_validity_proven_current: bool,
    pub probe_status: HostCapabilityProbeStatus,
    pub stale_reason: HostCapabilityStaleReason,
    pub native_prerequisite_state: HostCapabilityNativePrerequisiteState,
    pub selected_mode: HostCapabilitySelectedMode,
    pub mode_override: Option<HostCapabilityModeOverride>,
    pub inactive_reason: Option<HostCapabilityInactiveReason>,
}

/// Bounded inconsistencies in a caller-supplied selected mode and its facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapabilitySelectedModeConsistencyError {
    /// EMBEDDED was supplied but the existing eligibility predicate is false.
    EmbeddedNotEligible,
    /// Eligible EMBEDDED carries a concrete inactive-native-path reason.
    EmbeddedHasContradictoryInactiveReason,
    /// Healthy Hybrid/Supervised departure has no explicit existing override.
    HealthyNativePathDepartureRequiresModeOverride,
    /// Healthy departure has an override but lacks exactly `Some(ModeOverride)`.
    HealthyNativePathDepartureRequiresModeOverrideInactiveReason,
}

/// Validates the caller-supplied mode without selecting or changing any mode.
///
/// EMBEDDED eligibility is delegated to [`is_embedded_eligible`], consuming only
/// the already-derived native prerequisite state (`Satisfied` means true).
/// Eligible EMBEDDED permits absent or explicit `None` inactive reason, even
/// with an override. Healthy Hybrid/Supervised departure requires an existing
/// explicit override plus `ModeOverride` inactive reason; missing override is
/// reported first. Its reason is not revalidated and its source grants no
/// authority. Unhealthy/incomplete paths impose no preference between Hybrid
/// and Supervised and no global inactive-reason validation.
///
/// All facts are caller supplied. No probing, I/O, persistence, global mutation,
/// or native prerequisite reassessment occurs. This is not a
/// `HostCapabilityReport` implementation.
pub fn validate_selected_mode_consistency(
    inputs: &HostCapabilitySelectedModeConsistencyInputs,
) -> Result<(), HostCapabilitySelectedModeConsistencyError> {
    use HostCapabilitySelectedModeConsistencyError::*;

    let eligible = is_embedded_eligible(
        inputs.report_validity_proven_current,
        inputs.probe_status,
        inputs.stale_reason,
        matches!(
            inputs.native_prerequisite_state,
            HostCapabilityNativePrerequisiteState::Satisfied
        ),
    );
    match inputs.selected_mode {
        HostCapabilitySelectedMode::Embedded => {
            if !eligible {
                return Err(EmbeddedNotEligible);
            }
            if !matches!(
                inputs.inactive_reason,
                None | Some(HostCapabilityInactiveReason::None)
            ) {
                return Err(EmbeddedHasContradictoryInactiveReason);
            }
        }
        HostCapabilitySelectedMode::Hybrid | HostCapabilitySelectedMode::Supervised => {
            if eligible {
                if inputs.mode_override.is_none() {
                    return Err(HealthyNativePathDepartureRequiresModeOverride);
                }
                if inputs.inactive_reason != Some(HostCapabilityInactiveReason::ModeOverride) {
                    return Err(HealthyNativePathDepartureRequiresModeOverrideInactiveReason);
                }
            }
        }
    }
    Ok(())
}
