//! Forward migration 21: atomic-publication identities and durable review
//! claims. Migrations 1-20 and their verifiers are unchanged; this step
//! verifies the exact v20 store before and the v21 store after.
//!
//! * `atomic_publication` records one admitted publication package
//!   (decision records + related events/receipts + availability facts) by
//!   its caller identity and content digest, so an exact retry returns the
//!   recorded outcome and a different package under the same identity is
//!   refused. `atomic_publication_availability` keeps the State-assigned
//!   availability sequences that outcome names.
//! * `review_claim` is the durable claim written BEFORE a review is routed
//!   or executed. A claim leaves `CLAIMED` exactly once and never returns:
//!   there is no TTL, expiry or automatic reopening anywhere in this schema.
//!
//! Every new table is `WITHOUT ROWID` (the v19 row-identity rule) and carries
//! immutability / no-replace guards like v20. Statements are separated by
//! `;\n`; trigger bodies must not contain `;\n`.

use super::Migration;
use crate::{GraphPhase, StateError, graph_repository as graph};
use rusqlite::{Connection, params};

pub(crate) const SQL: &str = "CREATE TABLE atomic_publication (
    project_id TEXT NOT NULL CHECK (length(project_id) BETWEEN 1 AND 200),
    publication_id TEXT NOT NULL CHECK (length(publication_id) BETWEEN 1 AND 200),
    package_sha256 TEXT NOT NULL CHECK (length(package_sha256) = 64 AND package_sha256 NOT GLOB '*[^0-9a-f]*'),
    recorded_at TEXT NOT NULL,
    PRIMARY KEY (project_id, publication_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE atomic_publication_availability (
    project_id TEXT NOT NULL CHECK (length(project_id) BETWEEN 1 AND 200),
    publication_id TEXT NOT NULL CHECK (length(publication_id) BETWEEN 1 AND 200),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    provider_id TEXT NOT NULL,
    runtime_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    sequence INTEGER NOT NULL CHECK (sequence >= 1),
    PRIMARY KEY (project_id, publication_id, ordinal),
    FOREIGN KEY (project_id, publication_id) REFERENCES atomic_publication(project_id, publication_id),
    FOREIGN KEY (project_id, provider_id, runtime_id, model_id, sequence) REFERENCES availability_fact(project_id, provider_id, runtime_id, model_id, sequence)
) STRICT, WITHOUT ROWID;
CREATE TABLE review_claim (
    project_id TEXT NOT NULL CHECK (length(project_id) BETWEEN 1 AND 200),
    claim_id TEXT NOT NULL CHECK (length(claim_id) BETWEEN 1 AND 200),
    task_id TEXT NOT NULL CHECK (length(task_id) BETWEEN 1 AND 200),
    subject_sha TEXT CHECK (subject_sha IS NULL OR (length(subject_sha) = 40 AND subject_sha NOT GLOB '*[^0-9a-f]*')),
    request_sha256 TEXT NOT NULL CHECK (length(request_sha256) = 64 AND request_sha256 NOT GLOB '*[^0-9a-f]*'),
    state TEXT NOT NULL CHECK (state IN ('CLAIMED','COMPLETED','REFUSED','ABANDONED')),
    outcome_kind TEXT CHECK (outcome_kind IS NULL OR length(outcome_kind) BETWEEN 1 AND 64),
    outcome_ref TEXT CHECK (outcome_ref IS NULL OR length(outcome_ref) BETWEEN 1 AND 200),
    outcome_sha256 TEXT CHECK (outcome_sha256 IS NULL OR (length(outcome_sha256) = 64 AND outcome_sha256 NOT GLOB '*[^0-9a-f]*')),
    authority_ref TEXT CHECK (authority_ref IS NULL OR length(authority_ref) BETWEEN 1 AND 200),
    claimed_at TEXT NOT NULL,
    finished_at TEXT,
    PRIMARY KEY (project_id, claim_id),
    CHECK ((state = 'CLAIMED') = (finished_at IS NULL)),
    CHECK ((state = 'CLAIMED') = (outcome_kind IS NULL)),
    CHECK ((state = 'ABANDONED') = (authority_ref IS NOT NULL))
) STRICT, WITHOUT ROWID;
CREATE TRIGGER atomic_publication_no_update BEFORE UPDATE ON atomic_publication BEGIN SELECT RAISE(ABORT,'immutable publication'); END;
CREATE TRIGGER atomic_publication_no_delete BEFORE DELETE ON atomic_publication BEGIN SELECT RAISE(ABORT,'immutable publication'); END;
CREATE TRIGGER atomic_publication_no_replace BEFORE INSERT ON atomic_publication WHEN EXISTS (SELECT 1 FROM atomic_publication WHERE project_id=NEW.project_id AND publication_id=NEW.publication_id) BEGIN SELECT RAISE(ABORT,'duplicate publication'); END;
CREATE TRIGGER atomic_publication_availability_no_update BEFORE UPDATE ON atomic_publication_availability BEGIN SELECT RAISE(ABORT,'immutable publication availability'); END;
CREATE TRIGGER atomic_publication_availability_no_delete BEFORE DELETE ON atomic_publication_availability BEGIN SELECT RAISE(ABORT,'immutable publication availability'); END;
CREATE TRIGGER atomic_publication_availability_no_replace BEFORE INSERT ON atomic_publication_availability WHEN EXISTS (SELECT 1 FROM atomic_publication_availability WHERE project_id=NEW.project_id AND publication_id=NEW.publication_id AND ordinal=NEW.ordinal) BEGIN SELECT RAISE(ABORT,'duplicate publication availability'); END;
CREATE TRIGGER review_claim_no_delete BEFORE DELETE ON review_claim BEGIN SELECT RAISE(ABORT,'immutable review claim'); END;
CREATE TRIGGER review_claim_no_replace BEFORE INSERT ON review_claim WHEN EXISTS (SELECT 1 FROM review_claim WHERE project_id=NEW.project_id AND claim_id=NEW.claim_id) BEGIN SELECT RAISE(ABORT,'duplicate review claim'); END;
CREATE TRIGGER review_claim_insert_claimed BEFORE INSERT ON review_claim WHEN NEW.state <> 'CLAIMED' BEGIN SELECT RAISE(ABORT,'review claim must start claimed'); END;
CREATE TRIGGER review_claim_leaves_claimed_once BEFORE UPDATE ON review_claim WHEN OLD.state <> 'CLAIMED' OR NEW.state = 'CLAIMED' OR NEW.project_id IS NOT OLD.project_id OR NEW.claim_id IS NOT OLD.claim_id OR NEW.task_id IS NOT OLD.task_id OR NEW.subject_sha IS NOT OLD.subject_sha OR NEW.request_sha256 IS NOT OLD.request_sha256 OR NEW.claimed_at IS NOT OLD.claimed_at BEGIN SELECT RAISE(ABORT,'review claim leaves claimed exactly once'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES (21,'publication_claims');";

pub(crate) const MIGRATION: Migration = Migration {
    version: 21,
    name: "publication_claims",
    sql: SQL,
};

const TABLES: [&str; 3] = [
    "atomic_publication",
    "atomic_publication_availability",
    "review_claim",
];

/// Applies v21 atomically on an exactly verified v20 store.
pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    let phase = GraphPhase::Migration;
    graph::migrate_step(conn, |conn| {
        graph::verify_v20(conn, phase)?;
        conn.execute_batch(SQL)
            .map_err(|e| graph::sqlite_error(phase, e))?;
        graph::verify_current(conn, phase)
    })
}

/// Exact v21 objects: every CREATE statement stored byte-for-byte, no extra
/// trigger or index on the new tables, every new table `WITHOUT ROWID`.
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
    Ok(triggers == expected_triggers && indexes == 0)
}
