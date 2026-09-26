//! `GraphNode` — one unit of planned or executed work.
//!
//! The node kind is an **extensible string**, not a closed enum: a new kind
//! must not require a schema or code change. Well-known kinds are provided as
//! constants for convenience only.
//!
//! `required_capabilities` is data only in this slice: it is stored and
//! returned verbatim and is never interpreted. Capability admission,
//! entitlement, tier, provider availability, and routing policy are outside
//! this slice.
//!
//! Every frozen `GraphNode` field is preserved physically. Optional fields keep
//! their absence; only `locked_reason` also admits an explicit null. No value
//! is defaulted, normalized, or interpreted, and physical data grants no
//! execution or admission authority.

use std::borrow::Cow;

use receipts_workspace_execution::CommitSha;

use crate::error::GraphError;
use crate::node_state::GraphNodeState;
use crate::version::GraphVersionV1;

/// Extensible node-kind string (frozen contract: not an enum).
///
/// Well-known values are exposed as associated constants; any other non-empty
/// value is equally valid.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GraphNodeKind(Cow<'static, str>);

impl GraphNodeKind {
    /// `GOAL`.
    pub const GOAL: Self = Self::new_unchecked("GOAL");
    /// `WORKSTREAM`.
    pub const WORKSTREAM: Self = Self::new_unchecked("WORKSTREAM");
    /// `TASK`.
    pub const TASK: Self = Self::new_unchecked("TASK");
    /// `ATTEMPT`.
    pub const ATTEMPT: Self = Self::new_unchecked("ATTEMPT");
    /// `IMPLEMENTATION`.
    pub const IMPLEMENTATION: Self = Self::new_unchecked("IMPLEMENTATION");
    /// `REVIEW`.
    pub const REVIEW: Self = Self::new_unchecked("REVIEW");
    /// `REPAIR`.
    pub const REPAIR: Self = Self::new_unchecked("REPAIR");
    /// `DETERMINISTIC_CHECK`.
    pub const DETERMINISTIC_CHECK: Self = Self::new_unchecked("DETERMINISTIC_CHECK");
    /// `ROUTING`.
    pub const ROUTING: Self = Self::new_unchecked("ROUTING");
    /// `INTEGRATION`.
    pub const INTEGRATION: Self = Self::new_unchecked("INTEGRATION");
    /// `HUMAN_GATE`.
    pub const HUMAN_GATE: Self = Self::new_unchecked("HUMAN_GATE");
    /// `GOAL_EVALUATION`.
    pub const GOAL_EVALUATION: Self = Self::new_unchecked("GOAL_EVALUATION");

    const fn new_unchecked(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }

    /// Creates an extensible kind from any non-empty string. Fails explicitly
    /// on empty input; unknown kinds are valid by contract.
    pub fn new(value: impl Into<Cow<'static, str>>) -> Result<Self, GraphError> {
        let value = value.into();
        if value.is_empty() {
            return Err(GraphError::EmptyIdentifier { field: "kind" });
        }
        Ok(Self(value))
    }

    /// The extensible kind string.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for GraphNodeKind {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A namespaced capability id carried by a node (e.g. `graph.core`).
///
/// Data only: this slice never interprets capability content.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CapabilityName(String);

impl CapabilityName {
    /// Creates an open capability id matching
    /// `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`, preserving the input exactly.
    /// Malformed syntax fails explicitly; no catalog or length limit applies.
    pub fn new(value: impl Into<String>) -> Result<Self, GraphError> {
        let value = value.into();
        if !value.contains('.')
            || !value.split('.').all(|component| {
                let mut bytes = component.bytes();
                matches!(bytes.next(), Some(b'a'..=b'z'))
                    && bytes.all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
            })
        {
            return Err(GraphError::InvalidCapabilitySyntax { value });
        }
        Ok(Self(value))
    }

    /// The capability string, verbatim.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for CapabilityName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// A canonical positive node attempt number with no numeric or digit-count
/// ceiling (`[1-9][0-9]*`). The value is lexical: no conversion, ordering, or
/// arithmetic is performed, and no attempt semantics are implied.
///
/// Raw construction cannot bypass validation:
/// ```compile_fail
/// use receipts_orchestration::GraphNodeAttemptNumber;
/// let invalid = GraphNodeAttemptNumber(String::from("0"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphNodeAttemptNumber(String);

impl GraphNodeAttemptNumber {
    /// Accepts only canonical positive ASCII decimal, preserving it exactly.
    pub fn try_new(value: impl Into<String>) -> Result<Self, GraphError> {
        let value = value.into();
        // Shares only the lexical positive-decimal check of the graph version
        // carrier; the value is never treated as a version.
        if GraphVersionV1::try_new(value.as_str()).is_err() {
            return Err(GraphError::InvalidAttemptNumber { value });
        }
        Ok(Self(value))
    }

