//! Deterministic tests for durable ContextEpoch history persistence
//! (migration 0007 parent-record slice).
//!
//! All tests use real temporary SQLite database files under the system
//! temporary directory (never inside the repository). Storage-backstop and
//! corruption checks that need SQL beyond the public repository API run
//! through crate-private test helpers only and are `#[cfg(test)]`-gated;
//! no production SQL surface exists for them.
//!
//! Compile-time/API-absence invariants (T54–T56 method absence, T65–T71,
//! T76–T78) are established by the public surface of this crate and by
//! source inspection, following the conventions of `context_manifest_tests`
//! and `event_tests`; the runtime-observable parts of those properties
//! (storage-backstop refusal, derived-latest stability, no bootstrap row,
//! cross-entity non-mutation, column-set immobility, and unchanged
//! baseline-slice behavior) are exercised in real tests below:
//!
//! * no `update_context_epoch`, `delete_context_epoch`,
//!   `replace_context_epoch`, `upsert_context_epoch`, `set_trigger`,
//!   `set_advanced_at`, or `renumber_epoch` — append/read only;
//! * no `INSERT OR REPLACE` / `REPLACE` / `UPSERT` /
//!   `ON CONFLICT DO UPDATE` / `UPDATE` / `DELETE` anywhere in this
//!   slice's SQL (duplicates are refused by pre-check + primary-key
//!   backstop);
//! * beyond the single authorized
//!   `advance_context_epoch(project_id, advanced_at, trigger, invalidated_role_ids)`, no
//!   `increment_context_epoch`, `next_context_epoch`, `peek_next_epoch`,
//!   `reserve_epoch`, `allocate_epoch`, `set_current_epoch`,
//!   `increment_epoch_without_insert`, `invalidate_context`,
//!   `reconcile_epoch`, `advance_if_changed`, or any other
//!   epoch-advancement/increment API;
//! * no trigger handler: `HOST_SWITCH`, `CONTEXT_COMPACTION`,
//!   `SERIOUS_A4_REJECTION`, `BEFORE_GOAL_COMPLETE`, and every other
//!   trigger are persisted metadata only;
//! * no digest comparison or computation API, no source loading of any
//!   kind (the module imports no `std::fs`, no HTTP client, no process
//!   spawning, never executes a `STATE_QUERY` target, never dereferences
//!   an artifact reference), and no hashing dependency;
//! * no `rehydrate`, `reconcile`, `resume_role`, `rebuild_context`,
//!   `mark_rehydrated`, or `set_last_rehydrated_at`;
//! * no `changed_sources` persistence and no invalidation data embedded in
//!   the four-field parent record;
//! * no `chrono`, `time`, `SystemTime`, `Instant`, or other clock
//!   dependency: timestamps are opaque strings;
//! * no ContextManifest/LogicalRole/ExecutorBinding mutation method and
//!   no `EventType` or event-schema change; `CONTEXT_EPOCH_ADVANCED` is
//!   never emitted by persistence;
//! * no arbitrary-SQL public API, no `rusqlite` type crossing the public
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

/// A minimal contract-valid ContextEpoch: only the four core fields.
fn minimal_epoch(project_id: &str, epoch: i64, trigger: ContextEpochTrigger) -> ContextEpoch {
    ContextEpoch {
        project_id: project_id.to_string(),
        epoch,
        advanced_at: "2026-08-17T10:00:00.000Z".to_string(),
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

// T01 — a fresh version-0 database bootstraps 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7,
// with exactly seven registered migrations ending at version 7 and exactly
// one metadata row per migration.
#[test]
fn t01_fresh_database_bootstraps_to_schema_version_7() {
    let registered = migrations::registered();
    assert_eq!(
        registered.len(),
        8,
        "exactly eight registered migrations (v0001–v0008) may exist"
    );
    assert_eq!(
        registered.last().expect("chain is non-empty").version,
        8,
        "the registered chain must end at version 8"
    );
    let tmp = TempDir::new("ce-t01");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("fresh database bootstraps");
    assert_eq!(repo.schema_version().expect("version read"), 8);
    assert_eq!(
        repo.count_table_rows("state_schema_version").expect("rows"),
        8,
        "one metadata row per applied migration"
    );
}

// T02 — a version-7 database reopens successfully and idempotently.
#[test]
fn t02_version_7_database_reopens_idempotently() {
    let tmp = TempDir::new("ce-t02");
    for _ in 0..3 {
        let repo = SqliteStateRepository::open(tmp.db_path()).expect("every reopen succeeds");
        assert_eq!(repo.schema_version().expect("version read"), 8);
        assert_eq!(
            repo.count_table_rows("state_schema_version").expect("rows"),
            8,
            "one metadata row per applied migration, never duplicated by reopen"
        );
    }
}

// T03 — ordinary open of an initialized schema-version-6 database fails
// closed with an explicit version mismatch and never silently migrates it.
#[test]
fn t03_ordinary_open_of_version_6_fails_closed() {
    let tmp = TempDir::new("ce-t03");
    let version_6_chain = &migrations::registered()[..6];
    drop(
        SqliteStateRepository::open_with_migrations(tmp.db_path(), version_6_chain)
            .expect("bootstrap at version 6"),
    );
    let error = SqliteStateRepository::open(tmp.db_path())
        .expect_err("ordinary open must not silently migrate a version-6 database");
    assert!(
        matches!(
            error,
            StateError::SchemaVersionMismatch {
                found: 6,
                supported: 8
            }
        ),
        "unexpected error: {error}"
    );
    // The refused open left the database untouched at version 6, with no
    // migration-7 tables materialized by the failed open.
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_6_chain)
        .expect("database still opens with its original chain");
    assert_eq!(repo.schema_version().expect("version read"), 6);
    assert!(
        !repo.table_exists("context_epoch").expect("table check"),
        "the failed ordinary open must not create migration-7 tables"
    );
}

// T04 — migration 7 creates exactly one `context_epoch` table with exactly
// its four conceptual columns in declared order, and no trigger or view on
// it; the full chain still creates exactly its own nine tables.
#[test]
fn t04_migration_v7_creates_exactly_the_authorized_schema() {
    let tmp = TempDir::new("ce-t04");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert!(
        repo.table_exists("context_epoch").expect("table check"),
        "context_epoch must exist after migration 7"
    );
    assert_eq!(
        repo.table_columns("context_epoch")
            .expect("columns")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["project_id", "epoch", "advanced_at", "trigger"],
        "context_epoch must have exactly its conceptual columns"
    );
    assert!(
        repo.sqlite_master_entries("trigger", "context_epoch")
            .expect("triggers")
            .is_empty(),
        "migration 7 must add no trigger on context_epoch"
    );
    assert!(
        repo.sqlite_master_entries("view", "context_epoch")
            .expect("views")
            .is_empty(),
        "migration 7 must add no view on context_epoch"
    );
    // The migration-7 SQL creates exactly one table (one CREATE TABLE, no
    // CREATE INDEX / TRIGGER / VIEW).
    let migration_sql = migrations::registered()[6].sql;
    assert_eq!(migration_sql.matches("CREATE TABLE").count(), 1);
    assert_eq!(migration_sql.matches("CREATE INDEX").count(), 0);
    assert_eq!(migration_sql.matches("CREATE TRIGGER").count(), 0);
    assert_eq!(migration_sql.matches("CREATE VIEW").count(), 0);
    // The full chain creates exactly its own nine tables.
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
    ];
    expected.sort_unstable();
    assert_eq!(
        repo.list_tables().expect("tables"),
        expected,
        "the registered chain must create exactly its own tables"
    );
}

