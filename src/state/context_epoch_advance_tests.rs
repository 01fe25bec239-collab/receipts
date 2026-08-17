//! Deterministic tests for the transactional ContextEpoch advancement
//! persistence primitive (A3-012 slice).
//!
//! All tests use real temporary SQLite database files under the system
//! temporary directory (never inside the repository). Storage-backstop,
//! concurrent-writer, and corruption checks that need SQL beyond the
//! public repository API run through crate-private test helpers only and
//! are `#[cfg(test)]`-gated; no production SQL surface exists for them.
//!
//! Compile-time/API-absence invariants are established by the public
//! surface of this crate and by source inspection, following the
//! conventions of `context_epoch_tests`; the runtime-observable parts are
//! exercised in real tests below:
//!
//! * exactly one advancement capability exists —
//!   `advance_context_epoch(&mut self, project_id: &str, advanced_at:
//!   &str, trigger: ContextEpochTrigger, invalidated_role_ids: &[String]) -> Result<ContextEpoch,
//!   StateError>` (pinned by a function-pointer type assertion below);
//!   no `next_context_epoch`, `peek_next_epoch`, `reserve_epoch`,
//!   `allocate_epoch`, `set_current_epoch`,
//!   `increment_epoch_without_insert`, or any other advancement API;
//! * no current-epoch pointer: no `current_epoch`/`latest_epoch`/
//!   `context_epoch_current`/`epoch_pointer` table or column is
//!   introduced, and the pre-existing LogicalRole `current_context_epoch`
//!   field is never modified or repurposed by advancement;
//! * no trigger handler exists: `on_host_switch`, `on_context_compaction`,
//!   `on_new_wave`, `on_a4_rejection`, `before_integration`,
//!   `before_goal_complete`, and every other trigger is caller-supplied
//!   closed typed metadata only — State detects no trigger;
//! * no digest comparison or computation, no source loading of any kind
//!   (the module imports no `std::fs`, no HTTP client, no process
//!   spawning, never executes a `STATE_QUERY` target, never dereferences
//!   an artifact reference), and no hashing dependency;
//! * no `rehydrate`, `reconcile_epoch`, `compare_manifest_epoch`,
//!   `rebuild_context`, `resume_role`, `mark_rehydrated`,
//!   `set_last_rehydrated_at`, or `load_required_sources`;
//! * no background worker, timer, cron loop, scheduler, watcher, or
//!   polling loop — advancement is explicit caller invocation only;
//! * no `update_context_epoch`, `delete_context_epoch`,
//!   `replace_context_epoch`, or `upsert_context_epoch`, and no
//!   `INSERT OR REPLACE` / `UPSERT` / `ON CONFLICT DO UPDATE` / `UPDATE` /
//!   `DELETE` anywhere in this slice's SQL;
//! * no `changed_sources` field, column, table, or calculation;
//! * no `chrono`, `time`, `SystemTime`, `Instant`, or other clock
//!   dependency: timestamps are opaque strings, never parsed, compared,
//!   or regenerated;
//! * no ContextManifest/LogicalRole/ExecutorBinding mutation method, no
//!   `append_event` call from the advancement path, and no `EventType`
//!   or event-schema change: `CONTEXT_EPOCH_ADVANCED` is never emitted
//!   by persistence;
//! * no dependency change (`Cargo.toml`/`Cargo.lock` untouched —
//!   established by the task's mandatory git-diff evidence, not a test),
//!   no arbitrary-SQL public API, no `rusqlite` type crossing the public
//!   surface, no worker/adapter/model write path, and no `unsafe` Rust
//!   anywhere in this crate.

use rusqlite::ToSql;

use crate::context_epoch::{ContextEpoch, ContextEpochTrigger};
use crate::context_manifest::{
    ContextManifest, ContextManifestSource, ContextSourceRef, ContextSourceRefType, RequiredFor,
    SourceClass,
};
use crate::error::StateError;
use crate::event::{
    ActorKind, EventActor, EventEnvelope, EventPayloadReference, EventSubject, EventType,
    SubjectKind,
};
use crate::executor_binding::{ExecutorBinding, ReleaseReason};
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

/// The opaque timestamp used by the advancement helper.
const ADVANCED_AT: &str = "2026-08-17T10:00:00.000Z";

type AdvanceContextEpochFn = fn(
    &mut SqliteStateRepository,
    &str,
    &str,
    ContextEpochTrigger,
    &[String],
) -> Result<ContextEpoch, StateError>;

/// Performs one authoritative advancement through the public API.
fn advance(
    repo: &mut SqliteStateRepository,
    project_id: &str,
    trigger: ContextEpochTrigger,
) -> Result<ContextEpoch, StateError> {
    repo.advance_context_epoch(project_id, ADVANCED_AT, trigger, &[])
}

/// A minimal contract-valid explicit ContextEpoch for history seeding.
fn explicit_epoch(project_id: &str, epoch: i64, trigger: ContextEpochTrigger) -> ContextEpoch {
    ContextEpoch {
        project_id: project_id.to_string(),
        epoch,
        advanced_at: ADVANCED_AT.to_string(),
        trigger,
    }
}

/// A minimal contract-valid LogicalRole for cross-entity isolation tests.
fn minimal_role(role_id: &str) -> LogicalRole {
    LogicalRole {
        role_id: role_id.to_string(),
        project_id: "project-1".to_string(),
        role_type: LogicalRoleType::RuntimeA1,
        status: LogicalRoleStatus::Active,
        current_context_epoch: 0,
        name: None,
        workstream_id: None,
        ownership_paths: Vec::new(),
        integration_branch: None,
        context_manifest_id: None,
        active_binding_id: None,
        created_at: None,
    }
}

/// A minimal contract-valid ContextManifest owned by `role_id`.
fn minimal_manifest(manifest_id: &str, role_id: &str) -> ContextManifest {
    ContextManifest {
        manifest_id: manifest_id.to_string(),
        role_id: role_id.to_string(),
        project_id: "project-1".to_string(),
        epoch: 3,
        sources: vec![ContextManifestSource {
            r#ref: ContextSourceRef {
                ref_type: ContextSourceRefType::RepoPath,
                target: "build-control/orchestrator-architecture/CONTEXT_MANIFEST_SPEC.md"
                    .to_string(),
            },
            source_class: SourceClass::Mandatory,
            digest: "digest-source-1".to_string(),
            last_read_at: None,
            required_for: vec![RequiredFor::Decomposition],
        }],
        created_at: "2026-08-16T09:00:00.000Z".to_string(),
        last_rehydrated_at: Some("2026-08-16T09:30:00.000Z".to_string()),
    }
}

