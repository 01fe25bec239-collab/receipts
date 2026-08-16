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
    /// A LogicalRole failed contract-level validation before persistence.
    LogicalRoleValidation {
        /// Which frozen constraint was violated.
        detail: String,
    },
    /// Creating a LogicalRole whose durable identity already exists.
    ///
    /// Durable role identities are never overwritten, replaced, upserted,
    /// or merged; the original row remains untouched.
    LogicalRoleAlreadyExists {
        /// The `role_id` that already exists.
        role_id: String,
    },
    /// Writing a LogicalRole failed at the storage layer.
    LogicalRoleWriteFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// A persisted LogicalRole row could not be decoded against the frozen
    /// contract.
    ///
    /// Decoding fails closed: partially decoded or contract-violating rows
    /// are never surfaced as valid roles.
    LogicalRoleDecodeFailed {
        /// What could not be decoded.
        detail: String,
    },
    /// An ExecutorBinding failed contract-level validation before
    /// persistence.
    ExecutorBindingValidation {
        /// Which frozen constraint was violated.
        detail: String,
    },
    /// Creating an ExecutorBinding whose durable identity already exists.
    ///
    /// Binding history is append-only durable evidence: an existing
    /// `binding_id` is never overwritten, replaced, upserted, merged, or
    /// deleted-and-reinserted; the original row remains untouched.
    ExecutorBindingAlreadyExists {
        /// The `binding_id` that already exists.
        binding_id: String,
    },
    /// Creating an ExecutorBinding whose `role_id` does not reference an
    /// existing persisted LogicalRole.
    ///
    /// No orphan binding may ever be persisted; durable role identity is the
    /// only valid binding target.
    ExecutorBindingRoleNotFound {
        /// The `role_id` that does not exist.
        role_id: String,
    },
    /// Writing an ExecutorBinding failed at the storage layer.
    ExecutorBindingWriteFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// A persisted ExecutorBinding row could not be decoded against the
    /// frozen contract.
    ///
    /// Decoding fails closed: unknown `release_reason` values, corrupt
    /// `rehydration_completed` values, and any other contract-violating row
    /// are never surfaced as valid bindings, and no plausible default
    /// binding is constructed.
    ExecutorBindingDecodeFailed {
        /// What could not be decoded.
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
            StateError::LogicalRoleValidation { detail } => {
                write!(f, "invalid LogicalRole: {detail}")
            }
            StateError::LogicalRoleAlreadyExists { role_id } => {
                write!(
                    f,
                    "a LogicalRole with role_id {role_id:?} already exists; durable role identities are never overwritten, replaced, or merged"
                )
            }
            StateError::LogicalRoleWriteFailed { detail } => {
                write!(f, "failed to write LogicalRole: {detail}")
            }
            StateError::LogicalRoleDecodeFailed { detail } => {
                write!(f, "failed to decode persisted LogicalRole: {detail}")
            }
            StateError::ExecutorBindingValidation { detail } => {
                write!(f, "invalid ExecutorBinding: {detail}")
            }
            StateError::ExecutorBindingAlreadyExists { binding_id } => {
                write!(
                    f,
                    "an ExecutorBinding with binding_id {binding_id:?} already exists; binding history is append-only and is never overwritten, replaced, or merged"
                )
            }
            StateError::ExecutorBindingRoleNotFound { role_id } => {
                write!(
                    f,
                    "no LogicalRole with role_id {role_id:?} exists; an ExecutorBinding may not be created for a nonexistent role"
                )
            }
            StateError::ExecutorBindingWriteFailed { detail } => {
                write!(f, "failed to write ExecutorBinding: {detail}")
            }
            StateError::ExecutorBindingDecodeFailed { detail } => {
                write!(f, "failed to decode persisted ExecutorBinding: {detail}")
            }
        }
    }
}

impl std::error::Error for StateError {}
