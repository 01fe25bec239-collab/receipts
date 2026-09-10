//! Deterministic lifecycle foundation. No dispatch, clocks, or recovery policy.
pub use crate::registry::ModelIntelligenceService;
use crate::registry::{CapabilityId, Observation, RuntimeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    Discovered,
    Unassessed,
    CapabilityVerified,
    Calibrating,
    Routable,
    Deprecated,
    Disabled,
    Unavailable,
    UserBlocked,
}
impl LifecycleState {
    pub const ALL: [Self; 9] = [
        Self::Discovered,
        Self::Unassessed,
        Self::CapabilityVerified,
        Self::Calibrating,
        Self::Routable,
        Self::Deprecated,
        Self::Disabled,
        Self::Unavailable,
        Self::UserBlocked,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Discovered => "DISCOVERED",
            Self::Unassessed => "UNASSESSED",
            Self::CapabilityVerified => "CAPABILITY_VERIFIED",
            Self::Calibrating => "CALIBRATING",
            Self::Routable => "ROUTABLE",
            Self::Deprecated => "DEPRECATED",
            Self::Disabled => "DISABLED",
            Self::Unavailable => "UNAVAILABLE",
            Self::UserBlocked => "USER_BLOCKED",
        }
    }
    /// Only the normal lifecycle gate. Authentication, availability, provider
    /// policy, entitlement, quality, quota and safety must pass independently.
    pub const fn passes_normal_lifecycle_gate(self) -> bool {
        matches!(self, Self::Routable)
    }
    pub(crate) const fn is_primary(self) -> bool {
        matches!(
            self,
            Self::Discovered
                | Self::Unassessed
                | Self::CapabilityVerified
                | Self::Calibrating
                | Self::Routable
        )
    }
}

/// The exact runtime and nonempty required capability path assessed by the
/// intelligence caller. Stored on successful verification for later promotions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationPath {
    pub runtime_id: RuntimeId,
    pub required_capabilities: Vec<CapabilityId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemotionReason {
    VendorRetirement,
    RepeatedRuntimeFailures,
    SustainedRejectionRate,
    AuthLoss,
    TemporarilyUnreachable,
    RateLimited,
    UserBlock,
}
impl DemotionReason {
    pub const fn target(self) -> LifecycleState {
        match self {
            Self::VendorRetirement => LifecycleState::Deprecated,
            Self::RepeatedRuntimeFailures | Self::SustainedRejectionRate => {
                LifecycleState::Disabled
            }
            Self::AuthLoss | Self::TemporarilyUnreachable | Self::RateLimited => {
                LifecycleState::Unavailable
            }
            Self::UserBlock => LifecycleState::UserBlocked,
        }
    }
}

/// Explicit caller results, scoped to the model's stored verification path.
/// The service checks the corresponding registry evidence as well. The caller
/// owns threshold evaluation and consent collection; neither is inferred here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutablePromotionEvidence {
    LocalCalibration {
        sufficient_acceptable_observations: bool,
    },
    OfficialAndIndependent {
        ask_on_uncertainty_user_consent: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleEvidence {
    ReadyForAssessment,
    CapabilityVerification(VerificationPath),
    BoundedCalibrationAdmission { authorized: bool },
    RoutablePromotion(RoutablePromotionEvidence),
    Demotion(DemotionReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleTransition {
    pub target: LifecycleState,
    pub evidence: LifecycleEvidence,
    pub observation: Observation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransitionRecord {
    pub from: LifecycleState,
    pub transition: LifecycleTransition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleError {
    IllegalTransition,
    MissingProvenance,
    MissingAuthoritativeCapability,
    MissingRuntimeCompatibility,
    EmptyRequiredCapabilities,
    BoundedAdmissionRequired,
    SufficientEvidenceRequired,
}

pub(crate) fn validate_transition_shape(
    from: LifecycleState,
    transition: &LifecycleTransition,
) -> Result<(), LifecycleError> {
    use LifecycleEvidence::*;
    use LifecycleState::*;
    let legal = match (&transition.evidence, from, transition.target) {
        (ReadyForAssessment, Discovered, Unassessed)
        | (CapabilityVerification(_), Unassessed, CapabilityVerified)
        | (BoundedCalibrationAdmission { .. }, CapabilityVerified, Calibrating)
        | (RoutablePromotion(_), Calibrating, Routable) => true,
        (Demotion(reason), from, target) => from.is_primary() && target == reason.target(),
        _ => false,
    };
    if !legal {
        return Err(LifecycleError::IllegalTransition);
    }
    if transition.observation.source_ref.is_none() {
        return Err(LifecycleError::MissingProvenance);
    }
    Ok(())
}
