//! Deterministic tests for immutable ExecutorBinding create/read persistence
//! (T01–T35 of migration 0003).
//!
//! All tests use real temporary SQLite database files under the system
//! temporary directory (never inside the repository). Storage-backstop
//! checks that need SQL beyond the public repository API run through
//! crate-private test helpers only and are `#[cfg(test)]`-gated.
//!
//! T35 (no public update/delete/release/renew/expiry API) is a
//! compile-time/API-absence invariant established by inspecting the public
//! surface of this crate, not a runtime test; it is documented in the task
//! handoff. Two later authorized slices each added exactly one bounded
//! mutation capability — A3-005 `release_executor_binding` and A3-008
//! `renew_executor_binding_lease` (see the T35 note at the bottom of this
//! module and `executor_binding_lease_tests`).

use crate::error::StateError;
use crate::executor_binding::{ExecutorBinding, ReleaseReason};
use crate::executor_binding_single_active_tests::direct_insert_binding;
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

/// A minimal contract-valid LogicalRole for binding targets.
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

/// A minimal contract-valid binding: only required fields set, every
/// optional field absent, and contract date-time strings stored as
/// provided.
fn minimal_binding(binding_id: &str, role_id: &str) -> ExecutorBinding {
    ExecutorBinding {
        binding_id: binding_id.to_string(),
        role_id: role_id.to_string(),
        provider_id: "provider-alpha".to_string(),
        model_id: "model-one".to_string(),
        runtime_id: "runtime-a1-host".to_string(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: "2026-08-16T10:00:00.000Z".to_string(),
        lease_expires_at: "2026-08-16T11:00:00.000Z".to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    }
}

/// The nine frozen release reasons in contract order, with their exact
/// durable representations.
const ALL_NINE_REASONS: [ReleaseReason; 9] = [
    ReleaseReason::RateLimited,
    ReleaseReason::SessionExhausted,
    ReleaseReason::AuthRequired,
    ReleaseReason::ProviderDown,
    ReleaseReason::Crash,
    ReleaseReason::HostSwitch,
    ReleaseReason::UserRequest,
    ReleaseReason::Completed,
    ReleaseReason::LeaseExpired,
];

const ALL_NINE_STRINGS: [&str; 9] = [
    "RATE_LIMITED",
    "SESSION_EXHAUSTED",
    "AUTH_REQUIRED",
    "PROVIDER_DOWN",
    "CRASH",
    "HOST_SWITCH",
    "USER_REQUEST",
    "COMPLETED",
    "LEASE_EXPIRED",
];

// T01 — a fresh version-0 database bootstraps through the full chain
// 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7, and migration 3 creates only ExecutorBinding storage.
#[test]
fn t01_fresh_database_reaches_schema_version_7() {
    let tmp = TempDir::new("eb-t01");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("fresh database bootstraps");
    assert_eq!(repo.schema_version().expect("version read"), 10);
    assert!(
        repo.table_exists("executor_binding").expect("table check"),
        "executor_binding must exist after migration 3"
    );
    // Migration 3 creates no other domain storage (AC-05); the
    // context_manifest tables belong to the later migration 6 and the
    // context_epoch table to the later migration 7.
    for forbidden in [
        "event_log",
        "entitlement",
        "graph",
        "graph_node",
        "graph_edge",
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
    ] {
        assert!(
            !repo.table_exists(forbidden).expect("table check"),
            "no {forbidden} storage may be created by migration 3"
        );
    }
}

// T02 — a version-7 database reopens successfully and idempotently.
#[test]
fn t02_version_7_reopen_idempotent() {
    let tmp = TempDir::new("eb-t02");
    for _ in 0..3 {
        let repo = SqliteStateRepository::open(tmp.db_path()).expect("every reopen succeeds");
        assert_eq!(repo.schema_version().expect("version read"), 10);
        assert_eq!(
            repo.count_table_rows("state_schema_version").expect("rows"),
            10,
            "one metadata row per applied migration, never duplicated by reopen"
        );
    }
}

// T03 — ordinary open of an existing version-2 database fails closed
// instead of silently upgrading it.
#[test]
fn t03_ordinary_open_of_version_2_database_fails() {
    let tmp = TempDir::new("eb-t03");
    let version_2_chain = &migrations::registered()[..2];
    drop(
        SqliteStateRepository::open_with_migrations(tmp.db_path(), version_2_chain)
            .expect("bootstrap at version 2"),
    );
    let error = SqliteStateRepository::open(tmp.db_path())
        .expect_err("ordinary open must not silently upgrade a version-2 database");
    assert!(
        matches!(
            error,
            StateError::SchemaVersionMismatch {
                found: 2,
                supported: 10
            }
        ),
        "unexpected error: {error}"
    );
    // The failed open left the database untouched at version 2.
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_2_chain)
        .expect("database still opens at version 2");
    assert_eq!(repo.schema_version().expect("version read"), 2);
}

