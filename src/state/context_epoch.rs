//! Durable ContextEpoch history and invalidation persistence (migrations
//! 0007–0008 and 0011), including ordered changed-source capture and
//! transactional advancement.
//!
//! [`ContextEpoch`] is the immutable, project-scoped history record for a
//! context-epoch transition: which project reached which epoch number, the
//! opaque timestamp supplied for that transition, and the frozen
//! closed-set trigger category that caused it. One row per
//! `(project_id, epoch)`: duplicates are refused, records are never
//! updated, replaced, or deleted, and there is no mutable `current_epoch`
//! pointer — the latest epoch is always a derived query over the history
//! (highest numeric `epoch`), never a duplicated fact.
//!
//! Scope of this slice: append + exact read + latest read, plus exactly
//! transactional advancement through [`SqliteStateRepository::advance_context_epoch`]
//! and [`SqliteStateRepository::advance_context_epoch_with_changed_sources`].
//! State stores epoch
//! records the trusted core boundary has already constructed; it never
//! decides that an epoch should advance — the advancement operation is
//! invoked only after trusted core/orchestration logic has already made
//! that decision, and the caller supplies the trigger as closed typed
//! data. Advancement derives the next epoch strictly inside one
//! transaction from the persisted history (no history → epoch `0`;
//! existing history → `max(epoch) + 1` with checked arithmetic, failing
//! closed at `i64::MAX`), inserts exactly that derived record in the same
//! transaction, and returns the committed record. A conflicting insert is
//! a failure, never a retry with a different number. There is still
//! deliberately no `increment`, `next_epoch`, `peek`, `reserve`,
//! `allocate`, `set_current_epoch`, `invalidate`, `reconcile`,
//! monotonic-sequencing enforcement, or contiguity rule: structurally
//! valid out-of-order explicit history remains storable through
//! [`SqliteStateRepository::append_context_epoch`], because explicit
//! immutable append and authoritative advancement are distinct
//! capabilities. The trigger enum is persisted metadata only: State
//! implements none of the behavior any trigger name represents.
//!
//! This module loads no source contents and computes or compares no
//! digests. The caller supplies ordered changed sources and the complete
//! invalidated-role set; State only validates and persists it atomically
//! with the new epoch. It mutates no ContextManifest, LogicalRole, or
//! ExecutorBinding; and it emits no events — `CONTEXT_EPOCH_ADVANCED` production belongs to the
//! later authorized orchestration action. `advanced_at` is an opaque
//! contract timestamp string stored exactly as supplied: State never
//! calls a clock, parses, normalizes, or compares timestamps, and never
//! uses timestamp or insertion order to determine the latest epoch.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};

use crate::epoch_value::StateEpochValueV1;
use crate::error::StateError;
use crate::repository::{SqliteStateRepository, UnitOfWork};

/// Maximum allowed length, in Unicode scalar values, of the constrained
/// ContextEpoch identifier (accepted State convention: 200).
pub const MAX_IDENTIFIER_LENGTH: usize = 200;

/// The trigger category that caused a context-epoch transition (frozen
/// closed enum of exactly the fifteen rehydration-architecture triggers).
///
/// There is no `UNKNOWN`, `OTHER`, `CUSTOM`, `MANUAL`, `TIMER`, or
/// fallback variant: any value outside the fifteen frozen triggers fails
/// closed at the decode boundary and is rejected by the storage backstop.
/// This slice persists the category only; the behavior a trigger name
/// represents (host-switch detection, compaction, security escalation,
/// and so on) belongs to later orchestration slices and is never
/// implemented or wired here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextEpochTrigger {
    /// `A1_INIT` — RUNTIME-A1 initialization.
    A1Init,
    /// `A2_INIT` — RUNTIME-A2 initialization.
    A2Init,
    /// `MODEL_REPLACEMENT` — the bound model was replaced.
    ModelReplacement,
    /// `PROVIDER_REPLACEMENT` — the provider was replaced.
    ProviderReplacement,
    /// `HOST_SWITCH` — execution switched host. Metadata only here: State
    /// detects no host change.
    HostSwitch,
    /// `CONTEXT_COMPACTION` — host context compaction. Metadata only
    /// here: State detects and summarizes nothing.
    ContextCompaction,
    /// `ARCHITECTURE_CHANGE` — an authoritative architecture source
    /// changed. Authorizes no digest comparison in this slice.
    ArchitectureChange,
    /// `CONTRACT_CHANGE` — an authoritative contract changed. Authorizes
    /// no digest comparison in this slice.
    ContractChange,
    /// `NEW_WAVE` — a new implementation wave began.
    NewWave,
    /// `TASK_THRESHOLD` — the configurable completed-task threshold was
    /// reached.
    TaskThreshold,
    /// `SERIOUS_A4_REJECTION` — a serious architecture/security review
    /// rejection. Metadata only here: State inspects no reviews.
    SeriousA4Rejection,
    /// `SECURITY_ESCALATION` — a security escalation occurred.
    SecurityEscalation,
    /// `BEFORE_A2_INTEGRATION` — immediately before A2 integration.
    BeforeA2Integration,
    /// `BEFORE_A1_INTEGRATION` — immediately before A1 integration.
    BeforeA1Integration,
    /// `BEFORE_GOAL_COMPLETE` — immediately before declaring the goal
    /// complete. Metadata only here: State evaluates no goal completion.
    BeforeGoalComplete,
}

