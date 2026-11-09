//! Shared atomic publication controls: graph mutation with its results, and
//! decision packages (records + events/receipts + availability facts + claim
//! settlement). Failure injection runs at EVERY write boundary and compares a
//! full table/watermark/head fingerprint; retries, conflicts, immutable
//! REJECT history, close/reopen and authority mismatches are covered. Synthetic
//! same-user controls only.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use crate::error::{
    GraphFailureCode as GCode, PublicationFailureCode as PCode, RecordFailureCode as RCode,
    StateError,
};
use crate::write_fault::with_fault;
use crate::{
    ActorKind, AvailabilityFactKindV1 as Kind, AvailabilityFactSourceV1 as Source,
    AvailabilityFactV1, DecisionPublicationReceiptV1, DecisionPublicationV1, DecisionRecordKind,
    DecisionRecordV1, EventActor, EventPayloadReference, EventProposalOriginV1, EventProposalV1,
    EventSubject, EventType, GraphCompilerRefV1, GraphGenesisReasonV1, GraphMutationPublicationV1,
    GraphPublicationOutcomeV1, GraphResultAttachmentV1, GraphSourceEvidenceRefV1,
    PublicationEventV1, ReviewClaimOutcomeV1, ReviewClaimRequestV1, ReviewClaimSettlementV1,
    ReviewClaimTicketV1, StateEpochValueV1, StateGraphEdgeRelationV1, StateGraphEdgeV1,
    StateGraphGenesisProvenanceV1, StateGraphGenesisV1, StateGraphMutationV1,
    StateGraphNodeResultV1, StateGraphNodeV1, StateGraphSnapshotV1, StateWriteOperationV1 as Op,
    StateWriteRefusalV1 as R, StateWriteSubjectV1, SubjectKind, TrustedClockV1, TrustedCoreStore,
    TrustedTimeSampleV1,
};

pub(crate) const EVENT_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
pub(crate) const EVENT_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
pub(crate) const EVENT_C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";

pub(crate) struct Clock(pub(crate) &'static str);
impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: self.0.into(),
            clock_source_id: "synthetic-clock".into(),
            clock_contract_version: "1".into(),
        })
    }
}
pub(crate) const G0: Clock = Clock("2026-10-28T03:00:00.000000000Z");
pub(crate) const G1: Clock = Clock("2026-10-28T04:00:00.000000000Z");
pub(crate) const G_EARLY: Clock = Clock("2026-10-28T02:00:00.000000000Z");
pub(crate) const T1: Clock = Clock("2026-11-08T10:00:00.000000000Z");
pub(crate) const T2: Clock = Clock("2026-11-08T11:00:00.000000000Z");

