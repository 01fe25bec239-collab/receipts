//! Forward migration 22: State-owned rehydration order and durable review
//! seals. Migrations 1-21 (and their SQL) are unchanged; this step verifies
//! the exact v21 store before and the v22 store after.
//!
//! * `context_rehydration_order` gives every rehydration attempt a per-project
//!   commit sequence assigned by State inside the attempt's own transaction,
//!   plus the `fence_sequence` (the highest sequence committed when the
//!   attempt began reading). Continuation clearance orders attempts by this
//!   sequence, never by caller-chosen ids or timestamps, and a success whose
//!   fence precedes a newer invalidation can never clear. Attempts that exist
//!   at upgrade time are backfilled at sequence 0: their relative order was
//!   never recorded, so they are ambiguous history (see `recovery`).
//! * `review_claim_seal` stores the exact, immutable recovery data a claimant
//!   sealed for its unfinished review claim, bound to the claim, the
//!   publication package digest and the settlement it will apply. At most one
//!   seal exists per claim; it can only be inserted while the claim it names
//!   is still `CLAIMED`.
//!
//! Both tables are `WITHOUT ROWID` (v19 row-identity rule) with no-update /
//! no-delete / no-replace guards like v20 and v21. Statements are separated by
//! `;\n`; trigger bodies must not contain `;\n`.

use super::Migration;
use crate::{GraphPhase, StateError, graph_repository as graph};
use rusqlite::{Connection, params};