impl ContextEpochTrigger {
    /// Every frozen trigger, in contract order.
    pub const ALL: [ContextEpochTrigger; 15] = [
        ContextEpochTrigger::A1Init,
        ContextEpochTrigger::A2Init,
        ContextEpochTrigger::ModelReplacement,
        ContextEpochTrigger::ProviderReplacement,
        ContextEpochTrigger::HostSwitch,
        ContextEpochTrigger::ContextCompaction,
        ContextEpochTrigger::ArchitectureChange,
        ContextEpochTrigger::ContractChange,
        ContextEpochTrigger::NewWave,
        ContextEpochTrigger::TaskThreshold,
        ContextEpochTrigger::SeriousA4Rejection,
        ContextEpochTrigger::SecurityEscalation,
        ContextEpochTrigger::BeforeA2Integration,
        ContextEpochTrigger::BeforeA1Integration,
        ContextEpochTrigger::BeforeGoalComplete,
    ];

    /// The durable storage representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            ContextEpochTrigger::A1Init => "A1_INIT",
            ContextEpochTrigger::A2Init => "A2_INIT",
            ContextEpochTrigger::ModelReplacement => "MODEL_REPLACEMENT",
            ContextEpochTrigger::ProviderReplacement => "PROVIDER_REPLACEMENT",
            ContextEpochTrigger::HostSwitch => "HOST_SWITCH",
            ContextEpochTrigger::ContextCompaction => "CONTEXT_COMPACTION",
            ContextEpochTrigger::ArchitectureChange => "ARCHITECTURE_CHANGE",
            ContextEpochTrigger::ContractChange => "CONTRACT_CHANGE",
            ContextEpochTrigger::NewWave => "NEW_WAVE",
            ContextEpochTrigger::TaskThreshold => "TASK_THRESHOLD",
            ContextEpochTrigger::SeriousA4Rejection => "SERIOUS_A4_REJECTION",
            ContextEpochTrigger::SecurityEscalation => "SECURITY_ESCALATION",
            ContextEpochTrigger::BeforeA2Integration => "BEFORE_A2_INTEGRATION",
            ContextEpochTrigger::BeforeA1Integration => "BEFORE_A1_INTEGRATION",
            ContextEpochTrigger::BeforeGoalComplete => "BEFORE_GOAL_COMPLETE",
        }
    }

    /// Decodes the durable representation, failing closed on anything
    /// other than the fifteen frozen triggers.
    pub(crate) fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "A1_INIT" => Ok(ContextEpochTrigger::A1Init),
            "A2_INIT" => Ok(ContextEpochTrigger::A2Init),
            "MODEL_REPLACEMENT" => Ok(ContextEpochTrigger::ModelReplacement),
            "PROVIDER_REPLACEMENT" => Ok(ContextEpochTrigger::ProviderReplacement),
            "HOST_SWITCH" => Ok(ContextEpochTrigger::HostSwitch),
            "CONTEXT_COMPACTION" => Ok(ContextEpochTrigger::ContextCompaction),
            "ARCHITECTURE_CHANGE" => Ok(ContextEpochTrigger::ArchitectureChange),
            "CONTRACT_CHANGE" => Ok(ContextEpochTrigger::ContractChange),
            "NEW_WAVE" => Ok(ContextEpochTrigger::NewWave),
            "TASK_THRESHOLD" => Ok(ContextEpochTrigger::TaskThreshold),
            "SERIOUS_A4_REJECTION" => Ok(ContextEpochTrigger::SeriousA4Rejection),
            "SECURITY_ESCALATION" => Ok(ContextEpochTrigger::SecurityEscalation),
            "BEFORE_A2_INTEGRATION" => Ok(ContextEpochTrigger::BeforeA2Integration),
            "BEFORE_A1_INTEGRATION" => Ok(ContextEpochTrigger::BeforeA1Integration),
            "BEFORE_GOAL_COMPLETE" => Ok(ContextEpochTrigger::BeforeGoalComplete),
            other => Err(StateError::ContextEpochDecodeFailed {
                detail: format!(
                    "unknown trigger {other:?}: only the fifteen frozen rehydration triggers are representable"
                ),
            }),
        }
    }
}

