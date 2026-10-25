//! Focused coverage for the explicitly bound in-memory `ExecutionGraph` envelope.

use crate::edge::{GraphEdge, PrecedenceKind};
use crate::error::GraphError;
use crate::execution_graph::{
    ExecutionGraph, ExecutionGraphEnvelope, ExecutionGraphEnvelopeError,
    ExecutionGraphEnvelopeFields,
};
use crate::goal::GoalEvaluationContextEpoch;
use crate::node::{GraphNode, GraphNodeKind};
use crate::node_state::GraphNodeState;
use crate::orchestration::OrchestrationDateTimeV1;
use crate::records::GraphRecordError;
use crate::version::GraphVersionV1;

fn node(id: &str) -> GraphNode {
    GraphNode::new(id, "g", GraphNodeKind::TASK, GraphNodeState::Planned).unwrap()
}

fn graph() -> ExecutionGraph {
    let mut g = ExecutionGraph::new("g").unwrap();
    g.add_node(node("a")).unwrap();
    g
}

fn ver(v: &str) -> GraphVersionV1 {
    GraphVersionV1::try_new(v).unwrap()
}

fn fields() -> ExecutionGraphEnvelopeFields {
    ExecutionGraphEnvelopeFields::new(
        "proj",
        "goal",
        ver("1"),
        OrchestrationDateTimeV1::try_new("2026-10-25T00:00:00Z").unwrap(),
        "policy",
    )
}

#[test]
fn minimal_envelope_preserves_required_and_absent_optionals() {
    let env = ExecutionGraphEnvelope::bind(&graph(), fields()).unwrap();
    assert_eq!(env.graph_id(), "g");
    assert_eq!(env.graph_version().as_str(), "1");
    assert_eq!(env.project_id(), "proj");
    assert_eq!(env.goal_id(), "goal");
    assert_eq!(env.policy_id(), "policy");
    assert_eq!(env.created_at().as_str(), "2026-10-25T00:00:00Z");
    assert_eq!(env.parent_version(), None);
    assert_eq!(env.resulting_digest(), None);
    assert_eq!(env.compiled_from(), None);
    assert_eq!(env.context_epoch(), None);
    assert_eq!(env.graph().node_count(), 1);
}

#[test]
fn parent_version_distinguishes_absent_null_and_value() {
    let null = ExecutionGraphEnvelope::bind(&graph(), fields().with_null_parent_version()).unwrap();
    assert_eq!(null.parent_version(), Some(None));
    let huge = "9".repeat(500);
    let value =
        ExecutionGraphEnvelope::bind(&graph(), fields().with_parent_version(ver(&huge))).unwrap();
    assert_eq!(value.parent_version().unwrap().unwrap().as_str(), huge);
}

#[test]
fn huge_version_and_epoch_have_no_machine_ceiling() {
    let huge = format!("1{}", "0".repeat(600));
    let f = ExecutionGraphEnvelopeFields::new(
        "proj",
        "goal",
        ver(&huge),
        OrchestrationDateTimeV1::try_new("2026-10-25T00:00:00Z").unwrap(),
        "policy",
    )
    .with_context_epoch(GoalEvaluationContextEpoch::from_decimal(&huge).unwrap())
    .with_resulting_digest("0".repeat(64));
    let env = ExecutionGraphEnvelope::bind(&graph(), f).unwrap();
    assert_eq!(env.graph_version().as_str(), huge);
    assert_eq!(env.context_epoch().unwrap().decimal_digits(), huge);
    assert_eq!(env.resulting_digest(), Some("0".repeat(64).as_str()));
}

#[test]
fn compiled_from_is_exact_strings_order_and_duplicates() {
    let refs = vec![
        String::new(),
        "b".to_owned(),
        "a".to_owned(),
        "b".to_owned(),
        " 日本語\n".to_owned(),
    ];
    let env =
        ExecutionGraphEnvelope::bind(&graph(), fields().with_compiled_from(refs.clone())).unwrap();
    assert_eq!(env.compiled_from(), Some(refs.as_slice()));
    let empty =
        ExecutionGraphEnvelope::bind(&graph(), fields().with_compiled_from(vec![])).unwrap();
    assert_eq!(empty.compiled_from(), Some(&[][..]));
}

