//! Core→State graph persistence: genesis, CAS successor, history, results,
//! digest-equal reload, conflict without partial change, retry after a
//! failed write, and core-issuer refusals. Real store; synthetic clock.

use receipts_state::{
    GraphCompilerRefV1, GraphPublicationOutcomeV1, GraphSourceEvidenceRefV1, StateGraphGenesisV1,
};
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

/// Everything a graph publication may touch, as the reader sees it.
#[derive(Debug, PartialEq)]
struct Image {
    head: (String, Option<String>),
    history: Vec<GraphMutation>,
    result_rows: Vec<bool>,
    watermark: String,
}

fn image(writer: &CoreStateWriter, refs: &[&str]) -> Image {
    let head = stored(writer);
    Image {
        head: (
            head.graph_version().as_str().to_owned(),
            head.resulting_digest().map(str::to_owned),
        ),
        history: load_graph_history_v1(writer.reader(), "p", "g").unwrap(),
        result_rows: refs
            .iter()
            .map(|r| {
                writer
                    .reader()
                    .find_node_result_v1("p", "g", r)
                    .unwrap()
                    .is_some()
            })
            .collect(),
        watermark: format!(
            "{:?}",
            writer.reader().find_trusted_time_watermark("p").unwrap()
        ),
    }
}

/// The State pieces `persist_mutation_v1` builds for `outcome`, for tests that
/// drive the Core forwarder directly with their own attachment sets.
fn parts(
    parent: &ExecutionGraphEnvelope,
    mutation: &GraphMutation,
    outcome: &GraphMutationOutcomeV1,
    refs: &[&str],
) -> (
    Vec<(String, String, receipts_state::StateGraphNodeResultV1)>,
    receipts_state::StateGraphMutationV1,
    receipts_state::StateGraphSnapshotV1,
) {
    let content = state_content_v1(
        &outcome.envelope,
        compiled_provenance(
            GraphCompilerRefV1::parse(COMPILER).unwrap(),
            GraphSourceEvidenceRefV1::parse(SOURCE).unwrap(),
        ),
        false,
    )
    .unwrap();
    let results = refs
        .iter()
        .map(|r| {
            let result = outcome.results.get(r).unwrap();
            let json = crate::wire::GraphNodeResultWireV1::from_graph_node_result(result).to_json();
            (
                parent.graph_version().as_str().to_owned(),
                r.to_string(),
                receipts_state::StateGraphNodeResultV1::parse_json(&json).unwrap(),
            )
        })
        .collect();
    (
        results,
        receipts_state::StateGraphMutationV1::parse_json(
            &GraphMutationWireV1::from_mutation(mutation).to_json(),
        )
        .unwrap(),
        receipts_state::StateGraphSnapshotV1::new(
            outcome.envelope.graph_version().as_str().to_owned(),
            parent.graph_version().as_str().to_owned(),
            content,
        )
        .unwrap(),
    )
}

/// v2 holds the node in review; returns it with the m2 mutation that attaches
/// `res-1`, its outcome and the result ledger before the attachment (the base
/// for alternative attachments).
fn review_graph(
    writer: &mut CoreStateWriter,
) -> (
    ExecutionGraphEnvelope,
    GraphMutation,
    GraphMutationOutcomeV1,
    GraphResultLedgerV1,
) {
    let v1 = persist_compiled_graph_v1(writer, &CLOCK, genesis(&source_env())).unwrap();
    let none = GraphResultLedgerV1::new();
    let (m1, o1) = apply(&v1, &none, "m1", a2("w"), Value::Array(run_to_review("t")));
    let v2 = persist_mutation_v1(writer, &CLOCK, &v1, &m1, &o1).unwrap();
    let (m2, o2) = apply(
        &v2,
        &o1.results,
        "m2",
        a2("w"),
        json!([attach("t", "res-1", result("t", "PASS", "rv"))]),
    );
    (v2, m2, o2, o1.results)
}

