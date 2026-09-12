//! Pure eligibility for one exact candidate, limited to existing Model-Routing inputs.
//! Passing does not establish authentication, entitlement, quality, safety,
//! quota/cost admission, calibration exceptions, or final dispatch eligibility.

use crate::intelligence::LifecycleState;
use crate::policy_eligibility::{
    PolicyEligibilityEvaluator, PolicyStatus, ProviderPolicyEligibility,
};
use crate::registry::{CapabilityId, Compatibility, ModelId, ProviderId, Registry, RuntimeId};
use crate::{
    AvailabilityStateKind, AvailabilityStateNonTemporalCore, RoutingRequestNonTemporalCore,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateEligibilityRejection {
    RegistryModelMissing,
    RegistryRuntimeAssociationMissing,
    LifecycleNotNormallyRoutable(LifecycleState),
    RequiredCapabilityUnsupported(CapabilityId),
    RequiredCapabilityUnknown(CapabilityId),
    AvailabilityIdentityMismatch,
    AvailabilityScopeInsufficient,
    AvailabilityIneligible(AvailabilityStateKind),
    ProviderPolicyIdentityMismatch,
    ProviderPolicyStatusIneligible(PolicyStatus),
    ExecutionContextNotProvenAllowed,
    ReverificationEvidenceMissing,
    ReverificationDeadlinePassed,
}

/// An empty rejection list means only that this bounded pre-score subset passed.
/// Rejections are ordered by registry, lifecycle, required capabilities in request
/// order (including duplicates), availability, then provider policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateEligibilityOutcome {
    rejections: Vec<CandidateEligibilityRejection>,
}

impl CandidateEligibilityOutcome {
    pub fn rejections(&self) -> &[CandidateEligibilityRejection] {
        &self.rejections
    }

    pub fn passes_bounded_pre_score_eligibility(&self) -> bool {
        self.rejections.is_empty()
    }
}

/// Evaluates only required capabilities and the five documented bounded gates.
/// Other request fields are not enforced. All identities and contexts are exact.
///
/// No frozen/current contract establishes availability scope inheritance, so
/// both optional scope identifiers must be present and match this candidate.
/// A mismatched or insufficient record cannot establish its candidate's state.
/// Missing models suppress lifecycle and capability checks; missing associations
/// suppress capability checks. Independent availability/policy checks continue.
/// Mismatched policy identity suppresses policy evaluation; ineligible status
/// suppresses dependent context/deadline diagnostics, as in the policy evaluator.
#[allow(clippy::too_many_arguments)] // Explicit existing inputs; no new service or request contract.
pub fn evaluate_candidate_eligibility(
    request: &RoutingRequestNonTemporalCore,
    registry: &Registry,
    provider_id: &ProviderId,
    model_id: &ModelId,
    runtime_id: &RuntimeId,
    availability: &AvailabilityStateNonTemporalCore,
    policy: &ProviderPolicyEligibility,
    requested_execution_context: Option<&str>,
    reverification_deadline_passed: Option<bool>,
) -> CandidateEligibilityOutcome {
    use CandidateEligibilityRejection::*;
    let mut rejections = Vec::new();
    let provider = provider_id.as_str();
    let model = model_id.as_str();
    let runtime = runtime_id.as_str();

    if let Some(record) = registry.model(provider, model) {
        let associated = registry.association(provider, model, runtime).is_some();
        if !associated {
            rejections.push(RegistryRuntimeAssociationMissing);
        }
        if !record.lifecycle_state().passes_normal_lifecycle_gate() {
            rejections.push(LifecycleNotNormallyRoutable(record.lifecycle_state()));
        }
        if associated {
            for required in request.required_capabilities() {
                let capability = CapabilityId::try_new(required.clone())
                    .expect("RoutingRequestNonTemporalCore rejects empty required capabilities");
                match registry.compatibility(provider_id, model_id, runtime_id, &capability) {
                    Compatibility::Confirmed => {}
                    Compatibility::Unsupported => {
                        rejections.push(RequiredCapabilityUnsupported(capability));
                    }
                    Compatibility::Unknown => {
                        rejections.push(RequiredCapabilityUnknown(capability));
                    }
                }
            }
        }
    } else {
        rejections.push(RegistryModelMissing);
    }

    if availability.provider_id() != provider
        || availability.model_id().is_some_and(|id| id != model)
        || availability.runtime_id().is_some_and(|id| id != runtime)
    {
        rejections.push(AvailabilityIdentityMismatch);
    } else if availability.model_id().is_none() || availability.runtime_id().is_none() {
        rejections.push(AvailabilityScopeInsufficient);
    } else if !matches!(
        availability.state(),
        AvailabilityStateKind::Available | AvailabilityStateKind::Degraded
    ) {
        rejections.push(AvailabilityIneligible(availability.state()));
    }

    if policy.provider_id() != provider || policy.runtime_id() != runtime {
        rejections.push(ProviderPolicyIdentityMismatch);
    } else if !PolicyEligibilityEvaluator::evaluate(
        policy,
        requested_execution_context,
        reverification_deadline_passed,
    ) {
        if !policy.policy_status().passes_policy_gate_by_default() {
            rejections.push(ProviderPolicyStatusIneligible(policy.policy_status()));
        } else {
            // Explain the existing evaluator's rejection; these diagnostics
            // never grant eligibility or substitute deadline evidence.
            if !requested_execution_context.is_some_and(|requested| {
                policy
                    .allowed_execution_contexts()
                    .is_some_and(|allowed| allowed.iter().any(|context| context == requested))
            }) {
                rejections.push(ExecutionContextNotProvenAllowed);
            }
            if policy.reverification_deadline().flatten().is_some() {
                match reverification_deadline_passed {
                    None => rejections.push(ReverificationEvidenceMissing),
                    Some(true) => rejections.push(ReverificationDeadlinePassed),
                    Some(false) => {}
                }
            }
        }
    }

    CandidateEligibilityOutcome { rejections }
}
