//! Pure projection of a State-persisted current graph into a canonical
//! [`GraphSnapshot`] (BUILD-A1-ADR-ORCHESTRATION-PERSISTED-SNAPSHOT-PROJECTION-V1-001).
//!
//! Checks run fail-fast in this fixed order:
//!
//! 1. exact project/graph identity against the supplied expected strings,
//!    then exact `current_version == graph_version` and canonical
//!    [`GraphVersionV1`] parsing (no ordering, arithmetic or ceiling);
//! 2. reconstruction of every canonical node (15 fields) and edge (class,
//!    kind, note) through the existing graph constructors, preserving
//!    optional absence, explicit null, empty text, list order/duplicates and
//!    exact bytes;
//! 3. [`ExecutionGraph::from_parts`] containment, uniqueness, endpoint and
//!    `PRECEDENCE` acyclicity validation (`CONTROL` loops remain valid);
//! 4. recomputation of [`graph_content_digest_v1`] over the reconstructed
//!    children and exact comparison with the stored digest;
//! 5. exact parsing of the stored version `created_at` as `captured_at`.
//!
//! Only then is the snapshot built: node states in ascending node-ID UTF-8
//! byte order, exact state, `Some(kind)`, exact optional `code_sha`,
//! `locked = None` for every node, `Some(verified digest)` and no summary.
//! `locked = None` asserts neither locked nor unlocked: State stores no
//! separate snapshot lock flag, and none is derived from a state name or
//! `locked_reason`, whose forms are still preserved in the hashed children.
//!
//! The expected identifiers are comparison context, never authorization.
//! A matching digest validates child content only; it authenticates no
//! access, project, compiler/source, provenance, creation time, current
//! pointer, lineage or digest-excluded outer metadata, and proves no input
//! secret-free. This module performs no I/O, clock sample or logging.
//! Failures are closed categories that never carry or format supplied
//! values; nested errors are discarded unformatted.

use receipts_state::{
    PersistedCurrentGraphV1, StateGraphEdgeRelationV1, StateGraphEdgeV1, StateGraphNodeV1,
};

use crate::content_digest::graph_content_digest_v1;
use crate::edge::{ControlKind, GraphEdge, PrecedenceKind};
use crate::execution_graph::ExecutionGraph;
use crate::node::{CapabilityName, GraphNode, GraphNodeAttemptNumber, GraphNodeKind};
use crate::node_state::GraphNodeState;
use crate::orchestration::OrchestrationDateTimeV1;
use crate::records::{GraphSnapshot, GraphSnapshotNodeState};
use crate::version::GraphVersionV1;

use GraphSnapshotProjectionErrorV1 as Error;

/// Why no snapshot was projected. Each variant is a fixed category with no
/// payload; `Display`/`Debug` emit only fixed text and `source()` is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphSnapshotProjectionErrorV1 {
    /// Stored project or graph identity differs from the expected context.
    IdentityMismatch,
    /// Stored current-version pointer differs from the projected version.
    VersionMismatch,
    /// A stored field or the output record failed canonical parsing.
    InvalidProjection,
    /// Graph containment, uniqueness, endpoint or precedence validation failed.
    InvalidTopology,
    /// The recomputed child-content digest differs from the stored digest.
    DigestMismatch,
    /// The child-content digest could not be computed.
    DigestFailure,
    /// The stored creation time is not a valid Orchestration date-time.
    InvalidCapturedAt,
}

impl std::fmt::Display for GraphSnapshotProjectionErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::IdentityMismatch => {
                "persisted graph identity does not match the expected context"
            }
            Self::VersionMismatch => "persisted current version does not match the graph version",
            Self::InvalidProjection => "persisted graph projection is not canonical",
            Self::InvalidTopology => "persisted graph topology is invalid",
            Self::DigestMismatch => "persisted graph content digest does not match",
            Self::DigestFailure => "persisted graph content digest could not be computed",
            Self::InvalidCapturedAt => "persisted graph creation time is invalid",
        })
    }
}

impl std::error::Error for GraphSnapshotProjectionErrorV1 {}

/// Discards a payload-bearing failure without formatting it.
fn invalid<E>(_: E) -> Error {
    Error::InvalidProjection
}

