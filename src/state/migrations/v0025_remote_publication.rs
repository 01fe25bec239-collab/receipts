//! D12 immutable intent and ordered append-only observations.
use super::Migration;
use crate::{GraphPhase, StateError, graph_repository as graph};
use rusqlite::Connection;
pub(crate) const MIGRATION: Migration = Migration {
    version: 25,
    name: "remote_publication",
    sql: SQL,
};
pub(crate) const SQL: &str = r#"CREATE TABLE remote_publication_intent (
    project_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    record_json TEXT NOT NULL,
    PRIMARY KEY (project_id,operation_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE remote_publication_result (
    project_id TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence>0),
    record_json TEXT NOT NULL,
    PRIMARY KEY (project_id,operation_id,sequence),
    FOREIGN KEY (project_id,operation_id) REFERENCES remote_publication_intent(project_id,operation_id)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER remote_publication_intent_no_update BEFORE UPDATE ON remote_publication_intent BEGIN SELECT RAISE(ABORT,'immutable remote publication'); END;
CREATE TRIGGER remote_publication_intent_no_delete BEFORE DELETE ON remote_publication_intent BEGIN SELECT RAISE(ABORT,'immutable remote publication'); END;
CREATE TRIGGER remote_publication_intent_no_replace BEFORE INSERT ON remote_publication_intent WHEN EXISTS (SELECT 1 FROM remote_publication_intent WHERE project_id=NEW.project_id AND operation_id=NEW.operation_id) BEGIN SELECT RAISE(ABORT,'duplicate remote publication'); END;
CREATE TRIGGER remote_publication_result_no_update BEFORE UPDATE ON remote_publication_result BEGIN SELECT RAISE(ABORT,'immutable remote publication'); END;
CREATE TRIGGER remote_publication_result_no_delete BEFORE DELETE ON remote_publication_result BEGIN SELECT RAISE(ABORT,'immutable remote publication'); END;
CREATE TRIGGER remote_publication_result_no_replace BEFORE INSERT ON remote_publication_result WHEN EXISTS (SELECT 1 FROM remote_publication_result WHERE project_id=NEW.project_id AND operation_id=NEW.operation_id AND sequence=NEW.sequence) BEGIN SELECT RAISE(ABORT,'duplicate remote publication'); END;
CREATE TRIGGER remote_publication_result_order BEFORE INSERT ON remote_publication_result WHEN NEW.sequence != (SELECT COALESCE(MAX(sequence),0)+1 FROM remote_publication_result WHERE project_id=NEW.project_id AND operation_id=NEW.operation_id) BEGIN SELECT RAISE(ABORT,'publication sequence gap'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES (25,'remote_publication');"#;
pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    graph::migrate_step(conn, |conn| {
        graph::verify_v24(conn, GraphPhase::Migration)?;
        conn.execute_batch(SQL)
            .map_err(|e| graph::sqlite_error(GraphPhase::Migration, e))?;
        graph::verify_current(conn, GraphPhase::Migration)
    })
}
pub(crate) fn objects_match(conn: &Connection) -> Result<bool, rusqlite::Error> {
    super::v0023_graph_publication_identity::objects_match_definition(
        conn,
        SQL,
        &["remote_publication_intent", "remote_publication_result"],
    )
}
