//! Durable ContextManifest persistence (migration 0006 slice).
//!
//! [`ContextManifest`] is the frozen persistence contract for a durable
//! role's authoritative context-source list: which references constitute
//! the role's context, the content digest recorded for each source at last
//! rehydration, and which execution phases each source is required for.
//! Each durable role has exactly one authoritative manifest; this slice
//! enforces that at the storage layer with a UNIQUE constraint over
//! `context_manifest(role_id)` backed by a foreign key to the durable
//! [`LogicalRole`](crate::logical_role::LogicalRole) identity.
//!
//! Scope of this slice: create + durable read only. A manifest is
//! immutable once persisted — there is deliberately no public update,
//! replace, upsert, delete, or per-field mutation path (including
//! `last_rehydrated_at`); manifest mutation is a later orchestrator action
//! requiring its own event/epoch semantics. Source order and
//! `required_for` order are persisted explicitly through contiguous
//! ordinals and round-trip exactly: nothing is sorted, deduplicated,
//! normalized, or defaulted.
//!
//! This module persists references and digest metadata only. It never
//! loads source contents, dereferences `ref` targets (no filesystem,
//! artifact, SQL, network, or shell access — `STATE_QUERY` is opaque
//! persistence metadata, never an executed query), computes or compares
//! digests, decides freshness, rehydrates an executor, advances or
//! invalidates a ContextEpoch, compacts context, or emits events.
//! `derived_state` is a projection the architecture derives elsewhere and
//! is deliberately not persisted here. `digest`, `ref_target`,
//! `created_at`, `last_read_at`, and `last_rehydrated_at` are opaque
//! strings stored exactly as supplied and never parsed or normalized.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::StateError;
use crate::repository::{SqliteStateRepository, UnitOfWork};

/// Maximum allowed length, in Unicode scalar values, of every constrained
/// ContextManifest identifier (accepted State convention: 200).
pub const MAX_IDENTIFIER_LENGTH: usize = 200;

/// The kind of reference a context source points at (frozen closed enum of
/// exactly three values).
///
/// There is no `URL`, `UNKNOWN`, `OTHER`, `CUSTOM`, or fallback variant:
/// `URL` appears only in a non-frozen candidate JSON schema and is not
/// representable in this slice; any value outside the three frozen kinds
/// fails closed at the decode boundary and is rejected by the storage
/// backstop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSourceRefType {
    /// `REPO_PATH` — a repository path reference. Never opened by State.
    RepoPath,
    /// `STATE_QUERY` — a state query reference. Persistence metadata
    /// only; never executed as SQL by State.
    StateQuery,
    /// `ARTIFACT_ID` — an artifact identity reference. Never dereferenced
    /// by State.
    ArtifactId,
}

impl ContextSourceRefType {
    /// The durable storage representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            ContextSourceRefType::RepoPath => "REPO_PATH",
            ContextSourceRefType::StateQuery => "STATE_QUERY",
            ContextSourceRefType::ArtifactId => "ARTIFACT_ID",
        }
    }

    /// Decodes the durable representation, failing closed on anything
    /// other than the three frozen reference kinds (including `URL`).
    pub(crate) fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "REPO_PATH" => Ok(ContextSourceRefType::RepoPath),
            "STATE_QUERY" => Ok(ContextSourceRefType::StateQuery),
            "ARTIFACT_ID" => Ok(ContextSourceRefType::ArtifactId),
            other => Err(StateError::ContextManifestDecodeFailed {
                detail: format!(
                    "unknown ref_type {other:?}: only REPO_PATH, STATE_QUERY, and ARTIFACT_ID are representable"
                ),
            }),
        }
    }
}

/// The rehydration class of a context source (frozen closed enum of
/// exactly three values).
///
/// There is no `UNKNOWN`, `OPTIONAL`, `OTHER`, `CUSTOM`, or fallback
/// variant: any value outside the three frozen classes fails closed at the
/// decode boundary and is rejected by the storage backstop. This slice
/// persists the class only; class-based rehydration behavior belongs to
/// the later rehydration engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceClass {
    /// `MANDATORY` — reread in full on every rehydration.
    Mandatory,
    /// `CONSUMED` — reread when a task touches it or its digest changed.
    Consumed,
    /// `REFERENCE` — read on demand only.
    Reference,
}

