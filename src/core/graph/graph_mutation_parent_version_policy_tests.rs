use super::{
    ExecutionGraph, GraphMutation, GraphMutationActor, GraphMutationOperation,
    GraphMutationOperationKind, GraphMutationParentVersionError, GraphNode, GraphNodeKind,
    GraphNodeState, GraphVersionV1, validate_graph_mutation_parent_version,
};
use crate::orchestration::OrchestrationDateTimeV1;

fn version(value: &str) -> GraphVersionV1 {
    GraphVersionV1::try_new(value).unwrap()
}

fn mutation(
    parent: &str,
    resulting: &str,
    actor: GraphMutationActor,
    op: GraphMutationOperationKind,
    graph_id: &str,
) -> GraphMutation {
    GraphMutation::try_new(
        "mutation",
        graph_id,
        version(parent),
        version(resulting),
        actor,
        "reason",
        OrchestrationDateTimeV1::try_new("2026-09-13T12:00:00Z").unwrap(),
        vec![GraphMutationOperation::try_new(op, Some("node".into()), None, None).unwrap()],
        None,
    )
    .unwrap()
}

fn default_mutation(parent: &str) -> GraphMutation {
    mutation(
        parent,
        "2",
        GraphMutationActor::try_new("USER", None).unwrap(),
        GraphMutationOperationKind::AddNode,
        "graph",
    )
}

#[test]
fn same_versions_pass() {
    for value in ["1", "2", "10"] {
        assert_eq!(
            validate_graph_mutation_parent_version(&version(value), &default_mutation(value)),
            Ok(()),
            "{value}"
        );
    }
}

#[test]
fn different_versions_fail_with_typed_mismatch() {
    for (current, parent) in [
        ("1", "2"),
        ("2", "1"),
        ("9", "10"),
        ("10", "9"),
        ("10", "11"),
        ("11", "10"),
    ] {
        assert_eq!(
            validate_graph_mutation_parent_version(&version(current), &default_mutation(parent)),
            Err(GraphMutationParentVersionError),
            "current={current}, parent={parent}"
        );
    }
}

#[test]
fn arbitrary_size_versions_use_exact_equality() {
    let huge_a = "9".repeat(200);
    let huge_b = format!("{}8", "9".repeat(199));
    for (current, parent, expected) in [
        (&huge_a, &huge_a, Ok(())),
        (&huge_b, &huge_b, Ok(())),
        (&huge_a, &huge_b, Err(GraphMutationParentVersionError)),
        (&huge_b, &huge_a, Err(GraphMutationParentVersionError)),
    ] {
        assert_eq!(
            validate_graph_mutation_parent_version(&version(current), &default_mutation(parent)),
            expected
        );
    }
}

fn assert_parent_only(records: impl IntoIterator<Item = GraphMutation>) {
    for record in records {
        assert_eq!(record.parent_version(), &version("10"));
        for (current, expected) in [("10", Ok(())), ("11", Err(GraphMutationParentVersionError))] {
            assert_eq!(
                validate_graph_mutation_parent_version(&version(current), &record),
                expected,
                "{record:?}"
            );
        }
    }
}

#[test]
fn resulting_version_has_no_effect() {
    assert_parent_only(["2", "10", "11", &"9".repeat(200)].map(|resulting| {
        mutation(
            "10",
            resulting,
            GraphMutationActor::try_new("USER", None).unwrap(),
            GraphMutationOperationKind::AddNode,
            "graph",
        )
    }));
}

#[test]
fn actor_evidence_has_no_effect() {
    for role in ["USER", "RUNTIME_A1", "RUNTIME_A3", "", "FUTURE_ROLE"] {
        assert_parent_only([None, Some("logical-role".into())].map(|role_id| {
            mutation(
                "10",
                "2",
                GraphMutationActor::try_new(role, role_id).unwrap(),
                GraphMutationOperationKind::AddNode,
                "graph",
            )
        }));
    }
}

#[test]
fn operation_kind_has_no_effect() {
    assert_parent_only(GraphMutationOperationKind::ALL.map(|op| {
        mutation(
            "10",
            "2",
            GraphMutationActor::try_new("USER", None).unwrap(),
            op,
            "graph",
        )
    }));
}

#[test]
fn graph_id_has_no_effect() {
    assert_parent_only(["graph", "other-graph", "🦀"].map(|graph_id| {
        mutation(
            "10",
            "2",
            GraphMutationActor::try_new("USER", None).unwrap(),
            GraphMutationOperationKind::AddNode,
            graph_id,
        )
    }));
}

#[test]
fn repeated_validation_preserves_inputs_and_does_not_execute_operations() {
    let node =
        GraphNode::new("node", GraphNodeKind::TASK, GraphNodeState::Planned, vec![]).unwrap();
    let graph = ExecutionGraph::from_parts("graph", vec![node], vec![]).unwrap();
    let graph_before = graph.clone();
    for op in GraphMutationOperationKind::ALL {
        let record = mutation(
            "10",
            "2",
            GraphMutationActor::try_new("USER", None).unwrap(),
            op,
            "graph",
        );
        let record_before = record.clone();
        for (current, expected) in [("10", Ok(())), ("11", Err(GraphMutationParentVersionError))] {
            let current = version(current);
            let current_before = current.clone();
            for _ in 0..4 {
                // Only shared version and record evidence cross the policy boundary.
                assert_eq!(
                    validate_graph_mutation_parent_version(&current, &record),
                    expected
                );
                assert_eq!(record, record_before);
                assert_eq!(current, current_before);
                assert_eq!(graph, graph_before);
            }
        }
    }
}
