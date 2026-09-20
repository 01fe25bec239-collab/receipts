//! Deterministic tests for the State repository foundation (T1–T13).
//!
//! All tests use real temporary SQLite database files under the system
//! temporary directory (never inside the repository) and clean up after
//! themselves. Test-only SQL runs through crate-private helpers and is never
//! exposed through the public repository API.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::OptionalExtension;

use crate::error::StateError;
use crate::migrations::{self, Migration};
use crate::repository::SqliteStateRepository;
use crate::trusted_time::{TrustedClockV1, TrustedTimeSampleV1};

pub(crate) fn state_epoch(value: i64) -> crate::StateEpochValueV1 {
    crate::StateEpochValueV1::try_from(value).expect("non-negative test epoch")
}

pub(crate) fn state_epoch_text(value: &str) -> crate::StateEpochValueV1 {
    crate::StateEpochValueV1::try_from(value).expect("canonical test epoch")
}

pub(crate) struct FakeTrustedClock(pub(crate) TrustedTimeSampleV1);

impl TrustedClockV1 for FakeTrustedClock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(self.0.clone())
    }
}

pub(crate) fn trusted_clock_at(timestamp: &str) -> FakeTrustedClock {
    FakeTrustedClock(TrustedTimeSampleV1 {
        canonical_utc_timestamp: timestamp.to_string(),
        clock_source_id: "test-clock".to_string(),
        clock_contract_version: "v1".to_string(),
    })
}

pub(crate) fn trusted_clock() -> FakeTrustedClock {
    trusted_clock_at("2026-08-16T09:00:00.000000000Z")
}

/// A temporary directory holding one state database file, removed on drop.
pub(crate) struct TempDir {
    path: PathBuf,
}

impl TempDir {
    pub(crate) fn new(tag: &str) -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "receipts-state-test-{tag}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("temporary test directory creation");
        Self { path }
    }

    pub(crate) fn db_path(&self) -> PathBuf {
        self.path.join("state.sqlite3")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.db_path().display()));
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A test-only migration creating a probe table and recording its version,
/// used to construct databases at versions above the registered chain.
fn probe_migration(version: u32, name: &'static str) -> Migration {
    Migration {
        version,
        name,
        // Leaked so the SQL can be composed per test version; a handful of
        // static strings per test run, never freed — acceptable in tests.
        sql: Box::leak(
            format!(
                "CREATE TABLE IF NOT EXISTS migration_probe (
    id INTEGER PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO state_schema_version (version, migration_name) VALUES ({version}, '{name}');"
            )
            .into_boxed_str(),
        ),
    }
}

/// The registered chain extended with extra migrations (for constructing
/// mismatched-version databases in tests).
fn chain_with(extra: &[Migration]) -> Vec<Migration> {
    let mut chain = migrations::registered().to_vec();
    chain.extend_from_slice(extra);
    chain
}

fn forced_failure() -> StateError {
    StateError::UnitOfWorkFailed {
        detail: "forced test failure".to_string(),
    }
}

const TX_PROBE_DDL: &str = "CREATE TABLE tx_probe (
    id INTEGER PRIMARY KEY,
    value TEXT NOT NULL
);";

// T1 — a brand-new database bootstraps through the migration mechanism.
#[test]
fn t01_new_database_bootstrap() {
    let tmp = TempDir::new("t01");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("fresh database bootstraps");
    drop(repo);
    assert!(tmp.db_path().is_file(), "database file must exist on disk");
}

// T2 — the expected current schema version is durably recorded.
#[test]
fn t02_expected_schema_version_recorded() {
    let tmp = TempDir::new("t02");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let expected = migrations::registered()
        .last()
        .expect("registered chain is non-empty")
        .version;
    assert_eq!(repo.schema_version().expect("version read"), expected);
    // Exactly one metadata row per applied migration, no duplicates.
    assert_eq!(
        repo.count_table_rows("state_schema_version").expect("rows"),
        i64::from(expected)
    );
}

// T3 — journal_mode is WAL.
#[test]
fn t03_wal_enabled() {
    let tmp = TempDir::new("t03");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(
        repo.pragma_string_value("journal_mode").expect("pragma"),
        "wal"
    );
}

// T4 — busy_timeout is 5000 ms.
#[test]
fn t04_busy_timeout_5000() {
    let tmp = TempDir::new("t04");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(
        repo.pragma_integer_value("busy_timeout").expect("pragma"),
        5000
    );
}

