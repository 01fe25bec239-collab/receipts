//! `GraphEdge` — precedence or control relation.
//!
//! The graph distinguishes exactly two edge classes, and the classes are
//! mutually exclusive (frozen V1.3.1 contract):
//!
//! * a [`PRECEDENCE`](GraphEdge::precedence) edge carries exactly one
//!   [`PrecedenceKind`] and never a control kind;
//! * a [`CONTROL`](GraphEdge::control) edge carries exactly one
//!   [`ControlKind`] and never a precedence kind.
//!
//! Exclusivity is enforced by construction through [`GraphEdgeRelation`]:
//! an edge that mixes the classes or omits its class-specific kind cannot be
//! represented.
//!
//! Only `PRECEDENCE` edges participate in dependency DAG analysis and
//! precedence-cycle rejection. `CONTROL` edges record outcome transitions
//! (why nodes exist) and are not scheduling prerequisites; they are never
//! traversed by cycle validation.

use crate::error::GraphError;

/// Edge class (frozen contract: `PRECEDENCE` or `CONTROL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeClass {
    /// `PRECEDENCE`: forms the scheduling DAG; must remain acyclic.
    Precedence,
    /// `CONTROL`: records an outcome transition; never a scheduling
    /// prerequisite and never traversed by precedence-cycle validation.
    Control,
}

impl EdgeClass {
    /// The frozen storage representation.
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeClass::Precedence => "PRECEDENCE",
            EdgeClass::Control => "CONTROL",
        }
    }
}

/// Supported precedence relationship kind (frozen contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrecedenceKind {
    /// `REQUIRES_ACCEPTED`: dependency must be accepted before dispatch.
    RequiresAccepted,
    /// `REQUIRES_INTEGRATED`: dependency must be merged into the branch first.
    RequiresIntegrated,
    /// `REQUIRES_INTERFACE`: only the interface must be frozen first;
    /// implementations may proceed in parallel.
    RequiresInterface,
}

impl PrecedenceKind {
    /// The frozen storage representation.
    pub fn as_str(self) -> &'static str {
        match self {
            PrecedenceKind::RequiresAccepted => "REQUIRES_ACCEPTED",
            PrecedenceKind::RequiresIntegrated => "REQUIRES_INTEGRATED",
            PrecedenceKind::RequiresInterface => "REQUIRES_INTERFACE",
        }
    }
}

/// Supported control relationship kind (frozen contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ControlKind {
    /// `ON_PASS`.
    OnPass,
    /// `ON_REJECT`.
    OnReject,
    /// `ON_FAILURE`.
    OnFailure,
    /// `ON_BLOCKED`.
    OnBlocked,
    /// `ESCALATE`.
    Escalate,
    /// `EXPANDS_INTO`.
    ExpandsInto,
}

impl ControlKind {
    /// The frozen storage representation.
    pub fn as_str(self) -> &'static str {
        match self {
            ControlKind::OnPass => "ON_PASS",
            ControlKind::OnReject => "ON_REJECT",
            ControlKind::OnFailure => "ON_FAILURE",
            ControlKind::OnBlocked => "ON_BLOCKED",
            ControlKind::Escalate => "ESCALATE",
            ControlKind::ExpandsInto => "EXPANDS_INTO",
        }
    }
}

/// The class-and-kind pair of an edge.
///
/// This type is what makes class exclusivity structural: a precedence-classed
/// edge always carries exactly one precedence kind, a control-classed edge
/// always carries exactly one control kind, and neither ever carries both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphEdgeRelation {
    /// A `PRECEDENCE` edge with its required precedence kind.
    Precedence(PrecedenceKind),
    /// A `CONTROL` edge with its required control kind.
    Control(ControlKind),
}

impl GraphEdgeRelation {
    /// The edge class of this relation.
    pub fn class(&self) -> EdgeClass {
        match self {
            GraphEdgeRelation::Precedence(_) => EdgeClass::Precedence,
            GraphEdgeRelation::Control(_) => EdgeClass::Control,
        }
    }

    /// The precedence kind when this is a `PRECEDENCE` relation.
    pub fn precedence_kind(&self) -> Option<PrecedenceKind> {
        match self {
            GraphEdgeRelation::Precedence(kind) => Some(*kind),
            GraphEdgeRelation::Control(_) => None,
        }
    }

    /// The control kind when this is a `CONTROL` relation.
    pub fn control_kind(&self) -> Option<ControlKind> {
        match self {
            GraphEdgeRelation::Control(kind) => Some(*kind),
            GraphEdgeRelation::Precedence(_) => None,
        }
    }

    /// The frozen representation of the class-specific kind.
    pub fn kind_as_str(&self) -> &'static str {
        match self {
            GraphEdgeRelation::Precedence(kind) => kind.as_str(),
            GraphEdgeRelation::Control(kind) => kind.as_str(),
        }
    }
}

/// A directed relation between two nodes of one execution graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphEdge {
    edge_id: String,
    from_node: String,
    to_node: String,
    relation: GraphEdgeRelation,
}

impl GraphEdge {
    /// Creates a `PRECEDENCE` edge carrying exactly one precedence kind.
    pub fn precedence(
        edge_id: impl Into<String>,
        from_node: impl Into<String>,
        to_node: impl Into<String>,
        kind: PrecedenceKind,
    ) -> Result<Self, GraphError> {
        Self::new(
            edge_id,
            from_node,
            to_node,
            GraphEdgeRelation::Precedence(kind),
        )
    }

    /// Creates a `CONTROL` edge carrying exactly one control kind. Control
    /// edges are recorded but never treated as scheduling prerequisites.
    pub fn control(
        edge_id: impl Into<String>,
        from_node: impl Into<String>,
        to_node: impl Into<String>,
        kind: ControlKind,
    ) -> Result<Self, GraphError> {
        Self::new(
            edge_id,
            from_node,
            to_node,
            GraphEdgeRelation::Control(kind),
        )
    }

    fn new(
        edge_id: impl Into<String>,
        from_node: impl Into<String>,
        to_node: impl Into<String>,
        relation: GraphEdgeRelation,
    ) -> Result<Self, GraphError> {
        let edge_id = edge_id.into();
        let from_node = from_node.into();
        let to_node = to_node.into();
        GraphError::validate_identifier("edge_id", &edge_id)?;
        GraphError::validate_identifier("from_node", &from_node)?;
        GraphError::validate_identifier("to_node", &to_node)?;
        Ok(Self {
            edge_id,
            from_node,
            to_node,
            relation,
        })
    }

    /// The stable edge identity.
    pub fn edge_id(&self) -> &str {
        &self.edge_id
    }

    /// The source node id.
    pub fn from_node(&self) -> &str {
        &self.from_node
    }

    /// The target node id.
    pub fn to_node(&self) -> &str {
        &self.to_node
    }

    /// The edge class (`PRECEDENCE` or `CONTROL`).
    pub fn class(&self) -> EdgeClass {
        self.relation.class()
    }

    /// The full class-and-kind relation.
    pub fn relation(&self) -> GraphEdgeRelation {
        self.relation
    }

    /// The precedence kind, present only for `PRECEDENCE` edges.
    pub fn precedence_kind(&self) -> Option<PrecedenceKind> {
        self.relation.precedence_kind()
    }

    /// The control kind, present only for `CONTROL` edges.
    pub fn control_kind(&self) -> Option<ControlKind> {
        self.relation.control_kind()
    }
}
