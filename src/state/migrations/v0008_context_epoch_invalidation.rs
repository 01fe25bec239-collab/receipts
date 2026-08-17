//! Migration 0008: immutable ContextEpoch invalidated-role evidence.
//!
//! Adds the minimum child relation plus the composite parent key SQLite
//! requires to enforce that each referenced LogicalRole belongs to the same
//! project as its ContextEpoch. Existing epoch history is not backfilled.

use super::Migration;

const VERSION: u32 = 8;
const NAME: &str = "context_epoch_invalidation";

pub(crate) const MIGRATION: Migration = Migration {
    version: VERSION,
    name: NAME,
    sql: "CREATE UNIQUE INDEX idx_logical_role_role_id_project_id
ON logical_role (role_id, project_id);
CREATE TABLE context_epoch_invalidated_role (
    project_id TEXT NOT NULL,
    epoch INTEGER NOT NULL CHECK (epoch >= 0),
    role_id TEXT NOT NULL,
    PRIMARY KEY (project_id, epoch, role_id),
    FOREIGN KEY (project_id, epoch)
        REFERENCES context_epoch (project_id, epoch),
    FOREIGN KEY (role_id, project_id)
        REFERENCES logical_role (role_id, project_id)
);
INSERT INTO state_schema_version (version, migration_name)
VALUES (8, 'context_epoch_invalidation');",
};
