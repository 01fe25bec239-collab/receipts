use super::{
    ControlKind, ExecutionGraph, GraphEdge, GraphMutation, GraphMutationActor,
    GraphMutationOperation, GraphMutationOperationKind, GraphMutationTargetGraphError, GraphNode,
    GraphNodeKind, GraphNodeState, GraphVersionV1, validate_graph_mutation_target_graph,
};
use crate::orchestration::OrchestrationDateTimeV1;

fn version(value: &str) -> GraphVersionV1 {
    GraphVersionV1::try_new(value).unwrap()
}

struct MutationFixture {
    mutation_id: &'static str,
    parent_version: GraphVersionV1,
    resulting_version: GraphVersionV1,
    actor: GraphMutationActor,
    reason: &'static str,
    created_at: OrchestrationDateTimeV1,
    operations: Vec<GraphMutationOperation>,
    resulting_digest: Option<String>,
}

impl MutationFixture {
    fn new() -> Self {
        Self {
            mutation_id: "mutation",
            parent_version: version("1"),
            resulting_version: version("2"),
            actor: GraphMutationActor::try_new("USER", None).unwrap(),
            reason: "reason",
            created_at: OrchestrationDateTimeV1::try_new("2026-09-13T12:00:00Z").unwrap(),
            operations: vec![operation(GraphMutationOperationKind::AddNode)],
            resulting_digest: None,
        }
    }

    fn build(&self, graph_id: &str) -> GraphMutation {
        GraphMutation::try_new(
            self.mutation_id,
            graph_id,
            self.parent_version.clone(),
            self.resulting_version.clone(),
            self.actor.clone(),
            self.reason,
            self.created_at.clone(),
            self.operations.clone(),
            self.resulting_digest.clone(),
        )
        .unwrap()
    }
}

fn operation(kind: GraphMutationOperationKind) -> GraphMutationOperation {
    GraphMutationOperation::try_new(kind, Some("node".into()), None, None).unwrap()
}

// Every variant must retain both the pass and the fail-closed outcome.
fn assert_field_has_no_effect(change: impl FnOnce(&mut MutationFixture)) {
    let baseline = MutationFixture::new();
    let mut changed = MutationFixture::new();
    change(&mut changed);
    assert_ne!(baseline.build("graph-a"), changed.build("graph-a"));
    let graph = ExecutionGraph::new("graph-a").unwrap();
    for (id, expected) in [
        ("graph-a", Ok(())),
        ("graph-b", Err(GraphMutationTargetGraphError)),
    ] {
        for record in [baseline.build(id), changed.build(id)] {
            assert_eq!(
                validate_graph_mutation_target_graph(&graph, &record),
                expected
            );
        }
    }
}

#[test]
fn matching_graph_ids_pass() {
    for id in [
        "graph-a",
        "Graph-a",
        "graph-a ",
        " ",
        "🦀",
        &"g".repeat(200),
    ] {
        let graph = ExecutionGraph::new(id).unwrap();
        let record = MutationFixture::new().build(id);
        assert_eq!(
            validate_graph_mutation_target_graph(&graph, &record),
            Ok(())
        );
    }
}

#[test]
fn distinct_opaque_graph_ids_fail_with_typed_error() {
    for other in [
        "graph-b",
        "Graph-a",
        "graph-a ",
        " graph-a",
        "graph-a-child",
        "graph",
        "é",
        "e\u{301}",
    ] {
        for (graph_id, mutation_id) in [("graph-a", other), (other, "graph-a")] {
            let graph = ExecutionGraph::new(graph_id).unwrap();
            let record = MutationFixture::new().build(mutation_id);
            assert_eq!(
                validate_graph_mutation_target_graph(&graph, &record),
                Err(GraphMutationTargetGraphError)
            );
        }
    }
    let graph = ExecutionGraph::new("é").unwrap();
    let record = MutationFixture::new().build("e\u{301}");
    assert_eq!(
        validate_graph_mutation_target_graph(&graph, &record),
        Err(GraphMutationTargetGraphError)
    );
}

