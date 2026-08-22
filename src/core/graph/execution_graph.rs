//! `ExecutionGraph` — the versioned-plan graph core of this slice.
//!
//! This slice owns exactly one thing: the structural precedence-DAG core plus
//! deterministic precedence-cycle detection and rejection.
//!
//! Mutation rules honored here (frozen contract):
//!
//! * before accepting a structural precedence-edge addition, the resulting
//!   precedence subgraph is validated; a candidate that introduces a
//!   precedence cycle is rejected atomically — never repaired, never worked
//!   around by deleting another edge, reversing edges, or ignoring the
//!   cycle;
//! * `CONTROL` edges never participate in precedence-cycle traversal, so a
//!   conceptual control loop cannot cause false rejection;
//! * equivalent graph input produces identical validation behavior: node and
//!   edge storage is ordered (`BTreeMap`), traversal expands successors in
//!   ascending node order, and whole-graph scans examine edges in ascending
//!   edge-id order, so the identified closing edge and closing path are a
//!   pure function of graph content;
//! * the rejection names the candidate edge/change deterministically.
//!
//! Explicitly out of this slice: scheduler semantics, durable persistence,
//! digests, snapshots, results, budgets, admission, routing. Nothing here
//! interprets capabilities or performs entitlement, tier, provider, network,
//! workspace, git, or credential work. Rust `std` only.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::edge::{EdgeClass, GraphEdge};
use crate::error::GraphError;
use crate::node::GraphNode;

/// Adjacency view of the precedence subgraph: source node -> ordered set of
/// `(target node, edge id)`. Sorted structures keep traversal order a pure
/// function of graph content.
type PrecedenceAdjacency<'a> = BTreeMap<&'a str, BTreeSet<(&'a str, &'a str)>>;

/// The authoritative plan graph: nodes plus precedence and control edges.
///
/// Structural changes are validated before acceptance and applied atomically
/// or not at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionGraph {
    graph_id: String,
    nodes: BTreeMap<String, GraphNode>,
    edges: BTreeMap<String, GraphEdge>,
}

impl ExecutionGraph {
    /// Creates an empty graph, failing explicitly on a malformed `graph_id`.
    pub fn new(graph_id: impl Into<String>) -> Result<Self, GraphError> {
        let graph_id = graph_id.into();
        GraphError::validate_identifier("graph_id", &graph_id)?;
        Ok(Self {
            graph_id,
            nodes: BTreeMap::new(),
            edges: BTreeMap::new(),
        })
    }

    /// Builds a graph from complete node and edge sets in one step.
    ///
    /// Every structural invariant is validated on the assembled graph:
    /// duplicate node/edge ids are rejected, edges referencing nodes absent
    /// from the node set are rejected, and a precedence cycle anywhere in the
    /// resulting precedence subgraph rejects the whole construction with the
    /// closing edge identified. Checks run in deterministic (ascending id)
    /// order so equivalent input yields identical outcomes regardless of the
    /// order elements are listed in.
    pub fn from_parts(
        graph_id: impl Into<String>,
        nodes: Vec<GraphNode>,
        edges: Vec<GraphEdge>,
    ) -> Result<Self, GraphError> {
        let mut graph = Self::new(graph_id)?;

        for node in nodes {
            graph.add_node(node)?;
        }

        // Validate edges in ascending edge-id order so reordered equivalent
        // input produces byte-identical failure identification.
        let mut sorted_edges = edges;
        sorted_edges.sort_by(|a, b| a.edge_id().cmp(b.edge_id()));
        if let Some(duplicate) = sorted_edges
            .windows(2)
            .find(|pair| pair[0].edge_id() == pair[1].edge_id())
        {
            return Err(GraphError::DuplicateEdgeId {
                edge_id: duplicate[0].edge_id().to_owned(),
            });
        }
        for edge in &sorted_edges {
            for endpoint in [edge.from_node(), edge.to_node()] {
                if !graph.nodes.contains_key(endpoint) {
                    return Err(GraphError::UnknownNodeReference {
                        edge_id: edge.edge_id().to_owned(),
                        node_id: endpoint.to_owned(),
                    });
                }
            }
        }
        for edge in sorted_edges {
            graph.edges.insert(edge.edge_id().to_owned(), edge);
        }

        graph.validate_precedence_subgraph()?;
        Ok(graph)
    }