// T04 — a minimal valid binding for an existing RUNTIME_A1 role persists
// and reads back exactly.
#[test]
fn t04_create_minimal_binding_for_runtime_a1_role() {
    let tmp = TempDir::new("eb-t04");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-a1-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let binding = minimal_binding("binding-001", "role-a1-001");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    assert_eq!(
        repo.find_executor_binding("binding-001").expect("find"),
        Some(binding)
    );
}

// T05 — a minimal valid binding for an existing RUNTIME_A2 role persists
// and reads back exactly.
#[test]
fn t05_create_minimal_binding_for_runtime_a2_role() {
    let tmp = TempDir::new("eb-t05");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-a2-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let binding = minimal_binding("binding-002", "role-a2-001");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    assert_eq!(
        repo.find_executor_binding("binding-002").expect("find"),
        Some(binding)
    );
}

// T06 — every contract field round-trips exactly.
#[test]
fn t06_full_field_round_trip() {
    let tmp = TempDir::new("eb-t06");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-full-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let binding = ExecutorBinding {
        binding_id: "binding-full-001".to_string(),
        role_id: "role-full-001".to_string(),
        provider_id: "provider-omega".to_string(),
        model_id: "model-nine".to_string(),
        runtime_id: "runtime-a2-host".to_string(),
        session_ref: Some("sessions/0192-abc/0042".to_string()),
        routing_decision_id: Some("routing-decision-0177".to_string()),
        bound_at: "2026-08-16T09:30:00.000Z".to_string(),
        lease_expires_at: "2026-08-16T10:30:00.000Z".to_string(),
        released_at: Some("2026-08-16T09:58:11.250Z".to_string()),
        release_reason: Some(ReleaseReason::HostSwitch),
        rehydration_completed: Some(false),
    };
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    assert_eq!(
        repo.find_executor_binding("binding-full-001")
            .expect("find"),
        Some(binding)
    );
}

// T07 — a persisted binding survives database close and reopen.
#[test]
fn t07_binding_survives_close_and_reopen() {
    let tmp = TempDir::new("eb-t07");
    let binding = minimal_binding("binding-durable-001", "role-durable-001");
    {
        let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
        repo.create_logical_role(minimal_role("role-durable-001", LogicalRoleType::RuntimeA1))
            .expect("role create");
        repo.create_executor_binding(binding.clone())
            .expect("binding create");
    }
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_executor_binding("binding-durable-001")
            .expect("find"),
        Some(binding)
    );
}

// T08 — looking up a binding that was never created is a deterministic
// absence.
#[test]
fn t08_missing_binding_lookup_is_deterministic_absence() {
    let tmp = TempDir::new("eb-t08");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(
        repo.find_executor_binding("never-created").expect("find"),
        None,
        "a never-created binding must be reported absent"
    );
    repo.create_logical_role(minimal_role("role-present-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-present-001", "role-present-001"))
        .expect("binding create");
    assert_eq!(
        repo.find_executor_binding("never-created").expect("find"),
        None,
        "absence must remain deterministic once other bindings exist"
    );
    assert_eq!(
        repo.find_executor_binding("").expect("find"),
        None,
        "an empty identifier can never match a stored binding"
    );
}

// T09 — duplicate binding_id creation fails explicitly.
#[test]
fn t09_duplicate_binding_id_fails() {
    let tmp = TempDir::new("eb-t09");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-dup-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-dup-001", "role-dup-001"))
        .expect("binding create");
    let error = repo
        .create_executor_binding(minimal_binding("binding-dup-001", "role-dup-001"))
        .expect_err("duplicate binding_id must fail explicitly");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingAlreadyExists { binding_id }
                if binding_id == "binding-dup-001"
        ),
        "unexpected error: {error}"
    );
}

// T10 — duplicate-binding failure leaves the original binding unchanged.
#[test]
fn t10_duplicate_failure_leaves_original_unchanged() {
    let tmp = TempDir::new("eb-t10");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-orig-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let original = minimal_binding("binding-orig-001", "role-orig-001");
    repo.create_executor_binding(original.clone())
        .expect("binding create");

    let mut impostor = minimal_binding("binding-orig-001", "role-orig-001");
    impostor.provider_id = "provider-impostor".to_string();
    impostor.session_ref = Some("sessions/impostor".to_string());
    let error = repo
        .create_executor_binding(impostor)
        .expect_err("duplicate binding_id must fail explicitly");
    assert!(
        matches!(error, StateError::ExecutorBindingAlreadyExists { .. }),
        "unexpected error: {error}"
    );
    // The original is intact: not overwritten, replaced, merged, or
    // delete-reinserted.
    assert_eq!(
        repo.find_executor_binding("binding-orig-001")
            .expect("find"),
        Some(original)
    );
}

