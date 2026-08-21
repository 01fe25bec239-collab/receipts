//! Migration 0006: durable ContextManifest persistence.
//!
//! Creates only the normalized minimum schema required by the bounded
//! ContextManifest persistence slice: one row per authoritative manifest,
//! an ordered child table for its context sources, and a grandchild table
//! for each source's ordered `required_for` phase list. `role_id` carries
//! both a foreign key to the durable `logical_role` identity and a UNIQUE
//! constraint, so at most one authoritative manifest per role can ever
//! persist at the storage layer. No ContextEpoch, rehydration,
//! derived-state, source-content, digest-cache, event, binding, provider,
//! model, host, task, finding, or dependency structures belong here.
//!
//! The table-level CHECK constraints mirror the closed contract
//! enumerations (`ref_type`, `source_class`, `required_for`) and the
//! non-negative ordinal/epoch rules, so the storage itself remains the
//! durable backstop even against writes that bypass the typed repository
//! layer. `ref_type` is exactly the three frozen reference kinds; `URL`
//! appears only in a non-frozen candidate JSON schema and is deliberately
//! not representable. Source ordering is explicit through `source_ordinal`
//! and `required_for_ordinal`; contiguity (0..N-1) is enforced at the typed
//! decode boundary, never silently repaired. `digest`, `ref_target`,
//! `created_at`, `last_read_at`, and `last_rehydrated_at` are stored
//! exactly as provided: State does not compute digests, parse timestamps,
//! or dereference targets.

use super::Migration;

const VERSION: u32 = 6;
const NAME: &str = "context_manifest";

/// The registered sixth migration.
pub(crate) const MIGRATION: Migration = Migration {
    version: VERSION,
    name: NAME,
    sql: "CREATE TABLE context_manifest (
    manifest_id TEXT NOT NULL PRIMARY KEY,
    role_id TEXT NOT NULL UNIQUE REFERENCES logical_role (role_id),
    project_id TEXT NOT NULL,
    epoch INTEGER NOT NULL CHECK (epoch >= 0),
    created_at TEXT NOT NULL,
    last_rehydrated_at TEXT
);
CREATE TABLE context_manifest_source (
    manifest_id TEXT NOT NULL REFERENCES context_manifest (manifest_id),
    source_ordinal INTEGER NOT NULL CHECK (source_ordinal >= 0),
    ref_type TEXT NOT NULL CHECK (ref_type IN ('REPO_PATH', 'STATE_QUERY', 'ARTIFACT_ID')),
    ref_target TEXT NOT NULL,
    source_class TEXT NOT NULL CHECK (source_class IN ('MANDATORY', 'CONSUMED', 'REFERENCE')),
    digest TEXT NOT NULL,
    last_read_at TEXT,
    PRIMARY KEY (manifest_id, source_ordinal)
);
CREATE TABLE context_manifest_source_required_for (
    manifest_id TEXT NOT NULL,
    source_ordinal INTEGER NOT NULL CHECK (source_ordinal >= 0),
    required_for_ordinal INTEGER NOT NULL CHECK (required_for_ordinal >= 0),
    required_for TEXT NOT NULL CHECK (required_for IN (
        'DECOMPOSITION',
        'DISPATCH',
        'ACCEPTANCE',
        'INTEGRATION',
        'EVALUATION'
    )),
    PRIMARY KEY (manifest_id, source_ordinal, required_for_ordinal),
    FOREIGN KEY (manifest_id, source_ordinal)
        REFERENCES context_manifest_source (manifest_id, source_ordinal)
);
INSERT INTO state_schema_version (version, migration_name)
VALUES (6, 'context_manifest');",
};
