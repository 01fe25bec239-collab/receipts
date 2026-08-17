//! Deterministic tests for durable LogicalRole create/read persistence
//! (T01–T18 of migration 0002).
//!
//! All tests use real temporary SQLite database files under the system
//! temporary directory (never inside the repository). Storage-backstop
//! checks that need SQL beyond the public repository API run through
//! crate-private test helpers only and are `#[cfg(test)]`-gated.

use crate::error::StateError;
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

/// A minimal contract-valid role: only required fields set, epoch 0.
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

// T01 — a fresh database bootstraps through the registered chain, which
// since migration 0007 ends at schema version 7.
#[test]
fn t01_fresh_database_reaches_schema_version_7() {
    let tmp = TempDir::new("lr-t01");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("fresh database bootstraps");
    assert_eq!(repo.schema_version().expect("version read"), 8);
    assert!(
        repo.table_exists("logical_role").expect("table check"),
        "logical_role must exist after migration 2"
    );
    assert!(
        repo.table_exists("logical_role_ownership_path")
            .expect("table check"),
        "ownership-path table must exist after migration 2"
    );
    assert!(
        repo.table_exists("executor_binding").expect("table check"),
        "executor_binding must exist after migration 3"
    );
    assert!(
        repo.table_exists("event").expect("table check"),
        "event must exist after migration 4"
    );
    // The registered chain creates no domain storage beyond roles, executor
    // bindings, the event log, the migration-6 context manifests, and the
    // migration-7 context epochs.
    for forbidden in ["event_log", "entitlement", "binding_lease"] {
        assert!(
            !repo.table_exists(forbidden).expect("table check"),
            "no {forbidden} storage may be created by the registered chain"
        );
    }
}

// T02 — reopening a version-7 database is idempotent.
#[test]
fn t02_version_7_reopen_idempotent() {
    let tmp = TempDir::new("lr-t02");
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

// T03 — ordinary open of an existing version-1 database fails closed
// instead of silently upgrading it.
#[test]
fn t03_ordinary_open_of_version_1_database_fails() {
    let tmp = TempDir::new("lr-t03");
    let version_1_chain = &migrations::registered()[..1];
    drop(
        SqliteStateRepository::open_with_migrations(tmp.db_path(), version_1_chain)
            .expect("bootstrap at version 1"),
    );
    let error = SqliteStateRepository::open(tmp.db_path())
        .expect_err("ordinary open must not silently upgrade a version-1 database");
    assert!(
        matches!(
            error,
            StateError::SchemaVersionMismatch {
                found: 1,
                supported: 8
            }
        ),
        "unexpected error: {error}"
    );
    // The failed open left the database untouched at version 1.
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_1_chain)
        .expect("database still opens at version 1");
    assert_eq!(repo.schema_version().expect("version read"), 1);
}

// T04 — a minimal valid RUNTIME_A1 LogicalRole persists and reads back
// exactly.
#[test]
fn t04_create_minimal_runtime_a1_role() {
    let tmp = TempDir::new("lr-t04");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let role = minimal_role("role-a1-001", LogicalRoleType::RuntimeA1);
    repo.create_logical_role(role.clone()).expect("create");
    assert_eq!(
        repo.find_logical_role("role-a1-001").expect("find"),
        Some(role)
    );
}

// T05 — a minimal valid RUNTIME_A2 LogicalRole persists and reads back
// exactly.
#[test]
fn t05_create_minimal_runtime_a2_role() {
    let tmp = TempDir::new("lr-t05");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let role = minimal_role("role-a2-001", LogicalRoleType::RuntimeA2);
    repo.create_logical_role(role.clone()).expect("create");
    assert_eq!(
        repo.find_logical_role("role-a2-001").expect("find"),
        Some(role)
    );
}

// T06 — every contract field round-trips exactly.
#[test]
fn t06_full_field_round_trip() {
    let tmp = TempDir::new("lr-t06");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let role = LogicalRole {
        role_id: "role-full-001".to_string(),
        project_id: "project-42".to_string(),
        role_type: LogicalRoleType::RuntimeA2,
        status: LogicalRoleStatus::Suspended,
        current_context_epoch: 7,
        name: Some("Receipts builder role".to_string()),
        workstream_id: Some("workstream-alpha".to_string()),
        ownership_paths: vec!["receipts/core".to_string(), "receipts/edge".to_string()],
        integration_branch: Some("build/role-full-001".to_string()),
        context_manifest_id: Some("manifest-0009".to_string()),
        active_binding_id: Some("binding-0004".to_string()),
        created_at: Some("2026-08-16T10:15:00.000Z".to_string()),
    };
    repo.create_logical_role(role.clone()).expect("create");
    assert_eq!(
        repo.find_logical_role("role-full-001").expect("find"),
        Some(role)
    );
}

