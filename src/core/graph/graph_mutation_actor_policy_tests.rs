use super::{
    ExecutionGraph, GraphEdge, GraphMutationActor, GraphMutationActorAuthorizationError, GraphNode,
    GraphNodeKind, GraphNodeState, PrecedenceKind, validate_graph_mutation_actor,
};

const AUTHORIZED: [&str; 5] = [
    "GRAPH_COMPILER",
    "RUNTIME_A1",
    "RUNTIME_A2",
    "GOAL_EVALUATOR",
    "USER",
];
const REJECTED: [&str; 26] = [
    "RUNTIME_A3",
    "RUNTIME_A4",
    "",
    "graph_compiler",
    "Graph_Compiler",
    "runtime_a1",
    "Runtime_A1",
    " RUNTIME_A1",
    "RUNTIME_A1 ",
    "A1",
    "A2",
    "A3",
    "A4",
    "ADMIN",
    "SYSTEM",
    "MANAGER",
    "ROOT",
    "ORCHESTRATOR",
    "FUTURE_ROLE",
    "RUNTIME_A10",
    "RUNTIME_A1_ADMIN",
    "PREFIX_RUNTIME_A1",
    "PREFIX_RUNTIME_A1_SUFFIX",
    "RUNTIME_A1\0",
    "ＲＵＮＴＩＭＥ＿Ａ１",
    "🦀",
];

#[test]
fn graph_compiler_is_authorized_without_role_id() {
    let actor = GraphMutationActor::try_new("GRAPH_COMPILER", None).unwrap();
    assert_eq!(validate_graph_mutation_actor(&actor), Ok(()));
}

#[test]
fn runtime_a1_is_authorized_without_role_id() {
    let actor = GraphMutationActor::try_new("RUNTIME_A1", None).unwrap();
    assert_eq!(validate_graph_mutation_actor(&actor), Ok(()));
}

#[test]
fn runtime_a2_is_authorized_without_role_id() {
    let actor = GraphMutationActor::try_new("RUNTIME_A2", None).unwrap();
    assert_eq!(validate_graph_mutation_actor(&actor), Ok(()));
}

#[test]
fn goal_evaluator_is_authorized_without_role_id() {
    let actor = GraphMutationActor::try_new("GOAL_EVALUATOR", None).unwrap();
    assert_eq!(validate_graph_mutation_actor(&actor), Ok(()));
}

#[test]
fn user_is_authorized_without_role_id() {
    let actor = GraphMutationActor::try_new("USER", None).unwrap();
    assert_eq!(validate_graph_mutation_actor(&actor), Ok(()));
}

#[test]
fn unauthorized_roles_fail_closed_with_or_without_plausible_role_id() {
    for role in REJECTED {
        for role_id in [None, Some("logical-role-runtime-a1-001".into())] {
            let actor = GraphMutationActor::try_new(role, role_id).unwrap();
            assert_eq!(
                validate_graph_mutation_actor(&actor),
                Err(GraphMutationActorAuthorizationError),
                "{actor:?}"
            );
        }
    }
}

#[test]
fn role_id_presence_and_content_do_not_change_authorization() {
    for role in AUTHORIZED.into_iter().chain(REJECTED) {
        let without_id = GraphMutationActor::try_new(role, None).unwrap();
        let expected = validate_graph_mutation_actor(&without_id);
        for role_id in ["logical-role-runtime-a1-001", "USER", "RUNTIME_A3"] {
            let actor = GraphMutationActor::try_new(role, Some(role_id.into())).unwrap();
            assert_eq!(actor.role_id(), Some(role_id));
            assert_eq!(validate_graph_mutation_actor(&actor), expected, "{actor:?}");
        }
    }
}

#[test]
fn every_authorized_role_requires_exact_string_identity() {
    for role in AUTHORIZED {
        for variant in [
            role.to_lowercase(),
            format!(" {role}"),
            format!("{role} "),
            format!("\t{role}"),
            format!("{role}\n"),
            format!("{role}\u{200b}"),
            format!("PREFIX_{role}"),
            format!("{role}_SUFFIX"),
            format!("PREFIX_{role}_SUFFIX"),
        ] {
            let actor = GraphMutationActor::try_new(variant, None).unwrap();
            assert_eq!(
                validate_graph_mutation_actor(&actor),
                Err(GraphMutationActorAuthorizationError),
                "{actor:?}"
            );
        }
    }
}

#[test]
fn repeated_validation_preserves_actor_evidence() {
    for role in AUTHORIZED.into_iter().chain(REJECTED) {
        for role_id in [None, Some("logical-role-runtime-a1-001".into())] {
            let actor = GraphMutationActor::try_new(role, role_id).unwrap();
            let before = actor.clone();
            let expected = validate_graph_mutation_actor(&actor);
            for _ in 0..4 {
                assert_eq!(validate_graph_mutation_actor(&actor), expected);
                assert_eq!(actor, before);
            }
        }
    }
}

#[test]
fn validation_leaves_execution_graph_unchanged() {
    let nodes = ["first", "second"]
        .map(|id| GraphNode::new(id, GraphNodeKind::TASK, GraphNodeState::Planned, vec![]).unwrap())
        .to_vec();
    let edge =
        GraphEdge::precedence("edge", "first", "second", PrecedenceKind::RequiresAccepted).unwrap();
    let graph = ExecutionGraph::from_parts("graph", nodes, vec![edge]).unwrap();
    let before = graph.clone();
    // The policy's only input is shared actor evidence, never graph authority.
    for role in AUTHORIZED.into_iter().chain(REJECTED) {
        let actor = GraphMutationActor::try_new(role, None).unwrap();
        assert_eq!(
            validate_graph_mutation_actor(&actor).is_ok(),
            AUTHORIZED.contains(&role)
        );
        assert_eq!(graph, before);
    }
}

#[test]
fn actor_construction_still_enforces_role_id_structure() {
    for role in ["RUNTIME_A1", "RUNTIME_A3"] {
        for invalid_id in [String::new(), "x".repeat(201)] {
            assert!(GraphMutationActor::try_new(role, Some(invalid_id)).is_err());
        }
    }
}