/// One immutable project-scoped context-epoch history record (frozen
/// contract core fields).
///
/// `project_id` is opaque structural metadata (no project table or
/// cross-component validation) persisted byte-for-byte. `epoch` is a
/// non-negative integer exactly as supplied by the trusted core boundary:
/// State neither increments it nor enforces any contiguity or ordering
/// rule against other records. `advanced_at` is an opaque contract
/// timestamp string stored exactly as supplied and never parsed,
/// normalized, compared, or regenerated. Changed sources and
/// invalidated role identities live in immutable normalized child rows and
/// do not alter this four-field carrier. Exact changed-source reads use a
/// separate presence-aware API that rejects uncaptured legacy values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEpoch {
    /// Owning project identity. Non-empty, at most
    /// [`MAX_IDENTIFIER_LENGTH`] scalar values; persisted byte-for-byte.
    pub project_id: String,
    /// The epoch number reached. Must be >= 0.
    pub epoch: StateEpochValueV1,
    /// The opaque timestamp supplied for the transition. Non-empty;
    /// stored exactly as provided and never parsed.
    pub advanced_at: String,
    /// The frozen trigger category that caused the transition.
    pub trigger: ContextEpochTrigger,
}

/// Closed changed-source reference categories; URL is opaque metadata only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangedSourceRefType {
    RepoPath,
    StateQuery,
    ArtifactId,
    Url,
}

impl ChangedSourceRefType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RepoPath => "REPO_PATH",
            Self::StateQuery => "STATE_QUERY",
            Self::ArtifactId => "ARTIFACT_ID",
            Self::Url => "URL",
        }
    }

    fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "REPO_PATH" => Ok(Self::RepoPath),
            "STATE_QUERY" => Ok(Self::StateQuery),
            "ARTIFACT_ID" => Ok(Self::ArtifactId),
            "URL" => Ok(Self::Url),
            _ => Err(changed_sources_decode(format!(
                "invalid ref_type {value:?}"
            ))),
        }
    }
}

/// One caller-supplied item. Strings are preserved byte-for-byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedSource {
    pub ref_type: ChangedSourceRefType,
    pub target: String,
    pub digest: Option<String>,
    pub section: Option<String>,
}

/// Valid captured values only. Legacy unavailable values are read errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangedSources {
    Omitted,
    Present(Vec<ChangedSource>),
}

impl SqliteStateRepository {
    /// Durably appends one immutable ContextEpoch with changed_sources Omitted.
    /// On v11 the column default records this presence state atomically.
    ///
    /// The append is atomic: validation, the duplicate pre-check, and the
    /// insert commit together in one transaction or not at all; there is
    /// no success before commit. A failure caused by validation, a
    /// duplicate, a SQLite constraint, or a transaction failure leaves no
    /// record and modifies nothing else.
    ///
    /// Fail-closed results, in deterministic precedence order:
    ///
    /// * an invalid input shape fails with
    ///   [`StateError::ContextEpochValidation`] before any storage
    ///   access;
    /// * a `(project_id, epoch)` pair that already exists fails with
    ///   [`StateError::ContextEpochAlreadyExists`], leaving the original
    ///   record untouched — this repository never overwrites, replaces,
    ///   upserts, merges, or deletes-and-reinserts epoch history.
    ///
    /// Uniqueness is per `(project_id, epoch)`, never database-global:
    /// the same epoch number for a different project remains appendable.
    /// Out-of-order epochs are accepted as supplied: this operation
    /// stores a record the trusted core boundary already constructed and
    /// never decides advancement, incrementing, sequencing, or trigger
    /// behavior. The schema's primary-key, CHECK, and NOT NULL
    /// constraints re-enforce the same rules as durable backstops, and
    /// all caller values reach SQL through bound parameters.
    pub fn append_context_epoch(&mut self, context_epoch: ContextEpoch) -> Result<(), StateError> {
        validate_for_append(&context_epoch)?;
        self.run_transaction(|uow| uow.insert_context_epoch(&context_epoch))
    }

    /// Atomically appends a parent, presence state and ordered source children.
    pub fn append_context_epoch_with_changed_sources(
        &mut self,
        context_epoch: ContextEpoch,
        changed_sources: ChangedSources,
    ) -> Result<(), StateError> {
        validate_for_append(&context_epoch)?;
        validate_changed_sources(&changed_sources)?;
        self.run_transaction(|uow| insert_context_epoch(uow.tx(), &context_epoch, &changed_sources))
    }

    /// Exact captured changed_sources for an epoch, on one read snapshot.
    /// Missing parent returns None; legacy uncaptured values fail closed.
    pub fn find_context_epoch_changed_sources(
        &self,
        project_id: &str,
        epoch: i64,
    ) -> Result<Option<ChangedSources>, StateError> {
        let epoch = StateEpochValueV1::try_from(epoch)?;
        self.find_context_epoch_changed_sources_value(project_id, &epoch)
    }

    pub fn find_context_epoch_changed_sources_value(
        &self,
        project_id: &str,
        epoch: &StateEpochValueV1,
    ) -> Result<Option<ChangedSources>, StateError> {
        validate_project_id(project_id)?;
        let tx = self
            .connection()
            .unchecked_transaction()
            .map_err(internal_query_failure)?;
        let parent = read_context_epoch(&tx, project_id, epoch)?;
        let sources = read_changed_source_items(&tx, project_id, epoch)?;
        let found = if parent.is_some() {
            let marker: Option<i64> = tx.query_row(
                "SELECT changed_sources_present FROM context_epoch WHERE project_id = ?1 AND epoch = ?2",
                params![project_id, epoch.as_str()], |row| row.get(0),
            ).map_err(|e| changed_sources_decode(e.to_string()))?;
            Some(match marker {
                Some(1) => ChangedSources::Present(sources),
                Some(0) if sources.is_empty() => ChangedSources::Omitted,
                None if sources.is_empty() => {
                    return Err(StateError::ContextEpochChangedSourcesNotCaptured {
                        project_id: project_id.to_string(),
                        epoch: epoch.clone(),
                    });
                }
                _ => return Err(changed_sources_decode("invalid presence/child composition")),
            })
        } else {
            if !sources.is_empty() {
                return Err(changed_sources_decode("changed sources without parent"));
            }
            None
        };
        tx.commit().map_err(internal_query_failure)?;
        Ok(found)
    }

