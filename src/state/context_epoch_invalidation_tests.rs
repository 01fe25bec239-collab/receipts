//! Schema-v8 and atomic ContextEpoch invalidation evidence tests.

use rusqlite::ToSql;

use crate::context_epoch::{ContextEpoch, ContextEpochTrigger};
use crate::error::StateError;
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

const ADVANCED_AT: &str = "2026-08-17T12:00:00.000Z";
const INSERT_CHILD: &str = "INSERT INTO context_epoch_invalidated_role
    (project_id, epoch, role_id) VALUES (?1, ?2, ?3)";

type AdvanceContextEpochFn = fn(
    &mut SqliteStateRepository,
    &str,
    &str,
    ContextEpochTrigger,
    &[String],
) -> Result<ContextEpoch, StateError>;
type FindInvalidatedRoleIdsFn =
    fn(&SqliteStateRepository, &str, i64) -> Result<Option<Vec<String>>, StateError>;

fn role(role_id: &str, project_id: &str) -> LogicalRole {
    LogicalRole {
        role_id: role_id.to_string(),
        project_id: project_id.to_string(),
        role_type: LogicalRoleType::RuntimeA2,
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

fn epoch(project_id: &str, epoch: i64) -> ContextEpoch {
    ContextEpoch {
        project_id: project_id.to_string(),
        epoch,
        advanced_at: ADVANCED_AT.to_string(),
        trigger: ContextEpochTrigger::NewWave,
    }
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn direct_child(
    repo: &mut SqliteStateRepository,
    project_id: &str,
    epoch: i64,
    role_id: &str,
) -> Result<(), StateError> {
    repo.run_transaction(|uow| {
        uow.execute(INSERT_CHILD, &[&project_id as &dyn ToSql, &epoch, &role_id])?;
        Ok(())
    })
}

#[test]
fn v8_bootstrap_reopen_and_v7_open_fail_closed() {
    let fresh = TempDir::new("cei-v8-fresh");
    let repo = SqliteStateRepository::open(fresh.db_path()).expect("bootstrap v8");
    assert_eq!(repo.schema_version().expect("version"), 8);
    assert_eq!(migrations::registered().len(), 8);
    drop(repo);
    assert_eq!(
        SqliteStateRepository::open(fresh.db_path())
            .expect("reopen v8")
            .schema_version()
            .expect("version"),
        8
    );

    let old = TempDir::new("cei-v7-open");
    drop(
        SqliteStateRepository::open_with_migrations(old.db_path(), &migrations::registered()[..7])
            .expect("create v7"),
    );
    assert!(matches!(
        SqliteStateRepository::open(old.db_path()).expect_err("ordinary open refuses upgrade"),
        StateError::SchemaVersionMismatch {
            found: 7,
            supported: 8
        }
    ));
}

#[test]
fn v8_schema_is_exact_and_has_both_composite_foreign_keys() {
    let tmp = TempDir::new("cei-schema");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(
        repo.table_columns("context_epoch_invalidated_role")
            .expect("columns"),
        ids(&["project_id", "epoch", "role_id"])
    );
    let table_sql = repo
        .sqlite_master_entries("table", "context_epoch_invalidated_role")
        .expect("table SQL")
        .pop()
        .expect("table")
        .1;
    assert!(table_sql.contains("PRIMARY KEY (project_id, epoch, role_id)"));
    assert!(table_sql.contains("REFERENCES context_epoch (project_id, epoch)"));
    assert!(table_sql.contains("REFERENCES logical_role (role_id, project_id)"));
    for forbidden in [
        "ordinal",
        "reason",
        "payload",
        "timestamp",
        "invalidated_at",
    ] {
        assert!(!table_sql.to_lowercase().contains(forbidden));
    }

    let indexes = repo
        .sqlite_master_entries("index", "logical_role")
        .expect("indexes");
    assert_eq!(
        indexes
            .iter()
            .filter(|(name, _)| !name.starts_with("sqlite_autoindex"))
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec!["idx_logical_role_role_id_project_id"]
    );
    let migration_sql = migrations::registered()[7].sql;
    assert_eq!(migration_sql.matches("CREATE TABLE").count(), 1);
    assert_eq!(migration_sql.matches("CREATE UNIQUE INDEX").count(), 1);
    assert_eq!(migration_sql.matches("CREATE TRIGGER").count(), 0);
    assert_eq!(migration_sql.matches("CREATE VIEW").count(), 0);
    for forbidden in [
        "context_epoch_changed_source",
        "context_epoch_current",
        "context_epoch_reconciliation",
        "context_epoch_invalidation_status",
    ] {
        assert!(!repo.table_exists(forbidden).expect("table check"));
    }
}

#[test]
fn manual_v7_to_v8_migration_invents_no_historical_children() {
    let tmp = TempDir::new("cei-v7-v8");
    let mut repo =
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &migrations::registered()[..7])
            .expect("v7");
    repo.append_context_epoch(epoch("P", 4)).expect("old epoch");
    repo.run_transaction(|uow| uow.execute_batch(migrations::registered()[7].sql))
        .expect("authorized migration application");
    assert_eq!(repo.schema_version().expect("version"), 8);
    assert_eq!(
        repo.count_table_rows("context_epoch_invalidated_role")
            .expect("rows"),
        0
    );
    assert_eq!(
        repo.find_context_epoch("P", 4).expect("find"),
        Some(epoch("P", 4))
    );
}

#[test]
fn failed_v8_migration_rolls_back_index_table_and_version() {
    let tmp = TempDir::new("cei-v8-rollback");
    let mut repo =
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &migrations::registered()[..7])
            .expect("v7");
    repo.run_transaction(|uow| {
        uow.execute_batch("CREATE TABLE context_epoch_invalidated_role (collision TEXT);")
    })
    .expect("collision table");
    repo.run_transaction(|uow| uow.execute_batch(migrations::registered()[7].sql))
        .expect_err("migration must fail atomically");
    assert_eq!(repo.schema_version().expect("version"), 7);
    assert!(
        repo.sqlite_master_entries("index", "logical_role")
            .expect("indexes")
            .iter()
            .all(|(name, _)| name != "idx_logical_role_role_id_project_id")
    );
    assert_eq!(
        repo.table_columns("context_epoch_invalidated_role")
            .expect("collision columns"),
        ids(&["collision"])
    );
}

#[test]
fn empty_and_multiple_sets_round_trip_and_survive_reopen() {
    let tmp = TempDir::new("cei-roundtrip");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    for role_id in ["R3", "R1", "R2"] {
        repo.create_logical_role(role(role_id, "P")).expect("role");
    }
    let zero = repo
        .advance_context_epoch("P", ADVANCED_AT, ContextEpochTrigger::A1Init, &[])
        .expect("empty set");
    assert_eq!(zero.epoch, 0);
    assert_eq!(
        repo.find_context_epoch_invalidated_role_ids("P", 0)
            .expect("read"),
        Some(Vec::new())
    );
    let supplied = ids(&["R3", "R1", "R2"]);
    let one = repo
        .advance_context_epoch("P", ADVANCED_AT, ContextEpochTrigger::NewWave, &supplied)
        .expect("roles");
    assert_eq!(one.epoch, 1);
    assert_eq!(repo.find_context_epoch("P", 1).expect("parent"), Some(one));
    assert_eq!(
        repo.find_context_epoch_invalidated_role_ids("P", 1)
            .expect("read"),
        Some(ids(&["R1", "R2", "R3"]))
    );
    assert_eq!(
        repo.find_context_epoch_invalidated_role_ids("P", 99)
            .expect("missing"),
        None
    );
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_context_epoch_invalidated_role_ids("P", 1)
            .expect("read"),
        Some(ids(&["R1", "R2", "R3"]))
    );
}

