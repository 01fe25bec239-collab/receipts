//! Core→State graph persistence: genesis, CAS successor, history, results,
//! digest-equal reload, conflict without partial change, retry after a
//! failed write, and core-issuer refusals. Real store; synthetic clock.

use receipts_state::{GraphCompilerRefV1, GraphSourceEvidenceRefV1, StateGraphGenesisV1};
use serde_json::{Value, json};

use super::mutation_interpreter_tests::{
    SHA, a1, a2, attach, envelope_in, node, result, role, run_to_review, scope,
};
use super::store::{compiled_provenance, state_content_v1};
use super::*;
use crate::orchestration::{CorePolicyRefusal, CoreStateWriteError, CoreStateWriter};
use crate::test_support::{CLOCK, COMPILER, Clock, Dir, SOURCE, writer_with_epochs};
use crate::wire::GraphMutationWireV1;
use crate::{
    ExecutionGraphEnvelope, GraphEdge, GraphMutation, GraphMutationOutcomeV1, GraphNodeKind,
    GraphNodeState as S, GraphResultLedgerV1, GraphVersionV1, PrecedenceKind, ScopedActorV1,
    apply_graph_mutation_v1, graph_content_digest_v1,
};

const POLICY: &str = "free.single_runtime";

fn source_env() -> ExecutionGraphEnvelope {
    envelope_in(
        "p",
        "g",
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

fn genesis(env: &ExecutionGraphEnvelope) -> StateGraphGenesisV1 {
    state_content_v1(
        env,
        compiled_provenance(
            GraphCompilerRefV1::parse(COMPILER).unwrap(),
            GraphSourceEvidenceRefV1::parse(SOURCE).unwrap(),
        ),
        true,
    )
    .unwrap()
}

fn mutation(
    env: &ExecutionGraphEnvelope,
    id: &str,
    actor: &ScopedActorV1,
    ops: Value,
) -> GraphMutation {
    let parent: u128 = env.graph_version().as_str().parse().unwrap();
    let text = json!({
        "mutation_id": id, "graph_id": env.graph_id(), "parent_version": parent,
        "resulting_version": parent + 1, "actor": role(actor), "reason": "test",
        "created_at": "2026-11-08T09:30:00Z", "operations": ops,
    });
    GraphMutationWireV1::from_json(&text.to_string())
        .unwrap()
        .to_mutation()
        .unwrap()
}

fn apply(
    env: &ExecutionGraphEnvelope,
    results: &GraphResultLedgerV1,
    id: &str,
    actor: ScopedActorV1,
    ops: Value,
) -> (GraphMutation, GraphMutationOutcomeV1) {
    let m = mutation(env, id, &actor, ops);
    let outcome = apply_graph_mutation_v1(env, results, &m, &scope(env, actor), None).unwrap();
    (m, outcome)
}

fn add_node(id: &str, version: &str) -> Value {
    json!([{"op":"ADD_NODE","node_id":id,"detail":{"node":{
        "node_id":id,"graph_id":"g","kind":"TASK","state":"PLANNED","workstream_id":"w",
        "created_in_version": version.parse::<u64>().unwrap()}}}])
}

fn stored(writer: &CoreStateWriter) -> ExecutionGraphEnvelope {
    load_current_graph_v1(writer.reader(), "p", "g")
        .unwrap()
        .unwrap()
}

#[test]
fn compiled_genesis_persists_and_reloads_digest_equal() {
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let env = source_env();
    let v1 = persist_compiled_graph_v1(&mut writer, &CLOCK, genesis(&env)).unwrap();
    assert_eq!(v1.graph(), env.graph());
    assert_eq!(
        v1.resulting_digest(),
        Some(graph_content_digest_v1(env.graph()).unwrap().as_str())
    );
    assert_eq!(v1.compiled_from(), env.compiled_from());
    assert_eq!(
        (v1.project_id(), v1.goal_id(), v1.policy_id()),
        ("p", "goal", POLICY)
    );
    // created_at is State's trusted clock time, not the caller's.
    assert_eq!(v1.created_at().as_str(), "2026-11-08T10:00:00.000000000Z");
    assert!(
        load_graph_history_v1(writer.reader(), "p", "g")
            .unwrap()
            .is_empty()
    );
    // Reopen: a fresh writer reloads the same graph.
    drop(writer);
    let writer = CoreStateWriter::open(dir.db(), crate::test_support::identity(POLICY)).unwrap();
    assert_eq!(stored(&writer), v1);
}

#[test]
fn mutation_persist_reload_digest_equal_with_history_and_results() {
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let v1 = persist_compiled_graph_v1(&mut writer, &CLOCK, genesis(&source_env())).unwrap();
    let none = GraphResultLedgerV1::new();
    let (m1, o1) = apply(&v1, &none, "m1", a2("w"), Value::Array(run_to_review("t")));
    let v2 = persist_mutation_v1(&mut writer, &CLOCK, &v1, &m1, &o1).unwrap();
    assert_eq!(v2.graph(), o1.envelope.graph());
    assert_eq!(v2.resulting_digest(), o1.envelope.resulting_digest());

    let (m2, o2) = apply(
        &v2,
        &o1.results,
        "m2",
        a2("w"),
        json!([attach("t", "res-1", result("t", "PASS", "rv"))]),
    );
    let v3 = persist_mutation_v1(&mut writer, &CLOCK, &v2, &m2, &o2).unwrap();
    assert_eq!(v3.graph_version().as_str(), "3");
    assert_eq!(v3.graph(), o2.envelope.graph());
    assert_eq!(stored(&writer), v3);

    // History is the exact applied mutations; older versions are unchanged.
    let history = load_graph_history_v1(writer.reader(), "p", "g").unwrap();
    assert_eq!(history, [m1, m2]);
    let old = load_graph_version_v1(
        writer.reader(),
        "p",
        "g",
        &GraphVersionV1::try_new("1").unwrap(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(old.graph(), v1.graph());
    // The attached result reloads exactly.
    let results = load_node_results_v1(
        writer.reader(),
        "p",
        "g",
        &GraphVersionV1::try_new("2").unwrap(),
        "t",
    )
    .unwrap();
    assert_eq!(
        results,
        [(
            "res-1".to_string(),
            o2.results.get("res-1").unwrap().clone()
        )]
    );
}

#[test]
fn concurrent_writer_conflict_is_retryable_and_changes_nothing() {
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let v1 = persist_compiled_graph_v1(&mut writer, &CLOCK, genesis(&source_env())).unwrap();
    let none = GraphResultLedgerV1::new();
    // Two writers derive successors from the same parent.
    let (ma, oa) = apply(&v1, &none, "m-a", a1(), add_node("x", "2"));
    let (mb, ob) = apply(&v1, &none, "m-b", a1(), add_node("y", "2"));
    let v2 = persist_mutation_v1(&mut writer, &CLOCK, &v1, &ma, &oa).unwrap();
    match persist_mutation_v1(&mut writer, &CLOCK, &v1, &mb, &ob) {
        Err(GraphStoreErrorV1::Conflict { stored_version }) => assert_eq!(stored_version, "2"),
        other => panic!("expected a retryable conflict, got {other:?}"),
    }
    assert_eq!(stored(&writer), v2);
    assert_eq!(
        load_graph_history_v1(writer.reader(), "p", "g").unwrap(),
        std::slice::from_ref(&ma)
    );

    // A forged parent at the current version but different content conflicts too.
    let (_, forged) = apply(&v1, &none, "m-f", a1(), add_node("z", "2"));
    let (mf, of) = apply(&forged.envelope, &none, "m-f", a1(), add_node("w2", "3"));
    assert!(matches!(
        persist_mutation_v1(&mut writer, &CLOCK, &forged.envelope, &mf, &of),
        Err(GraphStoreErrorV1::Conflict { .. })
    ));

    // Retry: reload, re-derive on the stored head, persist.
    let head = stored(&writer);
    let (mb2, ob2) = apply(&head, &none, "m-b", a1(), add_node("y", "3"));
    let v3 = persist_mutation_v1(&mut writer, &CLOCK, &head, &mb2, &ob2).unwrap();
    assert!(v3.graph().contains_node("x") && v3.graph().contains_node("y"));

    // Reusing a mutation id refuses before anything is written.
    let (dup, od) = apply(&v3, &none, "m-a", a1(), add_node("q", "4"));
    assert!(matches!(
        persist_mutation_v1(&mut writer, &CLOCK, &v3, &dup, &od),
        Err(GraphStoreErrorV1::MutationExists)
    ));
    assert_eq!(stored(&writer), v3);
    // An outcome that is not the successor of the named parent refuses.
    assert!(matches!(
        persist_mutation_v1(&mut writer, &CLOCK, &v1, &mb2, &ob2),
        Err(GraphStoreErrorV1::NotASuccessor)
    ));
}

#[test]
fn failed_snapshot_write_rolls_back_the_head_and_retry_reuses_identical_results() {
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let v1 = persist_compiled_graph_v1(&mut writer, &CLOCK, genesis(&source_env())).unwrap();
    let none = GraphResultLedgerV1::new();
    let (m1, o1) = apply(&v1, &none, "m1", a2("w"), Value::Array(run_to_review("t")));
    let v2 = persist_mutation_v1(&mut writer, &CLOCK, &v1, &m1, &o1).unwrap();
    let (m2, o2) = apply(
        &v2,
        &o1.results,
        "m2",
        a2("w"),
        json!([attach("t", "res-1", result("t", "PASS", "rv"))]),
    );
    // A regressing trusted clock refuses the snapshot after the result row.
    let earlier = Clock("2026-11-08T09:00:00.000000000Z");
    assert!(matches!(
        persist_mutation_v1(&mut writer, &earlier, &v2, &m2, &o2),
        Err(GraphStoreErrorV1::Write(CoreStateWriteError::State(_)))
    ));
    assert_eq!(stored(&writer), v2, "head unchanged");
    // A different result under the same reference refuses.
    let (m2x, o2x) = apply(
        &v2,
        &o1.results,
        "m2",
        a2("w"),
        json!([attach("t", "res-1", result("t", "FAIL", "rv"))]),
    );
    assert!(matches!(
        persist_mutation_v1(&mut writer, &CLOCK, &v2, &m2x, &o2x),
        Err(GraphStoreErrorV1::ResultConflict { .. })
    ));
    // The identical retry succeeds and reuses the stored result.
    let v3 = persist_mutation_v1(&mut writer, &CLOCK, &v2, &m2, &o2).unwrap();
    assert_eq!(v3.graph(), o2.envelope.graph());
}

#[test]
fn core_issuer_refuses_foreign_policy_compiler_and_unknown_graphs() {
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, "pro.distributed_orchestration", &[]);
    match persist_compiled_graph_v1(&mut writer, &CLOCK, genesis(&source_env())) {
        Err(GraphStoreErrorV1::Write(CoreStateWriteError::Policy(refusal))) => {
            assert_eq!(refusal, CorePolicyRefusal::PolicyNotAdmitted)
        }
        other => panic!("expected a policy refusal, got {other:?}"),
    }
    assert!(
        load_current_graph_v1(writer.reader(), "p", "g")
            .unwrap()
            .is_none()
    );

    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let foreign = state_content_v1(
        &source_env(),
        compiled_provenance(
            GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAX").unwrap(),
            GraphSourceEvidenceRefV1::parse(SOURCE).unwrap(),
        ),
        true,
    )
    .unwrap();
    assert!(matches!(
        persist_compiled_graph_v1(&mut writer, &CLOCK, foreign),
        Err(GraphStoreErrorV1::Write(CoreStateWriteError::Policy(
            CorePolicyRefusal::CompilerNotAdmitted
        )))
    ));
    // A successor of a graph that was never stored refuses.
    let env = source_env();
    let (m, o) = apply(
        &env,
        &GraphResultLedgerV1::new(),
        "m",
        a1(),
        add_node("x", "2"),
    );
    assert!(matches!(
        persist_mutation_v1(&mut writer, &CLOCK, &env, &m, &o),
        Err(GraphStoreErrorV1::UnknownGraph)
    ));
}
