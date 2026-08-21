//! Migration 0003: durable ExecutorBinding persistence.
//!
//! Creates only the structure required by the frozen `ExecutorBinding`
//! contract: one row per binding, with a foreign key backstop from
//! `role_id` to the durable `logical_role` identity. No lease-scheduler,
//! event, context, epoch, entitlement, graph, task, review, evidence,
//! routing-decision, provider, model, host-session, workspace, or recovery
//! structures belong here.
//!
//! The table-level CHECK constraint mirrors the closed `release_reason`
//! enumeration of the contract so the storage itself remains the durable
//! backstop even against writes that bypass the typed repository layer.
//! Identifier non-emptiness and length limits are enforced at the typed
//! boundary, exactly as for LogicalRole identifiers in migration 0002.
//! `bound_at`, `lease_expires_at`, and `released_at` are stored as provided:
//! State does not parse, compare, or evaluate them.

use super::Migration;

const VERSION: u32 = 3;
const NAME: &str = "executor_binding";

/// The registered third migration.
pub(crate) const MIGRATION: Migration = Migration {
    version: VERSION,
    name: NAME,
    sql: "CREATE TABLE executor_binding (
    binding_id TEXT NOT NULL PRIMARY KEY,
    role_id TEXT NOT NULL REFERENCES logical_role (role_id),
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    runtime_id TEXT NOT NULL,
    session_ref TEXT,
    routing_decision_id TEXT,
    bound_at TEXT NOT NULL,
    lease_expires_at TEXT NOT NULL,
    released_at TEXT,
    release_reason TEXT CHECK (
        release_reason IS NULL
        OR release_reason IN (
            'RATE_LIMITED',
            'SESSION_EXHAUSTED',
            'AUTH_REQUIRED',
            'PROVIDER_DOWN',
            'CRASH',
            'HOST_SWITCH',
            'USER_REQUEST',
            'COMPLETED',
            'LEASE_EXPIRED'
        )
    ),
    rehydration_completed INTEGER
);
INSERT INTO state_schema_version (version, migration_name)
VALUES (3, 'executor_binding');",
};