// T5 — synchronous is FULL.
#[test]
fn t05_synchronous_full() {
    let tmp = TempDir::new("t05");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(repo.pragma_integer_value("synchronous").expect("pragma"), 2);
}

// T6 — foreign_keys is ON.
#[test]
fn t06_foreign_keys_on() {
    let tmp = TempDir::new("t06");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(
        repo.pragma_integer_value("foreign_keys").expect("pragma"),
        1
    );
}

// T7 — reopening a current-version database succeeds.
#[test]
fn t07_current_version_reopen_succeeds() {
    let tmp = TempDir::new("t07");
    drop(SqliteStateRepository::open(tmp.db_path()).expect("bootstrap"));
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen succeeds");
    assert_eq!(repo.schema_version().expect("version read"), 12);
}

// T8 — bootstrap/reopen is idempotent: no duplicate metadata, no reinit.
#[test]
fn t08_reopen_idempotent() {
    let tmp = TempDir::new("t08");
    for _ in 0..3 {
        let repo = SqliteStateRepository::open(tmp.db_path()).expect("every reopen succeeds");
        assert_eq!(
            repo.count_table_rows("state_schema_version").expect("rows"),
            12,
            "one metadata row per applied migration, never duplicated by reopen"
        );
    }
}

// T9 — an existing database at a lower (older) version than supported fails
// to open, and is not silently upgraded.
#[test]
fn t09_lower_unsupported_version_fails() {
    let tmp = TempDir::new("t09");
    // Database initialized one version above the registered chain.
    let v13_chain = chain_with(&[probe_migration(13, "probe_v13")]);
    drop(
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &v13_chain)
            .expect("bootstrap at version 13"),
    );
    // Opening against a chain supporting version 14 must fail closed.
    let v14_chain = chain_with(&[
        probe_migration(13, "probe_v13"),
        probe_migration(14, "probe_v14"),
    ]);
    let error = SqliteStateRepository::open_with_migrations(tmp.db_path(), &v14_chain)
        .expect_err("older version must not be silently upgraded");
    assert!(
        matches!(
            error,
            StateError::SchemaVersionMismatch {
                found: 13,
                supported: 14
            }
        ),
        "unexpected error: {error}"
    );
    // The stored version was not altered by the failed open.
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), &v13_chain)
        .expect("database still opens with its original chain");
    assert_eq!(repo.schema_version().expect("version read"), 13);
}

// T10 — an existing database at a higher/unknown version fails to open and
// is not rewritten or downgraded.
#[test]
fn t10_higher_unsupported_version_fails() {
    let tmp = TempDir::new("t10");
    let v13_chain = chain_with(&[probe_migration(13, "probe_v13")]);
    drop(
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &v13_chain)
            .expect("bootstrap at version 13"),
    );
    let error = SqliteStateRepository::open(tmp.db_path())
        .expect_err("newer/unknown version must fail closed");
    assert!(
        matches!(
            error,
            StateError::SchemaVersionMismatch {
                found: 13,
                supported: 12
            }
        ),
        "unexpected error: {error}"
    );
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), &v13_chain)
        .expect("database still opens with its original chain");
    assert_eq!(repo.schema_version().expect("version read"), 13);
}

// T11 — a successful transaction commits all of its mutations.
#[test]
fn t11_transaction_commits_all_mutations() {
    let tmp = TempDir::new("t11");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.run_transaction(|uow| {
        uow.execute_batch(TX_PROBE_DDL)?;
        uow.execute("INSERT INTO tx_probe (value) VALUES (?1)", &[&"one"])?;
        uow.execute("INSERT INTO tx_probe (value) VALUES (?1)", &[&"two"])?;
        Ok(())
    })
    .expect("transaction commits");
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert!(repo.table_exists("tx_probe").expect("table check"));
    assert_eq!(repo.count_table_rows("tx_probe").expect("rows"), 2);
}

// T12 — a forced failure after multiple mutations rolls all of them back.
#[test]
fn t12_transaction_failure_rolls_back_all_mutations() {
    let tmp = TempDir::new("t12");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let error = repo
        .run_transaction(|uow| {
            uow.execute_batch(TX_PROBE_DDL)?;
            uow.execute("INSERT INTO tx_probe (value) VALUES (?1)", &[&"one"])?;
            uow.execute("INSERT INTO tx_probe (value) VALUES (?1)", &[&"two"])?;
            Err::<(), StateError>(forced_failure())
        })
        .expect_err("failed work surfaces its error");
    assert!(
        matches!(error, StateError::UnitOfWorkFailed { .. }),
        "unexpected error: {error}"
    );
    // Nothing from the transaction is visible on the same connection.
    assert!(!repo.table_exists("tx_probe").expect("table check"));
}

