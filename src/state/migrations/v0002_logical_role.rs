//! Migration 0002: durable LogicalRole identity persistence.
//!
//! Creates only the structures required by the frozen `LogicalRole`
//! contract: one row per durable role, plus an ordered child table for
//! `ownership_paths`. No executor-binding, lease, event, context, epoch,
//! entitlement, graph, task, review, or evidence structures belong here.
//!
//! The table-level CHECK constraints mirror the contract-level validation
//! (`role_type` and `status` enumerations, non-negative context epoch) so
//! the storage itself remains the durable backstop even against writes that
//! bypass the typed repository layer.

use super::Migration;

const VERSION: u32 = 2;
const NAME: &str = "logical_role";

/// The registered second migration.
pub(crate) const MIGRATION: Migration = Migration {
    version: VERSION,
    name: NAME,
    sql: "CREATE TABLE logical_role (
    role_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL,
    role_type TEXT NOT NULL CHECK (role_type IN ('RUNTIME_A1', 'RUNTIME_A2')),
    status TEXT NOT NULL CHECK (status IN ('ACTIVE', 'SUSPENDED', 'RETIRED')),
    current_context_epoch INTEGER NOT NULL CHECK (current_context_epoch >= 0),
    name TEXT,
    workstream_id TEXT,
    integration_branch TEXT,
    context_manifest_id TEXT,
    active_binding_id TEXT,
    created_at TEXT
);
CREATE TABLE logical_role_ownership_path (
    role_id TEXT NOT NULL REFERENCES logical_role (role_id),
    position INTEGER NOT NULL CHECK (position >= 0),
    path TEXT NOT NULL,
    PRIMARY KEY (role_id, position)
);
INSERT INTO state_schema_version (version, migration_name)
VALUES (2, 'logical_role');",
};
