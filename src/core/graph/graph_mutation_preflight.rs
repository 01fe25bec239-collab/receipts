//! Pure, fail-fast composition of the three canonical mutation guards.

use super::{
    ExecutionGraph, GraphMutation, GraphMutationActorAuthorizationError,
    GraphMutationParentVersionError, GraphMutationTargetGraphError, GraphVersionV1,
    validate_graph_mutation_actor, validate_graph_mutation_parent_version,
    validate_graph_mutation_target_graph,
};

/// The first failing guard, preserving its canonical error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphMutationPreflightError {
    TargetGraph(GraphMutationTargetGraphError),
    ActorAuthorization(GraphMutationActorAuthorizationError),
    ParentVersion(GraphMutationParentVersionError),
}

impl std::fmt::Display for GraphMutationPreflightError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TargetGraph(error) => error.fmt(f),
            Self::ActorAuthorization(error) => error.fmt(f),
            Self::ParentVersion(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for GraphMutationPreflightError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TargetGraph(error) => Some(error),
            Self::ActorAuthorization(error) => Some(error),
            Self::ParentVersion(error) => Some(error),
        }
    }
}

/// Checks target identity, actor authorization, then parent-version equality,
/// stopping at the first failure.
///
/// Success means only these three guards passed. It neither applies nor
/// authorizes execution, persistence, replay, dispatch, or a State transaction,
/// and establishes no operation, resulting-version, or digest validity.
pub fn validate_graph_mutation_preflight(
    graph: &ExecutionGraph,
    current_version: &GraphVersionV1,
    mutation: &GraphMutation,
) -> Result<(), GraphMutationPreflightError> {
    validate_graph_mutation_target_graph(graph, mutation)
        .map_err(GraphMutationPreflightError::TargetGraph)?;
    validate_graph_mutation_actor(mutation.actor())
        .map_err(GraphMutationPreflightError::ActorAuthorization)?;
    validate_graph_mutation_parent_version(current_version, mutation)
        .map_err(GraphMutationPreflightError::ParentVersion)?;
    Ok(())
}