#[test]
fn structural_duplicate_missing_and_cross_project_inputs_fail_without_epoch() {
    let tmp = TempDir::new("cei-validation");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(role("R1", "P1")).expect("role");
    repo.create_logical_role(role("R2", "P2")).expect("role");
    let error = repo
        .advance_context_epoch(
            "P1",
            ADVANCED_AT,
            ContextEpochTrigger::NewWave,
            &ids(&["R1", "R1"]),
        )
        .expect_err("duplicate");
    assert!(matches!(
        error,
        StateError::ContextEpochInvalidatedRoleDuplicate { .. }
    ));
    let error = repo
        .advance_context_epoch(
            "P1",
            ADVANCED_AT,
            ContextEpochTrigger::NewWave,
            &ids(&["R404"]),
        )
        .expect_err("missing");
    assert!(matches!(
        error,
        StateError::ContextEpochInvalidatedRoleNotFound { .. }
    ));
    let error = repo
        .advance_context_epoch(
            "P1",
            ADVANCED_AT,
            ContextEpochTrigger::NewWave,
            &ids(&["R2"]),
        )
        .expect_err("cross project");
    assert!(matches!(
        error,
        StateError::ContextEpochInvalidatedRoleProjectMismatch { .. }
    ));
    assert_eq!(repo.find_latest_context_epoch("P1").expect("latest"), None);
    for role_ids in [vec![String::new()], vec!["x".repeat(201)]] {
        assert!(matches!(
            repo.advance_context_epoch("P1", ADVANCED_AT, ContextEpochTrigger::NewWave, &role_ids)
                .expect_err("invalid role id"),
            StateError::ContextEpochValidation { .. }
        ));
    }
    assert_eq!(repo.count_table_rows("context_epoch").expect("parents"), 0);
    assert_eq!(
        repo.count_table_rows("context_epoch_invalidated_role")
            .expect("children"),
        0
    );
}

