//! Bind atomic graph publication attachment membership without changing graph v1 digests.
use super::Migration;
use crate::{GraphPhase, StateError, graph_repository as graph};
use rusqlite::{Connection, params};
pub(crate) const SQL: &str = "CREATE TABLE graph_publication_identity (
    project_id TEXT NOT NULL,
    graph_id TEXT NOT NULL,
    mutation_id TEXT NOT NULL,
    attachments_sha256 TEXT NOT NULL CHECK (length(attachments_sha256) = 64 AND attachments_sha256 NOT GLOB '*[^0-9a-f]*'),
    PRIMARY KEY (project_id, graph_id, mutation_id),
    FOREIGN KEY (project_id, graph_id, mutation_id) REFERENCES graph_mutations(project_id, graph_id, mutation_id)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER graph_publication_identity_no_update BEFORE UPDATE ON graph_publication_identity BEGIN SELECT RAISE(ABORT,'immutable graph publication'); END;
CREATE TRIGGER graph_publication_identity_no_delete BEFORE DELETE ON graph_publication_identity BEGIN SELECT RAISE(ABORT,'immutable graph publication'); END;
CREATE TRIGGER graph_publication_identity_no_replace BEFORE INSERT ON graph_publication_identity WHEN EXISTS (SELECT 1 FROM graph_publication_identity WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND mutation_id=NEW.mutation_id) BEGIN SELECT RAISE(ABORT,'duplicate graph publication'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES (23,'graph_publication_identity');";
pub(crate) const MIGRATION: Migration = Migration {
    version: 23,
    name: "graph_publication_identity",
    sql: SQL,
};
const TABLES: [&str; 1] = ["graph_publication_identity"];
pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    graph::migrate_step(conn, |conn| {
        graph::verify_v22(conn, GraphPhase::Migration)?;
        conn.execute_batch(SQL)
            .map_err(|e| graph::sqlite_error(GraphPhase::Migration, e))?;
        graph::verify_current(conn, GraphPhase::Migration)
    })
}
/// Exact v23 objects: every CREATE statement stored byte-for-byte, no extra
/// trigger or index on the new tables, every new table `WITHOUT ROWID`.
pub(crate) fn objects_match(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let (mut expected_triggers, mut expected_indexes) = (0, 0);
    for statement in SQL.split(";\n") {
        let statement = statement.trim();
        let mut words = statement.split_whitespace();
        let (Some("CREATE"), Some(mut kind)) = (words.next(), words.next()) else {
            continue;
        };
        if kind == "UNIQUE" {
            kind = words.next().unwrap_or_default();
        }
        let Some(name) = words.next() else {
            return Ok(false);
        };
        match kind {
            "TRIGGER" => expected_triggers += 1,
            "INDEX" => expected_indexes += 1,
            _ => {}
        }
        let stored: Option<String> = rusqlite::OptionalExtension::optional(conn.query_row(
            "SELECT sql FROM sqlite_master WHERE type=?1 AND name=?2",
            params![kind.to_ascii_lowercase(), name],
            |r| r.get(0),
        ))?;
        if stored.as_deref() != Some(statement) {
            return Ok(false);
        }
    }
    let (mut triggers, mut indexes) = (0i64, 0i64);
    for table in TABLES {
        triggers += conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND tbl_name=?1",
            [table],
            |r| r.get::<_, i64>(0),
        )?;
        indexes += conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND tbl_name=?1 AND sql IS NOT NULL",
            [table],
            |r| r.get::<_, i64>(0),
        )?;
        let wr: i64 = conn.query_row(
            "SELECT wr FROM pragma_table_list WHERE schema='main' AND name=?1",
            [table],
            |r| r.get(0),
        )?;
        if wr != 1 {
            return Ok(false);
        }
    }
    Ok(triggers == expected_triggers && indexes == expected_indexes)
}
