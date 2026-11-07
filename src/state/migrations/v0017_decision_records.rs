//! v17: immutable decision/provenance records, append-only role history and
//! immutable event rows.
//!
//! Decision records keep the exact supplied schema-shaped JSON bytes plus
//! their SHA-256 and extracted identity metadata. Provenance links are filled
//! one immutable row per link kind. Role pointer columns stay operational
//! state, but every pointer update must be paired with a fresh history row in
//! the same transaction; role identity columns and rows are immutable.
//! Event rows are immutable too: UPDATE, DELETE and any replacing INSERT
//! (`OR REPLACE`, `REPLACE`, upsert, `OR IGNORE` on an existing id) abort;
//! a plain INSERT of a new `event_id` is the only way a row appears. These are
//! direct-SQL backstops, not a defence against a caller who can drop schema
//! objects; `schema_matches` detects a missing or altered guard on open.
//! Statements are separated by `;\n`; trigger bodies must not contain `;\n`.
use super::Migration;

pub(crate) const MIGRATION: Migration = Migration {
    version: 17,
    name: "decision_records",
    sql: SQL,
};

pub(crate) const SQL: &str = r#"
CREATE TABLE decision_records (
    project_id TEXT NOT NULL COLLATE BINARY CHECK (length(CAST(project_id AS BLOB))>0),
    record_kind TEXT NOT NULL CHECK (record_kind IN ('ROUTING_DECISION','REGISTRY_CAPABILITY','MODEL_OBSERVATION','AVAILABILITY_OBSERVATION','REVIEW_REQUEST','A4_REVIEW','INTEGRATION_REQUEST','INTEGRATION_DECISION')),
    record_id TEXT NOT NULL COLLATE BINARY CHECK (length(CAST(record_id AS BLOB))>0),
    task_id TEXT COLLATE BINARY,
    link_id TEXT COLLATE BINARY,
    subject_sha TEXT CHECK (subject_sha IS NULL OR (length(CAST(subject_sha AS BLOB))=40 AND subject_sha NOT GLOB '*[^0-9a-f]*')),
    record_json TEXT NOT NULL,
    record_sha256 TEXT NOT NULL CHECK (length(CAST(record_sha256 AS BLOB))=64 AND record_sha256 NOT GLOB '*[^0-9a-f]*'),
    PRIMARY KEY (project_id,record_kind,record_id)
) STRICT;
CREATE TABLE provenance_links (
    project_id TEXT NOT NULL COLLATE BINARY CHECK (length(CAST(project_id AS BLOB))>0),
    task_id TEXT NOT NULL COLLATE BINARY CHECK (length(CAST(task_id AS BLOB))>0),
    link_kind TEXT NOT NULL CHECK (link_kind IN ('START_SHA','IMPLEMENTATION_SHA','REVIEW_SHA','VERDICT_REF','ACCEPTANCE_REF','WORKSTREAM_SHA','MAIN_SHA')),
    link_value TEXT NOT NULL COLLATE BINARY CHECK (length(CAST(link_value AS BLOB))>0),
    PRIMARY KEY (project_id,task_id,link_kind)
) STRICT;
CREATE TABLE logical_role_history (
    role_id TEXT NOT NULL COLLATE BINARY,
    project_id TEXT NOT NULL COLLATE BINARY,
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    change_kind TEXT NOT NULL CHECK (change_kind IN ('STATUS','BINDING','CONTEXT')),
    from_status TEXT NOT NULL CHECK (from_status IN ('ACTIVE','SUSPENDED','RETIRED')),
    to_status TEXT NOT NULL CHECK (to_status IN ('ACTIVE','SUSPENDED','RETIRED')),
    from_binding_id TEXT,
    to_binding_id TEXT,
    from_epoch TEXT NOT NULL,
    to_epoch TEXT NOT NULL,
    from_manifest_id TEXT,
    to_manifest_id TEXT,
    changed_at TEXT NOT NULL CHECK (length(CAST(changed_at AS BLOB))=30),
    reason TEXT CHECK (reason IS NULL OR length(CAST(reason AS BLOB))>0),
    PRIMARY KEY (role_id,sequence),
    FOREIGN KEY (role_id,project_id) REFERENCES logical_role(role_id,project_id),
    CHECK (from_status IS NOT to_status OR from_binding_id IS NOT to_binding_id OR from_epoch IS NOT to_epoch OR from_manifest_id IS NOT to_manifest_id)
) STRICT;
CREATE TRIGGER decision_records_no_update BEFORE UPDATE ON decision_records BEGIN SELECT RAISE(ABORT,'immutable decision record'); END;
CREATE TRIGGER decision_records_no_delete BEFORE DELETE ON decision_records BEGIN SELECT RAISE(ABORT,'immutable decision record'); END;
CREATE TRIGGER decision_records_no_replace BEFORE INSERT ON decision_records WHEN EXISTS (SELECT 1 FROM decision_records WHERE project_id=NEW.project_id AND record_kind=NEW.record_kind AND record_id=NEW.record_id) BEGIN SELECT RAISE(ABORT,'duplicate decision record'); END;
CREATE TRIGGER provenance_links_no_update BEFORE UPDATE ON provenance_links BEGIN SELECT RAISE(ABORT,'immutable provenance link'); END;
CREATE TRIGGER provenance_links_no_delete BEFORE DELETE ON provenance_links BEGIN SELECT RAISE(ABORT,'immutable provenance link'); END;
CREATE TRIGGER provenance_links_no_replace BEFORE INSERT ON provenance_links WHEN EXISTS (SELECT 1 FROM provenance_links WHERE project_id=NEW.project_id AND task_id=NEW.task_id AND link_kind=NEW.link_kind) BEGIN SELECT RAISE(ABORT,'duplicate provenance link'); END;
CREATE TRIGGER logical_role_history_no_update BEFORE UPDATE ON logical_role_history BEGIN SELECT RAISE(ABORT,'immutable role history'); END;
CREATE TRIGGER logical_role_history_no_delete BEFORE DELETE ON logical_role_history BEGIN SELECT RAISE(ABORT,'immutable role history'); END;
CREATE TRIGGER logical_role_history_from_current BEFORE INSERT ON logical_role_history WHEN NEW.sequence IS NOT (SELECT COALESCE(MAX(sequence),0)+1 FROM logical_role_history WHERE role_id=NEW.role_id) OR NOT EXISTS (SELECT 1 FROM logical_role r WHERE r.role_id=NEW.role_id AND r.project_id=NEW.project_id AND r.status=NEW.from_status AND r.active_binding_id IS NEW.from_binding_id AND r.current_context_epoch=NEW.from_epoch AND r.context_manifest_id IS NEW.from_manifest_id) BEGIN SELECT RAISE(ABORT,'role history must start from current role'); END;
CREATE TRIGGER logical_role_no_delete BEFORE DELETE ON logical_role BEGIN SELECT RAISE(ABORT,'immutable role identity'); END;
CREATE TRIGGER logical_role_identity_immutable BEFORE UPDATE OF role_id,project_id,role_type,name,workstream_id,integration_branch,created_at ON logical_role BEGIN SELECT RAISE(ABORT,'immutable role identity'); END;
CREATE TRIGGER logical_role_pointer_requires_history BEFORE UPDATE OF status,active_binding_id,current_context_epoch,context_manifest_id ON logical_role WHEN NOT EXISTS (SELECT 1 FROM logical_role_history h WHERE h.role_id=OLD.role_id AND h.sequence=(SELECT MAX(sequence) FROM logical_role_history WHERE role_id=OLD.role_id) AND h.from_status=OLD.status AND h.to_status=NEW.status AND h.from_binding_id IS OLD.active_binding_id AND h.to_binding_id IS NEW.active_binding_id AND h.from_epoch=OLD.current_context_epoch AND h.to_epoch=NEW.current_context_epoch AND h.from_manifest_id IS OLD.context_manifest_id AND h.to_manifest_id IS NEW.context_manifest_id) BEGIN SELECT RAISE(ABORT,'role pointer change requires matching history'); END;
CREATE TRIGGER logical_role_ownership_path_no_update BEFORE UPDATE ON logical_role_ownership_path BEGIN SELECT RAISE(ABORT,'immutable role ownership'); END;
CREATE TRIGGER logical_role_ownership_path_no_delete BEFORE DELETE ON logical_role_ownership_path BEGIN SELECT RAISE(ABORT,'immutable role ownership'); END;
CREATE TRIGGER event_no_update BEFORE UPDATE ON event BEGIN SELECT RAISE(ABORT,'immutable event'); END;
CREATE TRIGGER event_no_delete BEFORE DELETE ON event BEGIN SELECT RAISE(ABORT,'immutable event'); END;
CREATE TRIGGER event_no_replace BEFORE INSERT ON event WHEN EXISTS (SELECT 1 FROM event WHERE event_id=NEW.event_id) BEGIN SELECT RAISE(ABORT,'duplicate event'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES (17,'decision_records');
"#;

/// Exact stored-definition check for every v17 object.
pub(crate) fn schema_matches(conn: &rusqlite::Connection) -> Result<bool, rusqlite::Error> {
    use rusqlite::{OptionalExtension, params};
    let mut triggers = 0;
    for statement in SQL.split(";\n") {
        let statement = statement.trim();
        let mut words = statement.split_whitespace();
        let (Some("CREATE"), Some(kind), Some(name)) = (words.next(), words.next(), words.next())
        else {
            continue;
        };
        if kind == "TRIGGER" {
            triggers += 1;
        }
        let stored: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type=?1 AND name=?2",
                params![kind.to_ascii_lowercase(), name],
                |r| r.get(0),
            )
            .optional()?;
        if stored.as_deref() != Some(statement) {
            return Ok(false);
        }
    }
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('decision_records','provenance_links','logical_role_history','logical_role','logical_role_ownership_path','event')",
        [],
        |r| r.get(0),
    )?;
    Ok(count == triggers)
}
