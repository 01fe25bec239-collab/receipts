//! Operational six-axis dispatch admission derivation (D11).
//!
//! See `docs/orchestration/ADR-D11-admission-precedence.md`. The evaluator is
//! stricter than the schema-shaped value constructors, which keep accepting
//! every schema-permitted representation (absent reasons, NOT_APPLICABLE).
//!
//! - ALLOW only when all six axes are evaluated PASS from present, fresh,
//!   subject-bound evidence consistent with the authoritative policy.
//! - Every axis is evaluated and recorded; the primary failure follows the
//!   explicit precedence [`PRIMARY_FAILURE_PRECEDENCE`], never enum order.
//! - Missing, stale or unknown evidence fails its own axis conservatively.
//! - Evidence bound to another node/graph/epoch, or an internally
//!   contradictory safety state, is a typed pre-dispatch refusal returned
//!   outside the frozen decision: no decision referencing those facts exists.

use super::{
    DispatchAdmissionAxisReferenceError, DispatchAdmissionAxisResult, DispatchAdmissionAxisResults,
    DispatchAdmissionDecisionCore, DispatchAdmissionDecisionCoreError,
    DispatchAdmissionDecisionNonTemporalCore, DispatchAdmissionDecisionNonTemporalCoreError,
    DispatchAdmissionDenialReason, DispatchAdmissionEntitlementAxisResult,
    DispatchAdmissionFailingAxis, DispatchAdmissionOutcome,
    DispatchAdmissionProviderAuthAxisResult, DispatchAdmissionProviderAvailabilityAxisResult,
    DispatchAdmissionProviderPolicyAxisError, DispatchAdmissionProviderPolicyAxisResult,
    DispatchAdmissionProviderPolicyStatus, DispatchAdmissionQualityFloorAxisResult,
    DispatchAdmissionSafetyAxisResult,
};
use crate::orchestration::FeatureAdmissionOutcome;

use DispatchAdmissionDenialReason as Reason;
use DispatchAdmissionFailingAxis as Axis;

/// Explicit primary-failure precedence (ADR-D11), highest first.
pub const PRIMARY_FAILURE_PRECEDENCE: [DispatchAdmissionFailingAxis; 6] = [
    Axis::Safety,
    Axis::Entitlement,
    Axis::ProviderPolicy,
    Axis::ProviderAuth,
    Axis::ProviderAvailability,
    Axis::QualityFloor,
];

/// The node, graph and context epoch every piece of evidence must be bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionBindingV1 {
    pub node_id: String,
    pub graph_id: Option<String>,
    pub context_epoch: String,
}

/// Whether the evidence producer still considers the observation current.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceFreshnessV1 {
    Fresh,
    Stale,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntitlementEvidenceV1 {
    pub binding: AdmissionBindingV1,
    pub freshness: EvidenceFreshnessV1,
    pub feature_admission_decision_id: String,
    pub outcome: FeatureAdmissionOutcome,
}