impl SourceClass {
    /// The durable storage representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            SourceClass::Mandatory => "MANDATORY",
            SourceClass::Consumed => "CONSUMED",
            SourceClass::Reference => "REFERENCE",
        }
    }

    /// Decodes the durable representation, failing closed on anything
    /// other than the three frozen source classes.
    pub(crate) fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "MANDATORY" => Ok(SourceClass::Mandatory),
            "CONSUMED" => Ok(SourceClass::Consumed),
            "REFERENCE" => Ok(SourceClass::Reference),
            other => Err(StateError::ContextManifestDecodeFailed {
                detail: format!(
                    "unknown source_class {other:?}: only MANDATORY, CONSUMED, and REFERENCE are representable"
                ),
            }),
        }
    }
}

/// One phase of the `required_for` list (frozen closed enum of exactly
/// five values).
///
/// There is no fallback variant: any value outside the five frozen phases
/// fails closed at the decode boundary and is rejected by the storage
/// backstop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequiredFor {
    /// `DECOMPOSITION`.
    Decomposition,
    /// `DISPATCH`.
    Dispatch,
    /// `ACCEPTANCE`.
    Acceptance,
    /// `INTEGRATION`.
    Integration,
    /// `EVALUATION`.
    Evaluation,
}

impl RequiredFor {
    /// The durable storage representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            RequiredFor::Decomposition => "DECOMPOSITION",
            RequiredFor::Dispatch => "DISPATCH",
            RequiredFor::Acceptance => "ACCEPTANCE",
            RequiredFor::Integration => "INTEGRATION",
            RequiredFor::Evaluation => "EVALUATION",
        }
    }

    /// Decodes the durable representation, failing closed on anything
    /// other than the five frozen phases.
    pub(crate) fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "DECOMPOSITION" => Ok(RequiredFor::Decomposition),
            "DISPATCH" => Ok(RequiredFor::Dispatch),
            "ACCEPTANCE" => Ok(RequiredFor::Acceptance),
            "INTEGRATION" => Ok(RequiredFor::Integration),
            "EVALUATION" => Ok(RequiredFor::Evaluation),
            other => Err(StateError::ContextManifestDecodeFailed {
                detail: format!(
                    "unknown required_for {other:?}: only DECOMPOSITION, DISPATCH, ACCEPTANCE, INTEGRATION, and EVALUATION are representable"
                ),
            }),
        }
    }
}

/// An opaque reference to one context source (frozen contract).
///
/// `target` is opaque reference metadata: State persists it exactly as
/// supplied and never opens, canonicalizes, resolves, executes,
/// dereferences, fetches, or interprets it. `STATE_QUERY` targets in
/// particular are persistence metadata only and never become SQL
/// execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSourceRef {
    /// The frozen reference kind.
    pub ref_type: ContextSourceRefType,
    /// The opaque reference target. Non-empty; persisted byte-for-byte.
    pub target: String,
}

/// One ordered context source of a manifest (frozen contract).
///
/// Required fields are non-nullable; `last_read_at` is nullable exactly as
/// the contract lists it. `digest` is opaque structural integrity
/// metadata: State persists it byte-for-byte and never computes, chooses
/// an algorithm for, validates the formatting of, or compares it. The
/// `required_for` list is ordered: an empty vector means "required for no
/// phases", and the stored order round-trips exactly (duplicates
/// included).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextManifestSource {
    /// The source reference.
    pub r#ref: ContextSourceRef,
    /// The frozen rehydration class.
    pub source_class: SourceClass,
    /// Opaque content digest recorded at last rehydration. Non-empty;
    /// persisted byte-for-byte.
    pub digest: String,
    /// Optional opaque last-read timestamp. When present: non-empty,
    /// stored as provided and never parsed.
    pub last_read_at: Option<String>,
    /// Ordered required-for phase list. Order and duplicates round-trip
    /// exactly.
    pub required_for: Vec<RequiredFor>,
}

