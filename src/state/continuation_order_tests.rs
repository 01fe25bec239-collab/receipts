//! Continuation invalidation controls (ST-W2 remaining item 1).
//!
//! An earlier unchanged SUCCEEDED reread must never clear continuation once a
//! later FAILED or CHANGED reread exists for the same project / role /
//! binding / manifest / epoch. "Later" is State's own durable commit order
//! (never attempt ids or caller-supplied timestamps), and a success whose
//! read began before a newer invalidation committed never clears.

use crate::context_epoch::ContextEpochTrigger;
use crate::context_rehydration::{
    ArtifactMaterializer, ArtifactRefV1, ContextRehydrationAttempt, ContextRehydrationStatus,
    ExternalSourceMaterialization, SourceDigestComparison, SourceMaterializationFailure,
};
use crate::logical_role::LogicalRoleType;
use crate::recovery::{ChangedSourceSuccessionRequestV1, RecoveryRefusalV1 as R};
use crate::recovery_tests::{
    ARTIFACT_BYTES, Artifacts, NoRepository, PROJECT, Supplier, T_BEFORE, T_BOUND, authority,
    digest_of, rehydration_request, seeded,
};
use crate::repository::SqliteStateRepository;
use crate::tests::{TempDir, trusted_clock_at};

const CHANGED_BYTES: &[u8] = b"authoritative architecture v2";

/// A unique, canonical-ULID-shaped success event id per call.
fn event_id() -> &'static str {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Box::leak(format!("01ARZ3NDEKTSV4RRFFQ69G{n:04}").into_boxed_str())
}

/// Always fails to read the artifact (an unreadable authoritative source).
struct Unreadable;
impl ArtifactMaterializer for Unreadable {
    fn materialize(
        &mut self,
        _reference: &ArtifactRefV1,
        _accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        Err(SourceMaterializationFailure {
            code: "ARTIFACT_UNREADABLE".into(),
            materializer_id: Some("test-artifacts".into()),
            provenance: Some("test".into()),
            materialized_at: Some(T_BEFORE.into()),
            failure_detail: None,
        })
    }
}

fn digest() -> String {
    digest_of("artifact-arch", ARTIFACT_BYTES)
}

fn good(
    repo: &mut SqliteStateRepository,
    attempt_id: &str,
    manifest: &str,
    epoch: i64,
    binding: &str,
) -> ContextRehydrationAttempt {
    read_with(
        repo,
        rehydration_request(attempt_id, manifest, epoch, binding),
        &mut Artifacts(ARTIFACT_BYTES.to_vec()),
    )
}

fn changed(repo: &mut SqliteStateRepository, attempt_id: &str) -> ContextRehydrationAttempt {
    let attempt = read_with(
        repo,
        rehydration_request(attempt_id, "manifest-role-1", 0, "b1"),
        &mut Artifacts(CHANGED_BYTES.to_vec()),
    );
    assert_eq!(
        attempt.source_evidence[0].comparison,
        SourceDigestComparison::Changed
    );
    attempt
}

fn unreadable(repo: &mut SqliteStateRepository, attempt_id: &str) -> ContextRehydrationAttempt {
    let attempt = read_with(
        repo,
        rehydration_request(attempt_id, "manifest-role-1", 0, "b1"),
        &mut Unreadable,
    );
    assert_eq!(attempt.status, ContextRehydrationStatus::Failed);
    attempt
}

fn read_with(
    repo: &mut SqliteStateRepository,
    request: crate::ContextRehydrationRequest,
    materializer: &mut impl ArtifactMaterializer,
) -> ContextRehydrationAttempt {
    repo.rehydrate_context(
        request,
        &mut NoRepository,
        materializer,
        &mut Supplier(event_id()),
    )
    .unwrap()
    .attempt
}

fn clear(repo: &SqliteStateRepository) -> Result<crate::ContinuationClearanceV1, R> {
    repo.continuation_clearance(&trusted_clock_at(T_BEFORE), "role-1")
        .map_err(|e| match e {
            crate::StateError::RecoveryRefused { reason, .. } => reason,
            other => panic!("expected a recovery refusal, got {other:?}"),
        })
}