#[test]
fn parent_version_has_no_effect_including_match_and_mismatch() {
    // ExecutionGraph stores no version; relative to supplied current "1",
    // these parents include both matching and mismatching evidence.
    for parent in ["2", "10", &"9".repeat(200)] {
        assert_field_has_no_effect(|fixture| fixture.parent_version = version(parent));
    }
}

#[test]
fn resulting_version_has_no_effect() {
    for resulting in ["3", "10", &"9".repeat(200)] {
        assert_field_has_no_effect(|fixture| fixture.resulting_version = version(resulting));
    }
}

#[test]
fn allowed_and_unauthorized_actor_evidence_has_no_effect() {
    for role in [
        "GRAPH_COMPILER",
        "RUNTIME_A1",
        "RUNTIME_A2",
        "GOAL_EVALUATOR",
        "RUNTIME_A3",
        "",
        "user",
        "FUTURE_ROLE",
    ] {
        assert_field_has_no_effect(|fixture| {
            fixture.actor = GraphMutationActor::try_new(role, None).unwrap();
        });
    }
    assert_field_has_no_effect(|fixture| {
        fixture.actor = GraphMutationActor::try_new("USER", Some("role-id".into())).unwrap();
    });
}

#[test]
fn reason_has_no_effect() {
    assert_field_has_no_effect(|fixture| fixture.reason = "different reason");
}

#[test]
fn operations_have_no_effect() {
    for kind in GraphMutationOperationKind::ALL {
        assert_field_has_no_effect(|fixture| {
            fixture.operations = vec![
                GraphMutationOperation::try_new(kind, None, Some("edge".into()), None).unwrap(),
                operation(kind),
            ];
        });
    }
}

#[test]
fn mutation_id_has_no_effect() {
    assert_field_has_no_effect(|fixture| fixture.mutation_id = "other-mutation");
}

#[test]
fn created_at_has_no_effect() {
    assert_field_has_no_effect(|fixture| {
        fixture.created_at = OrchestrationDateTimeV1::try_new("2026-09-14T15:30:00Z").unwrap();
    });
}

#[test]
fn resulting_digest_has_no_effect() {
    for digest in ["0".repeat(64), "abcdef01".repeat(8)] {
        assert_field_has_no_effect(|fixture| fixture.resulting_digest = Some(digest));
    }
}

#[test]
fn repeated_pass_and_rejection_preserve_graph_and_mutation_exactly() {
    let node =
        GraphNode::new("node", GraphNodeKind::TASK, GraphNodeState::Planned, vec![]).unwrap();
    let edge = GraphEdge::control("edge", "node", "node", ControlKind::OnPass).unwrap();
    let graph = ExecutionGraph::from_parts("graph-a", vec![node], vec![edge]).unwrap();
    let graph_before = graph.clone();
    for kind in GraphMutationOperationKind::ALL {
        let mut fixture = MutationFixture::new();
        fixture.operations = vec![operation(kind)];
        for (id, expected) in [
            ("graph-a", Ok(())),
            ("graph-b", Err(GraphMutationTargetGraphError)),
        ] {
            let record = fixture.build(id);
            let record_before = record.clone();
            for _ in 0..4 {
                assert_eq!(
                    validate_graph_mutation_target_graph(&graph, &record),
                    expected
                );
                assert_eq!(graph, graph_before);
                assert_eq!(record, record_before);
            }
        }
    }
}

#[test]
fn mismatch_error_is_typed_and_deterministic() {
    let graph = ExecutionGraph::new("graph-a").unwrap();
    let record = MutationFixture::new().build("graph-b");
    for _ in 0..4 {
        let error: GraphMutationTargetGraphError =
            validate_graph_mutation_target_graph(&graph, &record).unwrap_err();
        assert_eq!(error, GraphMutationTargetGraphError);
        let error: &dyn std::error::Error = &error;
        assert_eq!(
            error.to_string(),
            "graph mutation does not target this execution graph"
        );
        assert!(error.source().is_none());
    }
}
