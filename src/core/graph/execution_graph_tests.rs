//! Deterministic coverage for the frozen `M-ORCH-1A` slice: precedence-DAG
//! core semantics, cycle detection and rejection, control-edge separation,
//! malformed-input failure, and determinism of identified rejections.

use std::borrow::Cow;

use crate::edge::{ControlKind, EdgeClass, GraphEdge, PrecedenceKind};
use crate::error::GraphError;
use crate::execution_graph::ExecutionGraph;
use crate::node::{CapabilityName, GraphNode, GraphNodeKind};

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Creates a plain node of an arbitrary kind with no required capabilities.
fn task(node_id: &str) -> GraphNode {
    GraphNode::new(node_id, GraphNodeKind::TASK, Vec::new()).expect("valid fixture node")
}

fn prec(edge_id: &str, from: &str, to: &str) -> GraphEdge {
    GraphEdge::precedence(edge_id, from, to, PrecedenceKind::RequiresAccepted)
        .expect("valid fixture precedence edge")
}

fn prec_kind(edge_id: &str, from: &str, to: &str, kind: PrecedenceKind) -> GraphEdge {
    GraphEdge::precedence(edge_id, from, to, kind).expect("valid fixture precedence edge")
}

fn ctrl(edge_id: &str, from: &str, to: &str, kind: ControlKind) -> GraphEdge {
    GraphEdge::control(edge_id, from, to, kind).expect("valid fixture control edge")
}

/// Builds a chain `ids[0] -> ids[1] -> ..` of `REQUIRES_ACCEPTED` precedence.
fn chain_graph(graph_id: &str, ids: &[&str]) -> ExecutionGraph {
    let mut graph = ExecutionGraph::new(graph_id).expect("valid graph id");
    for id in ids {
        graph.add_node(task(id)).expect("unique fixture node");
    }
    for pair in ids.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        graph
            .add_edge(prec(&format!("p_{from}_{to}"), from, to))
            .expect("acyclic fixture edge");
    }
    graph
}

