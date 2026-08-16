//! Deterministic tests for append-only EventEnvelope persistence under
//! STRICT_W1_EVENT_BOUNDARY (T01–T55 of migration 0004).
//!
//! All runtime tests use real temporary SQLite database files under the
//! system temporary directory (never inside the repository). Storage-backstop
//! checks that need SQL beyond the public repository API run through
//! crate-private test helpers only and are `#[cfg(test)]`-gated.
//!
//! API-absence properties (T33, T34, T37, T38, T39, T40, T52, T53, T54,
//! T55) are established by compile-time type assertions and source
//! inspection of this crate's public surface, not by fabricated runtime
//! tests; the compile-time assertions are collected in dedicated `#[test]`
//! functions below and the inspection evidence is summarized at the bottom
//! of this module and reported in the task handoff.

use crate::error::StateError;
use crate::event::{
    ActorKind, EventActor, EventEnvelope, EventPayloadReference, EventSubject, EventType,
    SubjectKind,
};
use crate::executor_binding::ExecutorBinding;
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

/// The exact 13 structural columns of the `event` table, in declared order.
const EXPECTED_EVENT_COLUMNS: [&str; 13] = [
    "event_id",
    "project_id",
    "goal_id",
    "event_type",
    "actor_kind",
    "actor_id",
    "subject_kind",
    "subject_id",
    "occurred_at",
    "payload_reference",
    "payload_digest",
    "correlation_id",
    "epoch",
];

/// A canonical 26-character Crockford Base32 ULID used as the base for test
/// identities.
const BASE_ULID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAK";

/// A distinct canonical ULID per index: the last six characters encode the
/// index in Crockford Base32, so every generated id is exactly 26 alphabet
/// characters and distinct.
fn ulid_for(index: u64) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut suffix = [b'0'; 6];
    let mut value = index;
    for slot in suffix.iter_mut().rev() {
        *slot = ALPHABET[(value % 32) as usize];
        value /= 32;
    }
    let suffix = std::str::from_utf8(&suffix).expect("suffix is pure ASCII");
    format!("01ARZ3NDEKTSV4RRFFQ6{suffix}")
}

