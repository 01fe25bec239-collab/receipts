use super::*;
use crate::orchestration::{
    OrchestrationDateTimeV1, OrchestrationJsonNumberV1, OrchestrationJsonObjectV1,
    OrchestrationJsonValueV1 as Json,
};

fn version(value: &str) -> GraphVersionV1 {
    GraphVersionV1::try_new(value).unwrap()
}
fn time() -> OrchestrationDateTimeV1 {
    OrchestrationDateTimeV1::try_new("2026-09-09t12:34:56.100z").unwrap()
}
fn object() -> OrchestrationJsonObjectV1 {
    OrchestrationJsonObjectV1::new(std::collections::BTreeMap::from([(
        "nested".into(),
        Json::Array(vec![
            Json::Null,
            Json::Boolean(true),
            Json::String("text 🦀".into()),
            Json::Number(OrchestrationJsonNumberV1::try_new("1E+0").unwrap()),
            Json::Object(OrchestrationJsonObjectV1::default()),
        ]),
    )]))
}
fn operation() -> GraphMutationOperation {
    GraphMutationOperation::try_new(GraphMutationOperationKind::AddNode, None, None, None).unwrap()
}
fn mutation(parent: &str, resulting: &str) -> Result<GraphMutation, GraphRecordError> {
    GraphMutation::try_new(
        "mutation",
        "graph",
        version(parent),
        version(resulting),
        GraphMutationActor::try_new("", None).unwrap(),
        "reason",
        time(),
        vec![operation()],
        None,
    )
}

#[test]
fn graph_version_canonical_unbounded_and_exact() {
    for input in [
        "1",
        "2",
        "10",
        "18446744073709551615",
        "18446744073709551616",
        &"9".repeat(10_000),
    ] {
        let value = version(input);
        assert_eq!(value.as_str(), input);
        assert_eq!(value, value.clone());
        assert!(std::collections::HashSet::from([value.clone()]).contains(&value));
    }
    for input in [
        "",
        "0",
        "00",
        "01",
        "001",
        "+1",
        "-1",
        "1.0",
        "1e0",
        "1E0",
        "1E+0",
        " 1",
        "1 ",
        "1\t0",
        "1\n",
        "１",
        "١",
        "−1",
        "1🦀",
        "1é",
        ".",
        "NaN",
        "Infinity",
        &format!("{}é", "9".repeat(10_000)),
        &format!("0{}", "9".repeat(10_000)),
    ] {
        assert_eq!(
            GraphVersionV1::try_new(input),
            Err(GraphVersionError),
            "{input:?}"
        );
    }
}

#[test]
fn record_versions_require_only_the_frozen_minima() {
    for input in ["1", "18446744073709551616", &"9".repeat(10_000)] {
        let snapshot =
            GraphSnapshot::try_new("g", version(input), time(), vec![], None, None).unwrap();
        assert_eq!(snapshot.graph_version().as_str(), input);
        let record = mutation(input, "2").unwrap();
        assert_eq!(record.parent_version().as_str(), input);
    }
    assert_eq!(
        mutation("1", "1"),
        Err(GraphRecordError::ResultingVersionBelowTwo)
    );
    for input in ["2", "18446744073709551616", &"9".repeat(10_000)] {
        assert_eq!(
            mutation("1", input).unwrap().resulting_version().as_str(),
            input
        );
    }
    for (parent, resulting) in [("5", "9"), ("9", "5"), ("5", "5")] {
        let record = mutation(parent, resulting).unwrap();
        assert_eq!(record.parent_version().as_str(), parent);
        assert_eq!(record.resulting_version().as_str(), resulting);
    }
}

