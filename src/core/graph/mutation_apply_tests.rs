//! Tests exercise the public typed append boundary and its unchanged inputs.

use crate::goal::GoalEvaluationContextEpoch;
use crate::orchestration::OrchestrationDateTimeV1;
use crate::{
    CapabilityName, ControlKind, ExecutionGraph, ExecutionGraphEnvelope,
    ExecutionGraphEnvelopeFields, GraphAppendError, GraphEdge, GraphError, GraphNode,
    GraphNodeAttemptNumber, GraphNodeKind, GraphNodeState, GraphVersionV1, PrecedenceKind,
    graph_content_digest_v1,
};

fn ver(value: &str) -> GraphVersionV1 {
    GraphVersionV1::try_new(value).unwrap()
}

fn time() -> OrchestrationDateTimeV1 {
    OrchestrationDateTimeV1::try_new("2026-10-27T12:34:56.123+05:30").unwrap()
}

fn node(id: &str) -> GraphNode {
    GraphNode::new(id, "g", GraphNodeKind::TASK, GraphNodeState::Planned).unwrap()
}

fn edge(id: &str, from: &str, to: &str) -> GraphEdge {
    GraphEdge::precedence(id, "g", from, to, PrecedenceKind::RequiresAccepted).unwrap()
}

fn envelope(version: &str) -> ExecutionGraphEnvelope {
    let graph =
        ExecutionGraph::from_parts("g", vec![node("a"), node("b")], vec![edge("ab", "a", "b")])
            .unwrap();
    ExecutionGraphEnvelope::bind(
        &graph,
        ExecutionGraphEnvelopeFields::new(
            "project",
            "goal",
            ver(version),
            OrchestrationDateTimeV1::try_new("2026-10-25T00:00:00Z").unwrap(),
            "policy",
        ),
    )
    .unwrap()
}

fn refused(
    source: &ExecutionGraphEnvelope,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
    expected: GraphAppendError,
) {
    let before = (source.clone(), nodes.to_vec(), edges.to_vec());
    assert_eq!(
        source.append(source.graph_version(), time(), nodes, edges, None),
        Err(expected)
    );
    assert_eq!((source.clone(), nodes.to_vec(), edges.to_vec()), before);
}

#[test]
fn multi_node_edge_append_preserves_values_and_binds_computed_digest() {
    let source = envelope("9");
    let nodes = vec![
        node("d").with_created_in_version(ver("10")),
        node("c")
            .with_title(" 日本語\n")
            .with_null_locked_reason()
            .with_parent_node_id("unresolved-parent")
            .unwrap()
            .with_attempt_number(
                GraphNodeAttemptNumber::try_new("999999999999999999999999").unwrap(),
            )
            .with_required_capabilities(vec![CapabilityName::new("test.cap").unwrap(); 2])
            .with_task_capsule_ref("")
            .with_workstream_id("stream")
            .unwrap()
            .with_code_sha("a".repeat(40))
            .unwrap()
            .with_workspace_id("workspace")
            .unwrap()
            .with_result_ref("existing data only"),
    ];
    let edges = vec![edge("cd", "c", "d").with_note(""), edge("bc", "b", "c")];
    let before = (source.clone(), nodes.clone(), edges.clone());
    let child = source
        .append(&ver("9"), time(), &nodes, &edges, None)
        .unwrap();
    assert_eq!(child.graph().node_count(), 4);
    assert_eq!(child.graph().edge_count(), 3);
    assert_eq!(child.graph_version(), &ver("10"));
    assert_eq!(child.parent_version(), Some(Some(&ver("9"))));
    assert_eq!(child.created_at(), &time());
    assert_eq!(child.project_id(), source.project_id());
    assert_eq!(child.graph_id(), source.graph_id());
    assert_eq!(child.goal_id(), source.goal_id());
    assert_eq!(child.policy_id(), source.policy_id());
    for n in source.graph().nodes().chain(nodes.iter()) {
        assert_eq!(child.graph().node(n.node_id()), Some(n));
    }
    for e in source.graph().edges().chain(edges.iter()) {
        assert_eq!(child.graph().edge(e.edge_id()), Some(e));
    }
    assert_eq!(child.graph().node("c").unwrap().created_in_version(), None);
    assert_eq!(child.compiled_from(), None);
    assert_eq!(child.context_epoch(), None);
    let digest = graph_content_digest_v1(child.graph()).unwrap();
    assert_eq!(child.resulting_digest(), Some(digest.as_str()));
    assert_eq!(
        source
            .append(&ver("9"), time(), &nodes, &edges, Some(&digest))
            .unwrap(),
        child
    );
    assert_eq!((source, nodes, edges), before);
}

