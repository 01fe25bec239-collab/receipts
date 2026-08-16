//! SQLite-backed State repository boundary.
//!
//! [`SqliteStateRepository`] owns a single local per-project SQLite database.
//! All SQLite specifics (driver types, PRAGMA statements, migration SQL,
//! internal queries) stay inside this crate: the public surface exposes no
//! `rusqlite` type, no raw SQL string, and no arbitrary-SQL execution path.
//! Mutation flows only through [`SqliteStateRepository::run_transaction`],
//! which the orchestrator core (the single authoritative State writer in the
//! frozen architecture) is the intended future caller of.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, Transaction};

#[cfg(test)]
use rusqlite::ToSql;

use crate::error::StateError;
use crate::migrations::{self, Migration};

/// Required `busy_timeout` in milliseconds (BUILD-A1 bootstrap decision).
const REQUIRED_BUSY_TIMEOUT_MS: i64 = 5000;

/// SQLite's numeric `synchronous` value for `FULL`.
const SYNCHRONOUS_FULL: i64 = 2;

/// SQLite's numeric boolean for `foreign_keys = ON`.
const FOREIGN_KEYS_ON: i64 = 1;

/// The durable schema-version table owned by migration 0001.
const SCHEMA_VERSION_TABLE: &str = "state_schema_version";

/// A local, per-project SQLite State store.
///
/// Opening applies the mandatory connection configuration and reconciles the
/// durably recorded schema version against the supported migration chain:
///
/// * an uninitialized database bootstraps from version `0` forward through
///   the registered chain;
/// * a database already at the supported version opens without reapplying
///   anything;
/// * a database at any other version (older *or* newer/unknown) fails
///   closed; ordinary open never silently upgrades or downgrades.
pub struct SqliteStateRepository {
    conn: Connection,
}

impl fmt::Debug for SqliteStateRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Connection internals are deliberately not surfaced.
        f.write_str("SqliteStateRepository")
    }
}

