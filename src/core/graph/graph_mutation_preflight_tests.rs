use super::{
    ControlKind, ExecutionGraph, GraphEdge, GraphMutation, GraphMutationActor,
    GraphMutationActorAuthorizationError, GraphMutationOperation, GraphMutationOperationKind,
    GraphMutationParentVersionError, GraphMutationPreflightError, GraphMutationTargetGraphError,
    GraphNode, GraphNodeKind, GraphNodeState, GraphVersionV1, validate_graph_mutation_actor,
    validate_graph_mutation_parent_version, validate_graph_mutation_preflight,
    validate_graph_mutation_target_graph,
};
use crate::orchestration::OrchestrationDateTimeV1;
use std::error::Error;

fn version(value: &str) -> GraphVersionV1 {
    GraphVersionV1::try_new(value).unwrap()
}

struct MutationFixture {
    mutation_id: &'static str,
    resulting_version: GraphVersionV1,
    role_id: Option<String>,
    reason: &'static str,
    created_at: OrchestrationDateTimeV1,
    operations: Vec<GraphMutationOperation>,
    resulting_digest: Option<String>,
}

impl MutationFixture {
    fn new() -> Self {
        Self {
            mutation_id: "mutation",
            resulting_version: version("2"),
            role_id: None,
            reason: "reason",
            created_at: OrchestrationDateTimeV1::try_new("2026-09-13T12:00:00Z").unwrap(),
            operations: vec![
                GraphMutationOperation::try_new(
                    GraphMutationOperationKind::AddNode,
                    Some("node".into()),
                    None,
                    None,
                )
                .unwrap(),
            ],
            resulting_digest: None,
        }
    }

    fn build(&self, graph_id: &str, role: &str, parent: &str) -> GraphMutation {
        GraphMutation::try_new(
            self.mutation_id,
            graph_id,
            version(parent),
            self.resulting_version.clone(),
            GraphMutationActor::try_new(role, self.role_id.clone()).unwrap(),
            self.reason,
            self.created_at.clone(),
            self.operations.clone(),
            self.resulting_digest.clone(),
        )
        .unwrap()
    }
}

fn graph() -> ExecutionGraph {
    let node =
        GraphNode::new("node", GraphNodeKind::TASK, GraphNodeState::Planned, vec![]).unwrap();
    let edge = GraphEdge::control("edge", "node", "node", ControlKind::OnPass).unwrap();
    ExecutionGraph::from_parts("graph", vec![node], vec![edge]).unwrap()
}

#[test]
fn all_eight_guard_combinations_have_fixed_precedence_and_preserve_inputs() {
    use GraphMutationPreflightError::{ActorAuthorization, ParentVersion, TargetGraph};
    let graph = graph();
    let before = graph.clone();
    let current = version("10");
    let current_before = current.clone();
    for (id, role, parent, expected) in [
        ("graph", "USER", "10", Ok(())),
        (
            "other",
            "USER",
            "10",
            Err(TargetGraph(GraphMutationTargetGraphError)),
        ),
        (
            "other",
            "RUNTIME_A3",
            "10",
            Err(TargetGraph(GraphMutationTargetGraphError)),
        ),
        (
            "other",
            "USER",
            "11",
            Err(TargetGraph(GraphMutationTargetGraphError)),
        ),
        (
            "other",
            "RUNTIME_A3",
            "11",
            Err(TargetGraph(GraphMutationTargetGraphError)),
        ),
        (
            "graph",
            "RUNTIME_A3",
            "10",
            Err(ActorAuthorization(GraphMutationActorAuthorizationError)),
        ),
        (
            "graph",
            "RUNTIME_A3",
            "11",
            Err(ActorAuthorization(GraphMutationActorAuthorizationError)),
        ),
        (
            "graph",
            "USER",
            "11",
            Err(ParentVersion(GraphMutationParentVersionError)),
        ),
    ] {
        let mutation = MutationFixture::new().build(id, role, parent);
        let mutation_before = mutation.clone();
        for _ in 0..4 {
            assert_eq!(
                validate_graph_mutation_preflight(&graph, &current, &mutation),
                expected
            );
            assert_eq!(graph, before);
            assert_eq!(mutation, mutation_before);
            assert_eq!(current, current_before);
        }
    }
}