fn seeded_a2(tag: &str) -> (TempDir, SqliteStateRepository) {
    seeded(tag, LogicalRoleType::RuntimeA2, &digest())
}

/// Reviewer regression A: an older unchanged success, then a successful
/// reread that observed CHANGED source evidence, must be CHANGED_SOURCES_PENDING.
#[test]
fn older_success_then_changed_reread_is_changed_sources_pending() {
    let (_tmp, mut repo) = seeded_a2("co-regression-a");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    assert_eq!(
        clear(&repo).unwrap().rehydration_attempt_id,
        "attempt-1",
        "the lone success clears"
    );
    changed(&mut repo, "attempt-2");
    assert_eq!(clear(&repo), Err(R::ChangedSourcesPending));
}

/// Reviewer regression B: an older unchanged success, then a failed
/// (unreadable) reread, must refuse continuation.
#[test]
fn older_success_then_failed_reread_refuses_continuation() {
    let (_tmp, mut repo) = seeded_a2("co-regression-b");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    assert!(clear(&repo).is_ok());
    unreadable(&mut repo, "attempt-2");
    assert_eq!(clear(&repo), Err(R::RehydrationRequired));
}

/// Order is State's commit order, not the sort order of caller-chosen ids:
/// the invalidating attempt has the lexicographically SMALLEST id.
#[test]
fn reverse_sorted_ids_do_not_decide_order() {
    let (_tmp, mut repo) = seeded_a2("co-reverse-ids");
    good(&mut repo, "z-attempt", "manifest-role-1", 0, "b1");
    unreadable(&mut repo, "a-attempt");
    assert_eq!(clear(&repo), Err(R::RehydrationRequired));
    // And the reverse: an old failure under a large id, a fresh success under a
    // small id, restores clearance.
    let (_tmp, mut repo) = seeded_a2("co-reverse-ids-2");
    unreadable(&mut repo, "z-attempt");
    assert_eq!(clear(&repo), Err(R::RehydrationRequired));
    good(&mut repo, "a-attempt", "manifest-role-1", 0, "b1");
    assert_eq!(clear(&repo).unwrap().rehydration_attempt_id, "a-attempt");
}

/// Caller-supplied started_at / completed_at are untrusted: equal stamps and
/// stamps that point the wrong way change nothing.
#[test]
fn equal_and_misleading_timestamps_do_not_decide_order() {
    // Equal stamps on both attempts.
    let (_tmp, mut repo) = seeded_a2("co-equal-ts");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    unreadable(&mut repo, "attempt-2"); // same T_BEFORE stamps
    assert_eq!(clear(&repo), Err(R::RehydrationRequired));

    // The newer failure claims to have completed BEFORE the older success.
    let (_tmp, mut repo) = seeded_a2("co-misleading-ts");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    let mut request = rehydration_request("attempt-2", "manifest-role-1", 0, "b1");
    request.started_at = T_BOUND.into();
    request.completed_at = T_BOUND.into();
    let failed = read_with(&mut repo, request, &mut Unreadable);
    assert_eq!(failed.status, ContextRehydrationStatus::Failed);
    assert_eq!(clear(&repo), Err(R::RehydrationRequired));

    // A fresh success that claims an EARLIER time than the failure still clears.
    let mut request = rehydration_request("attempt-3", "manifest-role-1", 0, "b1");
    request.started_at = T_BOUND.into();
    request.completed_at = T_BOUND.into();
    read_with(&mut repo, request, &mut Artifacts(ARTIFACT_BYTES.to_vec()));
    assert_eq!(clear(&repo).unwrap().rehydration_attempt_id, "attempt-3");
}