// T05 — no changed-source or reconciliation storage is introduced.
#[test]
fn t05_no_changed_source_or_reconciliation_schema() {
    let tmp = TempDir::new("ce-t05");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    for forbidden in [
        "context_epoch_changed_source",
        "context_epoch_reconciliation",
        "context_epoch_current",
        "context_epoch_state",
        "context_epoch_pointer",
        "context_epoch_manifest",
        "context_epoch_event",
        "context_epoch_source_digest",
        "invalidated_role",
        "role_epoch",
        "role_invalidated_at",
    ] {
        assert!(
            !repo.table_exists(forbidden).expect("table check"),
            "no {forbidden} storage may exist"
        );
    }
}

// T06 — no current-epoch pointer schema exists: no `current_epoch` column
// on any table and no current-epoch table; appending epochs never changes
// the table set.
#[test]
fn t06_no_current_epoch_pointer_schema() {
    let tmp = TempDir::new("ce-t06");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    for table in [
        "state_schema_version",
        "logical_role",
        "executor_binding",
        "event",
        "context_manifest",
        "context_epoch",
    ] {
        assert!(
            !repo
                .table_columns(table)
                .expect("columns")
                .iter()
                .any(|column| column == "current_epoch"),
            "no current_epoch column may exist on {table}"
        );
    }
    let tables_before = repo.list_tables().expect("tables");
    repo.append_context_epoch(minimal_epoch("project-1", 4, ContextEpochTrigger::NewWave))
        .expect("append");
    repo.append_context_epoch(minimal_epoch(
        "project-1",
        9,
        ContextEpochTrigger::HostSwitch,
    ))
    .expect("append");
    assert_eq!(
        repo.list_tables().expect("tables"),
        tables_before,
        "appending epochs must not create any pointer/projection storage"
    );
}

// T07 — the smallest valid ContextEpoch (epoch 0, minimal fields)
// appends successfully as exactly one row.
#[test]
fn t07_append_minimum_valid_epoch() {
    let (tmp, mut repo) = opened_repo("ce-t07");
    repo.append_context_epoch(minimal_epoch("project-1", 0, ContextEpochTrigger::A1Init))
        .expect("minimum valid ContextEpoch appends");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        1,
        "exactly one row per appended record"
    );
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        1,
        "the row is durable"
    );
}

// T08 — the exact-record read finds the persisted record.
#[test]
fn t08_find_exact_context_epoch() {
    let (_tmp, repo) = opened_repo("ce-t08");
    assert!(
        repo.find_context_epoch("project-1", 2)
            .expect("find")
            .is_none(),
        "nothing persisted yet"
    );
}

// T09 — a ContextEpoch round-trips exactly through append + find.
#[test]
fn t09_exact_round_trip() {
    let (_tmp, mut repo) = opened_repo("ce-t09");
    let record = ContextEpoch {
        project_id: "project-1".to_string(),
        epoch: 12,
        advanced_at: "2026-08-17T12:34:56.789Z".to_string(),
        trigger: ContextEpochTrigger::SecurityEscalation,
    };
    repo.append_context_epoch(record.clone())
        .expect("append succeeds");
    assert_eq!(
        repo.find_context_epoch("project-1", 12).expect("find"),
        Some(record),
        "the exact persisted record must read back unchanged"
    );
}

// T10 — a successful append survives close/reopen for both exact and
// latest reads.
#[test]
fn t10_close_reopen_durability() {
    let (tmp, mut repo) = opened_repo("ce-t10");
    let first = minimal_epoch("project-1", 0, ContextEpochTrigger::A1Init);
    let second = minimal_epoch("project-1", 1, ContextEpochTrigger::A2Init);
    repo.append_context_epoch(first.clone()).expect("append 0");
    repo.append_context_epoch(second.clone()).expect("append 1");
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_context_epoch("project-1", 0).expect("find"),
        Some(first),
        "exact read survives close/reopen"
    );
    assert_eq!(
        repo.find_context_epoch("project-1", 1).expect("find"),
        Some(second.clone()),
        "exact read survives close/reopen"
    );
    assert_eq!(
        repo.find_latest_context_epoch("project-1").expect("latest"),
        Some(second),
        "latest read survives close/reopen"
    );
}

// T11 — a missing exact epoch returns deterministic absence.
#[test]
fn t11_missing_exact_epoch_returns_none() {
    let (_tmp, mut repo) = opened_repo("ce-t11");
    repo.append_context_epoch(minimal_epoch("project-1", 5, ContextEpochTrigger::NewWave))
        .expect("append");
    assert_eq!(
        repo.find_context_epoch("project-1", 4).expect("find"),
        None,
        "a gap epoch is deterministic absence, never fabricated"
    );
    assert_eq!(
        repo.find_context_epoch("other-project", 5).expect("find"),
        None,
        "the same epoch for another project is absence"
    );
}

// T12 — no epochs for a project: latest is deterministic None.
#[test]
fn t12_latest_none_for_project_without_epochs() {
    let (_tmp, mut repo) = opened_repo("ce-t12");
    repo.append_context_epoch(minimal_epoch("project-1", 3, ContextEpochTrigger::NewWave))
        .expect("append");
    assert_eq!(
        repo.find_latest_context_epoch("project-2").expect("latest"),
        None,
        "a project with no records has no latest epoch"
    );
}

// T13 — latest lookup with one record returns that record.
#[test]
fn t13_latest_with_one_record_returns_it() {
    let (_tmp, mut repo) = opened_repo("ce-t13");
    let record = minimal_epoch("project-1", 6, ContextEpochTrigger::ContractChange);
    repo.append_context_epoch(record.clone()).expect("append");
    assert_eq!(
        repo.find_latest_context_epoch("project-1").expect("latest"),
        Some(record)
    );
}

