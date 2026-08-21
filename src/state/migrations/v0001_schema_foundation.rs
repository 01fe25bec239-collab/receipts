//! Migration 0001: durable schema-version metadata foundation.
//!
//! This first migration creates only what the repository itself needs: the
//! table that durably records the applied schema version. No domain tables
//! (roles, bindings, events, contexts, entitlements, ...) belong here.

use super::Migration;

const VERSION: u32 = 1;
const NAME: &str = "schema_foundation";

/// The registered first migration.
pub(crate) const MIGRATION: Migration = Migration {
    version: VERSION,
    name: NAME,
    sql: "CREATE TABLE state_schema_version (
    version INTEGER NOT NULL CHECK (version > 0),
    migration_name TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
INSERT INTO state_schema_version (version, migration_name)
VALUES (1, 'schema_foundation');",
};
