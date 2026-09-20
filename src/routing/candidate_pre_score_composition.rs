//! Independent identity and bounded eligibility outcomes for one supplied candidate.

use crate::policy_eligibility::ProviderPolicyEligibility;
use crate::registry::Registry;
use crate::{
    AvailabilityState, CandidateEligibilityOutcome, CandidateIdentityConstraintOutcome,
    RegistryCandidateIdentity, RoutingRequestNonTemporalCore, evaluate_candidate_eligibility,
    evaluate_candidate_identity_constraints,
};

/// Retains both subordinate outcomes without establishing final routing eligibility
/// or dispatch permission. Neither rejection suppresses the other assessment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedPreScoreCandidateAssessment {
    candidate: RegistryCandidateIdentity,
    identity_constraint_outcome: CandidateIdentityConstraintOutcome,
    bounded_eligibility_outcome: CandidateEligibilityOutcome,
}

impl BoundedPreScoreCandidateAssessment {
    pub fn candidate(&self) -> &RegistryCandidateIdentity {
        &self.candidate
    }

    pub fn identity_constraint_outcome(&self) -> CandidateIdentityConstraintOutcome {
        self.identity_constraint_outcome
    }

    pub fn bounded_eligibility_outcome(&self) -> &CandidateEligibilityOutcome {
        &self.bounded_eligibility_outcome
    }
}

/// Delegates both assessments independently for the exact supplied identity.
/// Availability observation time is passive evidence; only its core is delegated.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_bounded_pre_score_candidate(
    candidate: &RegistryCandidateIdentity,
    request: &RoutingRequestNonTemporalCore,
    registry: &Registry,
    availability: &AvailabilityState,
    policy: &ProviderPolicyEligibility,
    requested_execution_context: Option<&str>,
    reverification_deadline_passed: Option<bool>,
) -> BoundedPreScoreCandidateAssessment {
    let identity_constraint_outcome =
        evaluate_candidate_identity_constraints(candidate, request.constraints());
    let bounded_eligibility_outcome = evaluate_candidate_eligibility(
        request,
        registry,
        candidate.provider_id(),
        candidate.model_id(),
        candidate.runtime_id(),
        availability.core(),
        policy,
        requested_execution_context,
        reverification_deadline_passed,
    );
    BoundedPreScoreCandidateAssessment {
        candidate: candidate.clone(),
        identity_constraint_outcome,
        bounded_eligibility_outcome,
    }
}