/// A minimal contract-valid ExecutorBinding for cross-entity isolation
/// tests.
fn minimal_binding(binding_id: &str, role_id: &str) -> ExecutorBinding {
    ExecutorBinding {
        binding_id: binding_id.to_string(),
        role_id: role_id.to_string(),
        provider_id: "provider-alpha".to_string(),
        model_id: "model-alpha".to_string(),
        runtime_id: "runtime-a1-host".to_string(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: "2026-08-16T08:00:00.000Z".to_string(),
        lease_expires_at: "2027-01-01T00:00:00.000Z".to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    }
}

/// A canonical 26-character Crockford Base32 ULID event identity.
const EVENT_ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAK";

/// A minimal strict EventEnvelope for event-isolation probes.
fn minimal_event() -> EventEnvelope {
    EventEnvelope {
        event_id: EVENT_ULID.to_string(),
        project_id: "project-1".to_string(),
        goal_id: None,
        event_type: EventType::TaskCreated,
        actor: EventActor {
            kind: ActorKind::System,
            id: None,
        },
        subject: EventSubject {
            kind: SubjectKind::Task,
            id: "task-001".to_string(),
        },
        occurred_at: "2026-08-17T10:00:00.000Z".to_string(),
        payload: EventPayloadReference {
            reference: "blob://events/0001".to_string(),
            digest: "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
                .to_string(),
        },
        correlation_id: "corr-0001".to_string(),
        epoch: 0,
    }
}

/// An opened repository; ContextEpoch persistence has no foreign-key
/// dependencies, so no seeding is required.
fn opened_repo(tag: &str) -> (TempDir, SqliteStateRepository) {
    let tmp = TempDir::new(tag);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    (tmp, repo)
}

/// Direct test-only SQL with bound parameters on the internal connection,
/// mirroring the single-active-binding test convention.
fn direct_exec(repo: &mut SqliteStateRepository, sql: &str, params: &[&dyn ToSql]) {
    repo.run_transaction(|uow| uow.execute(sql, params))
        .expect("test corruption statement");
}

// T01 — the schema remains at the current version after advancement.
#[test]
fn t01_schema_remains_version_8() {
    let (tmp, mut repo) = opened_repo("cea-t01");
    assert_eq!(repo.schema_version().expect("version read"), 9);
    advance(&mut repo, "project-1", ContextEpochTrigger::A1Init).expect("advance");
    assert_eq!(
        repo.schema_version().expect("version read"),
        9,
        "advancement must not change the schema version"
    );
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(repo.schema_version().expect("version read"), 9);
}

// T02 — migration v9 is the exact registered chain head.
#[test]
fn t02_migration_v8_registered() {
    let registered = migrations::registered();
    assert_eq!(
        registered.len(),
        9,
        "exactly nine registered migrations may exist"
    );
    let versions: Vec<u32> = registered.iter().map(|m| m.version).collect();
    assert_eq!(versions, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
}

// T03 — the first advancement for a project with no history returns and
// persists epoch 0.
#[test]
fn t03_first_advancement_creates_epoch_zero() {
    let (_tmp, mut repo) = opened_repo("cea-t03");
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::A1Init)
        .expect("first advancement succeeds");
    assert_eq!(created.epoch, 0, "no history → epoch 0");
    assert_eq!(
        repo.find_context_epoch("project-1", 0).expect("find"),
        Some(created.clone()),
        "epoch 0 is durably persisted"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        1,
        "exactly one row per advancement"
    );
}

// T04 — the first advancement survives close/reopen.
#[test]
fn t04_first_advancement_survives_reopen() {
    let (tmp, mut repo) = opened_repo("cea-t04");
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::A1Init)
        .expect("first advancement succeeds");
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_context_epoch("project-1", 0).expect("find"),
        Some(created),
        "the advancement-created epoch 0 is durable"
    );
}

// T05 — the second advancement after epoch 0 creates epoch 1.
#[test]
fn t05_second_advancement_creates_epoch_one() {
    let (_tmp, mut repo) = opened_repo("cea-t05");
    advance(&mut repo, "project-1", ContextEpochTrigger::A1Init).expect("advance to 0");
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect("second advancement succeeds");
    assert_eq!(created.epoch, 1, "history max 0 → epoch 1");
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .epoch,
        1
    );
}

// T06 — sequential advancements produce epochs 0, 1, 2.
#[test]
fn t06_sequential_advancements_produce_0_1_2() {
    let (_tmp, mut repo) = opened_repo("cea-t06");
    for expected in [0, 1, 2] {
        let created = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
            .expect("sequential advancement succeeds");
        assert_eq!(created.epoch, expected, "advancement #{expected}");
    }
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        3,
        "three advancement records"
    );
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .epoch,
        2
    );
}

// T07 — existing explicit history with max 4 → advancement creates 5.
#[test]
fn t07_explicit_history_max_4_advances_to_5() {
    let (_tmp, mut repo) = opened_repo("cea-t07");
    for epoch in 0..=4 {
        repo.append_context_epoch(explicit_epoch(
            "project-1",
            epoch,
            ContextEpochTrigger::NewWave,
        ))
        .expect("seed explicit history");
    }
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::HostSwitch)
        .expect("advancement over explicit history");
    assert_eq!(created.epoch, 5, "history max 4 → epoch 5");
}

// T08 — non-contiguous history 1, 3, 9 → advancement creates 10.
#[test]
fn t08_non_contiguous_history_advances_to_max_plus_one() {
    let (_tmp, mut repo) = opened_repo("cea-t08");
    for epoch in [1, 3, 9] {
        repo.append_context_epoch(explicit_epoch(
            "project-1",
            epoch,
            ContextEpochTrigger::TaskThreshold,
        ))
        .expect("seed non-contiguous history");
    }
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect("advancement over non-contiguous history");
    assert_eq!(created.epoch, 10, "history 1,3,9 → epoch 10 (max + 1)");
}

// T09 — out-of-order history 10 then 3 → advancement creates 11.
#[test]
fn t09_out_of_order_history_advances_to_11() {
    let (_tmp, mut repo) = opened_repo("cea-t09");
    repo.append_context_epoch(explicit_epoch(
        "project-1",
        10,
        ContextEpochTrigger::NewWave,
    ))
    .expect("append epoch 10");
    repo.append_context_epoch(explicit_epoch(
        "project-1",
        3,
        ContextEpochTrigger::ContextCompaction,
    ))
    .expect("append epoch 3 out of order");
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect("advancement over out-of-order history");
    assert_eq!(
        created.epoch, 11,
        "numeric maximum 10 is authoritative → epoch 11, never insertion order"
    );
}

// T10 — the returned ContextEpoch equals the exact durable readback.
#[test]
fn t10_returned_record_equals_exact_readback() {
    let (_tmp, mut repo) = opened_repo("cea-t10");
    repo.append_context_epoch(explicit_epoch("project-1", 6, ContextEpochTrigger::NewWave))
        .expect("seed history");
    let created = repo
        .advance_context_epoch(
            "project-1",
            "2026-08-17T12:34:56.789Z",
            ContextEpochTrigger::SecurityEscalation,
            &[],
        )
        .expect("advance");
    assert_eq!(
        repo.find_context_epoch("project-1", 7).expect("find"),
        Some(created),
        "the returned record must equal the exact durable readback"
    );
}

