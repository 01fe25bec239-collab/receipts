//! Materialised snapshot and auditable mutation data. These records hold no
//! ExecutionGraph authority and perform no operations or version computation.

use super::{
    GraphError, GraphMutationOperationKind, GraphNodeKind, GraphNodeState, GraphVersionV1,
};
use crate::orchestration::{OrchestrationDateTimeV1, OrchestrationJsonObjectV1};

/// A frozen record constraint failed at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphRecordError {
    Identifier(GraphError),
    InvalidHex { field: &'static str, length: usize },
    EmptyReason,
    EmptyOperations,
    ResultingVersionBelowTwo,
}

impl std::fmt::Display for GraphRecordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Identifier(error) => error.fmt(f),
            Self::InvalidHex { field, length } => {
                write!(f, "{field} must be {length} lowercase ASCII hex digits")
            }
            Self::EmptyReason => f.write_str("reason must be non-empty"),
            Self::EmptyOperations => f.write_str("operations must contain at least one record"),
            Self::ResultingVersionBelowTwo => f.write_str("resulting_version must be at least 2"),
        }
    }
}

impl std::error::Error for GraphRecordError {}

impl From<GraphError> for GraphRecordError {
    fn from(error: GraphError) -> Self {
        Self::Identifier(error)
    }
}

fn validate_hex(
    field: &'static str,
    value: Option<&str>,
    length: usize,
) -> Result<(), GraphRecordError> {
    if value.is_some_and(|value| {
        value.len() != length
            || !value
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }) {
        return Err(GraphRecordError::InvalidHex { field, length });
    }
    Ok(())
}

/// One materialised node state, in the caller's snapshot order.
/// Required state cannot be omitted:
/// ```compile_fail
/// use receipts_orchestration::GraphSnapshotNodeState;
/// GraphSnapshotNodeState::try_new("n", None, None, None, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSnapshotNodeState {
    node_id: String,
    state: GraphNodeState,
    kind: Option<GraphNodeKind>,
    locked: Option<bool>,
    code_sha: Option<String>,
}

impl GraphSnapshotNodeState {
    pub fn try_new(
        node_id: impl Into<String>,
        state: GraphNodeState,
        kind: Option<GraphNodeKind>,
        locked: Option<bool>,
        code_sha: Option<String>,
    ) -> Result<Self, GraphRecordError> {
        let node_id = node_id.into();
        GraphError::validate_identifier("node_id", &node_id)?;
        validate_hex("code_sha", code_sha.as_deref(), 40)?;
        Ok(Self {
            node_id,
            state,
            kind,
            locked,
            code_sha,
        })
    }
    pub fn node_id(&self) -> &str {
        &self.node_id
    }
    pub fn state(&self) -> GraphNodeState {
        self.state
    }
    pub fn kind(&self) -> Option<&GraphNodeKind> {
        self.kind.as_ref()
    }
    pub fn locked(&self) -> Option<bool> {
        self.locked
    }
    pub fn code_sha(&self) -> Option<&str> {
        self.code_sha.as_deref()
    }
}

/// Materialised data for display or resume; not a mutable execution graph.
/// Required version, timestamp, and node-state collection are non-optional:
/// ```compile_fail
/// use receipts_orchestration::GraphSnapshot;
/// GraphSnapshot::try_new("g", None, None, None, None, None);
/// ```
/// Snapshot access grants no mutable node-state authority:
/// ```compile_fail
/// use receipts_orchestration::GraphSnapshot;
/// fn clear(snapshot: &mut GraphSnapshot) { snapshot.node_states().clear(); }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphSnapshot {
    graph_id: String,
    graph_version: GraphVersionV1,
    captured_at: OrchestrationDateTimeV1,
    node_states: Vec<GraphSnapshotNodeState>,
    resulting_digest: Option<String>,
    summary: Option<OrchestrationJsonObjectV1>,
}

impl GraphSnapshot {
    pub fn try_new(
        graph_id: impl Into<String>,
        graph_version: GraphVersionV1,
        captured_at: OrchestrationDateTimeV1,
        node_states: Vec<GraphSnapshotNodeState>,
        resulting_digest: Option<String>,
        summary: Option<OrchestrationJsonObjectV1>,
    ) -> Result<Self, GraphRecordError> {
        let graph_id = graph_id.into();
        GraphError::validate_identifier("graph_id", &graph_id)?;
        validate_hex("resulting_digest", resulting_digest.as_deref(), 64)?;
        Ok(Self {
            graph_id,
            graph_version,
            captured_at,
            node_states,
            resulting_digest,
            summary,
        })
    }
    pub fn graph_id(&self) -> &str {
        &self.graph_id
    }
    pub fn graph_version(&self) -> &GraphVersionV1 {
        &self.graph_version
    }
    pub fn captured_at(&self) -> &OrchestrationDateTimeV1 {
        &self.captured_at
    }
    pub fn node_states(&self) -> &[GraphSnapshotNodeState] {
        &self.node_states
    }
    pub fn resulting_digest(&self) -> Option<&str> {
        self.resulting_digest.as_deref()
    }
    pub fn summary(&self) -> Option<&OrchestrationJsonObjectV1> {
        self.summary.as_ref()
    }
}

