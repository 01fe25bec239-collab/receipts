//! Explicit failure surface for the bounded graph domain.
//!
//! Malformed input accepted by this API fails explicitly: it is never
//! converted into a permissive successful state. Cycle rejections carry the
//! deterministically identified candidate edge that closes the cycle plus the
//! concrete existing precedence path it would close.

/// Maximum allowed length, in Unicode scalar values, of every constrained
/// graph identifier (graph, node, edge): the frozen contract value 200.
pub const MAX_IDENTIFIER_LENGTH: usize = 200;

/// Every way a graph-domain operation can fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    /// A required identifier was empty.
    EmptyIdentifier {
        /// Which field was empty (e.g. `node_id`, `edge_id`, `graph_id`).
        field: &'static str,
    },
    /// An identifier exceeded [`MAX_IDENTIFIER_LENGTH`].
    IdentifierTooLong {
        /// Which field was too long.
        field: &'static str,
        /// Observed length in Unicode scalar values.
        length: usize,
        /// Maximum allowed length ([`MAX_IDENTIFIER_LENGTH`]).
        max: usize,
    },
    /// A node id was already present in the graph.
    DuplicateNodeId {
        /// The duplicated node id.
        node_id: String,
    },
    /// An edge id was already present in the graph.
    DuplicateEdgeId {
        /// The duplicated edge id.
        edge_id: String,
    },
    /// An edge referenced a node id that is not in the graph. Edges may only
    /// connect nodes the graph already knows; dangling references are never
    /// silently accepted.
    UnknownNodeReference {
        /// The offending edge id.
        edge_id: String,
        /// The referenced node id that does not exist.
        node_id: String,
    },
    /// A candidate precedence edge addition/mutation was rejected because the
    /// resulting precedence subgraph contains a cycle. The graph is left
    /// unchanged: cycles are rejected, not repaired, deleted around,
    /// reversed, or ignored.
    PrecedenceCycleRejected {
        /// Id of the candidate edge whose acceptance would close the cycle.
        candidate_edge_id: String,
        /// Source node of the candidate edge.
        candidate_from_node: String,
        /// Target node of the candidate edge.
        candidate_to_node: String,
        /// Deterministic existing precedence path from
        /// `candidate_to_node` back to `candidate_from_node`, expressed as
        /// node ids `[candidate_to_node, .., candidate_from_node]`. Empty for
        /// a self-cycle where `candidate_from_node == candidate_to_node`.
        closing_path_nodes: Vec<String>,
        /// Edge ids of the existing precedence edges traversed by
        /// `closing_path_nodes`, aligned pairwise with its transitions
        /// (`closing_path_edges[i]` joins `closing_path_nodes[i]` to
        /// `closing_path_nodes[i + 1]`). Empty for self-cycles and when the
        /// closing path is a single hop.
        closing_path_edge_ids: Vec<String>,
    },
}

impl GraphError {
    /// Validates an identifier against the frozen length contract.
    pub(crate) fn validate_identifier(field: &'static str, value: &str) -> Result<(), Self> {
        if value.is_empty() {
            return Err(GraphError::EmptyIdentifier { field });
        }
        let length = value.chars().count();
        if length > MAX_IDENTIFIER_LENGTH {
            return Err(GraphError::IdentifierTooLong {
                field,
                length,
                max: MAX_IDENTIFIER_LENGTH,
            });
        }
        Ok(())
    }
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GraphError::EmptyIdentifier { field } => {
                write!(f, "empty identifier for {field}")
            }
            GraphError::IdentifierTooLong { field, length, max } => {
                write!(
                    f,
                    "identifier for {field} has {length} scalar values, exceeding the maximum {max}"
                )
            }
            GraphError::DuplicateNodeId { node_id } => {
                write!(f, "duplicate node id {node_id:?}")
            }
            GraphError::DuplicateEdgeId { edge_id } => {
                write!(f, "duplicate edge id {edge_id:?}")
            }
            GraphError::UnknownNodeReference { edge_id, node_id } => {
                write!(f, "edge {edge_id:?} references unknown node {node_id:?}")
            }
            GraphError::PrecedenceCycleRejected {
                candidate_edge_id,
                candidate_from_node,
                candidate_to_node,
                closing_path_nodes,
                ..
            } => {
                write!(
                    f,
                    "precedence edge {candidate_edge_id:?} ({candidate_from_node:?} -> \
{candidate_to_node:?}) closes precedence cycle through {:?}; change rejected",
                    closing_path_nodes
                )
            }
        }
    }
}

impl std::error::Error for GraphError {}