// T11 — binding creation for a nonexistent LogicalRole fails explicitly.
#[test]
fn t11_nonexistent_role_fails() {
    let tmp = TempDir::new("eb-t11");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let error = repo
        .create_executor_binding(minimal_binding("binding-orphan-001", "role-never-created"))
        .expect_err("a binding for a nonexistent role must fail explicitly");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingRoleNotFound { role_id } if role_id == "role-never-created"
        ),
        "unexpected error: {error}"
    );
}

// T12 — the nonexistent-role failure leaves no ExecutorBinding row.
#[test]
fn t12_nonexistent_role_failure_leaves_no_row() {
    let tmp = TempDir::new("eb-t12");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-real-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-real-001", "role-real-001"))
        .expect("binding create");
    repo.create_executor_binding(minimal_binding("binding-orphan-001", "role-ghost-001"))
        .expect_err("a binding for a nonexistent role must fail explicitly");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "no orphan binding row may survive a failed create"
    );
    assert_eq!(
        repo.find_executor_binding("binding-orphan-001")
            .expect("find"),
        None
    );
    // Atomicity is durable: still exactly one row after close/reopen.
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "no orphan binding row may appear after reopen"
    );
}

// T13 — an unfamiliar non-empty provider_id is accepted without any
// registry or enum restriction.
#[test]
fn t13_unfamiliar_provider_id_succeeds() {
    let tmp = TempDir::new("eb-t13");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-probe-013", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut binding = minimal_binding("binding-probe-013", "role-probe-013");
    binding.provider_id = "provider-brand-new-never-seen".to_string();
    repo.create_executor_binding(binding.clone())
        .expect("unknown providers must be persistable as opaque identifiers");
    assert_eq!(
        repo.find_executor_binding("binding-probe-013")
            .expect("find"),
        Some(binding)
    );
}

// T14 — an unfamiliar non-empty model_id is accepted.
#[test]
fn t14_unfamiliar_model_id_succeeds() {
    let tmp = TempDir::new("eb-t14");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-probe-014", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut binding = minimal_binding("binding-probe-014", "role-probe-014");
    binding.model_id = "model-unknown-2099-x".to_string();
    repo.create_executor_binding(binding.clone())
        .expect("unknown models must be persistable as opaque identifiers");
    assert_eq!(
        repo.find_executor_binding("binding-probe-014")
            .expect("find"),
        Some(binding)
    );
}

// T15 — an unfamiliar non-empty runtime_id is accepted.
#[test]
fn t15_unfamiliar_runtime_id_succeeds() {
    let tmp = TempDir::new("eb-t15");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-probe-015", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut binding = minimal_binding("binding-probe-015", "role-probe-015");
    binding.runtime_id = "runtime-quantum-relay-7".to_string();
    repo.create_executor_binding(binding.clone())
        .expect("unknown runtimes must be persistable as opaque identifiers");
    assert_eq!(
        repo.find_executor_binding("binding-probe-015")
            .expect("find"),
        Some(binding)
    );
}

// T16 — an empty provider_id is rejected.
#[test]
fn t16_empty_provider_id_rejected() {
    let tmp = TempDir::new("eb-t16");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-probe-016", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut binding = minimal_binding("binding-probe-016", "role-probe-016");
    binding.provider_id = String::new();
    let error = repo
        .create_executor_binding(binding)
        .expect_err("an empty provider_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.find_executor_binding("binding-probe-016")
            .expect("find"),
        None,
        "a rejected create must persist nothing"
    );
}

// T17 — an empty model_id is rejected.
#[test]
fn t17_empty_model_id_rejected() {
    let tmp = TempDir::new("eb-t17");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-probe-017", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut binding = minimal_binding("binding-probe-017", "role-probe-017");
    binding.model_id = String::new();
    let error = repo
        .create_executor_binding(binding)
        .expect_err("an empty model_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.find_executor_binding("binding-probe-017")
            .expect("find"),
        None
    );
}

// T18 — an empty runtime_id is rejected.
#[test]
fn t18_empty_runtime_id_rejected() {
    let tmp = TempDir::new("eb-t18");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-probe-018", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut binding = minimal_binding("binding-probe-018", "role-probe-018");
    binding.runtime_id = String::new();
    let error = repo
        .create_executor_binding(binding)
        .expect_err("an empty runtime_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.find_executor_binding("binding-probe-018")
            .expect("find"),
        None
    );
}