/// Logical actor evidence. Role is an open string, including the empty string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphMutationActor {
    role: String,
    role_id: Option<String>,
}

impl GraphMutationActor {
    pub fn try_new(
        role: impl Into<String>,
        role_id: Option<String>,
    ) -> Result<Self, GraphRecordError> {
        if let Some(id) = &role_id {
            GraphError::validate_identifier("role_id", id)?;
        }
        Ok(Self {
            role: role.into(),
            role_id,
        })
    }
    pub fn role(&self) -> &str {
        &self.role
    }
    pub fn role_id(&self) -> Option<&str> {
        self.role_id.as_deref()
    }
}

/// One auditable operation description; construction never executes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphMutationOperation {
    op: GraphMutationOperationKind,
    node_id: Option<String>,
    edge_id: Option<String>,
    detail: Option<OrchestrationJsonObjectV1>,
}

impl GraphMutationOperation {
    pub fn try_new(
        op: GraphMutationOperationKind,
        node_id: Option<String>,
        edge_id: Option<String>,
        detail: Option<OrchestrationJsonObjectV1>,
    ) -> Result<Self, GraphRecordError> {
        if let Some(id) = &node_id {
            GraphError::validate_identifier("node_id", id)?;
        }
        if let Some(id) = &edge_id {
            GraphError::validate_identifier("edge_id", id)?;
        }
        Ok(Self {
            op,
            node_id,
            edge_id,
            detail,
        })
    }
    pub fn op(&self) -> GraphMutationOperationKind {
        self.op
    }
    pub fn node_id(&self) -> Option<&str> {
        self.node_id.as_deref()
    }
    pub fn edge_id(&self) -> Option<&str> {
        self.edge_id.as_deref()
    }
    pub fn detail(&self) -> Option<&OrchestrationJsonObjectV1> {
        self.detail.as_ref()
    }
}

/// An auditable change record. Versions are supplied evidence; no adjacency
/// rule, automatic version update, or execution is performed.
/// Required fields cannot be omitted:
/// ```compile_fail
/// use receipts_orchestration::GraphMutation;
/// GraphMutation::try_new("m", "g", None, None, None, "reason", None, None, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphMutation {
    mutation_id: String,
    graph_id: String,
    parent_version: GraphVersionV1,
    resulting_version: GraphVersionV1,
    actor: GraphMutationActor,
    reason: String,
    created_at: OrchestrationDateTimeV1,
    operations: Vec<GraphMutationOperation>,
    resulting_digest: Option<String>,
}

impl GraphMutation {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        mutation_id: impl Into<String>,
        graph_id: impl Into<String>,
        parent_version: GraphVersionV1,
        resulting_version: GraphVersionV1,
        actor: GraphMutationActor,
        reason: impl Into<String>,
        created_at: OrchestrationDateTimeV1,
        operations: Vec<GraphMutationOperation>,
        resulting_digest: Option<String>,
    ) -> Result<Self, GraphRecordError> {
        let mutation_id = mutation_id.into();
        let graph_id = graph_id.into();
        let reason = reason.into();
        GraphError::validate_identifier("mutation_id", &mutation_id)?;
        GraphError::validate_identifier("graph_id", &graph_id)?;
        if resulting_version.as_str() == "1" {
            return Err(GraphRecordError::ResultingVersionBelowTwo);
        }
        if reason.is_empty() {
            return Err(GraphRecordError::EmptyReason);
        }
        if operations.is_empty() {
            return Err(GraphRecordError::EmptyOperations);
        }
        validate_hex("resulting_digest", resulting_digest.as_deref(), 64)?;
        Ok(Self {
            mutation_id,
            graph_id,
            parent_version,
            resulting_version,
            actor,
            reason,
            created_at,
            operations,
            resulting_digest,
        })
    }
    pub fn mutation_id(&self) -> &str {
        &self.mutation_id
    }
    pub fn graph_id(&self) -> &str {
        &self.graph_id
    }
    pub fn parent_version(&self) -> &GraphVersionV1 {
        &self.parent_version
    }
    pub fn resulting_version(&self) -> &GraphVersionV1 {
        &self.resulting_version
    }
    pub fn actor(&self) -> &GraphMutationActor {
        &self.actor
    }
    pub fn reason(&self) -> &str {
        &self.reason
    }
    pub fn created_at(&self) -> &OrchestrationDateTimeV1 {
        &self.created_at
    }
    pub fn operations(&self) -> &[GraphMutationOperation] {
        &self.operations
    }
    pub fn resulting_digest(&self) -> Option<&str> {
        self.resulting_digest.as_deref()
    }
}