pub(crate) const SQL: &str = "CREATE TABLE context_rehydration_order (
    project_id TEXT NOT NULL CHECK (length(project_id) BETWEEN 1 AND 200),
    rehydration_attempt_id TEXT NOT NULL CHECK (length(rehydration_attempt_id) BETWEEN 1 AND 200),
    sequence INTEGER NOT NULL CHECK (sequence >= 0),
    fence_sequence INTEGER NOT NULL CHECK (fence_sequence >= 0),
    PRIMARY KEY (project_id, rehydration_attempt_id),
    FOREIGN KEY (project_id, rehydration_attempt_id) REFERENCES context_rehydration_attempt(project_id, rehydration_attempt_id),
    CHECK ((sequence = 0 AND fence_sequence = 0) OR (sequence > 0 AND fence_sequence < sequence))
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX context_rehydration_order_sequence ON context_rehydration_order(project_id, sequence) WHERE sequence > 0;
INSERT INTO context_rehydration_order(project_id,rehydration_attempt_id,sequence,fence_sequence) SELECT project_id,rehydration_attempt_id,0,0 FROM context_rehydration_attempt;
CREATE TRIGGER context_rehydration_order_no_update BEFORE UPDATE ON context_rehydration_order BEGIN SELECT RAISE(ABORT,'immutable rehydration order'); END;
CREATE TRIGGER context_rehydration_order_no_delete BEFORE DELETE ON context_rehydration_order BEGIN SELECT RAISE(ABORT,'immutable rehydration order'); END;
CREATE TRIGGER context_rehydration_order_no_replace BEFORE INSERT ON context_rehydration_order WHEN EXISTS (SELECT 1 FROM context_rehydration_order WHERE project_id=NEW.project_id AND rehydration_attempt_id=NEW.rehydration_attempt_id) BEGIN SELECT RAISE(ABORT,'duplicate rehydration order'); END;
CREATE TRIGGER context_rehydration_order_appends BEFORE INSERT ON context_rehydration_order WHEN NEW.sequence <> 1 + COALESCE((SELECT MAX(sequence) FROM context_rehydration_order WHERE project_id=NEW.project_id), 0) BEGIN SELECT RAISE(ABORT,'rehydration order must append'); END;
CREATE TABLE review_claim_seal (
    project_id TEXT NOT NULL CHECK (length(project_id) BETWEEN 1 AND 200),
    claim_id TEXT NOT NULL CHECK (length(claim_id) BETWEEN 1 AND 200),
    task_id TEXT NOT NULL CHECK (length(task_id) BETWEEN 1 AND 200),
    subject_sha TEXT CHECK (subject_sha IS NULL OR (length(subject_sha) = 40 AND subject_sha NOT GLOB '*[^0-9a-f]*')),
    request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64 AND request_sha256 NOT GLOB '*[^0-9a-f]*'),
    publication_id TEXT NOT NULL CHECK (length(publication_id) BETWEEN 1 AND 200),
    package_sha256 TEXT NOT NULL CHECK (length(package_sha256) = 64 AND package_sha256 NOT GLOB '*[^0-9a-f]*'),
    resolution TEXT NOT NULL CHECK (resolution IN ('COMPLETED','REFUSED')),
    outcome_kind TEXT NOT NULL CHECK (length(outcome_kind) BETWEEN 1 AND 64),
    outcome_ref TEXT CHECK (outcome_ref IS NULL OR length(outcome_ref) BETWEEN 1 AND 200),
    outcome_sha256 TEXT CHECK (outcome_sha256 IS NULL OR (length(outcome_sha256) = 64 AND outcome_sha256 NOT GLOB '*[^0-9a-f]*')),
    recovery_kind TEXT NOT NULL CHECK (length(recovery_kind) BETWEEN 1 AND 64),
    recovery_data BLOB NOT NULL CHECK (length(recovery_data) BETWEEN 1 AND 4194304),
    recovery_sha256 TEXT NOT NULL CHECK (length(recovery_sha256) = 64 AND recovery_sha256 NOT GLOB '*[^0-9a-f]*'),
    seal_sha256 TEXT NOT NULL CHECK (length(seal_sha256) = 64 AND seal_sha256 NOT GLOB '*[^0-9a-f]*'),
    sealed_at TEXT NOT NULL,
    PRIMARY KEY (project_id, claim_id),
    FOREIGN KEY (project_id, claim_id) REFERENCES review_claim(project_id, claim_id)
) STRICT, WITHOUT ROWID;
CREATE UNIQUE INDEX review_claim_seal_publication ON review_claim_seal(project_id, publication_id);
CREATE TRIGGER review_claim_seal_no_update BEFORE UPDATE ON review_claim_seal BEGIN SELECT RAISE(ABORT,'immutable review seal'); END;
CREATE TRIGGER review_claim_seal_no_delete BEFORE DELETE ON review_claim_seal BEGIN SELECT RAISE(ABORT,'immutable review seal'); END;
CREATE TRIGGER review_claim_seal_no_replace BEFORE INSERT ON review_claim_seal WHEN EXISTS (SELECT 1 FROM review_claim_seal WHERE project_id=NEW.project_id AND claim_id=NEW.claim_id) BEGIN SELECT RAISE(ABORT,'duplicate review seal'); END;
CREATE TRIGGER review_claim_seal_needs_unfinished_claim BEFORE INSERT ON review_claim_seal WHEN NOT EXISTS (SELECT 1 FROM review_claim WHERE project_id=NEW.project_id AND claim_id=NEW.claim_id AND state='CLAIMED' AND task_id=NEW.task_id AND subject_sha IS NEW.subject_sha AND request_sha256=NEW.request_sha256) BEGIN SELECT RAISE(ABORT,'a seal needs the unfinished claim it names'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES (22,'order_and_seal');";

pub(crate) const MIGRATION: Migration = Migration {
    version: 22,
    name: "order_and_seal",
    sql: SQL,
};

const TABLES: [&str; 2] = ["context_rehydration_order", "review_claim_seal"];

/// Applies v22 atomically on an exactly verified v21 store.
pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    let phase = GraphPhase::Migration;
    graph::migrate_step(conn, |conn| {
        graph::verify_v21(conn, phase)?;
        conn.execute_batch(SQL)
            .map_err(|e| graph::sqlite_error(phase, e))?;
        graph::verify_current(conn, phase)
    })
}

/// Exact v22 objects: every CREATE statement stored byte-for-byte, no extra
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