/// A private 0700 directory removed on drop.
pub(crate) struct Dir(pub(crate) PathBuf);
impl Dir {
    pub(crate) fn new(tag: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "receipts-publication-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
    pub(crate) fn db(&self) -> PathBuf {
        self.0.join("state.db")
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn refusal(error: StateError) -> R {
    match error {
        StateError::WriteAuthorityRefused { reason } => reason,
        other => panic!("expected an authority refusal, got {other:?}"),
    }
}
pub(crate) fn publication_code(error: StateError) -> PCode {
    match error {
        StateError::Publication { code, .. } => code,
        other => panic!("expected a publication code, got {other:?}"),
    }
}
pub(crate) fn graph_code(error: StateError) -> GCode {
    match error {
        StateError::GraphPersistence { code, .. } => code,
        other => panic!("expected a graph persistence code, got {other:?}"),
    }
}

pub(crate) fn project_subject(op: Op, project: &str) -> StateWriteSubjectV1 {
    StateWriteSubjectV1 {
        operation: op,
        project_id: project.into(),
        graph_id: None,
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    }
}
fn result_subject(project: &str, graph: &str) -> StateWriteSubjectV1 {
    StateWriteSubjectV1 {
        operation: Op::GraphNodeResult,
        project_id: project.into(),
        graph_id: Some(graph.into()),
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    }
}
fn graph_subject(op: Op, project: &str, graph: &str) -> StateWriteSubjectV1 {
    StateWriteSubjectV1 {
        operation: op,
        project_id: project.into(),
        graph_id: Some(graph.into()),
        source_ref: Some("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
        compiler_id: Some("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
        policy_id: Some("policy".into()),
    }
}

/// Everything a partial write could leave behind: every table's row count,
/// the project watermark and each graph head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fingerprint {
    tables: Vec<(String, i64)>,
    watermarks: Vec<String>,
    heads: Vec<String>,
}

pub(crate) fn fingerprint(store: &TrustedCoreStore) -> Fingerprint {
    let repo = store.reader();
    let conn = repo.connection();
    let column = |sql: &str| -> Vec<String> {
        let mut stmt = conn.prepare(sql).unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    Fingerprint {
        tables: repo
            .list_tables()
            .unwrap()
            .into_iter()
            .map(|t| {
                let n = repo.count_table_rows(&t).unwrap();
                (t, n)
            })
            .collect(),
        watermarks: column(
            "SELECT project_id||'@'||last_accepted_trusted_time FROM trusted_time_watermark ORDER BY 1",
        ),
        heads: column(
            "SELECT project_id||'/'||graph_id||'='||current_version FROM graphs ORDER BY 1",
        ),
    }
}

pub(crate) fn rows(store: &TrustedCoreStore, table: &str) -> i64 {
    store.reader().count_table_rows(table).unwrap()
}

/// Runs `run` once to count its write boundaries, then once per boundary with
/// that boundary failing. Every injected failure must return `Err`, leave the
/// fingerprint exactly as it was, and a fault-free retry from the rolled-back
/// state must reach the same final fingerprint as the clean run.
pub(crate) fn assert_atomic_at_every_boundary<S>(
    setup: impl Fn() -> S,
    observe: impl Fn(&S) -> Fingerprint,
    run: impl Fn(&mut S) -> Result<(), StateError>,
) -> usize {
    let mut clean = setup();
    let before = observe(&clean);
    let (result, boundaries) = with_fault(None, || run(&mut clean));
    result.expect("clean run succeeds");
    let after = observe(&clean);
    assert_ne!(before, after, "the operation writes something");
    assert!(boundaries >= 2, "the operation has write boundaries");
    for k in 1..=boundaries {
        let mut state = setup();
        let before = observe(&state);
        let (result, _) = with_fault(Some(k), || run(&mut state));
        assert!(
            matches!(result, Err(StateError::InternalQueryFailed { .. })),
            "boundary {k} must surface the injected fault, got {result:?}"
        );
        assert_eq!(
            observe(&state),
            before,
            "boundary {k} left partial rows, receipts, watermark or head"
        );
        run(&mut state).expect("a retry after the rollback succeeds");
        assert_eq!(
            observe(&state),
            after,
            "boundary {k}: retry differs from a clean run"
        );
    }
    boundaries
}

// ---- graph fixtures -------------------------------------------------------------

fn content(reference: Option<&str>) -> StateGraphGenesisV1 {
    StateGraphGenesisV1::new(
        "p".into(),
        "g".into(),
        "goal".into(),
        "policy".into(),
        None,
        Some(vec!["source".into()]),
        None,
        "a".repeat(64),
        vec![
            StateGraphNodeV1::new(
                "n".into(),
                "g".into(),
                "TASK".into(),
                "PLANNED".into(),
                None,
                None,
                None,
                Some(vec!["graph.core".into()]),
                None,
                None,
                None,
                None,
                reference.map(str::to_owned),
                Some(None),
                None,
            )
            .unwrap(),
        ],
        vec![
            StateGraphEdgeV1::new(
                "e".into(),
                "g".into(),
                "n".into(),
                "n".into(),
                StateGraphEdgeRelationV1::Control("ON_PASS".into()),
                None,
            )
            .unwrap(),
        ],
        StateGraphGenesisProvenanceV1::new(
            GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
            GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
            GraphGenesisReasonV1::InitialCompilation,
        ),
    )
    .unwrap()
}

fn mutation_json(id: &str, parent: &str, version: &str, reason: &str) -> String {
    format!(
        r#"{{"mutation_id":"{id}","graph_id":"g","parent_version":{parent},"resulting_version":{version},"actor":{{"role":"USER"}},"reason":"{reason}","created_at":"2026-10-28T03:00:00Z","operations":[{{"op":"ATTACH_RESULT","node_id":"n"}}]}}"#
    )
}

fn result_json(outcome: &str) -> StateGraphNodeResultV1 {
    StateGraphNodeResultV1::parse_json(&format!(
        r#"{{"node_id":"n","graph_id":"g","outcome":"{outcome}","produced_at":"2026-10-28T03:00:00Z"}}"#
    ))
    .unwrap()
}

/// results r1 + r2 (version 1), snapshot 2 referencing r1.
fn publication(id: &str, reason: &str, outcome: &str) -> GraphMutationPublicationV1 {
    GraphMutationPublicationV1::new(
        vec![
            GraphResultAttachmentV1::new("1", "r1", result_json(outcome)).unwrap(),
            GraphResultAttachmentV1::new("1", "r2", result_json("FAIL")).unwrap(),
        ],
        StateGraphMutationV1::parse_json(&mutation_json(id, "1", "2", reason)).unwrap(),
        StateGraphSnapshotV1::new("2".into(), "1".into(), content(Some("r1"))).unwrap(),
    )
    .unwrap()
}

fn graph_store(tag: &str) -> (Dir, TrustedCoreStore) {
    let dir = Dir::new(tag);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let admission = store.admit(graph_subject(Op::GraphGenesis, "p", "g"));
    store
        .create_initial_graph_v1(admission, &G0, content(None))
        .unwrap();
    (dir, store)
}

fn publish_graph(
    store: &mut TrustedCoreStore,
    clock: &Clock,
    p: &GraphMutationPublicationV1,
) -> Result<GraphPublicationOutcomeV1, StateError> {
    let admission = store.admit(graph_subject(Op::GraphMutation, "p", "g"));
    store.publish_graph_mutation_v1(admission, clock, p)
}

fn head(store: &TrustedCoreStore) -> String {
    store
        .reader()
        .load_current_graph_v1("p", "g")
        .unwrap()
        .unwrap()
        .graph_version()
        .to_owned()
}

#[test]
fn graph_results_snapshot_history_and_head_are_atomic_at_every_write_boundary() {
    let boundaries = assert_atomic_at_every_boundary(
        || graph_store("graph-fault"),
        |(_, store)| fingerprint(store),
        |(_, store)| {
            publish_graph(store, &G1, &publication("m2", "attach", "PASS")).map(|outcome| {
                assert_eq!(outcome, GraphPublicationOutcomeV1::Applied);
            })
        },
    );
    // two result rows, watermark, snapshot, head, mutation, seal and the closing mark
    assert!(boundaries >= 7, "{boundaries}");
    let (_dir, mut store) = graph_store("graph-final");
    publish_graph(&mut store, &G1, &publication("m2", "attach", "PASS")).unwrap();
    assert_eq!(head(&store), "2");
    assert_eq!(rows(&store, "graph_node_results"), 2);
    assert_eq!(rows(&store, "graph_mutations"), 1);
    assert_eq!(
        store
            .reader()
            .find_trusted_time_watermark("p")
            .unwrap()
            .unwrap()
            .last_accepted_trusted_time,
        G1.0
    );
}

#[test]
fn graph_publication_failures_leave_no_orphan_result_rows() {
    // The reviewer scenario: a regressing clock fails the snapshot step AFTER
    // the result rows. Nothing may survive, and a later correct publication
    // with changed content for the same reference must still be possible.
    let (_dir, mut store) = graph_store("graph-regress");
    let before = fingerprint(&store);
    let error =
        publish_graph(&mut store, &G_EARLY, &publication("m2", "attach", "PASS")).unwrap_err();
    assert_eq!(graph_code(error), GCode::ClockRegression);
    assert_eq!(fingerprint(&store), before);
    assert_eq!(rows(&store, "graph_node_results"), 0);
    publish_graph(&mut store, &G1, &publication("m2", "attach", "FAIL")).unwrap();
    assert_eq!(rows(&store, "graph_node_results"), 2);

    // A snapshot citing a result that is not attached refuses with no rows.
    let (_dir, mut store) = graph_store("graph-missing");
    let before = fingerprint(&store);
    let missing = GraphMutationPublicationV1::new(
        vec![GraphResultAttachmentV1::new("1", "r2", result_json("PASS")).unwrap()],
        StateGraphMutationV1::parse_json(&mutation_json("m2", "1", "2", "attach")).unwrap(),
        StateGraphSnapshotV1::new("2".into(), "1".into(), content(Some("absent"))).unwrap(),
    )
    .unwrap();
    assert_eq!(
        graph_code(publish_graph(&mut store, &G1, &missing).unwrap_err()),
        GCode::IdentityMismatch
    );
    assert_eq!(fingerprint(&store), before);

    // A stale parent loses the CAS after the result rows were staged.
    let (_dir, mut store) = graph_store("graph-stale");
    publish_graph(&mut store, &G1, &publication("m2", "attach", "PASS")).unwrap();
    let before = fingerprint(&store);
    let stale = GraphMutationPublicationV1::new(
        vec![GraphResultAttachmentV1::new("1", "r9", result_json("PASS")).unwrap()],
        StateGraphMutationV1::parse_json(&mutation_json("m3", "1", "3", "again")).unwrap(),
        StateGraphSnapshotV1::new("3".into(), "1".into(), content(Some("r9"))).unwrap(),
    )
    .unwrap();
    assert_eq!(
        graph_code(publish_graph(&mut store, &G1, &stale).unwrap_err()),
        GCode::ParentConflict
    );
    assert_eq!(fingerprint(&store), before);
}

#[test]
fn exact_graph_retry_replays_and_changed_content_refuses() {
    let (_dir, mut store) = graph_store("graph-retry");
    let first = publication("m2", "attach", "PASS");
    assert_eq!(
        publish_graph(&mut store, &G1, &first).unwrap(),
        GraphPublicationOutcomeV1::Applied
    );
    let applied = fingerprint(&store);
    // The exact retry changes nothing, even with a later clock.
    assert_eq!(
        publish_graph(&mut store, &T1, &publication("m2", "attach", "PASS")).unwrap(),
        GraphPublicationOutcomeV1::Replayed
    );
    assert_eq!(fingerprint(&store), applied);
    // Same mutation identity with a different reason, or a different result
    // body, is a conflicting retry.
    for changed in [
        publication("m2", "different reason", "PASS"),
        publication("m2", "attach", "BLOCKED"),
    ] {
        assert_eq!(
            publication_code(publish_graph(&mut store, &T1, &changed).unwrap_err()),
            PCode::ConflictingRetry
        );
        assert_eq!(fingerprint(&store), applied);
    }
    // The exact retry still replays after close/reopen.
    let path = _dir.db();
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(path).unwrap();
    assert_eq!(
        publish_graph(&mut store, &T1, &publication("m2", "attach", "PASS")).unwrap(),
        GraphPublicationOutcomeV1::Replayed
    );
    assert_eq!(fingerprint(&store), applied);
}

#[test]
fn identical_orphan_results_are_adopted_and_conflicting_ones_refuse() {
    // Left by the older one-row-per-transaction writer.
    let (_dir, mut store) = graph_store("graph-orphan");
    let admission = store.admit(result_subject("p", "g"));
    store
        .append_node_result_v1(admission, "p", "1", "r1", result_json("PASS"))
        .unwrap();
    publish_graph(&mut store, &G1, &publication("m2", "attach", "PASS")).unwrap();
    assert_eq!(rows(&store, "graph_node_results"), 2);

    let (_dir, mut store) = graph_store("graph-orphan-conflict");
    let admission = store.admit(result_subject("p", "g"));
    store
        .append_node_result_v1(admission, "p", "1", "r1", result_json("FAIL"))
        .unwrap();
    let before = fingerprint(&store);
    assert_eq!(
        graph_code(
            publish_graph(&mut store, &G1, &publication("m2", "attach", "PASS")).unwrap_err()
        ),
        GCode::AlreadyExists
    );
    assert_eq!(fingerprint(&store), before);
}

#[test]
fn graph_publication_keeps_project_graph_operation_and_instance_checks() {
    let (dir, mut store) = graph_store("graph-auth");
    let before = fingerprint(&store);
    let p = publication("m2", "attach", "PASS");
    for (subject, expected) in [
        (
            graph_subject(Op::GraphMutation, "q", "g"),
            R::ProjectMismatch,
        ),
        (
            graph_subject(Op::GraphMutation, "p", "other"),
            R::GraphMismatch,
        ),
        (
            graph_subject(Op::GraphNodeResult, "p", "g"),
            R::WrongOperation,
        ),
        (
            StateWriteSubjectV1 {
                policy_id: Some("other-policy".into()),
                ..graph_subject(Op::GraphMutation, "p", "g")
            },
            R::PolicyMismatch,
        ),
        (
            StateWriteSubjectV1 {
                source_ref: Some("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW".into()),
                ..graph_subject(Op::GraphMutation, "p", "g")
            },
            R::SourceMismatch,
        ),
        (
            StateWriteSubjectV1 {
                compiler_id: Some("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAW".into()),
                ..graph_subject(Op::GraphMutation, "p", "g")
            },
            R::CompilerMismatch,
        ),
    ] {
        let admission = store.admit(subject);
        assert_eq!(
            refusal(
                store
                    .publish_graph_mutation_v1(admission, &G1, &p)
                    .unwrap_err()
            ),
            expected
        );
        assert_eq!(fingerprint(&store), before);
    }
    // An admission minted before a reopen never transfers.
    let stale = store.admit(graph_subject(Op::GraphMutation, "p", "g"));
    drop(store);
    let mut reopened = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert_eq!(
        refusal(
            reopened
                .publish_graph_mutation_v1(stale, &G1, &p)
                .unwrap_err()
        ),
        R::WrongStore
    );
    // A result for another graph cannot even be packaged.
    let foreign = StateGraphNodeResultV1::parse_json(
        r#"{"node_id":"n","graph_id":"other","outcome":"PASS","produced_at":"2026-10-28T03:00:00Z"}"#,
    )
    .unwrap();
    assert!(
        GraphMutationPublicationV1::new(
            vec![GraphResultAttachmentV1::new("1", "r1", foreign).unwrap()],
            StateGraphMutationV1::parse_json(&mutation_json("m2", "1", "2", "attach")).unwrap(),
            StateGraphSnapshotV1::new("2".into(), "1".into(), content(Some("r1"))).unwrap(),
        )
        .is_err()
    );
}

// ---- decision packages ---------------------------------------------------------

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub(crate) fn record(kind: DecisionRecordKind, id: &str, json: &str) -> DecisionRecordV1 {
    DecisionRecordV1::parse("p", kind, id, json).unwrap()
}

pub(crate) fn routing_record() -> DecisionRecordV1 {
    record(
        DecisionRecordKind::RoutingDecision,
        "rd-1",
        r#"{"decision_id":"rd-1","request_id":"rq-1","task_id":"t-1","outcome":"SELECTED","selected_provider":"p","registry_freshness":{"stale":false},"mode":"AUTO_CURRENT","user_involved":false}"#,
    )
}

pub(crate) fn reject_review() -> DecisionRecordV1 {
    record(
        DecisionRecordKind::A4Review,
        "rev-1",
        &format!(
            r#"{{"review_id":"rev-1","task_id":"t-1","reviewed_sha":"{A}","verdict":"REJECT","independence_attested":true,"blocking_findings":[{{"finding_id":"f-1","severity":"HIGH","category":"CORRECTNESS","description":"d","blocking":true}}],"nonblocking_findings":[],"dimensions":[{{"dimension":"CORRECTNESS","assessment":"SATISFIED"}}]}}"#
        ),
    )
}

pub(crate) fn pass_review() -> DecisionRecordV1 {
    record(
        DecisionRecordKind::A4Review,
        "rev-1",
        &format!(
            r#"{{"review_id":"rev-1","task_id":"t-1","reviewed_sha":"{A}","verdict":"PASS","independence_attested":true,"blocking_findings":[],"nonblocking_findings":[],"dimensions":[{{"dimension":"CORRECTNESS","assessment":"SATISFIED"}}]}}"#
        ),
    )
}

pub(crate) fn proposal(event_type: EventType, digest: &str) -> EventProposalV1 {
    EventProposalV1::new(
        EventProposalOriginV1::Routing,
        "p".into(),
        None,
        event_type,
        EventActor {
            kind: ActorKind::System,
            id: None,
        },
        EventSubject {
            kind: SubjectKind::Task,
            id: "task-1".into(),
        },
        EventPayloadReference {
            reference: "artifact:rd-1".into(),
            digest: digest.into(),
        },
        "corr-1".into(),
        StateEpochValueV1::try_from(0i64).unwrap(),
        None,
    )
    .unwrap()
}

pub(crate) fn event(event_type: EventType, digest: &str, id: &str) -> PublicationEventV1 {
    PublicationEventV1::new(proposal(event_type, digest), id).unwrap()
}

pub(crate) fn fact(kind: Kind, source: Source, follows: Option<u64>) -> AvailabilityFactV1 {
    AvailabilityFactV1::new(
        "p".into(),
        "anthropic".into(),
        "claude".into(),
        String::new(),
        kind,
        "PROVIDER_RATE_LIMITED".into(),
        source,
        "attempt:1".into(),
        follows,
    )
    .unwrap()
}

/// Routing decision + two events + a demotion followed by an unauthenticated claim.
pub(crate) fn routing_package(id: &str) -> DecisionPublicationV1 {
    DecisionPublicationV1::new(
        "p",
        id,
        vec![routing_record()],
        vec![
            event(EventType::RoutingDecided, "sha256:aa", EVENT_A),
            event(EventType::ExecutorSelected, "sha256:bb", EVENT_B),
        ],
        vec![
            fact(Kind::Demotion, Source::RuntimeFailureClass, None),
            fact(
                Kind::PromotionUnauthenticated,
                Source::UnauthenticatedClaim,
                Some(1),
            ),
        ],
    )
    .unwrap()
}

pub(crate) fn open_store(tag: &str) -> (Dir, TrustedCoreStore) {
    let dir = Dir::new(tag);
    let store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    (dir, store)
}

pub(crate) fn publish(
    store: &mut TrustedCoreStore,
    clock: &Clock,
    package: &DecisionPublicationV1,
) -> Result<DecisionPublicationReceiptV1, StateError> {
    let admission = store.admit(project_subject(Op::DecisionPublication, "p"));
    store.publish_decision_package_v1(admission, clock, package)
}

pub(crate) fn claim(
    store: &mut TrustedCoreStore,
    clock: &Clock,
    claim_id: &str,
) -> Result<ReviewClaimOutcomeV1, StateError> {
    let admission = store.admit(project_subject(Op::ReviewClaim, "p"));
    store.claim_review_v1(
        admission,
        clock,
        &ReviewClaimRequestV1::new("p", claim_id, "t-1", Some(A), &"b".repeat(64)).unwrap(),
    )
}

pub(crate) fn ticket(outcome: ReviewClaimOutcomeV1) -> ReviewClaimTicketV1 {
    match outcome {
        ReviewClaimOutcomeV1::Claimed(ticket) => ticket,
        other => panic!("expected a fresh claim, got {other:?}"),
    }
}

#[test]
fn decision_package_is_atomic_at_every_write_boundary() {
    let boundaries = assert_atomic_at_every_boundary(
        || {
            let (dir, mut store) = open_store("pkg-fault");
            let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
            (dir, store, t)
        },
        |(_, store, _)| fingerprint(store),
        |(_, store, t)| {
            let package = routing_package("pub-1")
                .with_claim_settlement(
                    t,
                    ReviewClaimSettlementV1::completed("SELECTED", Some("rd-1"), None).unwrap(),
                )
                .unwrap();
            publish(store, &T1, &package).map(|receipt| {
                assert!(!receipt.replayed);
                assert_eq!(receipt.events.len(), 2);
                assert_eq!(
                    receipt
                        .availability
                        .iter()
                        .map(|f| f.sequence)
                        .collect::<Vec<_>>(),
                    [1, 2]
                );
                assert!(receipt.claim.is_some());
            })
        },
    );
    // watermark, publication row, record, two events, two facts (each with a
    // child row) and the claim settlement all have their own boundary.
    assert!(boundaries >= 8, "{boundaries}");
}

#[test]
fn exact_retry_returns_the_recorded_outcome_across_reopen_and_conflicts_refuse() {
    let (dir, mut store) = open_store("pkg-retry");
    let first = publish(&mut store, &T1, &routing_package("pub-1")).unwrap();
    assert!(!first.replayed);
    let applied = fingerprint(&store);

    // Later clock, same package: the recorded outcome, nothing written.
    let again = publish(&mut store, &T2, &routing_package("pub-1")).unwrap();
    assert!(again.replayed);
    assert_eq!(again.recorded_at, first.recorded_at);
    assert_eq!(again.events, first.events);
    assert_eq!(again.availability, first.availability);
    assert_eq!(fingerprint(&store), applied, "no watermark or row moved");

    // Same identity, different content.
    let extra = DecisionPublicationV1::new(
        "p",
        "pub-1",
        vec![routing_record()],
        vec![event(EventType::RoutingDecided, "sha256:aa", EVENT_A)],
        vec![],
    )
    .unwrap();
    assert_eq!(
        publication_code(publish(&mut store, &T2, &extra).unwrap_err()),
        PCode::ConflictingRetry
    );
    // A new publication reusing a stored record, or a stored proposal.
    let reuse_record =
        DecisionPublicationV1::new("p", "pub-2", vec![routing_record()], vec![], vec![]).unwrap();
    assert!(matches!(
        publish(&mut store, &T2, &reuse_record).unwrap_err(),
        StateError::DecisionRecord {
            code: RCode::AlreadyExists,
            ..
        }
    ));
    let reuse_proposal = DecisionPublicationV1::new(
        "p",
        "pub-3",
        vec![],
        vec![event(EventType::RoutingDecided, "sha256:aa", EVENT_C)],
        vec![],
    )
    .unwrap();
    assert_eq!(
        graph_code(publish(&mut store, &T2, &reuse_proposal).unwrap_err()),
        GCode::AlreadyExists
    );
    assert_eq!(fingerprint(&store), applied);

    // Close/reopen: the same recorded outcome.
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let after_reopen = publish(&mut store, &T2, &routing_package("pub-1")).unwrap();
    assert!(after_reopen.replayed);
    assert_eq!(after_reopen.events, first.events);
    assert_eq!(after_reopen.availability, first.availability);
    assert_eq!(fingerprint(&store), applied);
}

#[test]
fn rejected_review_history_is_immutable_and_published_with_its_events() {
    let (dir, mut store) = open_store("pkg-reject");
    let reject = DecisionPublicationV1::new(
        "p",
        "pub-reject",
        vec![reject_review()],
        vec![event(EventType::ReviewRejected, "sha256:cc", EVENT_A)],
        vec![],
    )
    .unwrap();
    publish(&mut store, &T1, &reject).unwrap();
    let applied = fingerprint(&store);
    // A later PASS under the same review identity (new publication id, new event).
    let pass = DecisionPublicationV1::new(
        "p",
        "pub-pass",
        vec![pass_review()],
        vec![event(EventType::ReviewPassed, "sha256:dd", EVENT_B)],
        vec![],
    )
    .unwrap();
    assert!(matches!(
        publish(&mut store, &T2, &pass).unwrap_err(),
        StateError::DecisionRecord {
            code: RCode::AlreadyExists,
            ..
        }
    ));
    assert_eq!(
        fingerprint(&store),
        applied,
        "no PASS event or watermark leaked"
    );
    drop(store);
    let store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let stored = store
        .reader()
        .find_decision_record("p", DecisionRecordKind::A4Review, "rev-1")
        .unwrap()
        .unwrap();
    assert!(stored.record_json().contains(r#""verdict":"REJECT""#));
    assert!(store.reader().find_event(EVENT_A).unwrap().is_some());
    assert!(store.reader().find_event(EVENT_B).unwrap().is_none());
}

#[test]
fn record_present_without_events_from_the_older_writer_refuses_and_rolls_back() {
    // The reviewers' partial state: a record committed by the one-write
    // writer. The package must not half-publish around it.
    let (_dir, mut store) = open_store("pkg-legacy");
    let admission = store.admit(project_subject(Op::DecisionRecord, "p"));
    store
        .append_decision_record(admission, &reject_review())
        .unwrap();
    let before = fingerprint(&store);
    let package = DecisionPublicationV1::new(
        "p",
        "pub-reject",
        vec![reject_review()],
        vec![event(EventType::ReviewRejected, "sha256:cc", EVENT_A)],
        vec![],
    )
    .unwrap();
    assert!(matches!(
        publish(&mut store, &T1, &package).unwrap_err(),
        StateError::DecisionRecord {
            code: RCode::AlreadyExists,
            ..
        }
    ));
    assert_eq!(fingerprint(&store), before);
}

#[test]
fn package_authority_project_and_shape_mismatches_write_nothing() {
    let (dir, mut store) = open_store("pkg-auth");
    let before = fingerprint(&store);
    let package = routing_package("pub-1");
    for (subject, expected) in [
        (
            project_subject(Op::DecisionPublication, "q"),
            R::ProjectMismatch,
        ),
        (project_subject(Op::DecisionRecord, "p"), R::WrongOperation),
        (project_subject(Op::EventProposal, "p"), R::WrongOperation),
        (
            project_subject(Op::AvailabilityFact, "p"),
            R::WrongOperation,
        ),
    ] {
        let admission = store.admit(subject);
        assert_eq!(
            refusal(
                store
                    .publish_decision_package_v1(admission, &T1, &package)
                    .unwrap_err()
            ),
            expected
        );
        assert_eq!(fingerprint(&store), before);
    }
    // Items from another project, an empty package and repeated items.
    let other_event = EventProposalV1::new(
        EventProposalOriginV1::Routing,
        "q".into(),
        None,
        EventType::RoutingDecided,
        EventActor {
            kind: ActorKind::System,
            id: None,
        },
        EventSubject {
            kind: SubjectKind::Task,
            id: "t".into(),
        },
        EventPayloadReference {
            reference: "r".into(),
            digest: "d".into(),
        },
        "c".into(),
        StateEpochValueV1::try_from(0i64).unwrap(),
        None,
    )
    .unwrap();
    for bad in [
        DecisionPublicationV1::new(
            "p",
            "x",
            vec![],
            vec![PublicationEventV1::new(other_event, EVENT_A).unwrap()],
            vec![],
        ),
        DecisionPublicationV1::new("p", "x", vec![], vec![], vec![]),
        DecisionPublicationV1::new(
            "p",
            "x",
            vec![routing_record(), routing_record()],
            vec![],
            vec![],
        ),
        DecisionPublicationV1::new(
            "p",
            "x",
            vec![],
            vec![
                event(EventType::RoutingDecided, "sha256:aa", EVENT_A),
                event(EventType::RoutingDecided, "sha256:bb", EVENT_A),
            ],
            vec![],
        ),
    ] {
        assert_eq!(publication_code(bad.unwrap_err()), PCode::InvalidShape);
    }
    // A malformed event id is refused when the event is built.
    assert!(
        PublicationEventV1::new(
            proposal(EventType::RoutingDecided, "sha256:aa"),
            "not-a-ulid"
        )
        .is_err()
    );
    // An admission from before a reopen never transfers.
    let stale = store.admit(project_subject(Op::DecisionPublication, "p"));
    drop(store);
    let mut reopened = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert_eq!(
        refusal(
            reopened
                .publish_decision_package_v1(stale, &T1, &package)
                .unwrap_err()
        ),
        R::WrongStore
    );
    // A claim ticket for another project cannot join a package.
    let mut other = TrustedCoreStore::open_exclusive(Dir::new("pkg-auth-other").db()).unwrap();
    let admission = other.admit(project_subject(Op::ReviewClaim, "q"));
    let foreign = ticket(
        other
            .claim_review_v1(
                admission,
                &G0,
                &ReviewClaimRequestV1::new("q", "c", "t", None, &"b".repeat(64)).unwrap(),
            )
            .unwrap(),
    );
    assert_eq!(
        publication_code(
            routing_package("pub-1")
                .with_claim_settlement(
                    &foreign,
                    ReviewClaimSettlementV1::completed("SELECTED", None, None).unwrap()
                )
                .unwrap_err()
        ),
        PCode::InvalidShape
    );
    // A ticket from another live store instance is a wrong-store refusal.
    let own = ticket(claim(&mut reopened, &G0, "claim-1").unwrap());
    drop(reopened);
    let mut again = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let with_old_ticket = routing_package("pub-1")
        .with_claim_settlement(
            &own,
            ReviewClaimSettlementV1::completed("SELECTED", None, None).unwrap(),
        )
        .unwrap();
    let before = fingerprint(&again);
    assert_eq!(
        refusal(publish(&mut again, &T1, &with_old_ticket).unwrap_err()),
        R::WrongStore
    );
    assert_eq!(fingerprint(&again), before);
}

#[test]
fn package_claim_settlement_replays_with_the_claim_and_refuses_a_foreign_outcome() {
    let (_dir, mut store) = open_store("pkg-claim");
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let settled = ReviewClaimSettlementV1::completed("REJECT", Some("rev-1"), None).unwrap();
    let package = DecisionPublicationV1::new(
        "p",
        "pub-reject",
        vec![reject_review()],
        vec![event(EventType::ReviewRejected, "sha256:cc", EVENT_A)],
        vec![],
    )
    .unwrap()
    .with_claim_settlement(&t, settled.clone())
    .unwrap();
    let first = publish(&mut store, &T1, &package).unwrap();
    let record = first.claim.clone().unwrap();
    assert_eq!(record.outcome_kind.as_deref(), Some("REJECT"));
    let replay = publish(&mut store, &T2, &package).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.claim, first.claim);
    // A different settlement is a different package digest.
    let other = DecisionPublicationV1::new(
        "p",
        "pub-reject",
        vec![reject_review()],
        vec![event(EventType::ReviewRejected, "sha256:cc", EVENT_A)],
        vec![],
    )
    .unwrap()
    .with_claim_settlement(
        &t,
        ReviewClaimSettlementV1::completed("PASS", Some("rev-1"), None).unwrap(),
    )
    .unwrap();
    assert_eq!(
        publication_code(publish(&mut store, &T2, &other).unwrap_err()),
        PCode::ConflictingRetry
    );
}

#[test]
fn v20_store_migrates_to_v21_and_a_failed_step_leaves_exact_v20() {
    let dir = Dir::new("migrate21");
    drop(
        crate::SqliteStateRepository::open_with_migrations(
            dir.db(),
            &crate::migrations::registered()[..20],
        )
        .unwrap(),
    );
    std::fs::set_permissions(dir.db(), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        TrustedCoreStore::open_exclusive(dir.db()).is_err(),
        "ordinary open never upgrades"
    );
    let conn = Connection::open(dir.db()).unwrap();
    crate::graph_repository::verify_v20(&conn, crate::GraphPhase::Read).unwrap();
    conn.execute_batch("CREATE TRIGGER fail21 BEFORE INSERT ON state_schema_version WHEN NEW.version=21 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(TrustedCoreStore::migrate_exclusive(dir.db()).is_err());
    crate::graph_repository::verify_v20(&conn, crate::GraphPhase::Read).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('atomic_publication','atomic_publication_availability','review_claim')", [], |r| r.get::<_, i64>(0)).unwrap(),
        0
    );
    conn.execute_batch("DROP TRIGGER fail21").unwrap();
    drop(conn);
    let (store, version) = TrustedCoreStore::migrate_exclusive(dir.db()).unwrap();
    assert_eq!(version, 23);
    assert_eq!(store.reader().schema_version().unwrap(), 23);
    drop(store);
    // Idempotent on a current store.
    let (store, version) = TrustedCoreStore::migrate_exclusive(dir.db()).unwrap();
    assert_eq!(version, 23);
    drop(store);
    // A removed v21 guard is detected as corruption on open.
    let conn = Connection::open(dir.db()).unwrap();
    conn.execute_batch("DROP TRIGGER review_claim_leaves_claimed_once")
        .unwrap();
    drop(conn);
    assert!(TrustedCoreStore::open_exclusive(dir.db()).is_err());
}

#[test]
fn publication_tables_reject_direct_updates_deletes_and_replacements() {
    let (_dir, mut store) = open_store("pkg-sql");
    publish(&mut store, &T1, &routing_package("pub-1")).unwrap();
    let conn = store.reader().connection();
    for sql in [
        "UPDATE atomic_publication SET package_sha256='00'",
        "DELETE FROM atomic_publication",
        "INSERT OR REPLACE INTO atomic_publication SELECT * FROM atomic_publication",
        "UPDATE atomic_publication_availability SET sequence=9",
        "DELETE FROM atomic_publication_availability",
        "INSERT OR REPLACE INTO atomic_publication_availability SELECT * FROM atomic_publication_availability",
    ] {
        assert!(conn.execute_batch(sql).is_err(), "{sql}");
    }
    assert_eq!(rows(&store, "atomic_publication"), 1);
    assert_eq!(rows(&store, "atomic_publication_availability"), 2);
}

#[test]
fn repair_graph_replay_binds_complete_attachment_set() {
    let (dir, mut store) = graph_store("repair-membership");
    publish_graph(&mut store, &G1, &publication("m2", "attach", "PASS")).unwrap();
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert_eq!(
        publish_graph(&mut store, &T1, &publication("m2", "attach", "PASS")).unwrap(),
        GraphPublicationOutcomeV1::Replayed
    );
    for refs in [vec![], vec!["r1"], vec!["r1", "r2", "r3"]] {
        let p = GraphMutationPublicationV1::new(
            refs.iter()
                .map(|r| {
                    GraphResultAttachmentV1::new(
                        "1",
                        r,
                        result_json(if *r == "r1" { "PASS" } else { "FAIL" }),
                    )
                    .unwrap()
                })
                .collect(),
            StateGraphMutationV1::parse_json(&mutation_json("m2", "1", "2", "attach")).unwrap(),
            StateGraphSnapshotV1::new("2".into(), "1".into(), content(Some("r1"))).unwrap(),
        )
        .unwrap();
        let before = fingerprint(&store);
        assert_eq!(
            publication_code(publish_graph(&mut store, &T1, &p).unwrap_err()),
            PCode::ConflictingRetry
        );
        assert_eq!(before, fingerprint(&store));
    }
}

#[test]
fn repair_v22_upgrade_keeps_legacy_membership_unknown_and_guards_identity() {
    let (dir, mut store) = graph_store("repair-upgrade");
    publish_graph(&mut store, &G1, &publication("m2", "attach", "PASS")).unwrap();
    drop(store);
    let conn = Connection::open(dir.db()).unwrap();
    conn.execute_batch(
        "DROP TABLE graph_publication_identity; DELETE FROM state_schema_version WHERE version=23;",
    )
    .unwrap();
    crate::graph_repository::verify_v22(&conn, crate::GraphPhase::Read).unwrap();
    conn.execute_batch("CREATE TRIGGER fail23 BEFORE INSERT ON state_schema_version WHEN NEW.version=23 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(TrustedCoreStore::migrate_exclusive(dir.db()).is_err());
    crate::graph_repository::verify_v22(&conn, crate::GraphPhase::Read).unwrap();
    conn.execute_batch("DROP TRIGGER fail23").unwrap();
    drop(conn);
    let (mut store, version) = TrustedCoreStore::migrate_exclusive(dir.db()).unwrap();
    assert_eq!(version, 23);
    let before = fingerprint(&store);
    assert_eq!(
        publication_code(
            publish_graph(&mut store, &T1, &publication("m2", "attach", "PASS")).unwrap_err()
        ),
        PCode::ConflictingRetry
    );
    assert_eq!(before, fingerprint(&store));
    drop(store);
    let conn = Connection::open(dir.db()).unwrap();
    conn.execute_batch("DROP TRIGGER graph_publication_identity_no_update")
        .unwrap();
    drop(conn);
    assert!(TrustedCoreStore::open_exclusive(dir.db()).is_err());
}
