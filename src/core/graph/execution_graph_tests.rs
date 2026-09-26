//! Deterministic coverage for the frozen `M-ORCH-1A` slice: precedence-DAG
//! core semantics, cycle detection and rejection, control-edge separation,
//! malformed-input failure, and determinism of identified rejections.

use std::borrow::Cow;

use crate::edge::{ControlKind, EdgeClass, GraphEdge, PrecedenceKind};
use crate::error::GraphError;
use crate::execution_graph::ExecutionGraph;
use crate::node::{CapabilityName, GraphNode, GraphNodeAttemptNumber, GraphNodeKind};
use crate::node_state::GraphNodeState;
use crate::version::GraphVersionV1;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Graph identity shared by every fixture node, edge, and containing graph.
const G: &str = "g";

/// Creates a plain node of an arbitrary kind with no required capabilities.
fn task(node_id: &str) -> GraphNode {
    GraphNode::new(node_id, G, GraphNodeKind::TASK, GraphNodeState::Planned)
        .expect("valid fixture node")
}

fn prec(edge_id: &str, from: &str, to: &str) -> GraphEdge {
    GraphEdge::precedence(edge_id, G, from, to, PrecedenceKind::RequiresAccepted)
        .expect("valid fixture precedence edge")
}

fn prec_kind(edge_id: &str, from: &str, to: &str, kind: PrecedenceKind) -> GraphEdge {
    GraphEdge::precedence(edge_id, G, from, to, kind).expect("valid fixture precedence edge")
}