/// A minimal strict EventEnvelope: only required optional-absent structure,
/// epoch 0.
fn minimal_event(event_id: &str) -> EventEnvelope {
    EventEnvelope {
        event_id: event_id.to_string(),
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

/// A fully populated EventEnvelope with every optional field present.
fn full_event(event_id: &str) -> EventEnvelope {
    EventEnvelope {
        event_id: event_id.to_string(),
        project_id: "project-42".to_string(),
        goal_id: Some("goal-0007".to_string()),
        event_type: EventType::RoutingDecided,
        actor: EventActor {
            kind: ActorKind::Role,
            id: Some("role-a2-001".to_string()),
        },
        subject: EventSubject {
            kind: SubjectKind::Workspace,
            id: "worktree-state-context-007".to_string(),
        },
        occurred_at: "2026-08-17T10:15:00.000Z".to_string(),
        payload: EventPayloadReference {
            reference: "blob://routing-decisions/000042/record.md".to_string(),
            digest: "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
                .to_string(),
        },
        correlation_id: "corr-lineage-0001".to_string(),
        epoch: 7,
    }
}

/// A minimal contract-valid LogicalRole for cross-entity non-mutation tests.
fn minimal_role(role_id: &str, role_type: LogicalRoleType) -> LogicalRole {
    LogicalRole {
        role_id: role_id.to_string(),
        project_id: "project-1".to_string(),
        role_type,
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

/// A minimal contract-valid ExecutorBinding for cross-entity non-mutation
/// tests.
fn minimal_binding(binding_id: &str, role_id: &str) -> ExecutorBinding {
    ExecutorBinding {
        binding_id: binding_id.to_string(),
        role_id: role_id.to_string(),
        provider_id: "provider-alpha".to_string(),
        model_id: "model-one".to_string(),
        runtime_id: "runtime-a1-host".to_string(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: "2026-08-17T10:00:00.000Z".to_string(),
        lease_expires_at: "2026-08-17T11:00:00.000Z".to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    }
}

/// Extracts the quoted values of the `CHECK (<column> IN (...))` list for
/// `column` from a migration's SQL, so the storage backstop lists can be
/// compared against the typed closed enums for drift.
fn check_values(sql: &str, column: &str) -> Vec<String> {
    let marker = format!("{column} IN (");
    let start = sql
        .find(&marker)
        .unwrap_or_else(|| panic!("CHECK marker {marker:?} missing from migration SQL"));
    let rest = &sql[start + marker.len()..];
    let end = rest
        .find(')')
        .expect("closing parenthesis of the CHECK list");
    rest[..end]
        .split(',')
        .map(|token| token.trim().trim_matches('\'').to_string())
        .collect()
}

// T01 — a fresh version-0 database bootstraps 0 → 1 → 2 → 3 → 4 → 5 → 6,
// and migration 4 creates exactly the event-log schema objects.
#[test]
fn t01_fresh_database_bootstraps_to_schema_version_6() {
    let tmp = TempDir::new("ev-t01");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("fresh database bootstraps");
    assert_eq!(repo.schema_version().expect("version read"), 6);
    assert!(
        repo.table_exists("event").expect("table check"),
        "event must exist after migration 4"
    );
    // Exactly one metadata row per applied migration.
    assert_eq!(
        repo.count_table_rows("state_schema_version").expect("rows"),
        6
    );
}

// T02 — a version-6 database reopens successfully and idempotently.
#[test]
fn t02_version_6_database_reopens() {
    let tmp = TempDir::new("ev-t02");
    for _ in 0..3 {
        let repo = SqliteStateRepository::open(tmp.db_path()).expect("every reopen succeeds");
        assert_eq!(repo.schema_version().expect("version read"), 6);
        assert_eq!(
            repo.count_table_rows("state_schema_version").expect("rows"),
            6,
            "one metadata row per applied migration, never duplicated by reopen"
        );
    }
}

// T03 — ordinary open of an existing version-3 database fails closed
// instead of silently migrating it to version 5.
#[test]
fn t03_ordinary_open_of_version_3_database_fails() {
    let tmp = TempDir::new("ev-t03");
    let version_3_chain = &migrations::registered()[..3];
    drop(
        SqliteStateRepository::open_with_migrations(tmp.db_path(), version_3_chain)
            .expect("bootstrap at version 3"),
    );
    let error = SqliteStateRepository::open(tmp.db_path())
        .expect_err("ordinary open must not silently migrate a version-3 database");
    assert!(
        matches!(
            error,
            StateError::SchemaVersionMismatch {
                found: 3,
                supported: 6
            }
        ),
        "unexpected error: {error}"
    );
    // The failed open left the database untouched at version 3.
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_3_chain)
        .expect("database still opens at version 3");
    assert_eq!(repo.schema_version().expect("version read"), 3);
}

// T04 — the smallest valid strict EventEnvelope appends successfully.
#[test]
fn t04_append_minimal_valid_event() {
    let tmp = TempDir::new("ev-t04");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.append_event(minimal_event(BASE_ULID))
        .expect("append minimal strict event");
    assert_eq!(repo.count_table_rows("event").expect("rows"), 1);
}

// T05 — an appended event reads back by event_id.
#[test]
fn t05_read_appended_event_by_id() {
    let tmp = TempDir::new("ev-t05");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let event = minimal_event(BASE_ULID);
    repo.append_event(event.clone()).expect("append");
    let found = repo
        .find_event(BASE_ULID)
        .expect("find")
        .expect("appended event is readable by event_id");
    assert_eq!(found.event_id, BASE_ULID);
    assert_eq!(found.event_type, EventType::TaskCreated);
}

// T06 — every structural field round-trips exactly.
#[test]
fn t06_full_structural_round_trip() {
    let tmp = TempDir::new("ev-t06");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let event = full_event(&ulid_for(6));
    repo.append_event(event.clone()).expect("append");
    assert_eq!(
        repo.find_event(&ulid_for(6)).expect("find"),
        Some(event),
        "the full structural envelope must round-trip byte-for-byte"
    );
}

// T07 — an appended event survives close and reopen of the repository.
#[test]
fn t07_event_survives_close_and_reopen() {
    let tmp = TempDir::new("ev-t07");
    let event = minimal_event(BASE_ULID);
    {
        let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
        repo.append_event(event.clone()).expect("append");
    }
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(repo.find_event(BASE_ULID).expect("find"), Some(event));
}

// T08 — a missing event is a deterministic absence, before and after other
// events exist.
#[test]
fn t08_missing_event_is_deterministic_absence() {
    let tmp = TempDir::new("ev-t08");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(
        repo.find_event(&ulid_for(999)).expect("find"),
        None,
        "a never-appended event must be reported absent"
    );
    repo.append_event(minimal_event(BASE_ULID)).expect("append");
    assert_eq!(
        repo.find_event(&ulid_for(999)).expect("find"),
        None,
        "absence must remain deterministic once other events exist"
    );
}

// T09 — appending a duplicate event_id fails explicitly.
#[test]
fn t09_duplicate_event_id_fails_explicitly() {
    let tmp = TempDir::new("ev-t09");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.append_event(minimal_event(BASE_ULID))
        .expect("first append");
    let error = repo
        .append_event(minimal_event(BASE_ULID))
        .expect_err("duplicate event_id must fail explicitly");
    assert!(
        matches!(
            &error,
            StateError::EventAlreadyExists { event_id } if event_id == BASE_ULID
        ),
        "unexpected error: {error}"
    );
}

// T10 — a failed duplicate append leaves the original row byte-for-byte
// unchanged.
#[test]
fn t10_duplicate_leaves_original_row_unchanged() {
    let tmp = TempDir::new("ev-t10");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let original = full_event(BASE_ULID);
    repo.append_event(original.clone()).expect("first append");

    let mut impostor = minimal_event(BASE_ULID);
    impostor.event_type = EventType::HumanRequired;
    impostor.correlation_id = "corr-impostor".to_string();
    let error = repo
        .append_event(impostor)
        .expect_err("duplicate event_id must fail explicitly");
    assert!(
        matches!(error, StateError::EventAlreadyExists { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.find_event(BASE_ULID).expect("find"),
        Some(original),
        "the original event must remain exactly as first persisted"
    );
    assert_eq!(repo.count_table_rows("event").expect("rows"), 1);
}

// T11 — a failed append (validation or duplicate) leaves no partial row.
#[test]
fn t11_failed_append_leaves_no_partial_row() {
    let tmp = TempDir::new("ev-t11");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");

    // Validation failure is rejected before any storage access.
    let mut invalid = minimal_event(BASE_ULID);
    invalid.payload.digest = String::new();
    let error = repo
        .append_event(invalid)
        .expect_err("invalid envelope must be rejected");
    assert!(
        matches!(error, StateError::EventValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("event").expect("rows"),
        0,
        "no partial envelope may persist after a rejected append"
    );

    // Duplicate failure adds nothing.
    repo.append_event(minimal_event(BASE_ULID))
        .expect("valid append");
    let error = repo
        .append_event(minimal_event(BASE_ULID))
        .expect_err("duplicate event_id must fail explicitly");
    assert!(
        matches!(error, StateError::EventAlreadyExists { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.count_table_rows("event").expect("rows"), 1);
}

// T12 — all 53 frozen EventType values round-trip exactly, one per event.
#[test]
fn t12_all_53_event_types_round_trip() {
    assert_eq!(EventType::ALL.len(), 53);
    let tmp = TempDir::new("ev-t12");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    for (index, event_type) in EventType::ALL.into_iter().enumerate() {
        let mut event = minimal_event(&ulid_for(index as u64));
        event.event_type = event_type;
        repo.append_event(event.clone())
            .unwrap_or_else(|e| panic!("append of {:?} failed: {e}", event_type.as_str()));
        let found = repo
            .find_event(&ulid_for(index as u64))
            .expect("find")
            .expect("event exists");
        assert_eq!(found.event_type, event_type);
        assert_eq!(found.event_type.as_str(), event_type.as_str());
    }
    assert_eq!(repo.count_table_rows("event").expect("rows"), 53);
}

// T13 — an invalid persisted event_type fails closed: the decode boundary
// rejects unknown values, and the storage CHECK backstop refuses to store
// them at all.
#[test]
fn t13_invalid_event_type_fails_closed() {
    // Decode boundary: anything outside the frozen 53 fails closed.
    for invalid in [
        "GOAL_VANISHED",
        "TASK_ABORTED",
        "UNKNOWN",
        "OTHER",
        "CUSTOM",
        "task_created",
        "",
    ] {
        assert!(
            matches!(
                EventType::from_storage(invalid),
                Err(StateError::EventDecodeFailed { .. })
            ),
            "event_type {invalid:?} must not decode to a frozen event type"
        );
    }
    // Storage backstop: direct SQL inside the State layer cannot even
    // persist a non-frozen event type.
    let tmp = TempDir::new("ev-t13");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let error = repo
        .run_transaction(|uow| uow.execute(&probe_insert_sql("event_type", "GOAL_VANISHED"), &[]))
        .expect_err("storage must reject a non-frozen event type");
    assert!(
        matches!(error, StateError::InternalQueryFailed { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.count_table_rows("event").expect("rows"), 0);
}

// T14 — all five ActorKind values round-trip.
#[test]
fn t14_all_actor_kinds_round_trip() {
    assert_eq!(ActorKind::ALL.len(), 5);
    let tmp = TempDir::new("ev-t14");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    for (index, kind) in ActorKind::ALL.into_iter().enumerate() {
        let mut event = minimal_event(&ulid_for(index as u64));
        event.actor.kind = kind;
        repo.append_event(event.clone()).expect("append");
        let found = repo
            .find_event(&ulid_for(index as u64))
            .expect("find")
            .expect("event exists");
        assert_eq!(found.actor.kind, kind);
    }
}

// T15 — all six SubjectKind values round-trip.
#[test]
fn t15_all_subject_kinds_round_trip() {
    assert_eq!(SubjectKind::ALL.len(), 6);
    let tmp = TempDir::new("ev-t15");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    for (index, kind) in SubjectKind::ALL.into_iter().enumerate() {
        let mut event = minimal_event(&ulid_for(index as u64));
        event.subject.kind = kind;
        repo.append_event(event.clone()).expect("append");
        let found = repo
            .find_event(&ulid_for(index as u64))
            .expect("find")
            .expect("event exists");
        assert_eq!(found.subject.kind, kind);
    }
}

// T16 — an absent goal_id round-trips as absent.
#[test]
fn t16_goal_id_absent_round_trips() {
    let tmp = TempDir::new("ev-t16");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.goal_id = None;
    repo.append_event(event).expect("append");
    assert_eq!(
        repo.find_event(BASE_ULID).expect("find").unwrap().goal_id,
        None
    );
}

// T17 — a present goal_id round-trips exactly.
#[test]
fn t17_goal_id_present_round_trips() {
    let tmp = TempDir::new("ev-t17");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.goal_id = Some("goal-0042".to_string());
    repo.append_event(event).expect("append");
    assert_eq!(
        repo.find_event(BASE_ULID).expect("find").unwrap().goal_id,
        Some("goal-0042".to_string())
    );
}

// T18 — an absent actor.id round-trips as absent.
#[test]
fn t18_actor_id_absent_round_trips() {
    let tmp = TempDir::new("ev-t18");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.actor.id = None;
    repo.append_event(event).expect("append");
    assert_eq!(
        repo.find_event(BASE_ULID).expect("find").unwrap().actor.id,
        None
    );
}

// T19 — a present actor.id round-trips exactly.
#[test]
fn t19_actor_id_present_round_trips() {
    let tmp = TempDir::new("ev-t19");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.actor.id = Some("role-a1-001".to_string());
    repo.append_event(event).expect("append");
    assert_eq!(
        repo.find_event(BASE_ULID).expect("find").unwrap().actor.id,
        Some("role-a1-001".to_string())
    );
}

// T20 — a canonical ULID event_id is accepted.
#[test]
fn t20_canonical_ulid_accepted() {
    let tmp = TempDir::new("ev-t20");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    // The canonical ULID example spelling: 26 Crockford Base32 characters.
    let canonical = "01ARZ3NDEKTSV4RRFFQ69G5FAK";
    assert_eq!(canonical.len(), 26);
    repo.append_event(minimal_event(canonical))
        .expect("canonical ULID event_id is accepted");
    assert!(repo.find_event(canonical).expect("find").is_some());
}

// T21 — malformed/non-canonical ULIDs are rejected before persistence.
#[test]
fn t21_malformed_ulid_rejected_before_persistence() {
    let tmp = TempDir::new("ev-t21");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let invalid_ids = [
        "",                                     // empty
        "01ARZ3NDEKTSV4RRFFQ69G5FA",            // 25 chars: too short
        "01ARZ3NDEKTSV4RRFFQ69G5FAKZ",          // 27 chars: too long
        "01ARZ3NDEKTSV4RRFFQ69G5FAiZ",          // lowercase i: non-canonical
        "01ARZ3NDEKTSV4RRFFQ69G5FAKl",          // lowercase l: non-canonical
        "01ARZ3NDEKTSV4RRFFQ69G5FAKo",          // lowercase o: non-canonical
        "01ARZ3NDEKTSV4RRFFQ69G5FAKu",          // lowercase u: non-canonical
        "01ARZ3NDEKTSV4RRFFQ69G5FAKU",          // uppercase U: not in alphabet
        "01ARZ3NDEKTSV4RRFFQ69G5FA-I",          // separator/other symbol
        "01ARZ3NDEKTSV4RRFFQ69G5FAé",           // non-ASCII
        "not a ulid at all, just text padding", // free-form prose
    ];
    for invalid in invalid_ids {
        let error = repo
            .append_event(minimal_event(invalid))
            .expect_err(&format!("event_id {invalid:?} must be rejected"));
        assert!(
            matches!(error, StateError::EventValidation { .. }),
            "event_id {invalid:?}: unexpected error: {error}"
        );
    }
    assert_eq!(
        repo.count_table_rows("event").expect("rows"),
        0,
        "no malformed identity may reach storage"
    );
}

// T22 — the exact event_id persists byte-for-byte unchanged through
// validated input → persisted row → readback.
#[test]
fn t22_event_id_persists_byte_for_byte() {
    let tmp = TempDir::new("ev-t22");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let supplied = ulid_for(22);
    repo.append_event(minimal_event(&supplied)).expect("append");
    let found = repo.find_event(&supplied).expect("find").expect("present");
    assert_eq!(found.event_id, supplied);
    assert_eq!(found.event_id.as_bytes(), supplied.as_bytes());
    // The structural identity survives close/reopen byte-for-byte too.
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    let found = repo.find_event(&supplied).expect("find").expect("present");
    assert_eq!(found.event_id.as_bytes(), supplied.as_bytes());
}

// T23 — project_id structural validation is enforced (non-empty).
#[test]
fn t23_project_id_validation_enforced() {
    let tmp = TempDir::new("ev-t23");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.project_id = String::new();
    let error = repo
        .append_event(event)
        .expect_err("empty project_id must be rejected");
    assert!(
        matches!(error, StateError::EventValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.count_table_rows("event").expect("rows"), 0);
}

// T24 — subject.id structural validation is enforced (non-empty).
#[test]
fn t24_subject_id_validation_enforced() {
    let tmp = TempDir::new("ev-t24");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.subject.id = String::new();
    let error = repo
        .append_event(event)
        .expect_err("empty subject.id must be rejected");
    assert!(
        matches!(error, StateError::EventValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.count_table_rows("event").expect("rows"), 0);
}

// T25 — correlation_id structural validation is enforced (non-empty).
#[test]
fn t25_correlation_id_validation_enforced() {
    let tmp = TempDir::new("ev-t25");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.correlation_id = String::new();
    let error = repo
        .append_event(event)
        .expect_err("empty correlation_id must be rejected");
    assert!(
        matches!(error, StateError::EventValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.count_table_rows("event").expect("rows"), 0);
}

// T26 — epoch zero is accepted.
#[test]
fn t26_epoch_zero_accepted() {
    let tmp = TempDir::new("ev-t26");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.epoch = 0;
    repo.append_event(event).expect("append");
    assert_eq!(repo.find_event(BASE_ULID).expect("find").unwrap().epoch, 0);
}

// T27 — a positive epoch is accepted.
#[test]
fn t27_positive_epoch_accepted() {
    let tmp = TempDir::new("ev-t27");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.epoch = 1_048_576;
    repo.append_event(event).expect("append");
    assert_eq!(
        repo.find_event(BASE_ULID).expect("find").unwrap().epoch,
        1_048_576
    );
}

// T28 — a negative epoch is rejected before persistence, at both the typed
// boundary and the storage backstop.
#[test]
fn t28_negative_epoch_rejected() {
    let tmp = TempDir::new("ev-t28");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.epoch = -1;
    let error = repo
        .append_event(event)
        .expect_err("negative epoch must be rejected");
    assert!(
        matches!(error, StateError::EventValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.count_table_rows("event").expect("rows"), 0);
    // Storage backstop re-enforces the same constraint.
    let error = repo
        .run_transaction(|uow| {
            uow.execute(
                "INSERT INTO event (
                    event_id, project_id, event_type, actor_kind,
                    subject_kind, subject_id, occurred_at,
                    payload_reference, payload_digest, correlation_id, epoch
                ) VALUES (
                    '01ARZ3NDEKTSV4RRFFQ69G5FAK', 'project-1', 'TASK_CREATED', 'SYSTEM',
                    'TASK', 'task-001', '2026-08-17T10:00:00.000Z',
                    'blob://probe', 'sha256:probe', 'corr-probe', -1
                )",
                &[],
            )
        })
        .expect_err("storage must reject a negative epoch");
    assert!(
        matches!(error, StateError::InternalQueryFailed { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.count_table_rows("event").expect("rows"), 0);
}

// T29 — occurred_at is an opaque string: it round-trips exactly, with no
// time dependency, parsing, or comparison anywhere.
#[test]
fn t29_occurred_at_round_trips_opaquely() {
    let tmp = TempDir::new("ev-t29");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    // A deliberately non-timestamp, non-empty contract string: if State
    // parsed or compared occurred_at, this would fail.
    let opaque = "opaque-contract-instant-7f3e-not-a-clock-value";
    let mut event = minimal_event(BASE_ULID);
    event.occurred_at = opaque.to_string();
    repo.append_event(event).expect("append");
    assert_eq!(
        repo.find_event(BASE_ULID)
            .expect("find")
            .unwrap()
            .occurred_at,
        opaque
    );
    // An empty occurred_at is rejected structurally.
    let mut event = minimal_event(&ulid_for(29));
    event.occurred_at = String::new();
    assert!(matches!(
        repo.append_event(event),
        Err(StateError::EventValidation { .. })
    ));
}

// T30 — the payload reference and digest round-trip exactly as supplied.
#[test]
fn t30_payload_reference_and_digest_round_trip() {
    let tmp = TempDir::new("ev-t30");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let reference = "blob://attempts/000042/output.md";
    let digest = "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    let mut event = full_event(BASE_ULID);
    event.payload = EventPayloadReference {
        reference: reference.to_string(),
        digest: digest.to_string(),
    };
    repo.append_event(event).expect("append");
    let found = repo.find_event(BASE_ULID).expect("find").expect("present");
    assert_eq!(found.payload.reference, reference);
    assert_eq!(found.payload.digest, digest);
}

// T31 — an empty payload reference is rejected.
#[test]
fn t31_empty_payload_reference_rejected() {
    let tmp = TempDir::new("ev-t31");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.payload.reference = String::new();
    let error = repo
        .append_event(event)
        .expect_err("empty payload reference must be rejected");
    assert!(
        matches!(error, StateError::EventValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.count_table_rows("event").expect("rows"), 0);
}

// T32 — an empty payload digest is rejected.
#[test]
fn t32_empty_payload_digest_rejected() {
    let tmp = TempDir::new("ev-t32");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut event = minimal_event(BASE_ULID);
    event.payload.digest = String::new();
    let error = repo
        .append_event(event)
        .expect_err("empty payload digest must be rejected");
    assert!(
        matches!(error, StateError::EventValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.count_table_rows("event").expect("rows"), 0);
}

// T33 — compile-time evidence: the EventEnvelope payload is exactly the
// structural EventPayloadReference type; there is no raw payload String
// body field. The destructuring binding below compiles only if `payload`
// has that exact structural type.
#[test]
fn t33_payload_field_is_structural_reference() {
    let event = full_event(BASE_ULID);
    let EventPayloadReference { reference, digest } = &event.payload;
    assert!(!reference.is_empty());
    assert!(!digest.is_empty());
}

// T34 — compile-time evidence: append_event takes exactly
// (repository, EventEnvelope); there is no EventRedaction, secret-list,
// or redaction-set parameter. The function-pointer coercion below compiles
// only for that exact signature. (No `EventRedaction` type exists in this
// crate at all — see the module documentation.)
#[test]
fn t34_no_caller_secret_list_parameter() {
    let append: fn(&mut SqliteStateRepository, EventEnvelope) -> Result<(), StateError> =
        SqliteStateRepository::append_event;
    let event = minimal_event(BASE_ULID);
    let tmp = TempDir::new("ev-t34");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    append(&mut repo, event).expect("append through the exact public signature");
}

// T37 — compile-time evidence: raw free-form payload text cannot be
// supplied through the public append API. The only argument is the
// envelope, whose only payload representation is the structural
// EventPayloadReference verified in T33; no String body parameter exists
// on any append path.
#[test]
fn t37_no_raw_payload_text_path() {
    let append: fn(&mut SqliteStateRepository, EventEnvelope) -> Result<(), StateError> =
        SqliteStateRepository::append_event;
    let event = full_event(BASE_ULID);
    let tmp = TempDir::new("ev-t37");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    append(&mut repo, event).expect("append accepts only the structural envelope");
}

// T36 / REPAIR-F2 — a canonical event_id containing a substring that could
// previously have been declared a "secret" ("SECRET") is never processed by
// any redaction/substitution mechanism and persists/read-backs
// byte-for-byte unchanged, because structural identifiers are never
// redacted.
#[test]
fn t36_sensitive_substring_event_id_never_redacted() {
    let mut event_id = BASE_ULID.to_string();
    // Replace six middle characters with "SECRET"; all six characters are
    // in the Crockford Base32 alphabet, so the id stays canonical.
    event_id.replace_range(10..16, "SECRET");
    assert_eq!(event_id.len(), 26);
    let tmp = TempDir::new("ev-t36");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.append_event(minimal_event(&event_id)).expect("append");
    assert_eq!(
        repo.find_event(&event_id).expect("find").unwrap().event_id,
        event_id,
        "the sensitive-looking structural identity must persist byte-for-byte"
    );
    // No "[REDACTED]" substitution, masking, or truncation occurred.
    assert!(repo.find_event(&event_id).expect("find").is_some());
    assert!(!event_id.contains("[REDACTED]"));
}

// T41 — appending a later event leaves the earlier event unchanged.
#[test]
fn t41_later_append_leaves_earlier_event_unchanged() {
    let tmp = TempDir::new("ev-t41");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let first = full_event(&ulid_for(0));
    repo.append_event(first.clone()).expect("first append");
    let second = minimal_event(&ulid_for(1));
    repo.append_event(second.clone()).expect("second append");
    assert_eq!(
        repo.find_event(&ulid_for(0)).expect("find"),
        Some(first),
        "an earlier immutable event must never change"
    );
    assert_eq!(repo.find_event(&ulid_for(1)).expect("find"), Some(second));
}

// T42 — multiple events survive close/reopen.
#[test]
fn t42_multiple_events_survive_close_and_reopen() {
    let tmp = TempDir::new("ev-t42");
    let events: Vec<EventEnvelope> = (0..3)
        .map(|index| {
            let mut event = minimal_event(&ulid_for(index));
            event.correlation_id = format!("corr-lineage-{index}");
            event
        })
        .collect();
    {
        let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
        for event in &events {
            repo.append_event(event.clone()).expect("append");
        }
    }
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    for event in &events {
        assert_eq!(
            repo.find_event(&event.event_id).expect("find"),
            Some(event.clone()),
            "every appended event must survive close/reopen"
        );
    }
    assert_eq!(repo.count_table_rows("event").expect("rows"), 3);
}

// T43 — appending an event does not mutate any persisted LogicalRole.
#[test]
fn t43_event_append_does_not_mutate_logical_role() {
    let tmp = TempDir::new("ev-t43");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut role = minimal_role("role-ev-001", LogicalRoleType::RuntimeA1);
    role.current_context_epoch = 5;
    repo.create_logical_role(role.clone()).expect("role create");
    repo.append_event(minimal_event(BASE_ULID)).expect("append");
    assert_eq!(
        repo.find_logical_role("role-ev-001").expect("find"),
        Some(role),
        "appending an event must leave the LogicalRole untouched"
    );
}

// T44 — appending an event does not mutate any persisted ExecutorBinding.
#[test]
fn t44_event_append_does_not_mutate_executor_binding() {
    let tmp = TempDir::new("ev-t44");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-ev-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let binding = minimal_binding("binding-ev-001", "role-ev-001");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    repo.append_event(minimal_event(BASE_ULID)).expect("append");
    assert_eq!(
        repo.find_executor_binding("binding-ev-001").expect("find"),
        Some(binding),
        "appending an event must leave the ExecutorBinding untouched"
    );
}

// T45 — applying migration 4 to an existing version-3 database preserves
// existing LogicalRole records. The migration SQL is applied exactly as
// bootstrap would apply it: atomically, on the version-3 database.
#[test]
fn t45_migration4_preserves_logical_roles() {
    let tmp = TempDir::new("ev-t45");
    let version_3_chain = &migrations::registered()[..3];
    let role = minimal_role("role-mig-001", LogicalRoleType::RuntimeA1);
    {
        let mut repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_3_chain)
            .expect("bootstrap at version 3");
        repo.create_logical_role(role.clone()).expect("role create");
    }
    {
        let mut repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_3_chain)
            .expect("reopen at version 3");
        let migration = migrations::registered()[3];
        assert_eq!(migration.version, 4);
        repo.run_transaction(|uow| uow.execute_batch(migration.sql))
            .expect("apply migration 4");
    }
    // The ordinary registered chain now ends at version 6 and refuses to
    // open a version-4 database, so the migrated database is verified
    // through the version-4 prefix of the same chain.
    let version_4_chain = &migrations::registered()[..4];
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_4_chain)
        .expect("open at version 4");
    assert_eq!(repo.schema_version().expect("version read"), 4);
    assert_eq!(
        repo.find_logical_role("role-mig-001").expect("find"),
        Some(role),
        "migration 4 must preserve existing LogicalRole records"
    );
}

// T46 — applying migration 4 to an existing version-3 database preserves
// existing ExecutorBinding records (and the LogicalRole they reference).
#[test]
fn t46_migration4_preserves_executor_bindings() {
    let tmp = TempDir::new("ev-t46");
    let version_3_chain = &migrations::registered()[..3];
    let role = minimal_role("role-mig-001", LogicalRoleType::RuntimeA1);
    let binding = minimal_binding("binding-mig-001", "role-mig-001");
    {
        let mut repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_3_chain)
            .expect("bootstrap at version 3");
        repo.create_logical_role(role).expect("role create");
        repo.create_executor_binding(binding.clone())
            .expect("binding create");
    }
    {
        let mut repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_3_chain)
            .expect("reopen at version 3");
        repo.run_transaction(|uow| uow.execute_batch(migrations::registered()[3].sql))
            .expect("apply migration 4");
    }
    // Verified through the version-4 prefix of the registered chain; see
    // the T45 note about ordinary open refusing a version-4 database.
    let version_4_chain = &migrations::registered()[..4];
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_4_chain)
        .expect("open at version 4");
    assert_eq!(
        repo.find_executor_binding("binding-mig-001").expect("find"),
        Some(binding),
        "migration 4 must preserve existing ExecutorBinding records"
    );
}

// T47 — migration 4 creates only event-log-required schema objects: the
// exact table set after a full bootstrap is the four prior tables plus
// `event`, nothing else; and applying migration 4 to a version-3 database
// adds exactly one table.
#[test]
fn t47_migration4_creates_only_event_schema() {
    // Full bootstrap: exactly the eight expected tables exist (the four
    // prior tables, `event`, and the migration-6 context manifest tables).
    let tmp = TempDir::new("ev-t47a");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut expected = vec![
        "event",
        "executor_binding",
        "logical_role",
        "logical_role_ownership_path",
        "state_schema_version",
        "context_manifest",
        "context_manifest_source",
        "context_manifest_source_required_for",
    ];
    expected.sort_unstable();
    assert_eq!(
        repo.list_tables().expect("tables"),
        expected,
        "the registered chain must create exactly its own eight tables"
    );

    // Applying migration 4 to a version-3 database adds exactly `event`.
    let tmp = TempDir::new("ev-t47b");
    let version_3_chain = &migrations::registered()[..3];
    let before = {
        let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_3_chain)
            .expect("bootstrap at version 3");
        repo.list_tables().expect("tables")
    };
    {
        let mut repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_3_chain)
            .expect("reopen at version 3");
        repo.run_transaction(|uow| uow.execute_batch(migrations::registered()[3].sql))
            .expect("apply migration 4");
    }
    // Verified through the version-4 prefix of the registered chain; the
    // ordinary chain now ends at version 6 and refuses a version-4 database.
    let version_4_chain = &migrations::registered()[..4];
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_4_chain)
        .expect("open at version 4");
    let after = repo.list_tables().expect("tables");
    let mut added = after.clone();
    added.retain(|table| !before.contains(table));
    assert_eq!(added, vec!["event".to_string()]);
    // No unrelated future storage appeared.
    for forbidden in [
        "context_manifest",
        "context_epoch",
        "entitlement",
        "graph",
        "task",
        "task_attempt",
        "review",
        "finding",
        "evidence",
        "routing_decision",
        "provider",
        "model",
        "host_session",
        "workspace",
        "recovery",
        "lease_scheduler_state",
        "event_payload",
        "event_redaction",
    ] {
        assert!(
            !repo.table_exists(forbidden).expect("table check"),
            "no {forbidden} storage may be created by migration 4"
        );
    }
}

// Extra — the migration's storage CHECK lists match the typed closed enums
// exactly, so the durable backstop cannot drift from the typed boundary.
#[test]
fn extra_schema_check_lists_match_enums_exactly() {
    let sql = migrations::registered()[3].sql;
    let event_types: Vec<&str> = EventType::ALL.iter().map(|t| t.as_str()).collect();
    let actor_kinds: Vec<&str> = ActorKind::ALL.iter().map(|k| k.as_str()).collect();
    let subject_kinds: Vec<&str> = SubjectKind::ALL.iter().map(|k| k.as_str()).collect();
    assert_eq!(
        check_values(sql, "event_type"),
        event_types,
        "the event_type CHECK list must be exactly the 53 frozen event types"
    );
    assert_eq!(
        check_values(sql, "actor_kind"),
        actor_kinds,
        "the actor_kind CHECK list must be exactly the five frozen actor kinds"
    );
    assert_eq!(
        check_values(sql, "subject_kind"),
        subject_kinds,
        "the subject_kind CHECK list must be exactly the six frozen subject kinds"
    );
}

// T48 — the event table contains exactly the authorized structural columns
// and no raw free-form payload body column of any name.
#[test]
fn t48_event_table_has_no_raw_payload_column() {
    let tmp = TempDir::new("ev-t48");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let columns = repo.table_columns("event").expect("columns");
    assert_eq!(columns, EXPECTED_EVENT_COLUMNS.to_vec());
    for forbidden in [
        "payload",
        "payload_text",
        "raw_payload",
        "event_body",
        "raw_json",
        "body",
    ] {
        assert!(
            !columns.iter().any(|column| column == forbidden),
            "no {forbidden} column may exist on the event table"
        );
    }
}

// T49 — a storage/transaction failure during append rolls back completely:
// no partial envelope row persists.
#[test]
fn t49_transaction_failure_produces_no_row() {
    let tmp = TempDir::new("ev-t49");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    // A trigger forces the INSERT itself to fail at the storage layer,
    // exercising the transaction-failure path of the public append API.
    repo.run_transaction(|uow| {
        uow.execute_batch(
            "CREATE TRIGGER force_event_append_failure BEFORE INSERT ON event
             BEGIN SELECT RAISE(ABORT, 'forced append failure'); END;",
        )
    })
    .expect("trigger create");
    let error = repo
        .append_event(minimal_event(BASE_ULID))
        .expect_err("storage-forced failure must surface as an append error");
    assert!(
        matches!(error, StateError::EventWriteFailed { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("event").expect("rows"),
        0,
        "a failed append must leave no partial row"
    );
}

// T50 — success is returned only after the transaction commits: an Ok from
// append is durable for a fresh connection (close → reopen finds the row),
// and the failure path returns Err with nothing persisted (T49).
#[test]
fn t50_success_only_after_commit() {
    let tmp = TempDir::new("ev-t50");
    {
        let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
        repo.append_event(minimal_event(BASE_ULID))
            .expect("append succeeds only after commit");
    }
    // A brand-new connection observing the row proves the commit happened
    // before Ok was returned.
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert!(repo.find_event(BASE_ULID).expect("find").is_some());
}

// T51 — a corrupt persisted structural envelope fails closed during decode:
// unknown enum representations are refused (never mapped to UNKNOWN), and
// the storage CHECK backstop prevents such rows from being written at all.
#[test]
fn t51_corrupt_envelope_fails_closed_during_decode() {
    for invalid_type in [
        "GOAL_VANISHED",
        "UNKNOWN",
        "OTHER",
        "CUSTOM",
        "",
        "task_created",
    ] {
        assert!(
            matches!(
                EventType::from_storage(invalid_type),
                Err(StateError::EventDecodeFailed { .. })
            ),
            "corrupt event_type {invalid_type:?} must fail closed"
        );
    }
    for invalid_actor in ["AGENT", "WORKER", "MODEL", "", "system"] {
        assert!(
            matches!(
                ActorKind::from_storage(invalid_actor),
                Err(StateError::EventDecodeFailed { .. })
            ),
            "corrupt actor_kind {invalid_actor:?} must fail closed"
        );
    }
    for invalid_subject in ["SESSION", "EVENT", "PLUGIN", "", "task"] {
        assert!(
            matches!(
                SubjectKind::from_storage(invalid_subject),
                Err(StateError::EventDecodeFailed { .. })
            ),
            "corrupt subject_kind {invalid_subject:?} must fail closed"
        );
    }
    // Storage backstop: none of these corrupt shapes can even be persisted
    // through direct SQL inside the State layer.
    let tmp = TempDir::new("ev-t51");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    for (column, value) in [
        ("event_type", "GOAL_VANISHED"),
        ("actor_kind", "AGENT"),
        ("subject_kind", "SESSION"),
    ] {
        let error = repo
            .run_transaction(|uow| uow.execute(&probe_insert_sql(column, value), &[]))
            .expect_err(&format!("storage must reject corrupt {column} {value:?}"));
        assert!(
            matches!(error, StateError::InternalQueryFailed { .. }),
            "unexpected error: {error}"
        );
    }
    assert_eq!(repo.count_table_rows("event").expect("rows"), 0);
}

/// Builds a direct-SQL probe inserting an out-of-contract `value` into
/// `column`, with every other enum column holding a valid value, so exactly
/// the targeted CHECK is violated.
fn probe_insert_sql(column: &str, value: &str) -> String {
    let event_type = if column == "event_type" {
        value
    } else {
        "TASK_CREATED"
    };
    let actor_kind = if column == "actor_kind" {
        value
    } else {
        "SYSTEM"
    };
    let subject_kind = if column == "subject_kind" {
        value
    } else {
        "TASK"
    };
    format!(
        "INSERT INTO event (
            event_id, project_id, event_type, actor_kind,
            subject_kind, subject_id, occurred_at,
            payload_reference, payload_digest, correlation_id, epoch
        ) VALUES (
            '01ARZ3NDEKTSV4RRFFQ69G5FAK', 'project-1', '{event_type}', '{actor_kind}',
            '{subject_kind}', 'task-001', '2026-08-17T10:00:00.000Z',
            'blob://probe', 'sha256:probe', 'corr-probe', 0
        )"
    )
}

// T35 — no redaction/replacement path mutates event_id.
//
// Compile-time/API-absence invariant: no redaction, substitution, masking,
// hashing, or rewriting function exists anywhere in this crate's event
// path; validation accepts or rejects and never transforms (see
// `validate_for_append` and `ensure_canonical_ulid` in `src/state/event.rs`).
// The runtime byte-for-byte behavior is covered by T22 and T36; no runtime
// test is fabricated here.

// T38 — no event UPDATE API.
// T39 — no event DELETE API.
// T40 — no event UPSERT/REPLACE API.
//
// Compile-time/API-absence invariants established by inspecting the public
// surface of this crate (src/state/**): the only event operations are
// `SqliteStateRepository::append_event` and `find_event`, plus the
// construction-only types re-exported from `src/state/event.rs`. There is
// no `update_event`, `delete_event`, `replace_event`, `upsert_event`,
// `rewrite_event`, `truncate_events`, or `clear_events` method on any
// public type, and `src/state/event.rs` contains exactly one INSERT
// statement and zero UPDATE/DELETE/REPLACE statements against `event`.
// No runtime test is fabricated for an absent API.

// T52 — no arbitrary SQL API is introduced.
//
// Pre-existing accepted invariant, preserved: all SQL lives inside
// `src/state/**`; the public surface exposes no `rusqlite` type, no SQL
// string parameter, and no arbitrary-execution path. `run_transaction`
// hands callers an opaque `UnitOfWork` with no public method. The only
// additions in this task (`append_event`, `find_event`, the event types)
// pass only structural typed values; caller data reaches SQL exclusively
// through bound parameters.

// T53 — no regex/pattern secret detector is introduced.
//
// Source-inspection invariant: no regex, pattern, entropy, token-shape, or
// provider-key detection exists in this crate (no such dependency exists in
// Cargo.toml, and no such code exists under src/state/**). The W1 control
// is the strict data-shape boundary, not secret classification.

// T54 — no credential/keychain/provider integration is introduced.
//
// Source-inspection invariant: this crate depends only on `rusqlite`
// (bundled SQLite) and `tokio` (no features enabled); no keychain,
// credential-broker, OAuth, provider-API, or runtime-authentication code
// exists under src/state/**.

// T55 — no routing/failover/context/entitlement/orchestration semantics
// are introduced.
//
// Source-inspection invariant: migration 4 creates only the `event` table
// (T47); `src/state/event.rs` contains no routing, failover, context,
// rehydration, recovery, entitlement, dispatch, emission-policy, or
// acceptance logic. State persists already-constructed structural event
// evidence; it does not decide when events should exist.
