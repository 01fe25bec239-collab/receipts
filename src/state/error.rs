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
    /// Releasing or lease-renewing an ExecutorBinding whose durable identity
    /// does not exist.
    ///
    /// Both operations require an already persisted binding: they never
    /// fabricate, create, or implicitly materialize one, and never silently
    /// succeed.
    ExecutorBindingNotFound {
        /// The `binding_id` that does not exist.
        binding_id: String,
    },
    /// Releasing, or renewing the lease of, an ExecutorBinding whose
    /// write-once terminal release slot is already occupied.
    ///
    /// `released_at` and `release_reason` are terminal fields recorded at
    /// most once: the originally recorded release evidence is never
    /// overwritten, replaced, or merged, a repeat release is never treated
    /// as idempotent success, and a terminally released binding — including
    /// one released with `LEASE_EXPIRED` — can never be renewed or reopened.
    /// This also covers any persisted partial terminal shape (exactly one
    /// of the two fields non-NULL), which this repository never produces
    /// but must still refuse to complete or renew.
    ExecutorBindingAlreadyReleased {
        /// The `binding_id` that is already released.
        binding_id: String,
    },
    /// Writing an ExecutorBinding failed at the storage layer.
    ExecutorBindingWriteFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// Creating an ExecutorBinding for a role that already has one
    /// not-fully-released binding.
    ///
    /// At most one binding per LogicalRole may exist that is not conclusively
    /// fully released — that is, whose durable terminal pair
    /// `released_at`/`release_reason` is not both recorded. A valid unreleased
    /// binding, a renewed-but-unreleased binding, a binding whose
    /// `lease_expires_at` merely looks old, and either corrupt partial
    /// terminal shape all block creation of another binding for the same
    /// role until an authorized explicit release records the complete
    /// terminal pair. This error names no wall-clock verdict: it never claims
    /// the blocking lease is currently valid, and State never evaluates
    /// `lease_expires_at` against any clock. The blocking row is durable
    /// evidence and is never deleted, released, repaired, or replaced by
    /// conflict handling; it is distinct from
    /// [`StateError::ExecutorBindingAlreadyExists`], which keeps reporting
    /// duplicate durable `binding_id` alone.
    ExecutorBindingUnreleasedConflict {
        /// The `role_id` that already has a not-fully-released binding.
        role_id: String,
        /// The `binding_id` of the existing blocking binding.
        blocking_binding_id: String,
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
    /// An EventEnvelope failed structural validation before persistence.
    ///
    /// Validation accepts each supplied structural value byte-for-byte
    /// unchanged or rejects the append; it never transforms, repairs, or
    /// redacts a value into a different persisted value.
    EventValidation {
        /// Which frozen structural constraint was violated.
        detail: String,
    },
    /// Appending an EventEnvelope whose durable identity already exists.
    ///
    /// Events are immutable append-only evidence: an existing `event_id` is
    /// never overwritten, replaced, upserted, merged, or
    /// deleted-and-reinserted; the original row remains untouched.
    EventAlreadyExists {
        /// The `event_id` that already exists.
        event_id: String,
    },
    /// Writing an EventEnvelope failed at the storage layer.
    EventWriteFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// A persisted EventEnvelope row could not be decoded against the
    /// frozen contract.
    ///
    /// Decoding fails closed: unknown `event_type`, `actor_kind`, or
    /// `subject_kind` values, a negative `epoch`, and any other
    /// contract-violating row are never surfaced as valid events, never
    /// mapped to an `UNKNOWN` variant, and never repaired into a plausible
    /// default envelope.
    EventDecodeFailed {
        /// What could not be decoded.
        detail: String,
    },
    /// A ContextManifest failed contract-level validation before
    /// persistence.
    ///
    /// Validation accepts each supplied structural value byte-for-byte
    /// unchanged or rejects the create; it never normalizes, repairs,
    /// trims, or rewrites a value into a different persisted value.
    ContextManifestValidation {
        /// Which frozen structural constraint was violated.
        detail: String,
    },
    /// Creating a ContextManifest whose durable identity already exists.
    ///
    /// Manifests are immutable at this slice: an existing `manifest_id` is
    /// never overwritten, replaced, upserted, merged, or
    /// deleted-and-reinserted; the original row remains untouched.
    ContextManifestAlreadyExists {
        /// The `manifest_id` that already exists.
        manifest_id: String,
    },
    /// Creating a ContextManifest whose `role_id` does not reference an
    /// existing persisted LogicalRole.
    ///
    /// No orphan manifest may ever be persisted; a durable role is the
    /// only valid manifest owner.
    ContextManifestRoleNotFound {
        /// The `role_id` that does not exist.
        role_id: String,
    },
    /// Creating a ContextManifest for a role that already owns one.
    ///
    /// Each durable role has exactly one authoritative ContextManifest, so
    /// at most one manifest per `role_id` may persist. The existing
    /// manifest is never replaced, deleted, updated, or silently chosen
    /// over: the refusal names the existing manifest instead. This is
    /// deliberately distinct from
    /// [`StateError::ContextManifestAlreadyExists`], which keeps reporting
    /// duplicate durable `manifest_id` alone.
    ContextManifestRoleAlreadyHasManifest {
        /// The `role_id` that already owns a manifest.
        role_id: String,
        /// The `manifest_id` of the existing authoritative manifest.
        existing_manifest_id: String,
    },
    /// Writing a ContextManifest failed at the storage layer.
    ContextManifestWriteFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// A persisted ContextManifest graph could not be decoded against the
    /// frozen contract.
    ///
    /// Decoding fails closed: unknown `ref_type`, `source_class`, or
    /// `required_for` values, empty structural strings, a negative epoch,
    /// a zero-source manifest, ordinal gaps, and any other
    /// contract-violating row are never surfaced as a valid manifest,
    /// never mapped to an `UNKNOWN` variant, never silently dropped, and
    /// never repaired into a plausible default manifest.
    ContextManifestDecodeFailed {
        /// What could not be decoded.
        detail: String,
    },
    /// A ContextEpoch failed contract-level validation before persistence.
    ///
    /// Validation accepts each supplied structural value byte-for-byte
    /// unchanged or rejects the append; it never normalizes, repairs,
    /// trims, or rewrites a value into a different persisted value.
    ContextEpochValidation {
        /// Which frozen structural constraint was violated.
        detail: String,
    },
    /// Appending a ContextEpoch whose `(project_id, epoch)` already
    /// exists.
    ///
    /// Epoch history is immutable append-only evidence: an existing
    /// `(project_id, epoch)` record is never overwritten, replaced,
    /// upserted, merged, or deleted-and-reinserted; the original record
    /// remains untouched. Uniqueness is per `(project_id, epoch)`, never
    /// database-global: the same epoch number may legitimately exist for
    /// different projects.
    ContextEpochAlreadyExists {
        /// The `project_id` of the duplicate record.
        project_id: String,
        /// The `epoch` number of the duplicate record.
        epoch: i64,
    },
    /// Writing a ContextEpoch failed at the storage layer.
    ContextEpochWriteFailed {
        /// Underlying driver detail.
        detail: String,
    },
    /// A persisted ContextEpoch row could not be decoded against the
    /// frozen contract.
    ///
    /// Decoding fails closed: an unknown `trigger` value, an empty or
    /// overlong `project_id`, a negative `epoch`, an empty `advanced_at`,
    /// and any other contract-violating row are never surfaced as a valid
    /// epoch record, never mapped to an `UNKNOWN`/fallback variant, never
    /// clamped or defaulted, and never silently skipped in favor of
    /// another row.
    ContextEpochDecodeFailed {
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
            StateError::ExecutorBindingNotFound { binding_id } => {
                write!(
                    f,
                    "no ExecutorBinding with binding_id {binding_id:?} exists; release and lease renewal require an already persisted binding and never fabricate one"
                )
            }
            StateError::ExecutorBindingAlreadyReleased { binding_id } => {
                write!(
                    f,
                    "ExecutorBinding with binding_id {binding_id:?} is already released; released_at/release_reason are write-once terminal evidence that are never overwritten, and a released binding cannot be renewed"
                )
            }
            StateError::ExecutorBindingWriteFailed { detail } => {
                write!(f, "failed to write ExecutorBinding: {detail}")
            }
            StateError::ExecutorBindingUnreleasedConflict {
                role_id,
                blocking_binding_id,
            } => {
                write!(
                    f,
                    "ExecutorBinding with binding_id {blocking_binding_id:?} for role_id {role_id:?} is not fully released; a role may have at most one not-fully-released binding, so the existing binding must be explicitly released before another binding for this role may be created (no wall-clock lease verdict is implied)"
                )
            }
            StateError::ExecutorBindingDecodeFailed { detail } => {
                write!(f, "failed to decode persisted ExecutorBinding: {detail}")
            }
            StateError::EventValidation { detail } => {
                write!(f, "invalid EventEnvelope: {detail}")
            }
            StateError::EventAlreadyExists { event_id } => {
                write!(
                    f,
                    "an event with event_id {event_id:?} already exists; the event log is append-only and is never overwritten, replaced, or merged"
                )
            }
            StateError::EventWriteFailed { detail } => {
                write!(f, "failed to append event: {detail}")
            }
            StateError::EventDecodeFailed { detail } => {
                write!(f, "failed to decode persisted event: {detail}")
            }
            StateError::ContextManifestValidation { detail } => {
                write!(f, "invalid ContextManifest: {detail}")
            }
            StateError::ContextManifestAlreadyExists { manifest_id } => {
                write!(
                    f,
                    "a ContextManifest with manifest_id {manifest_id:?} already exists; manifests are immutable at this slice and are never overwritten, replaced, or merged"
                )
            }
            StateError::ContextManifestRoleNotFound { role_id } => {
                write!(
                    f,
                    "no LogicalRole with role_id {role_id:?} exists; a ContextManifest may not be created for a nonexistent role"
                )
            }
            StateError::ContextManifestRoleAlreadyHasManifest {
                role_id,
                existing_manifest_id,
            } => {
                write!(
                    f,
                    "role_id {role_id:?} already owns authoritative manifest {existing_manifest_id:?}; a role may have at most one manifest, and the existing manifest is never replaced, updated, or deleted by conflict handling"
                )
            }
            StateError::ContextManifestWriteFailed { detail } => {
                write!(f, "failed to write ContextManifest: {detail}")
            }
            StateError::ContextManifestDecodeFailed { detail } => {
                write!(f, "failed to decode persisted ContextManifest: {detail}")
            }
            StateError::ContextEpochValidation { detail } => {
                write!(f, "invalid ContextEpoch: {detail}")
            }
            StateError::ContextEpochAlreadyExists { project_id, epoch } => {
                write!(
                    f,
                    "a ContextEpoch for project_id {project_id:?} epoch {epoch} already exists; epoch history is immutable and append-only and is never overwritten, replaced, or merged"
                )
            }
            StateError::ContextEpochWriteFailed { detail } => {
                write!(f, "failed to write ContextEpoch: {detail}")
            }
            StateError::ContextEpochDecodeFailed { detail } => {
                write!(f, "failed to decode persisted ContextEpoch: {detail}")
            }
        }
    }
}

impl std::error::Error for StateError {}