// T19 — binding_id constraints are enforced: non-empty, at most 200
// characters, with exactly 200 accepted.
#[test]
fn t19_binding_id_constraints_enforced() {
    let tmp = TempDir::new("eb-t19");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-probe-019", LogicalRoleType::RuntimeA1))
        .expect("role create");

    let mut empty = minimal_binding("binding-probe-019", "role-probe-019");
    empty.binding_id = String::new();
    let error = repo
        .create_executor_binding(empty)
        .expect_err("an empty binding_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );

    let too_long = "b".repeat(201);
    let error = repo
        .create_executor_binding(minimal_binding(&too_long, "role-probe-019"))
        .expect_err("a 201-character binding_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        0,
        "rejected creates must persist nothing"
    );

    let exact = "b".repeat(200);
    let accepted = minimal_binding(&exact, "role-probe-019");
    repo.create_executor_binding(accepted.clone())
        .expect("a 200-character binding_id is valid");
    assert_eq!(
        repo.find_executor_binding(&exact).expect("find"),
        Some(accepted)
    );
}

// T20 — role_id constraints are enforced: non-empty, at most 200
// characters, with exactly 200 accepted.
#[test]
fn t20_role_id_constraints_enforced() {
    let tmp = TempDir::new("eb-t20");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");

    let mut empty_role = minimal_binding("binding-probe-020a", "");
    empty_role.role_id = String::new();
    let error = repo
        .create_executor_binding(empty_role)
        .expect_err("an empty role_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );

    let too_long = "r".repeat(201);
    let error = repo
        .create_executor_binding(minimal_binding("binding-probe-020b", &too_long))
        .expect_err("a 201-character role_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        0,
        "rejected creates must persist nothing"
    );

    let exact = "r".repeat(200);
    repo.create_logical_role(minimal_role(&exact, LogicalRoleType::RuntimeA1))
        .expect("a 200-character role_id is a valid LogicalRole");
    let accepted = minimal_binding("binding-probe-020c", &exact);
    repo.create_executor_binding(accepted.clone())
        .expect("a 200-character role_id is valid on a binding");
    assert_eq!(
        repo.find_executor_binding("binding-probe-020c")
            .expect("find"),
        Some(accepted)
    );
}

// T21 — routing_decision_id constraints are enforced when present:
// non-empty, at most 200 characters; absent is accepted.
#[test]
fn t21_routing_decision_id_constraints_enforced() {
    let tmp = TempDir::new("eb-t21");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-probe-021", LogicalRoleType::RuntimeA1))
        .expect("role create");

    let mut empty = minimal_binding("binding-probe-021a", "role-probe-021");
    empty.routing_decision_id = Some(String::new());
    let error = repo
        .create_executor_binding(empty)
        .expect_err("an empty routing_decision_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );

    let too_long = "d".repeat(201);
    let mut over = minimal_binding("binding-probe-021b", "role-probe-021");
    over.routing_decision_id = Some(too_long);
    let error = repo
        .create_executor_binding(over)
        .expect_err("a 201-character routing_decision_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        0,
        "rejected creates must persist nothing"
    );

    let exact = "d".repeat(200);
    let mut accepted = minimal_binding("binding-probe-021c", "role-probe-021");
    accepted.routing_decision_id = Some(exact);
    repo.create_executor_binding(accepted.clone())
        .expect("a 200-character routing_decision_id is valid");
    assert_eq!(
        repo.find_executor_binding("binding-probe-021c")
            .expect("find"),
        Some(accepted)
    );

    // The single-active-binding guard (A3-009) permits one not-fully-
    // released binding per role, so the prior binding is explicitly
    // released before the next create for the same role; the
    // routing_decision_id-absence assertion below is unaffected.
    repo.release_executor_binding(
        "binding-probe-021c",
        "2026-08-16T10:45:00.000Z",
        ReleaseReason::UserRequest,
    )
    .expect("release");

    let absent = minimal_binding("binding-probe-021d", "role-probe-021");
    repo.create_executor_binding(absent.clone())
        .expect("an absent routing_decision_id is valid");
    assert_eq!(
        repo.find_executor_binding("binding-probe-021d")
            .expect("find"),
        Some(absent)
    );
}

// T22 — session_ref = null round-trips as absent.
#[test]
fn t22_null_session_ref_round_trips() {
    let tmp = TempDir::new("eb-t22");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-sref-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let binding = minimal_binding("binding-sref-001", "role-sref-001");
    assert_eq!(binding.session_ref, None);
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    assert_eq!(
        repo.find_executor_binding("binding-sref-001")
            .expect("find")
            .expect("present")
            .session_ref,
        None
    );
}

// T23 — a non-null session_ref round-trips as an opaque string.
#[test]
fn t23_non_null_session_ref_round_trips() {
    let tmp = TempDir::new("eb-t23");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-sref-002", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let mut binding = minimal_binding("binding-sref-002", "role-sref-002");
    binding.session_ref = Some("session://host-3/conversations/98765".to_string());
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    assert_eq!(
        repo.find_executor_binding("binding-sref-002")
            .expect("find"),
        Some(binding)
    );
}

// T24 — each of the nine release_reason values round-trips exactly, with
// its exact durable representation.
#[test]
fn t24_all_nine_release_reasons_round_trip() {
    let tmp = TempDir::new("eb-t24");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-reasons-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    for (index, (reason, expected)) in ALL_NINE_REASONS
        .iter()
        .zip(ALL_NINE_STRINGS.iter())
        .enumerate()
    {
        assert_eq!(reason.as_str(), *expected);
        let binding_id = format!("binding-reason-{index:02}");
        let mut binding = minimal_binding(&binding_id, "role-reasons-001");
        binding.released_at = Some("2026-08-16T09:59:00.000Z".to_string());
        binding.release_reason = Some(*reason);
        if *reason == ReleaseReason::LeaseExpired {
            // LEASE_EXPIRED remains a fully representable, round-trippable
            // durable reason, but only the trusted expiry transaction may
            // write it: public creation refuses it, so the pre-existing row
            // is set up below the typed boundary.
            assert!(
                matches!(
                    repo.create_executor_binding(binding.clone()),
                    Err(StateError::ExecutorBindingValidation { .. })
                ),
                "public create must never manufacture new LEASE_EXPIRED state"
            );
            direct_insert_binding(&mut repo, &binding).expect("historical row reaches storage");
        } else {
            repo.create_executor_binding(binding.clone())
                .expect("each frozen release reason must be persistable");
        }
        let found = repo
            .find_executor_binding(&binding_id)
            .expect("find")
            .expect("present");
        assert_eq!(found.release_reason, Some(*reason));
        assert_eq!(found, binding);
    }
}

// T25 — unknown/invalid persisted release_reason values fail closed during
// decode, as does any other corrupt persisted binding data.
#[test]
fn t25_unknown_release_reason_fails_closed_decode() {
    // The decode boundary rejects everything outside the nine frozen
    // reasons: no UNKNOWN, OTHER, CUSTOM, fallback, or provider-specific
    // value exists.
    for invalid in [
        "UNKNOWN",
        "OTHER",
        "CUSTOM",
        "RATE_LIMIT",
        "rate_limited",
        "SESSION_TIMEOUT_PROVIDER_SPECIFIC",
        "",
    ] {
        assert!(
            matches!(
                ReleaseReason::from_storage(invalid),
                Err(StateError::ExecutorBindingDecodeFailed { .. })
            ),
            "release_reason {invalid:?} must not decode"
        );
    }

    let tmp = TempDir::new("eb-t25");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-decode-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    // The schema-level CHECK is the durable backstop: even direct SQL
    // inside the State layer cannot persist a non-contract release reason.
    let error = repo
        .run_transaction(|uow| {
            uow.execute(
                "INSERT INTO executor_binding (
                    binding_id, role_id, provider_id, model_id, runtime_id,
                    bound_at, lease_expires_at, release_reason
                ) VALUES (
                    'binding-bogus-reason', 'role-decode-001', 'p', 'm', 'r',
                    '2026-08-16T10:00:00.000Z', '2026-08-16T11:00:00.000Z', 'SOMETHING_ELSE'
                )",
                &[],
            )
        })
        .expect_err("storage must reject a non-contract release_reason");
    assert!(
        matches!(error, StateError::InternalQueryFailed { .. }),
        "unexpected error: {error}"
    );

    // A corrupt rehydration_completed value is not CHECK-constrained, so it
    // can reach storage through non-typed writers; decode must fail closed
    // rather than surface a plausible default binding.
    repo.run_transaction(|uow| {
        let values: &[&dyn rusqlite::ToSql] = &[
            &"binding-corrupt-bool",
            &"role-decode-001",
            &"p",
            &"m",
            &"r",
            &"2026-08-16T10:00:00.000Z",
            &"2026-08-16T11:00:00.000Z",
            &2_i64,
        ];
        uow.execute(
            "INSERT INTO executor_binding (
                binding_id, role_id, provider_id, model_id, runtime_id,
                bound_at, lease_expires_at, rehydration_completed
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            values,
        )
    })
    .expect("corrupt-bool insert reaches storage through direct SQL");
    let error = repo
        .find_executor_binding("binding-corrupt-bool")
        .expect_err("a corrupt persisted binding must fail closed on read");
    assert!(
        matches!(error, StateError::ExecutorBindingDecodeFailed { .. }),
        "unexpected error: {error}"
    );
    // The healthy binding on the same database still reads fine. The
    // single-active-binding guard (A3-009) makes the corrupt row block its
    // own role, so the healthy binding targets a second role.
    repo.create_logical_role(minimal_role("role-decode-002", LogicalRoleType::RuntimeA2))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-healthy-001", "role-decode-002"))
        .expect("binding create");
    assert!(
        repo.find_executor_binding("binding-healthy-001")
            .expect("find")
            .is_some()
    );
}

// T26 — released_at = null round-trips as absent.
#[test]
fn t26_null_released_at_round_trips() {
    let tmp = TempDir::new("eb-t26");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rel-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let binding = minimal_binding("binding-rel-001", "role-rel-001");
    assert_eq!(binding.released_at, None);
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    assert_eq!(
        repo.find_executor_binding("binding-rel-001")
            .expect("find")
            .expect("present")
            .released_at,
        None
    );
}

// T27 — a non-null released_at round-trips without any timestamp
// interpretation or cross-field rule: even a value lexically earlier than
// bound_at is stored and returned exactly as provided.
#[test]
fn t27_non_null_released_at_round_trips_without_interpretation() {
    let tmp = TempDir::new("eb-t27");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rel-002", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut binding = minimal_binding("binding-rel-002", "role-rel-002");
    binding.bound_at = "2026-08-16T12:00:00.000Z".to_string();
    binding.released_at = Some("1999-01-01T00:00:00.000Z".to_string());
    repo.create_executor_binding(binding.clone())
        .expect("State must not compare released_at against bound_at");
    assert_eq!(
        repo.find_executor_binding("binding-rel-002").expect("find"),
        Some(binding)
    );
}

// T28 — rehydration_completed = absent/null round-trips without an
// invented default: absence stays distinguishable from false.
#[test]
fn t28_rehydration_completed_absent_round_trips() {
    let tmp = TempDir::new("eb-t28");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rh-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let binding = minimal_binding("binding-rh-001", "role-rh-001");
    assert_eq!(binding.rehydration_completed, None);
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    let found = repo
        .find_executor_binding("binding-rh-001")
        .expect("find")
        .expect("present");
    assert_eq!(found.rehydration_completed, None);
    assert_ne!(found.rehydration_completed, Some(false));
}

// T29 — rehydration_completed = true round-trips (persistence data only;
// State never initiates or gates rehydration).
#[test]
fn t29_rehydration_completed_true_round_trips() {
    let tmp = TempDir::new("eb-t29");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rh-002", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut binding = minimal_binding("binding-rh-002", "role-rh-002");
    binding.rehydration_completed = Some(true);
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    assert_eq!(
        repo.find_executor_binding("binding-rh-002").expect("find"),
        Some(binding)
    );
}

// T30 — rehydration_completed = false round-trips and stays distinct from
// absence.
#[test]
fn t30_rehydration_completed_false_round_trips() {
    let tmp = TempDir::new("eb-t30");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rh-003", LogicalRoleType::RuntimeA2))
        .expect("role create");
    // The single-active-binding guard (A3-009) permits one not-fully-
    // released binding per role, so the absent-value probe targets a second
    // role; the false-versus-absence comparison is unaffected.
    repo.create_logical_role(minimal_role("role-rh-004", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut explicit_false = minimal_binding("binding-rh-003a", "role-rh-003");
    explicit_false.rehydration_completed = Some(false);
    repo.create_executor_binding(explicit_false.clone())
        .expect("binding create");
    let absent = minimal_binding("binding-rh-003b", "role-rh-004");
    repo.create_executor_binding(absent.clone())
        .expect("binding create");
    let found_false = repo
        .find_executor_binding("binding-rh-003a")
        .expect("find")
        .expect("present");
    let found_absent = repo
        .find_executor_binding("binding-rh-003b")
        .expect("find")
        .expect("present");
    assert_eq!(found_false.rehydration_completed, Some(false));
    assert_eq!(found_absent.rehydration_completed, None);
    assert_ne!(
        found_false.rehydration_completed, found_absent.rehydration_completed,
        "explicit false and absence must remain distinguishable"
    );
}

// T31 — bound_at and lease_expires_at round-trip losslessly without
// comparison or lease evaluation: even a lease_expires_at lexically before
// bound_at is stored and returned exactly as provided.
#[test]
fn t31_timestamps_round_trip_losslessly_without_evaluation() {
    let tmp = TempDir::new("eb-t31");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-ts-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut binding = minimal_binding("binding-ts-001", "role-ts-001");
    binding.bound_at = "2026-08-16T12:00:00.000Z".to_string();
    binding.lease_expires_at = "2020-01-01T00:00:00.000Z".to_string();
    repo.create_executor_binding(binding.clone())
        .expect("State must not compare lease_expires_at against bound_at");
    assert_eq!(
        repo.find_executor_binding("binding-ts-001").expect("find"),
        Some(binding)
    );
}

// T32 — binding creation does not mutate the referenced LogicalRole in any
// way.
#[test]
fn t32_binding_creation_does_not_mutate_role() {
    let tmp = TempDir::new("eb-t32");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut role = minimal_role("role-mut-001", LogicalRoleType::RuntimeA2);
    role.current_context_epoch = 5;
    role.name = Some("Role under binding".to_string());
    role.ownership_paths = vec!["receipts/one".to_string(), "receipts/two".to_string()];
    repo.create_logical_role(role.clone()).expect("role create");

    repo.create_executor_binding(minimal_binding("binding-mut-001", "role-mut-001"))
        .expect("binding create");
    assert_eq!(
        repo.find_logical_role("role-mut-001").expect("find"),
        Some(role),
        "creating a binding must leave every LogicalRole field untouched"
    );
}

// T33 — binding creation does not modify LogicalRole.active_binding_id:
// persisting a binding does not establish it as the active binding.
#[test]
fn t33_binding_creation_does_not_set_active_binding_id() {
    let tmp = TempDir::new("eb-t33");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-active-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-active-001", "role-active-001"))
        .expect("binding create");
    let found = repo
        .find_logical_role("role-active-001")
        .expect("find")
        .expect("present");
    assert_eq!(
        found.active_binding_id, None,
        "persisting a binding must not attach it to the role as active"
    );
}

// T34 — failed binding creation is atomic: neither a duplicate create nor a
// nonexistent-role create leaves any partial or changed state, durably.
#[test]
fn t34_failed_creation_is_atomic() {
    let tmp = TempDir::new("eb-t34");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-atomic-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let original = minimal_binding("binding-atomic-001", "role-atomic-001");
    repo.create_executor_binding(original.clone())
        .expect("binding create");

    let mut duplicate = minimal_binding("binding-atomic-001", "role-atomic-001");
    duplicate.provider_id = "provider-changed".to_string();
    repo.create_executor_binding(duplicate)
        .expect_err("duplicate binding_id must fail explicitly");
    repo.create_executor_binding(minimal_binding("binding-atomic-002", "role-ghost-001"))
        .expect_err("nonexistent role must fail explicitly");

    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "failed creates must leave exactly the one original binding"
    );
    assert_eq!(
        repo.find_executor_binding("binding-atomic-001")
            .expect("find"),
        Some(original),
        "the original binding must be unchanged by failed creates"
    );
    // Atomicity is durable across close/reopen.
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(repo.count_table_rows("executor_binding").expect("rows"), 1);
    assert_eq!(
        repo.find_executor_binding("binding-atomic-002")
            .expect("find"),
        None
    );
}

// T35 — no public update/delete/release/renew/expiry-evaluation
// ExecutorBinding API is introduced.
//
// This is a compile-time/API-absence invariant, not a runtime behavior, so
// no runtime test is fabricated for it. It is established by inspecting
// the public surface of the crate (see the task handoff's
// COMPILE_TIME_API_INVARIANTS section): the only public ExecutorBinding
// capabilities of this slice are `SqliteStateRepository::create_executor_binding`
// and `SqliteStateRepository::find_executor_binding`, and no method named
// or behaving like update/delete/release/renew/expire exists on any public
// type.
//
// The accepted release slice (A3-005) later added exactly one further
// public capability, `SqliteStateRepository::release_executor_binding`
// (the one-time terminal released_at/release_reason transition), and the
// accepted lease-persistence slice (A3-008) added exactly one further
// bounded capability, `SqliteStateRepository::renew_executor_binding_lease`
// (the guarded lease_expires_at-only renewal of a non-released binding);
// the update/delete/expire absence invariant above continues to hold
// beyond those two authorized additions.

// CREATE-BYPASS — public creation is not a second semantic writer of new
// LEASE_EXPIRED state.
//
// LEASE_EXPIRED is reserved to the explicit trusted lease-expiry
// transaction, which alone applies the trusted-time fences, the deadline
// predicate, provenance/actor/correlation validation, and the atomic
// release + EXECUTOR_RELEASED append. Public creation must therefore refuse
// a candidate binding already carrying that reason, before any durable
// write, whatever `released_at` the caller supplies. The complementary
// halves of the invariant — that the generic release path stays closed and
// that the explicit expiry path stays fully functional and atomic — are
// proven in `executor_binding_release_tests` and
// `executor_binding_lease_expiry_tests`.
#[test]
fn create_bypass_public_create_cannot_manufacture_lease_expired_state() {
    let tmp = TempDir::new("eb-create-bypass");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-cb-001", LogicalRoleType::RuntimeA1))
        .expect("role create");

    // CREATE-BYPASS-02 — the caller-controlled released_at carries no
    // authority whatsoever: neither an arbitrary past nor future value, nor
    // a missing one, can buy a new LEASE_EXPIRED row.
    for (index, released_at) in [
        Some("1900-01-01T00:00:00.000000000Z"),
        Some("2099-12-31T23:59:59.999999999Z"),
        Some("2026-08-16T09:59:00.000Z"),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        let mut candidate = minimal_binding(&format!("binding-cb-{index:02}"), "role-cb-001");
        candidate.released_at = released_at.map(str::to_string);
        candidate.release_reason = Some(ReleaseReason::LeaseExpired);
        // CREATE-BYPASS-01 — rejected, and rejected as a validation refusal
        // rather than a storage accident.
        let error = repo
            .create_executor_binding(candidate.clone())
            .expect_err("public create must refuse new LEASE_EXPIRED state");
        assert!(
            matches!(error, StateError::ExecutorBindingValidation { .. }),
            "unexpected error: {error}"
        );
        assert_eq!(
            repo.find_executor_binding(&candidate.binding_id)
                .expect("find"),
            None,
            "the refused candidate must leave no durable row behind"
        );
    }

    // CREATE-BYPASS-02 — no binding row of any kind was inserted.
    assert_eq!(repo.count_table_rows("executor_binding").expect("rows"), 0);
    // CREATE-BYPASS-03 — and no EXECUTOR_RELEASED (or any other) event.
    assert_eq!(repo.count_table_rows("event").expect("events"), 0);
    // CREATE-BYPASS-04 — a rejected create never creates or advances
    // trusted-time state: only the fenced expiry path touches watermarks.
    assert_eq!(
        repo.count_table_rows("trusted_time_watermark")
            .expect("watermarks"),
        0
    );
    assert_eq!(
        repo.find_trusted_time_watermark("project-1")
            .expect("watermark"),
        None
    );

    // CREATE-BYPASS-05 — the refusal is inert: an ordinary valid active
    // binding for the same role still creates normally afterwards, so the
    // rejected attempt neither consumed the role nor poisoned any state.
    let valid = minimal_binding("binding-cb-live", "role-cb-001");
    repo.create_executor_binding(valid.clone())
        .expect("a normal active binding still creates after the refusal");
    assert_eq!(
        repo.find_executor_binding("binding-cb-live").expect("find"),
        Some(valid)
    );

    // CREATE-BYPASS-07/08 (non-regression half) — the other eight frozen
    // reasons keep their existing creation behavior, including the
    // pre-released shapes the accepted baseline supports. Creation-time
    // release semantics are narrowed for LEASE_EXPIRED only.
    repo.create_logical_role(minimal_role("role-cb-002", LogicalRoleType::RuntimeA1))
        .expect("role create");
    for (index, reason) in ALL_NINE_REASONS
        .iter()
        .filter(|reason| **reason != ReleaseReason::LeaseExpired)
        .enumerate()
    {
        let mut released = minimal_binding(&format!("binding-cb-ok-{index:02}"), "role-cb-002");
        released.released_at = Some("2026-08-16T09:59:00.000Z".to_string());
        released.release_reason = Some(*reason);
        repo.create_executor_binding(released.clone())
            .expect("non-LEASE_EXPIRED creation behavior is unchanged");
        assert_eq!(
            repo.find_executor_binding(&released.binding_id)
                .expect("find"),
            Some(released)
        );
    }
}

// CREATE-BYPASS-11 — exactly one semantic writer of new LEASE_EXPIRED state
// remains in the whole State crate.
//
// Source-inspection invariant over all of `src/state/**` (not only the
// binding modules), established by searching for `LeaseExpired`,
// `LEASE_EXPIRED`, `release_reason`, `released_at`, `INSERT_BINDING_SQL`,
// `create_executor_binding`, and `release_executor_binding`:
//
// * `INSERT_BINDING_SQL` (`create_executor_binding`) — the only INSERT that
//   can write `release_reason`. `validate_for_create` now refuses a
//   `LEASE_EXPIRED` candidate before any storage access, so this path can
//   no longer create that state. `UnitOfWork::insert_executor_binding` is
//   crate-private and has exactly one production caller, that same guarded
//   `create_executor_binding`.
// * `RELEASE_BINDING_SQL` (`apply_release`) — the only UPDATE that writes
//   `release_reason`, reachable from exactly two callers:
//   `release_executor_binding`, which rejects `LEASE_EXPIRED` outright, and
//   `apply_expiry_with_fenced_time`, the explicit trusted lease-expiry
//   transaction. The latter is the one authorized writer: TrustedClockV1 →
//   Phase A fence → Phase B fence → current-deadline reread → expiry
//   predicate → provenance/actor/correlation validation → LEASE_EXPIRED
//   release plus EXECUTOR_RELEASED in one atomic transaction.
// * `RENEW_LEASE_SQL` — writes `lease_expires_at` only, never the terminal
//   pair, so renewal cannot produce a release of any reason.
// * `src/state/context_rehydration.rs` — reads `release_reason` through
//   `find_executor_binding` for projection only; it holds no binding write.
// * `src/state/migrations/**` — v0003 declares the closed `release_reason`
//   CHECK and v0005 the partial unique index; neither backfills, rewrites,
//   nor converts any row, and this slice adds no migration.
//
// No scanner, timer, startup sweep, failover, rebind, or alternate public
// helper exists that could write the terminal pair.