// T14 — latest among epochs 0, 2, 7 is epoch 7: highest numeric epoch.
#[test]
fn t14_latest_is_highest_numeric_epoch() {
    let (_tmp, mut repo) = opened_repo("ce-t14");
    for epoch in [0, 2, 7] {
        repo.append_context_epoch(minimal_epoch(
            "project-1",
            epoch,
            ContextEpochTrigger::NewWave,
        ))
        .expect("append");
    }
    let latest = repo
        .find_latest_context_epoch("project-1")
        .expect("latest")
        .expect("present");
    assert_eq!(latest.epoch, 7, "latest must be the highest numeric epoch");
}

// T15 — latest uses the numeric epoch, never advanced_at ordering.
#[test]
fn t15_latest_uses_epoch_not_advanced_at() {
    let (_tmp, mut repo) = opened_repo("ce-t15");
    let mut lexically_latest = minimal_epoch("project-1", 4, ContextEpochTrigger::NewWave);
    lexically_latest.advanced_at = "9999-12-31T23:59:59.999Z".to_string();
    let mut lexically_earliest = minimal_epoch("project-1", 9, ContextEpochTrigger::NewWave);
    lexically_earliest.advanced_at = "0001-01-01T00:00:00.001Z".to_string();
    repo.append_context_epoch(lexically_latest).expect("append");
    repo.append_context_epoch(lexically_earliest)
        .expect("append");
    let latest = repo
        .find_latest_context_epoch("project-1")
        .expect("latest")
        .expect("present");
    assert_eq!(
        latest.epoch, 9,
        "the numeric epoch alone defines latest, never the timestamp string"
    );
}

// T16 — out-of-order insertion 8 then 4 succeeds and latest remains 8.
#[test]
fn t16_out_of_order_insertion_latest_unchanged() {
    let (_tmp, mut repo) = opened_repo("ce-t16");
    repo.append_context_epoch(minimal_epoch("project-1", 8, ContextEpochTrigger::NewWave))
        .expect("append epoch 8");
    repo.append_context_epoch(minimal_epoch("project-1", 4, ContextEpochTrigger::NewWave))
        .expect("append epoch 4 out of order");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        2,
        "both out-of-order records persist"
    );
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .epoch,
        8,
        "out-of-order insertion must not change the derived latest"
    );
}

// T17 — the same epoch number may exist for two different project_ids.
#[test]
fn t17_same_epoch_for_different_projects() {
    let (_tmp, mut repo) = opened_repo("ce-t17");
    let first = minimal_epoch("project-A", 1, ContextEpochTrigger::A1Init);
    let second = minimal_epoch("project-B", 1, ContextEpochTrigger::A2Init);
    repo.append_context_epoch(first.clone())
        .expect("append A/1");
    repo.append_context_epoch(second.clone())
        .expect("append B/1: uniqueness is (project_id, epoch), never global");
    assert_eq!(
        repo.find_context_epoch("project-A", 1).expect("find"),
        Some(first)
    );
    assert_eq!(
        repo.find_context_epoch("project-B", 1).expect("find"),
        Some(second)
    );
}

// T18 — a duplicate (project_id, epoch) fails explicitly with
// ContextEpochAlreadyExists.
#[test]
fn t18_duplicate_fails_explicitly() {
    let (_tmp, mut repo) = opened_repo("ce-t18");
    repo.append_context_epoch(minimal_epoch("project-1", 3, ContextEpochTrigger::NewWave))
        .expect("first append");
    let error = repo
        .append_context_epoch(minimal_epoch(
            "project-1",
            3,
            ContextEpochTrigger::HostSwitch,
        ))
        .expect_err("duplicate (project_id, epoch) must fail");
    assert!(
        matches!(
            &error,
            StateError::ContextEpochAlreadyExists {
                project_id,
                epoch: 3
            } if project_id == "project-1"
        ),
        "unexpected error: {error}"
    );
}

// T19 — a duplicate failure leaves the original record unchanged.
#[test]
fn t19_duplicate_leaves_original_unchanged() {
    let (_tmp, mut repo) = opened_repo("ce-t19");
    let original = ContextEpoch {
        project_id: "project-1".to_string(),
        epoch: 3,
        advanced_at: "2026-08-16T10:00:00.000Z".to_string(),
        trigger: ContextEpochTrigger::TaskThreshold,
    };
    repo.append_context_epoch(original.clone()).expect("append");
    let mut conflicting = original.clone();
    conflicting.advanced_at = "2027-01-01T00:00:00.000Z".to_string();
    conflicting.trigger = ContextEpochTrigger::SecurityEscalation;
    repo.append_context_epoch(conflicting)
        .expect_err("duplicate with changed advanced_at/trigger must fail");
    assert_eq!(
        repo.find_context_epoch("project-1", 3).expect("find"),
        Some(original),
        "the original record must remain exactly as first persisted"
    );
}

// T20 — a duplicate failure creates no second record.
#[test]
fn t20_duplicate_failure_creates_no_second_record() {
    let (_tmp, mut repo) = opened_repo("ce-t20");
    repo.append_context_epoch(minimal_epoch("project-1", 3, ContextEpochTrigger::NewWave))
        .expect("first append");
    repo.append_context_epoch(minimal_epoch("project-1", 3, ContextEpochTrigger::NewWave))
        .expect_err("duplicate fails");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        1,
        "exactly one record may exist for (project-1, 3)"
    );
}

// T21 — an empty project_id is rejected by validation.
#[test]
fn t21_empty_project_id_rejected() {
    let (_tmp, mut repo) = opened_repo("ce-t21");
    let error = repo
        .append_context_epoch(minimal_epoch("", 0, ContextEpochTrigger::A1Init))
        .expect_err("empty project_id must fail validation");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "no record may persist from a rejected append"
    );
}

// T22 — an overlong project_id (201 scalar values) is rejected.
#[test]
fn t22_overlong_project_id_rejected() {
    let (_tmp, mut repo) = opened_repo("ce-t22");
    let overlong = "p".repeat(201);
    let error = repo
        .append_context_epoch(minimal_epoch(&overlong, 0, ContextEpochTrigger::A1Init))
        .expect_err("overlong project_id must fail validation");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
    // Exactly 200 remains accepted (boundary).
    repo.append_context_epoch(minimal_epoch(
        &"p".repeat(200),
        0,
        ContextEpochTrigger::A1Init,
    ))
    .expect("200-character project_id is the accepted boundary");
}

// T23 — project_id round-trips byte-for-byte unchanged.
#[test]
fn t23_project_id_round_trips_byte_for_byte() {
    let (_tmp, mut repo) = opened_repo("ce-t23");
    let project_id = "project / ☃-42 :: with spaces and punctuation";
    let record = minimal_epoch(project_id, 2, ContextEpochTrigger::NewWave);
    repo.append_context_epoch(record.clone()).expect("append");
    assert_eq!(
        repo.find_context_epoch(project_id, 2).expect("find"),
        Some(record),
        "project_id must persist byte-for-byte, never normalized"
    );
}