/// A durable role's authoritative context-source list (frozen contract).
///
/// Required fields are non-nullable; `last_rehydrated_at` is nullable
/// exactly as the contract lists it. `epoch` is a non-negative integer
/// snapshot: State persists it and implements no ContextEpoch lifecycle
/// behavior (no advance, increment, invalidate, reconcile, or
/// epoch-triggered mutation). `sources` is the authoritative ordered
/// context-source list: at least one source, with order preserved exactly
/// through explicit ordinals. `derived_state` is deliberately absent: it
/// is a projection the architecture derives elsewhere, and its durable
/// nested representation/mutation lifecycle is not part of this slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextManifest {
    /// Durable manifest identity. Non-empty, at most
    /// [`MAX_IDENTIFIER_LENGTH`] scalar values; the durable primary key.
    pub manifest_id: String,
    /// The durable LogicalRole that owns this manifest. Non-empty, at
    /// most [`MAX_IDENTIFIER_LENGTH`] scalar values; must reference an
    /// existing persisted role.
    pub role_id: String,
    /// Owning project identity. Non-empty, at most
    /// [`MAX_IDENTIFIER_LENGTH`] scalar values; opaque structural
    /// metadata only (no project table or cross-component validation).
    pub project_id: String,
    /// Manifest epoch snapshot. Must be >= 0.
    pub epoch: i64,
    /// The authoritative ordered context-source list. Non-empty; order
    /// round-trips exactly.
    pub sources: Vec<ContextManifestSource>,
    /// Creation timestamp, opaque to State and stored exactly as provided.
    /// Non-empty.
    pub created_at: String,
    /// Optional last-rehydration timestamp, opaque to State and stored
    /// exactly as provided. When present: non-empty. This slice exposes
    /// no method for updating this value.
    pub last_rehydrated_at: Option<String>,
}

impl SqliteStateRepository {
    /// Durably creates a new immutable ContextManifest.
    ///
    /// Creation is atomic: the manifest row, all of its ordered source
    /// rows, and all of their ordered `required_for` rows commit together
    /// in one transaction or not at all; a failure anywhere rolls back the
    /// entire graph and never persists a partial manifest. There is no
    /// success before commit.
    ///
    /// Fail-closed results, in deterministic precedence order:
    ///
    /// * a `manifest_id` that already exists fails with
    ///   [`StateError::ContextManifestAlreadyExists`], leaving the
    ///   original manifest untouched — this repository never overwrites,
    ///   replaces, upserts, merges, or deletes-and-reinserts a manifest;
    /// * a `role_id` that does not reference an existing persisted
    ///   LogicalRole fails with [`StateError::ContextManifestRoleNotFound`]
    ///   (the schema's foreign key is the durable backstop), and no parent
    ///   or child rows remain;
    /// * a `role_id` that already owns a different manifest fails with
    ///   [`StateError::ContextManifestRoleAlreadyHasManifest`] naming the
    ///   existing manifest, leaving it untouched — at most one
    ///   authoritative manifest per role may persist, and the existing
    ///   manifest is never replaced, deleted, updated, or silently chosen
    ///   over;
    /// * an invalid input shape fails with
    ///   [`StateError::ContextManifestValidation`] before any storage
    ///   access.
    ///
    /// Contract-level validation runs before any storage access, and the
    /// schema's primary-key, foreign-key, uniqueness, CHECK, and NOT NULL
    /// constraints re-enforce the same rules as durable backstops. All
    /// caller values reach SQL through bound parameters.
    pub fn create_context_manifest(&mut self, manifest: ContextManifest) -> Result<(), StateError> {
        validate_for_create(&manifest)?;
        self.run_transaction(|uow| uow.insert_context_manifest(&manifest))
    }

    /// Reads one ContextManifest by durable identity.
    ///
    /// `Ok(None)` is the deterministic absence result for a manifest that
    /// does not exist. The manifest row, its sources (ordered by stored
    /// ordinal), and each source's `required_for` list (ordered by stored
    /// ordinal) are read on a single snapshot and reconstructed exactly.
    /// Decoding fails closed: unknown enum values, empty structural
    /// strings, a zero-source manifest, ordinal gaps, and any other
    /// contract-violating row surface as
    /// [`StateError::ContextManifestDecodeFailed`] instead of a partially
    /// decoded manifest, a fabricated default, or silently dropped
    /// malformed children — corrupted rows are never renumbered.
    ///
    /// This read loads no source contents and compares no digests.
    pub fn find_context_manifest(
        &self,
        manifest_id: &str,
    ) -> Result<Option<ContextManifest>, StateError> {
        let conn = self.connection();
        let tx = conn
            .unchecked_transaction()
            .map_err(internal_query_failure)?;
        let found = read_context_manifest(&tx, manifest_id)?;
        // Read-only snapshot: commit merely ends it. On the error path above
        // the transaction rolls back on drop, with nothing written to undo.
        tx.commit().map_err(internal_query_failure)?;
        Ok(found)
    }
}