// T11 — the returned ContextEpoch equals the latest durable record
// immediately after the commit.
#[test]
fn t11_returned_record_equals_latest_after_commit() {
    let (_tmp, mut repo) = opened_repo("cea-t11");
    let created =
        advance(&mut repo, "project-1", ContextEpochTrigger::A1Init).expect("first advancement");
    let second = advance(
        &mut repo,
        "project-1",
        ContextEpochTrigger::ModelReplacement,
    )
    .expect("second advancement");
    assert_eq!(
        repo.find_latest_context_epoch("project-1").expect("latest"),
        Some(second.clone()),
        "the committed record is immediately the derived latest"
    );
    assert_ne!(second, created);
}

// T12 — project_id round-trips unchanged through advancement.
#[test]
fn t12_project_id_round_trips_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t12");
    let project_id = "project / ☃-42 :: with spaces and punctuation";
    let created = advance(&mut repo, project_id, ContextEpochTrigger::A2Init).expect("advance");
    assert_eq!(created.project_id, project_id);
    assert_eq!(
        repo.find_context_epoch(project_id, 0).expect("find"),
        Some(created),
        "project_id must persist byte-for-byte, never normalized"
    );
}

// T13 — advanced_at round-trips unchanged through advancement.
#[test]
fn t13_advanced_at_round_trips_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t13");
    let advanced_at = "  not-a-parsed-timestamp ☃ ";
    let created = repo
        .advance_context_epoch(
            "project-1",
            advanced_at,
            ContextEpochTrigger::HostSwitch,
            &[],
        )
        .expect("advance");
    assert_eq!(created.advanced_at, advanced_at);
    assert_eq!(
        repo.find_context_epoch("project-1", 0).expect("find"),
        Some(created),
        "advanced_at must persist byte-for-byte, never normalized"
    );
}

// T14 — the supplied trigger round-trips unchanged through advancement.
#[test]
fn t14_trigger_round_trips_unchanged() {
    let (tmp, mut repo) = opened_repo("cea-t14");
    for (index, trigger) in ContextEpochTrigger::ALL.iter().enumerate() {
        let project = format!("project-{index}");
        let created = repo
            .advance_context_epoch(&project, ADVANCED_AT, *trigger, &[])
            .expect("every frozen trigger advances");
        assert_eq!(created.trigger, *trigger);
        assert_eq!(
            repo.find_context_epoch(&project, 0)
                .expect("find")
                .expect("present")
                .trigger,
            *trigger,
            "trigger {:?} must round-trip through advancement",
            trigger
        );
    }
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        15,
        "one advancement per trigger, each persisting exactly its trigger"
    );
}

// T15 — an empty project_id is rejected before any storage access.
#[test]
fn t15_empty_project_id_rejected() {
    let (_tmp, mut repo) = opened_repo("cea-t15");
    let error = repo
        .advance_context_epoch("", ADVANCED_AT, ContextEpochTrigger::A1Init, &[])
        .expect_err("empty project_id must fail validation");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "no record may persist from a rejected advancement"
    );
}

// T16 — an overlong project_id (201 scalar values) is rejected; exactly
// 200 remains the accepted boundary.
#[test]
fn t16_overlong_project_id_rejected() {
    let (_tmp, mut repo) = opened_repo("cea-t16");
    let error = repo
        .advance_context_epoch(
            &"p".repeat(201),
            ADVANCED_AT,
            ContextEpochTrigger::A1Init,
            &[],
        )
        .expect_err("overlong project_id must fail validation");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "no record may persist from a rejected advancement"
    );
    repo.advance_context_epoch(
        &"p".repeat(200),
        ADVANCED_AT,
        ContextEpochTrigger::A1Init,
        &[],
    )
    .expect("200-character project_id is the accepted boundary");
}

// T17 — an empty advanced_at is rejected before any storage access.
#[test]
fn t17_empty_advanced_at_rejected() {
    let (_tmp, mut repo) = opened_repo("cea-t17");
    let error = repo
        .advance_context_epoch("project-1", "", ContextEpochTrigger::A1Init, &[])
        .expect_err("empty advanced_at must fail validation");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "no record may persist from a rejected advancement"
    );
}

// T18 — no timestamp parsing or normalization: a non-RFC3339 advanced_at
// is accepted and stored exactly as supplied on the advancement path.
#[test]
fn t18_no_timestamp_parsing_or_normalization() {
    let (_tmp, mut repo) = opened_repo("cea-t18");
    let advanced_at = "sometime-yesterday-ish";
    let created = repo
        .advance_context_epoch("project-1", advanced_at, ContextEpochTrigger::NewWave, &[])
        .expect("advanced_at is opaque and needs no timestamp format");
    assert_eq!(created.advanced_at, advanced_at);
    assert_eq!(
        repo.find_context_epoch("project-1", 0)
            .expect("find")
            .expect("present")
            .advanced_at,
        advanced_at,
        "the opaque timestamp string must be stored exactly as supplied"
    );
}

// T19 — timestamp ordering does not influence the derived epoch: the
// numeric maximum alone is authoritative.
#[test]
fn t19_timestamp_ordering_does_not_influence_derivation() {
    let (_tmp, mut repo) = opened_repo("cea-t19");
    let mut lexically_latest = explicit_epoch("project-1", 4, ContextEpochTrigger::NewWave);
    lexically_latest.advanced_at = "9999-12-31T23:59:59.999Z".to_string();
    let mut lexically_earliest = explicit_epoch("project-1", 9, ContextEpochTrigger::NewWave);
    lexically_earliest.advanced_at = "0001-01-01T00:00:00.000Z".to_string();
    repo.append_context_epoch(lexically_latest)
        .expect("append 4");
    repo.append_context_epoch(lexically_earliest)
        .expect("append 9");
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect("advancement derives from the numeric maximum");
    assert_eq!(
        created.epoch, 10,
        "numeric max 9 → epoch 10, never the lexically-latest timestamp"
    );
}

// T20 — multi-project derivation is independent: no global counter.
#[test]
fn t20_multi_project_derivation_independent() {
    let (_tmp, mut repo) = opened_repo("cea-t20");
    for epoch in 0..=2 {
        repo.append_context_epoch(explicit_epoch("P1", epoch, ContextEpochTrigger::NewWave))
            .expect("seed P1");
    }
    let p2 = advance(&mut repo, "P2", ContextEpochTrigger::A2Init).expect("advance P2");
    assert_eq!(
        p2.epoch, 0,
        "P2 has no history → epoch 0 despite P1's max 2"
    );
    let p1 = advance(&mut repo, "P1", ContextEpochTrigger::NewWave).expect("advance P1");
    assert_eq!(p1.epoch, 3, "P1 history max 2 → epoch 3");
}

