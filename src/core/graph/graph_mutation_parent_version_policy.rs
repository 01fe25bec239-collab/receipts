//! Pure equality-only graph mutation parent-version compatibility.

use super::{GraphMutation, GraphVersionV1};

/// The mutation's parent version does not equal the supplied current version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphMutationParentVersionError;

impl std::fmt::Display for GraphMutationParentVersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("graph mutation parent version does not match current version")
    }
}

impl std::error::Error for GraphMutationParentVersionError {}

/// Passes exactly when the parent version equals the supplied current version.
///
/// A mismatch carries no ordering meaning. This policy inspects no other
/// mutation fields, consults no state, and does not apply or execute a mutation.
pub fn validate_graph_mutation_parent_version(
    current_version: &GraphVersionV1,
    mutation: &GraphMutation,
) -> Result<(), GraphMutationParentVersionError> {
    if mutation.parent_version() == current_version {
        Ok(())
    } else {
        Err(GraphMutationParentVersionError)
    }
}
