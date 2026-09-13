//! Pure authorization of open-string graph mutation actor evidence.

use super::GraphMutationActor;

/// The actor's exact role is outside the authorized role set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphMutationActorAuthorizationError;

impl std::fmt::Display for GraphMutationActorAuthorizationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("graph mutation actor role is not authorized")
    }
}

impl std::error::Error for GraphMutationActorAuthorizationError {}

/// Authorizes only exact role strings; every other string fails closed.
///
/// `role_id` supplies no authority and is not inspected. This policy neither
/// applies a mutation nor consults any graph or external state.
pub fn validate_graph_mutation_actor(
    actor: &GraphMutationActor,
) -> Result<(), GraphMutationActorAuthorizationError> {
    match actor.role() {
        "GRAPH_COMPILER" | "RUNTIME_A1" | "RUNTIME_A2" | "GOAL_EVALUATOR" | "USER" => Ok(()),
        _ => Err(GraphMutationActorAuthorizationError),
    }
}