#[test]
fn a_failed_graph_publication_changes_nothing_and_does_not_block_other_content() {
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let (v2, m2, o2, base) = review_graph(&mut writer);
    let before = image(&writer, &["res-1"]);
    assert_eq!(before.result_rows, [false]);

    // A regressing trusted clock fails the publication after its result row
    // would have been written: no result, history, head or watermark moves.
    let earlier = Clock("2026-11-08T09:00:00.000000000Z");
    assert!(matches!(
        persist_mutation_v1(&mut writer, &earlier, &v2, &m2, &o2),
        Err(GraphStoreErrorV1::Write(CoreStateWriteError::State(_)))
    ));
    assert_eq!(image(&writer, &["res-1"]), before);

    // A result outcome missing from the ledger is refused before State.
    let missing = GraphMutationOutcomeV1 {
        envelope: o2.envelope.clone(),
        results: GraphResultLedgerV1::new(),
    };
    assert!(matches!(
        persist_mutation_v1(&mut writer, &CLOCK, &v2, &m2, &missing),
        Err(GraphStoreErrorV1::MissingResult { .. })
    ));
    assert_eq!(image(&writer, &["res-1"]), before);

    // Changed content under the same reference is not blocked by the failure:
    // no orphan row survived it.
    let (m2x, o2x) = apply(
        &v2,
        &base,
        "m2x",
        a2("w"),
        json!([attach("t", "res-1", result("t", "FAIL", "rv"))]),
    );
    let v3x = persist_mutation_v1(&mut writer, &CLOCK, &v2, &m2x, &o2x).unwrap();
    assert_eq!(v3x.graph(), o2x.envelope.graph());
    let rows = load_node_results_v1(
        writer.reader(),
        "p",
        "g",
        &GraphVersionV1::try_new("2").unwrap(),
        "t",
    )
    .unwrap();
    assert_eq!(
        rows,
        [(
            "res-1".to_string(),
            o2x.results.get("res-1").unwrap().clone()
        )]
    );
}

#[test]
fn a_foreign_orphan_result_row_refuses_the_publication_and_an_identical_one_is_adopted() {
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let (v2, m2, o2, base) = review_graph(&mut writer);
    let (results, _, _) = parts(&v2, &m2, &o2, &["res-1"]);
    let (version, reference, row) = results.into_iter().next().unwrap();
    // The legacy writer left a DIFFERENT row under this reference.
    let foreign_row = {
        let (ox_m, ox_o) = apply(
            &v2,
            &base,
            "mx",
            a2("w"),
            json!([attach("t", "res-1", result("t", "FAIL", "rv"))]),
        );
        parts(&v2, &ox_m, &ox_o, &["res-1"]).0.remove(0).2
    };
    writer
        .append_node_result("p", &version, &reference, foreign_row)
        .unwrap();
    let before = image(&writer, &["res-1"]);
    assert_eq!(before.result_rows, [true]);
    assert!(matches!(
        persist_mutation_v1(&mut writer, &CLOCK, &v2, &m2, &o2),
        Err(GraphStoreErrorV1::ResultConflict { .. })
    ));
    assert_eq!(image(&writer, &["res-1"]), before, "nothing else moved");

    // An identical orphan (left by the old writer) is adopted, never replaced.
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let (v2, m2, o2, _) = review_graph(&mut writer);
    writer
        .append_node_result("p", &version, &reference, row)
        .unwrap();
    let v3 = persist_mutation_v1(&mut writer, &CLOCK, &v2, &m2, &o2).unwrap();
    assert_eq!(v3.graph(), o2.envelope.graph());
}

#[test]
fn a_complete_retry_replays_after_reopen_and_conflicting_membership_refuses() {
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let (v2, m2, o2, base) = review_graph(&mut writer);
    let (v3, applied) = persist_mutation_outcome_v1(&mut writer, &CLOCK, &v2, &m2, &o2).unwrap();
    assert_eq!(applied, GraphPublicationOutcomeV1::Applied);
    let done = image(&writer, &["res-1"]);
    assert_eq!(done.result_rows, [true]);

    // The exact retry is recognized although the head already moved on.
    let (again, replayed) =
        persist_mutation_outcome_v1(&mut writer, &CLOCK, &v2, &m2, &o2).unwrap();
    assert_eq!(
        (again, replayed),
        (v3.clone(), GraphPublicationOutcomeV1::Replayed)
    );
    assert_eq!(image(&writer, &["res-1"]), done);
    // A later mutation does not make the retry of the earlier one a conflict.
    let (m3, o3) = apply(&v3, &o2.results, "m3", a1(), add_node("x", "4"));
    let v4 = persist_mutation_v1(&mut writer, &CLOCK, &v3, &m3, &o3).unwrap();
    let (replayed_v3, replayed) =
        persist_mutation_outcome_v1(&mut writer, &CLOCK, &v2, &m2, &o2).unwrap();
    assert_eq!(
        (replayed_v3.graph(), replayed),
        (v3.graph(), GraphPublicationOutcomeV1::Replayed)
    );
    let done = image(&writer, &["res-1"]);
    assert_eq!(done.head.0, v4.graph_version().as_str());

    // After a reopen the retry still replays and writes nothing.
    drop(writer);
    let mut writer =
        CoreStateWriter::open(dir.db(), crate::test_support::identity(POLICY)).unwrap();
    let (_, replayed) = persist_mutation_outcome_v1(&mut writer, &CLOCK, &v2, &m2, &o2).unwrap();
    assert_eq!(replayed, GraphPublicationOutcomeV1::Replayed);
    assert_eq!(image(&writer, &["res-1"]), done);
    // Different content under the same mutation id is refused outright.
    let (_, other) = apply(
        &v2,
        &base,
        "m2",
        a2("w"),
        json!([attach("t", "res-1", result("t", "FAIL", "rv"))]),
    );
    let m2_other = mutation(
        &v2,
        "m2",
        &a2("w"),
        json!([attach("t", "res-1", result("t", "FAIL", "rv"))]),
    );
    assert!(matches!(
        persist_mutation_v1(&mut writer, &CLOCK, &v2, &m2_other, &other),
        Err(GraphStoreErrorV1::MutationExists)
    ));
    assert_eq!(image(&writer, &["res-1"]), done);
}