/// A failure invalidates only until a fresh successful reread: history is
/// kept, clearance is not blocked forever.
#[test]
fn failure_then_fresh_success_restores_clearance() {
    let (_tmp, mut repo) = seeded_a2("co-restore");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    unreadable(&mut repo, "attempt-2");
    assert_eq!(clear(&repo), Err(R::RehydrationRequired));
    good(&mut repo, "attempt-3", "manifest-role-1", 0, "b1");
    assert_eq!(clear(&repo).unwrap().rehydration_attempt_id, "attempt-3");
    // Immutable history: all three attempts remain.
    for id in ["attempt-1", "attempt-2", "attempt-3"] {
        assert!(
            repo.find_context_rehydration_attempt(PROJECT, id)
                .unwrap()
                .is_some()
        );
    }
}

/// CHANGED evidence needs the changed-source reconciliation (a successor
/// epoch) followed by a successful reread there. A later reread at the SAME
/// epoch that happens to match again does not undo it.
#[test]
fn changed_requires_succession_not_a_later_matching_reread() {
    let (_tmp, mut repo) = seeded_a2("co-changed");
    changed(&mut repo, "attempt-1");
    assert_eq!(clear(&repo), Err(R::ChangedSourcesPending));
    good(&mut repo, "attempt-2", "manifest-role-1", 0, "b1");
    assert_eq!(
        clear(&repo),
        Err(R::ChangedSourcesPending),
        "a matching reread at the same epoch is not reconciliation"
    );
    repo.succeed_changed_context_sources(
        &trusted_clock_at(T_BEFORE),
        &ChangedSourceSuccessionRequestV1 {
            role_id: "role-1".into(),
            rehydration_attempt_id: "attempt-1".into(),
            successor_manifest_id: "manifest-role-1-e1".into(),
            trigger: ContextEpochTrigger::ArchitectureChange,
            authority: authority(),
        },
    )
    .unwrap();
    assert_eq!(clear(&repo), Err(R::RehydrationRequired));
    read_with(
        &mut repo,
        rehydration_request("attempt-3", "manifest-role-1-e1", 1, "b1"),
        &mut Artifacts(CHANGED_BYTES.to_vec()),
    );
    assert_eq!(clear(&repo).unwrap().rehydration_attempt_id, "attempt-3");
}

/// Failures of another binding, manifest, epoch or role never invalidate this
/// scope's clearance.
#[test]
fn invalidation_is_isolated_to_its_exact_scope() {
    let (_tmp, mut repo) = seeded_a2("co-scope");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");

    // Another role in the same project fails to reread.
    crate::recovery_tests::add_role(&mut repo, "role-2", LogicalRoleType::RuntimeA2, &digest());
    let mut other_role = rehydration_request("other-role", "manifest-role-2", 0, "b1");
    other_role.durable_role_id = "role-2".into();
    other_role.executor_binding_id = None;
    let failed = read_with(&mut repo, other_role, &mut Unreadable);
    assert_eq!(failed.status, ContextRehydrationStatus::Failed);

    // A failure recorded against a different binding (another role's) of the
    // same manifest and epoch.
    repo.create_executor_binding(crate::recovery_tests::binding(
        "b-other",
        "role-2",
        "2026-11-08T10:00:00.000000000Z",
    ))
    .unwrap();
    let other_binding = rehydration_request("other-binding", "manifest-role-1", 0, "b-other");
    read_with(&mut repo, other_binding, &mut Unreadable);

    // A request that names no binding at all fails.
    let mut no_binding = rehydration_request("no-binding", "manifest-role-1", 0, "b1");
    no_binding.executor_binding_id = None;
    read_with(&mut repo, no_binding, &mut Unreadable);

    assert_eq!(
        clear(&repo).unwrap().rehydration_attempt_id,
        "attempt-1",
        "foreign-scope failures leave this scope cleared"
    );
    // The same failure for the exact scope does invalidate.
    unreadable(&mut repo, "exact");
    assert_eq!(clear(&repo), Err(R::RehydrationRequired));
}