    /// Derives and durably appends exactly one next ContextEpoch record
    /// for `project_id`, returning the committed record.
    ///
    /// This is the authoritative advancement primitive, invoked only after
    /// the trusted core boundary has already decided that advancement is
    /// required and has chosen the trigger: State performs no trigger
    /// detection of its own. The authoritative derivation rule is applied
    /// transactionally, per the frozen A1 bootstrap decision:
    ///
    /// * no persisted history for the project → the next epoch is `0`;
    /// * existing history → the next epoch is `max(persisted epoch) + 1`,
    ///   where the maximum is the highest numeric `epoch` (never the
    ///   latest timestamp, insertion order, rowid, or row count, and
    ///   history is not required to be contiguous).
    ///
    /// The latest-epoch lookup, the successor derivation, and the insert
    /// all occur inside one transaction: there is no success before
    /// commit, and any failure after derivation leaves the project
    /// history byte-for-byte unchanged, consuming nothing — because there
    /// is no sequence or pointer storage, a later successful call derives
    /// the same next number again. The latest lookup decodes the selected
    /// highest-epoch row through the same fail-closed contract decoder as
    /// every other read: a corrupt highest row fails the advancement
    /// rather than being ignored or skipped in favor of a lower row.
    ///
    /// Fail-closed results, in deterministic precedence order:
    ///
    /// * an invalid `project_id` or an empty `advanced_at` fails with
    ///   [`StateError::ContextEpochValidation`] before any storage
    ///   access — inputs are accepted unchanged or rejected, never
    ///   normalized, trimmed, parsed, or regenerated;
    /// * a `(project_id, derived epoch)` pair that already exists fails
    ///   with [`StateError::ContextEpochAlreadyExists`] (explicit
    ///   in-transaction pre-check, with the composite primary key as the
    ///   durable backstop against a concurrent writer). The conflict is
    ///   never resolved by retrying with a different epoch number,
    ///   skipping over the conflicting value, deleting, or replacing a
    ///   record.
    ///
    /// Advancement mutates nothing but immutable `context_epoch` parent and
    /// invalidated-role child history: no LogicalRole (including any `current_context_epoch`
    /// field), no ContextManifest (including its epoch snapshot and
    /// `last_rehydrated_at`), no ExecutorBinding, and no event — in
    /// particular no automatic `CONTEXT_EPOCH_ADVANCED` emission. There is
    /// no current-epoch pointer: the authoritative latest epoch remains
    /// the derived [`Self::find_latest_context_epoch`] query. All caller
    /// values reach SQL through bound parameters.
    pub fn advance_context_epoch(
        &mut self,
        project_id: &str,
        advanced_at: &str,
        trigger: ContextEpochTrigger,
        invalidated_role_ids: &[String],
    ) -> Result<ContextEpoch, StateError> {
        self.advance_context_epoch_with_changed_sources(
            project_id,
            advanced_at,
            trigger,
            invalidated_role_ids,
            ChangedSources::Omitted,
        )
    }

    /// Advances atomically with caller-supplied changed sources and invalidated roles.
    pub fn advance_context_epoch_with_changed_sources(
        &mut self,
        project_id: &str,
        advanced_at: &str,
        trigger: ContextEpochTrigger,
        invalidated_role_ids: &[String],
        changed_sources: ChangedSources,
    ) -> Result<ContextEpoch, StateError> {
        validate_changed_sources(&changed_sources)?;
        validate_project_id(project_id)?;
        ensure_non_empty("advanced_at", advanced_at)?;
        validate_invalidated_role_ids(invalidated_role_ids)?;
        self.run_transaction(|uow| {
            let latest = uow.read_latest_context_epoch(project_id)?;
            let epoch = derive_next_epoch(latest.as_ref().map(|record| &record.epoch))?;
            validate_invalidated_roles(uow.tx(), project_id, invalidated_role_ids)?;
            let created = ContextEpoch {
                project_id: project_id.to_string(),
                epoch: epoch.clone(),
                advanced_at: advanced_at.to_string(),
                trigger,
            };
            insert_context_epoch(uow.tx(), &created, &changed_sources)?;
            insert_invalidated_roles(uow.tx(), project_id, &epoch, invalidated_role_ids)?;
            Ok(created)
        })
    }

