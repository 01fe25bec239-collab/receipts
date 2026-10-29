//! Immutable checkpoint metadata and opaque output references; no payload storage.
use super::Migration;
pub(crate) const MIGRATION: Migration = Migration {
    version: 16,
    name: "checkpoint_refs",
    sql: SQL,
};
// Definition verification uses the same statement separator as migrations 14–15.
pub(crate) const SQL: &str = r#"
CREATE TABLE workspace_checkpoints (
    project_id TEXT NOT NULL COLLATE BINARY,
    checkpoint_id TEXT NOT NULL COLLATE BINARY,
    workspace_id TEXT NOT NULL COLLATE BINARY,
    checkpoint_json TEXT NOT NULL,
    PRIMARY KEY (project_id,checkpoint_id)
) STRICT;
CREATE TABLE output_refs (
    project_id TEXT NOT NULL COLLATE BINARY,
    output_ref TEXT NOT NULL COLLATE BINARY CHECK (length(CAST(output_ref AS BLOB))>0),
    sha256 TEXT NOT NULL CHECK (length(CAST(sha256 AS BLOB))=64 AND instr(sha256,char(0))=0 AND sha256 NOT GLOB '*[^0-9a-f]*'),
    storage_uri TEXT NOT NULL CHECK (length(CAST(storage_uri AS BLOB))>0),
    byte_length TEXT NOT NULL CHECK (length(byte_length)>0 AND instr(byte_length,char(0))=0 AND byte_length NOT GLOB '*[^0-9]*' AND (byte_length='0' OR substr(byte_length,1,1) BETWEEN '1' AND '9')),
    PRIMARY KEY (project_id,output_ref)
) STRICT;
CREATE TRIGGER workspace_checkpoints_no_update BEFORE UPDATE ON workspace_checkpoints BEGIN SELECT RAISE(ABORT,'immutable checkpoint'); END;
CREATE TRIGGER workspace_checkpoints_no_delete BEFORE DELETE ON workspace_checkpoints BEGIN SELECT RAISE(ABORT,'immutable checkpoint'); END;
CREATE TRIGGER workspace_checkpoints_no_replace BEFORE INSERT ON workspace_checkpoints WHEN EXISTS (SELECT 1 FROM workspace_checkpoints WHERE project_id=NEW.project_id AND checkpoint_id=NEW.checkpoint_id) BEGIN SELECT RAISE(ABORT,'duplicate checkpoint'); END;
CREATE TRIGGER output_refs_no_update BEFORE UPDATE ON output_refs BEGIN SELECT RAISE(ABORT,'immutable output reference'); END;
CREATE TRIGGER output_refs_no_delete BEFORE DELETE ON output_refs BEGIN SELECT RAISE(ABORT,'immutable output reference'); END;
CREATE TRIGGER output_refs_no_replace BEFORE INSERT ON output_refs WHEN EXISTS (SELECT 1 FROM output_refs WHERE project_id=NEW.project_id AND output_ref=NEW.output_ref) BEGIN SELECT RAISE(ABORT,'duplicate output reference'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES (16,'checkpoint_refs');
"#;
