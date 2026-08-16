//! Migration 0007: durable ContextEpoch history persistence.
//!
//! Creates exactly one table: the project-scoped, immutable `context_epoch`
//! history. Each row records that a project's context epoch reached a
//! number, the opaque timestamp supplied for that transition, and the frozen
//! closed-set trigger category that caused it. The composite primary key
//! `(project_id, epoch)` makes history append-only at the storage layer:
//! duplicates are refused, there is no update/delete path, and there is no
//! mutable `current_epoch` pointer anywhere — the latest epoch is always
//! derived from the history by numeric `MAX(epoch)`.
//!
//! The table-level CHECK constraints mirror the closed contract
//! enumerations: `epoch` must be non-negative and `trigger` must be exactly
//! one of the fifteen frozen rehydration-architecture triggers, so the
//! storage itself remains the durable backstop even against writes that
//! bypass the typed repository layer. No `changed_sources`,
//! `invalidated_role_ids`, source-content, digest, reconciliation, or
//! current-epoch structures belong here: those are later, separately
//! authorized lifecycle slices. `project_id` is opaque structural metadata
//! with no project table or foreign key, and `advanced_at` is stored
//! exactly as provided — State never parses, normalizes, compares, or
//! generates timestamps. No foreign key to `context_manifest` exists:
//! manifests carry epoch snapshots, and pre-existing v6 manifests may
//! legitimately reference epochs whose history rows do not exist (yet).
//! The composite primary key's leading `project_id` already supports
//! project-scoped epoch lookup and ordering, so no additional index is
//! created.

use super::Migration;

const VERSION: u32 = 7;
const NAME: &str = "context_epoch";

/// The registered seventh migration.
pub(crate) const MIGRATION: Migration = Migration {
    version: VERSION,
    name: NAME,
    sql: "CREATE TABLE context_epoch (
    project_id TEXT NOT NULL,
    epoch INTEGER NOT NULL CHECK (epoch >= 0),
    advanced_at TEXT NOT NULL,
    trigger TEXT NOT NULL CHECK (trigger IN (
        'A1_INIT',
        'A2_INIT',
        'MODEL_REPLACEMENT',
        'PROVIDER_REPLACEMENT',
        'HOST_SWITCH',
        'CONTEXT_COMPACTION',
        'ARCHITECTURE_CHANGE',
        'CONTRACT_CHANGE',
        'NEW_WAVE',
        'TASK_THRESHOLD',
        'SERIOUS_A4_REJECTION',
        'SECURITY_ESCALATION',
        'BEFORE_A2_INTEGRATION',
        'BEFORE_A1_INTEGRATION',
        'BEFORE_GOAL_COMPLETE'
    )),
    PRIMARY KEY (project_id, epoch)
);
INSERT INTO state_schema_version (version, migration_name)
VALUES (7, 'context_epoch');",
};