#[test]
fn typed_cross_project_error_names_both_projects() {
    let tmp = TempDir::new("cei-cross-error");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(role("R", "P2")).expect("role");
    let error = repo
        .advance_context_epoch(
            "P1",
            ADVANCED_AT,
            ContextEpochTrigger::HostSwitch,
            &ids(&["R"]),
        )
        .expect_err("cross project");
    assert!(matches!(
        error,
        StateError::ContextEpochInvalidatedRoleProjectMismatch {
            epoch_project_id,
            role_id,
            role_project_id
        } if epoch_project_id == "P1" && role_id == "R" && role_project_id == "P2"
    ));
}

#[test]
fn database_rejects_missing_parent_missing_role_cross_project_and_duplicate() {
    let tmp = TempDir::new("cei-db-constraints");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(role("R1", "P1")).expect("role P1");
    repo.create_logical_role(role("R2", "P2")).expect("role P2");
    repo.append_context_epoch(epoch("P1", 0)).expect("parent");

    for (project_id, epoch, role_id) in [("P1", 99, "R1"), ("P1", 0, "R404"), ("P1", 0, "R2")] {
        assert!(direct_child(&mut repo, project_id, epoch, role_id).is_err());
    }
    direct_child(&mut repo, "P1", 0, "R1").expect("valid child");
    assert!(direct_child(&mut repo, "P1", 0, "R1").is_err());
    assert_eq!(
        repo.count_table_rows("context_epoch_invalidated_role")
            .expect("rows"),
        1
    );
}

#[test]
fn later_child_failure_rolls_back_parent_and_earlier_child() {
    let tmp = TempDir::new("cei-child-rollback");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(role("R1", "P1")).expect("role P1");
    repo.create_logical_role(role("R2", "P2")).expect("role P2");
    repo.run_transaction(|uow| {
        uow.insert_context_epoch(&epoch("P1", 0))?;
        uow.execute(INSERT_CHILD, &[&"P1", &0_i64, &"R1"])?;
        uow.execute(INSERT_CHILD, &[&"P1", &0_i64, &"R2"])?;
        Ok(())
    })
    .expect_err("second child violates same-project FK");
    assert_eq!(repo.find_context_epoch("P1", 0).expect("parent"), None);
    assert_eq!(
        repo.count_table_rows("context_epoch_invalidated_role")
            .expect("children"),
        0
    );
}

