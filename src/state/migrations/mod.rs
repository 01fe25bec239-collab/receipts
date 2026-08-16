//! Repository-owned, versioned, forward-only schema migrations.
//!
//! Migrations live under `src/state/migrations/**`, are registered in a
//! single deterministic chain, and may only move a database forward. There is
//! no downgrade mechanism. The recorded schema-version representation is a
//! repository implementation detail and is not a cross-manager contract.

mod v0001_schema_foundation;
mod v0002_logical_role;
mod v0003_executor_binding;
mod v0004_event;
mod v0005_single_active_binding;

use crate::error::StateError;

/// One forward-only migration.
#[derive(Clone, Copy)]
pub(crate) struct Migration {
    /// Explicit, monotonically increasing schema version produced by this
    /// migration.
    pub(crate) version: u32,
    /// Stable human-readable migration name.
    pub(crate) name: &'static str,
    /// SQL applied atomically by this migration.
    pub(crate) sql: &'static str,
}

/// The registered migration chain, in deterministic application order.
static REGISTERED: &[Migration] = &[
    v0001_schema_foundation::MIGRATION,
    v0002_logical_role::MIGRATION,
    v0003_executor_binding::MIGRATION,
    v0004_event::MIGRATION,
    v0005_single_active_binding::MIGRATION,
];

/// The registered migration chain in application order.
pub(crate) fn registered() -> &'static [Migration] {
    REGISTERED
}

/// Validates that a chain is non-empty, starts at version 1, and increases
/// contiguously, so application order is fully deterministic.
pub(crate) fn validate_chain(chain: &[Migration]) -> Result<(), StateError> {
    let Some(first) = chain.first() else {
        return Err(StateError::MigrationChainInvalid {
            detail: "the migration chain is empty".to_string(),
        });
    };
    if first.version != 1 {
        return Err(StateError::MigrationChainInvalid {
            detail: format!(
                "the first migration must be version 1, found {}",
                first.version
            ),
        });
    }
    for (previous, current) in chain.iter().zip(chain.iter().skip(1)) {
        if current.version != previous.version + 1 {
            return Err(StateError::MigrationChainInvalid {
                detail: format!(
                    "migration versions must increase contiguously, but {} is followed by {}",
                    previous.version, current.version
                ),
            });
        }
    }
    for migration in chain {
        if migration.sql.trim().is_empty() {
            return Err(StateError::MigrationChainInvalid {
                detail: format!("migration {} has empty SQL", migration.version),
            });
        }
    }
    Ok(())
}