/// Startup recovery shares the one decision: its per-role continuation
/// reports the invalidation and the restoration.
#[test]
fn startup_recovery_reports_the_same_invalidation() {
    let (_tmp, mut repo) = seeded_a2("co-startup");
    let scan = |repo: &mut SqliteStateRepository| {
        repo.recover_startup(&trusted_clock_at(T_BEFORE), PROJECT, &authority())
            .unwrap()
            .roles
            .remove(0)
            .continuation
    };
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    assert!(scan(&mut repo).is_ok());
    unreadable(&mut repo, "attempt-2");
    assert_eq!(scan(&mut repo), Err(R::RehydrationRequired));
    good(&mut repo, "attempt-3", "manifest-role-1", 0, "b1");
    assert_eq!(scan(&mut repo).unwrap().rehydration_attempt_id, "attempt-3");
    changed(&mut repo, "attempt-4");
    assert_eq!(scan(&mut repo), Err(R::ChangedSourcesPending));
}

/// The decision is durable: it is identical after the store is reopened.
#[test]
fn invalidation_survives_restart() {
    let (tmp, mut repo) = seeded_a2("co-restart");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    unreadable(&mut repo, "attempt-2");
    drop(repo);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert_eq!(clear(&repo), Err(R::RehydrationRequired));
    good(&mut repo, "attempt-3", "manifest-role-1", 0, "b1");
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert_eq!(clear(&repo).unwrap().rehydration_attempt_id, "attempt-3");
}

/// Delayed stale outcome: the success began reading BEFORE a newer failure
/// committed (another connection, the actual concurrency model) but commits
/// AFTER it. It must not override that invalidation.
#[test]
fn delayed_stale_success_cannot_override_a_newer_failure() {
    struct Racing<'a> {
        other: &'a mut SqliteStateRepository,
        fired: bool,
    }
    impl ArtifactMaterializer for Racing<'_> {
        fn materialize(
            &mut self,
            _reference: &ArtifactRefV1,
            accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
        ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
            if !self.fired {
                self.fired = true;
                // While this reread is in flight, a newer reread fails and commits.
                read_with(
                    self.other,
                    rehydration_request("newer-failure", "manifest-role-1", 0, "b1"),
                    &mut Unreadable,
                );
            }
            accept_chunk(ARTIFACT_BYTES)?;
            Ok(ExternalSourceMaterialization {
                materializer_id: "test-artifacts".into(),
                provenance: "test".into(),
                materialized_at: T_BEFORE.into(),
            })
        }
    }
    let (tmp, mut repo) = seeded_a2("co-delayed");
    let mut other = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let stale = read_with(
        &mut repo,
        rehydration_request("stale-success", "manifest-role-1", 0, "b1"),
        &mut Racing {
            other: &mut other,
            fired: false,
        },
    );
    assert_eq!(
        stale.status,
        ContextRehydrationStatus::Succeeded,
        "its own record stays an honest immutable success"
    );
    assert_eq!(
        clear(&repo),
        Err(R::RehydrationRequired),
        "but it began before the newer failure committed, so it cannot clear"
    );
    // A genuinely fresh reread (begun after the failure) restores clearance.
    good(&mut repo, "fresh", "manifest-role-1", 0, "b1");
    assert_eq!(clear(&repo).unwrap().rehydration_attempt_id, "fresh");
}