impl SqliteStateRepository {
    /// Opens (creating if necessary) the local SQLite database at `path` and
    /// reconciles its schema version against the registered migration chain.
    ///
    /// The parent directory of `path` must already exist. Every required
    /// PRAGMA is applied and verified; any failure is returned as an
    /// explicit error.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StateError> {
        Self::open_with_migrations(path, migrations::registered())
    }

    /// Version-reconciliation core of [`Self::open`], parameterized by the
    /// migration chain so tests can construct supported/unsupported version
    /// combinations. Not part of the public surface.
    pub(crate) fn open_with_migrations(
        path: impl AsRef<Path>,
        chain: &[Migration],
    ) -> Result<Self, StateError> {
        migrations::validate_chain(chain)?;
        let supported = Self::supported_version(chain)?;
        let conn = Connection::open(path.as_ref()).map_err(|e| StateError::OpenFailed {
            detail: e.to_string(),
        })?;
        configure_connection(&conn)?;
        let repo = Self { conn };
        match repo.read_schema_version()? {
            found if found == supported => Ok(repo),
            0 => {
                let mut repo = repo;
                bootstrap(&mut repo.conn, chain)?;
                Ok(repo)
            }
            found => Err(StateError::SchemaVersionMismatch { found, supported }),
        }
    }

    /// The schema version durably recorded in this database.
    pub fn schema_version(&self) -> Result<u32, StateError> {
        self.read_schema_version()
    }

    /// Crate-private read access to the underlying connection for
    /// repository-internal domain reads (e.g. LogicalRole lookups). The
    /// connection never crosses the crate boundary.
    pub(crate) fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Runs `work` inside one SQLite transaction.
    ///
    /// This is the State-layer primitive for the future invariant
    /// "one orchestration action = one SQLite transaction". If `work`
    /// succeeds, its mutations commit atomically; if `work` fails, the
    /// entire transaction rolls back and no partial state is persisted.
    /// Commit, begin, and rollback failures are surfaced as explicit errors.
    ///
    /// `work` receives an opaque [`UnitOfWork`] handle. The handle exposes
    /// no public capability; repository-internal operations and colocated
    /// tests use crate-private methods.
    pub fn run_transaction<T>(
        &mut self,
        work: impl FnOnce(&mut UnitOfWork<'_>) -> Result<T, StateError>,
    ) -> Result<T, StateError> {
        let tx = self
            .conn
            .transaction()
            .map_err(|e| StateError::TransactionBeginFailed {
                detail: e.to_string(),
            })?;
        let mut unit = UnitOfWork { tx };
        match work(&mut unit) {
            Ok(value) => unit
                .tx
                .commit()
                .map_err(|e| StateError::TransactionCommitFailed {
                    detail: e.to_string(),
                })
                .map(|_| value),
            Err(work_error) => {
                unit.tx
                    .rollback()
                    .map_err(|e| StateError::TransactionRollbackFailed {
                        detail: e.to_string(),
                    })?;
                Err(work_error)
            }
        }
    }

    /// Reads the current string value of a PRAGMA on this connection.
    ///
    /// Crate-private test/inspection support; `name` is always a
    /// repository-internal literal, never caller- or model-supplied data.
    #[cfg(test)]
    pub(crate) fn pragma_string_value(&self, name: &str) -> Result<String, StateError> {
        self.conn
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .map_err(|e| StateError::InternalQueryFailed {
                detail: e.to_string(),
            })
    }

    /// Reads the current integer value of a PRAGMA on this connection.
    ///
    /// Crate-private test/inspection support; `name` is always a
    /// repository-internal literal, never caller- or model-supplied data.
    #[cfg(test)]
    pub(crate) fn pragma_integer_value(&self, name: &str) -> Result<i64, StateError> {
        self.conn
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .map_err(|e| StateError::InternalQueryFailed {
                detail: e.to_string(),
            })
    }

    /// Reports whether an internal table exists. Crate-private
    /// test/inspection support; `table` is always a repository-internal or
    /// test-literal identifier.
    pub(crate) fn table_exists(&self, table: &str) -> Result<bool, StateError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .map_err(|e| StateError::InternalQueryFailed {
                detail: e.to_string(),
            })?;
        Ok(count > 0)
    }

    /// Counts rows in an internal table. Crate-private test/inspection
    /// support; `table` is always a repository-internal or test-literal
    /// identifier.
    #[cfg(test)]
    pub(crate) fn count_table_rows(&self, table: &str) -> Result<i64, StateError> {
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(|e| StateError::InternalQueryFailed {
                detail: e.to_string(),
            })
    }

    fn supported_version(chain: &[Migration]) -> Result<u32, StateError> {
        chain
            .last()
            .map(|m| m.version)
            .ok_or_else(|| StateError::MigrationChainInvalid {
                detail: "the migration chain is empty".to_string(),
            })
    }

    fn read_schema_version(&self) -> Result<u32, StateError> {
        if !self.table_exists(SCHEMA_VERSION_TABLE)? {
            return Ok(0);
        }
        let version: i64 = self
            .conn
            .query_row(
                &format!("SELECT COALESCE(MAX(version), 0) FROM {SCHEMA_VERSION_TABLE}"),
                [],
                |row| row.get(0),
            )
            .map_err(|e| StateError::SchemaVersionReadFailed {
                detail: e.to_string(),
            })?;
        u32::try_from(version).map_err(|_| StateError::SchemaVersionReadFailed {
            detail: format!("recorded schema version {version} is negative"),
        })
    }
}

