//! Apply-v1 through the public interpreter: lifecycle, atomic refusal,
//! scope, results, repair expansion and cancellation.

use serde_json::{Value, json};

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
    let graph = ExecutionGraph::from_parts("g", nodes, edges).unwrap();
    ExecutionGraphEnvelope::bind(
        &graph,
        ExecutionGraphEnvelopeFields::new(
            "P",
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
        "mutation_id": "m", "graph_id": "g", "parent_version": parent,
        "resulting_version": parent + 1, "actor": role(actor), "reason": "test",
        "created_at": "2026-10-29T01:02:03Z", "operations": ops,
    });
    GraphMutationWireV1::from_json(&text.to_string())
        .unwrap()
        .to_mutation()
        .unwrap()
}

fn scope(env: &ExecutionGraphEnvelope, actor: ScopedActorV1) -> GraphMutationScopeV1 {
    GraphMutationScopeV1::new("P", "g", env.graph_version().clone(), actor)
}

fn apply(
    env: &ExecutionGraphEnvelope,
    results: &GraphResultLedgerV1,
    actor: ScopedActorV1,
    ops: Value,
) -> Result<GraphMutationOutcomeV1, GraphMutationApplyError> {
    let before = (env.clone(), results.clone());
    let m = mutation(env, &actor, ops);
    let out = apply_graph_mutation_v1(env, results, &m, &scope(env, actor), None);
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
            json!({"kind":"DISPATCH","attempt_id":"at1"}),
        ),
        set(
            id,
            "DISPATCHED",
            "RUNNING",
            json!({"kind":"START","attempt_id":"at1"}),
        ),
        set(
            id,
            "RUNNING",
            "AWAITING_REVIEW",
            json!({"kind":"SUBMIT","attempt_id":"at1"}),
        ),
    ]
}

fn result(id: &str, outcome: &str, review: &str) -> Value {
    json!({"node_id":id,"graph_id":"g","outcome":outcome,"produced_at":"2026-10-29T00:00:00Z",
           "review_id":review,"code_sha":SHA,"checks":[{"command":["cargo","test"],"exit_code":0}]})
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
            node("t", GraphNodeKind::TASK, S::Running)
                .with_code_sha(SHA)
                .unwrap()
                .with_attempt_number(crate::GraphNodeAttemptNumber::try_new("1").unwrap()),
        ],
        vec![],
        "1",
    );
    let none = GraphResultLedgerV1::new();
    let mut good = result("t", "PASS", "rv");
    good["attempt_id"] = json!("at1");
    let (env2, results) = ok(&env, &none, a1(), json!([attach("t", "r1", good.clone())]));
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
    ] {
        let mut bad = good.clone();
        for (k, v) in edit.as_object().unwrap() {
            bad[k] = v.clone();
        }
        assert_eq!(
            op_err(&env, &none, a1(), json!([attach("t", "r", bad)])).1,
            O::ResultAssociation(field)
        );
    }
    let mut no_attempt = good.clone();
    no_attempt.as_object_mut().unwrap().remove("attempt_id");
    assert_eq!(
        op_err(&env, &none, a1(), json!([attach("t", "r", no_attempt)])).1,
        O::ResultAssociation("attempt_id")
    );
    // Verdict needs a matching attached result; a missing one is unavailable.
    let (env3, r3) = ok(
        &env,
        &none,
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
                attach("t", "r", {
                    let mut b = result("t", "FAIL", "rv");
                    b["attempt_id"] = json!("a");
                    b
                }),
                verdict("t", "PASSED", "t")
            ])
        )
        .0,
        1
    );
}

/// S32: rejected review → repair expansion with ON_REJECT control edge,
/// no precedence cycle, rejected source/result retained, fresh identities.
#[test]
fn repair_expansion_cycle_retains_rejected_history() {
    let env = task_env();
    let none = GraphResultLedgerV1::new();
    let mut ops = run_to_review("t");
    ops.push(attach("t", "res-1", result("t", "REJECTED", "rv1")));
    ops.push(verdict("t", "REJECTED", "t"));
    let (env, results) = ok(&env, &none, a2("w"), Value::Array(ops));

    let repair = json!({"node_id":"t-r2","graph_id":"g","kind":"REPAIR","state":"PLANNED","workstream_id":"w","parent_node_id":"t","attempt_number":2,"created_in_version":3});
    let control = json!({"edge_id":"c1","graph_id":"g","from_node":"t","to_node":"t-r2","edge_class":"CONTROL","control_kind":"ON_REJECT"});
    let forward = json!({"edge_id":"p1","graph_id":"g","from_node":"t-r2","to_node":"next","edge_class":"PRECEDENCE","precedence_kind":"REQUIRES_ACCEPTED"});
    let expand = |nodes: Value, edges: Value| json!({"op":"EXPAND_REPAIR","node_id":"t","detail":{"nodes":nodes,"edges":edges}});
    // Refusals: missing ON_REJECT, precedence into the source, missing version, back-loop.
    let back = json!({"edge_id":"p2","graph_id":"g","from_node":"next","to_node":"t-r2","edge_class":"PRECEDENCE","precedence_kind":"REQUIRES_ACCEPTED"});
    let into_source = json!({"edge_id":"p3","graph_id":"g","from_node":"t-r2","to_node":"t","edge_class":"PRECEDENCE","precedence_kind":"REQUIRES_ACCEPTED"});
    let mut unversioned = repair.clone();
    unversioned
        .as_object_mut()
        .unwrap()
        .remove("created_in_version");
    for (nodes, edges) in [
        (json!([repair]), json!([forward])),
        (json!([repair]), json!([control, into_source])),
        (json!([unversioned]), json!([control])),
        (json!([repair]), json!([control, forward, back])),
    ] {
        op_err(&env, &results, a2("w"), json!([expand(nodes, edges)]));
    }
    let (env, results) = ok(
        &env,
        &results,
        a2("w"),
        json!([expand(json!([repair]), json!([control, forward]))]),
    );
    assert!(!env.graph().has_precedence_cycle());
    assert_eq!(
        env.graph().edge("c1").unwrap().control_kind(),
        Some(crate::ControlKind::OnReject)
    );

    // The repaired subject cannot reuse its rejected round's result.
    let mut ops = vec![set(
        "t",
        "REJECTED",
        "REPAIRING",
        json!({"kind":"REPAIR_ENTRY","repair_node_id":"t-r2"}),
    )];
    ops.extend(run_to_review("t-r2"));
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
    // History retained: original rejected result and its slot unchanged.
    assert_eq!(env.graph().node("t").unwrap().result_ref(), Some("res-1"));
    assert_eq!(
        results.get("res-1").unwrap().outcome(),
        crate::GraphNodeResultOutcome::Rejected
    );
    assert_eq!(results.len(), 2);
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