// T21 — P1 max 5 / P2 max 11 → advancements create 6 and 12.
#[test]
fn t21_multi_project_max_plus_one() {
    let (_tmp, mut repo) = opened_repo("cea-t21");
    repo.append_context_epoch(explicit_epoch("P1", 5, ContextEpochTrigger::A1Init))
        .expect("seed P1 max 5");
    repo.append_context_epoch(explicit_epoch("P2", 11, ContextEpochTrigger::A2Init))
        .expect("seed P2 max 11");
    let p1 = advance(&mut repo, "P1", ContextEpochTrigger::NewWave).expect("advance P1");
    let p2 = advance(&mut repo, "P2", ContextEpochTrigger::NewWave).expect("advance P2");
    assert_eq!(p1.epoch, 6, "P1 max 5 → 6");
    assert_eq!(p2.epoch, 12, "P2 max 11 → 12");
    assert_eq!(
        repo.find_latest_context_epoch("P1")
            .expect("latest")
            .expect("present")
            .epoch,
        6
    );
    assert_eq!(
        repo.find_latest_context_epoch("P2")
            .expect("latest")
            .expect("present")
            .epoch,
        12
    );
}

// T22 — existing history at i64::MAX → advancement fails closed with
// ContextEpochAdvanceOverflow.
#[test]
fn t22_overflow_fails_closed() {
    let (_tmp, mut repo) = opened_repo("cea-t22");
    repo.append_context_epoch(explicit_epoch(
        "project-1",
        i64::MAX,
        ContextEpochTrigger::TaskThreshold,
    ))
    .expect("seed i64::MAX history");
    let error = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect_err("i64::MAX history must fail advancement");
    assert!(
        matches!(
            &error,
            StateError::ContextEpochAdvanceOverflow { project_id } if project_id == "project-1"
        ),
        "unexpected error: {error}"
    );
}

// T23 — an overflow failure inserts no row.
#[test]
fn t23_overflow_failure_inserts_no_row() {
    let (_tmp, mut repo) = opened_repo("cea-t23");
    repo.append_context_epoch(explicit_epoch(
        "project-1",
        i64::MAX,
        ContextEpochTrigger::TaskThreshold,
    ))
    .expect("seed i64::MAX history");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        1,
        "pre-overflow history"
    );
    advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect_err("overflow fails closed");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        1,
        "the overflow failure must not insert a row"
    );
}

// T24 — an overflow failure leaves the prior history unchanged.
#[test]
fn t24_overflow_failure_leaves_history_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t24");
    let first = explicit_epoch("project-1", 4, ContextEpochTrigger::NewWave);
    let max = explicit_epoch("project-1", i64::MAX, ContextEpochTrigger::TaskThreshold);
    repo.append_context_epoch(first.clone()).expect("seed 4");
    repo.append_context_epoch(max.clone()).expect("seed max");
    advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect_err("overflow fails closed");
    assert_eq!(
        repo.find_context_epoch("project-1", 4).expect("find"),
        Some(first),
        "the lower record is untouched by the overflow failure"
    );
    assert_eq!(
        repo.find_latest_context_epoch("project-1").expect("latest"),
        Some(max),
        "the maximum record is untouched by the overflow failure"
    );
}

// T25 — the overflow path never wraps, panics, or saturates: the failure
// is a deterministic typed error, on every repeated attempt.
#[test]
fn t25_no_wrap_panic_or_saturation() {
    let (_tmp, mut repo) = opened_repo("cea-t25");
    repo.append_context_epoch(explicit_epoch(
        "project-1",
        i64::MAX,
        ContextEpochTrigger::TaskThreshold,
    ))
    .expect("seed i64::MAX history");
    for _ in 0..3 {
        let error = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
            .expect_err("every attempt at i64::MAX fails identically");
        assert!(
            matches!(error, StateError::ContextEpochAdvanceOverflow { .. }),
            "no wrap/saturation success, no panic: {error}"
        );
    }
    // No wrapped negative or saturated record ever appeared.
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        1,
        "only the seeded history row exists"
    );
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .epoch,
        i64::MAX
    );
    // Overflow is project-scoped: another project still advances to 0.
    let other = advance(&mut repo, "project-2", ContextEpochTrigger::A1Init)
        .expect("overflow on P1 does not block P2");
    assert_eq!(other.epoch, 0);
}

// T26/T27/T28 — a forced transaction failure after derivation and insert
// rolls the new row back, consumes no sequence value, and the next
// successful advancement derives the same correct next epoch again.
#[test]
fn t26_t27_t28_forced_rollback_derives_same_next_epoch_again() {
    let (_tmp, mut repo) = opened_repo("cea-t26");
    advance(&mut repo, "project-1", ContextEpochTrigger::A1Init).expect("advance to 0");
    advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance to 1");
    // Reproduce the exact advancement transaction shape — snapshot latest,
    // derive the successor (known history → 2), insert, then force the
    // unit of work to fail before commit.
    let error = repo
        .run_transaction(|uow| {
            let latest = uow.read_latest_context_epoch("project-1")?;
            assert_eq!(
                latest.expect("present").epoch,
                1,
                "the in-transaction lookup sees the committed history"
            );
            uow.insert_context_epoch(&explicit_epoch(
                "project-1",
                2,
                ContextEpochTrigger::HostSwitch,
            ))?;
            Err::<(), StateError>(StateError::UnitOfWorkFailed {
                detail: "forced test failure after insert".to_string(),
            })
        })
        .expect_err("forced failure surfaces its error");
    assert!(
        matches!(error, StateError::UnitOfWorkFailed { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        2,
        "the rolled-back advancement leaves no row"
    );
    assert_eq!(
        repo.find_context_epoch("project-1", 2).expect("find"),
        None,
        "the derived epoch 2 was not consumed or reserved by the failure"
    );
    // The next successful advancement derives the same correct next value.
    let retried = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect("advancement after rollback succeeds");
    assert_eq!(
        retried.epoch, 2,
        "no sequence was consumed: the same next epoch is derived again"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        3,
        "exactly one new row from the retried advancement"
    );
}

// T29/T30/T31 — a concurrent writer that commits the derived epoch after
// this transaction's snapshot causes a fail-closed conflict: the
// composite primary-key backstop refuses the insert, the failure never
// retries with a different epoch number, and existing history is
// untouched. (This reproduces the exact advancement transaction shape
// with an interleaved second connection; a real multi-process race is
// not required under the frozen single-writer MVP.)
#[test]
fn t29_t30_t31_concurrent_writer_conflict_fails_closed_no_retry() {
    let (tmp, mut repo) = opened_repo("cea-t29");
    for epoch in 0..=4 {
        repo.append_context_epoch(explicit_epoch(
            "project-1",
            epoch,
            ContextEpochTrigger::NewWave,
        ))
        .expect("seed history");
    }
    let tables_before = repo.list_tables().expect("tables");
    let error = repo
        .run_transaction(|uow| {
            let latest = uow.read_latest_context_epoch("project-1")?;
            assert_eq!(latest.expect("present").epoch, 4, "snapshot max is 4");
            // A second connection — the "future concurrent writer" —
            // commits epoch 5 after this transaction's snapshot was taken.
            let mut concurrent =
                SqliteStateRepository::open(tmp.db_path()).expect("concurrent writer connection");
            concurrent
                .append_context_epoch(explicit_epoch(
                    "project-1",
                    5,
                    ContextEpochTrigger::SecurityEscalation,
                ))
                .expect("concurrent writer commits epoch 5");
            // Inserting the derived epoch 5 against the stale snapshot now
            // conflicts with the durable backstop.
            uow.insert_context_epoch(&explicit_epoch(
                "project-1",
                5,
                ContextEpochTrigger::HostSwitch,
            ))
        })
        .expect_err("the conflicting insert must fail closed");
    assert!(
        matches!(
            error,
            StateError::ContextEpochWriteFailed { .. }
                | StateError::ContextEpochAlreadyExists { .. }
        ),
        "unexpected error: {error}"
    );
    // No retry with a different number: rows 0–5 exist (5 from the
    // concurrent writer), and no epoch 6 row was manufactured by a
    // retry-to-next-value loop.
    for epoch in 0..=5 {
        assert!(
            repo.find_context_epoch("project-1", epoch)
                .expect("find")
                .is_some(),
            "history epoch {epoch} remains"
        );
    }
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        6,
        "exactly the six legitimate rows: no extra row from any retry"
    );
    // The concurrent writer's committed record is intact.
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .epoch,
        5,
        "existing history is unchanged by the failed advancement"
    );
    assert_eq!(
        repo.list_tables().expect("tables"),
        tables_before,
        "the failed advancement created no new schema object"
    );
    // Advancement remains usable after the conflict, deriving from the
    // new maximum — proof that no free-epoch search loop exists.
    let next = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect("advancement after conflict succeeds");
    assert_eq!(next.epoch, 6, "the next legitimate successor is 6");
}

