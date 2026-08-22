//! Orchestration graph domain — the first accepted frozen slice of
//! `M-ORCH-1A`: the [`ExecutionGraph`] / [`GraphNode`] / [`GraphEdge`]
//! precedence-DAG core plus deterministic precedence-cycle detection and
//! rejection.
//!
//! Frozen semantics implemented here:
//!
//! * edges are classed `PRECEDENCE` or `CONTROL`, mutually exclusive by
//!   construction; precedence kinds are `REQUIRES_ACCEPTED`,
//!   `REQUIRES_INTEGRATED`, `REQUIRES_INTERFACE`; control kinds are
//!   `ON_PASS`, `ON_REJECT`, `ON_FAILURE`, `ON_BLOCKED`, `ESCALATE`,
//!   `EXPANDS_INTO`;
//! * only `PRECEDENCE` edges form the dependency DAG; cycle analysis never
//!   traverses `CONTROL` edges, so conceptual control loops (repair
//!   expansions, escalations) cannot cause false rejection;
//! * a candidate structural precedence change that introduces a cycle is
//!   rejected atomically with the closing edge/change deterministically
//!   identified — never repaired, never silently worked around;
//! * node kinds are extensible strings, not a closed enum;
//! * `required_capabilities` is data only in this slice: stored verbatim,
//!   never interpreted — no entitlement, tier, admission, provider, or
//!   routing behavior exists here.
//!
//! Boundary rules honored by this crate:
//!
//! * Rust `std` only; no dependencies, no feature flags;
//! * no product API outside this bounded graph domain;
//! * malformed input fails explicitly instead of becoming a permissive
//!   successful state;
//! * no credentials access, network operations, shell execution, git
//!   execution, workspace manipulation, State-Context writes, or
//!   model/provider routing.

pub mod edge;
pub mod error;
pub mod execution_graph;
pub mod node;
pub mod node_state;

#[cfg(test)]
mod execution_graph_tests;
#[cfg(test)]
mod node_state_tests;

pub use edge::{ControlKind, EdgeClass, GraphEdge, GraphEdgeRelation, PrecedenceKind};
pub use error::{GraphError, MAX_IDENTIFIER_LENGTH};
pub use execution_graph::ExecutionGraph;
pub use node::{CapabilityName, GraphNode, GraphNodeKind};
pub use node_state::{GraphNodeState, GraphNodeStateParseError};
