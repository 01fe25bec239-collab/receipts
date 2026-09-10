//! Independent install lineage. Each entry creates one table and records one step.
mod v0001_schema_foundation;
mod v0002_activation_state;
mod v0003_entitlement_cache;

use rusqlite::{Connection, TransactionBehavior};

use super::install_store::InstallEntitlementStoreError as Error;

const STEPS: [(&str, &str, &str); 3] = [
    (
        "schema_foundation",
        "install_entitlement_schema_version",
        v0001_schema_foundation::SQL,
    ),
    (
        "activation_state",
        "install_activation_state",
        v0002_activation_state::SQL,
    ),
    (
        "entitlement_cache",
        "install_entitlement_cache",
        v0003_entitlement_cache::SQL,
    ),
];

/// Recognize only our exact historical DDL and contiguous migration ledger.
/// This also rejects extra tables, views, indexes and triggers before any writes.
pub(super) fn inspect(conn: &Connection) -> Result<usize, Error> {
    let objects = conn.prepare(
        "SELECT type, name, sql FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY name",
    )?.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    if objects.is_empty() {
        return Ok(0);
    }
    if !objects.iter().any(|(_, name, _)| name == STEPS[0].1) {
        return Err(Error::UnrecognizedSchema);
    }
    let versions = conn.prepare("SELECT version, migration_name FROM install_entitlement_schema_version ORDER BY version")?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>().map_err(|_| Error::UnrecognizedSchema)?;
    let version = versions.len();
    if !(1..=STEPS.len()).contains(&version) || objects.len() != version {
        return Err(Error::UnrecognizedSchema);
    }
    for (index, (number, name)) in versions.iter().enumerate() {
        let step = STEPS[index];
        if *number != (index + 1) as i64
            || name != step.0
            || !objects
                .iter()
                .any(|(kind, name, sql)| kind == "table" && name == step.1 && sql == step.2)
        {
            return Err(Error::UnrecognizedSchema);
        }
    }
    Ok(version)
}

pub(super) fn reconcile(conn: &mut Connection) -> Result<(), Error> {
    loop {
        // Reinspect under the write lock: concurrent openers cannot apply stale steps.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version = inspect(&tx)?;
        if version == STEPS.len() {
            return Ok(());
        }
        apply_step(&tx, version)?;
        if inspect(&tx)? != version + 1 {
            return Err(Error::UnrecognizedSchema);
        }
        tx.commit()?;
    }
}

// The caller owns the transaction: DDL and ledger advancement are inseparable.
pub(super) fn apply_step(tx: &rusqlite::Transaction<'_>, version: usize) -> Result<(), Error> {
    let (name, _, sql) = STEPS[version];
    tx.execute_batch(sql)?;
    tx.execute(
        "INSERT INTO install_entitlement_schema_version VALUES (?1, ?2)",
        rusqlite::params![(version + 1) as i64, name],
    )?;
    Ok(())
}
