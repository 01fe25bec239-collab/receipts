//! Request identity assessments for every raw Registry candidate.

use crate::registry::Registry;
use crate::{
    CandidateIdentityConstraintOutcome, RegistryCandidateIdentity, RoutingRequestConstraints,
    enumerate_registry_candidates, evaluate_candidate_identity_constraints,
};

/// An identity assessment only; satisfaction does not establish overall eligibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryCandidateIdentityConstraintAssessment {
    candidate: RegistryCandidateIdentity,
    outcome: CandidateIdentityConstraintOutcome,
}

impl RegistryCandidateIdentityConstraintAssessment {
    pub fn candidate(&self) -> &RegistryCandidateIdentity {
        &self.candidate
    }

    pub fn outcome(&self) -> CandidateIdentityConstraintOutcome {
        self.outcome
    }
}

/// Preserves raw enumeration order and retains every candidate, including rejections.
/// Delegates identity policy unchanged; `max_cost` remains outside this composition.
pub fn evaluate_registry_candidate_identity_constraints(
    registry: &Registry,
    constraints: Option<&RoutingRequestConstraints>,
) -> Vec<RegistryCandidateIdentityConstraintAssessment> {
    enumerate_registry_candidates(registry)
        .into_iter()
        .map(|candidate| {
            let outcome = evaluate_candidate_identity_constraints(&candidate, constraints);
            RegistryCandidateIdentityConstraintAssessment { candidate, outcome }
        })
        .collect()
}