/// Projects a State-persisted current graph into a canonical
/// [`GraphSnapshot`], or returns a closed error and no partial output.
pub fn project_persisted_current_graph_v1(
    persisted: &PersistedCurrentGraphV1,
    expected_project_id: &str,
    expected_graph_id: &str,
) -> Result<GraphSnapshot, GraphSnapshotProjectionErrorV1> {
    if persisted.project_id() != expected_project_id || persisted.graph_id() != expected_graph_id {
        return Err(Error::IdentityMismatch);
    }
    if persisted.current_version() != persisted.graph_version() {
        return Err(Error::VersionMismatch);
    }
    let graph_version = GraphVersionV1::try_new(persisted.graph_version()).map_err(invalid)?;

    let nodes = persisted
        .nodes()
        .iter()
        .map(node)
        .collect::<Result<Vec<_>, _>>()?;
    let edges = persisted
        .edges()
        .iter()
        .map(edge)
        .collect::<Result<Vec<_>, _>>()?;
    let graph = ExecutionGraph::from_parts(persisted.graph_id(), nodes, edges)
        .map_err(|_| Error::InvalidTopology)?;

    let digest = graph_content_digest_v1(&graph).map_err(|_| Error::DigestFailure)?;
    if digest != persisted.resulting_digest() {
        return Err(Error::DigestMismatch);
    }

    let captured_at = OrchestrationDateTimeV1::try_new(persisted.created_at().as_str())
        .map_err(|_| Error::InvalidCapturedAt)?;
    // ExecutionGraph iterates nodes in ascending node-ID UTF-8 byte order.
    let node_states = graph
        .nodes()
        .map(|node| {
            GraphSnapshotNodeState::try_new(
                node.node_id(),
                node.state(),
                Some(node.kind().clone()),
                None,
                node.code_sha().map(str::to_owned),
            )
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(invalid)?;
    GraphSnapshot::try_new(
        graph.graph_id(),
        graph_version,
        captured_at,
        node_states,
        Some(digest),
        None,
    )
    .map_err(invalid)
}

fn node(stored: &StateGraphNodeV1) -> Result<GraphNode, Error> {
    let kind = GraphNodeKind::new(stored.kind().to_owned()).map_err(invalid)?;
    let state = GraphNodeState::parse(stored.state()).map_err(invalid)?;
    let mut node =
        GraphNode::new(stored.node_id(), stored.graph_id(), kind, state).map_err(invalid)?;
    if let Some(title) = stored.title() {
        node = node.with_title(title);
    }
    if let Some(parent) = stored.parent_node_id() {
        node = node.with_parent_node_id(parent).map_err(invalid)?;
    }
    if let Some(attempt) = stored.attempt_number() {
        node = node.with_attempt_number(GraphNodeAttemptNumber::try_new(attempt).map_err(invalid)?);
    }
    if let Some(capabilities) = stored.required_capabilities() {
        let capabilities = capabilities
            .iter()
            .map(|capability| CapabilityName::new(capability.as_str()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(invalid)?;
        node = node.with_required_capabilities(capabilities);
    }
    if let Some(reference) = stored.task_capsule_ref() {
        node = node.with_task_capsule_ref(reference);
    }
    if let Some(workstream) = stored.workstream_id() {
        node = node.with_workstream_id(workstream).map_err(invalid)?;
    }
    if let Some(code_sha) = stored.code_sha() {
        node = node.with_code_sha(code_sha).map_err(invalid)?;
    }
    if let Some(workspace) = stored.workspace_id() {
        node = node.with_workspace_id(workspace).map_err(invalid)?;
    }
    if let Some(result) = stored.result_ref() {
        node = node.with_result_ref(result);
    }
    node = match stored.locked_reason() {
        None => node,
        Some(None) => node.with_null_locked_reason(),
        Some(Some(reason)) => node.with_locked_reason(reason),
    };
    if let Some(version) = stored.created_in_version() {
        node = node.with_created_in_version(GraphVersionV1::try_new(version).map_err(invalid)?);
    }
    Ok(node)
}

fn edge(stored: &StateGraphEdgeV1) -> Result<GraphEdge, Error> {
    let (id, graph, from, to) = (
        stored.edge_id(),
        stored.graph_id(),
        stored.from_node(),
        stored.to_node(),
    );
    // Kinds are matched against the canonical enum spellings; anything else
    // is refused, never defaulted.
    let edge = match stored.relation() {
        StateGraphEdgeRelationV1::Precedence(kind) => {
            let kind = [
                PrecedenceKind::RequiresAccepted,
                PrecedenceKind::RequiresIntegrated,
                PrecedenceKind::RequiresInterface,
            ]
            .into_iter()
            .find(|candidate| candidate.as_str() == kind)
            .ok_or(Error::InvalidProjection)?;
            GraphEdge::precedence(id, graph, from, to, kind)
        }
        StateGraphEdgeRelationV1::Control(kind) => {
            let kind = [
                ControlKind::OnPass,
                ControlKind::OnReject,
                ControlKind::OnFailure,
                ControlKind::OnBlocked,
                ControlKind::Escalate,
                ControlKind::ExpandsInto,
            ]
            .into_iter()
            .find(|candidate| candidate.as_str() == kind)
            .ok_or(Error::InvalidProjection)?;
            GraphEdge::control(id, graph, from, to, kind)
        }
    }
    .map_err(invalid)?;
    Ok(match stored.note() {
        Some(note) => edge.with_note(note),
        None => edge,
    })
}