// T24 — epoch zero is accepted.
#[test]
fn t24_epoch_zero_accepted() {
    let (_tmp, mut repo) = opened_repo("ce-t24");
    repo.append_context_epoch(minimal_epoch("project-1", 0, ContextEpochTrigger::A1Init))
        .expect("epoch 0 is a valid non-negative epoch");
    assert_eq!(
        repo.find_context_epoch("project-1", 0)
            .expect("find")
            .expect("present")
            .epoch,
        0
    );
}

// T25 — a large positive epoch is accepted unchanged.
#[test]
fn t25_positive_epoch_accepted() {
    let (_tmp, mut repo) = opened_repo("ce-t25");
    let large = 4_611_686_018_427_387_903_i64;
    repo.append_context_epoch(minimal_epoch(
        "project-1",
        large,
        ContextEpochTrigger::NewWave,
    ))
    .expect("a positive epoch appends");
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .epoch,
        large,
        "the exact supplied epoch persists, never incremented or renumbered"
    );
}

// T26 — a negative epoch is rejected by validation.
#[test]
fn t26_negative_epoch_rejected() {
    let (_tmp, mut repo) = opened_repo("ce-t26");
    let error = repo
        .append_context_epoch(minimal_epoch("project-1", -1, ContextEpochTrigger::A1Init))
        .expect_err("negative epoch must fail validation");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "no record may persist from a rejected append"
    );
    // A negative query epoch fails validation too, never a SQL query for an
    // invalid persisted identity.
    let error = repo
        .find_context_epoch("project-1", -5)
        .expect_err("negative query epoch must fail validation");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
    // Invalid read inputs fail the same way.
    let error = repo
        .find_latest_context_epoch("")
        .expect_err("empty query project_id must fail validation");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
}

// T27 — an empty advanced_at is rejected.
#[test]
fn t27_empty_advanced_at_rejected() {
    let (_tmp, mut repo) = opened_repo("ce-t27");
    let mut record = minimal_epoch("project-1", 1, ContextEpochTrigger::A1Init);
    record.advanced_at = String::new();
    let error = repo
        .append_context_epoch(record)
        .expect_err("empty advanced_at must fail validation");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
}

// T28 — advanced_at round-trips byte-for-byte.
#[test]
fn t28_advanced_at_round_trips_byte_for_byte() {
    let (_tmp, mut repo) = opened_repo("ce-t28");
    let advanced_at = "  not-a-parsed-timestamp ☃ ";
    let record = ContextEpoch {
        project_id: "project-1".to_string(),
        epoch: 2,
        advanced_at: advanced_at.to_string(),
        trigger: ContextEpochTrigger::ContextCompaction,
    };
    repo.append_context_epoch(record.clone()).expect("append");
    assert_eq!(
        repo.find_context_epoch("project-1", 2).expect("find"),
        Some(record),
        "advanced_at must persist byte-for-byte, never normalized"
    );
}

// T29 — no timestamp parsing or normalization occurs: a non-RFC3339
// advanced_at is accepted unchanged on the append path.
#[test]
fn t29_no_timestamp_parsing_or_normalization() {
    let (_tmp, mut repo) = opened_repo("ce-t29");
    let advanced_at = "sometime-yesterday-ish";
    repo.append_context_epoch(ContextEpoch {
        project_id: "project-1".to_string(),
        epoch: 2,
        advanced_at: advanced_at.to_string(),
        trigger: ContextEpochTrigger::HostSwitch,
    })
    .expect("advanced_at is opaque and needs no timestamp format");
    assert_eq!(
        repo.find_context_epoch("project-1", 2)
            .expect("find")
            .expect("present")
            .advanced_at,
        advanced_at,
        "the opaque timestamp string must be stored exactly as supplied"
    );
}

// T30 — A1_INIT round-trips.
#[test]
fn t30_a1_init_round_trips() {
    assert_eq!(ContextEpochTrigger::A1Init.as_str(), "A1_INIT");
}

// T31 — A2_INIT round-trips.
#[test]
fn t31_a2_init_round_trips() {
    assert_eq!(ContextEpochTrigger::A2Init.as_str(), "A2_INIT");
}

// T32 — MODEL_REPLACEMENT round-trips.
#[test]
fn t32_model_replacement_round_trips() {
    assert_eq!(
        ContextEpochTrigger::ModelReplacement.as_str(),
        "MODEL_REPLACEMENT"
    );
}

// T33 — PROVIDER_REPLACEMENT round-trips.
#[test]
fn t33_provider_replacement_round_trips() {
    assert_eq!(
        ContextEpochTrigger::ProviderReplacement.as_str(),
        "PROVIDER_REPLACEMENT"
    );
}

// T34 — HOST_SWITCH round-trips.
#[test]
fn t34_host_switch_round_trips() {
    assert_eq!(ContextEpochTrigger::HostSwitch.as_str(), "HOST_SWITCH");
}

// T35 — CONTEXT_COMPACTION round-trips.
#[test]
fn t35_context_compaction_round_trips() {
    assert_eq!(
        ContextEpochTrigger::ContextCompaction.as_str(),
        "CONTEXT_COMPACTION"
    );
}

// T36 — ARCHITECTURE_CHANGE round-trips.
#[test]
fn t36_architecture_change_round_trips() {
    assert_eq!(
        ContextEpochTrigger::ArchitectureChange.as_str(),
        "ARCHITECTURE_CHANGE"
    );
}

// T37 — CONTRACT_CHANGE round-trips.
#[test]
fn t37_contract_change_round_trips() {
    assert_eq!(
        ContextEpochTrigger::ContractChange.as_str(),
        "CONTRACT_CHANGE"
    );
}

// T38 — NEW_WAVE round-trips.
#[test]
fn t38_new_wave_round_trips() {
    assert_eq!(ContextEpochTrigger::NewWave.as_str(), "NEW_WAVE");
}

// T39 — TASK_THRESHOLD round-trips.
#[test]
fn t39_task_threshold_round_trips() {
    assert_eq!(
        ContextEpochTrigger::TaskThreshold.as_str(),
        "TASK_THRESHOLD"
    );
}

// T40 — SERIOUS_A4_REJECTION round-trips.
#[test]
fn t40_serious_a4_rejection_round_trips() {
    assert_eq!(
        ContextEpochTrigger::SeriousA4Rejection.as_str(),
        "SERIOUS_A4_REJECTION"
    );
}

