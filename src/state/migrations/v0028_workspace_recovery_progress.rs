//! State-ordered workspace baselines; legacy history is never backfilled.
use super::Migration;
use crate::{GraphPhase, StateError, graph_repository as graph};
use rusqlite::Connection;
pub(crate) const MIGRATION: Migration = Migration {
    version: 28,
    name: "workspace_recovery_progress",
    sql: SQL,
};
pub(crate) const SQL: &str = r#"CREATE TABLE workspace_recovery_progress (project_id TEXT NOT NULL, workspace_id TEXT NOT NULL, sequence INTEGER NOT NULL CHECK(sequence>0), kind TEXT NOT NULL CHECK(kind IN('RESET','START','ACCEPT')), baseline_sequence INTEGER NOT NULL CHECK(baseline_sequence>=0 AND baseline_sequence<sequence), attempt_id TEXT NOT NULL REFERENCES attempt_started(attempt_id), anchor_id TEXT, recovery_id TEXT, CHECK((kind='START' AND anchor_id IS NULL AND recovery_id IS NULL) OR (kind='ACCEPT' AND anchor_id IS NOT NULL AND recovery_id IS NULL) OR (kind='RESET' AND anchor_id IS NOT NULL AND recovery_id IS NOT NULL)), PRIMARY KEY(project_id,workspace_id,sequence)) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX workspace_recovery_progress_start ON workspace_recovery_progress(attempt_id) WHERE kind='START';
CREATE UNIQUE INDEX workspace_recovery_progress_reset ON workspace_recovery_progress(project_id,recovery_id) WHERE kind='RESET';
CREATE TRIGGER workspace_recovery_progress_no_update BEFORE UPDATE ON workspace_recovery_progress BEGIN SELECT RAISE(ABORT,'immutable workspace progress'); END;
CREATE TRIGGER workspace_recovery_progress_no_delete BEFORE DELETE ON workspace_recovery_progress BEGIN SELECT RAISE(ABORT,'immutable workspace progress'); END;
CREATE TRIGGER workspace_recovery_progress_no_replace BEFORE INSERT ON workspace_recovery_progress WHEN EXISTS(SELECT 1 FROM workspace_recovery_progress WHERE project_id=NEW.project_id AND workspace_id=NEW.workspace_id AND sequence=NEW.sequence) BEGIN SELECT RAISE(ABORT,'duplicate workspace progress'); END;
CREATE TRIGGER workspace_recovery_progress_order BEFORE INSERT ON workspace_recovery_progress WHEN NEW.sequence<>(SELECT COALESCE(MAX(sequence),0)+1 FROM workspace_recovery_progress WHERE project_id=NEW.project_id AND workspace_id=NEW.workspace_id) OR NEW.baseline_sequence<>(SELECT COALESCE(MAX(sequence),0) FROM workspace_recovery_progress WHERE project_id=NEW.project_id AND workspace_id=NEW.workspace_id AND kind IN('RESET','ACCEPT')) BEGIN SELECT RAISE(ABORT,'stale workspace progress'); END;
CREATE TRIGGER workspace_recovery_progress_subject BEFORE INSERT ON workspace_recovery_progress WHEN NOT EXISTS(SELECT 1 FROM attempt_started WHERE attempt_id=NEW.attempt_id AND project_id=NEW.project_id AND workspace_id=NEW.workspace_id) OR (NEW.kind IN('RESET','ACCEPT') AND NOT EXISTS(SELECT 1 FROM workspace_recovery_acceptance WHERE project_id=NEW.project_id AND workspace_id=NEW.workspace_id AND anchor_id=NEW.anchor_id)) OR (NEW.kind='ACCEPT' AND NOT EXISTS(SELECT 1 FROM workspace_recovery_progress WHERE project_id=NEW.project_id AND workspace_id=NEW.workspace_id AND kind='START' AND attempt_id=NEW.attempt_id AND baseline_sequence=NEW.baseline_sequence)) OR (NEW.kind='RESET' AND NOT EXISTS(SELECT 1 FROM workspace_recovery_resolution WHERE project_id=NEW.project_id AND attempt_id=NEW.attempt_id AND recovery_id=NEW.recovery_id)) BEGIN SELECT RAISE(ABORT,'unbound workspace progress'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES(28,'workspace_recovery_progress');"#;
/// Preserve v27's exact SQL while retiring its current-head check.
pub(crate) fn through_v27(conn: &Connection) -> Result<(), StateError> {
    graph::migrate_step(conn, |conn| {
        graph::verify_v26(conn, GraphPhase::Migration)?;
        conn.execute_batch(super::v0027_workspace_recovery::MIGRATION.sql)
            .map_err(|e| graph::sqlite_error(GraphPhase::Migration, e))?;
        graph::verify_v27(conn, GraphPhase::Migration)
    })
}
pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    graph::migrate_step(conn, |conn| {
        graph::verify_v27(conn, GraphPhase::Migration)?;
        conn.execute_batch(SQL)
            .map_err(|e| graph::sqlite_error(GraphPhase::Migration, e))?;
        graph::verify_current(conn, GraphPhase::Migration)
    })
}
pub(crate) fn objects_match(conn: &Connection) -> Result<bool, rusqlite::Error> {
    super::v0023_graph_publication_identity::objects_match_definition(
        conn,
        SQL,
        &["workspace_recovery_progress"],
    )
}