#[test]
fn identifier_boundaries_use_unicode_scalar_count() {
    let max = "é".repeat(200);
    let env = ExecutionGraphEnvelope::bind(
        &graph(),
        ExecutionGraphEnvelopeFields::new(
            max.clone(),
            max.clone(),
            ver("1"),
            OrchestrationDateTimeV1::try_new("2026-10-25T00:00:00Z").unwrap(),
            max.clone(),
        ),
    )
    .unwrap();
    assert_eq!(env.project_id(), max);

    let long = "é".repeat(201);
    let when = || OrchestrationDateTimeV1::try_new("2026-10-25T00:00:00Z").unwrap();
    let cases = [
        (
            "project_id",
            ExecutionGraphEnvelopeFields::new(long.clone(), "g", ver("1"), when(), "p"),
        ),
        (
            "goal_id",
            ExecutionGraphEnvelopeFields::new("p", long.clone(), ver("1"), when(), "p"),
        ),
        (
            "policy_id",
            ExecutionGraphEnvelopeFields::new("p", "g", ver("1"), when(), long.clone()),
        ),
    ];
    for (field, f) in cases {
        assert_eq!(
            ExecutionGraphEnvelope::bind(&graph(), f),
            Err(ExecutionGraphEnvelopeError::Record(
                GraphRecordError::Identifier(GraphError::IdentifierTooLong {
                    field,
                    length: 201,
                    max: 200
                })
            ))
        );
    }
    for (field, f) in [
        (
            "project_id",
            ExecutionGraphEnvelopeFields::new("", "g", ver("1"), when(), "p"),
        ),
        (
            "goal_id",
            ExecutionGraphEnvelopeFields::new("p", "", ver("1"), when(), "p"),
        ),
        (
            "policy_id",
            ExecutionGraphEnvelopeFields::new("p", "g", ver("1"), when(), ""),
        ),
    ] {
        assert_eq!(
            ExecutionGraphEnvelope::bind(&graph(), f),
            Err(ExecutionGraphEnvelopeError::Record(
                GraphRecordError::Identifier(GraphError::EmptyIdentifier { field })
            ))
        );
    }
}

#[test]
fn invalid_digest_is_refused() {
    for bad in [
        "",
        "A".repeat(64).as_str(),
        &"a".repeat(63),
        &"a".repeat(65),
    ] {
        assert_eq!(
            ExecutionGraphEnvelope::bind(&graph(), fields().with_resulting_digest(bad)),
            Err(ExecutionGraphEnvelopeError::Record(
                GraphRecordError::InvalidHex {
                    field: "resulting_digest",
                    length: 64
                }
            ))
        );
    }
}

#[test]
fn empty_graph_is_refused_but_legacy_unbound_empty_graph_still_builds() {
    let empty = ExecutionGraph::new("g").unwrap();
    assert_eq!(
        ExecutionGraphEnvelope::bind(&empty, fields()),
        Err(ExecutionGraphEnvelopeError::EmptyGraph)
    );
    assert_eq!(
        ExecutionGraph::from_parts("g", vec![], vec![]).unwrap(),
        empty
    );
}

#[test]
fn bind_does_not_mutate_input_and_envelope_graph_is_independent() {
    let g = graph();
    let before = g.clone();
    let env = ExecutionGraphEnvelope::bind(&g, fields()).unwrap();
    assert_eq!(g, before);
    assert_eq!(env.graph(), &g);
    // Failure leaves the input untouched as well.
    assert!(ExecutionGraphEnvelope::bind(&g, fields().with_resulting_digest("x")).is_err());
    assert_eq!(g, before);
}

#[test]
fn containment_duplicates_endpoints_and_cycles_still_refused_before_binding() {
    let mut g = graph();
    let foreign =
        GraphNode::new("x", "other", GraphNodeKind::TASK, GraphNodeState::Planned).unwrap();
    assert!(matches!(
        g.add_node(foreign),
        Err(GraphError::ChildGraphIdMismatch { .. })
    ));
    assert!(matches!(
        g.add_node(node("a")),
        Err(GraphError::DuplicateNodeId { .. })
    ));
    g.add_node(node("b")).unwrap();
    let e = |id: &str, f: &str, t: &str| {
        GraphEdge::precedence(id, "g", f, t, PrecedenceKind::RequiresAccepted).unwrap()
    };
    g.add_edge(e("e1", "a", "b")).unwrap();
    assert!(matches!(
        g.add_edge(e("e2", "b", "a")),
        Err(GraphError::PrecedenceCycleRejected { .. })
    ));
    assert!(matches!(
        g.add_edge(e("e3", "a", "zz")),
        Err(GraphError::UnknownNodeReference { .. })
    ));
    let env = ExecutionGraphEnvelope::bind(&g, fields()).unwrap();
    assert_eq!(env.graph().edge_count(), 1);
}