#[test]
fn each_error_preserves_the_canonical_value_and_source() {
    let graph = graph();
    let current = version("10");
    for (id, role, parent) in [
        ("other", "USER", "10"),
        ("graph", "RUNTIME_A3", "10"),
        ("graph", "USER", "11"),
    ] {
        let mutation = MutationFixture::new().build(id, role, parent);
        let error = validate_graph_mutation_preflight(&graph, &current, &mutation).unwrap_err();
        let source = error.source().unwrap();
        match error {
            GraphMutationPreflightError::TargetGraph(inner) => {
                assert_eq!(
                    Err(inner),
                    validate_graph_mutation_target_graph(&graph, &mutation)
                );
                assert_eq!(
                    source.downcast_ref::<GraphMutationTargetGraphError>(),
                    Some(&inner)
                );
            }
            GraphMutationPreflightError::ActorAuthorization(inner) => {
                assert_eq!(Err(inner), validate_graph_mutation_actor(mutation.actor()));
                assert_eq!(
                    source.downcast_ref::<GraphMutationActorAuthorizationError>(),
                    Some(&inner)
                );
            }
            GraphMutationPreflightError::ParentVersion(inner) => {
                assert_eq!(
                    Err(inner),
                    validate_graph_mutation_parent_version(&current, &mutation)
                );
                assert_eq!(
                    source.downcast_ref::<GraphMutationParentVersionError>(),
                    Some(&inner)
                );
            }
        }
        assert_eq!(error.to_string(), source.to_string());
        assert!(source.source().is_none());
    }
}

#[test]
fn opaque_target_identity_follows_the_canonical_policy() {
    for id in ["graph", "Graph", "graph ", " ", "é", "e\u{301}", "🦀"] {
        let graph = ExecutionGraph::new(id).unwrap();
        for target in [id, "other"] {
            let mutation = MutationFixture::new().build(target, "USER", "10");
            assert_eq!(
                validate_graph_mutation_preflight(&graph, &version("10"), &mutation),
                validate_graph_mutation_target_graph(&graph, &mutation)
                    .map_err(GraphMutationPreflightError::TargetGraph)
            );
        }
    }
}

#[test]
fn actor_roles_and_role_ids_follow_the_canonical_policy() {
    for role in [
        "GRAPH_COMPILER",
        "RUNTIME_A1",
        "RUNTIME_A2",
        "GOAL_EVALUATOR",
        "USER",
        "RUNTIME_A3",
        "",
        "user",
        " USER",
        "USER ",
        "FUTURE_ROLE",
        "ＲＵＮＴＩＭＥ＿Ａ１",
    ] {
        for role_id in [None, Some("RUNTIME_A1".into())] {
            let mut fixture = MutationFixture::new();
            fixture.role_id = role_id;
            let mutation = fixture.build("graph", role, "10");
            assert_eq!(
                validate_graph_mutation_preflight(&graph(), &version("10"), &mutation),
                validate_graph_mutation_actor(mutation.actor())
                    .map_err(GraphMutationPreflightError::ActorAuthorization)
            );
        }
    }
}

#[test]
fn parent_versions_follow_canonical_equality_without_a_numeric_ceiling() {
    let huge = "9".repeat(200);
    for current in ["1", "9", "10", &huge] {
        for parent in ["1", "9", "10", &huge] {
            let mutation = MutationFixture::new().build("graph", "USER", parent);
            let current = version(current);
            assert_eq!(
                validate_graph_mutation_preflight(&graph(), &current, &mutation),
                validate_graph_mutation_parent_version(&current, &mutation)
                    .map_err(GraphMutationPreflightError::ParentVersion)
            );
        }
    }
}

fn assert_field_has_no_effect(change: impl FnOnce(&mut MutationFixture)) {
    let baseline = MutationFixture::new();
    let mut changed = MutationFixture::new();
    change(&mut changed);
    for (id, role, parent) in [
        ("graph", "USER", "10"),
        ("other", "USER", "10"),
        ("graph", "RUNTIME_A3", "10"),
        ("graph", "USER", "11"),
    ] {
        let baseline = baseline.build(id, role, parent);
        let changed = changed.build(id, role, parent);
        assert_ne!(baseline, changed);
        assert_eq!(
            validate_graph_mutation_preflight(&graph(), &version("10"), &baseline),
            validate_graph_mutation_preflight(&graph(), &version("10"), &changed)
        );
    }
}

#[test]
fn resulting_version_has_no_effect() {
    for value in ["3", "10", "11", &"9".repeat(200)] {
        assert_field_has_no_effect(|fixture| fixture.resulting_version = version(value));
    }
}

#[test]
fn unrelated_record_fields_have_no_effect() {
    assert_field_has_no_effect(|fixture| fixture.mutation_id = "other-mutation");
    assert_field_has_no_effect(|fixture| fixture.reason = "different reason");
    assert_field_has_no_effect(|fixture| {
        fixture.created_at = OrchestrationDateTimeV1::try_new("2026-09-14T15:30:00Z").unwrap();
    });
    assert_field_has_no_effect(|fixture| fixture.resulting_digest = Some("0".repeat(64)));
    for kind in GraphMutationOperationKind::ALL {
        // Valid record evidence need not describe an executable operation.
        assert_field_has_no_effect(|fixture| {
            fixture.operations =
                vec![GraphMutationOperation::try_new(kind, None, None, None).unwrap()];
        });
    }
}