    /// The canonical decimal text, verbatim.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One unit of planned work in the execution graph.
///
/// Required fields are supplied to [`GraphNode::new`]; each optional field is
/// absent until set through its validating `with_*` method. Fields are private,
/// so construction cannot bypass validation:
/// ```compile_fail
/// use receipts_orchestration::{GraphNode, GraphNodeKind, GraphNodeState};
/// let node = GraphNode {
///     node_id: String::new(),
///     graph_id: String::new(),
///     kind: GraphNodeKind::TASK,
///     state: GraphNodeState::Planned,
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    node_id: String,
    graph_id: String,
    kind: GraphNodeKind,
    state: GraphNodeState,
    title: Option<String>,
    parent_node_id: Option<String>,
    attempt_number: Option<GraphNodeAttemptNumber>,
    required_capabilities: Option<Vec<CapabilityName>>,
    task_capsule_ref: Option<String>,
    workstream_id: Option<String>,
    code_sha: Option<CommitSha>,
    workspace_id: Option<String>,
    result_ref: Option<String>,
    /// Outer `None`: absent; `Some(None)`: explicit null.
    locked_reason: Option<Option<String>>,
    created_in_version: Option<GraphVersionV1>,
}

impl GraphNode {
    /// Creates a node with every optional field absent, failing explicitly on
    /// malformed input:
    ///
    /// * `node_id` and `graph_id` must be non-empty and at most 200 scalar
    ///   values;
    /// * `kind` must be non-empty (`GraphNodeKind::new` enforces this).
    ///
    /// `state` is required and stored exactly as supplied. `graph_id` names
    /// the containing graph and is checked by [`ExecutionGraph`] on insertion;
    /// it is never defaulted.
    ///
    /// [`ExecutionGraph`]: crate::ExecutionGraph
    pub fn new(
        node_id: impl Into<String>,
        graph_id: impl Into<String>,
        kind: GraphNodeKind,
        state: GraphNodeState,
    ) -> Result<Self, GraphError> {
        let node_id = node_id.into();
        let graph_id = graph_id.into();
        GraphError::validate_identifier("node_id", &node_id)?;
        GraphError::validate_identifier("graph_id", &graph_id)?;
        Ok(Self {
            node_id,
            graph_id,
            kind,
            state,
            title: None,
            parent_node_id: None,
            attempt_number: None,
            required_capabilities: None,
            task_capsule_ref: None,
            workstream_id: None,
            code_sha: None,
            workspace_id: None,
            result_ref: None,
            locked_reason: None,
            created_in_version: None,
        })
    }

    /// Sets `title`; any string, including empty, is preserved exactly.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets `parent_node_id` after identifier validation. Recorded only; no
    /// parent resolution is performed.
    pub fn with_parent_node_id(
        mut self,
        parent_node_id: impl Into<String>,
    ) -> Result<Self, GraphError> {
        let parent_node_id = parent_node_id.into();
        GraphError::validate_identifier("parent_node_id", &parent_node_id)?;
        self.parent_node_id = Some(parent_node_id);
        Ok(self)
    }

    /// Sets `attempt_number`.
    pub fn with_attempt_number(mut self, attempt_number: GraphNodeAttemptNumber) -> Self {
        self.attempt_number = Some(attempt_number);
        self
    }

    /// Sets `required_capabilities` as present, preserving order and
    /// duplicates. A present empty list is distinct from absence.
    pub fn with_required_capabilities(mut self, capabilities: Vec<CapabilityName>) -> Self {
        self.required_capabilities = Some(capabilities);
        self
    }

    /// Sets `task_capsule_ref`; any string, including empty, is preserved
    /// exactly and never resolved.
    pub fn with_task_capsule_ref(mut self, task_capsule_ref: impl Into<String>) -> Self {
        self.task_capsule_ref = Some(task_capsule_ref.into());
        self
    }

