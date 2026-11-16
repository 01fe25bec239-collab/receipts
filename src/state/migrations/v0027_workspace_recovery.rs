//! Internal D4 custody and recovery records; no frozen wire changes.
use super::Migration;
use crate::{GraphPhase, StateError, graph_repository as graph};
use rusqlite::Connection;
pub(crate) const MIGRATION: Migration = Migration {
    version: 27,
    name: "workspace_recovery",
    sql: SQL,
};
pub(crate) const SQL: &str = r#"CREATE TABLE workspace_recovery_acceptance (project_id TEXT NOT NULL, anchor_id TEXT NOT NULL, workspace_id TEXT NOT NULL, record_json TEXT NOT NULL, PRIMARY KEY(project_id,anchor_id)) STRICT, WITHOUT ROWID;
CREATE TRIGGER workspace_recovery_acceptance_no_update BEFORE UPDATE ON workspace_recovery_acceptance BEGIN SELECT RAISE(ABORT,'immutable workspace recovery evidence'); END;
CREATE TRIGGER workspace_recovery_acceptance_no_delete BEFORE DELETE ON workspace_recovery_acceptance BEGIN SELECT RAISE(ABORT,'immutable workspace recovery evidence'); END;
CREATE TRIGGER workspace_recovery_acceptance_no_replace BEFORE INSERT ON workspace_recovery_acceptance WHEN EXISTS(SELECT 1 FROM workspace_recovery_acceptance WHERE project_id=NEW.project_id AND anchor_id=NEW.anchor_id) BEGIN SELECT RAISE(ABORT,'duplicate workspace recovery evidence'); END;
CREATE TABLE workspace_recovery_admission (project_id TEXT NOT NULL, attempt_id TEXT NOT NULL, workspace_id TEXT NOT NULL, role_id TEXT NOT NULL, binding_id TEXT NOT NULL, task_id TEXT NOT NULL, record_json TEXT NOT NULL, PRIMARY KEY(project_id,attempt_id), UNIQUE(attempt_id), FOREIGN KEY(attempt_id) REFERENCES attempt_started(attempt_id)) STRICT, WITHOUT ROWID;
CREATE TRIGGER workspace_recovery_admission_no_update BEFORE UPDATE ON workspace_recovery_admission BEGIN SELECT RAISE(ABORT,'immutable workspace recovery evidence'); END;
CREATE TRIGGER workspace_recovery_admission_no_delete BEFORE DELETE ON workspace_recovery_admission BEGIN SELECT RAISE(ABORT,'immutable workspace recovery evidence'); END;
CREATE TRIGGER workspace_recovery_admission_no_replace BEFORE INSERT ON workspace_recovery_admission WHEN EXISTS(SELECT 1 FROM workspace_recovery_admission WHERE project_id=NEW.project_id AND attempt_id=NEW.attempt_id) BEGIN SELECT RAISE(ABORT,'duplicate workspace recovery evidence'); END;
CREATE TABLE workspace_recovery_handoff (project_id TEXT NOT NULL, attempt_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), disposition TEXT NOT NULL CHECK(disposition IN ('PENDING','ELIGIBLE','CLOSED_WITHOUT_HANDOFF')), record_json TEXT NOT NULL, PRIMARY KEY(project_id,attempt_id,revision), FOREIGN KEY(project_id,attempt_id) REFERENCES workspace_recovery_admission(project_id,attempt_id)) STRICT, WITHOUT ROWID;
CREATE TRIGGER workspace_recovery_handoff_no_update BEFORE UPDATE ON workspace_recovery_handoff BEGIN SELECT RAISE(ABORT,'immutable workspace recovery evidence'); END;
CREATE TRIGGER workspace_recovery_handoff_no_delete BEFORE DELETE ON workspace_recovery_handoff BEGIN SELECT RAISE(ABORT,'immutable workspace recovery evidence'); END;
CREATE TRIGGER workspace_recovery_handoff_no_replace BEFORE INSERT ON workspace_recovery_handoff WHEN EXISTS(SELECT 1 FROM workspace_recovery_handoff WHERE project_id=NEW.project_id AND attempt_id=NEW.attempt_id AND revision=NEW.revision) BEGIN SELECT RAISE(ABORT,'duplicate workspace recovery evidence'); END;
CREATE TABLE workspace_recovery_control (project_id TEXT NOT NULL, attempt_id TEXT NOT NULL, recovery_id TEXT NOT NULL, workspace_id TEXT NOT NULL, binding_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>=0), phase TEXT NOT NULL CHECK(phase IN ('CLAIMED','CAPTURED','SIGNAL_PREPARED','QUIESCENT','RESET_PREPARED','RESOLVED')), active INTEGER NOT NULL CHECK(active IN(0,1)), record_json TEXT NOT NULL, claimed_at TEXT NOT NULL, PRIMARY KEY(project_id,recovery_id), UNIQUE(project_id,attempt_id), FOREIGN KEY(project_id,attempt_id) REFERENCES workspace_recovery_admission(project_id,attempt_id)) STRICT, WITHOUT ROWID;
CREATE TABLE workspace_recovery_evidence (project_id TEXT NOT NULL, recovery_id TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0), record_json TEXT NOT NULL, PRIMARY KEY(project_id,recovery_id,revision), FOREIGN KEY(project_id,recovery_id) REFERENCES workspace_recovery_control(project_id,recovery_id)) STRICT, WITHOUT ROWID;
CREATE TRIGGER workspace_recovery_evidence_no_update BEFORE UPDATE ON workspace_recovery_evidence BEGIN SELECT RAISE(ABORT,'immutable workspace recovery evidence'); END;
CREATE TRIGGER workspace_recovery_evidence_no_delete BEFORE DELETE ON workspace_recovery_evidence BEGIN SELECT RAISE(ABORT,'immutable workspace recovery evidence'); END;
CREATE TRIGGER workspace_recovery_evidence_no_replace BEFORE INSERT ON workspace_recovery_evidence WHEN EXISTS(SELECT 1 FROM workspace_recovery_evidence WHERE project_id=NEW.project_id AND recovery_id=NEW.recovery_id AND revision=NEW.revision) BEGIN SELECT RAISE(ABORT,'duplicate workspace recovery evidence'); END;
CREATE TABLE workspace_recovery_resolution (project_id TEXT NOT NULL, attempt_id TEXT NOT NULL, recovery_id TEXT NOT NULL, record_json TEXT NOT NULL, PRIMARY KEY(project_id,attempt_id), UNIQUE(project_id,recovery_id), FOREIGN KEY(project_id,recovery_id) REFERENCES workspace_recovery_control(project_id,recovery_id)) STRICT, WITHOUT ROWID;
CREATE TRIGGER workspace_recovery_resolution_no_update BEFORE UPDATE ON workspace_recovery_resolution BEGIN SELECT RAISE(ABORT,'immutable workspace recovery evidence'); END;
CREATE TRIGGER workspace_recovery_resolution_no_delete BEFORE DELETE ON workspace_recovery_resolution BEGIN SELECT RAISE(ABORT,'immutable workspace recovery evidence'); END;
CREATE TRIGGER workspace_recovery_resolution_no_replace BEFORE INSERT ON workspace_recovery_resolution WHEN EXISTS(SELECT 1 FROM workspace_recovery_resolution WHERE project_id=NEW.project_id AND attempt_id=NEW.attempt_id) BEGIN SELECT RAISE(ABORT,'duplicate workspace recovery evidence'); END;
CREATE UNIQUE INDEX workspace_recovery_workspace_active ON workspace_recovery_control(project_id,workspace_id) WHERE active=1;
CREATE UNIQUE INDEX workspace_recovery_binding_active ON workspace_recovery_control(binding_id) WHERE active=1;
CREATE TRIGGER workspace_recovery_control_no_delete BEFORE DELETE ON workspace_recovery_control BEGIN SELECT RAISE(ABORT,'retained recovery claim'); END;
CREATE TRIGGER workspace_recovery_control_no_replace BEFORE INSERT ON workspace_recovery_control WHEN EXISTS(SELECT 1 FROM workspace_recovery_control WHERE project_id=NEW.project_id AND (recovery_id=NEW.recovery_id OR attempt_id=NEW.attempt_id)) BEGIN SELECT RAISE(ABORT,'duplicate recovery claim'); END;
CREATE TRIGGER workspace_recovery_control_initial BEFORE INSERT ON workspace_recovery_control WHEN NEW.revision<>0 OR NEW.phase<>'CLAIMED' OR NEW.active<>1 OR NOT EXISTS(SELECT 1 FROM workspace_recovery_admission WHERE project_id=NEW.project_id AND attempt_id=NEW.attempt_id AND workspace_id=NEW.workspace_id AND binding_id=NEW.binding_id) BEGIN SELECT RAISE(ABORT,'invalid initial recovery claim'); END;
CREATE TRIGGER workspace_recovery_control_update BEFORE UPDATE ON workspace_recovery_control WHEN NOT ((OLD.phase='CLAIMED' AND NEW.phase='CAPTURED') OR (OLD.phase='CAPTURED' AND NEW.phase IN('SIGNAL_PREPARED','QUIESCENT')) OR (OLD.phase='SIGNAL_PREPARED' AND NEW.phase IN('CAPTURED','QUIESCENT')) OR (OLD.phase='QUIESCENT' AND NEW.phase='RESET_PREPARED') OR (OLD.phase='RESET_PREPARED' AND NEW.phase IN('QUIESCENT','RESOLVED'))) OR OLD.active<>1 OR NEW.project_id IS NOT OLD.project_id OR NEW.attempt_id IS NOT OLD.attempt_id OR NEW.recovery_id IS NOT OLD.recovery_id OR NEW.workspace_id IS NOT OLD.workspace_id OR NEW.binding_id IS NOT OLD.binding_id OR NEW.record_json IS NOT OLD.record_json OR NEW.claimed_at IS NOT OLD.claimed_at OR NEW.revision<>OLD.revision+1 OR NOT EXISTS(SELECT 1 FROM workspace_recovery_evidence WHERE project_id=NEW.project_id AND recovery_id=NEW.recovery_id AND revision=NEW.revision) OR (NEW.active=0 AND (NEW.phase<>'RESOLVED' OR NOT EXISTS(SELECT 1 FROM workspace_recovery_resolution WHERE project_id=NEW.project_id AND attempt_id=NEW.attempt_id AND recovery_id=NEW.recovery_id))) OR (NEW.active=1 AND NEW.phase='RESOLVED') BEGIN SELECT RAISE(ABORT,'invalid recovery transition or release'); END;
CREATE TRIGGER workspace_recovery_handoff_order BEFORE INSERT ON workspace_recovery_handoff WHEN NEW.revision<>(SELECT COALESCE(MAX(revision),0)+1 FROM workspace_recovery_handoff WHERE project_id=NEW.project_id AND attempt_id=NEW.attempt_id) OR EXISTS(SELECT 1 FROM workspace_recovery_control WHERE project_id=NEW.project_id AND attempt_id=NEW.attempt_id) OR EXISTS(SELECT 1 FROM workspace_recovery_handoff WHERE project_id=NEW.project_id AND attempt_id=NEW.attempt_id AND disposition<>'PENDING') BEGIN SELECT RAISE(ABORT,'handoff is closed or under recovery'); END;
CREATE TRIGGER workspace_recovery_evidence_order BEFORE INSERT ON workspace_recovery_evidence WHEN NOT EXISTS(SELECT 1 FROM workspace_recovery_control WHERE project_id=NEW.project_id AND recovery_id=NEW.recovery_id AND active=1 AND revision+1=NEW.revision) BEGIN SELECT RAISE(ABORT,'stale recovery evidence'); END;
CREATE TRIGGER workspace_recovery_resolution_prepared BEFORE INSERT ON workspace_recovery_resolution WHEN NOT EXISTS(SELECT 1 FROM workspace_recovery_control WHERE project_id=NEW.project_id AND recovery_id=NEW.recovery_id AND attempt_id=NEW.attempt_id AND active=1 AND phase='RESET_PREPARED') BEGIN SELECT RAISE(ABORT,'reset is not prepared'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES(27,'workspace_recovery');"#;
/// Preserve the exact historical v26 bytes without its retired current-head check.
pub(crate) fn through_v26(conn: &Connection) -> Result<(), StateError> {
    graph::migrate_step(conn, |conn| {
        graph::verify_v25(conn, GraphPhase::Migration)?;
        conn.execute_batch(super::v0026_remote_workflow::MIGRATION.sql)
            .map_err(|e| graph::sqlite_error(GraphPhase::Migration, e))?;
        graph::verify_v26(conn, GraphPhase::Migration)
    })
}
pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    graph::migrate_step(conn, |conn| {
        graph::verify_v26(conn, GraphPhase::Migration)?;
        conn.execute_batch(SQL)
            .map_err(|e| graph::sqlite_error(GraphPhase::Migration, e))?;
        graph::verify_current(conn, GraphPhase::Migration)
    })
}
pub(crate) fn objects_match(conn: &Connection) -> Result<bool, rusqlite::Error> {
    super::v0023_graph_publication_identity::objects_match_definition(
        conn,
        SQL,
        &[
            "workspace_recovery_acceptance",
            "workspace_recovery_admission",
            "workspace_recovery_handoff",
            "workspace_recovery_control",
            "workspace_recovery_evidence",
            "workspace_recovery_resolution",
        ],
    )
}