// T13 — rollback remains intact after close/reopen, and the store stays
// usable.
#[test]
fn t13_rollback_intact_after_close_reopen() {
    let tmp = TempDir::new("t13");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.run_transaction(|uow| {
        uow.execute_batch(TX_PROBE_DDL)?;
        uow.execute("INSERT INTO tx_probe (value) VALUES (?1)", &[&"one"])?;
        uow.execute("INSERT INTO tx_probe (value) VALUES (?1)", &[&"two"])?;
        Err::<(), StateError>(forced_failure())
    })
    .expect_err("failed work surfaces its error");
    drop(repo);
    // After close/reopen the rolled-back work left no partial state.
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert!(
        !repo.table_exists("tx_probe").expect("table check"),
        "rolled-back transaction must leave no trace after reopen"
    );
    // The store remains usable afterwards.
    repo.run_transaction(|uow| {
        uow.execute_batch(TX_PROBE_DDL)?;
        uow.execute("INSERT INTO tx_probe (value) VALUES (?1)", &[&"fresh"])?;
        Ok(())
    })
    .expect("store usable after rollback");
    assert_eq!(repo.count_table_rows("tx_probe").expect("rows"), 1);
}

fn seed_epoch_families(repo: &mut SqliteStateRepository, epoch: &str) {
    repo.run_transaction(|uow| {
        uow.execute_batch(&format!(
            "INSERT INTO logical_role VALUES ('role-1','project-1','RUNTIME_A2','ACTIVE',{epoch},NULL,NULL,NULL,'manifest-1',NULL,'created');
             INSERT INTO logical_role_ownership_path VALUES ('role-1',0,'src/state');
             INSERT INTO executor_binding VALUES ('binding-1','role-1','provider','model','runtime',NULL,NULL,'bound','lease',NULL,NULL,NULL);
             INSERT INTO event VALUES ('01ARZ3NDEKTSV4RRFFQ69G5FAV','project-1',NULL,'TASK_CREATED','SYSTEM',NULL,'TASK','task-1','occurred','ref','digest','corr',{epoch});
             INSERT INTO context_manifest VALUES ('manifest-1','role-1','project-1',{epoch},'created',NULL);
             INSERT INTO context_manifest_source VALUES ('manifest-1',0,'ARTIFACT_ID','artifact-1','MANDATORY','digest',NULL);
             INSERT INTO context_manifest_source_required_for VALUES ('manifest-1',0,0,'DISPATCH');
             INSERT INTO context_epoch VALUES ('project-1',{epoch},'advanced','A1_INIT',1);
             INSERT INTO context_epoch_invalidated_role VALUES ('project-1',{epoch},'role-1');
             INSERT INTO context_epoch_changed_source VALUES ('project-1',{epoch},0,'URL','https://example.invalid',NULL,NULL);
             INSERT INTO context_rehydration_attempt VALUES ('project-1','attempt-1','role-1','manifest-1',{epoch},'A1_INIT',NULL,NULL,NULL,'SYSTEM',NULL,NULL,NULL,'started','completed','FAILED','FAILED');
             INSERT INTO context_rehydration_repository_snapshot VALUES ('project-1','attempt-1',0,'repo','0123456789012345678901234567890123456789','src/lib.rs');
             INSERT INTO context_rehydration_source_evidence VALUES ('project-1','attempt-1',0,'source-1','ARTIFACT_ID','MANDATORY','identity',NULL,NULL,'sha256:v1:0000000000000000000000000000000000000000000000000000000000000000',NULL,'NOT_CHECKED',NULL,'DEFERRED',NULL,NULL,NULL);"
        ))
    })
    .expect("seed all epoch families");
}