    /// Sets `workstream_id` after identifier validation.
    pub fn with_workstream_id(
        mut self,
        workstream_id: impl Into<String>,
    ) -> Result<Self, GraphError> {
        let workstream_id = workstream_id.into();
        GraphError::validate_identifier("workstream_id", &workstream_id)?;
        self.workstream_id = Some(workstream_id);
        Ok(self)
    }

    /// Sets `code_sha` through the canonical Workspace [`CommitSha`], which
    /// accepts exactly 40 lowercase ASCII hex digits.
    pub fn with_code_sha(mut self, code_sha: impl Into<String>) -> Result<Self, GraphError> {
        let code_sha = code_sha.into();
        let parsed = CommitSha::parse(&code_sha)
            .map_err(|_| GraphError::InvalidCodeSha { value: code_sha })?;
        self.code_sha = Some(parsed);
        Ok(self)
    }

    /// Sets `workspace_id` after identifier validation.
    pub fn with_workspace_id(
        mut self,
        workspace_id: impl Into<String>,
    ) -> Result<Self, GraphError> {
        let workspace_id = workspace_id.into();
        GraphError::validate_identifier("workspace_id", &workspace_id)?;
        self.workspace_id = Some(workspace_id);
        Ok(self)
    }

    /// Sets `result_ref`; any string, including empty, is preserved exactly
    /// and never resolved.
    pub fn with_result_ref(mut self, result_ref: impl Into<String>) -> Self {
        self.result_ref = Some(result_ref.into());
        self
    }

    /// Sets `locked_reason` to a string; any string, including empty, is
    /// preserved exactly.
    pub fn with_locked_reason(mut self, locked_reason: impl Into<String>) -> Self {
        self.locked_reason = Some(Some(locked_reason.into()));
        self
    }

    /// Sets `locked_reason` to explicit null, distinct from absence.
    pub fn with_null_locked_reason(mut self) -> Self {
        self.locked_reason = Some(None);
        self
    }

    /// Sets `created_in_version`.
    pub fn with_created_in_version(mut self, created_in_version: GraphVersionV1) -> Self {
        self.created_in_version = Some(created_in_version);
        self
    }

    /// The stable node identity.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// The identity of the graph this node declares it belongs to.
    pub fn graph_id(&self) -> &str {
        &self.graph_id
    }

    /// The extensible node kind.
    pub fn kind(&self) -> &GraphNodeKind {
        &self.kind
    }

    /// The exact stored current state.
    pub fn state(&self) -> GraphNodeState {
        self.state
    }

    /// The title, or `None` when absent.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The expansion parent id, or `None` when absent.
    pub fn parent_node_id(&self) -> Option<&str> {
        self.parent_node_id.as_deref()
    }

    /// The attempt number, or `None` when absent.
    pub fn attempt_number(&self) -> Option<&GraphNodeAttemptNumber> {
        self.attempt_number.as_ref()
    }

    /// Required capabilities, verbatim and data only, or `None` when absent.
    /// A present empty slice means FREE-executable by convention elsewhere;
    /// nothing here interprets them.
    pub fn required_capabilities(&self) -> Option<&[CapabilityName]> {
        self.required_capabilities.as_deref()
    }

    /// The task capsule reference, or `None` when absent.
    pub fn task_capsule_ref(&self) -> Option<&str> {
        self.task_capsule_ref.as_deref()
    }

    /// The workstream id, or `None` when absent.
    pub fn workstream_id(&self) -> Option<&str> {
        self.workstream_id.as_deref()
    }

    /// The code SHA, or `None` when absent.
    pub fn code_sha(&self) -> Option<&str> {
        self.code_sha.as_ref().map(CommitSha::as_str)
    }

    /// The workspace id, or `None` when absent.
    pub fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }

    /// The result reference, or `None` when absent.
    pub fn result_ref(&self) -> Option<&str> {
        self.result_ref.as_deref()
    }

    /// The lock reason: `None` when absent, `Some(None)` for explicit null,
    /// `Some(Some(reason))` for a string.
    pub fn locked_reason(&self) -> Option<Option<&str>> {
        self.locked_reason.as_ref().map(Option::as_deref)
    }

    /// The graph version this node was created in, or `None` when absent.
    pub fn created_in_version(&self) -> Option<&GraphVersionV1> {
        self.created_in_version.as_ref()
    }
}