// T07 — a persisted role survives database close and reopen.
#[test]
fn t07_role_survives_close_and_reopen() {
    let tmp = TempDir::new("lr-t07");
    let role = minimal_role("role-durable-001", LogicalRoleType::RuntimeA1);
    {
        let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
        repo.create_logical_role(role.clone()).expect("create");
    }
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_logical_role("role-durable-001").expect("find"),
        Some(role)
    );
}

// T08 — ownership_paths round-trip exactly, preserving order (and
// duplicates): only stored positions can reproduce the original sequence.
#[test]
fn t08_ownership_paths_round_trip_in_order() {
    let tmp = TempDir::new("lr-t08");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut role = minimal_role("role-paths-001", LogicalRoleType::RuntimeA1);
    // Deliberately non-alphabetical, with a duplicate and an empty entry.
    role.ownership_paths = vec![
        "zeta".to_string(),
        "alpha".to_string(),
        "mid/child".to_string(),
        "alpha".to_string(),
        String::new(),
    ];
    repo.create_logical_role(role.clone()).expect("create");
    let found = repo
        .find_logical_role("role-paths-001")
        .expect("find")
        .expect("role exists");
    assert_eq!(found.ownership_paths, role.ownership_paths);
    assert_eq!(found.ownership_paths.first(), Some(&"zeta".to_string()));
    assert_eq!(found.ownership_paths.len(), 5);
}

// T09 — optional nullable fields round-trip as absent or exactly as stored.
#[test]
fn t09_optional_fields_round_trip() {
    let tmp = TempDir::new("lr-t09");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");

    // Every nullable scalar absent stays absent.
    let bare = minimal_role("role-bare-001", LogicalRoleType::RuntimeA1);
    repo.create_logical_role(bare).expect("create");
    let found = repo
        .find_logical_role("role-bare-001")
        .expect("find")
        .expect("present");
    assert_eq!(found.name, None);
    assert_eq!(found.workstream_id, None);
    assert_eq!(found.integration_branch, None);
    assert_eq!(found.context_manifest_id, None);
    assert_eq!(found.active_binding_id, None);
    assert_eq!(found.created_at, None);

    // Mixed presence round-trips each field independently.
    let mut mixed = minimal_role("role-mixed-001", LogicalRoleType::RuntimeA2);
    mixed.name = Some("Named role".to_string());
    mixed.integration_branch = Some("build/role-mixed-001".to_string());
    mixed.created_at = Some("2026-08-16T00:00:00.000Z".to_string());
    repo.create_logical_role(mixed.clone()).expect("create");
    assert_eq!(
        repo.find_logical_role("role-mixed-001").expect("find"),
        Some(mixed)
    );
}

// T10 — duplicate role_id creation fails explicitly and never overwrites the
// original.
#[test]
fn t10_duplicate_role_id_fails_without_overwrite() {
    let tmp = TempDir::new("lr-t10");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut original = minimal_role("role-dup-001", LogicalRoleType::RuntimeA1);
    original.status = LogicalRoleStatus::Retired;
    original.current_context_epoch = 3;
    original.name = Some("original".to_string());
    repo.create_logical_role(original.clone()).expect("create");

    let mut duplicate = minimal_role("role-dup-001", LogicalRoleType::RuntimeA2);
    duplicate.current_context_epoch = 99;
    duplicate.name = Some("impostor".to_string());
    duplicate.ownership_paths = vec!["should/not/persist".to_string()];
    let error = repo
        .create_logical_role(duplicate)
        .expect_err("duplicate role_id must fail explicitly");
    assert!(
        matches!(
            &error,
            StateError::LogicalRoleAlreadyExists { role_id } if role_id == "role-dup-001"
        ),
        "unexpected error: {error}"
    );
    // The original is intact, not overwritten, replaced, or merged.
    assert_eq!(
        repo.find_logical_role("role-dup-001").expect("find"),
        Some(original)
    );
}

