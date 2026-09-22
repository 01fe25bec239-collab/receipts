//! Ordered pre-score assessments for the complete canonical Registry candidate set.

use crate::policy_eligibility::ProviderPolicyEligibility;
use crate::registry::Registry;
use crate::{
    AvailabilityState, BoundedPreScoreCandidateAssessment, RegistryCandidateIdentity,
    RoutingRequestNonTemporalCore, enumerate_registry_candidates,
    evaluate_bounded_pre_score_candidate,
};

/// Caller-resolved evidence for exactly one canonical candidate position.
#[derive(Debug, Clone)]
pub struct BoundedPreScoreCandidateEvidence<'a> {
    candidate: RegistryCandidateIdentity,
    availability: &'a AvailabilityState,
    policy: &'a ProviderPolicyEligibility,
    reverification_deadline_passed: Option<bool>,
}

impl<'a> BoundedPreScoreCandidateEvidence<'a> {
    pub fn new(
        candidate: RegistryCandidateIdentity,
        availability: &'a AvailabilityState,
        policy: &'a ProviderPolicyEligibility,
        reverification_deadline_passed: Option<bool>,
    ) -> Self {
        Self {
            candidate,
            availability,
            policy,
            reverification_deadline_passed,
        }
    }

    pub fn candidate(&self) -> &RegistryCandidateIdentity {
        &self.candidate
    }

    pub fn availability(&self) -> &AvailabilityState {
        self.availability
    }

    pub fn policy(&self) -> &ProviderPolicyEligibility {
        self.policy
    }

    pub fn reverification_deadline_passed(&self) -> Option<bool> {
        self.reverification_deadline_passed
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundedPreScoreEvidenceBindingError {
    EvidenceCountMismatch,
    CandidateIdentityMismatch { index: usize },
}

/// Validates every positional binding before evaluating any candidate.
/// Evidence internals remain subject to the existing single-candidate evaluator.
pub fn evaluate_bounded_pre_score_candidate_set(
    request: &RoutingRequestNonTemporalCore,
    registry: &Registry,
    evidence: &[BoundedPreScoreCandidateEvidence<'_>],
    requested_execution_context: Option<&str>,
) -> Result<Vec<BoundedPreScoreCandidateAssessment>, BoundedPreScoreEvidenceBindingError> {
    let candidates = enumerate_registry_candidates(registry);
    if evidence.len() != candidates.len() {
        return Err(BoundedPreScoreEvidenceBindingError::EvidenceCountMismatch);
    }
    for (index, (candidate, bundle)) in candidates.iter().zip(evidence).enumerate() {
        if candidate != bundle.candidate() {
            return Err(BoundedPreScoreEvidenceBindingError::CandidateIdentityMismatch { index });
        }
    }
    Ok(candidates
        .iter()
        .zip(evidence)
        .map(|(candidate, bundle)| {
            evaluate_bounded_pre_score_candidate(
                candidate,
                request,
                registry,
                bundle.availability(),
                bundle.policy(),
                requested_execution_context,
                bundle.reverification_deadline_passed(),
            )
        })
        .collect())
}