fn ctrl(edge_id: &str, from: &str, to: &str, kind: ControlKind) -> GraphEdge {
    GraphEdge::control(edge_id, G, from, to, kind).expect("valid fixture control edge")
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

#[test]
fn all_node_states_are_stored_and_read_exactly() {
    let mut incremental = ExecutionGraph::new(G).unwrap();
    let mut nodes = Vec::new();
    for state in GraphNodeState::ALL {
        let node = GraphNode::new(state.as_str(), G, GraphNodeKind::TASK, state).unwrap();
        assert_eq!(node.state(), state);
        let cloned = node.clone();
        assert_eq!(cloned, node);
        assert_eq!(cloned.state(), state);
        incremental.add_node(cloned).unwrap();
        assert_eq!(incremental.node_state(node.node_id()), Some(state));
        nodes.push(node);
    }
    let batched = ExecutionGraph::from_parts(G, nodes.clone(), vec![]).unwrap();
    assert_eq!(batched, incremental);
    let cloned = batched.clone();
    assert_eq!(cloned, batched);
    for graph in [&incremental, &batched, &cloned] {
        for node in &nodes {
            assert_eq!(graph.node(node.node_id()), Some(node));
            assert_eq!(graph.node_state(node.node_id()), Some(node.state()));
        }
        for _ in 0..3 {
            assert_eq!(graph.node_state("unknown"), None);
            assert_eq!(graph.node_state(""), None);
        }
    }
    let planned = GraphNode::new("same", G, GraphNodeKind::TASK, GraphNodeState::Planned).unwrap();
    let ready = GraphNode::new("same", G, GraphNodeKind::TASK, GraphNodeState::Ready).unwrap();
    assert_ne!(planned, ready, "node equality includes stored state");
    assert_ne!(
        ExecutionGraph::from_parts(G, vec![planned], vec![]).unwrap(),
        ExecutionGraph::from_parts(G, vec![ready], vec![]).unwrap(),
        "graph equality includes stored state"
    );
}

#[test]
fn structural_operations_preserve_live_node_states() {
    let nodes: Vec<_> = [
        ("a", GraphNodeState::Ready),
        ("b", GraphNodeState::Accepted),
        ("c", GraphNodeState::Integrated),
    ]
    .into_iter()
    .map(|(id, state)| GraphNode::new(id, G, GraphNodeKind::TASK, state).unwrap())
    .collect();
    let edges = vec![
        prec("p_a_b", "a", "b"),
        ctrl("c_b_a", "b", "a", ControlKind::OnPass),
    ];
    let mut graph = ExecutionGraph::from_parts(G, nodes.clone(), edges.clone()).unwrap();
    let assert_states = |graph: &ExecutionGraph| {
        for node in &nodes {
            assert_eq!(graph.node(node.node_id()), Some(node));
            assert_eq!(graph.node_state(node.node_id()), Some(node.state()));
        }
        assert_eq!(graph.nodes().cloned().collect::<Vec<_>>(), nodes);
    };
    assert_states(&graph);
    for edge in [
        prec("p_b_c", "b", "c"),
        ctrl("c_c_a", "c", "a", ControlKind::OnReject),
    ] {
        let before = graph.clone();
        assert_eq!(graph.validate_edge_addition(&edge), Ok(()));
        assert_eq!(graph, before);
        graph.add_edge(edge).unwrap();
        assert_states(&graph);
    }
    let before = graph.clone();
    for (edge, error) in [
        (
            prec("p_c_a", "c", "a"),
            cycle_error("p_c_a", "c", "a", &["a", "b", "c"], &["p_a_b", "p_b_c"]),
        ),
        (
            edges[0].clone(),
            GraphError::DuplicateEdgeId {
                edge_id: "p_a_b".into(),
            },
        ),
        (
            prec("missing", "a", "unknown"),
            GraphError::UnknownNodeReference {
                edge_id: "missing".into(),
                node_id: "unknown".into(),
            },
        ),
    ] {
        assert_eq!(graph.validate_edge_addition(&edge), Err(error.clone()));
        assert_eq!(graph.add_edge(edge), Err(error));
        assert_eq!(graph, before);
        assert_states(&graph);
    }
    let replacement =
        GraphNode::new("a", G, GraphNodeKind::TASK, GraphNodeState::Cancelled).unwrap();
    assert_eq!(
        graph.add_node(replacement),
        Err(GraphError::DuplicateNodeId {
            node_id: "a".into()
        })
    );
    assert_eq!(graph, before);
    assert_states(&graph);
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
    let mut graph = ExecutionGraph::new(G).expect("graph constructs");
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
    let graph = chain_graph(G, &["a", "b", "c"]);
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
    let mut graph = ExecutionGraph::new(G).expect("graph constructs");
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
    let batched = ExecutionGraph::from_parts(G, nodes.clone(), edges.clone())
        .expect("disconnected components accepted");
    assert_eq!(batched.node_count(), 6);
    assert_eq!(batched.precedence_edges().count(), 3);
    assert!(!batched.has_precedence_cycle());

    // The same topology built incrementally compares equal.
    let mut incremental = ExecutionGraph::new(G).expect("graph constructs");
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
    let mut graph = chain_graph(G, &["b"]);
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
    let mut graph = chain_graph(G, &["a", "b"]);
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
    let mut graph = chain_graph(G, &["a", "b", "c", "d"]);
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
    let mut graph = ExecutionGraph::new(G).expect("graph constructs");
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
    let mut graph = ExecutionGraph::new(G).expect("graph constructs");
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
    let mut graph = ExecutionGraph::new(G).expect("graph constructs");
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
        let mut graph = ExecutionGraph::new(G).expect("graph constructs");
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

    let forward_graph =
        ExecutionGraph::from_parts(G, nodes.clone(), edges_forward).expect("acyclic forward build");
    let reverse_graph =
        ExecutionGraph::from_parts(G, nodes, edges_reverse).expect("acyclic reversed-order build");
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
        G,
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
        GraphNode::new("", G, GraphNodeKind::TASK, GraphNodeState::Planned),
        Err(GraphError::EmptyIdentifier { field: "node_id" }),
    );
    let oversized: String = "n".repeat(201);
    assert!(matches!(
        GraphNode::new(oversized, G, GraphNodeKind::TASK, GraphNodeState::Planned),
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
    let node = GraphNode::new("n1", G, fresh, GraphNodeState::Planned).expect("valid node");
    assert_eq!(node.kind().as_str(), "BRAND_NEW_KIND_42");

    assert_eq!(
        CapabilityName::new(""),
        Err(GraphError::InvalidCapabilitySyntax {
            value: String::new()
        }),
    );
}

#[test]
fn malformed_edges_fail_explicitly() {
    assert_eq!(
        GraphEdge::precedence("", G, "a", "b", PrecedenceKind::RequiresAccepted),
        Err(GraphError::EmptyIdentifier { field: "edge_id" }),
    );
    assert_eq!(
        GraphEdge::control("e", G, "", "b", ControlKind::OnPass),
        Err(GraphError::EmptyIdentifier { field: "from_node" }),
    );
    assert_eq!(
        GraphEdge::control("e", G, "a", "", ControlKind::OnReject),
        Err(GraphError::EmptyIdentifier { field: "to_node" }),
    );
    let oversized: String = "n".repeat(201);
    assert!(matches!(
        GraphEdge::precedence("e", G, oversized, "b", PrecedenceKind::RequiresInterface),
        Err(GraphError::IdentifierTooLong {
            field: "from_node",
            ..
        })
    ));
}

#[test]
fn duplicate_node_id_fails_explicitly() {
    let mut graph = chain_graph(G, &["a", "b"]);
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
    let mut graph = chain_graph(G, &["a", "b"]);
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
    let mut graph = chain_graph(G, &["a"]);
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
            G,
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
        ExecutionGraph::from_parts(G, vec![task("a")], vec![prec("p_a_a", "a", "a")]),
        Err(cycle_error("p_a_a", "a", "a", &["a"], &[])),
    );

    // Dangling reference rejects the entire construction.
    assert_eq!(
        ExecutionGraph::from_parts(G, vec![task("a")], vec![prec("p_a_z", "a", "z")]),
        Err(GraphError::UnknownNodeReference {
            edge_id: "p_a_z".to_owned(),
            node_id: "z".to_owned(),
        }),
    );

    // Duplicate ids reject the entire construction.
    assert_eq!(
        ExecutionGraph::from_parts(G, vec![task("a"), task("a")], vec![]),
        Err(GraphError::DuplicateNodeId {
            node_id: "a".to_owned()
        }),
    );
    assert_eq!(
        ExecutionGraph::from_parts(
            G,
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
        let edge = GraphEdge::control("id", G, "a", "b", kind).expect("valid control edge");
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
    let node = GraphNode::new(
        "cap-node",
        G,
        GraphNodeKind::IMPLEMENTATION,
        GraphNodeState::Planned,
    )
    .expect("valid node")
    .with_required_capabilities(capabilities);

    // Stored verbatim, order preserved, returned unchanged: nothing here
    // interprets, filters, admits, routes, or tiers on capability data.
    let stored = node.required_capabilities().expect("present capabilities");
    assert_eq!(stored.len(), 3);
    assert_eq!(stored[0].as_str(), "graph.core");
    assert_eq!(stored[1].as_str(), "review.independent_a4");
    assert_eq!(stored[2].as_str(), "graph.core");
    assert_eq!(task("plain").required_capabilities(), None);
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
    let mut graph = ExecutionGraph::new(G).expect("graph constructs");
    let exotic = GraphNode::new(
        "future-node",
        G,
        GraphNodeKind::new(Cow::Owned("KIND_NOT_YET_INVENTED".to_owned())).expect("open kind"),
        GraphNodeState::Planned,
    )
    .expect("valid node");
    graph.add_node(exotic).expect("open-kind node accepted");
    graph.add_node(task("t")).expect("task node accepted");
    graph
        .add_edge(prec("p_future_t", "future-node", "t"))
        .expect("open-kind node participates in topology");
    assert!(!graph.has_precedence_cycle());
}

// ---------------------------------------------------------------------------
// Full-fidelity node and edge records and child graph identity
// ---------------------------------------------------------------------------

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const ABOVE_U64: &str = "18446744073709551616";

fn attempt(value: &str) -> GraphNodeAttemptNumber {
    GraphNodeAttemptNumber::try_new(value).expect("valid attempt number")
}

fn version(value: &str) -> GraphVersionV1 {
    GraphVersionV1::try_new(value).expect("valid graph version")
}

fn full_node() -> GraphNode {
    task("full")
        .with_title("Tïtle ✓")
        .with_parent_node_id("parent")
        .unwrap()
        .with_attempt_number(attempt(ABOVE_U64))
        .with_required_capabilities(vec![
            CapabilityName::new("graph.core").unwrap(),
            CapabilityName::new("review.independent_a4").unwrap(),
            CapabilityName::new("graph.core").unwrap(),
        ])
        .with_task_capsule_ref("capsule://ref")
        .with_workstream_id("ws-1")
        .unwrap()
        .with_code_sha(SHA)
        .unwrap()
        .with_workspace_id("wsp-1")
        .unwrap()
        .with_result_ref("")
        .with_locked_reason("requires pro")
        .with_created_in_version(version("7"))
}

#[test]
fn node_preserves_every_field_through_getters() {
    let node = full_node();
    assert_eq!(node.node_id(), "full");
    assert_eq!(node.graph_id(), G);
    assert_eq!(node.kind(), &GraphNodeKind::TASK);
    assert_eq!(node.state(), GraphNodeState::Planned);
    assert_eq!(node.title(), Some("Tïtle ✓"));
    assert_eq!(node.parent_node_id(), Some("parent"));
    assert_eq!(node.attempt_number().map(|a| a.as_str()), Some(ABOVE_U64));
    let capabilities: Vec<_> = node
        .required_capabilities()
        .unwrap()
        .iter()
        .map(CapabilityName::as_str)
        .collect();
    assert_eq!(
        capabilities,
        ["graph.core", "review.independent_a4", "graph.core"]
    );
    assert_eq!(node.task_capsule_ref(), Some("capsule://ref"));
    assert_eq!(node.workstream_id(), Some("ws-1"));
    assert_eq!(node.code_sha(), Some(SHA));
    assert_eq!(node.workspace_id(), Some("wsp-1"));
    assert_eq!(node.result_ref(), Some(""));
    assert_eq!(node.locked_reason(), Some(Some("requires pro")));
    assert_eq!(node.created_in_version(), Some(&version("7")));

    // A graph holding the node returns it unchanged.
    let graph = ExecutionGraph::from_parts(G, vec![node.clone()], vec![]).unwrap();
    assert_eq!(graph.node("full"), Some(&node));
}

#[test]
fn node_optional_fields_start_absent_and_each_presence_is_distinct() {
    let bare = task("n");
    assert_eq!(bare.title(), None);
    assert_eq!(bare.parent_node_id(), None);
    assert_eq!(bare.attempt_number(), None);
    assert_eq!(bare.required_capabilities(), None);
    assert_eq!(bare.task_capsule_ref(), None);
    assert_eq!(bare.workstream_id(), None);
    assert_eq!(bare.code_sha(), None);
    assert_eq!(bare.workspace_id(), None);
    assert_eq!(bare.result_ref(), None);
    assert_eq!(bare.locked_reason(), None);
    assert_eq!(bare.created_in_version(), None);

    let variants = [
        bare.clone().with_title(""),
        bare.clone().with_parent_node_id("p").unwrap(),
        bare.clone().with_attempt_number(attempt("1")),
        bare.clone().with_required_capabilities(vec![]),
        bare.clone().with_task_capsule_ref(""),
        bare.clone().with_workstream_id("w").unwrap(),
        bare.clone().with_code_sha(SHA).unwrap(),
        bare.clone().with_workspace_id("w").unwrap(),
        bare.clone().with_result_ref(""),
        bare.clone().with_locked_reason(""),
        bare.clone().with_null_locked_reason(),
        bare.clone().with_created_in_version(version("1")),
    ];
    for (i, left) in variants.iter().enumerate() {
        assert_ne!(left, &bare, "variant {i} equals the absent node");
        for (j, right) in variants.iter().enumerate() {
            assert_eq!(left == right, i == j, "variants {i} and {j}");
        }
    }
}

#[test]
fn locked_reason_distinguishes_absent_null_empty_and_string() {
    let absent = task("n");
    let null = task("n").with_null_locked_reason();
    let empty = task("n").with_locked_reason("");
    let text = task("n").with_locked_reason("requires pro");
    assert_eq!(absent.locked_reason(), None);
    assert_eq!(null.locked_reason(), Some(None));
    assert_eq!(empty.locked_reason(), Some(Some("")));
    assert_eq!(text.locked_reason(), Some(Some("requires pro")));
    let all = [&absent, &null, &empty, &text];
    for (i, left) in all.iter().enumerate() {
        for (j, right) in all.iter().enumerate() {
            assert_eq!(left == right, i == j);
        }
    }
}

#[test]
fn required_capabilities_distinguish_absent_empty_and_ordered_duplicates() {
    let absent = task("n");
    let empty = task("n").with_required_capabilities(vec![]);
    let names = ["b.x", "a.y", "b.x"];
    let ordered = task("n").with_required_capabilities(
        names
            .iter()
            .map(|name| CapabilityName::new(*name).unwrap())
            .collect(),
    );
    let reordered = task("n").with_required_capabilities(
        ["a.y", "b.x", "b.x"]
            .iter()
            .map(|name| CapabilityName::new(*name).unwrap())
            .collect(),
    );
    assert_eq!(absent.required_capabilities(), None);
    assert_eq!(empty.required_capabilities(), Some(&[][..]));
    let stored: Vec<_> = ordered
        .required_capabilities()
        .unwrap()
        .iter()
        .map(CapabilityName::as_str)
        .collect();
    assert_eq!(stored, names);
    assert_ne!(absent, empty);
    assert_ne!(ordered, reordered, "order is preserved, not normalized");
}

#[test]
fn unconstrained_strings_preserve_empty_whitespace_and_unicode_exactly() {
    for value in ["", " ", "\t\n", "Ünïcode ✓ 🚀", "e\u{301}", "\u{202e}rtl"] {
        let node = task("n")
            .with_title(value)
            .with_task_capsule_ref(value)
            .with_result_ref(value)
            .with_locked_reason(value);
        assert_eq!(node.title(), Some(value));
        assert_eq!(node.task_capsule_ref(), Some(value));
        assert_eq!(node.result_ref(), Some(value));
        assert_eq!(node.locked_reason(), Some(Some(value)));
        for edge in [
            prec("p", "a", "b").with_note(value),
            ctrl("c", "a", "b", ControlKind::OnPass).with_note(value),
        ] {
            assert_eq!(edge.note(), Some(value));
        }
    }
    // Canonically equivalent Unicode spellings stay distinct (no normalization).
    assert_ne!(task("n").with_title("é"), task("n").with_title("e\u{301}"));
}

#[test]
fn constrained_identifiers_enforce_scalar_boundaries_and_preserve_text() {
    let at = |n: usize| "🚀".repeat(n);
    let too_long = |field| GraphError::IdentifierTooLong {
        field,
        length: 201,
        max: crate::error::MAX_IDENTIFIER_LENGTH,
    };
    for n in [1, 200] {
        let id = at(n);
        let node = GraphNode::new(
            id.as_str(),
            id.as_str(),
            GraphNodeKind::TASK,
            GraphNodeState::Planned,
        )
        .unwrap()
        .with_parent_node_id(id.as_str())
        .unwrap()
        .with_workstream_id(id.as_str())
        .unwrap()
        .with_workspace_id(id.as_str())
        .unwrap();
        assert_eq!(node.node_id(), id);
        assert_eq!(node.graph_id(), id);
        assert_eq!(node.parent_node_id(), Some(id.as_str()));
        assert_eq!(node.workstream_id(), Some(id.as_str()));
        assert_eq!(node.workspace_id(), Some(id.as_str()));
        for edge in [
            GraphEdge::precedence(
                id.as_str(),
                id.as_str(),
                id.as_str(),
                id.as_str(),
                PrecedenceKind::RequiresAccepted,
            )
            .unwrap(),
            GraphEdge::control(
                id.as_str(),
                id.as_str(),
                id.as_str(),
                id.as_str(),
                ControlKind::OnPass,
            )
            .unwrap(),
        ] {
            assert_eq!(edge.edge_id(), id);
            assert_eq!(edge.graph_id(), id);
            assert_eq!(edge.from_node(), id);
            assert_eq!(edge.to_node(), id);
        }
    }
    let long = at(201);
    let kind = GraphNodeKind::TASK;
    let state = GraphNodeState::Planned;
    assert_eq!(
        GraphNode::new(long.as_str(), G, kind.clone(), state),
        Err(too_long("node_id"))
    );
    assert_eq!(
        GraphNode::new("n", long.as_str(), kind.clone(), state),
        Err(too_long("graph_id"))
    );
    assert_eq!(
        GraphNode::new("n", "", kind, state),
        Err(GraphError::EmptyIdentifier { field: "graph_id" })
    );
    assert_eq!(
        task("n").with_parent_node_id(long.as_str()),
        Err(too_long("parent_node_id"))
    );
    assert_eq!(
        task("n").with_workstream_id(long.as_str()),
        Err(too_long("workstream_id"))
    );
    assert_eq!(
        task("n").with_workspace_id(long.as_str()),
        Err(too_long("workspace_id"))
    );
    for field in ["parent_node_id", "workstream_id", "workspace_id"] {
        let result = match field {
            "parent_node_id" => task("n").with_parent_node_id(""),
            "workstream_id" => task("n").with_workstream_id(""),
            _ => task("n").with_workspace_id(""),
        };
        assert_eq!(result, Err(GraphError::EmptyIdentifier { field }));
    }
    let p = PrecedenceKind::RequiresAccepted;
    let c = ControlKind::OnPass;
    assert_eq!(
        GraphEdge::precedence("e", long.as_str(), "a", "b", p),
        Err(too_long("graph_id"))
    );
    assert_eq!(
        GraphEdge::control("e", long.as_str(), "a", "b", c),
        Err(too_long("graph_id"))
    );
    assert_eq!(
        GraphEdge::precedence("e", "", "a", "b", p),
        Err(GraphError::EmptyIdentifier { field: "graph_id" })
    );
    assert_eq!(
        GraphEdge::control("e", "", "a", "b", c),
        Err(GraphError::EmptyIdentifier { field: "graph_id" })
    );
    assert_eq!(
        GraphEdge::control(long.as_str(), G, "a", "b", c),
        Err(too_long("edge_id"))
    );
    assert_eq!(
        GraphEdge::control("e", G, "a", long.as_str(), c),
        Err(too_long("to_node"))
    );
}

#[test]
fn code_sha_requires_exactly_forty_lowercase_ascii_hex() {
    assert_eq!(task("n").with_code_sha(SHA).unwrap().code_sha(), Some(SHA));
    let invalid = [
        String::new(),
        SHA[..39].to_owned(),
        format!("{SHA}0"),
        SHA.to_uppercase(),
        format!("{}g", &SHA[..39]),
        format!("{}é", &SHA[..38]),
        format!(" {}", &SHA[..39]),
    ];
    for value in invalid {
        assert_eq!(
            task("n").with_code_sha(value.as_str()),
            Err(GraphError::InvalidCodeSha { value })
        );
    }
}

#[test]
fn positive_integer_fields_accept_unbounded_canonical_decimals_only() {
    let long = format!("1{}", "0".repeat(10_000));
    for value in ["1", "42", ABOVE_U64, long.as_str()] {
        let node = task("n")
            .with_attempt_number(attempt(value))
            .with_created_in_version(version(value));
        assert_eq!(node.attempt_number().unwrap().as_str(), value);
        assert_eq!(node.created_in_version().unwrap().as_str(), value);
    }
    for value in [
        "", "0", "00", "01", "-1", "+1", "1.0", "1e3", " 1", "1 ", "١", "１",
    ] {
        assert_eq!(
            GraphNodeAttemptNumber::try_new(value),
            Err(GraphError::InvalidAttemptNumber {
                value: value.to_owned()
            })
        );
        assert!(GraphVersionV1::try_new(value).is_err(), "{value:?}");
    }
}

#[test]
fn edge_note_and_graph_id_are_preserved_for_both_classes() {
    let precedence = prec("p", "a", "b");
    let control = ctrl("c", "a", "b", ControlKind::OnReject);
    for edge in [&precedence, &control] {
        assert_eq!(edge.graph_id(), G);
        assert_eq!(edge.note(), None);
        assert_ne!(
            edge.clone().with_note(""),
            *edge,
            "empty note differs from absent"
        );
    }
    assert_eq!(
        precedence.precedence_kind(),
        Some(PrecedenceKind::RequiresAccepted)
    );
    assert_eq!(precedence.control_kind(), None);
    assert_eq!(control.control_kind(), Some(ControlKind::OnReject));
    assert_eq!(control.precedence_kind(), None);
    // The note never alters class or kind.
    assert_eq!(
        precedence.clone().with_note("n").relation(),
        precedence.relation()
    );
    assert_eq!(
        control.clone().with_note("n").relation(),
        control.relation()
    );
}

fn mismatch(child: &'static str, child_id: &str, child_graph_id: &str) -> GraphError {
    GraphError::ChildGraphIdMismatch {
        child,
        child_id: child_id.to_owned(),
        graph_id: G.to_owned(),
        child_graph_id: child_graph_id.to_owned(),
    }
}

#[test]
fn matching_child_graph_ids_are_accepted_by_every_path() {
    let nodes = vec![task("a"), task("b")];
    let edges = vec![
        prec("p", "a", "b"),
        ctrl("c", "b", "a", ControlKind::OnReject),
    ];
    let batched = ExecutionGraph::from_parts(G, nodes.clone(), edges.clone()).unwrap();
    let mut incremental = ExecutionGraph::new(G).unwrap();
    for node in nodes {
        incremental.add_node(node).unwrap();
    }
    for edge in edges {
        assert_eq!(incremental.validate_edge_addition(&edge), Ok(()));
        incremental.add_edge(edge).unwrap();
    }
    assert_eq!(batched, incremental);
}

#[test]
fn mismatched_child_graph_ids_are_rejected_atomically_by_every_path() {
    // Exact comparison: case, whitespace and Unicode spelling all differ.
    for other in ["other", "G", "g ", "\u{0261}"] {
        let foreign_node =
            GraphNode::new("x", other, GraphNodeKind::TASK, GraphNodeState::Planned).unwrap();
        let foreign_edges = [
            GraphEdge::precedence("fe", other, "a", "b", PrecedenceKind::RequiresAccepted).unwrap(),
            GraphEdge::control("fe", other, "a", "b", ControlKind::OnPass).unwrap(),
        ];

        // Insertion: nodes, including a duplicate id with a foreign graph.
        let mut graph = chain_graph(G, &["a", "b"]);
        let before = graph.clone();
        assert_eq!(
            graph.add_node(foreign_node.clone()),
            Err(mismatch("node", "x", other))
        );
        let foreign_duplicate =
            GraphNode::new("a", other, GraphNodeKind::TASK, GraphNodeState::Planned).unwrap();
        assert_eq!(
            graph.add_node(foreign_duplicate),
            Err(mismatch("node", "a", other))
        );
        assert_eq!(graph, before);

        // Dry-run and insertion for both edge classes with valid endpoints.
        for edge in &foreign_edges {
            assert_eq!(
                graph.validate_edge_addition(edge),
                Err(mismatch("edge", "fe", other))
            );
            assert_eq!(
                graph.add_edge(edge.clone()),
                Err(mismatch("edge", "fe", other))
            );
            assert_eq!(graph, before);
        }

        // Batch construction returns no graph.
        assert_eq!(
            ExecutionGraph::from_parts(G, vec![task("a"), foreign_node.clone()], vec![]),
            Err(mismatch("node", "x", other)),
        );
        for edge in &foreign_edges {
            assert_eq!(
                ExecutionGraph::from_parts(
                    G,
                    vec![task("a"), task("b")],
                    vec![prec("p", "a", "b"), edge.clone()],
                ),
                Err(mismatch("edge", "fe", other)),
            );
        }
    }
}

#[test]
fn child_graph_identity_is_checked_before_other_edge_violations() {
    let mut graph = chain_graph(G, &["a", "b"]);
    let before = graph.clone();
    // Duplicate id, dangling endpoint and precedence cycle all lose to the
    // identity check, in both dry-run and insertion.
    for edge in [
        GraphEdge::precedence("p_a_b", "other", "a", "b", PrecedenceKind::RequiresAccepted)
            .unwrap(),
        GraphEdge::control("e", "other", "a", "ghost", ControlKind::OnPass).unwrap(),
        GraphEdge::precedence("e", "other", "b", "a", PrecedenceKind::RequiresAccepted).unwrap(),
    ] {
        let expected = Err(mismatch("edge", edge.edge_id(), "other"));
        assert_eq!(graph.validate_edge_addition(&edge), expected);
        assert_eq!(graph.add_edge(edge), expected);
        assert_eq!(graph, before);
    }
    // Batch identity check precedes duplicate-edge detection.
    assert_eq!(
        ExecutionGraph::from_parts(
            G,
            vec![task("a"), task("b")],
            vec![
                prec("e", "a", "b"),
                GraphEdge::control("e", "other", "a", "b", ControlKind::OnPass).unwrap(),
            ],
        ),
        Err(mismatch("edge", "e", "other")),
    );
}
