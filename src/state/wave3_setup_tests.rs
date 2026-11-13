use super::wave3_setup::*;
use crate::atomic_publication_tests::{Dir, G0};
use crate::write_fault::with_fault;
use crate::*;
use StateWriteOperationV1 as Op;
const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
fn subject(op: Op, project: &str, graph: Option<&str>) -> StateWriteSubjectV1 {
    StateWriteSubjectV1 {
        operation: op,
        project_id: project.into(),
        graph_id: graph.map(str::to_owned),
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    }
}
fn admission(store: &TrustedCoreStore, op: Op) -> StateWriteAdmission {
    store.admit(subject(op, "p", Some("g")))
}
fn seed() -> (Dir, TrustedCoreStore) {
    seed_with_limit("2")
}
fn seed_with_limit(limit: &str) -> (Dir, TrustedCoreStore) {
    let dir = Dir::new("wave3");
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let request = crate::graph_repository_tests::request("p", "g");
    let provenance = request.genesis_provenance();
    let mut auth = subject(Op::GraphGenesis, "p", Some("g"));
    auth.compiler_id = Some(provenance.compiler_id().as_str().into());
    auth.source_ref = Some(provenance.source_ref().as_str().into());
    auth.policy_id = Some("policy".into());
    let auth = store.admit(auth);
    store.create_initial_graph_v1(auth, &G0, request).unwrap();
    epoch(&mut store, "0");
    let auth = admission(&store, Op::Goal);
    let mut definition = definition();
    definition.dispatch_limit = limit.into();
    store.create_goal_v1(auth, &definition, "0").unwrap();
    (dir, store)
}
fn epoch(store: &mut TrustedCoreStore, value: &str) {
    let auth = store.admit(subject(Op::ContextEpoch, "p", None));
    store
        .append_context_epoch(
            auth,
            ContextEpoch {
                project_id: "p".into(),
                epoch: StateEpochValueV1::try_from(value).unwrap(),
                advanced_at: "opaque".into(),
                trigger: ContextEpochTrigger::A1Init,
            },
        )
        .unwrap();
}
fn definition() -> GoalDefinitionV1 {
    GoalDefinitionV1 {
        project_id: "p".into(),
        goal_id: "goal".into(),
        graph_id: "g".into(),
        spec_ref: "artifact:verbatim".into(),
        spec_sha256: "b".repeat(64),
        dispatch_limit: "2".into(),
    }
}
fn dispatch(id: &str, revision: &str) -> GoalDispatchV1 {
    GoalDispatchV1 {
        project_id: "p".into(),
        goal_id: "goal".into(),
        dispatch_id: id.into(),
        expected_revision: revision.into(),
        context_epoch: "0".into(),
        graph_version: "1".into(),
    }
}
fn start(id: &str, node: &str) -> AttemptStart {
    AttemptStart::new(
        AttemptId::parse(id).unwrap(),
        "p",
        "g",
        node,
        "1",
        AttemptRole::RuntimeA3,
        "runtime",
        "w",
        &"a".repeat(40),
        "2026-11-08T10:00:00.000000000Z",
    )
    .unwrap()
}
fn reserve(
    store: &mut TrustedCoreStore,
    request: &GoalDispatchV1,
    start: &AttemptStart,
) -> Result<SetupWriteOutcomeV1, StateError> {
    let auth = admission(store, Op::GoalDispatch);
    store.reserve_goal_dispatch_v1(auth, request, start)
}
fn reason(error: StateError) -> SetupRefusalV1 {
    match error {
        StateError::SetupRefused { reason } => reason,
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn reservation_reopen_replay_concurrent_nodes_and_unknown_launch_block_replacement() {
    let (dir, mut store) = seed();
    let req = dispatch("d", "0");
    let a = start(A, "n2");
    assert_eq!(
        reserve(&mut store, &req, &a).unwrap(),
        SetupWriteOutcomeV1::Applied
    );
    let stale = admission(&store, Op::GoalDispatch);
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert!(matches!(
        store.reserve_goal_dispatch_v1(stale, &req, &a),
        Err(StateError::WriteAuthorityRefused {
            reason: StateWriteRefusalV1::WrongStore
        })
    ));
    assert_eq!(
        reserve(&mut store, &req, &a).unwrap(),
        SetupWriteOutcomeV1::Replayed
    );
    let goal = store.reader().find_goal_v1("p", "goal").unwrap().unwrap();
    assert_eq!(goal.dispatches_used, "1");
    assert_eq!(goal.revision, "1");
    assert_eq!(goal.definition.spec_ref, "artifact:verbatim");
    let record = store
        .reader()
        .find_goal_dispatch_v1("p", "d")
        .unwrap()
        .unwrap();
    assert_eq!(record.resolution, None);
    assert_eq!(
        reason(reserve(&mut store, &dispatch("replace", "1"), &start(B, "n2")).unwrap_err()),
        SetupRefusalV1::UnresolvedLaunch
    );
    assert_eq!(
        reserve(&mut store, &dispatch("parallel", "1"), &start(B, "n\0😀")).unwrap(),
        SetupWriteOutcomeV1::Applied
    );
    let auth = admission(&store, Op::GoalDispatch);
    store
        .resolve_goal_dispatch_v1(
            auth,
            "p",
            "d",
            LaunchResolutionV1::NotLaunched,
            "artifact:launch-absent",
        )
        .unwrap();
    assert_eq!(
        reason(reserve(&mut store, &dispatch("over", "2"), &start(C, "n2")).unwrap_err()),
        SetupRefusalV1::BudgetExhausted
    );
    assert_eq!(
        store.reader().list_unfinished_attempts("p").unwrap().len(),
        2
    );
}

#[test]
fn stale_foreign_conflicting_and_malformed_reservations_write_nothing() {
    let (_dir, mut store) = seed();
    let req = dispatch("d", "0");
    let a = start(A, "n2");
    let mut bad = req.clone();
    bad.expected_revision = "1".into();
    assert_eq!(
        reason(reserve(&mut store, &bad, &a).unwrap_err()),
        SetupRefusalV1::StaleRevision
    );
    bad = req.clone();
    bad.graph_version = "2".into();
    assert_eq!(
        reason(reserve(&mut store, &bad, &a).unwrap_err()),
        SetupRefusalV1::ForeignEvidence
    );
    bad = req.clone();
    bad.project_id = "foreign".into();
    assert!(reserve(&mut store, &bad, &a).is_err());
    bad = req.clone();
    bad.expected_revision = "01".into();
    assert_eq!(
        reason(reserve(&mut store, &bad, &a).unwrap_err()),
        SetupRefusalV1::InvalidShape
    );
    assert!(
        store
            .reader()
            .find_attempt(a.attempt_id())
            .unwrap()
            .is_none()
    );
    reserve(&mut store, &req, &a).unwrap();
    assert_eq!(
        reason(reserve(&mut store, &req, &start(B, "n2")).unwrap_err()),
        SetupRefusalV1::ConflictingReplay
    );
    epoch(&mut store, "1");
    assert_eq!(
        reason(reserve(&mut store, &dispatch("stale", "1"), &start(B, "n\0😀")).unwrap_err()),
        SetupRefusalV1::StaleEpoch
    );
    let progress = GoalProgressV1 {
        project_id: "p".into(),
        goal_id: "goal".into(),
        expected_revision: "1".into(),
        expected_epoch: "0".into(),
        context_epoch: "1".into(),
        continuation_ref: Some("artifact:continuation".into()),
    };
    let auth = admission(&store, Op::Goal);
    store.advance_goal_v1(auth, &progress).unwrap();
    let auth = admission(&store, Op::Goal);
    assert_eq!(
        store.advance_goal_v1(auth, &progress).unwrap(),
        SetupWriteOutcomeV1::Replayed
    );
    assert_eq!(
        store
            .reader()
            .find_goal_v1("p", "goal")
            .unwrap()
            .unwrap()
            .context_epoch,
        "1"
    );
}

#[test]
fn every_reservation_write_failure_rolls_back_start_budget_and_reservation() {
    for boundary in 1..=3 {
        let (_dir, mut store) = seed();
        let (result, seen) = with_fault(Some(boundary), || {
            reserve(&mut store, &dispatch("d", "0"), &start(A, "n2"))
        });
        assert!(result.is_err());
        assert_eq!(seen, boundary);
        assert!(
            store
                .reader()
                .find_attempt(&AttemptId::parse(A).unwrap())
                .unwrap()
                .is_none()
        );
        assert!(
            store
                .reader()
                .find_goal_dispatch_v1("p", "d")
                .unwrap()
                .is_none()
        );
        let goal = store.reader().find_goal_v1("p", "goal").unwrap().unwrap();
        assert_eq!(goal.revision, "0");
        assert_eq!(goal.dispatches_used, "0");
        reserve(&mut store, &dispatch("d", "0"), &start(A, "n2")).unwrap();
    }
}
fn finalization(store: &mut TrustedCoreStore) -> (FinalizationRecordV1, AttemptFinish) {
    reserve(store, &dispatch("d", "0"), &start(A, "n2")).unwrap();
    let check = serde_json::json!({"source":"WORKER_EXECUTION","command":["check"],"exit_code":0,"code_sha":"a".repeat(40),"result":"PASS"});
    let checkpoint = serde_json::json!({"checkpoint_id":"cp","workspace_id":"w","attempt_id":A,"head_sha":"a".repeat(40),"captured_at":"2026-11-08T10:01:00Z","kind":"PROGRESS","executed_checks":[check.clone()]});
    let checkpoint = CheckpointRecord::parse_json("p", &checkpoint.to_string()).unwrap();
    let auth = store.admit(subject(Op::Checkpoint, "p", None));
    store.append_checkpoint(auth, checkpoint).unwrap();
    let auth = store.admit(subject(Op::OutputRef, "p", None));
    store
        .append_output_ref(
            auth,
            OutputRef::new("p", "stdout", &"a".repeat(64), "artifact:stdout", "3").unwrap(),
        )
        .unwrap();
    let record = FinalizationRecordV1 {
        project_id: "p".into(),
        graph_id: "g".into(),
        attempt_id: A.into(),
        workspace_id: "w".into(),
        checkpoint_id: "cp".into(),
        final_sha: "a".repeat(40),
        write_scope_verdict: "PASS".into(),
        check_results: vec![check],
        output_refs: vec!["stdout".into()],
    };
    let finish = AttemptFinish::new(
        AttemptId::parse(A).unwrap(),
        "2026-11-08T10:02:00.000000000Z",
        Some(&"a".repeat(40)),
        Some("artifact:finalization"),
        None,
    )
    .unwrap();
    (record, finish)
}
fn finish(
    store: &mut TrustedCoreStore,
    record: &FinalizationRecordV1,
    finish: &AttemptFinish,
) -> Result<SetupWriteOutcomeV1, StateError> {
    let auth = admission(store, Op::Finalization);
    store.finish_with_finalization_v1(auth, record, finish)
}
#[test]
fn finalization_and_successful_finish_are_atomic_replayable_and_scope_bound() {
    let (dir, mut store) = seed();
    let (record, result) = finalization(&mut store);
    for boundary in 1..=2 {
        let (failed, seen) = with_fault(Some(boundary), || finish(&mut store, &record, &result));
        assert!(failed.is_err());
        assert_eq!(seen, boundary);
        assert!(
            !store
                .reader()
                .find_attempt(result.attempt_id())
                .unwrap()
                .unwrap()
                .is_finished()
        );
        assert!(
            store
                .reader()
                .find_finalization_v1("p", result.attempt_id())
                .unwrap()
                .is_none()
        );
    }
    for field in [
        "workspace",
        "graph",
        "checkpoint",
        "checks",
        "outputs",
        "sha",
    ] {
        let mut bad = record.clone();
        match field {
            "workspace" => bad.workspace_id = "other".into(),
            "graph" => bad.graph_id = "foreign".into(),
            "checkpoint" => bad.checkpoint_id = "missing".into(),
            "checks" => bad.check_results.clear(),
            "outputs" => bad.output_refs = vec!["foreign".into()],
            _ => bad.final_sha = "b".repeat(40),
        }
        assert!(finish(&mut store, &bad, &result).is_err());
        assert!(
            !store
                .reader()
                .find_attempt(result.attempt_id())
                .unwrap()
                .unwrap()
                .is_finished()
        );
    }
    assert_eq!(
        finish(&mut store, &record, &result).unwrap(),
        SetupWriteOutcomeV1::Applied
    );
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert_eq!(
        finish(&mut store, &record, &result).unwrap(),
        SetupWriteOutcomeV1::Replayed
    );
    assert!(
        store
            .reader()
            .find_finalization_v1("p", result.attempt_id())
            .unwrap()
            .unwrap()
            == record
    );
    let mut conflict = record.clone();
    conflict.output_refs.clear();
    assert_eq!(
        reason(finish(&mut store, &conflict, &result).unwrap_err()),
        SetupRefusalV1::ConflictingReplay
    );
}

#[test]
fn goal_definition_is_immutable_and_budget_domain_is_not_a_machine_integer() {
    let (_dir, mut store) = seed();
    let mut changed = definition();
    changed.spec_ref = "different".into();
    let auth = admission(&store, Op::Goal);
    assert_eq!(
        reason(store.create_goal_v1(auth, &changed, "0").unwrap_err()),
        SetupRefusalV1::ConflictingReplay
    );
    let mut large = definition();
    large.goal_id = "goal2".into();
    large.dispatch_limit = "184467440737095516160000".into();
    // A foreign goal binding is refused even when its counter shape is valid.
    let auth = admission(&store, Op::Goal);
    assert_eq!(
        reason(store.create_goal_v1(auth, &large, "0").unwrap_err()),
        SetupRefusalV1::ForeignEvidence
    );
    assert_eq!(
        store
            .reader()
            .find_goal_v1("p", "goal")
            .unwrap()
            .unwrap()
            .revision,
        "0"
    );
}

/// Historical migration fixtures remove successors before testing old heads.
pub(crate) fn remove_v24(conn: &rusqlite::Connection) {
    conn.execute_batch("DROP TABLE IF EXISTS remote_publication_result; DROP TABLE IF EXISTS remote_publication_intent; DELETE FROM state_schema_version WHERE version>=25; DROP TABLE IF EXISTS attempt_finalization; DROP TABLE IF EXISTS goal_dispatch_resolution; DROP TABLE IF EXISTS goal_dispatch; DROP TABLE IF EXISTS goal_history; DELETE FROM state_schema_version WHERE version=24;").unwrap();
}

#[test]
fn v24_upgrade_is_forward_only_atomic_and_exactly_guarded() {
    use std::os::unix::fs::PermissionsExt;
    let dir = Dir::new("v24-migration");
    let repo = SqliteStateRepository::open_with_migrations(
        dir.db(),
        &crate::migrations::registered()[..23],
    )
    .unwrap();
    repo.connection().execute_batch("INSERT INTO logical_role(role_id,project_id,role_type,status,current_context_epoch) VALUES('sentinel','p','RUNTIME_A1','ACTIVE','0')").unwrap();
    drop(repo);
    std::fs::set_permissions(dir.db(), std::fs::Permissions::from_mode(0o600)).unwrap();
    let conn = rusqlite::Connection::open(dir.db()).unwrap();
    conn.execute_batch("CREATE TRIGGER fail24 BEFORE INSERT ON state_schema_version WHEN NEW.version=24 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(TrustedCoreStore::migrate_exclusive(dir.db()).is_err());
    crate::graph_repository::verify_v23(&conn, GraphPhase::Read).unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('goal_history','goal_dispatch','goal_dispatch_resolution','attempt_finalization')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    conn.execute_batch("DROP TRIGGER fail24").unwrap();
    drop(conn);
    let (store, version) = TrustedCoreStore::migrate_exclusive(dir.db()).unwrap();
    assert_eq!(version, 26);
    assert!(
        store
            .reader()
            .find_logical_role("sentinel")
            .unwrap()
            .is_some()
    );
    drop(store);
    let (mut store, version) = TrustedCoreStore::migrate_exclusive(dir.db()).unwrap();
    assert_eq!(version, 26);
    let conn = rusqlite::Connection::open(dir.db()).unwrap();
    for table in [
        "goal_history",
        "goal_dispatch",
        "goal_dispatch_resolution",
        "attempt_finalization",
    ] {
        assert_eq!(
            conn.query_row(
                "SELECT wr FROM pragma_table_list WHERE name=?1",
                [table],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    conn.execute_batch("DROP TRIGGER goal_history_no_update")
        .unwrap();
    assert!(store.reader().find_goal_v1("p", "goal").is_err());
    let auth = admission(&store, Op::Goal);
    assert!(store.create_goal_v1(auth, &definition(), "0").is_err());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM goal_history", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn unbounded_and_zero_dispatch_budgets_and_admission_bindings() {
    let (_dir, mut store) = seed_with_limit("184467440737095516160000");
    let wrong = store.admit(subject(Op::GoalDispatch, "foreign", Some("g")));
    assert!(matches!(
        store.reserve_goal_dispatch_v1(wrong, &dispatch("d", "0"), &start(A, "n2")),
        Err(StateError::WriteAuthorityRefused {
            reason: StateWriteRefusalV1::ProjectMismatch
        })
    ));
    let wrong = store.admit(subject(Op::Attempt, "p", Some("g")));
    assert!(matches!(
        store.reserve_goal_dispatch_v1(wrong, &dispatch("d", "0"), &start(A, "n2")),
        Err(StateError::WriteAuthorityRefused {
            reason: StateWriteRefusalV1::WrongOperation
        })
    ));
    reserve(&mut store, &dispatch("d", "0"), &start(A, "n2")).unwrap();
    assert_eq!(
        store
            .reader()
            .find_goal_v1("p", "goal")
            .unwrap()
            .unwrap()
            .definition
            .dispatch_limit,
        "184467440737095516160000"
    );
    let auth = admission(&store, Op::GoalDispatch);
    assert_eq!(
        store
            .resolve_goal_dispatch_v1(
                auth,
                "p",
                "d",
                LaunchResolutionV1::Uncertain,
                "artifact:interrupted"
            )
            .unwrap(),
        SetupWriteOutcomeV1::Applied
    );
    let auth = admission(&store, Op::GoalDispatch);
    assert_eq!(
        store
            .resolve_goal_dispatch_v1(
                auth,
                "p",
                "d",
                LaunchResolutionV1::Uncertain,
                "artifact:interrupted"
            )
            .unwrap(),
        SetupWriteOutcomeV1::Replayed
    );
    let auth = admission(&store, Op::GoalDispatch);
    assert_eq!(
        reason(
            store
                .resolve_goal_dispatch_v1(
                    auth,
                    "p",
                    "d",
                    LaunchResolutionV1::Launched,
                    "artifact:changed"
                )
                .unwrap_err()
        ),
        SetupRefusalV1::ConflictingReplay
    );
    assert_eq!(
        reason(reserve(&mut store, &dispatch("again", "1"), &start(B, "n2")).unwrap_err()),
        SetupRefusalV1::UnresolvedLaunch
    );
    let (_dir, mut store) = seed_with_limit("0");
    assert_eq!(
        reason(reserve(&mut store, &dispatch("d", "0"), &start(A, "n2")).unwrap_err()),
        SetupRefusalV1::BudgetExhausted
    );
    assert!(
        store
            .reader()
            .list_unfinished_attempts("p")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn selected_goal_projection_refuses_physical_revision_drift() {
    let (dir, store) = seed();
    let conn = rusqlite::Connection::open(dir.db()).unwrap();
    conn.execute_batch("INSERT INTO goal_history SELECT project_id,goal_id,'9',record_json FROM goal_history WHERE revision='0'").unwrap();
    assert!(matches!(
        store.reader().find_goal_v1("p", "goal"),
        Err(StateError::GraphPersistence {
            code: GraphFailureCode::CorruptStore,
            ..
        })
    ));
}