    /// The stable graph identity.
    pub fn graph_id(&self) -> &str {
        &self.graph_id
    }

    /// Adds a node, failing explicitly on a duplicate node id.
    pub fn add_node(&mut self, node: GraphNode) -> Result<(), GraphError> {
        if self.nodes.contains_key(node.node_id()) {
            return Err(GraphError::DuplicateNodeId {
                node_id: node.node_id().to_owned(),
            });
        }
        self.nodes.insert(node.node_id().to_owned(), node);
        Ok(())
    }

    /// Adds an edge after validating the resulting topology.
    ///
    /// Precedence candidates that introduce a precedence cycle are rejected
    /// atomically (the graph is left unchanged) with the candidate edge and
    /// the concrete existing precedence path it closes named in the error.
    /// Control edges are stored without precedence-cycle traversal.
    pub fn add_edge(&mut self, edge: GraphEdge) -> Result<(), GraphError> {
        self.validate_edge_addition(&edge)?;
        self.edges.insert(edge.edge_id().to_owned(), edge);
        Ok(())
    }

    /// Dry-runs [`ExecutionGraph::add_edge`] without mutating the graph.
    ///
    /// Performs the exact acceptance checks the mutating call performs:
    /// duplicate edge id, endpoint existence, and — for precedence
    /// candidates — deterministic cycle analysis of the resulting precedence
    /// subgraph.
    pub fn validate_edge_addition(&self, candidate: &GraphEdge) -> Result<(), GraphError> {
        if self.edges.contains_key(candidate.edge_id()) {
            return Err(GraphError::DuplicateEdgeId {
                edge_id: candidate.edge_id().to_owned(),
            });
        }
        for endpoint in [candidate.from_node(), candidate.to_node()] {
            if !self.nodes.contains_key(endpoint) {
                return Err(GraphError::UnknownNodeReference {
                    edge_id: candidate.edge_id().to_owned(),
                    node_id: endpoint.to_owned(),
                });
            }
        }
        if candidate.class() == EdgeClass::Precedence {
            self.reject_if_precedence_cycle(candidate)?;
        }
        Ok(())
    }

    /// Reports whether the current precedence subgraph contains a cycle.
    /// Control edges are invisible to this analysis.
    pub fn has_precedence_cycle(&self) -> bool {
        self.find_closed_precedence_cycle().is_some()
    }

    /// Validates the whole precedence subgraph for acyclicity, identifying
    /// deterministically the first closing edge when a cycle exists.
    fn validate_precedence_subgraph(&self) -> Result<(), GraphError> {
        if let Some(offender) = self.find_closed_precedence_cycle() {
            return Err(offender);
        }
        Ok(())
    }

    /// Scans precedence edges in ascending edge-id order and returns the
    /// rejection for the first edge whose tail can already reach its head
    /// through other precedence edges — i.e. the edge that closes a cycle.
    fn find_closed_precedence_cycle(&self) -> Option<GraphError> {
        let adjacency = self.precedence_adjacency();
        for edge in self.edges.values() {
            if edge.class() != EdgeClass::Precedence {
                continue;
            }
            if let Some((nodes, edge_ids)) =
                find_precedence_path(&adjacency, edge.to_node(), edge.from_node())
            {
                return Some(GraphError::PrecedenceCycleRejected {
                    candidate_edge_id: edge.edge_id().to_owned(),
                    candidate_from_node: edge.from_node().to_owned(),
                    candidate_to_node: edge.to_node().to_owned(),
                    closing_path_nodes: nodes,
                    closing_path_edge_ids: edge_ids,
                });
            }
        }
        None
    }

    /// Rejects a candidate precedence edge iff accepting it would introduce a
    /// precedence cycle: a self-loop, or an existing precedence path from the
    /// candidate's target back to its source. Control edges are absent from
    /// the adjacency and therefore from the search.
    fn reject_if_precedence_cycle(&self, candidate: &GraphEdge) -> Result<(), GraphError> {
        if candidate.from_node() == candidate.to_node() {
            return Err(GraphError::PrecedenceCycleRejected {
                candidate_edge_id: candidate.edge_id().to_owned(),
                candidate_from_node: candidate.from_node().to_owned(),
                candidate_to_node: candidate.to_node().to_owned(),
                closing_path_nodes: Vec::new(),
                closing_path_edge_ids: Vec::new(),
            });
        }
        let adjacency = self.precedence_adjacency();
        if let Some((nodes, edge_ids)) =
            find_precedence_path(&adjacency, candidate.to_node(), candidate.from_node())
        {
            return Err(GraphError::PrecedenceCycleRejected {
                candidate_edge_id: candidate.edge_id().to_owned(),
                candidate_from_node: candidate.from_node().to_owned(),
                candidate_to_node: candidate.to_node().to_owned(),
                closing_path_nodes: nodes,
                closing_path_edge_ids: edge_ids,
            });
        }
        Ok(())
    }