fn order_rows(repo: &SqliteStateRepository) -> Vec<(String, i64, i64)> {
    let mut statement = repo
        .connection()
        .prepare("SELECT rehydration_attempt_id, sequence, fence_sequence FROM context_rehydration_order WHERE project_id = ?1 ORDER BY sequence, rehydration_attempt_id")
        .unwrap();
    statement
        .query_map([PROJECT], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Every attempt (success, failure, request failure) gets exactly one order
/// row, sequences are contiguous per project and fences never run ahead.
#[test]
fn every_attempt_is_ordered_contiguously_with_a_fence_behind_it() {
    let (_tmp, mut repo) = seeded_a2("co-contiguous");
    good(&mut repo, "z-1", "manifest-role-1", 0, "b1");
    unreadable(&mut repo, "a-2");
    // A request-level failure (unknown manifest) reads nothing.
    let mut bad = rehydration_request("m-3", "manifest-role-1", 0, "b1");
    bad.context_manifest_id = "missing-manifest".into();
    repo.rehydrate_context(
        bad,
        &mut NoRepository,
        &mut Artifacts(Vec::new()),
        &mut Supplier(event_id()),
    )
    .ok();
    good(&mut repo, "b-4", "manifest-role-1", 0, "b1");
    let rows = order_rows(&repo);
    let attempts = repo
        .count_table_rows("context_rehydration_attempt")
        .unwrap();
    assert_eq!(rows.len() as i64, attempts, "one order row per attempt");
    for (index, (_, sequence, fence)) in rows.iter().enumerate() {
        assert_eq!(*sequence, index as i64 + 1, "contiguous from 1");
        assert!(
            fence < sequence,
            "a fence is strictly behind its own commit"
        );
    }
    assert_eq!(rows[0].0, "z-1", "commit order, not id order");
}

/// A success transaction that fails after its reads (the epoch moved on a
/// second connection) rolls back completely: no half-written order row, the
/// fallback FAILED record is ordered, and sequences stay contiguous.
#[test]
fn rolled_back_success_leaves_no_order_gap_or_orphan() {
    struct Advances<'a>(&'a mut SqliteStateRepository, bool);
    impl ArtifactMaterializer for Advances<'_> {
        fn materialize(
            &mut self,
            _reference: &ArtifactRefV1,
            accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
        ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
            if !self.1 {
                self.1 = true;
                self.0
                    .advance_context_epoch(PROJECT, T_BEFORE, ContextEpochTrigger::NewWave, &[])
                    .unwrap();
            }
            accept_chunk(ARTIFACT_BYTES)?;
            Ok(ExternalSourceMaterialization {
                materializer_id: "test-artifacts".into(),
                provenance: "test".into(),
                materialized_at: T_BEFORE.into(),
            })
        }
    }
    let (tmp, mut repo) = seeded_a2("co-rollback");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    let mut other = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let outcome = read_with(
        &mut repo,
        rehydration_request("attempt-2", "manifest-role-1", 0, "b1"),
        &mut Advances(&mut other, false),
    );
    assert_eq!(outcome.status, ContextRehydrationStatus::Failed);
    assert_eq!(outcome.failure_code.as_deref(), Some("STALE_CONTEXT_EPOCH"));
    let rows = order_rows(&repo);
    assert_eq!(
        rows.iter().map(|r| r.1).collect::<Vec<_>>(),
        vec![1, 2],
        "the rolled-back success consumed no sequence"
    );
    assert_eq!(
        repo.count_table_rows("context_rehydration_attempt")
            .unwrap(),
        2
    );
}

/// The order rows are immutable, append-only and cannot be forged in SQL.
#[test]
fn order_rows_reject_update_delete_replace_and_non_appending_inserts() {
    let (_tmp, mut repo) = seeded_a2("co-order-sql");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    unreadable(&mut repo, "attempt-2");
    let conn = repo.connection();
    for sql in [
        "UPDATE context_rehydration_order SET sequence = 9",
        "DELETE FROM context_rehydration_order",
        "INSERT OR REPLACE INTO context_rehydration_order SELECT * FROM context_rehydration_order",
        "UPDATE context_rehydration_order SET fence_sequence = 0",
    ] {
        assert!(conn.execute_batch(sql).is_err(), "{sql}");
    }
    // A gap, a repeat or a sequence-0 "legacy" row cannot be inserted now.
    conn.execute_batch("INSERT INTO context_rehydration_attempt(project_id,rehydration_attempt_id,durable_role_id,context_manifest_id,context_epoch_id,trigger_kind,requested_by_actor_kind,executor_binding_id,started_at,completed_at,status,failure_code) VALUES ('project-1','raw','role-1','manifest-role-1','0','A1_INIT','SYSTEM','b1','s','e','FAILED','X')").unwrap();
    for sequence in [0, 2, 4, 9] {
        let sql = format!(
            "INSERT INTO context_rehydration_order VALUES ('project-1','raw',{sequence},0)"
        );
        assert!(conn.execute_batch(&sql).is_err(), "{sql}");
    }
    // An attempt with no order row cannot be placed: fail closed.
    assert_eq!(clear(&repo), Err(R::DurableInconsistency));
}