    /// Reads the immutable invalidated-role set for an exact epoch.
    /// Missing epoch returns `None`; an existing epoch with no children
    /// returns `Some(Vec::new())`. Results are sorted by role identity only
    /// for deterministic presentation; no order is persisted.
    pub fn find_context_epoch_invalidated_role_ids(
        &self,
        project_id: &str,
        epoch: i64,
    ) -> Result<Option<Vec<String>>, StateError> {
        let epoch = StateEpochValueV1::try_from(epoch)?;
        self.find_context_epoch_invalidated_role_ids_value(project_id, &epoch)
    }

    pub fn find_context_epoch_invalidated_role_ids_value(
        &self,
        project_id: &str,
        epoch: &StateEpochValueV1,
    ) -> Result<Option<Vec<String>>, StateError> {
        validate_project_id(project_id)?;
        let conn = self.connection();
        let tx = conn
            .unchecked_transaction()
            .map_err(internal_query_failure)?;
        if read_context_epoch(&tx, project_id, epoch)?.is_none() {
            tx.commit().map_err(internal_query_failure)?;
            return Ok(None);
        }
        let found = read_invalidated_role_ids(&tx, project_id, epoch)?;
        tx.commit().map_err(internal_query_failure)?;
        Ok(Some(found))
    }

    /// Reads one ContextEpoch by exact `(project_id, epoch)` identity.
    ///
    /// `Ok(None)` is the deterministic absence result for a record that
    /// does not exist. Both inputs are structurally validated first: an
    /// invalid `project_id` or a negative query `epoch` fails with
    /// [`StateError::ContextEpochValidation`] rather than querying for a
    /// shape that could never have persisted. The found row is decoded
    /// through the same fail-closed decoder as every other read: an
    /// unknown trigger, empty or overlong `project_id`, negative epoch,
    /// or empty `advanced_at` surfaces as
    /// [`StateError::ContextEpochDecodeFailed`] instead of a partially
    /// decoded or repaired record.
    pub fn find_context_epoch(
        &self,
        project_id: &str,
        epoch: i64,
    ) -> Result<Option<ContextEpoch>, StateError> {
        let epoch = StateEpochValueV1::try_from(epoch)?;
        self.find_context_epoch_value(project_id, &epoch)
    }

    pub fn find_context_epoch_value(
        &self,
        project_id: &str,
        epoch: &StateEpochValueV1,
    ) -> Result<Option<ContextEpoch>, StateError> {
        validate_project_id(project_id)?;
        let conn = self.connection();
        let tx = conn
            .unchecked_transaction()
            .map_err(internal_query_failure)?;
        let found = read_context_epoch(conn, project_id, epoch);
        // Read-only snapshot: commit merely ends it. On the error path above
        // the transaction rolls back on drop, with nothing written to undo.
        tx.commit().map_err(internal_query_failure)?;
        found
    }

    /// Derives the latest ContextEpoch record for one project from the
    /// immutable history: the record with the highest numeric `epoch`.
    ///
    /// This is a read-only query over persisted records, not a mutable
    /// current-epoch pointer, and nothing is mutated. `Ok(None)` is the
    /// deterministic result for a project with no epoch records. The
    /// numeric `epoch` field alone defines latest: `advanced_at` is never
    /// parsed or compared, insertion order and rowids are never
    /// authority, and epochs are not required to be contiguous — for
    /// epochs `0, 2, 7` the record with epoch `7` is returned. The
    /// selected row is decoded through the same fail-closed decoder as
    /// [`Self::find_context_epoch`]; a corrupt latest row is an error,
    /// never a silent fallback to an older valid row. `project_id` is
    /// structurally validated first.
    pub fn find_latest_context_epoch(
        &self,
        project_id: &str,
    ) -> Result<Option<ContextEpoch>, StateError> {
        validate_project_id(project_id)?;
        let conn = self.connection();
        let tx = conn
            .unchecked_transaction()
            .map_err(internal_query_failure)?;
        let found = query_latest_context_epoch(conn, project_id);
        // Read-only snapshot: commit merely ends it. On the error path above
        // the transaction rolls back on drop, with nothing written to undo.
        tx.commit().map_err(internal_query_failure)?;
        found
    }
}

impl UnitOfWork<'_> {
    /// Inserts one validated ContextEpoch inside the open transaction.
    /// Crate-private; invoked by
    /// [`SqliteStateRepository::append_context_epoch`].
    pub(crate) fn insert_context_epoch(
        &self,
        context_epoch: &ContextEpoch,
    ) -> Result<(), StateError> {
        insert_context_epoch(self.tx(), context_epoch, &ChangedSources::Omitted)
    }

    /// Reads the latest ContextEpoch for `project_id` on the open
    /// transaction's snapshot: the row with the highest numeric `epoch`,
    /// decoded through the same fail-closed contract decoder as
    /// [`SqliteStateRepository::find_latest_context_epoch`]. Crate-private;
    /// the authoritative advancement derivation queries its maximum
    /// through this read so that lookup, derivation, and insert share one
    /// transaction.
    pub(crate) fn read_latest_context_epoch(
        &self,
        project_id: &str,
    ) -> Result<Option<ContextEpoch>, StateError> {
        query_latest_context_epoch(self.tx(), project_id)
    }
}