// T41 — SECURITY_ESCALATION round-trips.
#[test]
fn t41_security_escalation_round_trips() {
    assert_eq!(
        ContextEpochTrigger::SecurityEscalation.as_str(),
        "SECURITY_ESCALATION"
    );
}

// T42 — BEFORE_A2_INTEGRATION round-trips.
#[test]
fn t42_before_a2_integration_round_trips() {
    assert_eq!(
        ContextEpochTrigger::BeforeA2Integration.as_str(),
        "BEFORE_A2_INTEGRATION"
    );
}

// T43 — BEFORE_A1_INTEGRATION round-trips.
#[test]
fn t43_before_a1_integration_round_trips() {
    assert_eq!(
        ContextEpochTrigger::BeforeA1Integration.as_str(),
        "BEFORE_A1_INTEGRATION"
    );
}

// T44 — BEFORE_GOAL_COMPLETE round-trips.
#[test]
fn t44_before_goal_complete_round_trips() {
    assert_eq!(
        ContextEpochTrigger::BeforeGoalComplete.as_str(),
        "BEFORE_GOAL_COMPLETE"
    );
}

// T45 — exactly fifteen trigger values exist, each decoding from its
// exact durable representation, with no duplicates and no fallback
// variant, and every one round-trips through full persistence.
#[test]
fn t45_exactly_fifteen_triggers() {
    let all: Vec<&'static str> = ContextEpochTrigger::ALL
        .iter()
        .map(|t| t.as_str())
        .collect();
    assert_eq!(
        all,
        vec![
            "A1_INIT",
            "A2_INIT",
            "MODEL_REPLACEMENT",
            "PROVIDER_REPLACEMENT",
            "HOST_SWITCH",
            "CONTEXT_COMPACTION",
            "ARCHITECTURE_CHANGE",
            "CONTRACT_CHANGE",
            "NEW_WAVE",
            "TASK_THRESHOLD",
            "SERIOUS_A4_REJECTION",
            "SECURITY_ESCALATION",
            "BEFORE_A2_INTEGRATION",
            "BEFORE_A1_INTEGRATION",
            "BEFORE_GOAL_COMPLETE",
        ],
        "exactly the fifteen frozen rehydration triggers may exist"
    );
    let mut unique = all.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 15, "no duplicate trigger representations");
    // Every trigger round-trips through full append/find persistence.
    let (tmp, mut repo) = opened_repo("ce-t45");
    for (index, trigger) in ContextEpochTrigger::ALL.iter().enumerate() {
        let record = minimal_epoch("project-1", index as i64, *trigger);
        repo.append_context_epoch(record.clone())
            .expect("every frozen trigger appends");
        assert_eq!(
            repo.find_context_epoch("project-1", index as i64)
                .expect("find"),
            Some(record),
            "trigger {:?} must round-trip through persistence",
            trigger
        );
    }
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        15,
        "exactly fifteen records, one per trigger"
    );
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .trigger,
        ContextEpochTrigger::BeforeGoalComplete
    );
}

// T46 — an unknown stored trigger fails closed at the decode boundary:
// no UNKNOWN/OTHER/CUSTOM/MANUAL/TIMER fallback is ever produced.
#[test]
fn t46_unknown_stored_trigger_fails_closed() {
    for forbidden in [
        "NOT_A_TRIGGER",
        "UNKNOWN",
        "OTHER",
        "CUSTOM",
        "MANUAL",
        "TIMER",
        "a1_init",
        "",
    ] {
        assert!(
            ContextEpochTrigger::from_storage(forbidden).is_err(),
            "{forbidden:?} must not decode to any trigger"
        );
    }
    // Every real value still decodes.
    for trigger in ContextEpochTrigger::ALL {
        assert_eq!(
            ContextEpochTrigger::from_storage(trigger.as_str()).expect("frozen triggers decode"),
            trigger
        );
    }
}

// T47 — the SQLite CHECK independently rejects an unknown trigger on a
// direct SQL insert that bypasses the typed boundary.
#[test]
fn t47_sqlite_check_rejects_unknown_trigger() {
    let (_tmp, mut repo) = opened_repo("ce-t47");
    let result = repo.run_transaction(|uow| {
        uow.execute(
            "INSERT INTO context_epoch (project_id, epoch, advanced_at, trigger)
             VALUES (?1, ?2, ?3, ?4)",
            &[
                &"project-1",
                &0i64,
                &"2026-08-17T10:00:00.000Z",
                &"NOT_A_TRIGGER",
            ],
        )
    });
    assert!(
        result.is_err(),
        "the storage CHECK must refuse an unknown trigger"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "no row may persist from the refused insert"
    );
}

// T48 — a corrupt persisted negative epoch cannot exist: the CHECK
// prevents it at the storage layer.
#[test]
fn t48_negative_epoch_prevented_by_check() {
    let (_tmp, mut repo) = opened_repo("ce-t48");
    let result = repo.run_transaction(|uow| {
        uow.execute(
            "INSERT INTO context_epoch (project_id, epoch, advanced_at, trigger)
             VALUES (?1, ?2, ?3, ?4)",
            &[
                &"project-1",
                &-1i64,
                &"2026-08-17T10:00:00.000Z",
                &"NEW_WAVE",
            ],
        )
    });
    assert!(
        result.is_err(),
        "the storage CHECK must refuse a negative epoch"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "no row may persist from the refused insert"
    );
}

// T49 — a corrupt persisted empty advanced_at fails closed at decode:
// injectable (NOT NULL passes with the empty string), so the read must
// error rather than surface the record.
#[test]
fn t49_corrupt_empty_advanced_at_fails_closed() {
    let (_tmp, mut repo) = opened_repo("ce-t49");
    direct_exec(
        &mut repo,
        "INSERT INTO context_epoch (project_id, epoch, advanced_at, trigger)
         VALUES (?1, ?2, ?3, ?4)",
        &[&"project-1", &0i64, &"", &"NEW_WAVE"],
    );
    let error = repo
        .find_context_epoch("project-1", 0)
        .expect_err("empty persisted advanced_at must fail decode");
    assert!(
        matches!(error, StateError::ContextEpochDecodeFailed { .. }),
        "unexpected error: {error}"
    );
    let error = repo
        .find_latest_context_epoch("project-1")
        .expect_err("latest read must decode through the same fail-closed boundary");
    assert!(
        matches!(error, StateError::ContextEpochDecodeFailed { .. }),
        "unexpected error: {error}"
    );
}

