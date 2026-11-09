//! Forward migration 20: trusted-core store identity, event-proposal receipts
//! and durable availability lifecycle facts. Migrations 1-19 are unchanged;
//! this step verifies the exact v19 store before and the v20 store after.

use super::Migration;
use crate::{GraphPhase, StateError, graph_repository as graph};
use rusqlite::{Connection, params};
use std::io::Read;

pub(crate) const SQL: &str = "CREATE TABLE state_store_identity (
    singleton INTEGER NOT NULL PRIMARY KEY CHECK (singleton = 1),
    store_id TEXT NOT NULL CHECK (length(store_id) = 32 AND store_id NOT GLOB '*[^0-9a-f]*')
) STRICT, WITHOUT ROWID;
CREATE TRIGGER state_store_identity_no_update BEFORE UPDATE ON state_store_identity BEGIN SELECT RAISE(ABORT,'immutable store identity'); END;
CREATE TRIGGER state_store_identity_no_delete BEFORE DELETE ON state_store_identity BEGIN SELECT RAISE(ABORT,'immutable store identity'); END;
CREATE TRIGGER state_store_identity_no_replace BEFORE INSERT ON state_store_identity WHEN EXISTS (SELECT 1 FROM state_store_identity) BEGIN SELECT RAISE(ABORT,'duplicate store identity'); END;
CREATE TABLE event_proposal_receipt (
    event_id TEXT NOT NULL PRIMARY KEY REFERENCES event(event_id),
    project_id TEXT NOT NULL CHECK (length(project_id) BETWEEN 1 AND 200),
    proposal_sha256 TEXT NOT NULL CHECK (length(proposal_sha256) = 64 AND proposal_sha256 NOT GLOB '*[^0-9a-f]*'),
    origin TEXT NOT NULL CHECK (origin IN ('ORCHESTRATION','STATE','ROUTING','RUNTIME','WORKSPACE','REVIEW','HOST')),
    claimed_observed_at TEXT,
    store_id TEXT NOT NULL CHECK (length(store_id) = 32),
    occurred_at TEXT NOT NULL,
    UNIQUE (project_id, proposal_sha256)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER event_proposal_receipt_no_update BEFORE UPDATE ON event_proposal_receipt BEGIN SELECT RAISE(ABORT,'immutable event receipt'); END;
CREATE TRIGGER event_proposal_receipt_no_delete BEFORE DELETE ON event_proposal_receipt BEGIN SELECT RAISE(ABORT,'immutable event receipt'); END;
CREATE TRIGGER event_proposal_receipt_no_replace BEFORE INSERT ON event_proposal_receipt WHEN EXISTS (SELECT 1 FROM event_proposal_receipt WHERE event_id=NEW.event_id OR (project_id=NEW.project_id AND proposal_sha256=NEW.proposal_sha256)) BEGIN SELECT RAISE(ABORT,'duplicate event receipt'); END;
CREATE TABLE availability_fact (
    project_id TEXT NOT NULL CHECK (length(project_id) BETWEEN 1 AND 200),
    provider_id TEXT NOT NULL CHECK (length(provider_id) BETWEEN 1 AND 200),
    runtime_id TEXT NOT NULL CHECK (length(runtime_id) BETWEEN 1 AND 200),
    model_id TEXT NOT NULL CHECK (length(model_id) <= 200),
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    kind TEXT NOT NULL CHECK (kind IN ('DEMOTION','PROMOTION_UNAUTHENTICATED','RETRY_AFTER_ELAPSED')),
    reason TEXT NOT NULL CHECK (length(reason) BETWEEN 1 AND 200),
    source_kind TEXT NOT NULL CHECK (source_kind IN ('RUNTIME_FAILURE_CLASS','PROBE_REPORT','RETRY_AFTER_EXPIRY','UNAUTHENTICATED_CLAIM')),
    source_ref TEXT NOT NULL CHECK (length(source_ref) BETWEEN 1 AND 200),
    follows_sequence INTEGER CHECK (follows_sequence IS NULL OR follows_sequence >= 1),
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (project_id, provider_id, runtime_id, model_id, sequence)
) STRICT, WITHOUT ROWID;
CREATE TRIGGER availability_fact_no_update BEFORE UPDATE ON availability_fact BEGIN SELECT RAISE(ABORT,'immutable availability fact'); END;
CREATE TRIGGER availability_fact_no_delete BEFORE DELETE ON availability_fact BEGIN SELECT RAISE(ABORT,'immutable availability fact'); END;
CREATE TRIGGER availability_fact_no_replace BEFORE INSERT ON availability_fact WHEN EXISTS (SELECT 1 FROM availability_fact WHERE project_id=NEW.project_id AND provider_id=NEW.provider_id AND runtime_id=NEW.runtime_id AND model_id=NEW.model_id AND sequence=NEW.sequence) BEGIN SELECT RAISE(ABORT,'duplicate availability fact'); END;
CREATE TRIGGER availability_fact_monotonic BEFORE INSERT ON availability_fact WHEN NEW.sequence <> 1 + COALESCE((SELECT MAX(sequence) FROM availability_fact WHERE project_id=NEW.project_id AND provider_id=NEW.provider_id AND runtime_id=NEW.runtime_id AND model_id=NEW.model_id), 0) BEGIN SELECT RAISE(ABORT,'non-monotonic availability fact'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES (20,'trusted_core');";

pub(crate) const MIGRATION: Migration = Migration {
    version: 20,
    name: "trusted_core",
    sql: SQL,
};

const TABLES: [&str; 3] = [
    "state_store_identity",
    "event_proposal_receipt",
    "availability_fact",
];

/// Applies v20 atomically on an exactly verified v19 store and records a
/// fresh random store identity (OS randomness, as Workspace custody tokens).
pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    let phase = GraphPhase::Migration;
    let store_id = random_store_id()
        .map_err(|_| graph::failure(phase, crate::error::GraphFailureCode::StorageFailure, None))?;
    graph::migrate_step(conn, |conn| {
        graph::verify_v19(conn, phase)?;
        conn.execute_batch(SQL)
            .map_err(|e| graph::sqlite_error(phase, e))?;
        conn.execute(
            "INSERT INTO state_store_identity(singleton,store_id) VALUES (1,?1)",
            params![store_id],
        )
        .map_err(|e| graph::sqlite_error(phase, e))?;
        graph::verify_v20(conn, phase)
    })
}

/// Exact v20 objects: every CREATE statement stored byte-for-byte, no extra
/// trigger on the new tables, one identity row.
pub(crate) fn objects_match(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let mut expected_triggers = 0;
    for statement in SQL.split(";\n") {
        let statement = statement.trim();
        let mut words = statement.split_whitespace();
        let (Some("CREATE"), Some(kind), Some(name)) = (words.next(), words.next(), words.next())
        else {
            continue;
        };
        if kind == "TRIGGER" {
            expected_triggers += 1;
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
    let mut triggers = 0i64;
    let mut indexes = 0i64;
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
    let identities: i64 = conn.query_row("SELECT COUNT(*) FROM state_store_identity", [], |r| {
        r.get(0)
    })?;
    Ok(triggers == expected_triggers && indexes == 0 && identities == 1)
}

fn random_store_id() -> std::io::Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