// T32 — the existing explicit append API still allows out-of-order
// records alongside advancement.
#[test]
fn t32_append_still_allows_out_of_order() {
    let (_tmp, mut repo) = opened_repo("cea-t32");
    advance(&mut repo, "project-1", ContextEpochTrigger::A1Init).expect("advance to 0");
    repo.append_context_epoch(explicit_epoch(
        "project-1",
        10,
        ContextEpochTrigger::NewWave,
    ))
    .expect("explicit out-of-order append 10");
    repo.append_context_epoch(explicit_epoch(
        "project-1",
        3,
        ContextEpochTrigger::TaskThreshold,
    ))
    .expect("explicit out-of-order append 3");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        3,
        "all out-of-order records persist"
    );
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect("advancement over mixed history");
    assert_eq!(created.epoch, 11, "numeric maximum 10 → epoch 11");
}

// T33 — the existing explicit append duplicate behavior is unchanged.
#[test]
fn t33_append_duplicate_behavior_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t33");
    let original = explicit_epoch("project-1", 2, ContextEpochTrigger::NewWave);
    repo.append_context_epoch(original.clone()).expect("append");
    let error = repo
        .append_context_epoch(explicit_epoch(
            "project-1",
            2,
            ContextEpochTrigger::HostSwitch,
        ))
        .expect_err("duplicate explicit append still fails");
    assert!(
        matches!(
            &error,
            StateError::ContextEpochAlreadyExists {
                project_id,
                epoch: 2
            } if project_id == "project-1"
        ),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.find_context_epoch("project-1", 2).expect("find"),
        Some(original),
        "the original record is untouched"
    );
}

// T34 — the existing exact-read behavior is unchanged alongside
// advancement.
#[test]
fn t34_exact_read_behavior_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t34");
    let seeded = explicit_epoch("project-1", 7, ContextEpochTrigger::ContractChange);
    repo.append_context_epoch(seeded.clone()).expect("append");
    let created =
        advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance to 8");
    assert_eq!(
        repo.find_context_epoch("project-1", 7).expect("find"),
        Some(seeded),
        "pre-advancement records read back exactly"
    );
    assert_eq!(
        repo.find_context_epoch("project-1", 8).expect("find"),
        Some(created),
        "advancement records read back exactly"
    );
    assert_eq!(
        repo.find_context_epoch("project-1", 5).expect("find"),
        None,
        "a gap epoch remains deterministic absence"
    );
    assert_eq!(
        repo.find_context_epoch("other-project", 8).expect("find"),
        None,
        "per-project scoping is unchanged"
    );
}

// T35 — the existing latest-read behavior is unchanged: latest continues
// to mean highest numeric persisted epoch, never a pointer.
#[test]
fn t35_latest_read_behavior_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t35");
    for epoch in [1, 5, 3] {
        repo.append_context_epoch(explicit_epoch(
            "project-1",
            epoch,
            ContextEpochTrigger::NewWave,
        ))
        .expect("seed");
    }
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .epoch,
        5,
        "latest remains the highest numeric epoch"
    );
    advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance to 6");
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .epoch,
        6,
        "latest tracks the highest persisted epoch after advancement"
    );
    assert_eq!(
        repo.find_latest_context_epoch("project-2").expect("latest"),
        None,
        "absence for a project without history is unchanged"
    );
}

// T36 — no current-epoch pointer is introduced: no current_epoch column
// on any table, no pointer table, and advancing never widens the schema.
#[test]
fn t36_no_current_epoch_pointer() {
    let (_tmp, mut repo) = opened_repo("cea-t36");
    let tables_before = repo.list_tables().expect("tables");
    let columns_before = repo.table_columns("context_epoch").expect("columns");
    advance(&mut repo, "project-1", ContextEpochTrigger::A1Init).expect("advance to 0");
    advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance to 1");
    assert_eq!(
        repo.list_tables().expect("tables"),
        tables_before,
        "advancement must not create any pointer/projection storage"
    );
    assert_eq!(
        repo.table_columns("context_epoch").expect("columns"),
        columns_before,
        "advancement must not widen the epoch table"
    );
    for table in [
        "state_schema_version",
        "logical_role",
        "executor_binding",
        "event",
        "context_manifest",
        "context_manifest_source",
        "context_manifest_source_required_for",
        "context_epoch",
        "context_epoch_invalidated_role",
        "context_rehydration_attempt",
        "context_rehydration_repository_snapshot",
        "context_rehydration_source_evidence",
    ] {
        assert!(
            !repo
                .table_columns(table)
                .expect("columns")
                .iter()
                .any(|column| {
                    column == "current_epoch"
                        || column == "latest_epoch"
                        || column == "epoch_pointer"
                }),
            "no current-epoch pointer column may exist on {table}"
        );
    }
    for forbidden in [
        "context_epoch_current",
        "context_epoch_pointer",
        "context_epoch_state",
    ] {
        assert!(
            !repo.table_exists(forbidden).expect("table check"),
            "no {forbidden} pointer table may exist"
        );
    }
}