impl UnitOfWork<'_> {
    /// Inserts one validated ContextManifest and its ordered source and
    /// `required_for` rows inside the open transaction. Crate-private;
    /// invoked by [`SqliteStateRepository::create_context_manifest`].
    pub(crate) fn insert_context_manifest(
        &self,
        manifest: &ContextManifest,
    ) -> Result<(), StateError> {
        insert_context_manifest(self.tx(), manifest)
    }
}

const MANIFEST_EXISTS_SQL: &str = "SELECT 1 FROM context_manifest WHERE manifest_id = ?1";

const ROLE_EXISTS_FOR_MANIFEST_SQL: &str = "SELECT 1 FROM logical_role WHERE role_id = ?1";

/// Create-time one-manifest-per-role pre-check: the existing manifest for
/// the target role, if any. `ORDER BY manifest_id LIMIT 1` keeps the
/// reported existing manifest deterministic even for storage this
/// repository could not itself have produced.
const ROLE_MANIFEST_SQL: &str = "SELECT manifest_id FROM context_manifest
WHERE role_id = ?1
ORDER BY manifest_id
LIMIT 1";

const INSERT_MANIFEST_SQL: &str = "INSERT INTO context_manifest (
    manifest_id,
    role_id,
    project_id,
    epoch,
    created_at,
    last_rehydrated_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6)";

const INSERT_SOURCE_SQL: &str = "INSERT INTO context_manifest_source (
    manifest_id,
    source_ordinal,
    ref_type,
    ref_target,
    source_class,
    digest,
    last_read_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";

const INSERT_REQUIRED_FOR_SQL: &str = "INSERT INTO context_manifest_source_required_for (
    manifest_id,
    source_ordinal,
    required_for_ordinal,
    required_for
) VALUES (?1, ?2, ?3, ?4)";

const SELECT_MANIFEST_SQL: &str = "SELECT
    manifest_id,
    role_id,
    project_id,
    epoch,
    created_at,
    last_rehydrated_at
FROM context_manifest
WHERE manifest_id = ?1";

const SELECT_SOURCES_SQL: &str = "SELECT
    source_ordinal,
    ref_type,
    ref_target,
    source_class,
    digest,
    last_read_at
FROM context_manifest_source
WHERE manifest_id = ?1
ORDER BY source_ordinal";

const SELECT_REQUIRED_FOR_SQL: &str = "SELECT required_for_ordinal, required_for
FROM context_manifest_source_required_for
WHERE manifest_id = ?1 AND source_ordinal = ?2
ORDER BY required_for_ordinal";