/// Upgraded history has no recorded order. A lone unchanged success is
/// unambiguous and keeps clearing; any failure beside it is ambiguous and
/// fails closed until a fresh verified reread. A fresh failure still
/// invalidates the old success.
#[test]
fn upgraded_history_is_ambiguous_only_when_a_failure_sits_beside_a_success() {
    use crate::decision_records_tests::remove_v22;

    let downgrade = |repo: SqliteStateRepository, tmp: &TempDir| {
        drop(repo);
        let conn = rusqlite::Connection::open(tmp.db_path()).unwrap();
        remove_v22(&conn);
        drop(conn);
        assert_eq!(
            SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
            22
        );
        SqliteStateRepository::open(tmp.db_path()).unwrap()
    };

    // A lone legacy success.
    let (tmp, mut repo) = seeded_a2("co-legacy-1");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    let mut repo = downgrade(repo, &tmp);
    assert_eq!(
        order_rows(&repo),
        vec![("attempt-1".to_string(), 0, 0)],
        "backfilled at sequence 0"
    );
    assert_eq!(clear(&repo).unwrap().rehydration_attempt_id, "attempt-1");
    unreadable(&mut repo, "fresh-failure");
    assert_eq!(
        clear(&repo),
        Err(R::RehydrationRequired),
        "a post-upgrade failure invalidates upgraded history"
    );
    good(&mut repo, "fresh-success", "manifest-role-1", 0, "b1");
    assert_eq!(
        clear(&repo).unwrap().rehydration_attempt_id,
        "fresh-success"
    );

    // A legacy success beside a legacy failure: ambiguous order.
    let (tmp, mut repo) = seeded_a2("co-legacy-2");
    good(&mut repo, "attempt-1", "manifest-role-1", 0, "b1");
    unreadable(&mut repo, "attempt-2");
    let mut repo = downgrade(repo, &tmp);
    assert_eq!(clear(&repo), Err(R::RehydrationRequired));
    good(&mut repo, "fresh", "manifest-role-1", 0, "b1");
    assert_eq!(clear(&repo).unwrap().rehydration_attempt_id, "fresh");

    // Legacy CHANGED evidence stays pending.
    let (tmp, mut repo) = seeded_a2("co-legacy-3");
    changed(&mut repo, "attempt-1");
    let repo = downgrade(repo, &tmp);
    assert_eq!(clear(&repo), Err(R::ChangedSourcesPending));
}

/// v21 -> v22: a failed step leaves the exact v21 store, a retry succeeds, a
/// repeat is idempotent and a removed v22 guard is corruption on open.
#[test]
fn v21_store_migrates_to_v22_and_a_failed_step_leaves_exact_v21() {
    let tmp = TempDir::new("co-migrate22");
    drop(
        SqliteStateRepository::open_with_migrations(
            tmp.db_path(),
            &crate::migrations::registered()[..21],
        )
        .unwrap(),
    );
    let conn = rusqlite::Connection::open(tmp.db_path()).unwrap();
    crate::graph_repository::verify_v21(&conn, crate::GraphPhase::Read).unwrap();
    conn.execute_batch("CREATE TRIGGER fail22 BEFORE INSERT ON state_schema_version WHEN NEW.version=22 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).is_err());
    crate::graph_repository::verify_v21(&conn, crate::GraphPhase::Read).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name IN ('context_rehydration_order','review_claim_seal')",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    conn.execute_batch("DROP TRIGGER fail22").unwrap();
    drop(conn);
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
        22
    );
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
        22,
        "idempotent on a current store"
    );
    let conn = rusqlite::Connection::open(tmp.db_path()).unwrap();
    conn.execute_batch("DROP TRIGGER context_rehydration_order_appends")
        .unwrap();
    drop(conn);
    assert!(SqliteStateRepository::open(tmp.db_path()).is_err());
}
