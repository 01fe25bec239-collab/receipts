//! Migration 0010: one monotonic trusted-time watermark per project.

use super::Migration;

pub(crate) const MIGRATION: Migration = Migration {
    version: 10,
    name: "trusted_time_watermark",
    sql: "CREATE TABLE trusted_time_watermark (
    project_id TEXT NOT NULL PRIMARY KEY,
    clock_source_id TEXT NOT NULL,
    clock_contract_version TEXT NOT NULL,
    last_accepted_trusted_time TEXT NOT NULL
);
INSERT INTO state_schema_version (version, migration_name)
VALUES (10, 'trusted_time_watermark');",
};
