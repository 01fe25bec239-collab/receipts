//! Deterministic tests for the State repository foundation (T1–T13).
//!
//! All tests use real temporary SQLite database files under the system
//! temporary directory (never inside the repository) and clean up after
//! themselves. Test-only SQL runs through crate-private helpers and is never
//! exposed through the public repository API.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::StateError;
use crate::migrations::{self, Migration};
use crate::repository::SqliteStateRepository;

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
    assert_eq!(repo.schema_version().expect("version read"), 8);
}

// T8 — bootstrap/reopen is idempotent: no duplicate metadata, no reinit.
#[test]
fn t08_reopen_idempotent() {
    let tmp = TempDir::new("t08");
    for _ in 0..3 {
        let repo = SqliteStateRepository::open(tmp.db_path()).expect("every reopen succeeds");
        assert_eq!(
            repo.count_table_rows("state_schema_version").expect("rows"),
            8,
            "one metadata row per applied migration, never duplicated by reopen"
        );
    }
}

// T9 — an existing database at a lower (older) version than supported fails
// to open, and is not silently upgraded.
#[test]
fn t09_lower_unsupported_version_fails() {
    let tmp = TempDir::new("t09");
    // Database initialized at version 9 by the registered chain plus one.
    let v9_chain = chain_with(&[probe_migration(9, "probe_v9")]);
    drop(
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &v9_chain)
            .expect("bootstrap at version 9"),
    );
    // Opening against a chain supporting version 10 must fail closed.
    let v10_chain = chain_with(&[
        probe_migration(9, "probe_v9"),
        probe_migration(10, "probe_v10"),
    ]);
    let error = SqliteStateRepository::open_with_migrations(tmp.db_path(), &v10_chain)
        .expect_err("older version must not be silently upgraded");
    assert!(
        matches!(
            error,
            StateError::SchemaVersionMismatch {
                found: 9,
                supported: 10
            }
        ),
        "unexpected error: {error}"
    );
    // The stored version was not altered by the failed open.
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), &v9_chain)
        .expect("database still opens with its original chain");
    assert_eq!(repo.schema_version().expect("version read"), 9);
}

// T10 — an existing database at a higher/unknown version fails to open and
// is not rewritten or downgraded.
#[test]
fn t10_higher_unsupported_version_fails() {
    let tmp = TempDir::new("t10");
    let v9_chain = chain_with(&[probe_migration(9, "probe_v9")]);
    drop(
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &v9_chain)
            .expect("bootstrap at version 9"),
    );
    let error = SqliteStateRepository::open(tmp.db_path())
        .expect_err("newer/unknown version must fail closed");
    assert!(
        matches!(
            error,
            StateError::SchemaVersionMismatch {
                found: 9,
                supported: 8
            }
        ),
        "unexpected error: {error}"
    );
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), &v9_chain)
        .expect("database still opens with its original chain");
    assert_eq!(repo.schema_version().expect("version read"), 9);
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