    /// Builds the precedence-only adjacency of the graph. Control edges are
    /// deliberately excluded: they are outside precedence-cycle traversal.
    fn precedence_adjacency(&self) -> PrecedenceAdjacency<'_> {
        let mut adjacency: PrecedenceAdjacency<'_> = BTreeMap::new();
        for edge in self.edges.values() {
            if edge.class() == EdgeClass::Precedence {
                adjacency
                    .entry(edge.from_node())
                    .or_default()
                    .insert((edge.to_node(), edge.edge_id()));
            }
        }
        adjacency
    }

    /// Looks up a node by id.
    pub fn node(&self, node_id: &str) -> Option<&GraphNode> {
        self.nodes.get(node_id)
    }

    /// Looks up an edge by id.
    pub fn edge(&self, edge_id: &str) -> Option<&GraphEdge> {
        self.edges.get(edge_id)
    }

    /// Whether a node with this id exists.
    pub fn contains_node(&self, node_id: &str) -> bool {
        self.nodes.contains_key(node_id)
    }

    /// Whether an edge with this id exists.
    pub fn contains_edge(&self, edge_id: &str) -> bool {
        self.edges.contains_key(edge_id)
    }

    /// All nodes in ascending node-id order (deterministic).
    pub fn nodes(&self) -> impl Iterator<Item = &GraphNode> {
        self.nodes.values()
    }

    /// All edges in ascending edge-id order (deterministic).
    pub fn edges(&self) -> impl Iterator<Item = &GraphEdge> {
        self.edges.values()
    }

    /// Precedence-class edges only, ascending edge-id order.
    pub fn precedence_edges(&self) -> impl Iterator<Item = &GraphEdge> {
        self.edges()
            .filter(|edge| edge.class() == EdgeClass::Precedence)
    }

    /// Control-class edges only, ascending edge-id order.
    pub fn control_edges(&self) -> impl Iterator<Item = &GraphEdge> {
        self.edges()
            .filter(|edge| edge.class() == EdgeClass::Control)
    }

    /// Number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of edges (both classes).
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
}

/// Finds an existing precedence path from `start` back to `target` using
/// breadth-first search over the precedence adjacency, expanding successors
/// in ascending `(target node, edge id)` order.
///
/// Because the adjacency is fully ordered, the returned path — and therefore
/// the identified cycle — is a pure function of graph content: equivalent
/// graphs produce identical paths regardless of insertion history.
///
/// Returns `(node ids, edge ids)` where node ids run
/// `[start, .., target]` and `edge_ids[i]` joins `node_ids[i]` to
/// `node_ids[i + 1]`.
fn find_precedence_path(
    adjacency: &PrecedenceAdjacency<'_>,
    start: &str,
    target: &str,
) -> Option<(Vec<String>, Vec<String>)> {
    let mut visited: BTreeSet<&str> = BTreeSet::new();
    visited.insert(start);
    let mut parent_by_node: BTreeMap<&str, (&str, &str)> = BTreeMap::new();
    let mut queue: VecDeque<&str> = VecDeque::new();
    queue.push_back(start);

    while let Some(current) = queue.pop_front() {
        if current == target {
            let mut path_nodes: Vec<String> = Vec::new();
            let mut path_edges: Vec<String> = Vec::new();
            let mut cursor = current;
            path_nodes.push(cursor.to_owned());
            while let Some(&(previous, via_edge)) = parent_by_node.get(cursor) {
                path_nodes.push(previous.to_owned());
                path_edges.push(via_edge.to_owned());
                cursor = previous;
            }
            path_nodes.reverse();
            path_edges.reverse();
            return Some((path_nodes, path_edges));
        }
        if let Some(successors) = adjacency.get(current) {
            for &(next, via_edge) in successors {
                if visited.insert(next) {
                    parent_by_node.insert(next, (current, via_edge));
                    queue.push_back(next);
                }
            }
        }
    }
    None
}