// Extra — the latest read fails closed on a corrupt selected row: when
// the highest epoch's row is corrupted, find_latest returns an error and
// never silently falls back to an older valid row.
#[test]
fn extra_latest_read_corrupt_row_no_fallback() {
    let (_tmp, mut repo) = opened_repo("ce-extra");
    let valid = minimal_epoch("project-1", 1, ContextEpochTrigger::NewWave);
    repo.append_context_epoch(valid.clone()).expect("append 1");
    direct_exec(
        &mut repo,
        "INSERT INTO context_epoch (project_id, epoch, advanced_at, trigger)
         VALUES (?1, ?2, ?3, ?4)",
        &[&"project-1", &2i64, &"", &"HOST_SWITCH"],
    );
    let error = repo
        .find_latest_context_epoch("project-1")
        .expect_err("a corrupt latest row must fail the latest read");
    assert!(
        matches!(error, StateError::ContextEpochDecodeFailed { .. }),
        "unexpected error: {error}"
    );
    // The older valid record is still exactly readable; the failure did
    // not damage or replace it, and it was never returned as the latest.
    assert_eq!(
        repo.find_context_epoch("project-1", 1).expect("find"),
        Some(valid)
    );
}

// T50 — a corrupt persisted empty project_id fails closed at decode.
#[test]
fn t50_corrupt_empty_project_id_fails_closed() {
    let (_tmp, mut repo) = opened_repo("ce-t50");
    direct_exec(
        &mut repo,
        "INSERT INTO context_epoch (project_id, epoch, advanced_at, trigger)
         VALUES (?1, ?2, ?3, ?4)",
        &[&"", &0i64, &"2026-08-17T10:00:00.000Z", &"NEW_WAVE"],
    );
    let error = repo
        .find_context_epoch("", 0)
        .expect_err("empty project_id fails input validation");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
}

// T51 — a corrupt persisted overlong project_id fails closed: the row is
// injectable at the storage layer, but no typed query can ever surface it
// as a valid record — the query input is refused first, and the stored
// value is never normalized or truncated into validity.
#[test]
fn t51_corrupt_overlong_project_id_fails_closed() {
    let (_tmp, mut repo) = opened_repo("ce-t51");
    let overlong = "p".repeat(201);
    direct_exec(
        &mut repo,
        "INSERT INTO context_epoch (project_id, epoch, advanced_at, trigger)
         VALUES (?1, ?2, ?3, ?4)",
        &[&overlong, &0i64, &"2026-08-17T10:00:00.000Z", &"NEW_WAVE"],
    );
    let error = repo
        .find_context_epoch(&overlong, 0)
        .expect_err("an overlong project_id fails input validation before any query");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
    let error = repo
        .find_latest_context_epoch(&overlong)
        .expect_err("an overlong project_id fails input validation on latest too");
    assert!(
        matches!(error, StateError::ContextEpochValidation { .. }),
        "unexpected error: {error}"
    );
    // The corrupted row is durably present yet unreachable as a valid
    // record through every typed path.
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        1,
        "the corrupted row is present but never surfaced as valid"
    );
    // The 200-character boundary decodes fine through the same boundary.
    repo.append_context_epoch(minimal_epoch(
        &"q".repeat(200),
        0,
        ContextEpochTrigger::NewWave,
    ))
    .expect("boundary-length project_id appends");
    assert!(
        repo.find_context_epoch(&"q".repeat(200), 0)
            .expect("find")
            .is_some(),
        "boundary-length project_id decodes"
    );
}

// T52 — a successful append is visible to other connections only after
// commit: an uncommitted insert is invisible to a concurrent reader, and
// becomes visible after the transaction commits.
#[test]
fn t52_success_visible_only_after_commit() {
    let (tmp, mut repo) = opened_repo("ce-t52");
    let record = minimal_epoch("project-1", 7, ContextEpochTrigger::NewWave);
    repo.run_transaction(|uow| {
        uow.insert_context_epoch(&record)?;
        // A second connection on the same database file, inside the open
        // (uncommitted) writer transaction, must not observe the row.
        let reader = SqliteStateRepository::open(tmp.db_path()).expect("reader connection");
        assert_eq!(
            reader.count_table_rows("context_epoch").expect("rows"),
            0,
            "an uncommitted append must be invisible to other connections"
        );
        Ok(())
    })
    .expect("transaction commits");
    let reader = SqliteStateRepository::open(tmp.db_path()).expect("fresh reader");
    assert_eq!(
        reader.find_context_epoch("project-1", 7).expect("find"),
        Some(record),
        "the committed record is visible after commit"
    );
}

// T53 — a forced transaction rollback leaves no epoch row.
#[test]
fn t53_forced_rollback_leaves_no_row() {
    let (_tmp, mut repo) = opened_repo("ce-t53");
    let error = repo
        .run_transaction(|uow| {
            uow.insert_context_epoch(&minimal_epoch("project-1", 5, ContextEpochTrigger::NewWave))?;
            Err::<(), StateError>(StateError::UnitOfWorkFailed {
                detail: "forced test failure".to_string(),
            })
        })
        .expect_err("forced failure surfaces its error");
    assert!(
        matches!(error, StateError::UnitOfWorkFailed { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "the rolled-back append must leave no row"
    );
    assert_eq!(
        repo.find_latest_context_epoch("project-1").expect("latest"),
        None,
        "no latest may derive from a rolled-back append"
    );
}

// T54/T55/T56 — no update, delete, or replace/upsert capability exists:
// duplicates are the only mutation refusal, the storage-level backstop
// maps a direct duplicate insert to the same typed duplicate error, and
// no second row ever appears. (Full API absence is compile-time; this is
// the runtime-observable backstop evidence.)
#[test]
fn t54_t55_t56_no_mutation_paths_duplicate_backstop_only() {
    let (_tmp, mut repo) = opened_repo("ce-t54");
    let original = minimal_epoch("project-1", 2, ContextEpochTrigger::NewWave);
    repo.append_context_epoch(original.clone()).expect("append");
    // The storage primary-key backstop refuses a direct duplicate insert.
    let result = repo.run_transaction(|uow| {
        uow.execute(
            "INSERT INTO context_epoch (project_id, epoch, advanced_at, trigger)
             VALUES (?1, ?2, ?3, ?4)",
            &[
                &"project-1",
                &2i64,
                &"2027-01-01T00:00:00.000Z",
                &"HOST_SWITCH",
            ],
        )
    });
    assert!(
        result.is_err(),
        "the composite primary key must refuse a duplicate row"
    );
    assert_eq!(
        repo.find_context_epoch("project-1", 2).expect("find"),
        Some(original),
        "the original record is untouched by the refused backstop insert"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        1,
        "exactly one record remains"
    );
}

// T57 — no mutable current-epoch pointer exists: latest is a pure derived
// query; appending never mutates the max and reads are read-only.
#[test]
fn t57_latest_is_derived_not_a_pointer() {
    let (_tmp, mut repo) = opened_repo("ce-t57");
    for epoch in [1, 5, 3] {
        repo.append_context_epoch(minimal_epoch(
            "project-1",
            epoch,
            ContextEpochTrigger::NewWave,
        ))
        .expect("append");
    }
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .epoch,
        5
    );
    // Repeated latest reads are stable and mutate nothing.
    for _ in 0..3 {
        assert_eq!(
            repo.find_latest_context_epoch("project-1")
                .expect("latest")
                .expect("present")
                .epoch,
            5,
            "latest must remain a stable derived query"
        );
    }
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        3,
        "reads never append or modify rows"
    );
}

