//! Migration 0005: single-active-binding persistence guard.
//!
//! Adds exactly one database object: a partial unique index over
//! `executor_binding(role_id)`. The index is the durable storage backstop for
//! the invariant that at most one *not conclusively fully released* binding
//! may exist per LogicalRole at any time, so two executors can never both
//! hold durable write authority over one role's history.
//!
//! The index predicate is deliberately fail-closed: a row is
//! uniqueness-blocking unless its durable terminal pair is complete
//! (`released_at IS NOT NULL AND release_reason IS NOT NULL`). Both corrupt
//! partial terminal shapes — `released_at` recorded without
//! `release_reason`, and `release_reason` recorded without `released_at` —
//! remain inside the uniqueness set, so corrupt terminal history can never
//! make a role look safely rebindable. The predicate never consults
//! `lease_expires_at`: a lease that merely *looks* old or expired does not
//! release a binding, because this storage layer renders no wall-clock
//! verdicts.
//!
//! No trigger is used, and no table, column, view, or active-binding pointer
//! is added. The migration performs no history repair: if preexisting data
//! already violates the new constraint, index creation fails and the
//! conflicting durable evidence is left exactly as it was for later
//! authorized recovery.

use super::Migration;

const VERSION: u32 = 5;
const NAME: &str = "single_active_binding";

/// The registered fifth migration.
pub(crate) const MIGRATION: Migration = Migration {
    version: VERSION,
    name: NAME,
    sql: "CREATE UNIQUE INDEX idx_executor_binding_role_unreleased
ON executor_binding (role_id)
WHERE released_at IS NULL OR release_reason IS NULL;
INSERT INTO state_schema_version (version, migration_name)
VALUES (5, 'single_active_binding');",
};