// T37/T38 — advancement never mutates LogicalRole, in particular its
// current_context_epoch field: the row is byte-equivalent after
// advancement.
#[test]
fn t37_t38_logical_role_unchanged_after_advancement() {
    let (_tmp, mut repo) = opened_repo("cea-t37");
    let role = minimal_role("role-1");
    repo.create_logical_role(role.clone()).expect("role create");
    advance(&mut repo, "project-1", ContextEpochTrigger::A1Init).expect("advance to 0");
    advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance to 1");
    let after = repo
        .find_logical_role("role-1")
        .expect("find")
        .expect("present");
    assert_eq!(
        after, role,
        "the entire LogicalRole row, including current_context_epoch, must be unchanged"
    );
    assert_eq!(after.current_context_epoch, role.current_context_epoch);
}

// T39/T40/T41 — advancement never mutates ContextManifest: the row, its
// epoch snapshot, and last_rehydrated_at are byte-equivalent after
// advancement beyond the snapshot.
#[test]
fn t39_t40_t41_context_manifest_unchanged_after_advancement() {
    let (_tmp, mut repo) = opened_repo("cea-t39");
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    let manifest = minimal_manifest("manifest-1", "role-1");
    repo.create_context_manifest(manifest.clone())
        .expect("manifest create");
    advance(&mut repo, "project-1", ContextEpochTrigger::ContractChange).expect("advance to 0");
    advance(
        &mut repo,
        "project-1",
        ContextEpochTrigger::ArchitectureChange,
    )
    .expect("advance to 1 — beyond the manifest's epoch-3 snapshot");
    let after = repo
        .find_context_manifest("manifest-1")
        .expect("find")
        .expect("present");
    assert_eq!(after, manifest, "the manifest row must be unchanged");
    assert_eq!(
        after.epoch, 3,
        "the manifest epoch snapshot is not synchronized"
    );
    assert_eq!(
        after.last_rehydrated_at,
        Some("2026-08-16T09:30:00.000Z".to_string()),
        "last_rehydrated_at is not touched by advancement"
    );
}

// T42 — advancement never mutates ExecutorBinding state.
#[test]
fn t42_executor_binding_unchanged_after_advancement() {
    let (_tmp, mut repo) = opened_repo("cea-t42");
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    let binding = minimal_binding("binding-1", "role-1");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    advance(
        &mut repo,
        "project-1",
        ContextEpochTrigger::ModelReplacement,
    )
    .expect("advance to 0");
    assert_eq!(
        repo.find_executor_binding("binding-1").expect("find"),
        Some(binding),
        "the binding must be unchanged by advancement"
    );
}

// T43/T44 — advancement emits no event of any type, in particular no
// automatic CONTEXT_EPOCH_ADVANCED.
#[test]
fn t43_t44_no_event_emission() {
    let (_tmp, mut repo) = opened_repo("cea-t43");
    let event = minimal_event();
    repo.append_event(event.clone()).expect("seed one event");
    advance(
        &mut repo,
        "project-1",
        ContextEpochTrigger::BeforeGoalComplete,
    )
    .expect("advance with a lifecycle trigger");
    assert_eq!(
        repo.count_table_rows("event").expect("rows"),
        1,
        "advancement emits no event, in particular no CONTEXT_EPOCH_ADVANCED"
    );
    assert_eq!(
        repo.find_event(EVENT_ULID).expect("find"),
        Some(event),
        "the seeded event is untouched"
    );
}

// T45/T46 — no changed_sources persistence; the parent keeps its four
// core columns.
#[test]
fn t45_t46_no_changed_sources_and_parent_shape_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t45");
    let columns = repo.table_columns("context_epoch").expect("columns");
    assert_eq!(
        columns,
        vec![
            "project_id".to_string(),
            "epoch".to_string(),
            "advanced_at".to_string(),
            "trigger".to_string()
        ],
        "the epoch table keeps exactly its four conceptual columns"
    );
    advance(
        &mut repo,
        "project-1",
        ContextEpochTrigger::ArchitectureChange,
    )
    .expect("advance");
    assert_eq!(
        repo.table_columns("context_epoch").expect("columns"),
        columns,
        "advancement never widens the stored shape"
    );
    for forbidden in [
        "context_epoch_changed_source",
        "context_epoch_source_digest",
    ] {
        assert!(
            !repo.table_exists(forbidden).expect("table check"),
            "no {forbidden} storage may exist"
        );
    }
}

// T62/T63 — the trigger enum remains exact: no new variants decode, and
// exactly the fifteen frozen values persist through advancement.
#[test]
fn t62_t63_trigger_enum_remains_exact() {
    assert_eq!(
        ContextEpochTrigger::ALL.len(),
        15,
        "exactly fifteen triggers may exist"
    );
    for forbidden in [
        "NOT_A_TRIGGER",
        "UNKNOWN",
        "OTHER",
        "CUSTOM",
        "MANUAL",
        "TIMER",
        "ON_HOST_SWITCH",
        "INVALIDATED",
        "",
    ] {
        assert!(
            ContextEpochTrigger::from_storage(forbidden).is_err(),
            "{forbidden:?} must not decode to any trigger"
        );
    }
    let (_tmp, mut repo) = opened_repo("cea-t62");
    let created = advance(
        &mut repo,
        "project-1",
        ContextEpochTrigger::SeriousA4Rejection,
    )
    .expect("advance with a frozen trigger");
    assert_eq!(created.trigger, ContextEpochTrigger::SeriousA4Rejection);
    assert_eq!(
        repo.find_context_epoch("project-1", 0)
            .expect("find")
            .expect("present")
            .trigger
            .as_str(),
        "SERIOUS_A4_REJECTION"
    );
}

// T64 — no new schema object: the full table set and the index/trigger/
// view entries on context_epoch are identical before and after
// advancement.
#[test]
fn t64_no_new_schema_object() {
    let (_tmp, mut repo) = opened_repo("cea-t64");
    let mut expected = vec![
        "state_schema_version",
        "logical_role",
        "logical_role_ownership_path",
        "executor_binding",
        "event",
        "context_manifest",
        "context_manifest_source",
        "context_manifest_source_required_for",
        "context_epoch",
        "context_epoch_invalidated_role",
        "context_rehydration_attempt",
        "context_rehydration_repository_snapshot",
        "context_rehydration_source_evidence",
    ];
    expected.sort_unstable();
    let tables_before = repo.list_tables().expect("tables");
    assert_eq!(tables_before, expected, "exactly the known tables");
    advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance");
    assert_eq!(
        repo.list_tables().expect("tables"),
        tables_before,
        "advancement adds no table"
    );
    // The only index entries are SQLite's implicit `sqlite_autoindex_*`
    // for the composite primary key; no explicitly created index exists.
    let indexes = repo
        .sqlite_master_entries("index", "context_epoch")
        .expect("indexes");
    assert!(
        indexes
            .iter()
            .all(|(name, sql)| name.starts_with("sqlite_autoindex") && sql.is_empty()),
        "no explicit index on context_epoch; found: {indexes:?}"
    );
    assert!(
        !indexes.is_empty(),
        "the implicit primary-key autoindex remains the durable backstop"
    );
    assert!(
        repo.sqlite_master_entries("trigger", "context_epoch")
            .expect("triggers")
            .is_empty(),
        "no trigger on context_epoch"
    );
    assert!(
        repo.sqlite_master_entries("view", "context_epoch")
            .expect("views")
            .is_empty(),
        "no view on context_epoch"
    );
}