/// Inserts the manifest row, all ordered source rows, and all ordered
/// `required_for` rows using bound parameters.
///
/// The frozen relational rules are refused twice each: explicit
/// in-transaction checks give the common path deterministic,
/// driver-independent errors, while the primary-key, foreign-key, and
/// role-uniqueness constraints remain the durable backstops (including
/// against a concurrent writer on the same database file). Check order
/// preserves the contract precedence: duplicate `manifest_id` first, then
/// role existence, then the one-manifest-per-role guard, then the inserts.
fn insert_context_manifest(
    conn: &Connection,
    manifest: &ContextManifest,
) -> Result<(), StateError> {
    let duplicate: Option<i64> = conn
        .query_row(MANIFEST_EXISTS_SQL, [&manifest.manifest_id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(write_failure)?;
    if duplicate.is_some() {
        return Err(StateError::ContextManifestAlreadyExists {
            manifest_id: manifest.manifest_id.clone(),
        });
    }

    let role: Option<i64> = conn
        .query_row(ROLE_EXISTS_FOR_MANIFEST_SQL, [&manifest.role_id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(write_failure)?;
    if role.is_none() {
        return Err(StateError::ContextManifestRoleNotFound {
            role_id: manifest.role_id.clone(),
        });
    }

    if let Some(existing_manifest_id) = find_role_manifest(conn, &manifest.role_id)? {
        return Err(StateError::ContextManifestRoleAlreadyHasManifest {
            role_id: manifest.role_id.clone(),
            existing_manifest_id,
        });
    }

    conn.execute(
        INSERT_MANIFEST_SQL,
        params![
            manifest.manifest_id,
            manifest.role_id,
            manifest.project_id,
            manifest.epoch,
            manifest.created_at,
            manifest.last_rehydrated_at
        ],
    )
    .map_err(|error| {
        if is_role_uniqueness_violation(&error) {
            role_conflict_from_backstop(conn, &manifest.role_id, error)
        } else if is_manifest_id_constraint_violation(&error) {
            StateError::ContextManifestAlreadyExists {
                manifest_id: manifest.manifest_id.clone(),
            }
        } else if is_foreign_key_violation(&error) {
            StateError::ContextManifestRoleNotFound {
                role_id: manifest.role_id.clone(),
            }
        } else {
            write_failure(error)
        }
    })?;

    let mut source_statement = conn.prepare(INSERT_SOURCE_SQL).map_err(write_failure)?;
    for (index, source) in manifest.sources.iter().enumerate() {
        let source_ordinal = ordinal_for("sources", index)?;
        source_statement
            .execute(params![
                manifest.manifest_id,
                source_ordinal,
                source.r#ref.ref_type.as_str(),
                source.r#ref.target,
                source.source_class.as_str(),
                source.digest,
                source.last_read_at
            ])
            .map_err(write_failure)?;
    }

    let mut phase_statement = conn
        .prepare(INSERT_REQUIRED_FOR_SQL)
        .map_err(write_failure)?;
    for (index, source) in manifest.sources.iter().enumerate() {
        let source_ordinal = ordinal_for("sources", index)?;
        for (phase_index, phase) in source.required_for.iter().enumerate() {
            let phase_ordinal = ordinal_for("required_for", phase_index)?;
            phase_statement
                .execute(params![
                    manifest.manifest_id,
                    source_ordinal,
                    phase_ordinal,
                    phase.as_str()
                ])
                .map_err(write_failure)?;
        }
    }
    Ok(())
}

/// Converts a list position into the durable non-negative ordinal.
fn ordinal_for(field: &str, index: usize) -> Result<i64, StateError> {
    i64::try_from(index).map_err(|_| StateError::ContextManifestValidation {
        detail: format!("{field} is too long to assign ordered ordinals ({index} elements)"),
    })
}

/// Reads the deterministic first existing manifest for `role_id` on the
/// caller's transaction snapshot, if any.
///
/// This is the bounded internal inspection behind the create-time
/// one-manifest-per-role guard: one indexed lookup, no public
/// manifest-by-role query surface.
fn find_role_manifest(conn: &Connection, role_id: &str) -> Result<Option<String>, StateError> {
    conn.query_row(ROLE_MANIFEST_SQL, [role_id], |row| row.get::<_, String>(0))
        .optional()
        .map_err(write_failure)
}

/// Maps a storage-level unique-constraint hit on the
/// `context_manifest(role_id)` UNIQUE backstop to the deterministic
/// role-manifest conflict.
///
/// The failed statement leaves the still-open transaction usable, so the
/// existing manifest is re-read to identify the conflict. If it cannot be
/// re-read, the storage failure itself is surfaced: the operation fails
/// closed either way and is never reported as successful creation, and no
/// manifest identity is ever fabricated.
fn role_conflict_from_backstop(
    conn: &Connection,
    role_id: &str,
    error: rusqlite::Error,
) -> StateError {
    match find_role_manifest(conn, role_id) {
        Ok(Some(existing_manifest_id)) => StateError::ContextManifestRoleAlreadyHasManifest {
            role_id: role_id.to_string(),
            existing_manifest_id,
        },
        _ => write_failure(error),
    }
}

/// Reads one manifest graph on the caller's snapshot and applies contract
/// decoding, failing closed on any violation.
fn read_context_manifest(
    conn: &Connection,
    manifest_id: &str,
) -> Result<Option<ContextManifest>, StateError> {
    let Some(row) = conn
        .query_row(SELECT_MANIFEST_SQL, [manifest_id], extract_manifest_row)
        .optional()
        .map_err(internal_query_failure)?
    else {
        return Ok(None);
    };
    let source_rows = read_source_rows(conn, manifest_id)?;
    row.into_context_manifest(conn, source_rows).map(Some)
}

/// Reads all source rows of one manifest ordered by stored ordinal.
fn read_source_rows(conn: &Connection, manifest_id: &str) -> Result<Vec<SourceRow>, StateError> {
    let mut statement = conn
        .prepare(SELECT_SOURCES_SQL)
        .map_err(internal_query_failure)?;
    let rows = statement
        .query_map([manifest_id], extract_source_row)
        .map_err(internal_query_failure)?;
    rows.collect::<Result<Vec<SourceRow>, _>>()
        .map_err(internal_query_failure)
}

/// Reads one source's ordered `required_for` values as raw
/// `(ordinal, value)` pairs.
fn read_required_for_rows(
    conn: &Connection,
    manifest_id: &str,
    source_ordinal: i64,
) -> Result<Vec<(i64, String)>, StateError> {
    let mut statement = conn
        .prepare(SELECT_REQUIRED_FOR_SQL)
        .map_err(internal_query_failure)?;
    let rows = statement
        .query_map(params![manifest_id, source_ordinal], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(internal_query_failure)?;
    rows.collect::<Result<Vec<(i64, String)>, _>>()
        .map_err(internal_query_failure)
}

/// Raw column image of one `context_manifest` row, before any contract
/// interpretation is applied.
struct ManifestRow {
    manifest_id: String,
    role_id: String,
    project_id: String,
    epoch: i64,
    created_at: String,
    last_rehydrated_at: Option<String>,
}

impl ManifestRow {
    /// Applies contract decoding to the parent row and its already-read
    /// source rows, failing closed on any violation.
    ///
    /// Source ordinals must be exactly `0, 1, ..., N-1` in the ordered
    /// read: gaps, negatives, or any other deviation are rejected rather
    /// than renumbered, and a zero-source persisted manifest is rejected
    /// rather than decoded as a valid empty manifest. Malformed child rows
    /// are never silently dropped.
    fn into_context_manifest(
        self,
        conn: &Connection,
        source_rows: Vec<SourceRow>,
    ) -> Result<ContextManifest, StateError> {
        ensure_decoded_identifier("manifest_id", &self.manifest_id)?;
        ensure_decoded_identifier("role_id", &self.role_id)?;
        ensure_decoded_identifier("project_id", &self.project_id)?;
        if self.epoch < 0 {
            return Err(StateError::ContextManifestDecodeFailed {
                detail: format!("persisted epoch {} is negative", self.epoch),
            });
        }
        if self.created_at.is_empty() {
            return Err(StateError::ContextManifestDecodeFailed {
                detail: "persisted created_at is empty".to_string(),
            });
        }
        if let Some(last_rehydrated_at) = &self.last_rehydrated_at
            && last_rehydrated_at.is_empty()
        {
            return Err(StateError::ContextManifestDecodeFailed {
                detail: "persisted last_rehydrated_at is present but empty".to_string(),
            });
        }
        if source_rows.is_empty() {
            return Err(StateError::ContextManifestDecodeFailed {
                detail: format!(
                    "manifest {} has zero persisted sources; a valid manifest carries at least one",
                    self.manifest_id
                ),
            });
        }

        let mut sources = Vec::with_capacity(source_rows.len());
        for (index, row) in source_rows.into_iter().enumerate() {
            let expected = ordinal_for("sources", index)?;
            if row.source_ordinal != expected {
                return Err(StateError::ContextManifestDecodeFailed {
                    detail: format!(
                        "manifest {} source ordinals are not contiguous: expected {expected} at position {index}, found {}",
                        self.manifest_id, row.source_ordinal
                    ),
                });
            }
            let required_for = decode_required_for(conn, &self.manifest_id, row.source_ordinal)?;
            sources.push(row.into_source(required_for)?);
        }
        Ok(ContextManifest {
            manifest_id: self.manifest_id,
            role_id: self.role_id,
            project_id: self.project_id,
            epoch: self.epoch,
            sources,
            created_at: self.created_at,
            last_rehydrated_at: self.last_rehydrated_at,
        })
    }
}

/// Decodes one source's ordered `required_for` list, enforcing contiguous
/// ordinals `0, 1, ..., M-1` and the closed phase enumeration.
fn decode_required_for(
    conn: &Connection,
    manifest_id: &str,
    source_ordinal: i64,
) -> Result<Vec<RequiredFor>, StateError> {
    let rows = read_required_for_rows(conn, manifest_id, source_ordinal)?;
    let mut phases = Vec::with_capacity(rows.len());
    for (index, (stored_ordinal, value)) in rows.into_iter().enumerate() {
        let expected = ordinal_for("required_for", index)?;
        if stored_ordinal != expected {
            return Err(StateError::ContextManifestDecodeFailed {
                detail: format!(
                    "manifest {manifest_id} source {source_ordinal} required_for ordinals are not contiguous: expected {expected} at position {index}, found {stored_ordinal}"
                ),
            });
        }
        phases.push(RequiredFor::from_storage(&value)?);
    }
    Ok(phases)
}

/// Raw column image of one `context_manifest_source` row, before any
/// contract interpretation is applied.
struct SourceRow {
    source_ordinal: i64,
    ref_type: String,
    ref_target: String,
    source_class: String,
    digest: String,
    last_read_at: Option<String>,
}

impl SourceRow {
    /// Applies contract decoding (closed enumerations, non-empty
    /// structural strings) and fails closed on any violation.
    fn into_source(
        self,
        required_for: Vec<RequiredFor>,
    ) -> Result<ContextManifestSource, StateError> {
        if self.ref_target.is_empty() {
            return Err(StateError::ContextManifestDecodeFailed {
                detail: format!(
                    "persisted ref_target at source ordinal {} is empty",
                    self.source_ordinal
                ),
            });
        }
        if self.digest.is_empty() {
            return Err(StateError::ContextManifestDecodeFailed {
                detail: format!(
                    "persisted digest at source ordinal {} is empty",
                    self.source_ordinal
                ),
            });
        }
        if let Some(last_read_at) = &self.last_read_at
            && last_read_at.is_empty()
        {
            return Err(StateError::ContextManifestDecodeFailed {
                detail: format!(
                    "persisted last_read_at at source ordinal {} is present but empty",
                    self.source_ordinal
                ),
            });
        }
        Ok(ContextManifestSource {
            r#ref: ContextSourceRef {
                ref_type: ContextSourceRefType::from_storage(&self.ref_type)?,
                target: self.ref_target,
            },
            source_class: SourceClass::from_storage(&self.source_class)?,
            digest: self.digest,
            last_read_at: self.last_read_at,
            required_for,
        })
    }
}

fn extract_manifest_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManifestRow> {
    Ok(ManifestRow {
        manifest_id: row.get("manifest_id")?,
        role_id: row.get("role_id")?,
        project_id: row.get("project_id")?,
        epoch: row.get("epoch")?,
        created_at: row.get("created_at")?,
        last_rehydrated_at: row.get("last_rehydrated_at")?,
    })
}

fn extract_source_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceRow> {
    Ok(SourceRow {
        source_ordinal: row.get("source_ordinal")?,
        ref_type: row.get("ref_type")?,
        ref_target: row.get("ref_target")?,
        source_class: row.get("source_class")?,
        digest: row.get("digest")?,
        last_read_at: row.get("last_read_at")?,
    })
}

/// Contract-level validation performed before any storage access on
/// create.
///
/// Exactly the frozen constraints are enforced — no invented orchestration
/// rules: the constrained identifiers (`manifest_id`, `role_id`,
/// `project_id`) non-empty and at most [`MAX_IDENTIFIER_LENGTH`] scalar
/// values; a non-negative epoch; a non-empty source list whose opaque
/// `ref_target` and `digest` values are non-empty and whose optional
/// `last_read_at` is non-empty when present; a non-empty `created_at`; and
/// an optional `last_rehydrated_at` that is non-empty when present. Every
/// value is accepted unchanged or rejected — never normalized, trimmed,
/// hashed, rewritten, or regenerated. The closed enumerations are already
/// restricted to their contract sets by their enum types.
fn validate_for_create(manifest: &ContextManifest) -> Result<(), StateError> {
    ensure_identifier("manifest_id", &manifest.manifest_id)?;
    ensure_identifier("role_id", &manifest.role_id)?;
    ensure_identifier("project_id", &manifest.project_id)?;
    if manifest.epoch < 0 {
        return Err(StateError::ContextManifestValidation {
            detail: format!("epoch must be >= 0, found {}", manifest.epoch),
        });
    }
    if manifest.sources.is_empty() {
        return Err(StateError::ContextManifestValidation {
            detail: "sources must contain at least one source".to_string(),
        });
    }
    for (index, source) in manifest.sources.iter().enumerate() {
        ensure_non_empty(
            &format!("sources[{index}].ref.target"),
            &source.r#ref.target,
        )?;
        ensure_non_empty(&format!("sources[{index}].digest"), &source.digest)?;
        if let Some(last_read_at) = &source.last_read_at {
            ensure_non_empty(&format!("sources[{index}].last_read_at"), last_read_at)?;
        }
    }
    ensure_non_empty("created_at", &manifest.created_at)?;
    if let Some(last_rehydrated_at) = &manifest.last_rehydrated_at {
        ensure_non_empty("last_rehydrated_at", last_rehydrated_at)?;
    }
    Ok(())
}

fn ensure_identifier(field: &str, value: &str) -> Result<(), StateError> {
    ensure_non_empty(field, value)?;
    let length = value.chars().count();
    if length > MAX_IDENTIFIER_LENGTH {
        return Err(StateError::ContextManifestValidation {
            detail: format!(
                "{field} length {length} exceeds the maximum of {MAX_IDENTIFIER_LENGTH}"
            ),
        });
    }
    Ok(())
}

fn ensure_non_empty(field: &str, value: &str) -> Result<(), StateError> {
    if value.is_empty() {
        return Err(StateError::ContextManifestValidation {
            detail: format!("{field} must not be empty"),
        });
    }
    Ok(())
}

/// Decode-boundary identifier check mirroring the create-time rule so
/// contract-violating persisted rows fail closed instead of decoding.
fn ensure_decoded_identifier(field: &str, value: &str) -> Result<(), StateError> {
    if value.is_empty() {
        return Err(StateError::ContextManifestDecodeFailed {
            detail: format!("persisted {field} is empty"),
        });
    }
    let length = value.chars().count();
    if length > MAX_IDENTIFIER_LENGTH {
        return Err(StateError::ContextManifestDecodeFailed {
            detail: format!(
                "persisted {field} length {length} exceeds the maximum of {MAX_IDENTIFIER_LENGTH}"
            ),
        });
    }
    Ok(())
}

fn write_failure(error: rusqlite::Error) -> StateError {
    StateError::ContextManifestWriteFailed {
        detail: error.to_string(),
    }
}

fn internal_query_failure(error: rusqlite::Error) -> StateError {
    StateError::InternalQueryFailed {
        detail: error.to_string(),
    }
}

/// SQLite extended result codes for identity constraint violations on the
/// `context_manifest` primary key: 1555 = `SQLITE_CONSTRAINT_PRIMARYKEY`,
/// 2067 = `SQLITE_CONSTRAINT_UNIQUE`. Checked after
/// [`is_role_uniqueness_violation`] so a 2067 raised by the role UNIQUE
/// backstop is never misreported as a duplicate `manifest_id`.
fn is_manifest_id_constraint_violation(error: &rusqlite::Error) -> bool {
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

/// A storage-level violation of the UNIQUE backstop over
/// `context_manifest(role_id)`: 2067 = `SQLITE_CONSTRAINT_UNIQUE` with the
/// driver message naming `context_manifest.role_id`, the only column of
/// that uniqueness constraint.
fn is_role_uniqueness_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                extended_code: 2067,
                ..
            },
            Some(message),
        ) if message.contains("context_manifest.role_id")
    )
}

/// SQLite extended result code for a foreign-key violation:
/// 787 = `SQLITE_CONSTRAINT_FOREIGNKEY`.
fn is_foreign_key_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                extended_code: 787,
                ..
            },
            _
        )
    )
}
