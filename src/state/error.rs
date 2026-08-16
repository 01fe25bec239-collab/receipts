//! Fail-closed error surface for the State repository foundation.

use std::fmt;

/// Errors produced by the State repository foundation.
///
/// Every failure mode of opening, configuring, migrating, or transacting
/// against the store surfaces as an explicit error; the repository never
/// silently pretends success.
#[derive(Debug)]
#[non_exhaustive]
pub enum StateError {
    /// The SQLite database file could not be opened or created.
    OpenFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// A required PRAGMA could not be applied.
    PragmaNotApplied {
        /// PRAGMA name, e.g. `journal_mode`.
        name: &'static str,
        /// The required value.
        expected: String,
        /// Driver-reported failure detail.
        observed: String,
    },
    /// A required PRAGMA was applied but read back with an unexpected value.
    PragmaVerificationFailed {
        /// PRAGMA name, e.g. `journal_mode`.
        name: &'static str,
        /// The required value.
        expected: String,
        /// The value actually observed on the connection.
        observed: String,
    },
    /// The durably recorded schema version could not be read.
    SchemaVersionReadFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// The database records a schema version this build does not support.
    ///
    /// Covers both older (would require upgrade) and newer/unknown (would
    /// require downgrade) versions; ordinary open refuses both.
    SchemaVersionMismatch {
        /// Version durably recorded in the database.
        found: u32,
        /// Version supported by this build.
        supported: u32,
    },
    /// The registered migration chain is not a valid forward-only chain.
    MigrationChainInvalid {
        /// What is wrong with the chain.
        detail: String,
    },
    /// A migration failed to apply or verify.
    MigrationFailed {
        /// Version of the failing migration.
        version: u32,
        /// Name of the failing migration.
        name: &'static str,
        /// Underlying driver detail.
        detail: String,
    },
    /// A transaction could not be begun.
    TransactionBeginFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// A transaction could not be committed.
    TransactionCommitFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// A transaction could not be rolled back after a failed unit of work.
    TransactionRollbackFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// A unit of work reported an application-level failure and was rolled
    /// back.
    UnitOfWorkFailed {
        /// Application-level failure detail.
        detail: String,
    },
    /// An internal repository read failed.
    InternalQueryFailed {
        /// Underlying driver detail.
        detail: String,
    },
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::OpenFailed { detail } => {
                write!(f, "failed to open the state database: {detail}")
            }
            StateError::PragmaNotApplied {
                name,
                expected,
                observed,
            } => {
                write!(
                    f,
                    "required PRAGMA {name} = {expected} could not be applied: {observed}"
                )
            }
            StateError::PragmaVerificationFailed {
                name,
                expected,
                observed,
            } => {
                write!(
                    f,
                    "required PRAGMA {name} = {expected} was not in effect after application (observed: {observed})"
                )
            }
            StateError::SchemaVersionReadFailed { detail } => {
                write!(f, "failed to read the recorded schema version: {detail}")
            }
            StateError::SchemaVersionMismatch { found, supported } => {
                write!(
                    f,
                    "recorded schema version {found} is not supported by this build (supported: {supported}); refusing to alter the database"
                )
            }
            StateError::MigrationChainInvalid { detail } => {
                write!(f, "invalid migration chain: {detail}")
            }
            StateError::MigrationFailed {
                version,
                name,
                detail,
            } => {
                write!(f, "migration {version} ({name}) failed: {detail}")
            }
            StateError::TransactionBeginFailed { detail } => {
                write!(f, "failed to begin a transaction: {detail}")
            }
            StateError::TransactionCommitFailed { detail } => {
                write!(f, "failed to commit a transaction: {detail}")
            }
            StateError::TransactionRollbackFailed { detail } => {
                write!(f, "failed to roll back a failed transaction: {detail}")
            }
            StateError::UnitOfWorkFailed { detail } => {
                write!(f, "unit of work failed and was rolled back: {detail}")
            }
            StateError::InternalQueryFailed { detail } => {
                write!(f, "internal repository query failed: {detail}")
            }
        }
    }
}

impl std::error::Error for StateError {}