// T66 — the public advancement surface is exactly the one authorized
// capability with the authorized shape, pinned at compile time.
#[test]
fn t66_public_api_surface_pinned() {
    let advance: AdvanceContextEpochFn = SqliteStateRepository::advance_context_epoch;
    let _ = advance;
}

// Extra — corrupt selected history during derivation: an injectable
// corrupt highest row (empty advanced_at) fails the advancement closed;
// the malformed row is never silently ignored and no lower row is used
// as a fallback for the derivation.
#[test]
fn extra_corrupt_highest_row_fails_advancement_closed() {
    let (_tmp, mut repo) = opened_repo("cea-extra-corrupt");
    let valid = explicit_epoch("project-1", 1, ContextEpochTrigger::NewWave);
    repo.append_context_epoch(valid.clone()).expect("append 1");
    direct_exec(
        &mut repo,
        "INSERT INTO context_epoch (project_id, epoch, advanced_at, trigger)
         VALUES (?1, ?2, ?3, ?4)",
        &[&"project-1", &2i64, &"", &"HOST_SWITCH"],
    );
    let error = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect_err("a corrupt highest row must fail advancement");
    assert!(
        matches!(error, StateError::ContextEpochDecodeFailed { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        2,
        "no new row may persist from the refused advancement"
    );
    assert_eq!(
        repo.find_context_epoch("project-1", 1).expect("find"),
        Some(valid),
        "the lower valid record is untouched and was not used as a fallback"
    );
}

// Extra — no automatic epoch zero outside the advance API: repository
// open, LogicalRole/ContextManifest/ExecutorBinding creation, and
// explicit append for other projects never create an epoch row for a
// project; epoch 0 appears only when advancement is explicitly called.
#[test]
fn extra_no_automatic_epoch_zero_outside_advance_api() {
    let (tmp, mut repo) = opened_repo("cea-extra-no-auto-zero");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "bootstrap creates no epoch row"
    );
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    repo.create_context_manifest(minimal_manifest("manifest-1", "role-1"))
        .expect("manifest create");
    repo.create_executor_binding(minimal_binding("binding-1", "role-1"))
        .expect("binding create");
    repo.append_context_epoch(explicit_epoch(
        "other-project",
        7,
        ContextEpochTrigger::NewWave,
    ))
    .expect("append for another project");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        1,
        "still no epoch row for project-1"
    );
    assert_eq!(
        repo.find_latest_context_epoch("project-1").expect("latest"),
        None,
        "project-1 has no history until an authorized advancement"
    );
    drop(repo);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_latest_context_epoch("project-1").expect("latest"),
        None,
        "reopen creates no epoch row"
    );
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::A1Init)
        .expect("explicit advancement creates epoch 0");
    assert_eq!(created.epoch, 0);
}

// PROBE A — bootstrap: no history → advance → epoch 0.
#[test]
fn probe_a_bootstrap_creates_epoch_zero() {
    let (_tmp, mut repo) = opened_repo("cea-pa");
    assert_eq!(
        repo.find_latest_context_epoch("project-1").expect("latest"),
        None,
        "precondition: no history"
    );
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::A1Init).expect("advance");
    assert_eq!(
        created.epoch, 0,
        "PROBE A: bootstrap advancement is epoch 0"
    );
}

// PROBE B — existing max: history 2, 8, 4 → advance → 9.
#[test]
fn probe_b_existing_max_advances_to_9() {
    let (_tmp, mut repo) = opened_repo("cea-pb");
    for epoch in [2, 8, 4] {
        repo.append_context_epoch(explicit_epoch(
            "project-1",
            epoch,
            ContextEpochTrigger::NewWave,
        ))
        .expect("seed");
    }
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance");
    assert_eq!(created.epoch, 9, "PROBE B: numeric max 8 → epoch 9");
}

// PROBE C — timestamp disagreement: the max-epoch row carries an
// older-looking advanced_at than a lower epoch → derivation uses only
// the numeric max.
#[test]
fn probe_c_timestamp_disagreement_ignored() {
    let (_tmp, mut repo) = opened_repo("cea-pc");
    let mut lower = explicit_epoch("project-1", 4, ContextEpochTrigger::NewWave);
    lower.advanced_at = "9999-12-31T23:59:59.999Z".to_string();
    let mut highest = explicit_epoch("project-1", 9, ContextEpochTrigger::NewWave);
    highest.advanced_at = "0001-01-01T00:00:00.000Z".to_string();
    repo.append_context_epoch(lower).expect("append 4");
    repo.append_context_epoch(highest).expect("append 9");
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance");
    assert_eq!(
        created.epoch, 10,
        "PROBE C: numeric max 9 → epoch 10, timestamps are never compared"
    );
}