#[test]
fn changed_assessment_creates_later_immutable_history_and_role_is_unchanged() {
    let tmp = TempDir::new("cei-history");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let r1 = role("R1", "P");
    for role_record in [r1.clone(), role("R2", "P"), role("R3", "P")] {
        repo.create_logical_role(role_record).expect("role");
    }
    repo.advance_context_epoch("P", ADVANCED_AT, ContextEpochTrigger::A1Init, &ids(&["R1"]))
        .expect("epoch 0");
    repo.advance_context_epoch(
        "P",
        ADVANCED_AT,
        ContextEpochTrigger::ContractChange,
        &ids(&["R2", "R3"]),
    )
    .expect("epoch 1");
    assert_eq!(
        repo.find_context_epoch_invalidated_role_ids("P", 0)
            .expect("read"),
        Some(ids(&["R1"]))
    );
    assert_eq!(
        repo.find_context_epoch_invalidated_role_ids("P", 1)
            .expect("read"),
        Some(ids(&["R2", "R3"]))
    );
    assert_eq!(repo.find_logical_role("R1").expect("role"), Some(r1));

    repo.advance_context_epoch(
        "P",
        ADVANCED_AT,
        ContextEpochTrigger::NewWave,
        &ids(&["R1"]),
    )
    .expect("same role in later epoch");
    assert_eq!(
        repo.find_context_epoch_invalidated_role_ids("P", 2)
            .expect("read"),
        Some(ids(&["R1"]))
    );

    repo.create_logical_role(role("R4", "P2")).expect("P2 role");
    repo.advance_context_epoch(
        "P2",
        ADVANCED_AT,
        ContextEpochTrigger::A2Init,
        &ids(&["R4"]),
    )
    .expect("independent P2 epoch zero");
    assert_eq!(
        repo.find_context_epoch_invalidated_role_ids("P2", 0)
            .expect("P2 read"),
        Some(ids(&["R4"]))
    );
}

#[test]
fn malformed_persisted_role_id_fails_read_closed() {
    let tmp = TempDir::new("cei-corrupt-read");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.append_context_epoch(epoch("P", 0)).expect("parent");
    repo.run_transaction(|uow| {
        uow.execute(
            "INSERT INTO logical_role
             (role_id, project_id, role_type, status, current_context_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[&"", &"P", &"RUNTIME_A1", &"ACTIVE", &0_i64],
        )?;
        uow.execute(INSERT_CHILD, &[&"P", &0_i64, &""])?;
        Ok(())
    })
    .expect("inject storage the typed boundary cannot create");
    assert!(matches!(
        repo.find_context_epoch_invalidated_role_ids("P", 0)
            .expect_err("malformed child must fail closed"),
        StateError::ContextEpochInvalidationDecodeFailed { .. }
    ));
}

#[test]
fn invalidation_read_validates_query_shape_and_public_signature_is_pinned() {
    let tmp = TempDir::new("cei-read-validation");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert!(matches!(
        repo.find_context_epoch_invalidated_role_ids("", 0)
            .expect_err("project"),
        StateError::ContextEpochValidation { .. }
    ));
    assert!(matches!(
        repo.find_context_epoch_invalidated_role_ids("P", -1)
            .expect_err("epoch"),
        StateError::ContextEpochValidation { .. }
    ));
    let advance: AdvanceContextEpochFn = SqliteStateRepository::advance_context_epoch;
    let read: FindInvalidatedRoleIdsFn =
        SqliteStateRepository::find_context_epoch_invalidated_role_ids;
    let _ = (advance, read);
}