const EPOCH_EXISTS_SQL: &str = "SELECT 1 FROM context_epoch WHERE project_id = ?1 AND epoch = ?2";

const INSERT_EPOCH_SQL: &str = "INSERT INTO context_epoch (
    project_id,
    epoch,
    advanced_at,
    trigger
) VALUES (?1, ?2, ?3, ?4)";

const SELECT_EPOCH_SQL: &str = "SELECT
    project_id,
    epoch,
    advanced_at,
    trigger
FROM context_epoch
WHERE project_id = ?1 AND epoch = ?2";

/// Latest-by-numeric-epoch query: an ordered lookup, not a timestamp or
/// insertion-order derivation.
const SELECT_LATEST_SQL: &str = "SELECT
    project_id,
    epoch,
    advanced_at,
    trigger
FROM context_epoch
WHERE project_id = ?1
ORDER BY length(epoch) DESC, epoch COLLATE BINARY DESC
LIMIT 1";

const SELECT_ROLE_PROJECT_SQL: &str = "SELECT project_id FROM logical_role WHERE role_id = ?1";

const INSERT_INVALIDATED_ROLE_SQL: &str = "INSERT INTO context_epoch_invalidated_role (
    project_id,
    epoch,
    role_id
) VALUES (?1, ?2, ?3)";

const SELECT_INVALIDATED_ROLE_IDS_SQL: &str = "SELECT role_id
FROM context_epoch_invalidated_role
WHERE project_id = ?1 AND epoch = ?2
ORDER BY role_id";

fn validate_invalidated_role_ids(role_ids: &[String]) -> Result<(), StateError> {
    let mut seen = HashSet::with_capacity(role_ids.len());
    for role_id in role_ids {
        ensure_invalidated_role_id(role_id)?;
        if !seen.insert(role_id.as_str()) {
            return Err(StateError::ContextEpochInvalidatedRoleDuplicate {
                role_id: role_id.clone(),
            });
        }
    }
    Ok(())
}

fn ensure_invalidated_role_id(role_id: &str) -> Result<(), StateError> {
    if role_id.is_empty() {
        return Err(StateError::ContextEpochValidation {
            detail: "invalidated role_id must not be empty".to_string(),
        });
    }
    let length = role_id.chars().count();
    if length > MAX_IDENTIFIER_LENGTH {
        return Err(StateError::ContextEpochValidation {
            detail: format!(
                "invalidated role_id length {length} exceeds the maximum of {MAX_IDENTIFIER_LENGTH}"
            ),
        });
    }
    Ok(())
}

fn validate_invalidated_roles(
    conn: &Connection,
    project_id: &str,
    role_ids: &[String],
) -> Result<(), StateError> {
    for role_id in role_ids {
        let role_project_id = conn
            .query_row(SELECT_ROLE_PROJECT_SQL, [role_id], |row| {
                row.get::<_, String>(0)
            })
            .optional()
            .map_err(write_failure)?;
        let Some(role_project_id) = role_project_id else {
            return Err(StateError::ContextEpochInvalidatedRoleNotFound {
                project_id: project_id.to_string(),
                role_id: role_id.clone(),
            });
        };
        if role_project_id != project_id {
            return Err(StateError::ContextEpochInvalidatedRoleProjectMismatch {
                epoch_project_id: project_id.to_string(),
                role_id: role_id.clone(),
                role_project_id,
            });
        }
    }
    Ok(())
}

fn insert_invalidated_roles(
    conn: &Connection,
    project_id: &str,
    epoch: &StateEpochValueV1,
    role_ids: &[String],
) -> Result<(), StateError> {
    let mut statement = conn
        .prepare(INSERT_INVALIDATED_ROLE_SQL)
        .map_err(invalidation_write_failure)?;
    for role_id in role_ids {
        statement
            .execute(params![project_id, epoch.as_str(), role_id])
            .map_err(invalidation_write_failure)?;
    }
    Ok(())
}

fn read_invalidated_role_ids(
    conn: &Connection,
    project_id: &str,
    epoch: &StateEpochValueV1,
) -> Result<Vec<String>, StateError> {
    let mut statement = conn
        .prepare(SELECT_INVALIDATED_ROLE_IDS_SQL)
        .map_err(internal_query_failure)?;
    let rows = statement
        .query_map(params![project_id, epoch.as_str()], |row| {
            row.get::<_, String>(0)
        })
        .map_err(internal_query_failure)?;
    let role_ids = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(internal_query_failure)?;
    for role_id in &role_ids {
        if role_id.is_empty() || role_id.chars().count() > MAX_IDENTIFIER_LENGTH {
            return Err(StateError::ContextEpochInvalidationDecodeFailed {
                detail: format!("persisted role_id {role_id:?} violates identifier constraints"),
            });
        }
    }
    Ok(role_ids)
}

