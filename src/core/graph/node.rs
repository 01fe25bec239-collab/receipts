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

use std::borrow::Cow;

use crate::error::GraphError;

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
    /// Creates a capability name from a non-empty string. Fails explicitly on
    /// empty input.
    pub fn new(value: impl Into<String>) -> Result<Self, GraphError> {
        let value = value.into();
        if value.is_empty() {
            return Err(GraphError::EmptyIdentifier {
                field: "required_capabilities entry",
            });
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

/// One unit of planned work in the execution graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNode {
    node_id: String,
    kind: GraphNodeKind,
    required_capabilities: Vec<CapabilityName>,
}

impl GraphNode {
    /// Creates a node, failing explicitly on malformed input:
    ///
    /// * `node_id` must be non-empty and at most 200 scalar values;
    /// * `kind` must be non-empty (`GraphNodeKind::new` enforces this);
    /// * capability entries are [`CapabilityName`] values, which are
    ///   validated non-empty at their own construction.
    ///
    /// `required_capabilities` is stored verbatim (order preserved) and is
    /// data only in this slice.
    pub fn new(
        node_id: impl Into<String>,
        kind: GraphNodeKind,
        required_capabilities: Vec<CapabilityName>,
    ) -> Result<Self, GraphError> {
        let node_id = node_id.into();
        GraphError::validate_identifier("node_id", &node_id)?;
        Ok(Self {
            node_id,
            kind,
            required_capabilities,
        })
    }

    /// The stable node identity.
    pub fn node_id(&self) -> &str {
        &self.node_id
    }

    /// The extensible node kind.
    pub fn kind(&self) -> &GraphNodeKind {
        &self.kind
    }

    /// Required capabilities, verbatim and data only. Empty means
    /// FREE-executable by convention elsewhere; nothing here interprets them.
    pub fn required_capabilities(&self) -> &[CapabilityName] {
        &self.required_capabilities
    }
}