#[test]
fn v12_explicit_migration_preserves_all_families_and_restores_foreign_keys() {
    let tmp = TempDir::new("v12-explicit");
    let mut repo =
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &migrations::registered()[..11])
            .expect("v11");
    seed_epoch_families(&mut repo, "0");
    drop(repo);

    assert!(matches!(
        SqliteStateRepository::open(tmp.db_path()).expect_err("ordinary open refuses v11"),
        StateError::SchemaVersionMismatch {
            found: 11,
            supported: 12
        }
    ));
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).expect("migrate"),
        12
    );
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("v12 open");
    assert_eq!(repo.pragma_integer_value("foreign_keys").unwrap(), 1);
    for (table, column) in [
        ("logical_role", "current_context_epoch"),
        ("event", "epoch"),
        ("context_manifest", "epoch"),
        ("context_epoch", "epoch"),
        ("context_epoch_invalidated_role", "epoch"),
        ("context_epoch_changed_source", "epoch"),
        ("context_rehydration_attempt", "context_epoch_id"),
    ] {
        let (storage, value): (String, String) = repo
            .connection()
            .query_row(
                &format!("SELECT typeof({column}), {column} FROM {table}"),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((storage.as_str(), value.as_str()), ("text", "0"));
    }
    for table in [
        "logical_role_ownership_path",
        "executor_binding",
        "context_manifest_source",
        "context_manifest_source_required_for",
        "context_rehydration_repository_snapshot",
        "context_rehydration_source_evidence",
    ] {
        assert_eq!(repo.count_table_rows(table).unwrap(), 1, "{table}");
    }
    assert!(
        repo.connection()
            .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
            .optional()
            .unwrap()
            .is_none()
    );
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).expect("v12 noop"),
        12
    );
}

#[test]
fn v12_rejects_every_noncanonical_storage_shape_in_all_seven_columns() {
    use rusqlite::types::Value;

    let tmp = TempDir::new("v12-storage-checks");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("v12");
    seed_epoch_families(&mut repo, "'0'");
    let invalid = [
        Value::Integer(-1),
        Value::Real(1.0),
        Value::Blob(vec![b'1']),
        Value::Null,
        Value::Text(String::new()),
        Value::Text("00".into()),
        Value::Text("01".into()),
        Value::Text("+1".into()),
        Value::Text("-0".into()),
        Value::Text("-1".into()),
        Value::Text("1.0".into()),
        Value::Text("1e0".into()),
        Value::Text(" 1".into()),
        Value::Text("1 ".into()),
        Value::Text("١".into()),
    ];
    for (table, column) in [
        ("logical_role", "current_context_epoch"),
        ("event", "epoch"),
        ("context_manifest", "epoch"),
        ("context_epoch", "epoch"),
        ("context_epoch_invalidated_role", "epoch"),
        ("context_epoch_changed_source", "epoch"),
        ("context_rehydration_attempt", "context_epoch_id"),
    ] {
        for value in &invalid {
            assert!(
                repo.run_transaction(|uow| {
                    uow.execute(&format!("UPDATE {table} SET {column} = ?1"), &[value])?;
                    Ok(())
                })
                .is_err(),
                "{table}.{column} accepted {value:?}"
            );
        }
    }
}

#[test]
fn v12_corrupt_legacy_and_rebuild_collision_fail_atomically() {
    for collision in [false, true] {
        let tmp = TempDir::new(if collision {
            "v12-collision"
        } else {
            "v12-corrupt"
        });
        let mut repo = SqliteStateRepository::open_with_migrations(
            tmp.db_path(),
            &migrations::registered()[..11],
        )
        .expect("v11");
        seed_epoch_families(&mut repo, "0");
        if collision {
            repo.run_transaction(|uow| uow.execute_batch("CREATE TABLE logical_role_v12 (x)"))
                .unwrap();
        } else {
            repo.run_transaction(|uow| {
                uow.execute("UPDATE event SET epoch = ?1", &[&1.5_f64])?;
                Ok(())
            })
            .unwrap();
        }
        drop(repo);
        assert!(SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).is_err());
        let repo = SqliteStateRepository::open_with_migrations(
            tmp.db_path(),
            &migrations::registered()[..11],
        )
        .expect("v11 remains authoritative");
        assert_eq!(repo.schema_version().unwrap(), 11);
        assert_eq!(repo.pragma_integer_value("foreign_keys").unwrap(), 1);
        assert_eq!(repo.count_table_rows("event").unwrap(), 1);
    }
}

#[test]
fn explicit_v12_noop_rejects_a_corrupt_full_ledger() {
    let tmp = TempDir::new("v12-ledger");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("v12");
    repo.run_transaction(|uow| {
        uow.execute(
            "UPDATE state_schema_version SET migration_name = 'wrong' WHERE version = 5",
            &[],
        )?;
        Ok(())
    })
    .unwrap();
    drop(repo);
    assert!(matches!(
        SqliteStateRepository::migrate_existing_to_current(tmp.db_path()),
        Err(StateError::MigrationLedgerCorrupt { .. })
    ));
}