// T11 — a failed duplicate create leaves no partial child/ownership rows.
#[test]
fn t11_duplicate_failure_leaves_no_partial_rows() {
    let tmp = TempDir::new("lr-t11");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut original = minimal_role("role-atomic-001", LogicalRoleType::RuntimeA1);
    original.ownership_paths = vec!["p/one".to_string(), "p/two".to_string()];
    repo.create_logical_role(original).expect("create");

    let mut duplicate = minimal_role("role-atomic-001", LogicalRoleType::RuntimeA2);
    duplicate.ownership_paths = vec!["x".to_string(), "y".to_string(), "z".to_string()];
    let error = repo
        .create_logical_role(duplicate)
        .expect_err("duplicate role_id must fail explicitly");
    assert!(
        matches!(error, StateError::LogicalRoleAlreadyExists { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("logical_role").expect("rows"),
        1,
        "no partial role row may survive a failed create"
    );
    assert_eq!(
        repo.count_table_rows("logical_role_ownership_path")
            .expect("rows"),
        2,
        "no partial ownership rows may survive a failed create"
    );
}

// T12 — looking up a role that was never created is a deterministic
// absence.
#[test]
fn t12_missing_role_lookup_is_deterministic_absence() {
    let tmp = TempDir::new("lr-t12");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(
        repo.find_logical_role("never-created").expect("find"),
        None,
        "a never-created role must be reported absent"
    );
    repo.create_logical_role(minimal_role("role-present-001", LogicalRoleType::RuntimeA1))
        .expect("create");
    assert_eq!(
        repo.find_logical_role("never-created").expect("find"),
        None,
        "absence must remain deterministic once other roles exist"
    );
    assert_eq!(
        repo.find_logical_role("").expect("find"),
        None,
        "an empty identifier can never match a stored role"
    );
}

// T13 — current_context_epoch = 0 is accepted (epoch zero is valid).
#[test]
fn t13_epoch_zero_accepted() {
    let tmp = TempDir::new("lr-t13");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let role = minimal_role("role-epoch-zero-001", LogicalRoleType::RuntimeA1);
    assert_eq!(role.current_context_epoch, 0);
    repo.create_logical_role(role.clone()).expect("create");
    let found = repo
        .find_logical_role("role-epoch-zero-001")
        .expect("find")
        .expect("present");
    assert_eq!(found.current_context_epoch, 0);
}

// T14 — a negative context epoch cannot persist through the typed
// repository path.
#[test]
fn t14_negative_epoch_rejected() {
    let tmp = TempDir::new("lr-t14");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut role = minimal_role("role-epoch-neg-001", LogicalRoleType::RuntimeA1);
    role.current_context_epoch = -1;
    let error = repo
        .create_logical_role(role)
        .expect_err("negative context epoch must be rejected");
    assert!(
        matches!(error, StateError::LogicalRoleValidation { .. }),
        "unexpected error: {error}"
    );
    // Nothing from the rejected create persisted.
    assert_eq!(
        repo.find_logical_role("role-epoch-neg-001").expect("find"),
        None
    );
    assert_eq!(repo.count_table_rows("logical_role").expect("rows"), 0);
}

// T15 — no durable role type outside {RUNTIME_A1, RUNTIME_A2} can persist
// through the typed path.
#[test]
fn t15_invalid_durable_role_type_cannot_persist() {
    // The typed API accepts only the enum, so RUNTIME_A3/RUNTIME_A4 are not
    // representable; the storage decode boundary rejects them as well.
    for invalid in ["RUNTIME_A3", "RUNTIME_A4", "RUNTIME_A5", "runtime_a1", ""] {
        assert!(
            matches!(
                LogicalRoleType::from_storage(invalid),
                Err(StateError::LogicalRoleDecodeFailed { .. })
            ),
            "role_type {invalid:?} must not decode to a durable role type"
        );
    }
    // The schema-level CHECK is the durable backstop: even direct SQL inside
    // the State layer cannot persist an ephemeral runtime as a durable role.
    let tmp = TempDir::new("lr-t15");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let error = repo
        .run_transaction(|uow| {
            uow.execute(
                "INSERT INTO logical_role (
                    role_id, project_id, role_type, status, current_context_epoch
                ) VALUES ('probe-a3', 'project-1', 'RUNTIME_A3', 'ACTIVE', 0)",
                &[],
            )
        })
        .expect_err("storage must reject RUNTIME_A3 as a durable role type");
    assert!(
        matches!(error, StateError::InternalQueryFailed { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.find_logical_role("probe-a3").expect("find"), None);
}

// T16 — an invalid role status cannot persist through the typed path.
#[test]
fn t16_invalid_role_status_cannot_persist() {
    for invalid in ["ARCHIVED", "PENDING", "active", ""] {
        assert!(
            matches!(
                LogicalRoleStatus::from_storage(invalid),
                Err(StateError::LogicalRoleDecodeFailed { .. })
            ),
            "status {invalid:?} must not decode to a contract status"
        );
    }
    // Schema-level CHECK backstop for statuses.
    let tmp = TempDir::new("lr-t16");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let error = repo
        .run_transaction(|uow| {
            uow.execute(
                "INSERT INTO logical_role (
                    role_id, project_id, role_type, status, current_context_epoch
                ) VALUES ('probe-status', 'project-1', 'RUNTIME_A1', 'ARCHIVED', 0)",
                &[],
            )
        })
        .expect_err("storage must reject a non-contract status");
    assert!(
        matches!(error, StateError::InternalQueryFailed { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(repo.find_logical_role("probe-status").expect("find"), None);
}

// T17 — required-identifier constraints are enforced at the typed boundary.
#[test]
fn t17_identifier_constraints_enforced() {
    let tmp = TempDir::new("lr-t17");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");

    fn rejected(repo: &mut SqliteStateRepository, role: LogicalRole, what: &str) {
        let error = repo.create_logical_role(role).expect_err(what);
        assert!(
            matches!(error, StateError::LogicalRoleValidation { .. }),
            "{what}: unexpected error: {error}"
        );
    }

    let too_long = "r".repeat(201);

    let mut role = minimal_role("role-v-001", LogicalRoleType::RuntimeA1);
    role.role_id = String::new();
    rejected(&mut repo, role, "an empty role_id");
    rejected(
        &mut repo,
        minimal_role(&too_long, LogicalRoleType::RuntimeA1),
        "a 201-character role_id",
    );

    let mut role = minimal_role("role-v-002", LogicalRoleType::RuntimeA1);
    role.project_id = String::new();
    rejected(&mut repo, role, "an empty project_id");
    let mut role = minimal_role("role-v-003", LogicalRoleType::RuntimeA1);
    role.project_id = too_long.clone();
    rejected(&mut repo, role, "a 201-character project_id");

    let mut role = minimal_role("role-v-004", LogicalRoleType::RuntimeA1);
    role.workstream_id = Some(String::new());
    rejected(&mut repo, role, "an empty workstream_id");
    let mut role = minimal_role("role-v-005", LogicalRoleType::RuntimeA1);
    role.workstream_id = Some(too_long.clone());
    rejected(&mut repo, role, "a 201-character workstream_id");

    let mut role = minimal_role("role-v-006", LogicalRoleType::RuntimeA1);
    role.context_manifest_id = Some(String::new());
    rejected(&mut repo, role, "an empty context_manifest_id");
    let mut role = minimal_role("role-v-007", LogicalRoleType::RuntimeA1);
    role.context_manifest_id = Some(too_long);
    rejected(&mut repo, role, "a 201-character context_manifest_id");

    // Nothing from the rejected creates persisted.
    assert_eq!(repo.count_table_rows("logical_role").expect("rows"), 0);

    // Boundary: exactly 200 characters is accepted for every constrained
    // identifier.
    let exact = "e".repeat(200);
    let mut role = minimal_role(&exact, LogicalRoleType::RuntimeA1);
    role.project_id = exact.clone();
    role.workstream_id = Some(exact.clone());
    role.context_manifest_id = Some(exact);
    repo.create_logical_role(role.clone())
        .expect("200-character identifiers are valid");
    assert_eq!(
        repo.find_logical_role(&"e".repeat(200)).expect("find"),
        Some(role)
    );
}

// T18 — durable identity survives reopen without executor/session identity.
#[test]
fn t18_identity_survives_reopen_without_executor_or_session_identity() {
    let tmp = TempDir::new("lr-t18");
    let role = minimal_role("role-identity-001", LogicalRoleType::RuntimeA2);
    {
        let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
        repo.create_logical_role(role.clone()).expect("create");
    }
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    let found = repo
        .find_logical_role("role-identity-001")
        .expect("find")
        .expect("role survived reopen");
    assert_eq!(found, role);
    // Identity persists without any binding or manifest reference attached.
    assert_eq!(found.active_binding_id, None);
    assert_eq!(found.context_manifest_id, None);
    // A LogicalRole is durable identity, not an LLM session: no lease,
    // session, or epoch-attachment storage exists in this schema (the
    // executor_binding table of migration 3 stores associations only and
    // never attaches one to the role; the context_manifest table of
    // migration 6 references roles but is a separate graph; the
    // context_epoch table of migration 7 is project-scoped history and
    // never attaches to a role).
    for absent in ["binding_lease", "llm_session", "session", "role_epoch"] {
        assert!(
            !repo.table_exists(absent).expect("table check"),
            "no {absent} storage may exist in this slice"
        );
    }
}