// PROBE D — overflow: history contains i64::MAX → advance → explicit
// failure, no new row.
#[test]
fn probe_d_overflow_fails_with_no_new_row() {
    let (_tmp, mut repo) = opened_repo("cea-pd");
    repo.append_context_epoch(explicit_epoch(
        "project-1",
        i64::MAX,
        ContextEpochTrigger::TaskThreshold,
    ))
    .expect("seed i64::MAX");
    let rows_before = repo.count_table_rows("context_epoch").expect("rows");
    let error = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect_err("PROBE D: i64::MAX must fail");
    assert!(
        matches!(error, StateError::ContextEpochAdvanceOverflow { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        rows_before,
        "PROBE D: no new row"
    );
}

// PROBE E — rollback: derive next, insert inside the transaction, force
// the transaction to fail → no row; a rerun then succeeds with the same
// next value.
#[test]
fn probe_e_rollback_then_same_next_value_succeeds() {
    let (_tmp, mut repo) = opened_repo("cea-pe");
    advance(&mut repo, "project-1", ContextEpochTrigger::A1Init).expect("advance to 0");
    repo.run_transaction(|uow| {
        uow.insert_context_epoch(&explicit_epoch(
            "project-1",
            1,
            ContextEpochTrigger::NewWave,
        ))?;
        Err::<(), StateError>(StateError::UnitOfWorkFailed {
            detail: "forced test failure".to_string(),
        })
    })
    .expect_err("forced failure");
    assert_eq!(
        repo.find_context_epoch("project-1", 1).expect("find"),
        None,
        "PROBE E: the rolled-back insert left no row"
    );
    let retried = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave)
        .expect("PROBE E: rerun succeeds");
    assert_eq!(retried.epoch, 1, "PROBE E: the same next value succeeds");
}

// PROBE F — multi-project: P1 max 2, P2 max 20 → advancements create 3
// and 21.
#[test]
fn probe_f_multi_project_3_and_21() {
    let (_tmp, mut repo) = opened_repo("cea-pf");
    for epoch in 0..=2 {
        repo.append_context_epoch(explicit_epoch("P1", epoch, ContextEpochTrigger::NewWave))
            .expect("seed P1");
    }
    repo.append_context_epoch(explicit_epoch("P2", 20, ContextEpochTrigger::NewWave))
        .expect("seed P2");
    let p1 = advance(&mut repo, "P1", ContextEpochTrigger::NewWave).expect("advance P1");
    let p2 = advance(&mut repo, "P2", ContextEpochTrigger::NewWave).expect("advance P2");
    assert_eq!(p1.epoch, 3, "PROBE F: P1 max 2 → 3");
    assert_eq!(p2.epoch, 21, "PROBE F: P2 max 20 → 21");
}

// PROBE G — manifest isolation: snapshot the manifest, advance the
// epoch, read the manifest back — identical.
#[test]
fn probe_g_manifest_isolation() {
    let (_tmp, mut repo) = opened_repo("cea-pg");
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    repo.create_context_manifest(minimal_manifest("manifest-1", "role-1"))
        .expect("manifest create");
    let snapshot = repo
        .find_context_manifest("manifest-1")
        .expect("find")
        .expect("present");
    advance(&mut repo, "project-1", ContextEpochTrigger::ContractChange).expect("advance");
    assert_eq!(
        repo.find_context_manifest("manifest-1").expect("find"),
        Some(snapshot),
        "PROBE G: the manifest is identical after advancement"
    );
}

// PROBE H — LogicalRole isolation: snapshot the role, advance —
// identical.
#[test]
fn probe_h_logical_role_isolation() {
    let (_tmp, mut repo) = opened_repo("cea-ph");
    let role = minimal_role("role-1");
    repo.create_logical_role(role.clone()).expect("role create");
    advance(
        &mut repo,
        "project-1",
        ContextEpochTrigger::SecurityEscalation,
    )
    .expect("advance");
    assert_eq!(
        repo.find_logical_role("role-1").expect("find"),
        Some(role),
        "PROBE H: the role is identical after advancement"
    );
}

// PROBE I — event isolation: seed and count events, advance — count
// unchanged.
#[test]
fn probe_i_event_isolation() {
    let (_tmp, mut repo) = opened_repo("cea-pi");
    repo.append_event(minimal_event()).expect("seed one event");
    assert_eq!(repo.count_table_rows("event").expect("rows"), 1);
    advance(
        &mut repo,
        "project-1",
        ContextEpochTrigger::BeforeA2Integration,
    )
    .expect("advance");
    assert_eq!(
        repo.count_table_rows("event").expect("rows"),
        1,
        "PROBE I: the event count is unchanged by advancement"
    );
}

// PROBE J — explicit append interaction: append epoch 50 manually, then
// advance → 51.
#[test]
fn probe_j_explicit_append_then_advance_51() {
    let (_tmp, mut repo) = opened_repo("cea-pj");
    advance(&mut repo, "project-1", ContextEpochTrigger::A1Init).expect("advance to 0");
    repo.append_context_epoch(explicit_epoch(
        "project-1",
        50,
        ContextEpochTrigger::NewWave,
    ))
    .expect("explicit append 50");
    let created = advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance");
    assert_eq!(created.epoch, 51, "PROBE J: explicit max 50 → 51");
}

// T68 — the accepted A3-010 ContextManifest behavior remains unchanged:
// create/read round-trip, duplicate-manifest refusal, and the
// one-manifest-per-role guard all still behave identically alongside
// advancement.
#[test]
fn t68_context_manifest_behavior_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t68");
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    let manifest = minimal_manifest("manifest-1", "role-1");
    repo.create_context_manifest(manifest.clone())
        .expect("manifest create");
    assert_eq!(
        repo.find_context_manifest("manifest-1").expect("find"),
        Some(manifest.clone()),
        "manifest round-trip unchanged"
    );
    let error = repo
        .create_context_manifest(manifest)
        .expect_err("duplicate manifest_id still refused");
    assert!(
        matches!(error, StateError::ContextManifestAlreadyExists { .. }),
        "unexpected error: {error}"
    );
    let error = repo
        .create_context_manifest(minimal_manifest("manifest-2", "role-1"))
        .expect_err("second manifest for one role still refused");
    assert!(
        matches!(
            error,
            StateError::ContextManifestRoleAlreadyHasManifest { .. }
        ),
        "unexpected error: {error}"
    );
    advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance");
    assert_eq!(
        repo.find_context_manifest("manifest-1").expect("find"),
        Some(minimal_manifest("manifest-1", "role-1")),
        "the authoritative manifest is unchanged by advancement"
    );
}

// T69 — the accepted A3-007 strict EventEnvelope boundary remains
// unchanged: strict append/read still works beside advancement, and
// advancement never touches the event surface.
#[test]
fn t69_event_envelope_boundary_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t69");
    let event = minimal_event();
    repo.append_event(event.clone()).expect("append");
    assert_eq!(
        repo.find_event(EVENT_ULID).expect("find"),
        Some(event),
        "strict envelope round-trip unchanged"
    );
    advance(&mut repo, "project-1", ContextEpochTrigger::NewWave).expect("advance");
    assert_eq!(
        repo.count_table_rows("event").expect("rows"),
        1,
        "advancement leaves the event log untouched"
    );
}

// T70 — the accepted ExecutorBinding behavior through A3-009 remains
// unchanged: create/read and the single-active-binding guard still
// behave identically alongside advancement.
#[test]
fn t70_executor_binding_behavior_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t70");
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    let binding = minimal_binding("binding-1", "role-1");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    assert_eq!(
        repo.find_executor_binding("binding-1").expect("find"),
        Some(binding),
        "binding round-trip unchanged"
    );
    let error = repo
        .create_executor_binding(minimal_binding("binding-2", "role-1"))
        .expect_err("single-active-binding guard still refuses");
    assert!(
        matches!(error, StateError::ExecutorBindingUnreleasedConflict { .. }),
        "unexpected error: {error}"
    );
    repo.release_executor_binding(
        "binding-1",
        "2026-08-17T11:00:00.000Z",
        ReleaseReason::UserRequest,
    )
    .expect("release");
    let error = repo
        .renew_executor_binding_lease("binding-1", "2027-02-02T00:00:00.000Z")
        .expect_err("renewal after release still refused");
    assert!(
        matches!(error, StateError::ExecutorBindingAlreadyReleased { .. }),
        "unexpected error: {error}"
    );
}

// T71 — the accepted LogicalRole behavior remains unchanged: create/read
// and duplicate refusal still behave identically alongside advancement.
#[test]
fn t71_logical_role_behavior_unchanged() {
    let (_tmp, mut repo) = opened_repo("cea-t71");
    let role = minimal_role("role-1");
    repo.create_logical_role(role.clone()).expect("role create");
    assert_eq!(
        repo.find_logical_role("role-1").expect("find"),
        Some(role),
        "role round-trip unchanged"
    );
    let error = repo
        .create_logical_role(minimal_role("role-1"))
        .expect_err("duplicate role still refused");
    assert!(
        matches!(error, StateError::LogicalRoleAlreadyExists { .. }),
        "unexpected error: {error}"
    );
}
