//! v14: append-only execution attempt rows (start and finish are separate rows).
//!
//! History, results, checkpoint and output records use later migrations.
//! Statements are separated by `;\n`; State verifies each stored definition
//! against exactly these statements, so trigger bodies must not contain `;\n`.
use super::Migration;

pub(crate) const MIGRATION: Migration = Migration {
    version: 14,
    name: "execution_records",
    sql: SQL,
};

pub(crate) const SQL: &str = r#"
CREATE TABLE attempt_started (
    attempt_id ANY NOT NULL COLLATE BINARY CHECK (typeof(attempt_id)='text' AND length(CAST(attempt_id AS BLOB))=26 AND attempt_id NOT GLOB '*[^0-9A-HJKMNP-TV-Z]*' AND substr(attempt_id,1,1) BETWEEN '0' AND '7'),
    project_id ANY NOT NULL COLLATE BINARY CHECK (typeof(project_id)='text'),
    graph_id ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_id)='text'),
    node_id ANY NOT NULL COLLATE BINARY CHECK (typeof(node_id)='text'),
    attempt_number ANY NOT NULL COLLATE BINARY CHECK (typeof(attempt_number)='text' AND instr(attempt_number,char(0))=0 AND length(attempt_number)>=1 AND attempt_number NOT GLOB '*[^0-9]*' AND substr(attempt_number,1,1) BETWEEN '1' AND '9'),
    role ANY NOT NULL CHECK (role IN ('RUNTIME_A3','RUNTIME_A4')),
    runtime_ref ANY NOT NULL CHECK (typeof(runtime_ref)='text'),
    workspace_id ANY NOT NULL CHECK (typeof(workspace_id)='text'),
    start_sha ANY NOT NULL CHECK (typeof(start_sha)='text' AND instr(start_sha,char(0))=0 AND length(CAST(start_sha AS BLOB))=40 AND start_sha NOT GLOB '*[^0-9a-f]*'),
    started_at ANY NOT NULL CHECK (typeof(started_at)='text' AND length(CAST(started_at AS BLOB))=30),
    PRIMARY KEY (attempt_id),
    UNIQUE (project_id, graph_id, node_id, attempt_number)
) STRICT;
CREATE TABLE attempt_finished (
    attempt_id ANY NOT NULL COLLATE BINARY CHECK (typeof(attempt_id)='text'),
    ended_at ANY NOT NULL CHECK (typeof(ended_at)='text' AND length(CAST(ended_at AS BLOB))=30),
    end_sha ANY CHECK (end_sha IS NULL OR (typeof(end_sha)='text' AND instr(end_sha,char(0))=0 AND length(CAST(end_sha AS BLOB))=40 AND end_sha NOT GLOB '*[^0-9a-f]*')),
    outcome_ref ANY CHECK (outcome_ref IS NULL OR typeof(outcome_ref)='text'),
    failure_class ANY CHECK (failure_class IS NULL OR typeof(failure_class)='text'),
    PRIMARY KEY (attempt_id),
    FOREIGN KEY (attempt_id) REFERENCES attempt_started(attempt_id)
) STRICT;
CREATE TRIGGER attempt_started_no_update BEFORE UPDATE ON attempt_started BEGIN SELECT RAISE(ABORT, 'attempt rows are append-only'); END;
CREATE TRIGGER attempt_started_no_delete BEFORE DELETE ON attempt_started BEGIN SELECT RAISE(ABORT, 'attempt rows are append-only'); END;
CREATE TRIGGER attempt_started_no_replace BEFORE INSERT ON attempt_started WHEN EXISTS (SELECT 1 FROM attempt_started WHERE attempt_id=NEW.attempt_id OR (project_id=NEW.project_id AND graph_id=NEW.graph_id AND node_id=NEW.node_id AND attempt_number=NEW.attempt_number)) BEGIN SELECT RAISE(ABORT, 'attempt rows are append-only'); END;
CREATE TRIGGER attempt_finished_no_update BEFORE UPDATE ON attempt_finished BEGIN SELECT RAISE(ABORT, 'attempt rows are append-only'); END;
CREATE TRIGGER attempt_finished_no_delete BEFORE DELETE ON attempt_finished BEGIN SELECT RAISE(ABORT, 'attempt rows are append-only'); END;
CREATE TRIGGER attempt_finished_no_replace BEFORE INSERT ON attempt_finished WHEN EXISTS (SELECT 1 FROM attempt_finished WHERE attempt_id=NEW.attempt_id) BEGIN SELECT RAISE(ABORT, 'attempt rows are append-only'); END;
INSERT INTO state_schema_version(version, migration_name) VALUES (14, 'execution_records');
"#;
