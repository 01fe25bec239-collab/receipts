//! Internal goal reservations and finalization receipts; frozen wire schemas are unchanged.
use super::Migration;
use crate::{GraphPhase, StateError, graph_repository as graph};
use rusqlite::Connection;

pub(crate) const MIGRATION: Migration = Migration {
    version: 24,
    name: "wave3_setup",
    sql: SQL,
};
pub(crate) const SQL: &str = r#"CREATE TABLE goal_history (
    project_id TEXT NOT NULL,
    goal_id TEXT NOT NULL,
    revision TEXT NOT NULL CHECK (instr(revision,char(0))=0 AND (revision='0' OR (length(revision)>0 AND substr(revision,1,1) BETWEEN '1' AND '9' AND revision NOT GLOB '*[^0-9]*'))),
    record_json TEXT NOT NULL,
    PRIMARY KEY (project_id,goal_id,revision)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER goal_history_no_update BEFORE UPDATE ON goal_history BEGIN SELECT RAISE(ABORT,'immutable setup record'); END;
CREATE TRIGGER goal_history_no_delete BEFORE DELETE ON goal_history BEGIN SELECT RAISE(ABORT,'immutable setup record'); END;
CREATE TRIGGER goal_history_no_replace BEFORE INSERT ON goal_history WHEN EXISTS (SELECT 1 FROM goal_history WHERE project_id=NEW.project_id AND goal_id=NEW.goal_id AND revision=NEW.revision) BEGIN SELECT RAISE(ABORT,'duplicate setup record'); END;
CREATE TABLE goal_dispatch (
    project_id TEXT NOT NULL,
    dispatch_id TEXT NOT NULL,
    goal_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL UNIQUE,
    request_json TEXT NOT NULL,
    PRIMARY KEY (project_id,dispatch_id),
    FOREIGN KEY (attempt_id) REFERENCES attempt_started(attempt_id)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER goal_dispatch_no_update BEFORE UPDATE ON goal_dispatch BEGIN SELECT RAISE(ABORT,'immutable setup record'); END;
CREATE TRIGGER goal_dispatch_no_delete BEFORE DELETE ON goal_dispatch BEGIN SELECT RAISE(ABORT,'immutable setup record'); END;
CREATE TRIGGER goal_dispatch_no_replace BEFORE INSERT ON goal_dispatch WHEN EXISTS (SELECT 1 FROM goal_dispatch WHERE project_id=NEW.project_id AND dispatch_id=NEW.dispatch_id) BEGIN SELECT RAISE(ABORT,'duplicate setup record'); END;
CREATE TABLE goal_dispatch_resolution (
    project_id TEXT NOT NULL,
    dispatch_id TEXT NOT NULL,
    resolution TEXT NOT NULL CHECK (resolution IN ('LAUNCHED','NOT_LAUNCHED','UNCERTAIN')),
    evidence_ref TEXT NOT NULL CHECK (length(CAST(evidence_ref AS BLOB))>0),
    PRIMARY KEY (project_id,dispatch_id),
    FOREIGN KEY (project_id,dispatch_id) REFERENCES goal_dispatch(project_id,dispatch_id)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER goal_dispatch_resolution_no_update BEFORE UPDATE ON goal_dispatch_resolution BEGIN SELECT RAISE(ABORT,'immutable setup record'); END;
CREATE TRIGGER goal_dispatch_resolution_no_delete BEFORE DELETE ON goal_dispatch_resolution BEGIN SELECT RAISE(ABORT,'immutable setup record'); END;
CREATE TRIGGER goal_dispatch_resolution_no_replace BEFORE INSERT ON goal_dispatch_resolution WHEN EXISTS (SELECT 1 FROM goal_dispatch_resolution WHERE project_id=NEW.project_id AND dispatch_id=NEW.dispatch_id) BEGIN SELECT RAISE(ABORT,'duplicate setup record'); END;
CREATE TABLE attempt_finalization (
    project_id TEXT NOT NULL,
    attempt_id TEXT NOT NULL,
    record_json TEXT NOT NULL,
    finish_json TEXT NOT NULL,
    PRIMARY KEY (project_id,attempt_id),
    UNIQUE (attempt_id),
    FOREIGN KEY (attempt_id) REFERENCES attempt_finished(attempt_id)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER attempt_finalization_no_update BEFORE UPDATE ON attempt_finalization BEGIN SELECT RAISE(ABORT,'immutable setup record'); END;
CREATE TRIGGER attempt_finalization_no_delete BEFORE DELETE ON attempt_finalization BEGIN SELECT RAISE(ABORT,'immutable setup record'); END;
CREATE TRIGGER attempt_finalization_no_replace BEFORE INSERT ON attempt_finalization WHEN EXISTS (SELECT 1 FROM attempt_finalization WHERE project_id=NEW.project_id AND attempt_id=NEW.attempt_id) BEGIN SELECT RAISE(ABORT,'duplicate setup record'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES (24,'wave3_setup');"#;

pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    graph::migrate_step(conn, |conn| {
        graph::verify_v23(conn, GraphPhase::Migration)?;
        conn.execute_batch(SQL)
            .map_err(|e| graph::sqlite_error(GraphPhase::Migration, e))?;
        graph::verify_v24(conn, GraphPhase::Migration)
    })
}
pub(crate) fn objects_match(conn: &Connection) -> Result<bool, rusqlite::Error> {
    super::v0023_graph_publication_identity::objects_match_definition(
        conn,
        SQL,
        &[
            "goal_history",
            "goal_dispatch",
            "goal_dispatch_resolution",
            "attempt_finalization",
        ],
    )
}
