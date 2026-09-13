//! Pure request identity constraints for an already-enumerated candidate.

use crate::{RegistryCandidateIdentity, RoutingRequestConstraints};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateIdentityConstraintRejection {
    DistinctProviderFromViolation,
    AvoidedProvider,
    PinnedProviderMismatch,
    PinnedModelMismatch,
}

/// Satisfied means only that the request's provider/model identity constraints pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateIdentityConstraintOutcome {
    Satisfied,
    Rejected(CandidateIdentityConstraintRejection),
}

/// Compares opaque provider/model IDs exactly, without modifying either input.
/// Returns the first rejection in this explicit order: distinct provider, avoided
/// provider, pinned provider, pinned model. Absent constraints pass.
/// Runtime identity is unconstrained. `max_cost` is outside this identity-only
/// policy and is not evaluated; success does not establish cost admission or
/// overall candidate eligibility.
pub fn evaluate_candidate_identity_constraints(
    candidate: &RegistryCandidateIdentity,
    constraints: Option<&RoutingRequestConstraints>,
) -> CandidateIdentityConstraintOutcome {
    use CandidateIdentityConstraintOutcome::{Rejected, Satisfied};
    use CandidateIdentityConstraintRejection::*;

    let Some(constraints) = constraints else {
        return Satisfied;
    };
    let provider = candidate.provider_id().as_str();

    if constraints.distinct_provider_from() == Some(provider) {
        return Rejected(DistinctProviderFromViolation);
    }
    if constraints
        .avoid_providers()
        .is_some_and(|providers| providers.iter().any(|id| id == provider))
    {
        return Rejected(AvoidedProvider);
    }
    if constraints
        .user_pinned_provider()
        .is_some_and(|id| id != provider)
    {
        return Rejected(PinnedProviderMismatch);
    }
    if constraints
        .user_pinned_model()
        .is_some_and(|id| id != candidate.model_id().as_str())
    {
        return Rejected(PinnedModelMismatch);
    }
    Satisfied
}