#[test]
fn node_only_and_edge_only_each_advance_once() {
    let source = envelope("1");
    let child = source
        .append(&ver("1"), time(), &[node("c")], &[], None)
        .unwrap();
    assert_eq!(child.graph_version(), &ver("2"));
    assert_eq!(child.graph().edge_count(), 1);
    let next = child
        .append(&ver("2"), time(), &[], &[edge("bc", "b", "c")], None)
        .unwrap();
    assert_eq!(next.graph_version(), &ver("3"));
    assert_eq!(next.parent_version(), Some(Some(&ver("2"))));
    assert_eq!(next.graph().node_count(), 3);
    assert_eq!(next.graph().edge_count(), 2);
}

#[test]
fn empty_and_stale_parent_refused() {
    let source = envelope("10");
    refused(&source, &[], &[], GraphAppendError::EmptyBatch);
    let before = source.clone();
    for parent in ["1", "9", "11", "999999999999999999999999999999999"] {
        assert_eq!(
            source.append(&ver(parent), time(), &[node("c")], &[], None),
            Err(GraphAppendError::ParentVersionMismatch)
        );
    }
    assert_eq!(source, before);
}

#[test]
fn duplicate_nodes_and_edges_existing_and_in_batch_refused() {
    let source = envelope("1");
    refused(
        &source,
        &[node("c"), node("a")],
        &[],
        GraphAppendError::Graph(GraphError::DuplicateNodeId {
            node_id: "a".into(),
        }),
    );
    refused(
        &source,
        &[node("c"), node("c")],
        &[],
        GraphAppendError::Graph(GraphError::DuplicateNodeId {
            node_id: "c".into(),
        }),
    );
    refused(
        &source,
        &[node("c")],
        &[edge("bc", "b", "c"), edge("ab", "a", "b")],
        GraphAppendError::Graph(GraphError::DuplicateEdgeId {
            edge_id: "ab".into(),
        }),
    );
    refused(
        &source,
        &[],
        &[edge("extra", "a", "b"), edge("extra", "a", "b")],
        GraphAppendError::Graph(GraphError::DuplicateEdgeId {
            edge_id: "extra".into(),
        }),
    );
}

#[test]
fn wrong_child_graphs_and_each_missing_endpoint_refused() {
    let source = envelope("1");
    refused(
        &source,
        &[
            node("c"),
            GraphNode::new(
                "foreign",
                "other",
                GraphNodeKind::TASK,
                GraphNodeState::Planned,
            )
            .unwrap(),
        ],
        &[],
        GraphAppendError::Graph(GraphError::ChildGraphIdMismatch {
            child: "node",
            child_id: "foreign".into(),
            graph_id: "g".into(),
            child_graph_id: "other".into(),
        }),
    );
    refused(
        &source,
        &[node("c")],
        &[GraphEdge::control("foreign", "other", "a", "b", ControlKind::OnReject).unwrap()],
        GraphAppendError::Graph(GraphError::ChildGraphIdMismatch {
            child: "edge",
            child_id: "foreign".into(),
            graph_id: "g".into(),
            child_graph_id: "other".into(),
        }),
    );
    for (from, to) in [("missing", "c"), ("c", "missing")] {
        refused(
            &source,
            &[node("c")],
            &[edge("bc", "b", "c"), edge("late", from, to)],
            GraphAppendError::Graph(GraphError::UnknownNodeReference {
                edge_id: "late".into(),
                node_id: "missing".into(),
            }),
        );
    }
}

#[test]
fn precedence_cycles_refused_but_control_cycles_allowed() {
    let source = envelope("1");
    let before = source.clone();
    for edges in [
        vec![edge("bc", "b", "c"), edge("ca", "c", "a")],
        vec![edge("self", "c", "c")],
    ] {
        assert!(matches!(
            source.append(&ver("1"), time(), &[node("c")], &edges, None),
            Err(GraphAppendError::Graph(
                GraphError::PrecedenceCycleRejected { .. }
            ))
        ));
        assert_eq!(source, before);
    }
    let edges = [
        GraphEdge::control("bc", "g", "b", "c", ControlKind::OnReject).unwrap(),
        GraphEdge::control("cb", "g", "c", "b", ControlKind::OnFailure).unwrap(),
        GraphEdge::control("ba", "g", "b", "a", ControlKind::OnPass).unwrap(),
    ];
    let child = source
        .append(&ver("1"), time(), &[node("c")], &edges, None)
        .unwrap();
    assert!(!child.graph().has_precedence_cycle());
    assert_eq!(child.graph().edge_count(), 4);
}