// T58 — no automatic epoch-zero bootstrap: neither database open, nor
// LogicalRole / ContextManifest / ExecutorBinding creation, ever creates
// a ContextEpoch row.
#[test]
fn t58_no_automatic_epoch_zero_bootstrap() {
    let (tmp, mut repo) = opened_repo("ce-t58");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "bootstrap creates no epoch row"
    );
    assert_eq!(
        repo.find_latest_context_epoch("project-1").expect("latest"),
        None,
        "no latest may exist before an authorized append"
    );
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    repo.create_context_manifest(minimal_manifest("manifest-1", "role-1"))
        .expect("manifest create");
    repo.create_executor_binding(minimal_binding("binding-1", "role-1"))
        .expect("binding create");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "entity creation creates no epoch row"
    );
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "reopen creates no epoch row"
    );
}

// T59 — appending a ContextEpoch does not mutate any ContextManifest.
#[test]
fn t59_append_does_not_mutate_manifest() {
    let (_tmp, mut repo) = opened_repo("ce-t59");
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    let manifest = minimal_manifest("manifest-1", "role-1");
    repo.create_context_manifest(manifest.clone())
        .expect("manifest create");
    repo.append_context_epoch(minimal_epoch("project-1", 4, ContextEpochTrigger::NewWave))
        .expect("append epoch 4 — above the manifest's epoch-3 snapshot");
    assert_eq!(
        repo.find_context_manifest("manifest-1").expect("find"),
        Some(manifest),
        "the manifest (including its epoch snapshot and last_rehydrated_at) must be unchanged"
    );
}

// T60 — appending a ContextEpoch does not mutate any LogicalRole.
#[test]
fn t60_append_does_not_mutate_logical_role() {
    let (_tmp, mut repo) = opened_repo("ce-t60");
    let role = minimal_role("role-1");
    repo.create_logical_role(role.clone()).expect("role create");
    repo.append_context_epoch(minimal_epoch(
        "project-1",
        9,
        ContextEpochTrigger::HostSwitch,
    ))
    .expect("append");
    assert_eq!(
        repo.find_logical_role("role-1").expect("find"),
        Some(role),
        "the role (including its current_context_epoch field) must be unchanged"
    );
}

// T61 — appending a ContextEpoch does not mutate any ExecutorBinding.
#[test]
fn t61_append_does_not_mutate_executor_binding() {
    let (_tmp, mut repo) = opened_repo("ce-t61");
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    let binding = minimal_binding("binding-1", "role-1");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    repo.append_context_epoch(minimal_epoch(
        "project-1",
        2,
        ContextEpochTrigger::ModelReplacement,
    ))
    .expect("append");
    assert_eq!(
        repo.find_executor_binding("binding-1").expect("find"),
        Some(binding),
        "the binding must be unchanged"
    );
}

// T62 — appending a ContextEpoch emits no EventEnvelope of any type.
#[test]
fn t62_append_emits_no_event() {
    let (_tmp, mut repo) = opened_repo("ce-t62");
    repo.append_context_epoch(minimal_epoch("project-1", 1, ContextEpochTrigger::NewWave))
        .expect("append");
    repo.append_context_epoch(minimal_epoch(
        "project-1",
        2,
        ContextEpochTrigger::SecurityEscalation,
    ))
    .expect("append");
    assert_eq!(
        repo.count_table_rows("event").expect("rows"),
        0,
        "persistence emits no events, in particular no CONTEXT_EPOCH_ADVANCED"
    );
}

// T63/T64 — no changed_sources or embedded invalidation persistence exists
// on the context_epoch parent: it remains exactly its four core columns.
#[test]
fn t63_t64_no_changed_sources_or_invalidated_roles_columns() {
    let (_tmp, mut repo) = opened_repo("ce-t63");
    let columns = repo.table_columns("context_epoch").expect("columns");
    for forbidden in [
        "changed_sources",
        "changed_sources_json",
        "invalidated_role_ids",
        "invalidated_roles_json",
        "trigger_payload",
        "metadata_json",
        "reason",
        "details",
        "context_epoch_id",
    ] {
        assert!(
            !columns.iter().any(|column| column == forbidden),
            "no {forbidden} column may exist on context_epoch"
        );
    }
    repo.append_context_epoch(minimal_epoch("project-1", 1, ContextEpochTrigger::NewWave))
        .expect("append");
    assert_eq!(
        repo.table_columns("context_epoch").expect("columns"),
        vec![
            "project_id".to_string(),
            "epoch".to_string(),
            "advanced_at".to_string(),
            "trigger".to_string()
        ],
        "appending never widens the stored shape"
    );
}

// PROBE A — out-of-order persistence: append 10 then 3; both stored and
// latest = 10.
#[test]
fn probe_a_out_of_order_persistence() {
    let (_tmp, mut repo) = opened_repo("ce-pa");
    repo.append_context_epoch(minimal_epoch("project-1", 10, ContextEpochTrigger::NewWave))
        .expect("append 10");
    repo.append_context_epoch(minimal_epoch(
        "project-1",
        3,
        ContextEpochTrigger::TaskThreshold,
    ))
    .expect("append 3 out of order");
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        2,
        "both records persist"
    );
    assert_eq!(
        repo.find_latest_context_epoch("project-1")
            .expect("latest")
            .expect("present")
            .epoch,
        10
    );
}

// PROBE B — timestamp disagreement: epoch 4 with a lexically-later
// timestamp, epoch 9 with a lexically-earlier one; latest = 9.
#[test]
fn probe_b_timestamp_disagreement_ignored() {
    let (_tmp, mut repo) = opened_repo("ce-pb");
    let mut later_looking = minimal_epoch("project-1", 4, ContextEpochTrigger::NewWave);
    later_looking.advanced_at = "9999-12-31T23:59:59.999Z".to_string();
    let mut earlier_looking = minimal_epoch("project-1", 9, ContextEpochTrigger::NewWave);
    earlier_looking.advanced_at = "0001-01-01T00:00:00.000Z".to_string();
    repo.append_context_epoch(later_looking).expect("append 4");
    repo.append_context_epoch(earlier_looking)
        .expect("append 9");
    let latest = repo
        .find_latest_context_epoch("project-1")
        .expect("latest")
        .expect("present");
    assert_eq!(
        latest.epoch, 9,
        "latest must ignore advanced_at ordering entirely"
    );
}