/// Queries the highest-numeric-epoch record for `project_id` on the
/// caller's connection or open transaction snapshot and applies the
/// fail-closed contract decode: `Ok(None)` when the project has no
/// history, and a corrupt selected row is an error — never a silent
/// fallback to an older valid row.
fn query_latest_context_epoch(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<ContextEpoch>, StateError> {
    conn.query_row(SELECT_LATEST_SQL, [project_id], extract_epoch_row)
        .optional()
        .map_err(internal_query_failure)?
        .map(EpochRow::into_context_epoch)
        .transpose()
}

/// Derives the next epoch from the transaction-snapshot latest-epoch
/// lookup, per the frozen advancement rule: no persisted history for the
/// project → `0`; otherwise the exact decimal successor of `max`. Every
/// canonical finite decimal epoch has a successor; native integer bounds
/// are not part of this domain.
fn derive_next_epoch(
    latest_epoch: Option<&StateEpochValueV1>,
) -> Result<StateEpochValueV1, StateError> {
    match latest_epoch {
        None => StateEpochValueV1::try_from(0),
        Some(max) => max.successor(),
    }
}

/// Inserts one epoch record using bound parameters.
///
/// The composite `(project_id, epoch)` primary key is refused twice: an
/// explicit in-transaction check gives the common path a deterministic,
/// driver-independent error, while the primary-key constraint remains
/// the durable backstop (including against a concurrent writer on the
/// same database file). A backstop primary-key hit is mapped to the same
/// [`StateError::ContextEpochAlreadyExists`] result; the failed statement
/// leaves the still-open transaction rolled back by the caller's error
/// path, so no partial record can persist.
fn insert_context_epoch(
    conn: &Connection,
    epoch: &ContextEpoch,
    sources: &ChangedSources,
) -> Result<(), StateError> {
    let duplicate: Option<i64> = conn
        .query_row(
            EPOCH_EXISTS_SQL,
            params![epoch.project_id, epoch.epoch.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(write_failure)?;
    if duplicate.is_some() {
        return Err(StateError::ContextEpochAlreadyExists {
            project_id: epoch.project_id.clone(),
            epoch: epoch.epoch.clone(),
        });
    }
    conn.execute(
        match sources {
            ChangedSources::Omitted => INSERT_EPOCH_SQL,
            ChangedSources::Present(_) => {
                "INSERT INTO context_epoch
                (project_id, epoch, advanced_at, trigger, changed_sources_present)
                VALUES (?1, ?2, ?3, ?4, 1)"
            }
        },
        params![
            epoch.project_id,
            epoch.epoch.as_str(),
            epoch.advanced_at,
            epoch.trigger.as_str()
        ],
    )
    .map_err(|error| {
        if is_primary_key_violation(&error) {
            StateError::ContextEpochAlreadyExists {
                project_id: epoch.project_id.clone(),
                epoch: epoch.epoch.clone(),
            }
        } else {
            write_failure(error)
        }
    })?;
    if let ChangedSources::Present(items) = sources {
        let mut statement = conn
            .prepare(
                "INSERT INTO context_epoch_changed_source
            (project_id, epoch, source_ordinal, ref_type, target, digest, section)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )
            .map_err(write_failure)?;
        for (ordinal, item) in items.iter().enumerate() {
            let ordinal =
                i64::try_from(ordinal).map_err(|e| StateError::ContextEpochWriteFailed {
                    detail: e.to_string(),
                })?;
            statement
                .execute(params![
                    epoch.project_id,
                    epoch.epoch.as_str(),
                    ordinal,
                    item.ref_type.as_str(),
                    item.target,
                    item.digest,
                    item.section
                ])
                .map_err(write_failure)?;
        }
    }
    Ok(())
}

fn validate_changed_sources(sources: &ChangedSources) -> Result<(), StateError> {
    if let ChangedSources::Present(items) = sources {
        for item in items {
            ensure_non_empty("changed_source.target", &item.target)?;
        }
    }
    Ok(())
}

fn changed_sources_decode(detail: impl Into<String>) -> StateError {
    StateError::ContextEpochChangedSourcesDecodeFailed {
        detail: detail.into(),
    }
}

fn read_changed_source_items(
    conn: &Connection,
    project_id: &str,
    epoch: &StateEpochValueV1,
) -> Result<Vec<ChangedSource>, StateError> {
    let mut statement = conn.prepare("SELECT source_ordinal, ref_type, target, digest, section
        FROM context_epoch_changed_source WHERE project_id = ?1 AND epoch = ?2 ORDER BY source_ordinal")
        .map_err(|e| changed_sources_decode(e.to_string()))?;
    let mut rows = statement
        .query(params![project_id, epoch.as_str()])
        .map_err(|e| changed_sources_decode(e.to_string()))?;
    let mut items = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| changed_sources_decode(e.to_string()))?
    {
        let (ordinal, ref_type, target, digest, section) = (|| -> rusqlite::Result<_> {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
            ))
        })()
        .map_err(|e| changed_sources_decode(e.to_string()))?;
        if usize::try_from(ordinal).ok() != Some(items.len()) || target.is_empty() {
            return Err(changed_sources_decode(
                "invalid ordinal sequence or empty target",
            ));
        }
        items.push(ChangedSource {
            ref_type: ChangedSourceRefType::from_storage(&ref_type)?,
            target,
            digest,
            section,
        });
    }
    Ok(items)
}

/// Reads one epoch record on the caller's snapshot and applies contract
/// decoding, failing closed on any violation.
fn read_context_epoch(
    conn: &Connection,
    project_id: &str,
    epoch: &StateEpochValueV1,
) -> Result<Option<ContextEpoch>, StateError> {
    conn.query_row(
        SELECT_EPOCH_SQL,
        params![project_id, epoch.as_str()],
        extract_epoch_row,
    )
    .optional()
    .map_err(internal_query_failure)?
    .map(EpochRow::into_context_epoch)
    .transpose()
}

/// Raw column image of one `context_epoch` row, before any contract
/// interpretation is applied.
struct EpochRow {
    project_id: String,
    epoch: String,
    advanced_at: String,
    trigger: String,
}

impl EpochRow {
    /// Applies contract decoding to the row, failing closed on any
    /// violation: no fabricated trigger, no normalized `project_id`, no
    /// clamped epoch, no defaulted `advanced_at`.
    fn into_context_epoch(self) -> Result<ContextEpoch, StateError> {
        ensure_decoded_identifier("project_id", &self.project_id)?;
        let epoch = StateEpochValueV1::try_from(self.epoch).map_err(|error| {
            StateError::ContextEpochDecodeFailed {
                detail: error.to_string(),
            }
        })?;
        if self.advanced_at.is_empty() {
            return Err(StateError::ContextEpochDecodeFailed {
                detail: "persisted advanced_at is empty".to_string(),
            });
        }
        Ok(ContextEpoch {
            project_id: self.project_id,
            epoch,
            advanced_at: self.advanced_at,
            trigger: ContextEpochTrigger::from_storage(&self.trigger)?,
        })
    }
}

fn extract_epoch_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EpochRow> {
    Ok(EpochRow {
        project_id: row.get("project_id")?,
        epoch: row.get("epoch")?,
        advanced_at: row.get("advanced_at")?,
        trigger: row.get("trigger")?,
    })
}

/// Contract-level validation performed before any storage access on
/// append.
///
/// Exactly the frozen constraints are enforced — no invented lifecycle
/// rules: `project_id` non-empty and at most [`MAX_IDENTIFIER_LENGTH`]
/// scalar values; a non-negative `epoch`; a non-empty `advanced_at`. The
/// trigger is already restricted to the closed enumeration by its type.
/// Every value is accepted unchanged or rejected — never normalized,
/// trimmed, parsed, or regenerated.
fn validate_for_append(epoch: &ContextEpoch) -> Result<(), StateError> {
    validate_project_id(&epoch.project_id)?;
    ensure_non_empty("advanced_at", &epoch.advanced_at)?;
    Ok(())
}

/// Shared `project_id` structural check for the append and read paths.
fn validate_project_id(project_id: &str) -> Result<(), StateError> {
    ensure_non_empty("project_id", project_id)?;
    let length = project_id.chars().count();
    if length > MAX_IDENTIFIER_LENGTH {
        return Err(StateError::ContextEpochValidation {
            detail: format!(
                "project_id length {length} exceeds the maximum of {MAX_IDENTIFIER_LENGTH}"
            ),
        });
    }
    Ok(())
}

fn ensure_non_empty(field: &str, value: &str) -> Result<(), StateError> {
    if value.is_empty() {
        return Err(StateError::ContextEpochValidation {
            detail: format!("{field} must not be empty"),
        });
    }
    Ok(())
}

/// Decode-boundary identifier check mirroring the create-time rule so
/// contract-violating persisted rows fail closed instead of decoding.
fn ensure_decoded_identifier(field: &str, value: &str) -> Result<(), StateError> {
    if value.is_empty() {
        return Err(StateError::ContextEpochDecodeFailed {
            detail: format!("persisted {field} is empty"),
        });
    }
    let length = value.chars().count();
    if length > MAX_IDENTIFIER_LENGTH {
        return Err(StateError::ContextEpochDecodeFailed {
            detail: format!(
                "persisted {field} length {length} exceeds the maximum of {MAX_IDENTIFIER_LENGTH}"
            ),
        });
    }
    Ok(())
}

fn write_failure(error: rusqlite::Error) -> StateError {
    StateError::ContextEpochWriteFailed {
        detail: error.to_string(),
    }
}

fn invalidation_write_failure(error: rusqlite::Error) -> StateError {
    StateError::ContextEpochInvalidationWriteFailed {
        detail: error.to_string(),
    }
}

fn internal_query_failure(error: rusqlite::Error) -> StateError {
    StateError::InternalQueryFailed {
        detail: error.to_string(),
    }
}

/// SQLite extended result codes for a composite primary-key violation on
/// `context_epoch`: 1555 = `SQLITE_CONSTRAINT_PRIMARYKEY`, 2067 =
/// `SQLITE_CONSTRAINT_UNIQUE`.
fn is_primary_key_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                extended_code: 1555 | 2067,
                ..
            },
            _
        )
    )
}