#[test]
fn snapshot_required_and_optional_fields_and_order() {
    let minimal = GraphSnapshot::try_new("g", version("1"), time(), vec![], None, None).unwrap();
    assert_eq!(minimal.graph_id(), "g");
    assert_eq!(minimal.captured_at(), &time());
    assert!(minimal.node_states().is_empty());
    assert_eq!(minimal.summary(), None);
    assert_eq!(minimal.resulting_digest(), None);
    let nodes: Vec<_> = GraphNodeState::ALL
        .into_iter()
        .enumerate()
        .map(|(i, state)| {
            GraphSnapshotNodeState::try_new(
                format!("node-{i}"),
                state,
                Some(GraphNodeKind::new("FUTURE_KIND").unwrap()),
                Some(false),
                Some("a".repeat(40)),
            )
            .unwrap()
        })
        .collect();
    let snapshot = GraphSnapshot::try_new(
        "g",
        version("1"),
        time(),
        nodes.clone(),
        Some("b".repeat(64)),
        Some(object()),
    )
    .unwrap();
    assert_eq!(snapshot.node_states(), nodes);
    assert_eq!(snapshot.resulting_digest(), Some("b".repeat(64).as_str()));
    let summary: &OrchestrationJsonObjectV1 = snapshot.summary().unwrap();
    assert_eq!(summary, &object());
    let captured: &OrchestrationDateTimeV1 = snapshot.captured_at();
    assert_eq!(captured.as_str(), "2026-09-09t12:34:56.100z");
    for (i, node) in nodes.iter().enumerate() {
        assert_eq!(node.node_id(), format!("node-{i}"));
        assert_eq!(node.state(), GraphNodeState::ALL[i]);
        assert_eq!(node.kind().unwrap().as_str(), "FUTURE_KIND");
        assert_eq!(node.locked(), Some(false));
        assert_eq!(node.code_sha(), Some("a".repeat(40).as_str()));
    }
    let node =
        GraphSnapshotNodeState::try_new("n", GraphNodeState::Planned, None, None, None).unwrap();
    assert_eq!(
        (node.kind(), node.locked(), node.code_sha()),
        (None, None, None)
    );
    assert_eq!(
        GraphSnapshotNodeState::try_new("n", GraphNodeState::Ready, None, Some(true), None)
            .unwrap()
            .locked(),
        Some(true)
    );
}

#[test]
fn mutation_required_optional_fields_all_operations_and_order() {
    let operations: Vec<_> = GraphMutationOperationKind::ALL
        .into_iter()
        .map(|op| {
            GraphMutationOperation::try_new(
                op,
                Some("unknown-node".into()),
                Some("unknown-edge".into()),
                Some(object()),
            )
            .unwrap()
        })
        .collect();
    let record = GraphMutation::try_new(
        "m",
        "g",
        version("5"),
        version("9"),
        GraphMutationActor::try_new("FUTURE_ROLE", Some("role-id".into())).unwrap(),
        " ",
        time(),
        operations.clone(),
        Some("c".repeat(64)),
    )
    .unwrap();
    assert_eq!(record.mutation_id(), "m");
    assert_eq!(record.graph_id(), "g");
    assert_eq!(record.actor().role(), "FUTURE_ROLE");
    assert_eq!(record.actor().role_id(), Some("role-id"));
    assert_eq!(record.reason(), " ");
    let created: &OrchestrationDateTimeV1 = record.created_at();
    assert_eq!(created, &time());
    assert_eq!(record.resulting_digest(), Some("c".repeat(64).as_str()));
    assert_eq!(record.operations(), operations);
    for (i, op) in record.operations().iter().enumerate() {
        assert_eq!(op.op(), GraphMutationOperationKind::ALL[i]);
        assert_eq!(op.node_id(), Some("unknown-node"));
        assert_eq!(op.edge_id(), Some("unknown-edge"));
        let detail: &OrchestrationJsonObjectV1 = op.detail().unwrap();
        assert_eq!(detail, &object());
    }
    let minimal = mutation("1", "2").unwrap();
    assert_eq!(minimal.actor().role(), "");
    assert_eq!(minimal.actor().role_id(), None);
    assert_eq!(minimal.resulting_digest(), None);
    assert_eq!(minimal.operations()[0].node_id(), None);
    assert_eq!(minimal.operations()[0].edge_id(), None);
    assert_eq!(minimal.operations()[0].detail(), None);
    for (reason, ops, expected) in [
        ("", vec![operation()], GraphRecordError::EmptyReason),
        ("reason", vec![], GraphRecordError::EmptyOperations),
    ] {
        assert_eq!(
            GraphMutation::try_new(
                "m",
                "g",
                version("1"),
                version("2"),
                GraphMutationActor::try_new("role", None).unwrap(),
                reason,
                time(),
                ops,
                None
            ),
            Err(expected)
        );
    }
    assert!(
        GraphMutation::try_new(
            "m",
            "g",
            version("1"),
            version("2"),
            GraphMutationActor::try_new("role".repeat(1000), None).unwrap(),
            "r".repeat(1000),
            time(),
            vec![operation()],
            None
        )
        .is_ok()
    );
}