/// Applies the mandatory connection configuration required by the BUILD-A1
/// bootstrap decision and verifies every value by reading it back.
///
/// Any PRAGMA that cannot be applied or verified fails closed.
fn configure_connection(conn: &Connection) -> Result<(), StateError> {
    let journal_mode: String = conn
        .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
        .map_err(|e| StateError::PragmaNotApplied {
            name: "journal_mode",
            expected: "WAL".to_string(),
            observed: e.to_string(),
        })?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        return Err(StateError::PragmaVerificationFailed {
            name: "journal_mode",
            expected: "wal".to_string(),
            observed: journal_mode,
        });
    }

    conn.busy_timeout(Duration::from_millis(REQUIRED_BUSY_TIMEOUT_MS as u64))
        .map_err(|e| StateError::PragmaNotApplied {
            name: "busy_timeout",
            expected: REQUIRED_BUSY_TIMEOUT_MS.to_string(),
            observed: e.to_string(),
        })?;
    let busy_timeout = read_pragma_integer(conn, "busy_timeout")?;
    if busy_timeout != REQUIRED_BUSY_TIMEOUT_MS {
        return Err(StateError::PragmaVerificationFailed {
            name: "busy_timeout",
            expected: REQUIRED_BUSY_TIMEOUT_MS.to_string(),
            observed: busy_timeout.to_string(),
        });
    }

    conn.execute_batch("PRAGMA synchronous = FULL;")
        .map_err(|e| StateError::PragmaNotApplied {
            name: "synchronous",
            expected: "FULL".to_string(),
            observed: e.to_string(),
        })?;
    let synchronous = read_pragma_integer(conn, "synchronous")?;
    if synchronous != SYNCHRONOUS_FULL {
        return Err(StateError::PragmaVerificationFailed {
            name: "synchronous",
            expected: format!("FULL ({SYNCHRONOUS_FULL})"),
            observed: synchronous.to_string(),
        });
    }

    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| StateError::PragmaNotApplied {
            name: "foreign_keys",
            expected: "ON".to_string(),
            observed: e.to_string(),
        })?;
    let foreign_keys = read_pragma_integer(conn, "foreign_keys")?;
    if foreign_keys != FOREIGN_KEYS_ON {
        return Err(StateError::PragmaVerificationFailed {
            name: "foreign_keys",
            expected: format!("ON ({FOREIGN_KEYS_ON})"),
            observed: foreign_keys.to_string(),
        });
    }

    Ok(())
}

fn read_pragma_integer(conn: &Connection, name: &str) -> Result<i64, StateError> {
    conn.query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
        .map_err(|e| StateError::InternalQueryFailed {
            detail: e.to_string(),
        })
}

/// Applies the migration chain to an uninitialized database, one atomic
/// transaction per migration, verifying the recorded version after each.
///
/// A failing migration leaves no partially applied state: its transaction is
/// never committed.
fn bootstrap(conn: &mut Connection, chain: &[Migration]) -> Result<(), StateError> {
    for migration in chain {
        let tx = conn
            .transaction()
            .map_err(|e| StateError::TransactionBeginFailed {
                detail: e.to_string(),
            })?;
        tx.execute_batch(migration.sql)
            .map_err(|e| StateError::MigrationFailed {
                version: migration.version,
                name: migration.name,
                detail: e.to_string(),
            })?;
        let recorded: i64 = tx
            .query_row(
                &format!("SELECT COALESCE(MAX(version), 0) FROM {SCHEMA_VERSION_TABLE}"),
                [],
                |row| row.get(0),
            )
            .map_err(|e| StateError::MigrationFailed {
                version: migration.version,
                name: migration.name,
                detail: e.to_string(),
            })?;
        if recorded != i64::from(migration.version) {
            return Err(StateError::MigrationFailed {
                version: migration.version,
                name: migration.name,
                detail: format!(
                    "expected recorded version {} after applying, found {recorded}",
                    migration.version
                ),
            });
        }
        tx.commit().map_err(|e| StateError::MigrationFailed {
            version: migration.version,
            name: migration.name,
            detail: e.to_string(),
        })?;
    }
    Ok(())
}

/// An open transaction handed to a unit of work.
///
/// The handle is deliberately opaque: it exposes no public method, so no
/// caller outside this crate can execute SQL through it. Repository-internal
/// operations and colocated tests use crate-private methods.
pub struct UnitOfWork<'a> {
    tx: Transaction<'a>,
}

impl UnitOfWork<'_> {
    /// Crate-private access to the open transaction for repository-internal
    /// domain writes (e.g. LogicalRole insertion).
    pub(crate) fn tx(&self) -> &Transaction<'_> {
        &self.tx
    }

    /// Executes one statement with bound parameters. Crate-private;
    /// repository-internal and test-support only.
    #[cfg(test)]
    pub(crate) fn execute(&self, sql: &str, params: &[&dyn ToSql]) -> Result<usize, StateError> {
        self.tx
            .execute(sql, params)
            .map_err(|e| StateError::InternalQueryFailed {
                detail: e.to_string(),
            })
    }

    /// Executes a repository-internal statement batch. Crate-private;
    /// repository-internal and test-support only.
    #[cfg(test)]
    pub(crate) fn execute_batch(&self, sql: &str) -> Result<(), StateError> {
        self.tx
            .execute_batch(sql)
            .map_err(|e| StateError::InternalQueryFailed {
                detail: e.to_string(),
            })
    }
}
