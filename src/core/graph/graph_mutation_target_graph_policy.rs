//! Pure equality-only graph mutation target identity preflight.

use super::{ExecutionGraph, GraphMutation};

/// The mutation does not target the supplied execution graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphMutationTargetGraphError;

impl std::fmt::Display for GraphMutationTargetGraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("graph mutation does not target this execution graph")
    }
}

impl std::error::Error for GraphMutationTargetGraphError {}

/// Passes exactly when the mutation and graph have equal opaque graph IDs.
///
/// A mismatch means only that this mutation does not target this graph.
/// This policy inspects no other fields and neither applies a mutation nor
/// composes actor authorization or parent-version compatibility policies.
pub fn validate_graph_mutation_target_graph(
    graph: &ExecutionGraph,
    mutation: &GraphMutation,
) -> Result<(), GraphMutationTargetGraphError> {
    if mutation.graph_id() == graph.graph_id() {
        Ok(())
    } else {
        Err(GraphMutationTargetGraphError)
    }
}