#[test]
fn every_identifier_enforces_scalar_bounds() {
    for (id, valid) in [
        ("".to_owned(), false),
        ("x".into(), true),
        ("🦀".repeat(200), true),
        ("🦀".repeat(201), false),
    ] {
        assert_eq!(
            GraphSnapshot::try_new(&id, version("1"), time(), vec![], None, None).is_ok(),
            valid
        );
        assert_eq!(
            GraphSnapshotNodeState::try_new(&id, GraphNodeState::Planned, None, None, None).is_ok(),
            valid
        );
        assert_eq!(
            GraphMutationActor::try_new("role", Some(id.clone())).is_ok(),
            valid
        );
        assert_eq!(
            GraphMutationOperation::try_new(
                GraphMutationOperationKind::AddNode,
                Some(id.clone()),
                None,
                None
            )
            .is_ok(),
            valid
        );
        assert_eq!(
            GraphMutationOperation::try_new(
                GraphMutationOperationKind::AddEdge,
                None,
                Some(id.clone()),
                None
            )
            .is_ok(),
            valid
        );
        for (mutation_id, graph_id) in [(id.as_str(), "g"), ("m", id.as_str())] {
            assert_eq!(
                GraphMutation::try_new(
                    mutation_id,
                    graph_id,
                    version("1"),
                    version("2"),
                    GraphMutationActor::try_new("role", None).unwrap(),
                    "r",
                    time(),
                    vec![operation()],
                    None
                )
                .is_ok(),
                valid
            );
        }
    }
}

#[test]
fn hex_fields_require_exact_lowercase_ascii() {
    for length in [40, 64] {
        for (value, valid) in [
            ("a".repeat(length), true),
            (("0123456789abcdef".repeat(4))[..length].to_owned(), true),
            ("".into(), false),
            ("a".repeat(length - 1), false),
            ("a".repeat(length + 1), false),
            ("A".repeat(length), false),
            ("g".repeat(length), false),
            ("é".repeat(length / 2), false),
            (format!("{}\n", "a".repeat(length - 1)), false),
        ] {
            if length == 40 {
                assert_eq!(
                    GraphSnapshotNodeState::try_new(
                        "n",
                        GraphNodeState::Ready,
                        None,
                        None,
                        Some(value)
                    )
                    .is_ok(),
                    valid
                );
            } else {
                assert_eq!(
                    GraphSnapshot::try_new(
                        "g",
                        version("1"),
                        time(),
                        vec![],
                        Some(value.clone()),
                        None
                    )
                    .is_ok(),
                    valid
                );
                assert_eq!(
                    GraphMutation::try_new(
                        "m",
                        "g",
                        version("1"),
                        version("2"),
                        GraphMutationActor::try_new("role", None).unwrap(),
                        "r",
                        time(),
                        vec![operation()],
                        Some(value)
                    )
                    .is_ok(),
                    valid
                );
            }
        }
    }
}

#[test]
fn records_do_not_execute_or_mutate_a_graph() {
    let graph = ExecutionGraph::new("graph").unwrap();
    let before = graph.clone();
    let record = mutation("5", "9").unwrap();
    let snapshot = GraphSnapshot::try_new(
        "graph",
        version("9"),
        time(),
        vec![
            GraphSnapshotNodeState::try_new("absent", GraphNodeState::Integrated, None, None, None)
                .unwrap(),
        ],
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        record.operations()[0].op(),
        GraphMutationOperationKind::AddNode
    );
    assert_eq!(
        snapshot.node_states()[0].state(),
        GraphNodeState::Integrated
    );
    assert_eq!(graph, before);
    assert_eq!(graph.node_count(), 0);
}