fn cycle_error(
    candidate_edge_id: &str,
    candidate_from_node: &str,
    candidate_to_node: &str,
    closing_path_nodes: &[&str],
    closing_path_edge_ids: &[&str],
) -> GraphError {
    GraphError::PrecedenceCycleRejected {
        candidate_edge_id: candidate_edge_id.to_owned(),
        candidate_from_node: candidate_from_node.to_owned(),
        candidate_to_node: candidate_to_node.to_owned(),
        closing_path_nodes: closing_path_nodes
            .iter()
            .map(|id| (*id).to_owned())
            .collect(),
        closing_path_edge_ids: closing_path_edge_ids
            .iter()
            .map(|id| (*id).to_owned())
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Required functional coverage 1–5: acyclic topologies are accepted
// ---------------------------------------------------------------------------

/// Case 1: the empty graph is valid and acyclic.
#[test]
fn case_01_empty_graph_is_accepted() {
    let graph = ExecutionGraph::new("g-empty").expect("empty graph constructs");
    assert_eq!(graph.graph_id(), "g-empty");
    assert_eq!(graph.node_count(), 0);
    assert_eq!(graph.edge_count(), 0);
    assert!(!graph.has_precedence_cycle());
    assert_eq!(graph.nodes().count(), 0);
    assert_eq!(graph.precedence_edges().count(), 0);
    assert_eq!(graph.control_edges().count(), 0);
}

/// Case 2: a one-node graph is valid and acyclic.
#[test]
fn case_02_one_node_graph_is_accepted() {
    let mut graph = ExecutionGraph::new("g-one").expect("graph constructs");
    graph.add_node(task("solo")).expect("first node accepted");
    assert_eq!(graph.node_count(), 1);
    assert_eq!(graph.edge_count(), 0);
    assert!(!graph.has_precedence_cycle());
    assert!(graph.contains_node("solo"));
    assert_eq!(
        graph.node("solo").expect("solo present").kind(),
        &GraphNodeKind::TASK
    );
}

/// Case 3: a simple precedence chain is accepted.
#[test]
fn case_03_simple_precedence_chain_is_accepted() {
    let graph = chain_graph("g-chain", &["a", "b", "c"]);
    assert_eq!(graph.node_count(), 3);
    assert_eq!(graph.precedence_edges().count(), 2);
    assert!(!graph.has_precedence_cycle());
    assert!(graph.contains_edge("p_a_b"));
    assert!(graph.contains_edge("p_b_c"));
}

/// Case 4: a diamond-shaped precedence DAG is accepted, including a chord
/// between two existing parallel paths (redundant, still acyclic).
#[test]
fn case_04_diamond_dag_is_accepted() {
    let mut graph = ExecutionGraph::new("g-diamond").expect("graph constructs");
    for id in ["a", "b", "c", "d"] {
        graph.add_node(task(id)).expect("unique node");
    }
    for edge in [
        prec("p_a_b", "a", "b"),
        prec("p_a_c", "a", "c"),
        prec("p_b_d", "b", "d"),
        prec("p_c_d", "c", "d"),
    ] {
        graph.add_edge(edge).expect("diamond edge accepted");
    }
    // Chord joining two existing parallel paths: redundant, not cyclic.
    graph
        .add_edge(prec("p_a_d", "a", "d"))
        .expect("chord accepted");
    assert_eq!(graph.node_count(), 4);
    assert_eq!(graph.precedence_edges().count(), 5);
    assert!(!graph.has_precedence_cycle());
}

/// Case 5: disconnected acyclic components coexist in one graph.
#[test]
fn case_05_disconnected_acyclic_components_are_accepted() {
    let nodes: Vec<GraphNode> = ["w1-a", "w1-b", "w2-x", "w2-y", "w2-z", "island"]
        .iter()
        .map(|id| task(id))
        .collect();
    let edges = vec![
        prec("p_w1_a_b", "w1-a", "w1-b"),
        prec("p_w2_x_y", "w2-x", "w2-y"),
        prec("p_w2_y_z", "w2-y", "w2-z"),
    ];
    let batched = ExecutionGraph::from_parts("g-disconnected", nodes.clone(), edges.clone())
        .expect("disconnected components accepted");
    assert_eq!(batched.node_count(), 6);
    assert_eq!(batched.precedence_edges().count(), 3);
    assert!(!batched.has_precedence_cycle());

    // The same topology built incrementally compares equal.
    let mut incremental = ExecutionGraph::new("g-disconnected").expect("graph constructs");
    for node in nodes {
        incremental.add_node(node).expect("unique node");
    }
    for edge in edges {
        incremental.add_edge(edge).expect("component edge accepted");
    }
    assert_eq!(incremental, batched);
}

// ---------------------------------------------------------------------------
// Required functional coverage 6–9: precedence cycles are rejected
// ---------------------------------------------------------------------------

/// Case 6: a precedence self-cycle is rejected atomically.
#[test]
fn case_06_precedence_self_cycle_is_rejected() {
    let mut graph = chain_graph("g-self", &["b"]);
    graph.add_node(task("a")).expect("second node");
    graph.add_edge(prec("p_b_a", "b", "a")).expect("setup edge");

    let before = graph.clone();
    let candidate = prec("p_a_a", "a", "a");

    let rejection = graph
        .add_edge(candidate.clone())
        .expect_err("self-cycle rejected");
    assert_eq!(
        rejection,
        cycle_error("p_a_a", "a", "a", &[], &[]),
        "self-cycle names the candidate with an empty closing path"
    );
    // Atomic: nothing changed, nothing repaired, nothing deleted.
    assert_eq!(graph, before);
    assert!(!graph.contains_edge("p_a_a"));
    assert!(!graph.has_precedence_cycle());
    // The dry-run performs the identical check without mutation.
    assert_eq!(
        graph.validate_edge_addition(&candidate),
        Err(cycle_error("p_a_a", "a", "a", &[], &[])),
    );
    assert_eq!(graph, before);
}

/// Case 7: a two-node precedence cycle is rejected with the candidate edge
/// deterministically identified.
#[test]
fn case_07_two_node_precedence_cycle_is_rejected() {
    let mut graph = chain_graph("g-two", &["a", "b"]);
    let before = graph.clone();

    let rejection = graph
        .add_edge(prec("p_b_a", "b", "a"))
        .expect_err("two-node cycle rejected");
    assert_eq!(
        rejection,
        cycle_error("p_b_a", "b", "a", &["a", "b"], &["p_a_b"]),
    );
    assert_eq!(graph, before, "rejection leaves the graph unchanged");
    assert!(!graph.has_precedence_cycle());
}

/// Case 8: a longer precedence cycle (four nodes) is rejected with the full
/// concrete closing path.
#[test]
fn case_08_longer_precedence_cycle_is_rejected() {
    let mut graph = chain_graph("g-long", &["a", "b", "c", "d"]);
    let before = graph.clone();

    let rejection = graph
        .add_edge(prec("p_d_a", "d", "a"))
        .expect_err("long cycle rejected");
    assert_eq!(
        rejection,
        cycle_error(
            "p_d_a",
            "d",
            "a",
            &["a", "b", "c", "d"],
            &["p_a_b", "p_b_c", "p_c_d"],
        ),
    );
    assert_eq!(graph, before);
    assert!(!graph.has_precedence_cycle());
}

/// Case 9: a candidate precedence edge that closes an existing path is
/// rejected and never applied; the surviving graph stays usable.
#[test]
fn case_09_candidate_closing_existing_path_is_rejected() {
    let mut graph = ExecutionGraph::new("g-close").expect("graph constructs");
    for id in ["a", "b", "c", "d"] {
        graph.add_node(task(id)).expect("unique node");
    }
    for edge in [
        prec("p_a_b", "a", "b"),
        prec("p_a_c", "a", "c"),
        prec("p_b_d", "b", "d"),
        prec("p_c_d", "c", "d"),
    ] {
        graph.add_edge(edge).expect("diamond edge");
    }

    // d -> a closes both diamond paths; breadth-first identification reports
    // the lexicographically-first closing path.
    let rejection = graph
        .add_edge(prec("p_d_a", "d", "a"))
        .expect_err("closing edge rejected");
    assert_eq!(
        rejection,
        cycle_error("p_d_a", "d", "a", &["a", "b", "d"], &["p_a_b", "p_b_d"]),
    );

    // Atomic rejection: the graph is exactly what it was, remains acyclic,
    // and still accepts unrelated structural changes afterwards.
    assert_eq!(graph.precedence_edges().count(), 4);
    assert!(!graph.contains_edge("p_d_a"));
    assert!(!graph.has_precedence_cycle());
    graph.add_node(task("e")).expect("graph still mutable");
    graph
        .add_edge(prec("p_d_e", "d", "e"))
        .expect("extension still accepted");
    assert!(!graph.has_precedence_cycle());
}

// ---------------------------------------------------------------------------
// Required functional coverage 10–11: control-edge separation
// ---------------------------------------------------------------------------

/// Case 10: control edges forming conceptual loops never cause false
/// precedence-cycle rejection, including the frozen repair-expansion shape
/// (`Review-1 --CONTROL ON_REJECT--> Repair-2`) and pure control loops.
#[test]
fn case_10_control_loops_are_stored_without_false_rejection() {
    let mut graph = ExecutionGraph::new("g-control-loop").expect("graph constructs");
    for id in [
        "n_impl_1",
        "n_review_1",
        "n_repair_2",
        "n_review_2",
        "r1",
        "r2",
        "r3",
    ] {
        graph.add_node(task(id)).expect("unique node");
    }

    let setup = [
        prec("p_impl_review", "n_impl_1", "n_review_1"),
        ctrl(
            "c_review_repair",
            "n_review_1",
            "n_repair_2",
            ControlKind::OnReject,
        ),
        prec("p_repair_review2", "n_repair_2", "n_review_2"),
        ctrl(
            "c_repair_back",
            "n_repair_2",
            "n_review_1",
            ControlKind::OnFailure,
        ),
        ctrl(
            "c_expand_impl",
            "n_review_2",
            "n_impl_1",
            ControlKind::ExpandsInto,
        ),
        // Pure control loops, including a control self-loop: representable
        // because control edges are outside precedence traversal entirely.
        ctrl("c_x_y", "r1", "r2", ControlKind::OnPass),
        ctrl("c_y_z", "r2", "r3", ControlKind::OnBlocked),
        ctrl("c_z_x", "r3", "r1", ControlKind::Escalate),
        ctrl("c_self", "r1", "r1", ControlKind::OnPass),
    ];
    for edge in setup {
        graph.add_edge(edge).expect("control topology accepted");
    }

    assert!(!graph.has_precedence_cycle());
    assert_eq!(graph.precedence_edges().count(), 2);
    assert_eq!(graph.control_edges().count(), 7);
}

/// Case 11: in mixed topologies only PRECEDENCE relationships are examined;
/// control hops neither close cycles nor mask real ones.
#[test]
fn case_11_mixed_topology_examines_only_precedence() {
    let mut graph = ExecutionGraph::new("g-mixed").expect("graph constructs");
    for id in ["a", "b", "c", "d"] {
        graph.add_node(task(id)).expect("unique node");
    }
    graph
        .add_edge(prec("p_a_b", "a", "b"))
        .expect("precedence accepted");
    graph
        .add_edge(ctrl("c_b_c", "b", "c", ControlKind::OnPass))
        .expect("control shortcut accepted");

    // The only route from a to c passes the control hop b -> c; therefore the
    // candidate c -> a must be accepted even though a full-edge traversal
    // would see a closed loop.
    graph
        .add_edge(prec("p_c_a", "c", "a"))
        .expect("control hops do not close precedence cycles");

    // A conceptual loop through a control back-edge stays acyclic.
    graph
        .add_edge(ctrl("c_d_a", "d", "a", ControlKind::OnReject))
        .expect("control back-edge accepted");
    graph
        .add_edge(prec("p_c_d", "c", "d"))
        .expect("forward precedence accepted");
    assert!(!graph.has_precedence_cycle());

    // A genuine precedence cycle is still detected in the same mixed graph.
    let rejection = graph
        .add_edge(prec("p_b_a", "b", "a"))
        .expect_err("real precedence cycle still rejected");
    assert_eq!(
        rejection,
        cycle_error("p_b_a", "b", "a", &["a", "b"], &["p_a_b"]),
    );
    assert!(!graph.contains_edge("p_b_a"));
}

// ---------------------------------------------------------------------------
// Required functional coverage 12–13: determinism
// ---------------------------------------------------------------------------

/// Case 12: repeating the same validation produces the identical result every
/// time, and rebuilding equivalent graphs yields equal graphs.
#[test]
fn case_12_repeated_validation_is_identical() {
    let build = || {
        let mut graph = ExecutionGraph::new("g-repeat").expect("graph constructs");
        for id in ["a", "b", "c"] {
            graph.add_node(task(id)).expect("unique node");
        }
        for edge in [prec("p_a_b", "a", "b"), prec("p_b_c", "b", "c")] {
            graph.add_edge(edge).expect("chain edge");
        }
        graph
    };

    let graph = build();

    // Accepted candidate (acyclic chord): identical Ok across repetitions,
    // dry-run only so later expectations stay valid.
    let ok_candidate = prec("p_a_c", "a", "c");
    for _ in 0..5 {
        assert_eq!(graph.validate_edge_addition(&ok_candidate), Ok(()));
    }

    // Rejected candidate: identical Err payload across repetitions.
    let closing = prec("p_c_a", "c", "a");
    let expected = cycle_error("p_c_a", "c", "a", &["a", "b", "c"], &["p_a_b", "p_b_c"]);
    for _ in 0..5 {
        assert_eq!(
            graph.validate_edge_addition(&closing),
            Err(expected.clone())
        );
    }
    for _ in 0..5 {
        assert!(!graph.has_precedence_cycle());
    }

    // Equivalent input rebuilt independently compares equal.
    assert_eq!(build(), build());
}

/// Case 13: identification of the rejected closing edge is a pure function of
/// graph content — independent of insertion order and stable across repeats.
#[test]
fn case_13_closing_edge_identification_is_insertion_order_independent() {
    let nodes: Vec<GraphNode> = ["a", "b", "c", "d"].iter().map(|id| task(id)).collect();
    let edges_forward = vec![
        prec("p_a_b", "a", "b"),
        prec("p_b_c", "b", "c"),
        prec("p_c_d", "c", "d"),
    ];
    let edges_reverse: Vec<GraphEdge> = edges_forward.iter().rev().cloned().collect();

    let forward_graph = ExecutionGraph::from_parts("g-det", nodes.clone(), edges_forward)
        .expect("acyclic forward build");
    let reverse_graph = ExecutionGraph::from_parts("g-det", nodes, edges_reverse)
        .expect("acyclic reversed-order build");
    assert_eq!(
        forward_graph, reverse_graph,
        "equivalent input yields identical graph state"
    );

    let candidate = prec("p_d_a", "d", "a");
    let expected = cycle_error(
        "p_d_a",
        "d",
        "a",
        &["a", "b", "c", "d"],
        &["p_a_b", "p_b_c", "p_c_d"],
    );

    for graph in [&forward_graph, &reverse_graph] {
        for round in 0..3 {
            let outcome = graph.validate_edge_addition(&candidate);
            assert_eq!(outcome, Err(expected.clone()), "round {round}");
        }
        // Incremental rejection agrees byte-for-byte with batch analysis and
        // applies nothing.
        let mut live = graph.clone();
        assert_eq!(live.add_edge(candidate.clone()), Err(expected.clone()));
        assert_eq!(live, *graph, "rejected change applied nowhere");
    }

    // Whole-graph scan (used by batch construction) walks precedence edges in
    // ascending edge-id order and identifies the same cycle through the first
    // closing edge it reaches.
    let closed = ExecutionGraph::from_parts(
        "g-det-closed",
        vec![task("a"), task("b"), task("c"), task("d")],
        vec![
            prec("p_d_a", "d", "a"),
            prec("p_c_d", "c", "d"),
            prec("p_a_b", "a", "b"),
            prec("p_b_c", "b", "c"),
        ],
    )
    .expect_err("cycle present");
    assert_eq!(
        closed,
        cycle_error(
            "p_a_b",
            "a",
            "b",
            &["b", "c", "d", "a"],
            &["p_b_c", "p_c_d", "p_d_a"],
        ),
    );
}

// ---------------------------------------------------------------------------
// Malformed input fails explicitly
// ---------------------------------------------------------------------------

#[test]
fn malformed_graph_identifiers_fail_explicitly() {
    assert_eq!(
        ExecutionGraph::new(""),
        Err(GraphError::EmptyIdentifier { field: "graph_id" }),
    );
    let oversized: String = "n".repeat(201);
    assert_eq!(
        ExecutionGraph::new(oversized),
        Err(GraphError::IdentifierTooLong {
            field: "graph_id",
            length: 201,
            max: crate::error::MAX_IDENTIFIER_LENGTH,
        }),
    );
    assert_eq!(
        ExecutionGraph::new("ok").map(|graph| graph.graph_id().to_owned()),
        Ok("ok".to_owned()),
    );
}

#[test]
fn malformed_nodes_fail_explicitly() {
    assert_eq!(
        GraphNode::new("", GraphNodeKind::TASK, Vec::new()),
        Err(GraphError::EmptyIdentifier { field: "node_id" }),
    );
    let oversized: String = "n".repeat(201);
    assert!(matches!(
        GraphNode::new(oversized, GraphNodeKind::TASK, Vec::new()),
        Err(GraphError::IdentifierTooLong {
            field: "node_id",
            ..
        })
    ));

    // Extensible kind: empty is malformed, arbitrary fresh kinds are valid.
    assert_eq!(
        GraphNodeKind::new(""),
        Err(GraphError::EmptyIdentifier { field: "kind" }),
    );
    let fresh = GraphNodeKind::new(Cow::Owned("BRAND_NEW_KIND_42".to_owned())).expect("open kind");
    let node = GraphNode::new("n1", fresh, Vec::new()).expect("valid node");
    assert_eq!(node.kind().as_str(), "BRAND_NEW_KIND_42");

    assert_eq!(
        CapabilityName::new(""),
        Err(GraphError::EmptyIdentifier {
            field: "required_capabilities entry"
        }),
    );
}

#[test]
fn malformed_edges_fail_explicitly() {
    assert_eq!(
        GraphEdge::precedence("", "a", "b", PrecedenceKind::RequiresAccepted),
        Err(GraphError::EmptyIdentifier { field: "edge_id" }),
    );
    assert_eq!(
        GraphEdge::control("e", "", "b", ControlKind::OnPass),
        Err(GraphError::EmptyIdentifier { field: "from_node" }),
    );
    assert_eq!(
        GraphEdge::control("e", "a", "", ControlKind::OnReject),
        Err(GraphError::EmptyIdentifier { field: "to_node" }),
    );
    let oversized: String = "n".repeat(201);
    assert!(matches!(
        GraphEdge::precedence("e", oversized, "b", PrecedenceKind::RequiresInterface),
        Err(GraphError::IdentifierTooLong {
            field: "from_node",
            ..
        })
    ));
}

#[test]
fn duplicate_node_id_fails_explicitly() {
    let mut graph = chain_graph("g-dupe-node", &["a", "b"]);
    let before = graph.clone();
    assert_eq!(
        graph.add_node(task("a")),
        Err(GraphError::DuplicateNodeId {
            node_id: "a".to_owned()
        }),
    );
    assert_eq!(graph, before, "failed addition mutated nothing");
}

#[test]
fn duplicate_edge_id_fails_explicitly_even_across_classes() {
    let mut graph = chain_graph("g-dupe-edge", &["a", "b"]);
    let before = graph.clone();
    assert_eq!(
        graph.add_edge(prec("p_a_b", "b", "a")),
        Err(GraphError::DuplicateEdgeId {
            edge_id: "p_a_b".to_owned()
        }),
    );
    assert_eq!(
        graph.add_edge(ctrl("p_a_b", "a", "b", ControlKind::OnPass)),
        Err(GraphError::DuplicateEdgeId {
            edge_id: "p_a_b".to_owned()
        }),
        "edge ids are unique across both classes"
    );
    assert_eq!(graph, before);

    // Distinct-id parallel precedence edges remain structurally legal.
    graph
        .add_edge(prec("p_a_b_alias", "a", "b"))
        .expect("distinct-id parallel precedence edge accepted");
    assert!(!graph.has_precedence_cycle());
}

#[test]
fn unknown_endpoint_reference_fails_explicitly() {
    let mut graph = chain_graph("g-dangling", &["a"]);
    let dangling = prec("p_a_ghost", "a", "ghost");
    assert_eq!(
        graph.validate_edge_addition(&dangling),
        Err(GraphError::UnknownNodeReference {
            edge_id: "p_a_ghost".to_owned(),
            node_id: "ghost".to_owned(),
        }),
        "dry-run rejects dangling references",
    );
    assert_eq!(
        graph.add_edge(dangling),
        Err(GraphError::UnknownNodeReference {
            edge_id: "p_a_ghost".to_owned(),
            node_id: "ghost".to_owned(),
        }),
        "mutating call rejects dangling references identically",
    );
    assert_eq!(
        graph.add_edge(prec("p_ghost_a", "ghost", "a")),
        Err(GraphError::UnknownNodeReference {
            edge_id: "p_ghost_a".to_owned(),
            node_id: "ghost".to_owned(),
        }),
    );
    assert_eq!(graph.edge_count(), 0);
}

#[test]
fn batch_construction_rejects_every_violation_class_explicitly() {
    // Precedence cycle anywhere rejects the entire construction; the scan in
    // ascending edge-id order identifies the first closing edge.
    assert_eq!(
        ExecutionGraph::from_parts(
            "g-batch-cycle",
            vec![task("a"), task("b"), task("c")],
            vec![
                prec("p_a_b", "a", "b"),
                prec("p_b_c", "b", "c"),
                prec("p_c_a", "c", "a"),
            ],
        ),
        Err(cycle_error(
            "p_a_b",
            "a",
            "b",
            &["b", "c", "a"],
            &["p_b_c", "p_c_a"],
        )),
    );

    // Self-cycle inside a batch is rejected.
    assert_eq!(
        ExecutionGraph::from_parts(
            "g-batch-self",
            vec![task("a")],
            vec![prec("p_a_a", "a", "a")]
        ),
        Err(cycle_error("p_a_a", "a", "a", &["a"], &[])),
    );

    // Dangling reference rejects the entire construction.
    assert_eq!(
        ExecutionGraph::from_parts(
            "g-batch-dangling",
            vec![task("a")],
            vec![prec("p_a_z", "a", "z")]
        ),
        Err(GraphError::UnknownNodeReference {
            edge_id: "p_a_z".to_owned(),
            node_id: "z".to_owned(),
        }),
    );

    // Duplicate ids reject the entire construction.
    assert_eq!(
        ExecutionGraph::from_parts("g-batch-dupe", vec![task("a"), task("a")], vec![]),
        Err(GraphError::DuplicateNodeId {
            node_id: "a".to_owned()
        }),
    );
    assert_eq!(
        ExecutionGraph::from_parts(
            "g-batch-dupe-edge",
            vec![task("a"), task("b")],
            vec![prec("e1", "a", "b"), prec("e1", "b", "a")],
        ),
        Err(GraphError::DuplicateEdgeId {
            edge_id: "e1".to_owned()
        }),
    );
}

// ---------------------------------------------------------------------------
// Frozen contract surface: class exclusivity, extensibility, data-only caps
// ---------------------------------------------------------------------------

#[test]
fn edge_class_exclusivity_is_structural() {
    let precedence = prec("e-p", "a", "b");
    assert_eq!(precedence.class(), EdgeClass::Precedence);
    assert_eq!(
        precedence.precedence_kind(),
        Some(PrecedenceKind::RequiresAccepted)
    );
    assert_eq!(precedence.control_kind(), None);

    let integrated = prec_kind("e-q", "b", "c", PrecedenceKind::RequiresIntegrated);
    assert_eq!(integrated.relation().kind_as_str(), "REQUIRES_INTEGRATED");
    let interface = prec_kind("e-r", "c", "d", PrecedenceKind::RequiresInterface);
    assert_eq!(interface.relation().kind_as_str(), "REQUIRES_INTERFACE");

    let control = ctrl("e-c", "a", "b", ControlKind::Escalate);
    assert_eq!(control.class(), EdgeClass::Control);
    assert_eq!(control.control_kind(), Some(ControlKind::Escalate));
    assert_eq!(control.precedence_kind(), None);

    // All six frozen control kinds carry their exact frozen representation
    // and never expose a precedence kind.
    let control_kinds = [
        (ControlKind::OnPass, "ON_PASS"),
        (ControlKind::OnReject, "ON_REJECT"),
        (ControlKind::OnFailure, "ON_FAILURE"),
        (ControlKind::OnBlocked, "ON_BLOCKED"),
        (ControlKind::Escalate, "ESCALATE"),
        (ControlKind::ExpandsInto, "EXPANDS_INTO"),
    ];
    for (kind, representation) in control_kinds {
        let edge = GraphEdge::control("id", "a", "b", kind).expect("valid control edge");
        assert_eq!(edge.relation().kind_as_str(), representation);
        assert_eq!(edge.class().as_str(), "CONTROL");
        assert_eq!(edge.precedence_kind(), None);
    }
    assert_eq!(prec("id", "a", "b").class().as_str(), "PRECEDENCE");
}

#[test]
fn required_capabilities_remain_data_only() {
    let capabilities = vec![
        CapabilityName::new("graph.core").expect("valid capability"),
        CapabilityName::new("review.independent_a4").expect("valid capability"),
        CapabilityName::new("graph.core").expect("verbatim duplicates allowed"),
    ];
    let node = GraphNode::new("cap-node", GraphNodeKind::IMPLEMENTATION, capabilities)
        .expect("valid node");

    // Stored verbatim, order preserved, returned unchanged: nothing here
    // interprets, filters, admits, routes, or tiers on capability data.
    assert_eq!(node.required_capabilities().len(), 3);
    assert_eq!(node.required_capabilities()[0].as_str(), "graph.core");
    assert_eq!(
        node.required_capabilities()[1].as_str(),
        "review.independent_a4"
    );
    assert_eq!(node.required_capabilities()[2].as_str(), "graph.core");
    assert_eq!(task("plain").required_capabilities().len(), 0);
}

#[test]
fn well_known_node_kinds_are_extensible_strings_not_an_enum() {
    // Every well-known kind round-trips as its frozen string.
    let well_known = [
        (GraphNodeKind::GOAL, "GOAL"),
        (GraphNodeKind::WORKSTREAM, "WORKSTREAM"),
        (GraphNodeKind::TASK, "TASK"),
        (GraphNodeKind::ATTEMPT, "ATTEMPT"),
        (GraphNodeKind::IMPLEMENTATION, "IMPLEMENTATION"),
        (GraphNodeKind::REVIEW, "REVIEW"),
        (GraphNodeKind::REPAIR, "REPAIR"),
        (GraphNodeKind::DETERMINISTIC_CHECK, "DETERMINISTIC_CHECK"),
        (GraphNodeKind::ROUTING, "ROUTING"),
        (GraphNodeKind::INTEGRATION, "INTEGRATION"),
        (GraphNodeKind::HUMAN_GATE, "HUMAN_GATE"),
        (GraphNodeKind::GOAL_EVALUATION, "GOAL_EVALUATION"),
    ];
    for (kind, representation) in well_known {
        assert_eq!(kind.as_str(), representation);
    }

    // An unseen kind requires no code or schema change to participate fully
    // in graph topology and cycle validation.
    let mut graph = ExecutionGraph::new("g-open-kind").expect("graph constructs");
    let exotic = GraphNode::new(
        "future-node",
        GraphNodeKind::new(Cow::Owned("KIND_NOT_YET_INVENTED".to_owned())).expect("open kind"),
        Vec::new(),
    )
    .expect("valid node");
    graph.add_node(exotic).expect("open-kind node accepted");
    graph.add_node(task("t")).expect("task node accepted");
    graph
        .add_edge(prec("p_future_t", "future-node", "t"))
        .expect("open-kind node participates in topology");
    assert!(!graph.has_precedence_cycle());
}