#[test]
fn exact_decimal_successor_has_no_machine_or_digit_ceiling() {
    for (parent, successor) in [
        ("1".into(), "2".into()),
        ("1299".into(), "1300".into()),
        ("18446744073709551615".into(), "18446744073709551616".into()),
        ("9".repeat(10000), format!("1{}", "0".repeat(10000))),
        (
            format!("1{}", "0".repeat(10000)),
            format!("1{}1", "0".repeat(9999)),
        ),
    ] {
        let source = envelope(&parent);
        let child = source
            .append(
                &ver(&parent),
                time(),
                &[node("c").with_created_in_version(ver(&successor))],
                &[],
                None,
            )
            .unwrap();
        assert_eq!(child.graph_version().as_str(), successor);
        assert_eq!(child.parent_version(), Some(Some(&ver(&parent))));
    }
}

#[test]
fn all_optional_envelope_metadata_preserved_except_replaced_fields() {
    let graph = envelope("1").graph().clone();
    let refs = vec!["".into(), " 日本語\n".into(), "b".into(), "b".into()];
    for compiled in [None, Some(vec![]), Some(refs)] {
        for parent in [None, Some(None), Some(Some(ver("999")))] {
            let mut fields =
                ExecutionGraphEnvelopeFields::new("project", "goal", ver("8"), time(), "policy")
                    .with_resulting_digest("0".repeat(64))
                    .with_context_epoch(
                        GoalEvaluationContextEpoch::from_decimal(&"9".repeat(500)).unwrap(),
                    );
            if let Some(refs) = &compiled {
                fields = fields.with_compiled_from(refs.clone());
            }
            fields = match parent {
                None => fields,
                Some(None) => fields.with_null_parent_version(),
                Some(Some(v)) => fields.with_parent_version(v),
            };
            let source = ExecutionGraphEnvelope::bind(&graph, fields).unwrap();
            let before = source.clone();
            let child = source
                .append(&ver("8"), time(), &[node("c")], &[], None)
                .unwrap();
            assert_eq!(child.compiled_from(), source.compiled_from());
            assert_eq!(child.context_epoch(), source.context_epoch());
            assert_eq!(child.parent_version(), Some(Some(&ver("8"))));
            assert_eq!(
                child.resulting_digest(),
                Some(graph_content_digest_v1(child.graph()).unwrap().as_str())
            );
            assert_eq!(source, before);
        }
    }
}

#[test]
fn created_in_version_mismatch_and_late_digest_failure_leave_inputs_unchanged() {
    let source = envelope("1");
    for wrong in ["1", "3", "999999999999999999999999999"] {
        refused(
            &source,
            &[node("c"), node("d").with_created_in_version(ver(wrong))],
            &[],
            GraphAppendError::CreatedInVersionMismatch {
                node_id: "d".into(),
            },
        );
    }
    let nodes = [node("c"), node("d")];
    let edges = [edge("bc", "b", "c"), edge("cd", "c", "d")];
    let before = (source.clone(), nodes.clone(), edges.clone());
    let computed = source
        .append(&ver("1"), time(), &nodes, &edges, None)
        .unwrap()
        .resulting_digest()
        .unwrap()
        .to_owned();
    for wrong in [
        "".into(),
        "0".repeat(64),
        computed.to_uppercase(),
        format!(" {computed}"),
    ] {
        assert_eq!(
            source.append(&ver("1"), time(), &nodes, &edges, Some(&wrong)),
            Err(GraphAppendError::ExpectedDigestMismatch)
        );
        assert_eq!((source.clone(), nodes.clone(), edges.clone()), before);
    }
}

#[test]
fn equivalent_insertion_orders_produce_identical_envelopes_and_digest() {
    let first = envelope("1");
    let reordered_graph =
        ExecutionGraph::from_parts("g", vec![node("b"), node("a")], vec![edge("ab", "a", "b")])
            .unwrap();
    let second = ExecutionGraphEnvelope::bind(
        &reordered_graph,
        ExecutionGraphEnvelopeFields::new(
            "project",
            "goal",
            ver("1"),
            OrchestrationDateTimeV1::try_new("2026-10-25T00:00:00Z").unwrap(),
            "policy",
        ),
    )
    .unwrap();
    let nodes = [node("c"), node("d")];
    let edges = [edge("bc", "b", "c"), edge("cd", "c", "d")];
    let child = first
        .append(&ver("1"), time(), &nodes, &edges, None)
        .unwrap();
    let reversed = second
        .append(
            &ver("1"),
            time(),
            &[nodes[1].clone(), nodes[0].clone()],
            &[edges[1].clone(), edges[0].clone()],
            None,
        )
        .unwrap();
    assert_eq!(child, reversed);
    assert_eq!(
        child.resulting_digest(),
        Some(graph_content_digest_v1(reversed.graph()).unwrap().as_str())
    );
}