// PROBE C — unknown trigger through direct storage is refused by the
// storage CHECK (the typed path cannot even construct it).
#[test]
fn probe_c_unknown_trigger_direct_storage_refused() {
    let (_tmp, mut repo) = opened_repo("ce-pc");
    let result = repo.run_transaction(|uow| {
        uow.execute(
            "INSERT INTO context_epoch (project_id, epoch, advanced_at, trigger)
             VALUES (?1, ?2, ?3, ?4)",
            &[
                &"project-1",
                &1i64,
                &"2026-08-17T10:00:00.000Z",
                &"MYSTERY_TRIGGER",
            ],
        )
    });
    assert!(
        result.is_err(),
        "direct storage of an unknown trigger must be refused by the CHECK"
    );
    assert_eq!(
        repo.count_table_rows("context_epoch").expect("rows"),
        0,
        "no row may persist"
    );
}

// PROBE D — duplicate with changed advanced_at and trigger: explicit
// duplicate failure, original unchanged.
#[test]
fn probe_d_duplicate_with_changed_fields() {
    let (_tmp, mut repo) = opened_repo("ce-pd");
    let original = ContextEpoch {
        project_id: "project-1".to_string(),
        epoch: 6,
        advanced_at: "2026-08-16T10:00:00.000Z".to_string(),
        trigger: ContextEpochTrigger::ContextCompaction,
    };
    repo.append_context_epoch(original.clone()).expect("append");
    let mut duplicate = original.clone();
    duplicate.advanced_at = "2027-05-05T05:05:05.005Z".to_string();
    duplicate.trigger = ContextEpochTrigger::ArchitectureChange;
    let error = repo
        .append_context_epoch(duplicate)
        .expect_err("duplicate must fail regardless of changed fields");
    assert!(
        matches!(
            &error,
            StateError::ContextEpochAlreadyExists {
                project_id,
                epoch: 6
            } if project_id == "project-1"
        ),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.find_context_epoch("project-1", 6).expect("find"),
        Some(original),
        "the original record is unchanged"
    );
}

// PROBE E — multi-project same epoch number: P1 epoch 5 and P2 epoch 5
// both persist.
#[test]
fn probe_e_multi_project_same_epoch_number() {
    let (_tmp, mut repo) = opened_repo("ce-pe");
    let p1 = minimal_epoch("P1", 5, ContextEpochTrigger::A1Init);
    let p2 = minimal_epoch("P2", 5, ContextEpochTrigger::A2Init);
    repo.append_context_epoch(p1.clone()).expect("P1/5");
    repo.append_context_epoch(p2.clone()).expect("P2/5");
    assert_eq!(
        repo.find_context_epoch("P1", 5).expect("find"),
        Some(p1.clone())
    );
    assert_eq!(
        repo.find_context_epoch("P2", 5).expect("find"),
        Some(p2.clone())
    );
    assert_eq!(
        repo.find_latest_context_epoch("P1").expect("latest"),
        Some(p1),
        "each project derives its own latest independently"
    );
}

// PROBE F — manifest isolation: snapshot a manifest, append epochs, read
// the manifest back — byte-equivalent.
#[test]
fn probe_f_manifest_isolation() {
    let (_tmp, mut repo) = opened_repo("ce-pf");
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    let manifest = minimal_manifest("manifest-1", "role-1");
    repo.create_context_manifest(manifest.clone())
        .expect("manifest create");
    let snapshot = repo
        .find_context_manifest("manifest-1")
        .expect("find")
        .expect("present");
    repo.append_context_epoch(minimal_epoch(
        "project-1",
        11,
        ContextEpochTrigger::ContractChange,
    ))
    .expect("append beyond the manifest's epoch-3 snapshot");
    assert_eq!(
        repo.find_context_manifest("manifest-1").expect("find"),
        Some(snapshot.clone()),
        "the manifest must remain byte-equivalent after epoch appends"
    );
    assert_eq!(snapshot, manifest);
}

// PROBE G — event isolation: with an existing event in the log, appending
// an epoch emits nothing — in particular no CONTEXT_EPOCH_ADVANCED.
#[test]
fn probe_g_event_isolation() {
    let (_tmp, mut repo) = opened_repo("ce-pg");
    let event = minimal_event();
    repo.append_event(event.clone())
        .expect("seed one unrelated event");
    assert_eq!(repo.count_table_rows("event").expect("rows"), 1);
    repo.append_context_epoch(minimal_epoch(
        "project-1",
        8,
        ContextEpochTrigger::BeforeGoalComplete,
    ))
    .expect("append");
    assert_eq!(
        repo.count_table_rows("event").expect("rows"),
        1,
        "no event may be emitted by ContextEpoch persistence"
    );
    assert_eq!(
        repo.find_event(EVENT_ULID).expect("find"),
        Some(event),
        "the seeded event is untouched"
    );
}

// T72 — the accepted A3-010 ContextManifest behavior remains unchanged:
// create/read round-trip, duplicate-manifest refusal, and the
// one-manifest-per-role guard all still behave identically alongside the
// new epoch storage.
#[test]
fn t72_context_manifest_behavior_unchanged() {
    let (_tmp, mut repo) = opened_repo("ce-t72");
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
}

// T73 — the accepted A3-007 strict EventEnvelope boundary remains
// unchanged: strict append/read still works beside epoch persistence, and
// epoch appends never touch the event surface.
#[test]
fn t73_event_envelope_boundary_unchanged() {
    let (_tmp, mut repo) = opened_repo("ce-t73");
    let event = minimal_event();
    repo.append_event(event.clone()).expect("append");
    assert_eq!(
        repo.find_event(EVENT_ULID).expect("find"),
        Some(event),
        "strict envelope round-trip unchanged"
    );
    repo.append_context_epoch(minimal_epoch("project-1", 1, ContextEpochTrigger::NewWave))
        .expect("append");
    assert_eq!(
        repo.count_table_rows("event").expect("rows"),
        1,
        "epoch persistence leaves the event log untouched"
    );
}

// T74 — the accepted ExecutorBinding behavior through A3-009 remains
// unchanged: create/read and the single-active-binding guard still behave
// identically alongside the new epoch storage.
#[test]
fn t74_executor_binding_behavior_unchanged() {
    let (_tmp, mut repo) = opened_repo("ce-t74");
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
    // Release remains write-once and refuses a repeat.
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

// T75 — the accepted LogicalRole behavior remains unchanged: create/read
// and duplicate refusal still behave identically alongside the new epoch
// storage.
#[test]
fn t75_logical_role_behavior_unchanged() {
    let (_tmp, mut repo) = opened_repo("ce-t75");
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