#[test]
fn attachment_membership_is_part_of_the_publication_identity() {
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let (v2, m2, o2, base) = review_graph(&mut writer);
    // The mutation attaches res-1; the publication also carries an
    // unreferenced second row (another result of the same node).
    let (me, oe) = apply(
        &v2,
        &base,
        "me",
        a2("w"),
        json!([attach("t", "res-extra", result("t", "FAIL", "rv"))]),
    );
    let first = || parts(&v2, &m2, &o2, &["res-1"]).0.remove(0);
    let extra = || parts(&v2, &me, &oe, &["res-extra"]).0.remove(0);
    let publish = |writer: &mut CoreStateWriter, rows: Vec<_>| {
        let (_, mutation, snapshot) = parts(&v2, &m2, &o2, &["res-1"]);
        writer.publish_graph_mutation(&CLOCK, rows, mutation, snapshot)
    };
    assert_eq!(
        publish(&mut writer, vec![first(), extra()]).unwrap(),
        GraphPublicationOutcomeV1::Applied
    );
    let done = image(&writer, &["res-1", "res-extra"]);
    assert_eq!(done.result_rows, [true, true]);

    // Attachment order is immaterial.
    assert_eq!(
        publish(&mut writer, vec![extra(), first()]).unwrap(),
        GraphPublicationOutcomeV1::Replayed
    );
    // A subset, a superset, a changed version, a changed reference and a
    // changed body are all conflicting retries; nothing changes.
    let superset = vec![first(), extra(), ("2".into(), "res-3".into(), extra().2)];
    let mut moved_version = vec![first(), extra()];
    moved_version[1].0 = "1".into();
    let mut renamed = vec![first(), extra()];
    renamed[1].1 = "res-renamed".into();
    let mut changed_body = vec![first(), extra()];
    changed_body[1].2 = first().2;
    for rows in [
        vec![first()],
        superset,
        moved_version,
        renamed,
        changed_body,
    ] {
        match publish(&mut writer, rows) {
            Err(CoreStateWriteError::State(receipts_state::StateError::Publication {
                code: receipts_state::PublicationFailureCode::ConflictingRetry,
                ..
            })) => {}
            other => panic!("expected a conflicting retry, got {other:?}"),
        }
        assert_eq!(image(&writer, &["res-1", "res-extra"]), done);
    }
}

#[test]
fn a_legacy_publication_without_recorded_membership_refuses_an_exact_retry() {
    let dir = Dir::new();
    let mut writer = writer_with_epochs(&dir, POLICY, &[]);
    let (v2, m2, o2, _) = review_graph(&mut writer);
    // The old non-atomic writers: the row, then the mutation, as two commits.
    let (rows, mutation, snapshot) = parts(&v2, &m2, &o2, &["res-1"]);
    let (version, reference, row) = rows.into_iter().next().unwrap();
    writer
        .append_node_result("p", &version, &reference, row)
        .unwrap();
    writer
        .append_graph_mutation(&CLOCK, mutation, snapshot)
        .unwrap();
    let legacy = image(&writer, &["res-1"]);
    assert_eq!(legacy.head.0, "3");
    // Membership cannot be inferred from the stored rows: safe refusal.
    assert!(matches!(
        persist_mutation_v1(&mut writer, &CLOCK, &v2, &m2, &o2),
        Err(GraphStoreErrorV1::ConflictingRetry)
    ));
    assert_eq!(image(&writer, &["res-1"]), legacy);
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
