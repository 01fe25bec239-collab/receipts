//! Version barrier: v25 writers must not bypass deletion retention/serialization.
use super::Migration;
use crate::{GraphPhase, StateError, graph_repository as graph};
use rusqlite::Connection;
pub(crate) const MIGRATION: Migration = Migration {
    version: 26,
    name: "remote_workflow",
    sql: "INSERT INTO state_schema_version(version,migration_name) VALUES (26,'remote_workflow');",
};
pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    graph::migrate_step(conn, |conn| {
        graph::verify_v25(conn, GraphPhase::Migration)?;
        conn.execute_batch(MIGRATION.sql)
            .map_err(|e| graph::sqlite_error(GraphPhase::Migration, e))?;
        graph::verify_current(conn, GraphPhase::Migration)
    })
}