/// Technical provider connection state (not credential correctness).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderConnectionV1 {
    Authenticated,
    Unauthenticated,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderAuthEvidenceV1 {
    pub binding: AdmissionBindingV1,
    pub freshness: EvidenceFreshnessV1,
    pub provider_id: String,
    pub connection: ProviderConnectionV1,
    pub technical_status: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPolicyEvidenceV1 {
    pub binding: AdmissionBindingV1,
    pub freshness: EvidenceFreshnessV1,
    pub provider_policy_eligibility_id: String,
    /// The provider the referenced eligibility record is about.
    pub provider_id: String,
    pub status: DispatchAdmissionProviderPolicyStatus,
    pub execution_context: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityObservationV1 {
    Available,
    RateLimited,
    ProvenDown,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailabilityEvidenceV1 {
    pub binding: AdmissionBindingV1,
    pub freshness: EvidenceFreshnessV1,
    pub availability_state_ref: String,
    pub observation: AvailabilityObservationV1,
    /// The admitted available runtime, if any.
    pub runtime_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyStateV1 {
    Clear,
    Pending,
    Blocked,
    HumanRequired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyEvidenceV1 {
    pub binding: AdmissionBindingV1,
    pub freshness: EvidenceFreshnessV1,
    pub state: SafetyStateV1,
    /// The open SafetyInterruption, required unless the state is Clear.
    pub safety_interruption_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityFloorResultV1 {
    Satisfied,
    Unsatisfied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityFloorEvidenceV1 {
    pub binding: AdmissionBindingV1,
    pub freshness: EvidenceFreshnessV1,
    pub routing_decision_id: String,
    pub result: QualityFloorResultV1,
}

/// One admission request. `None` means the evidence is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionRequestV1 {
    pub decision_id: String,
    pub capability_id: Option<String>,
    pub binding: AdmissionBindingV1,
    /// The provider the trusted composition selected for this dispatch.
    /// Policy and authentication evidence are each evaluated against it,
    /// independently of one another; `None` never grants either axis.
    pub selected_provider_id: Option<String>,
    pub entitlement: Option<EntitlementEvidenceV1>,
    pub provider_auth: Option<ProviderAuthEvidenceV1>,
    pub provider_policy: Option<ProviderPolicyEvidenceV1>,
    pub provider_availability: Option<AvailabilityEvidenceV1>,
    pub safety: Option<SafetyEvidenceV1>,
    pub quality_floor: Option<QualityFloorEvidenceV1>,
}

/// Pre-dispatch refusals the frozen decision cannot truthfully represent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchAdmissionRefusalV1 {
    /// Evidence for `axis` is bound to another node, graph or context epoch.
    EvidenceBindingMismatch { axis: DispatchAdmissionFailingAxis },
    /// The supplied safety state contradicts its own interruption reference.
    ContradictorySafetyState,
    /// A mandatory identity or reference is outside its frozen domain.
    InvalidIdentity(String),
}

/// The derived decision plus every failing axis, primary first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionDerivationV1 {
    decision: DispatchAdmissionDecisionNonTemporalCore,
    failures: Vec<(DispatchAdmissionFailingAxis, DispatchAdmissionDenialReason)>,
}

impl DispatchAdmissionDerivationV1 {
    pub fn decision(&self) -> &DispatchAdmissionDecisionNonTemporalCore {
        &self.decision
    }
    /// All failing axes with their reasons, in precedence order.
    pub fn failures(&self) -> &[(DispatchAdmissionFailingAxis, DispatchAdmissionDenialReason)] {
        &self.failures
    }
    /// True only for ALLOW: every denial means zero dispatch.
    pub fn dispatch_permitted(&self) -> bool {
        self.decision.core().outcome() == DispatchAdmissionOutcome::Allow
    }
}

fn invalid(error: impl std::fmt::Debug) -> DispatchAdmissionRefusalV1 {
    DispatchAdmissionRefusalV1::InvalidIdentity(format!("{error:?}"))
}

impl From<DispatchAdmissionAxisReferenceError> for DispatchAdmissionRefusalV1 {
    fn from(e: DispatchAdmissionAxisReferenceError) -> Self {
        invalid(e)
    }
}
impl From<DispatchAdmissionProviderPolicyAxisError> for DispatchAdmissionRefusalV1 {
    fn from(e: DispatchAdmissionProviderPolicyAxisError) -> Self {
        invalid(e)
    }
}
impl From<DispatchAdmissionDecisionCoreError> for DispatchAdmissionRefusalV1 {
    fn from(e: DispatchAdmissionDecisionCoreError) -> Self {
        invalid(e)
    }
}
impl From<DispatchAdmissionDecisionNonTemporalCoreError> for DispatchAdmissionRefusalV1 {
    fn from(e: DispatchAdmissionDecisionNonTemporalCoreError) -> Self {
        invalid(e)
    }
}

const PASS: DispatchAdmissionAxisResult = DispatchAdmissionAxisResult::Pass;
const FAIL: DispatchAdmissionAxisResult = DispatchAdmissionAxisResult::Fail;

fn result(failure: Option<Reason>) -> DispatchAdmissionAxisResult {
    if failure.is_some() { FAIL } else { PASS }
}

/// Derives one operational decision. Total over every input combination.
pub fn derive_dispatch_admission(
    request: &DispatchAdmissionRequestV1,
) -> Result<DispatchAdmissionDerivationV1, DispatchAdmissionRefusalV1> {
    let bound = |axis, binding: &AdmissionBindingV1| {
        if binding == &request.binding {
            Ok(())
        } else {
            Err(DispatchAdmissionRefusalV1::EvidenceBindingMismatch { axis })
        }
    };
    let fresh = |freshness| freshness == EvidenceFreshnessV1::Fresh;

    // SAFETY: a supplied consistent state yields its own reason; missing or
    // stale safety evidence is conservatively still pending.
    let safety_failure = match &request.safety {
        None => Some(Reason::SafetyCheckPending),
        Some(e) => {
            bound(Axis::Safety, &e.binding)?;
            if (e.state == SafetyStateV1::Clear) == e.safety_interruption_id.is_some() {
                return Err(DispatchAdmissionRefusalV1::ContradictorySafetyState);
            }
            match (fresh(e.freshness), e.state) {
                (true, SafetyStateV1::Clear) => None,
                (_, SafetyStateV1::Blocked) => Some(Reason::PolicyBlocked),
                (_, SafetyStateV1::HumanRequired) => Some(Reason::HumanRequired),
                _ => Some(Reason::SafetyCheckPending),
            }
        }
    };

    let entitlement_failure = match &request.entitlement {
        None => Some(Reason::EntitlementUnknown),
        Some(e) => {
            bound(Axis::Entitlement, &e.binding)?;
            match (fresh(e.freshness), e.outcome) {
                (true, FeatureAdmissionOutcome::Allow) => None,
                (_, FeatureAdmissionOutcome::LockedRequiresPro) => Some(Reason::LockedRequiresPro),
                (_, FeatureAdmissionOutcome::EntitlementExpired) => {
                    Some(Reason::EntitlementExpired)
                }
                _ => Some(Reason::EntitlementUnknown),
            }
        }
    };

    let selected = |provider: &str| request.selected_provider_id.as_deref() == Some(provider);

    let auth_failure = match &request.provider_auth {
        None => Some(Reason::AuthRequired),
        Some(e) => {
            bound(Axis::ProviderAuth, &e.binding)?;
            // Authentication of another provider does not authenticate this one.
            (!(fresh(e.freshness)
                && e.connection == ProviderConnectionV1::Authenticated
                && selected(&e.provider_id)))
            .then_some(Reason::AuthRequired)
        }
    };

    let policy_failure = match &request.provider_policy {
        None => Some(Reason::ProviderPolicyUnknown),
        Some(e) => {
            bound(Axis::ProviderPolicy, &e.binding)?;
            // Policy evidence about another provider (or with no selected
            // provider to compare) says nothing about the selected provider:
            // neither permission nor a disallow is attributed to it.
            match e.status {
                _ if !selected(&e.provider_id) => Some(Reason::ProviderPolicyUnknown),
                DispatchAdmissionProviderPolicyStatus::VerifiedDisallowed => {
                    Some(Reason::ProviderPolicyDisallowed)
                }
                DispatchAdmissionProviderPolicyStatus::VerifiedAllowed if fresh(e.freshness) => {
                    None
                }
                _ => Some(Reason::ProviderPolicyUnknown),
            }
        }
    };

    let availability_failure = match &request.provider_availability {
        None => Some(Reason::NoEligibleRuntime),
        Some(e) => {
            bound(Axis::ProviderAvailability, &e.binding)?;
            match (fresh(e.freshness), e.observation) {
                // Only fresh evidence may assert a current limit or outage.
                (true, AvailabilityObservationV1::RateLimited) => Some(Reason::ProviderRateLimited),
                (true, AvailabilityObservationV1::ProvenDown) => Some(Reason::ProviderDown),
                (true, AvailabilityObservationV1::Available) if e.runtime_id.is_some() => None,
                // Unknown, stale or no admitted runtime is never called "down".
                _ => Some(Reason::NoEligibleRuntime),
            }
        }
    };

    let quality_failure = match &request.quality_floor {
        None => Some(Reason::QualityFloorUnsatisfied),
        Some(e) => {
            bound(Axis::QualityFloor, &e.binding)?;
            (!(fresh(e.freshness) && e.result == QualityFloorResultV1::Satisfied))
                .then_some(Reason::QualityFloorUnsatisfied)
        }
    };

    let axis_results = DispatchAdmissionAxisResults {
        entitlement: DispatchAdmissionEntitlementAxisResult::try_new(
            result(entitlement_failure),
            request
                .entitlement
                .as_ref()
                .map(|e| e.feature_admission_decision_id.clone()),
        )?,
        provider_auth: DispatchAdmissionProviderAuthAxisResult::new(
            result(auth_failure),
            request
                .provider_auth
                .as_ref()
                .map(|e| e.provider_id.clone()),
            request
                .provider_auth
                .as_ref()
                .map(|e| e.technical_status.clone()),
        ),
        provider_policy: DispatchAdmissionProviderPolicyAxisResult::try_new(
            result(policy_failure),
            request
                .provider_policy
                .as_ref()
                .map(|e| e.provider_policy_eligibility_id.clone()),
            request.provider_policy.as_ref().map(|e| e.status),
            request
                .provider_policy
                .as_ref()
                .map(|e| e.execution_context.clone()),
        )?,
        provider_availability: DispatchAdmissionProviderAvailabilityAxisResult::try_new(
            result(availability_failure),
            request
                .provider_availability
                .as_ref()
                .map(|e| e.availability_state_ref.clone()),
        )?,
        safety: DispatchAdmissionSafetyAxisResult::try_new(
            result(safety_failure),
            request
                .safety
                .as_ref()
                .and_then(|e| e.safety_interruption_id.clone()),
        )?,
        quality_floor: DispatchAdmissionQualityFloorAxisResult::try_new(
            result(quality_failure),
            request
                .quality_floor
                .as_ref()
                .map(|e| e.routing_decision_id.clone()),
        )?,
    };

    let by_axis = |axis| match axis {
        Axis::Safety => safety_failure,
        Axis::Entitlement => entitlement_failure,
        Axis::ProviderPolicy => policy_failure,
        Axis::ProviderAuth => auth_failure,
        Axis::ProviderAvailability => availability_failure,
        Axis::QualityFloor => quality_failure,
        Axis::None => None,
    };
    let failures: Vec<_> = PRIMARY_FAILURE_PRECEDENCE
        .into_iter()
        .filter_map(|axis| by_axis(axis).map(|reason| (axis, reason)))
        .collect();

    let (outcome, axis, reason, provider, runtime) = match failures.first() {
        None => (
            DispatchAdmissionOutcome::Allow,
            Axis::None,
            Reason::None,
            request.selected_provider_id.clone(),
            request
                .provider_availability
                .as_ref()
                .and_then(|e| e.runtime_id.clone()),
        ),
        Some(&(axis, reason)) => (DispatchAdmissionOutcome::Deny, axis, reason, None, None),
    };
    let core = DispatchAdmissionDecisionCore::try_new(
        request.decision_id.clone(),
        request.binding.node_id.clone(),
        request.binding.graph_id.clone(),
        request.capability_id.clone(),
        outcome,
        axis,
        Some(reason),
        provider,
        runtime,
    )?;
    Ok(DispatchAdmissionDerivationV1 {
        decision: DispatchAdmissionDecisionNonTemporalCore::try_new(core, axis_results)?,
        failures,
    })
}
