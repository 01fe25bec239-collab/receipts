//! Immutable manifest versions and duplicate-insert role backstops.
use super::Migration;
use crate::graph_repository as graph;
use crate::{GraphPhase, StateError};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) const MIGRATION: Migration = Migration {
    version: 18,
    name: "manifest_successors",
    sql: SQL,
};
pub(crate) const SQL: &str = r#"
CREATE TABLE context_manifest_v18 (
    manifest_id TEXT NOT NULL PRIMARY KEY,
    role_id TEXT NOT NULL REFERENCES logical_role (role_id),
    project_id TEXT NOT NULL,
    epoch TEXT NOT NULL CHECK (typeof(epoch) = 'text' AND length(epoch) >= 1 AND epoch NOT GLOB '*[^0-9]*' AND (epoch = '0' OR substr(epoch, 1, 1) BETWEEN '1' AND '9')),
    created_at TEXT NOT NULL,
    last_rehydrated_at TEXT
);
INSERT INTO context_manifest_v18 SELECT * FROM context_manifest;
DROP TABLE context_manifest;
ALTER TABLE context_manifest_v18 RENAME TO context_manifest;
CREATE TABLE context_manifest_seal (
    manifest_id TEXT NOT NULL PRIMARY KEY REFERENCES context_manifest(manifest_id)
);
INSERT INTO context_manifest_seal SELECT manifest_id FROM context_manifest;
CREATE TRIGGER context_manifest_seal_no_update BEFORE UPDATE ON context_manifest_seal BEGIN SELECT RAISE(ABORT,'immutable manifest seal'); END;
CREATE TRIGGER context_manifest_seal_no_delete BEFORE DELETE ON context_manifest_seal BEGIN SELECT RAISE(ABORT,'immutable manifest seal'); END;
CREATE TRIGGER context_manifest_seal_no_replace BEFORE INSERT ON context_manifest_seal WHEN EXISTS (SELECT 1 FROM context_manifest_seal WHERE manifest_id=NEW.manifest_id) BEGIN SELECT RAISE(ABORT,'duplicate manifest seal'); END;
CREATE TRIGGER context_manifest_source_sealed BEFORE INSERT ON context_manifest_source WHEN EXISTS (SELECT 1 FROM context_manifest_seal WHERE manifest_id=NEW.manifest_id) BEGIN SELECT RAISE(ABORT,'sealed manifest'); END;
CREATE TRIGGER context_manifest_source_required_for_sealed BEFORE INSERT ON context_manifest_source_required_for WHEN EXISTS (SELECT 1 FROM context_manifest_seal WHERE manifest_id=NEW.manifest_id) BEGIN SELECT RAISE(ABORT,'sealed manifest'); END;
CREATE INDEX idx_context_manifest_role ON context_manifest(role_id);
CREATE TRIGGER logical_role_no_replace BEFORE INSERT ON logical_role WHEN EXISTS (SELECT 1 FROM logical_role WHERE role_id=NEW.role_id) BEGIN SELECT RAISE(ABORT,'duplicate role identity'); END;
CREATE TRIGGER logical_role_ownership_path_no_replace BEFORE INSERT ON logical_role_ownership_path WHEN EXISTS (SELECT 1 FROM logical_role_ownership_path WHERE role_id=NEW.role_id AND position=NEW.position) BEGIN SELECT RAISE(ABORT,'duplicate role ownership'); END;
CREATE TRIGGER context_manifest_no_update BEFORE UPDATE ON context_manifest BEGIN SELECT RAISE(ABORT,'immutable manifest'); END;
CREATE TRIGGER context_manifest_no_delete BEFORE DELETE ON context_manifest BEGIN SELECT RAISE(ABORT,'immutable manifest'); END;
CREATE TRIGGER context_manifest_no_replace BEFORE INSERT ON context_manifest WHEN EXISTS (SELECT 1 FROM context_manifest WHERE manifest_id=NEW.manifest_id) BEGIN SELECT RAISE(ABORT,'duplicate manifest row'); END;
CREATE TRIGGER context_manifest_source_no_update BEFORE UPDATE ON context_manifest_source BEGIN SELECT RAISE(ABORT,'immutable manifest'); END;
CREATE TRIGGER context_manifest_source_no_delete BEFORE DELETE ON context_manifest_source BEGIN SELECT RAISE(ABORT,'immutable manifest'); END;
CREATE TRIGGER context_manifest_source_no_replace BEFORE INSERT ON context_manifest_source WHEN EXISTS (SELECT 1 FROM context_manifest_source WHERE manifest_id=NEW.manifest_id AND source_ordinal=NEW.source_ordinal) BEGIN SELECT RAISE(ABORT,'duplicate manifest row'); END;
CREATE TRIGGER context_manifest_source_required_for_no_update BEFORE UPDATE ON context_manifest_source_required_for BEGIN SELECT RAISE(ABORT,'immutable manifest'); END;
CREATE TRIGGER context_manifest_source_required_for_no_delete BEFORE DELETE ON context_manifest_source_required_for BEGIN SELECT RAISE(ABORT,'immutable manifest'); END;
CREATE TRIGGER context_manifest_source_required_for_no_replace BEFORE INSERT ON context_manifest_source_required_for WHEN EXISTS (SELECT 1 FROM context_manifest_source_required_for WHERE manifest_id=NEW.manifest_id AND source_ordinal=NEW.source_ordinal AND required_for_ordinal=NEW.required_for_ordinal) BEGIN SELECT RAISE(ABORT,'duplicate manifest row'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES (18,'manifest_successors');
"#;

pub(crate) fn schema_matches(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let mut triggers = 0;
    for sql in [super::v0017_decision_records::SQL, SQL] {
        for statement in sql.split(";\n") {
            let statement = statement.trim();
            let mut words = statement.split_whitespace();
            let (Some("CREATE"), Some(kind), Some(name)) =
                (words.next(), words.next(), words.next())
            else {
                continue;
            };
            let (name, expected) = if name == "context_manifest_v18" {
                (
                    "context_manifest",
                    statement.replace("context_manifest_v18", "\"context_manifest\""),
                )
            } else {
                (name, statement.to_string())
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
            if stored.as_deref() != Some(expected.as_str()) {
                return Ok(false);
            }
        }
    }
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('decision_records','provenance_links','logical_role_history','logical_role','logical_role_ownership_path','event','context_manifest','context_manifest_source','context_manifest_source_required_for','context_manifest_seal')", [], |r| r.get(0))?;
    Ok(count == triggers)
}

pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    let phase = GraphPhase::Migration;
    graph::verify_v17(conn, phase)?;
    let enabled: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .map_err(|e| graph::sqlite_error(phase, e))?;
    if enabled != 1 {
        return Err(graph::corrupt());
    }
    conn.execute_batch("PRAGMA foreign_keys=OFF")
        .map_err(|e| graph::sqlite_error(phase, e))?;
    let result = graph::migrate_step(conn, |conn| {
        graph::verify_v17(conn, phase)?;
        // Reject malformed snapshots before copying; never infer missing source data.
        let ids = conn
            .prepare("SELECT manifest_id FROM context_manifest")
            .map_err(|e| graph::sqlite_error(phase, e))?
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| graph::sqlite_error(phase, e))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| graph::sqlite_error(phase, e))?;
        for id in ids {
            crate::context_manifest::read_context_manifest(conn, &id)
                .map_err(|_| graph::failure(phase, crate::GraphFailureCode::CorruptStore, None))?;
        }
        conn.execute_batch(SQL)
            .map_err(|e| graph::sqlite_error(phase, e))?;
        graph::verify_v18(conn, phase)
    });
    let restored = conn
        .execute_batch("PRAGMA foreign_keys=ON")
        .map_err(|e| graph::sqlite_error(phase, e))
        .and_then(|()| {
            let enabled: i64 = conn
                .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
                .map_err(|e| graph::sqlite_error(phase, e))?;
            if enabled != 1 {
                return Err(graph::corrupt());
            }
            let violation: Option<i64> = conn
                .query_row("SELECT 1 FROM pragma_foreign_key_check LIMIT 1", [], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(|e| graph::sqlite_error(phase, e))?;
            if violation.is_some() {
                return Err(graph::corrupt());
            }
            Ok(())
        });
    result.and(restored)
}
