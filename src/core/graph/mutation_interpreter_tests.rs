//! Apply-v1 through the public interpreter: lifecycle, atomic refusal,
//! scope, results, repair expansion and cancellation.

use serde_json::{Value, json};

use crate::dag::{
    InterfaceArtifactRecord, InterfaceArtifactSource, InterfaceEvidenceVerifier,
    InterfaceProvenance, InterfaceSourceUnavailable,
};
use crate::orchestration::OrchestrationDateTimeV1;
use crate::wire::GraphMutationWireV1;
use crate::{
    ExecutionGraph, ExecutionGraphEnvelope, ExecutionGraphEnvelopeFields, GraphEdge, GraphMutation,
    GraphMutationApplyError, GraphMutationOutcomeV1, GraphMutationScopeV1, GraphNode,
    GraphNodeKind, GraphNodeState as S, GraphOperationError as O, GraphResultLedgerV1,
    GraphVersionV1, PrecedenceKind, ScopedActorV1, apply_graph_mutation_v1,
    graph_content_digest_v1,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn ver(v: &str) -> GraphVersionV1 {
    GraphVersionV1::try_new(v).unwrap()
}

fn node(id: &str, kind: GraphNodeKind, state: S) -> GraphNode {
    GraphNode::new(id, "g", kind, state)
        .unwrap()
        .with_workstream_id("w")
        .unwrap()
}

fn envelope(nodes: Vec<GraphNode>, edges: Vec<GraphEdge>, version: &str) -> ExecutionGraphEnvelope {
    envelope_in("P", "g", nodes, edges, version)
}

fn envelope_in(
    project: &str,
    graph_id: &str,
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    version: &str,
) -> ExecutionGraphEnvelope {
    let graph = ExecutionGraph::from_parts(graph_id, nodes, edges).unwrap();
    ExecutionGraphEnvelope::bind(
        &graph,
        ExecutionGraphEnvelopeFields::new(
            project,
            "goal",
            ver(version),
            OrchestrationDateTimeV1::try_new("2026-10-29T00:00:00Z").unwrap(),
            "free.single_runtime",
        )
        .with_compiled_from(vec!["spec".into()]),
    )
    .unwrap()
}

fn a1() -> ScopedActorV1 {
    ScopedActorV1::RuntimeA1 { role_id: None }
}
fn a2(ws: &str) -> ScopedActorV1 {
    ScopedActorV1::RuntimeA2 {
        role_id: Some("a2".into()),
        workstream_id: ws.into(),
    }
}
fn role(actor: &ScopedActorV1) -> Value {
    match actor {
        ScopedActorV1::User { .. } => json!({"role":"USER"}),
        ScopedActorV1::RuntimeA1 { .. } => json!({"role":"RUNTIME_A1"}),
        ScopedActorV1::RuntimeA2 { .. } => json!({"role":"RUNTIME_A2","role_id":"a2"}),
        ScopedActorV1::GoalEvaluator { .. } => json!({"role":"GOAL_EVALUATOR"}),
    }
}

fn mutation(env: &ExecutionGraphEnvelope, actor: &ScopedActorV1, ops: Value) -> GraphMutation {
    let parent: u128 = env.graph_version().as_str().parse().unwrap();
    let text = json!({
        "mutation_id": "m", "graph_id": env.graph_id(), "parent_version": parent,
        "resulting_version": parent + 1, "actor": role(actor), "reason": "test",
        "created_at": "2026-10-29T01:02:03Z", "operations": ops,
    });
    GraphMutationWireV1::from_json(&text.to_string())
        .unwrap()
        .to_mutation()
        .unwrap()
}

fn scope(env: &ExecutionGraphEnvelope, actor: ScopedActorV1) -> GraphMutationScopeV1 {
    GraphMutationScopeV1::new(
        env.project_id(),
        env.graph_id(),
        env.graph_version().clone(),
        actor,
    )
}

fn apply(
    env: &ExecutionGraphEnvelope,
    results: &GraphResultLedgerV1,
    actor: ScopedActorV1,
    ops: Value,
) -> Result<GraphMutationOutcomeV1, GraphMutationApplyError> {
    apply_with(env, results, actor, ops, None)
}

fn apply_with(
    env: &ExecutionGraphEnvelope,
    results: &GraphResultLedgerV1,
    actor: ScopedActorV1,
    ops: Value,
    interface: Option<InterfaceEvidenceVerifier<'_>>,
) -> Result<GraphMutationOutcomeV1, GraphMutationApplyError> {
    let before = (env.clone(), results.clone());
    let m = mutation(env, &actor, ops);
    let out = apply_graph_mutation_v1(env, results, &m, &scope(env, actor), interface);
    assert_eq!((env.clone(), results.clone()), before, "inputs changed");
    out
}

fn ok(
    env: &ExecutionGraphEnvelope,
    results: &GraphResultLedgerV1,
    actor: ScopedActorV1,
    ops: Value,
) -> (ExecutionGraphEnvelope, GraphResultLedgerV1) {
    let out = apply(env, results, actor, ops).unwrap();
    (out.envelope, out.results)
}

fn op_err(
    env: &ExecutionGraphEnvelope,
    results: &GraphResultLedgerV1,
    actor: ScopedActorV1,
    ops: Value,
) -> (usize, O) {
    match apply(env, results, actor, ops) {
        Err(GraphMutationApplyError::Operation { index, error }) => (index, error),
        other => panic!("expected operation refusal, got {other:?}"),
    }
}

fn set(id: &str, from: &str, to: &str, cause: Value) -> Value {
    json!({"op":"SET_NODE_STATE","node_id":id,"detail":{"from":from,"to":to,"cause":cause}})
}

fn run_to_review(id: &str) -> Vec<Value> {
    run_as(id, "at1")
}

/// PLANNED → AWAITING_REVIEW as one admitted execution `attempt`.
fn run_as(id: &str, attempt: &str) -> Vec<Value> {
    vec![
        set(
            id,
            "PLANNED",
            "READY",
            json!({"kind":"DEPENDENCIES_SATISFIED"}),
        ),
        set(
            id,
            "READY",
            "ADMITTED",
            json!({"kind":"ADMISSION","admission_decision_id":"ad"}),
        ),
        set(
            id,
            "ADMITTED",
            "DISPATCHED",
            json!({"kind":"DISPATCH","attempt_id":attempt}),
        ),
        set(
            id,
            "DISPATCHED",
            "RUNNING",
            json!({"kind":"START","attempt_id":attempt}),
        ),
        set(
            id,
            "RUNNING",
            "AWAITING_REVIEW",
            json!({"kind":"SUBMIT","attempt_id":attempt}),
        ),
    ]
}

fn result(id: &str, outcome: &str, review: &str) -> Value {
    json!({"node_id":id,"graph_id":"g","outcome":outcome,"produced_at":"2026-10-29T00:00:00Z",
           "attempt_id":"at1","review_id":review,"code_sha":SHA,
           "checks":[{"command":["cargo","test"],"exit_code":0}]})
}

fn attach(id: &str, r: &str, result: Value) -> Value {
    json!({"op":"ATTACH_RESULT","node_id":id,"detail":{"result_ref":r,"result":result}})
}

fn verdict(id: &str, to: &str, cited: &str) -> Value {
    set(
        id,
        "AWAITING_REVIEW",
        to,
        json!({"kind":"REVIEW_VERDICT","result_node_id":cited}),
    )
}

fn task_env() -> ExecutionGraphEnvelope {
    envelope(
        vec![
            node("t", GraphNodeKind::TASK, S::Planned)
                .with_code_sha(SHA)
                .unwrap(),
            node("next", GraphNodeKind::TASK, S::Planned),
        ],
        vec![
            GraphEdge::precedence("e", "g", "t", "next", PrecedenceKind::RequiresAccepted).unwrap(),
        ],
        "1",
    )
}

#[test]
fn full_lifecycle_through_apply_with_digest_and_successor() {
    let env = task_env();
    let none = GraphResultLedgerV1::new();
    let (env2, results) = ok(&env, &none, a2("w"), Value::Array(run_to_review("t")));
    assert_eq!(env2.graph_version().as_str(), "2");
    assert_eq!(env2.parent_version(), Some(Some(&ver("1"))));
    assert_eq!(env2.created_at().as_str(), "2026-10-29T01:02:03Z");
    assert_eq!(env2.compiled_from(), env.compiled_from());
    assert_eq!(
        env2.resulting_digest(),
        Some(graph_content_digest_v1(env2.graph()).unwrap().as_str())
    );
    assert_eq!(env2.graph().node_state("t"), Some(S::AwaitingReview));
    // `next` waits on t's acceptance.
    let (_, e) = op_err(
        &env2,
        &results,
        a1(),
        json!([set(
            "next",
            "PLANNED",
            "READY",
            json!({"kind":"DEPENDENCIES_SATISFIED"})
        )]),
    );
    assert_eq!(e, O::EvidenceRefused("dependencies"));

    let (env3, results) = ok(
        &env2,
        &results,
        a2("w"),
        json!([
            attach("t", "res-1", result("t", "PASS", "rv1")),
            verdict("t", "PASSED", "t")
        ]),
    );
    assert_eq!(env3.graph().node("t").unwrap().result_ref(), Some("res-1"));
    // A2 cannot accept its own work; A1 can, then integrate.
    let accept = set(
        "t",
        "PASSED",
        "ACCEPTED",
        json!({"kind":"ACCEPTANCE","decision_id":"d1"}),
    );
    assert!(matches!(
        op_err(&env3, &results, a2("w"), json!([accept.clone()])).1,
        O::NotPermitted(_)
    ));
    let (env4, results) = ok(
        &env3,
        &results,
        a1(),
        json!([
            accept,
            set(
                "t",
                "ACCEPTED",
                "INTEGRATED",
                json!({"kind":"INTEGRATION","decision_id":"d2"})
            ),
            set(
                "next",
                "PLANNED",
                "READY",
                json!({"kind":"DEPENDENCIES_SATISFIED"})
            )
        ]),
    );
    assert_eq!(env4.graph().node_state("t"), Some(S::Integrated));
    assert_eq!(env4.graph().node_state("next"), Some(S::Ready));
    assert_eq!(results.len(), 1);
}

#[test]
fn ordered_batch_refuses_atomically_at_late_failure() {
    let env = task_env();
    let none = GraphResultLedgerV1::new();
    let mut ops = run_to_review("t");
    ops.push(set(
        "t",
        "AWAITING_REVIEW",
        "ACCEPTED",
        json!({"kind":"ACCEPTANCE","decision_id":"d"}),
    ));
    let (index, error) = op_err(&env, &none, a1(), Value::Array(ops));
    assert_eq!(index, 5);
    assert!(matches!(error, O::Transition(_)));
    // Out-of-order steps refuse: compare-and-set on the current state.
    let (index, error) = op_err(&env, &none, a1(), json!([run_to_review("t")[1]]));
    assert_eq!(
        (index, error),
        (
            0,
            O::StateMismatch {
                expected: S::Ready,
                actual: S::Planned
            }
        )
    );
}

#[test]
fn preflight_scope_version_and_digest_refusals() {
    let env = task_env();
    let none = GraphResultLedgerV1::new();
    let ops = json!([run_to_review("t")[0]]);
    let m = mutation(&env, &a1(), ops.clone());
    // Authority comes from the caller scope, never the mutation actor.
    for (scope, field) in [
        (
            GraphMutationScopeV1::new("other", "g", ver("1"), a1()),
            "project_id",
        ),
        (
            GraphMutationScopeV1::new("P", "h", ver("1"), a1()),
            "graph_id",
        ),
        (
            GraphMutationScopeV1::new("P", "g", ver("2"), a1()),
            "graph_version",
        ),
        (
            GraphMutationScopeV1::new("P", "g", ver("1"), ScopedActorV1::User { role_id: None }),
            "actor.role",
        ),
        (
            GraphMutationScopeV1::new(
                "P",
                "g",
                ver("1"),
                ScopedActorV1::RuntimeA1 {
                    role_id: Some("x".into()),
                },
            ),
            "actor.role_id",
        ),
    ] {
        assert_eq!(
            apply_graph_mutation_v1(&env, &none, &m, &scope, None),
            Err(GraphMutationApplyError::Scope(field))
        );
    }
    // A3/A4 actors fail the canonical preflight; a compiler has no scope.
    for role in ["RUNTIME_A3", "RUNTIME_A4", "GRAPH_COMPILER"] {
        let text = GraphMutationWireV1::from_mutation(&m)
            .to_json()
            .replace("RUNTIME_A1", role);
        let m = GraphMutationWireV1::from_json(&text)
            .unwrap()
            .to_mutation()
            .unwrap();
        assert!(
            apply_graph_mutation_v1(&env, &none, &m, &scope(&env, a1()), None).is_err(),
            "{role}"
        );
    }
    // Stale parent and non-successor resulting version.
    let stale = envelope(
        vec![node("t", GraphNodeKind::TASK, S::Planned)],
        vec![],
        "2",
    );
    assert!(matches!(
        apply_graph_mutation_v1(&stale, &none, &m, &scope(&stale, a1()), None),
        Err(GraphMutationApplyError::Preflight(_))
    ));
    let skip = GraphMutationWireV1::from_mutation(&m)
        .to_json()
        .replace("\"resulting_version\":2", "\"resulting_version\":3");
    let skip = GraphMutationWireV1::from_json(&skip)
        .unwrap()
        .to_mutation()
        .unwrap();
    assert_eq!(
        apply_graph_mutation_v1(&env, &none, &skip, &scope(&env, a1()), None),
        Err(GraphMutationApplyError::ResultingVersionNotSuccessor)
    );
    // Supplied digest must equal the computed one.
    let good = apply_graph_mutation_v1(&env, &none, &m, &scope(&env, a1()), None).unwrap();
    let digest = good.envelope.resulting_digest().unwrap().to_owned();
    let with = |d: &str| {
        let mut v: Value =
            serde_json::from_str(&GraphMutationWireV1::from_mutation(&m).to_json()).unwrap();
        v["resulting_digest"] = json!(d);
        GraphMutationWireV1::from_json(&v.to_string())
            .unwrap()
            .to_mutation()
            .unwrap()
    };
    assert_eq!(
        apply_graph_mutation_v1(&env, &none, &with(&digest), &scope(&env, a1()), None),
        Ok(good)
    );
    assert_eq!(
        apply_graph_mutation_v1(
            &env,
            &none,
            &with(&"0".repeat(64)),
            &scope(&env, a1()),
            None
        ),
        Err(GraphMutationApplyError::DigestMismatch)
    );
}

#[test]
fn detail_shapes_are_exact() {
    let env = task_env();
    let none = GraphResultLedgerV1::new();
    let n = |id: &str| json!({"node_id":id,"graph_id":"g","kind":"TASK","state":"PLANNED","workstream_id":"w"});
    let refusals = [
        json!({"op":"ADD_NODE","node_id":"x"}),
        json!({"op":"ADD_NODE","node_id":"x","detail":{}}),
        json!({"op":"ADD_NODE","node_id":"x","detail":{"node":n("x"),"extra":1}}),
        json!({"op":"ADD_NODE","node_id":"y","detail":{"node":n("x")}}),
        json!({"op":"ADD_NODE","detail":{"node":n("x")}}),
        json!({"op":"ADD_NODE","node_id":"x","edge_id":"e","detail":{"node":n("x")}}),
        json!({"op":"ADD_NODE","node_id":"x","detail":{"node":"x"}}),
        json!({"op":"ADD_EDGE","node_id":"t","detail":{}}),
        json!({"op":"CANCEL_NODE","node_id":"t","detail":{"why":"x"}}),
        json!({"op":"SET_NODE_STATE","node_id":"t","detail":{"from":"PLANNED","to":"READY"}}),
        set(
            "t",
            "PLANNED",
            "READY",
            json!({"kind":"DEPENDENCIES_SATISFIED","extra":true}),
        ),
        set("t", "PLANNED", "READY", json!({"kind":"UNKNOWN"})),
        set(
            "t",
            "PLANNED",
            "Ready",
            json!({"kind":"DEPENDENCIES_SATISFIED"}),
        ),
        set(
            "t",
            "PLANNED",
            "BLOCKED",
            json!({"kind":"BLOCKED","reason":""}),
        ),
        set("t", "PLANNED", "CANCELLED", json!({"kind":"CANCELLATION"})),
        json!({"op":"ATTACH_RESULT","node_id":"t","detail":{"result_ref":"","result":result("t","PASS","r")}}),
        json!({"op":"ATTACH_RESULT","node_id":"t","detail":{"result_ref":"r","result":{"node_id":"t"}}}),
        set(
            "ghost",
            "PLANNED",
            "READY",
            json!({"kind":"DEPENDENCIES_SATISFIED"}),
        ),
    ];
    for op in refusals {
        let (index, _) = op_err(&env, &none, a1(), json!([op.clone()]));
        assert_eq!(index, 0, "{op}");
    }
    // Adding a duplicate node, a claimed-success node or one with a result.
    for (bad, why) in [
        (n("t"), None),
        (
            json!({"node_id":"x","graph_id":"g","kind":"TASK","state":"ACCEPTED"}),
            Some("state"),
        ),
        (
            json!({"node_id":"x","graph_id":"g","kind":"TASK","state":"PLANNED","result_ref":"r"}),
            Some("result_ref"),
        ),
        (
            json!({"node_id":"x","graph_id":"g","kind":"TASK","state":"PLANNED","created_in_version":3}),
            Some("created_in_version"),
        ),
    ] {
        let id = bad["node_id"].as_str().unwrap().to_owned();
        let (_, e) = op_err(
            &env,
            &none,
            a1(),
            json!([{"op":"ADD_NODE","node_id":id,"detail":{"node":bad}}]),
        );
        match why {
            Some(w) => assert_eq!(e, O::NewNode(w)),
            None => assert!(matches!(e, O::Graph(_))),
        }
    }
}

#[test]
fn workstream_and_evaluator_scope() {
    let env = envelope(
        vec![
            node("t", GraphNodeKind::TASK, S::Planned),
            GraphNode::new("f", "g", GraphNodeKind::TASK, S::Planned)
                .unwrap()
                .with_workstream_id("other")
                .unwrap(),
            GraphNode::new("u", "g", GraphNodeKind::TASK, S::Accepted).unwrap(),
        ],
        vec![],
        "1",
    );
    let none = GraphResultLedgerV1::new();
    let ready = |id: &str| {
        set(
            id,
            "PLANNED",
            "READY",
            json!({"kind":"DEPENDENCIES_SATISFIED"}),
        )
    };
    let edge = |id: &str, from: &str, to: &str| json!({"op":"ADD_EDGE","edge_id":id,"detail":{"edge":{"edge_id":id,"graph_id":"g","from_node":from,"to_node":to,"edge_class":"PRECEDENCE","precedence_kind":"REQUIRES_ACCEPTED"}}});
    let add = |id: &str, ws: Option<&str>| {
        let mut n = json!({"node_id":id,"graph_id":"g","kind":"TASK","state":"PLANNED"});
        if let Some(ws) = ws {
            n["workstream_id"] = json!(ws);
        }
        json!({"op":"ADD_NODE","node_id":id,"detail":{"node":n}})
    };
    ok(
        &env,
        &none,
        a2("w"),
        json!([ready("t"), add("n", Some("w")), edge("e", "t", "n")]),
    );
    for ops in [
        json!([ready("f")]),
        json!([add("n", Some("other"))]),
        json!([add("n", None)]),
        json!([edge("e", "t", "f")]),
        json!([edge("e", "u", "t")]),
        json!([{"op":"CANCEL_NODE","node_id":"f"}]),
    ] {
        assert!(matches!(
            op_err(&env, &none, a2("w"), ops).1,
            O::NotPermitted(_)
        ));
    }
    // Global amendments by A1/USER.
    ok(
        &env,
        &none,
        a1(),
        json!([edge("e", "t", "f"), {"op":"CANCEL_NODE","node_id":"f","detail":{}}]),
    );
    // Goal evaluator: append-only, edges into appended nodes only (S33).
    let eval = || ScopedActorV1::GoalEvaluator { role_id: None };
    ok(
        &env,
        &none,
        eval(),
        json!([add("gap", None), edge("e", "u", "gap")]),
    );
    for ops in [
        json!([ready("t")]),
        json!([edge("e", "u", "t")]),
        json!([{"op":"CANCEL_NODE","node_id":"t"}]),
    ] {
        assert!(matches!(
            op_err(&env, &none, eval(), ops).1,
            O::NotPermitted(_)
        ));
    }
}

#[test]
fn result_attachment_is_immutable_and_associated() {
    let env = envelope(
        vec![
            node("t", GraphNodeKind::TASK, S::Planned)
                .with_code_sha(SHA)
                .unwrap()
                .with_attempt_number(crate::GraphNodeAttemptNumber::try_new("1").unwrap()),
        ],
        vec![],
        "1",
    );
    let none = GraphResultLedgerV1::new();
    // A real dispatch → start history admits attempt `at1` for this node.
    let (env, admitted) = ok(
        &env,
        &none,
        a1(),
        Value::Array(run_to_review("t")[..4].to_vec()),
    );
    let good = result("t", "PASS", "rv");
    let (env2, results) = ok(
        &env,
        &admitted,
        a1(),
        json!([attach("t", "r1", good.clone())]),
    );
    // Attachment alone changes no state.
    assert_eq!(env2.graph().node_state("t"), Some(S::Running));
    assert_eq!(results.get("r1").unwrap().review_id(), Some("rv"));
    // Occupied slot refuses, even with identical content or another ref.
    for r in ["r1", "r2"] {
        assert_eq!(
            op_err(&env2, &results, a1(), json!([attach("t", r, good.clone())])).1,
            O::ResultSlotOccupied
        );
    }
    for (edit, field) in [
        (json!({"graph_id":"h"}), "graph_id"),
        (json!({"node_id":"x"}), "node_id"),
        (json!({"code_sha":"f".repeat(40)}), "code_sha"),
        (json!({"attempt_id":"other"}), "attempt_id"),
    ] {
        let mut bad = good.clone();
        for (k, v) in edit.as_object().unwrap() {
            bad[k] = v.clone();
        }
        assert_eq!(
            op_err(&env, &admitted, a1(), json!([attach("t", "r", bad)])).1,
            O::ResultAssociation(field)
        );
    }
    let mut no_attempt = good.clone();
    no_attempt.as_object_mut().unwrap().remove("attempt_id");
    assert_eq!(
        op_err(&env, &admitted, a1(), json!([attach("t", "r", no_attempt)])).1,
        O::ResultAssociation("attempt_id")
    );
    // Verdict needs a matching attached result; a missing one is unavailable.
    let (env3, r3) = ok(
        &env,
        &admitted,
        a1(),
        json!([set(
            "t",
            "RUNNING",
            "AWAITING_REVIEW",
            json!({"kind":"SUBMIT","attempt_id":"at1"})
        )]),
    );
    assert_eq!(
        op_err(&env3, &r3, a1(), json!([verdict("t", "PASSED", "t")])).1,
        O::EvidenceUnavailable("attached review result")
    );
    assert_eq!(
        op_err(
            &env3,
            &r3,
            a1(),
            json!([
                attach("t", "r", result("t", "FAIL", "rv")),
                verdict("t", "PASSED", "t")
            ])
        )
        .0,
        1
    );
}

// --- repair provenance (S32) ----------------------------------------------

fn repair_node() -> Value {
    json!({"node_id":"t-r2","graph_id":"g","kind":"REPAIR","state":"PLANNED","workstream_id":"w","parent_node_id":"t","attempt_number":2,"created_in_version":3})
}
fn review_node() -> Value {
    json!({"node_id":"r2","graph_id":"g","kind":"REVIEW","state":"PLANNED","workstream_id":"w","attempt_number":2,"created_in_version":3})
}
fn control_edge(id: &str, from: &str, to: &str) -> Value {
    json!({"edge_id":id,"graph_id":"g","from_node":from,"to_node":to,"edge_class":"CONTROL","control_kind":"ON_REJECT"})
}
fn prec_edge(id: &str, from: &str, to: &str) -> Value {
    json!({"edge_id":id,"graph_id":"g","from_node":from,"to_node":to,"edge_class":"PRECEDENCE","precedence_kind":"REQUIRES_ACCEPTED"})
}
fn expand(source: &str, nodes: Vec<Value>, edges: Vec<Value>) -> Value {
    json!({"op":"EXPAND_REPAIR","node_id":source,"detail":{"nodes":nodes,"edges":edges}})
}
fn edited(mut node: Value, edit: Value) -> Value {
    for (k, v) in edit.as_object().unwrap() {
        if v.is_null() {
            node.as_object_mut().unwrap().remove(k);
        } else {
            node[k] = v.clone();
        }
    }
    node
}

/// `t` (the implementation) precedes the retained REJECTED review `r1`;
/// `t` itself then runs and is rejected with its own attached result.
fn rejected_round() -> (ExecutionGraphEnvelope, GraphResultLedgerV1) {
    let env = envelope(
        vec![
            node("t", GraphNodeKind::TASK, S::Planned)
                .with_code_sha(SHA)
                .unwrap()
                .with_attempt_number(crate::GraphNodeAttemptNumber::try_new("1").unwrap()),
            node("r1", GraphNodeKind::REVIEW, S::Rejected)
                .with_attempt_number(crate::GraphNodeAttemptNumber::try_new("1").unwrap()),
            node("next", GraphNodeKind::TASK, S::Planned),
        ],
        vec![
            GraphEdge::precedence("e", "g", "t", "next", PrecedenceKind::RequiresAccepted).unwrap(),
            GraphEdge::precedence("pr", "g", "t", "r1", PrecedenceKind::RequiresAccepted).unwrap(),
        ],
        "1",
    );
    let mut ops = run_to_review("t");
    ops.push(attach("t", "res-1", result("t", "REJECTED", "rv1")));
    ops.push(verdict("t", "REJECTED", "t"));
    ok(
        &env,
        &GraphResultLedgerV1::new(),
        a2("w"),
        Value::Array(ops),
    )
}

#[test]
fn repair_expansion_refuses_incomplete_or_wrong_provenance() {
    let (env, results) = rejected_round();
    let (repair, review) = (repair_node(), review_node());
    let (c1, p1, p2) = (
        control_edge("c1", "r1", "t-r2"),
        prec_edge("p1", "t-r2", "r2"),
        prec_edge("p2", "r2", "next"),
    );
    let no_version = json!({"created_in_version": null});
    let other_repair = edited(repair.clone(), json!({"node_id":"t-r3","attempt_number":2}));
    let other_review = edited(review.clone(), json!({"node_id":"r3"}));
    let cases: Vec<(&str, &str, Value)> = vec![
        // A3 reproduction: no parent and no REVIEW at all.
        (
            "no parent, no review",
            "exactly one REPAIR and one REVIEW",
            expand(
                "r1",
                vec![edited(repair.clone(), json!({"parent_node_id": null}))],
                vec![c1.clone()],
            ),
        ),
        (
            "missing parent",
            "repair parent must be the reviewed subject",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"parent_node_id": null})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone(), p2.clone()],
            ),
        ),
        (
            "parent is the old review, not the reviewed implementation",
            "repair parent must be the reviewed subject",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"parent_node_id":"r1"})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone(), p2.clone()],
            ),
        ),
        (
            "parent is an unrelated node",
            "repair parent must be the reviewed subject",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"parent_node_id":"next"})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone(), p2.clone()],
            ),
        ),
        (
            "no fresh review",
            "exactly one REPAIR and one REVIEW",
            expand("r1", vec![repair.clone()], vec![c1.clone()]),
        ),
        (
            "orphan review (no repair → review precedence)",
            "one distinct fresh REVIEW",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![c1.clone(), p2.clone()],
            ),
        ),
        (
            "review precedes the repair instead of following it",
            "one distinct fresh REVIEW",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![c1.clone(), prec_edge("p1", "r2", "t-r2")],
            ),
        ),
        (
            "repair points back at the old review",
            "may not touch the rejected source",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![c1.clone(), p1.clone(), prec_edge("p3", "t-r2", "r1")],
            ),
        ),
        (
            "old review reused as the new review",
            "DuplicateNodeId",
            expand(
                "r1",
                vec![
                    repair.clone(),
                    edited(review.clone(), json!({"node_id":"r1"})),
                ],
                vec![c1.clone()],
            ),
        ),
        (
            "two repairs",
            "exactly one REPAIR and one REVIEW",
            expand(
                "r1",
                vec![repair.clone(), other_repair, review.clone()],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "two reviews",
            "exactly one REPAIR and one REVIEW",
            expand(
                "r1",
                vec![repair.clone(), review.clone(), other_review],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "rejected TASK as the source",
            "source must be a REJECTED REVIEW",
            expand(
                "t",
                vec![repair.clone(), review.clone()],
                vec![control_edge("c1", "t", "t-r2"), p1.clone()],
            ),
        ),
        (
            "no ON_REJECT edge",
            "missing ON_REJECT",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![p1.clone(), p2.clone()],
            ),
        ),
        (
            "ON_REJECT from the implementation, not the review",
            "missing ON_REJECT",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![control_edge("c1", "t", "t-r2"), p1.clone()],
            ),
        ),
        (
            "unversioned review",
            "created_in_version",
            expand(
                "r1",
                vec![repair.clone(), edited(review.clone(), no_version)],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "repair created in the wrong version",
            "created_in_version",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"created_in_version":4})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "child names another graph",
            "ChildGraphIdMismatch",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"graph_id":"h"})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "precedence cycle through the new round",
            "PrecedenceCycleRejected",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![
                    c1.clone(),
                    p1.clone(),
                    p2.clone(),
                    prec_edge("p4", "next", "t-r2"),
                ],
            ),
        ),
        (
            "repair in another workstream than the subject",
            "workstream binding",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"workstream_id":"x"})),
                    review.clone(),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "review in another workstream than the repair",
            "workstream binding",
            expand(
                "r1",
                vec![
                    repair.clone(),
                    edited(review.clone(), json!({"workstream_id":"x"})),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "repair round does not follow the subject's",
            "round ordinals",
            expand(
                "r1",
                vec![
                    edited(repair.clone(), json!({"attempt_number":1})),
                    edited(review.clone(), json!({"attempt_number":1})),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "review round differs from the repair's",
            "round ordinals",
            expand(
                "r1",
                vec![
                    repair.clone(),
                    edited(review.clone(), json!({"attempt_number":3})),
                ],
                vec![c1.clone(), p1.clone()],
            ),
        ),
        (
            "second execution subject also precedes the new review",
            "subject is ambiguous",
            expand(
                "r1",
                vec![repair.clone(), review.clone()],
                vec![c1.clone(), p1.clone(), prec_edge("p5", "t", "r2")],
            ),
        ),
    ];
    for (label, reason, op) in cases {
        let (index, error) = op_err(&env, &results, a2("w"), json!([op]));
        assert_eq!(index, 0, "{label}");
        assert!(format!("{error:?}").contains(reason), "{label}: {error:?}");
    }
    // Ambiguous or missing subject linkage on the retained review.
    for (label, reason, edges) in [
        ("no subject", "no reviewed subject", vec![]),
        (
            "two subjects",
            "subject is ambiguous",
            vec![
                GraphEdge::precedence("a", "g", "x", "rv", PrecedenceKind::RequiresAccepted)
                    .unwrap(),
                GraphEdge::precedence("b", "g", "y", "rv", PrecedenceKind::RequiresAccepted)
                    .unwrap(),
            ],
        ),
    ] {
        let env = envelope(
            vec![
                node("x", GraphNodeKind::TASK, S::Accepted),
                node("y", GraphNodeKind::IMPLEMENTATION, S::Accepted),
                node("rv", GraphNodeKind::REVIEW, S::Rejected),
            ],
            edges,
            "1",
        );
        let repair = edited(
            repair_node(),
            json!({"parent_node_id":"x","created_in_version":2}),
        );
        let review = edited(review_node(), json!({"created_in_version":2}));
        let op = expand(
            "rv",
            vec![repair, review],
            vec![
                control_edge("c1", "rv", "t-r2"),
                prec_edge("p1", "t-r2", "r2"),
            ],
        );
        let (_, error) = op_err(&env, &GraphResultLedgerV1::new(), a1(), json!([op]));
        assert!(format!("{error:?}").contains(reason), "{label}: {error:?}");
    }
    // A repair smuggled in through ADD_NODE/ADD_EDGE cannot be entered
    // without the same provenance.
    let smuggle = json!([
        {"op":"ADD_NODE","node_id":"x","detail":{"node":edited(repair_node(), json!({"node_id":"x"}))}},
        {"op":"ADD_EDGE","edge_id":"cx","detail":{"edge":control_edge("cx", "r1", "x")}},
        set("t", "REJECTED", "REPAIRING", json!({"kind":"REPAIR_ENTRY","repair_node_id":"x"})),
    ]);
    let (index, error) = op_err(&env, &results, a1(), smuggle);
    assert_eq!(index, 2);
    assert!(
        format!("{error:?}").contains("one distinct fresh REVIEW"),
        "{error:?}"
    );
    // Nothing above changed the retained rejected history.
    assert_eq!(env.graph().node_state("r1"), Some(S::Rejected));
}

/// S32: rejected REVIEW → repair expansion with ON_REJECT control edge, a
/// distinct fresh review after the repair, no precedence cycle, rejected
/// review/result retained, fresh identities — and the new round completes.
#[test]
fn repair_review_round_completes_and_retains_rejected_history() {
    let (env, results) = rejected_round();
    let ops = json!([expand(
        "r1",
        vec![repair_node(), review_node()],
        vec![
            control_edge("c1", "r1", "t-r2"),
            prec_edge("p1", "t-r2", "r2"),
            prec_edge("p2", "r2", "next"),
        ]
    )]);
    let (env, results) = ok(&env, &results, a2("w"), ops.clone());
    assert!(!env.graph().has_precedence_cycle());
    assert_eq!(
        env.graph().edge("c1").unwrap().control_kind(),
        Some(crate::ControlKind::OnReject)
    );
    assert_eq!(
        env.graph().node("t-r2").unwrap().parent_node_id(),
        Some("t")
    );
    // The same rejected review cannot be expanded a second time.
    let again = expand(
        "r1",
        vec![
            edited(
                repair_node(),
                json!({"node_id":"t-r3","created_in_version":4}),
            ),
            edited(
                review_node(),
                json!({"node_id":"r3","created_in_version":4}),
            ),
        ],
        vec![
            control_edge("c9", "r1", "t-r3"),
            prec_edge("p9", "t-r3", "r3"),
        ],
    );
    assert!(matches!(
        op_err(&env, &results, a2("w"), json!([again])).1,
        O::Repair(_)
    ));
    // The retained verdict cannot be reopened or revised.
    assert_eq!(
        op_err(
            &env,
            &results,
            a1(),
            json!([set(
                "r1",
                "REJECTED",
                "PASSED",
                json!({"kind":"REVIEW_VERDICT","result_node_id":"r1"})
            )])
        )
        .1,
        O::RetainedVerdict
    );

    // The repaired subject cannot reuse its rejected round's result.
    let mut ops = vec![set(
        "t",
        "REJECTED",
        "REPAIRING",
        json!({"kind":"REPAIR_ENTRY","repair_node_id":"t-r2"}),
    )];
    ops.extend(run_as("t-r2", "at2"));
    ops.push(attach("t-r2", "res-2", {
        let mut r = result("t-r2", "PASS", "rv2");
        r["attempt_id"] = json!("at2");
        r.as_object_mut().unwrap().remove("code_sha");
        r
    }));
    ops.push(verdict("t-r2", "PASSED", "t-r2"));
    ops.push(set(
        "t",
        "REPAIRING",
        "AWAITING_REVIEW",
        json!({"kind":"REPAIR_SUBMITTED","repair_node_id":"t-r2"}),
    ));
    let (env, results) = ok(&env, &results, a2("w"), Value::Array(ops));
    assert!(matches!(
        op_err(
            &env,
            &results,
            a2("w"),
            json!([verdict("t", "PASSED", "t")])
        )
        .1,
        O::EvidenceRefused(_)
    ));
    let (env, results) = ok(
        &env,
        &results,
        a2("w"),
        json!([verdict("t", "PASSED", "t-r2")]),
    );
    assert_eq!(env.graph().node_state("t"), Some(S::Passed));

    // The fresh review waits for the repair's acceptance, then completes its
    // own round with its own admitted attempt and verdict.
    let ready_r2 = set(
        "r2",
        "PLANNED",
        "READY",
        json!({"kind":"DEPENDENCIES_SATISFIED"}),
    );
    assert_eq!(
        op_err(&env, &results, a1(), json!([ready_r2])).1,
        O::EvidenceRefused("dependencies")
    );
    let (env, results) = ok(
        &env,
        &results,
        a1(),
        json!([set(
            "t-r2",
            "PASSED",
            "ACCEPTED",
            json!({"kind":"ACCEPTANCE","decision_id":"d3"})
        )]),
    );
    let mut ops = run_as("r2", "at3");
    ops.push(attach("r2", "res-3", {
        let mut r = result("r2", "PASS", "rv3");
        r["attempt_id"] = json!("at3");
        r
    }));
    ops.push(verdict("r2", "PASSED", "r2"));
    let (env, results) = ok(&env, &results, a2("w"), Value::Array(ops));
    assert_eq!(env.graph().node_state("r2"), Some(S::Passed));

    // History retained: rejected review, its subject's rejected result and
    // every attempt identity.
    assert_eq!(env.graph().node_state("r1"), Some(S::Rejected));
    assert_eq!(env.graph().node("t").unwrap().result_ref(), Some("res-1"));
    assert_eq!(
        results.get("res-1").unwrap().outcome(),
        crate::GraphNodeResultOutcome::Rejected
    );
    assert_eq!(results.len(), 3);
    for (node, attempt) in [("t", "at1"), ("t-r2", "at2"), ("r2", "at3")] {
        assert_eq!(results.attempt("P", "g", node), Some(attempt));
    }
}

#[test]
fn rejected_review_verdict_is_retained() {
    let env = envelope(
        vec![
            node("rv", GraphNodeKind::REVIEW, S::Rejected),
            node("p", GraphNodeKind::REVIEW, S::Passed),
        ],
        vec![],
        "1",
    );
    let none = GraphResultLedgerV1::new();
    for op in [
        set(
            "rv",
            "REJECTED",
            "REPAIRING",
            json!({"kind":"REPAIR_ENTRY","repair_node_id":"x"}),
        ),
        set(
            "rv",
            "REJECTED",
            "BLOCKED",
            json!({"kind":"BLOCKED","reason":"r"}),
        ),
        json!({"op":"CANCEL_NODE","node_id":"rv"}),
        json!({"op":"CANCEL_NODE","node_id":"p"}),
    ] {
        assert_eq!(op_err(&env, &none, a1(), json!([op])).1, O::RetainedVerdict);
    }
    ok(
        &env,
        &none,
        a1(),
        json!([set(
            "p",
            "PASSED",
            "ACCEPTED",
            json!({"kind":"ACCEPTANCE","decision_id":"d"})
        )]),
    );
}

#[test]
fn blocked_human_locked_and_cancel_controls() {
    let env = envelope(
        vec![
            node("t", GraphNodeKind::TASK, S::Running).with_result_ref("kept"),
            node("u", GraphNodeKind::TASK, S::Running),
            node("h", GraphNodeKind::HUMAN_GATE, S::HumanRequired),
            node("l", GraphNodeKind::REVIEW, S::LockedRequiresPro).with_required_capabilities(
                vec![crate::CapabilityName::new("review.independent_a4").unwrap()],
            ),
            node("r", GraphNodeKind::TASK, S::Ready).with_required_capabilities(vec![
                crate::CapabilityName::new("orchestration.multi_runtime").unwrap(),
            ]),
            node("a", GraphNodeKind::TASK, S::Accepted),
        ],
        vec![],
        "1",
    );
    let none = GraphResultLedgerV1::new();
    // Block with a reason, then resume only at READY by explicit resolution.
    let (b, r) = ok(
        &env,
        &none,
        a2("w"),
        json!([set(
            "t",
            "RUNNING",
            "BLOCKED",
            json!({"kind":"BLOCKED","reason":"budget"})
        )]),
    );
    assert!(matches!(
        op_err(
            &b,
            &r,
            a2("w"),
            json!([set(
                "t",
                "BLOCKED",
                "RUNNING",
                json!({"kind":"RESOLVED","resolution_ref":"x"})
            )])
        )
        .1,
        O::Transition(_)
    ));
    // A node holding an attached result cannot resume and reuse it.
    assert!(matches!(
        op_err(
            &b,
            &r,
            a2("w"),
            json!([set(
                "t",
                "BLOCKED",
                "READY",
                json!({"kind":"RESOLVED","resolution_ref":"x"})
            )])
        )
        .1,
        O::EvidenceRefused(_)
    ));
    let resume = |id: &str| {
        json!([
            set(
                id,
                "RUNNING",
                "BLOCKED",
                json!({"kind":"BLOCKED","reason":"budget"})
            ),
            set(
                id,
                "BLOCKED",
                "READY",
                json!({"kind":"RESOLVED","resolution_ref":"x"})
            )
        ])
    };
    let (resumed, _) = ok(&env, &none, a2("w"), resume("u"));
    assert_eq!(resumed.graph().node_state("u"), Some(S::Ready));
    // A human gate cannot be side-stepped through BLOCKED.
    assert!(matches!(
        op_err(
            &env,
            &none,
            a2("w"),
            json!([set(
                "h",
                "HUMAN_REQUIRED",
                "BLOCKED",
                json!({"kind":"BLOCKED","reason":"x"})
            )])
        )
        .1,
        O::Transition(_)
    ));
    // Human gate: only USER resolves; it is never readied by dependencies.
    let human = set(
        "h",
        "HUMAN_REQUIRED",
        "READY",
        json!({"kind":"HUMAN_RESOLVED","instruction_ref":"i"}),
    );
    assert!(matches!(
        op_err(&env, &none, a1(), json!([human.clone()])).1,
        O::NotPermitted(_)
    ));
    ok(
        &env,
        &none,
        ScopedActorV1::User { role_id: None },
        json!([human]),
    );
    // Locked: no Wave1 capability-admission source => unavailable.
    assert_eq!(
        op_err(
            &env,
            &none,
            a1(),
            json!([set(
                "l",
                "LOCKED_REQUIRES_PRO",
                "READY",
                json!({"kind":"CAPABILITY_ADMISSION"})
            )])
        )
        .1,
        O::EvidenceUnavailable("verified capability admission (no Wave1 source)")
    );
    assert!(matches!(
        op_err(
            &env,
            &none,
            a1(),
            json!([set(
                "l",
                "LOCKED_REQUIRES_PRO",
                "ADMITTED",
                json!({"kind":"ADMISSION","admission_decision_id":"x"})
            )])
        )
        .1,
        O::Transition(_)
    ));
    // Locking needs a capability the node actually requires.
    ok(
        &env,
        &none,
        a1(),
        json!([set(
            "r",
            "READY",
            "LOCKED_REQUIRES_PRO",
            json!({"kind":"FEATURE_LOCKED","capability_id":"orchestration.multi_runtime"})
        )]),
    );
    assert!(matches!(
        op_err(
            &env,
            &none,
            a1(),
            json!([set(
                "r",
                "READY",
                "LOCKED_REQUIRES_PRO",
                json!({"kind":"FEATURE_LOCKED","capability_id":"graph.core"})
            )])
        )
        .1,
        O::EvidenceRefused(_)
    ));
    // Cancel preserves results; terminal nodes are not reopened or cancelled.
    let (c, _) = ok(
        &env,
        &none,
        a1(),
        json!([{"op":"CANCEL_NODE","node_id":"t"}, {"op":"CANCEL_NODE","node_id":"l"}]),
    );
    assert_eq!(c.graph().node("t").unwrap().result_ref(), Some("kept"));
    assert_eq!(c.graph().node_count(), env.graph().node_count());
    for op in [
        json!({"op":"CANCEL_NODE","node_id":"a"}),
        set(
            "a",
            "ACCEPTED",
            "PLANNED",
            json!({"kind":"RESOLVED","resolution_ref":"x"}),
        ),
    ] {
        assert!(matches!(
            op_err(&env, &none, a1(), json!([op])).1,
            O::Transition(_)
        ));
    }
    assert!(matches!(
        op_err(&c, &none, a1(), json!([{"op":"CANCEL_NODE","node_id":"t"}])).1,
        O::Transition(_)
    ));
}

#[test]
fn apply_is_deterministic_across_runs() {
    let env = task_env();
    let none = GraphResultLedgerV1::new();
    let ops = Value::Array(run_to_review("t"));
    let first = apply(&env, &none, a1(), ops.clone()).unwrap();
    for _ in 0..3 {
        assert_eq!(apply(&env, &none, a1(), ops.clone()).unwrap(), first);
    }
}

// --- attempt identity binding ------------------------------------------------

const NOT_ADMITTED: O = O::Attempt("attempt id not admitted");
const NO_ATTEMPT: O = O::Attempt("node has no admitted attempt");
const REUSED: O = O::Attempt("attempt id already admitted");

fn dispatch(id: &str, attempt: &str) -> Value {
    set(
        id,
        "ADMITTED",
        "DISPATCHED",
        json!({"kind":"DISPATCH","attempt_id":attempt}),
    )
}
fn start(id: &str, attempt: &str) -> Value {
    set(
        id,
        "DISPATCHED",
        "RUNNING",
        json!({"kind":"START","attempt_id":attempt}),
    )
}
fn submit(id: &str, attempt: &str) -> Value {
    set(
        id,
        "RUNNING",
        "AWAITING_REVIEW",
        json!({"kind":"SUBMIT","attempt_id":attempt}),
    )
}
fn with_attempt(mut result: Value, attempt: &str) -> Value {
    result["attempt_id"] = json!(attempt);
    result
}

/// The A3 reproduction: dispatch at1, start/submit/result under other ids.
#[test]
fn different_attempt_ids_refuse_within_one_batch() {
    let env = task_env();
    let none = GraphResultLedgerV1::new();
    for (slot, error) in [(3, NOT_ADMITTED), (4, NOT_ADMITTED)] {
        let mut ops = run_to_review("t");
        ops[slot]["detail"]["cause"]["attempt_id"] = json!("different");
        assert_eq!(
            op_err(&env, &none, a1(), Value::Array(ops)),
            (slot, error),
            "slot {slot}"
        );
    }
    let mut ops = run_to_review("t");
    ops.push(attach(
        "t",
        "r",
        with_attempt(result("t", "PASS", "rv"), "unrelated-result"),
    ));
    assert_eq!(
        op_err(&env, &none, a1(), Value::Array(ops.clone())),
        (5, O::ResultAssociation("attempt_id"))
    );
    // The same batch with the admitted identity throughout is valid.
    ops[5] = attach("t", "r", result("t", "PASS", "rv"));
    let (env, results) = ok(&env, &none, a1(), Value::Array(ops));
    assert_eq!(env.graph().node("t").unwrap().result_ref(), Some("r"));
    assert_eq!(results.attempt("P", "g", "t"), Some("at1"));
}

#[test]
fn attempt_identity_persists_across_separate_calls_and_failed_batches() {
    let env = task_env();
    let ops = run_to_review("t");
    let (env, ledger) = ok(
        &env,
        &GraphResultLedgerV1::new(),
        a1(),
        Value::Array(ops[..3].to_vec()),
    );
    assert_eq!(ledger.attempt("P", "g", "t"), Some("at1"));
    assert_eq!(
        op_err(&env, &ledger, a1(), json!([start("t", "x")])),
        (0, NOT_ADMITTED)
    );
    // A failed batch records nothing: its good START is not kept.
    assert_eq!(
        op_err(
            &env,
            &ledger,
            a1(),
            json!([start("t", "at1"), submit("t", "x")])
        ),
        (1, NOT_ADMITTED)
    );
    let (env, ledger) = ok(&env, &ledger, a1(), json!([start("t", "at1")]));
    assert_eq!(
        op_err(&env, &ledger, a1(), json!([submit("t", "x")])),
        (0, NOT_ADMITTED)
    );
    let (env, ledger) = ok(&env, &ledger, a1(), json!([submit("t", "at1")]));
    for bad in ["unrelated-result", "x"] {
        let wrong = with_attempt(result("t", "PASS", "rv"), bad);
        assert_eq!(
            op_err(&env, &ledger, a1(), json!([attach("t", "r", wrong)])),
            (0, O::ResultAssociation("attempt_id"))
        );
    }
    let mut missing = result("t", "PASS", "rv");
    missing.as_object_mut().unwrap().remove("attempt_id");
    assert_eq!(
        op_err(&env, &ledger, a1(), json!([attach("t", "r", missing)])),
        (0, O::ResultAssociation("attempt_id"))
    );
    let (env, ledger) = ok(
        &env,
        &ledger,
        a1(),
        json!([attach("t", "r", result("t", "PASS", "rv"))]),
    );
    assert_eq!(env.graph().node("t").unwrap().result_ref(), Some("r"));
    assert_eq!(ledger.len(), 1);
}

#[test]
fn execution_without_an_admitted_dispatch_refuses() {
    let none = GraphResultLedgerV1::new();
    // Nodes already mid-lifecycle, but this ledger never saw their dispatch:
    // a result that merely claims its own attempt identity proves nothing.
    let env = envelope(
        vec![
            node("d", GraphNodeKind::TASK, S::Dispatched),
            node("r", GraphNodeKind::TASK, S::Running)
                .with_code_sha(SHA)
                .unwrap(),
            node("o", GraphNodeKind::TASK, S::Admitted),
        ],
        vec![],
        "1",
    );
    assert_eq!(
        op_err(&env, &none, a1(), json!([start("d", "at1")])),
        (0, NO_ATTEMPT)
    );
    assert_eq!(
        op_err(&env, &none, a1(), json!([submit("r", "at1")])),
        (0, NO_ATTEMPT)
    );
    assert_eq!(
        op_err(
            &env,
            &none,
            a1(),
            json!([attach("r", "x", result("r", "PASS", "rv"))])
        ),
        (0, NO_ATTEMPT)
    );
    // Another node's dispatch is no binding for this node.
    let (_, other) = ok(&env, &none, a1(), json!([dispatch("o", "at1")]));
    assert_eq!(
        op_err(&env, &other, a1(), json!([start("d", "at1")])),
        (0, NO_ATTEMPT)
    );
    assert_eq!(
        op_err(
            &env,
            &other,
            a1(),
            json!([attach("r", "x", result("r", "PASS", "rv"))])
        ),
        (0, NO_ATTEMPT)
    );
}

#[test]
fn attempt_ids_are_never_reused_across_nodes_or_rounds() {
    let env = envelope(
        vec![
            node("t", GraphNodeKind::TASK, S::Admitted),
            node("u", GraphNodeKind::TASK, S::Admitted),
        ],
        vec![],
        "1",
    );
    let none = GraphResultLedgerV1::new();
    let base = env.clone();
    let (env, ledger) = ok(&env, &none, a1(), json!([dispatch("t", "at1")]));
    // Cross-node reuse, in a later call and inside one batch.
    assert_eq!(
        op_err(&env, &ledger, a1(), json!([dispatch("u", "at1")])),
        (0, REUSED)
    );
    assert_eq!(
        op_err(
            &base,
            &none,
            a1(),
            json!([dispatch("t", "at1"), dispatch("u", "at1")])
        ),
        (1, REUSED)
    );
    let (env, ledger) = ok(&env, &ledger, a1(), json!([dispatch("u", "at2")]));
    // u's start cannot use t's attempt; each result needs its own node's.
    assert_eq!(
        op_err(&env, &ledger, a1(), json!([start("u", "at1")])),
        (0, NOT_ADMITTED)
    );
    let (env, ledger) = ok(
        &env,
        &ledger,
        a1(),
        json!([start("t", "at1"), start("u", "at2")]),
    );
    assert_eq!(
        op_err(
            &env,
            &ledger,
            a1(),
            json!([attach(
                "u",
                "x",
                with_attempt(result("u", "PASS", "rv"), "at1")
            )])
        ),
        (0, O::ResultAssociation("attempt_id"))
    );
    // A fresh round of the same node needs a fresh attempt: the old execution
    // can neither be re-dispatched nor started/submitted again.
    let block = set(
        "t",
        "RUNNING",
        "BLOCKED",
        json!({"kind":"BLOCKED","reason":"r"}),
    );
    let resume = [
        set(
            "t",
            "BLOCKED",
            "READY",
            json!({"kind":"RESOLVED","resolution_ref":"x"}),
        ),
        set(
            "t",
            "READY",
            "ADMITTED",
            json!({"kind":"ADMISSION","admission_decision_id":"ad"}),
        ),
    ];
    let mut ops = vec![block];
    ops.extend(resume);
    let (env, ledger) = ok(&env, &ledger, a1(), Value::Array(ops));
    assert_eq!(
        op_err(&env, &ledger, a1(), json!([dispatch("t", "at1")])),
        (0, REUSED)
    );
    let (env, ledger) = ok(&env, &ledger, a1(), json!([dispatch("t", "at3")]));
    assert_eq!(
        op_err(&env, &ledger, a1(), json!([start("t", "at1")])),
        (0, NOT_ADMITTED)
    );
    let (env, ledger) = ok(&env, &ledger, a1(), json!([start("t", "at3")]));
    assert_eq!(
        op_err(
            &env,
            &ledger,
            a1(),
            json!([attach(
                "t",
                "x",
                with_attempt(result("t", "PASS", "rv"), "at1")
            )])
        ),
        (0, O::ResultAssociation("attempt_id"))
    );
    assert_eq!(ledger.attempt("P", "g", "t"), Some("at3"));
    assert_eq!(ledger.attempt("P", "g", "u"), Some("at2"));
    assert_eq!(env.graph().node_state("t"), Some(S::Running));
}

#[test]
fn attempt_binding_does_not_cross_project_or_graph() {
    let nodes = |graph: &str, state| {
        vec![
            GraphNode::new("t", graph, GraphNodeKind::TASK, state).unwrap(),
            GraphNode::new("u", graph, GraphNodeKind::TASK, S::Admitted).unwrap(),
        ]
    };
    let home = envelope_in("P", "g", nodes("g", S::Admitted), vec![], "1");
    let (_, ledger) = ok(
        &home,
        &GraphResultLedgerV1::new(),
        a1(),
        json!([dispatch("t", "at1")]),
    );
    for (project, graph) in [("P2", "g"), ("P", "h")] {
        let other = envelope_in(project, graph, nodes(graph, S::Dispatched), vec![], "1");
        assert_eq!(
            op_err(&other, &ledger, a1(), json!([start("t", "at1")])),
            (0, NO_ATTEMPT),
            "{project}/{graph}"
        );
        // The foreign identity cannot be admitted again either.
        assert_eq!(
            op_err(&other, &ledger, a1(), json!([dispatch("u", "at1")])),
            (0, REUSED),
            "{project}/{graph}"
        );
    }
}

// --- dependency recheck on resumption, admission and dispatch ----------------

fn dep_env(pred: S, node_state: S, kind: GraphNodeKind) -> ExecutionGraphEnvelope {
    envelope(
        vec![
            node("p", GraphNodeKind::TASK, pred),
            node("t", kind, node_state),
            node("q", GraphNodeKind::TASK, S::Planned),
        ],
        vec![
            GraphEdge::precedence("dep", "g", "p", "t", PrecedenceKind::RequiresAccepted).unwrap(),
        ],
        "1",
    )
}

fn resume_and_dispatch(from: &str, cause: Value) -> Value {
    json!([
        set("t", from, "READY", cause),
        set(
            "t",
            "READY",
            "ADMITTED",
            json!({"kind":"ADMISSION","admission_decision_id":"ad"})
        ),
        dispatch("t", "at"),
    ])
}

fn add_dependency(from: &str) -> Value {
    json!({"op":"ADD_EDGE","edge_id":"late","detail":{"edge":prec_edge("late", from, "t")}})
}

const UNMET: O = O::EvidenceRefused("dependencies");

/// The A3 reproduction: BLOCKED → READY → ADMITTED → DISPATCHED while the
/// REQUIRES_ACCEPTED predecessor is still PLANNED.
#[test]
fn blocked_task_cannot_resume_or_dispatch_with_unmet_prerequisites() {
    let none = GraphResultLedgerV1::new();
    let resolved = json!({"kind":"RESOLVED","resolution_ref":"resolution"});
    for pred in [S::Planned, S::Running, S::Passed, S::Cancelled] {
        let env = dep_env(pred, S::Blocked, GraphNodeKind::TASK);
        assert_eq!(
            op_err(
                &env,
                &none,
                a1(),
                resume_and_dispatch("BLOCKED", resolved.clone())
            ),
            (0, UNMET),
            "{pred:?}"
        );
    }
    for pred in [S::Accepted, S::Integrated] {
        let env = dep_env(pred, S::Blocked, GraphNodeKind::TASK);
        let (env, ledger) = ok(
            &env,
            &none,
            a1(),
            resume_and_dispatch("BLOCKED", resolved.clone()),
        );
        assert_eq!(env.graph().node_state("t"), Some(S::Dispatched));
        assert_eq!(ledger.attempt("P", "g", "t"), Some("at"));
    }
}

#[test]
fn human_resolution_rechecks_dependencies_and_keeps_actor_gates() {
    let none = GraphResultLedgerV1::new();
    let human = json!({"kind":"HUMAN_RESOLVED","instruction_ref":"i"});
    let user = || ScopedActorV1::User { role_id: None };
    for kind in [GraphNodeKind::TASK, GraphNodeKind::HUMAN_GATE] {
        let unmet = dep_env(S::Planned, S::HumanRequired, kind.clone());
        assert_eq!(
            op_err(
                &unmet,
                &none,
                user(),
                resume_and_dispatch("HUMAN_REQUIRED", human.clone())
            ),
            (0, UNMET)
        );
        // Only USER resolves a human gate, whatever the dependencies say.
        assert!(matches!(
            op_err(
                &unmet,
                &none,
                a1(),
                resume_and_dispatch("HUMAN_REQUIRED", human.clone())
            )
            .1,
            O::NotPermitted(_)
        ));
        let met = dep_env(S::Accepted, S::HumanRequired, kind);
        ok(
            &met,
            &none,
            user(),
            json!([set("t", "HUMAN_REQUIRED", "READY", human.clone())]),
        );
        assert!(matches!(
            op_err(
                &met,
                &none,
                a1(),
                json!([set("t", "HUMAN_REQUIRED", "READY", human.clone())])
            )
            .1,
            O::NotPermitted(_)
        ));
    }
    // A satisfied locked node still has no capability-admission source.
    let met = dep_env(S::Accepted, S::LockedRequiresPro, GraphNodeKind::TASK);
    assert_eq!(
        op_err(
            &met,
            &none,
            a1(),
            json!([set(
                "t",
                "LOCKED_REQUIRES_PRO",
                "READY",
                json!({"kind":"CAPABILITY_ADMISSION"})
            )])
        )
        .1,
        O::EvidenceUnavailable("verified capability admission (no Wave1 source)")
    );
}

#[test]
fn admission_and_dispatch_recheck_dependencies_changed_between_steps() {
    let none = GraphResultLedgerV1::new();
    let admit = set(
        "t",
        "READY",
        "ADMITTED",
        json!({"kind":"ADMISSION","admission_decision_id":"ad"}),
    );
    // A prerequisite added in the same batch, then in a later call.
    let ready = dep_env(S::Accepted, S::Ready, GraphNodeKind::TASK);
    assert_eq!(
        op_err(
            &ready,
            &none,
            a1(),
            json!([add_dependency("q"), admit.clone()])
        ),
        (1, UNMET)
    );
    let (with_dep, ledger) = ok(&ready, &none, a1(), json!([add_dependency("q")]));
    assert_eq!(
        op_err(&with_dep, &ledger, a1(), json!([admit.clone()])),
        (0, UNMET)
    );
    // Admitted first, then a prerequisite appears before dispatch.
    let (admitted, ledger) = ok(&ready, &none, a1(), json!([admit]));
    assert_eq!(admitted.graph().node_state("t"), Some(S::Admitted));
    let (late, ledger) = ok(&admitted, &ledger, a1(), json!([add_dependency("q")]));
    assert_eq!(
        op_err(&late, &ledger, a1(), json!([dispatch("t", "at")])),
        (0, UNMET)
    );
    assert_eq!(
        op_err(
            &admitted,
            &ledger,
            a1(),
            json!([add_dependency("q"), dispatch("t", "at")])
        ),
        (1, UNMET)
    );
    // The same step is valid once the new prerequisite is itself accepted.
    let ok_env = envelope(
        vec![
            node("p", GraphNodeKind::TASK, S::Accepted),
            node("q", GraphNodeKind::TASK, S::Accepted),
            node("t", GraphNodeKind::TASK, S::Admitted),
        ],
        vec![
            GraphEdge::precedence("dep", "g", "p", "t", PrecedenceKind::RequiresAccepted).unwrap(),
            GraphEdge::precedence("late", "g", "q", "t", PrecedenceKind::RequiresAccepted).unwrap(),
        ],
        "1",
    );
    let (env, _) = ok(&ok_env, &none, a1(), json!([dispatch("t", "at")]));
    assert_eq!(env.graph().node_state("t"), Some(S::Dispatched));
    // Ordinary readiness is unchanged.
    let planned = dep_env(S::Planned, S::Planned, GraphNodeKind::TASK);
    assert_eq!(
        op_err(
            &planned,
            &none,
            a1(),
            json!([set(
                "t",
                "PLANNED",
                "READY",
                json!({"kind":"DEPENDENCIES_SATISFIED"})
            )])
        ),
        (0, UNMET)
    );
}

// --- REQUIRES_INTERFACE at resumption, admission and dispatch ----------------

struct MemSource {
    record: Option<InterfaceArtifactRecord>,
    content: Option<Vec<u8>>,
    offline: bool,
}

impl InterfaceArtifactSource for MemSource {
    fn interface_record(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<InterfaceArtifactRecord>, InterfaceSourceUnavailable> {
        if self.offline {
            return Err(InterfaceSourceUnavailable);
        }
        Ok(self.record.clone())
    }
    fn artifact_content(&self, _: &str) -> Result<Option<Vec<u8>>, InterfaceSourceUnavailable> {
        if self.offline {
            return Err(InterfaceSourceUnavailable);
        }
        Ok(self.content.clone())
    }
}

const IFACE: &[u8] = b"pub trait AuthApi { fn login(&self); }\n";

fn iface_source(version: &str) -> MemSource {
    use sha2::{Digest, Sha256};
    MemSource {
        record: Some(InterfaceArtifactRecord {
            graph_id: "g".into(),
            graph_version: version.into(),
            edge_id: "ei".into(),
            source_node_id: "a".into(),
            consumer_node_id: "t".into(),
            artifact_id: "iface-1".into(),
            code_sha: SHA.into(),
            content_sha256: Sha256::digest(IFACE)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            provenance: InterfaceProvenance::Accepted {
                acceptance_id: "acc-7".into(),
            },
        }),
        content: Some(IFACE.to_vec()),
        offline: false,
    }
}

fn iface_env(node_state: S, version: &str) -> ExecutionGraphEnvelope {
    envelope(
        vec![
            node("a", GraphNodeKind::TASK, S::Running)
                .with_code_sha(SHA)
                .unwrap(),
            node("t", GraphNodeKind::TASK, node_state),
        ],
        vec![
            GraphEdge::precedence("ei", "g", "a", "t", PrecedenceKind::RequiresInterface).unwrap(),
        ],
        version,
    )
}

#[test]
fn interface_evidence_is_rechecked_at_resumption_admission_and_dispatch() {
    let none = GraphResultLedgerV1::new();
    fn verifier(s: &MemSource) -> Option<InterfaceEvidenceVerifier<'_>> {
        Some(InterfaceEvidenceVerifier::new(s))
    }
    let admit = set(
        "t",
        "READY",
        "ADMITTED",
        json!({"kind":"ADMISSION","admission_decision_id":"ad"}),
    );
    let resume = set(
        "t",
        "BLOCKED",
        "READY",
        json!({"kind":"RESOLVED","resolution_ref":"x"}),
    );
    let run = |env: &ExecutionGraphEnvelope, ledger, op: &Value, src: Option<&MemSource>| {
        apply_with(env, ledger, a1(), json!([op]), src.and_then(verifier))
    };
    let refusal = |r: Result<GraphMutationOutcomeV1, GraphMutationApplyError>| match r {
        Err(GraphMutationApplyError::Operation { index: 0, error }) => error,
        other => panic!("expected refusal, got {other:?}"),
    };
    let unavailable = O::EvidenceUnavailable("interface artifact");

    let blocked = iface_env(S::Blocked, "4");
    let ready = iface_env(S::Ready, "4");
    let good = iface_source("4");
    // No source, offline source and missing record are unavailable.
    for op_env in [(&resume, &blocked), (&admit, &ready)] {
        assert_eq!(refusal(run(op_env.1, &none, op_env.0, None)), unavailable);
        let offline = MemSource {
            offline: true,
            ..iface_source("4")
        };
        assert_eq!(
            refusal(run(op_env.1, &none, op_env.0, Some(&offline))),
            unavailable
        );
        let missing = MemSource {
            record: None,
            ..iface_source("4")
        };
        assert_eq!(
            refusal(run(op_env.1, &none, op_env.0, Some(&missing))),
            unavailable
        );
        // Contradicted evidence is refused, not merely unavailable.
        let mut wrong_consumer = iface_source("4");
        wrong_consumer.record.as_mut().unwrap().consumer_node_id = "other".into();
        let mut unaccepted = iface_source("4");
        unaccepted.record.as_mut().unwrap().provenance = InterfaceProvenance::NotAccepted;
        let tampered = MemSource {
            content: Some(b"tampered".to_vec()),
            ..iface_source("4")
        };
        for bad in [&wrong_consumer, &unaccepted, &tampered] {
            assert_eq!(refusal(run(op_env.1, &none, op_env.0, Some(bad))), UNMET);
        }
        run(op_env.1, &none, op_env.0, Some(&good)).unwrap();
    }
    // Admitted at version 4, evidence bound to 4. At dispatch (version 5) the
    // old record is stale; the source must supply the current version.
    let (admitted, ledger) = apply_with(&ready, &none, a1(), json!([admit]), verifier(&good))
        .map(|o| (o.envelope, o.results))
        .unwrap();
    let go = dispatch("t", "at");
    assert_eq!(refusal(run(&admitted, &ledger, &go, Some(&good))), UNMET);
    assert_eq!(refusal(run(&admitted, &ledger, &go, None)), unavailable);
    let fresh = iface_source("5");
    let out = run(&admitted, &ledger, &go, Some(&fresh)).unwrap();
    assert_eq!(out.envelope.graph().node_state("t"), Some(S::Dispatched));
}

/// The frozen `03_repair_expansion` graph is the reference provenance shape:
/// `i2.parent = i1` (the work `r1` reviewed), `r1 ON_REJECT i2`, `i2 → r2`.
#[test]
fn frozen_repair_fixture_shape_is_accepted_provenance() {
    let text = std::fs::read_to_string(format!(
        "{}/../../fixtures/graphs/03_repair_expansion.json",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let rejected = |text: &str| {
        let from = crate::wire::ExecutionGraphWireV1::from_json(text)
            .unwrap()
            .to_envelope()
            .unwrap();
        let nodes = from
            .graph()
            .nodes()
            .map(|n| match n.node_id() {
                "i1" | "r1" => n.clone().with_state(S::Rejected),
                _ => n.clone(),
            })
            .collect();
        let edges = from.graph().edges().cloned().collect();
        envelope_in(from.project_id(), from.graph_id(), nodes, edges, "2")
    };
    let enter = json!([set(
        "i1",
        "REJECTED",
        "REPAIRING",
        json!({"kind":"REPAIR_ENTRY","repair_node_id":"i2"})
    )]);
    let none = GraphResultLedgerV1::new();
    let (env, _) = ok(&rejected(&text), &none, a1(), enter.clone());
    assert_eq!(env.graph().node_state("i1"), Some(S::Repairing));
    assert_eq!(env.graph().node_state("r1"), Some(S::Rejected));
    // The repair's parent is the reviewed implementation, never the review.
    let wrong_parent = text.replace("\"parent_node_id\": \"i1\"", "\"parent_node_id\": \"r1\"");
    assert_ne!(wrong_parent, text);
    assert!(matches!(
        op_err(&rejected(&wrong_parent), &none, a1(), enter.clone()).1,
        O::EvidenceRefused("repair parent mismatch")
    ));
    // The fixture's fresh review is what makes the round complete.
    let no_review = text.replace("\"from_node\": \"i2\"", "\"from_node\": \"t\"");
    assert_ne!(no_review, text);
    assert!(matches!(
        op_err(&rejected(&no_review), &none, a1(), enter).1,
        O::Repair(_)
    ));
}
