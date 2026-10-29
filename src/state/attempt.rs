//! Append-only execution attempt records (migration 0014 slice).
//!
//! An attempt is persisted as two immutable rows: one written when it starts
//! and, at most once, one written when it finishes. Nothing is ever updated or
//! deleted; SQL triggers enforce this independently of this API. Records hold
//! identifiers, SHAs, timestamps and opaque references only, never raw
//! output, prompts or error text. Constructing or holding these values grants
//! no authority: State only persists what its single writer is told to record.
//! Diagnostics are closed enums and never contain caller input.

use std::cmp::Ordering;
use std::fmt;

use rusqlite::{Connection, OptionalExtension, params};

use crate::canonical_timestamp::CanonicalTimestampV1;
use crate::error::{
    AttemptFailureCode as Code, AttemptField as Field, GraphFailureCode, StateError,
};
use crate::graph_repository::{self, hex, id, positive};
use crate::migrations::v0014_execution_records;
use crate::repository::SqliteStateRepository;

fn failure(code: Code, field: Option<Field>) -> StateError {
    StateError::AttemptPersistence { code, field }
}
fn invalid(field: Field) -> StateError {
    failure(Code::InvalidShape, Some(field))
}
fn corrupt() -> StateError {
    failure(Code::CorruptStore, None)
}

fn verification_error(error: StateError) -> StateError {
    match error {
        StateError::GraphPersistence {
            code: GraphFailureCode::StoreBusy,
            ..
        } => failure(Code::StoreBusy, None),
        _ => corrupt(),
    }
}

fn sqlite_error(error: rusqlite::Error) -> StateError {
    let code = match error {
        rusqlite::Error::SqliteFailure(ref cause, _) => match cause.code {
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => {
                Code::StoreBusy
            }
            rusqlite::ErrorCode::CannotOpen
            | rusqlite::ErrorCode::PermissionDenied
            | rusqlite::ErrorCode::ReadOnly => Code::StoreUnavailable,
            rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase => {
                Code::CorruptStore
            }
            _ => Code::StorageFailure,
        },
        rusqlite::Error::InvalidColumnType(..)
        | rusqlite::Error::FromSqlConversionFailure(..)
        | rusqlite::Error::Utf8Error(..) => Code::CorruptStore,
        _ => Code::StorageFailure,
    };
    failure(code, None)
}

fn timestamp(value: &str, field: Field) -> Result<CanonicalTimestampV1, StateError> {
    CanonicalTimestampV1::parse(value).map_err(|_| invalid(field))
}

fn checked(ok: bool, field: Field) -> Result<(), StateError> {
    if ok { Ok(()) } else { Err(invalid(field)) }
}

/// Canonical ULID attempt identity.
#[derive(Clone, PartialEq, Eq)]
pub struct AttemptId(String);
impl AttemptId {
    pub fn parse(value: &str) -> Result<Self, StateError> {
        let b = value.as_bytes();
        let ok = b.len() == 26
            && (b'0'..=b'7').contains(&b[0])
            && b.iter()
                .all(|x| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(x));
        checked(ok, Field::AttemptId)?;
        Ok(Self(value.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for AttemptId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AttemptId")
    }
}

/// Runtime role that executed the attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptRole {
    /// `RUNTIME_A3`.
    RuntimeA3,
    /// `RUNTIME_A4`.
    RuntimeA4,
}
impl AttemptRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeA3 => "RUNTIME_A3",
            Self::RuntimeA4 => "RUNTIME_A4",
        }
    }
    fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "RUNTIME_A3" => Ok(Self::RuntimeA3),
            "RUNTIME_A4" => Ok(Self::RuntimeA4),
            _ => Err(corrupt()),
        }
    }
}

/// Start half of an attempt. Written once by [`SqliteStateRepository::append_attempt_started`].
#[derive(Clone, PartialEq, Eq)]
pub struct AttemptStart {
    attempt_id: AttemptId,
    project_id: String,
    graph_id: String,
    node_id: String,
    attempt_number: String,
    role: AttemptRole,
    runtime_ref: String,
    workspace_id: String,
    start_sha: String,
    started_at: CanonicalTimestampV1,
}
impl AttemptStart {
    /// `attempt_number` is a positive decimal of any length; `start_sha` is a
    /// lowercase 40-hex Git SHA; `started_at` is a canonical UTC timestamp.
    /// `runtime_ref` is opaque and preserved exactly; it grants no admission or authority.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        attempt_id: AttemptId,
        project_id: &str,
        graph_id: &str,
        node_id: &str,
        attempt_number: &str,
        role: AttemptRole,
        runtime_ref: &str,
        workspace_id: &str,
        start_sha: &str,
        started_at: &str,
    ) -> Result<Self, StateError> {
        checked(id(project_id), Field::ProjectId)?;
        checked(id(graph_id), Field::GraphId)?;
        checked(id(node_id), Field::NodeId)?;
        checked(positive(attempt_number), Field::AttemptNumber)?;
        checked(id(workspace_id), Field::WorkspaceId)?;
        checked(hex(start_sha, 40), Field::StartSha)?;
        Ok(Self {
            attempt_id,
            project_id: project_id.to_owned(),
            graph_id: graph_id.to_owned(),
            node_id: node_id.to_owned(),
            attempt_number: attempt_number.to_owned(),
            role,
            runtime_ref: runtime_ref.to_owned(),
            workspace_id: workspace_id.to_owned(),
            start_sha: start_sha.to_owned(),
            started_at: timestamp(started_at, Field::StartedAt)?,
        })
    }
}
impl fmt::Debug for AttemptStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AttemptStart")
    }
}

/// Finish half of an attempt. Written at most once per attempt.
#[derive(Clone, PartialEq, Eq)]
pub struct AttemptFinish {
    attempt_id: AttemptId,
    ended_at: CanonicalTimestampV1,
    end_sha: Option<String>,
    outcome_ref: Option<String>,
    failure_class: Option<String>,
}
impl AttemptFinish {
    /// Optional strings are opaque and preserved exactly, including empty values.
    /// Callers must supply references/classes rather than raw output or error text.
    pub fn new(
        attempt_id: AttemptId,
        ended_at: &str,
        end_sha: Option<&str>,
        outcome_ref: Option<&str>,
        failure_class: Option<&str>,
    ) -> Result<Self, StateError> {
        if let Some(sha) = end_sha {
            checked(hex(sha, 40), Field::EndSha)?;
        }
        Ok(Self {
            attempt_id,
            ended_at: timestamp(ended_at, Field::EndedAt)?,
            end_sha: end_sha.map(str::to_owned),
            outcome_ref: outcome_ref.map(str::to_owned),
            failure_class: failure_class.map(str::to_owned),
        })
    }
}
impl fmt::Debug for AttemptFinish {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AttemptFinish")
    }
}

/// Persisted attempt: its start row and, once written, its finish row.
#[derive(Clone, PartialEq, Eq)]
pub struct AttemptRecord {
    start: AttemptStart,
    finish: Option<AttemptFinish>,
}
impl AttemptRecord {
    pub fn attempt_id(&self) -> &AttemptId {
        &self.start.attempt_id
    }
    pub fn project_id(&self) -> &str {
        &self.start.project_id
    }
    pub fn graph_id(&self) -> &str {
        &self.start.graph_id
    }
    pub fn node_id(&self) -> &str {
        &self.start.node_id
    }
    pub fn attempt_number(&self) -> &str {
        &self.start.attempt_number
    }
    pub fn role(&self) -> AttemptRole {
        self.start.role
    }
    pub fn runtime_ref(&self) -> &str {
        &self.start.runtime_ref
    }
    pub fn workspace_id(&self) -> &str {
        &self.start.workspace_id
    }
    pub fn start_sha(&self) -> &str {
        &self.start.start_sha
    }
    pub fn started_at(&self) -> &CanonicalTimestampV1 {
        &self.start.started_at
    }
    pub fn is_finished(&self) -> bool {
        self.finish.is_some()
    }
    pub fn ended_at(&self) -> Option<&CanonicalTimestampV1> {
        self.finish.as_ref().map(|f| &f.ended_at)
    }
    pub fn end_sha(&self) -> Option<&str> {
        self.finish.as_ref().and_then(|f| f.end_sha.as_deref())
    }
    pub fn outcome_ref(&self) -> Option<&str> {
        self.finish.as_ref().and_then(|f| f.outcome_ref.as_deref())
    }
    pub fn failure_class(&self) -> Option<&str> {
        self.finish
            .as_ref()
            .and_then(|f| f.failure_class.as_deref())
    }
}
impl fmt::Debug for AttemptRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AttemptRecord")
    }
}

/// True when the v14 tables and triggers are exactly the migration's own.
pub(crate) fn schema_matches(conn: &Connection) -> Result<bool, rusqlite::Error> {
    for statement in v0014_execution_records::SQL.split(";\n") {
        let statement = statement.trim();
        let mut words = statement.split_whitespace();
        let (Some("CREATE"), Some(kind), Some(name)) = (words.next(), words.next(), words.next())
        else {
            continue;
        };
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
    let triggers: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('attempt_started','attempt_finished')",
        [],
        |r| r.get(0),
    )?;
    Ok(triggers == 6)
}

fn verify_store(conn: &Connection) -> Result<(), StateError> {
    let head = graph_repository::schema_head(conn, crate::error::GraphPhase::Read)
        .map_err(verification_error)?;
    if head != Some(16) {
        return Err(failure(Code::UnsupportedSchema, None));
    }
    graph_repository::verify_ledger(conn, 16, crate::error::GraphPhase::Read)
        .map_err(verification_error)?;
    if !schema_matches(conn).map_err(sqlite_error)? {
        return Err(corrupt());
    }
    Ok(())
}

const SELECT_JOINED: &str = "SELECT s.attempt_id,s.project_id,s.graph_id,s.node_id,s.attempt_number,s.role,s.runtime_ref,s.workspace_id,s.start_sha,s.started_at,f.ended_at,f.end_sha,f.outcome_ref,f.failure_class FROM attempt_started s LEFT JOIN attempt_finished f ON f.attempt_id=s.attempt_id";

/// Decodes a joined row, re-validating stored values; anything invalid is corruption.
fn decode(row: &rusqlite::Row<'_>) -> Result<AttemptRecord, StateError> {
    let text = |i: usize| row.get::<_, String>(i).map_err(sqlite_error);
    let opt = |i: usize| row.get::<_, Option<String>>(i).map_err(sqlite_error);
    let attempt_id = AttemptId::parse(&text(0)?).map_err(|_| corrupt())?;
    let role = AttemptRole::from_storage(&text(5)?)?;
    let start = AttemptStart::new(
        attempt_id.clone(),
        &text(1)?,
        &text(2)?,
        &text(3)?,
        &text(4)?,
        role,
        &text(6)?,
        &text(7)?,
        &text(8)?,
        &text(9)?,
    )
    .map_err(|_| corrupt())?;
    let finish = match opt(10)? {
        None => None,
        Some(ended_at) => Some(
            AttemptFinish::new(
                attempt_id,
                &ended_at,
                opt(11)?.as_deref(),
                opt(12)?.as_deref(),
                opt(13)?.as_deref(),
            )
            .map_err(|_| corrupt())?,
        ),
    };
    if finish
        .as_ref()
        .is_some_and(|finish| finish.ended_at.cmp(&start.started_at) == Ordering::Less)
    {
        return Err(corrupt());
    }
    Ok(AttemptRecord { start, finish })
}

fn write<T>(
    conn: &Connection,
    work: impl FnOnce(&Connection) -> Result<T, StateError>,
) -> Result<T, StateError> {
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(sqlite_error)?;
    let outcome = verify_store(conn).and_then(|()| work(conn));
    match outcome {
        Ok(value) => match conn.execute_batch("COMMIT") {
            Ok(()) => Ok(value),
            Err(e) => {
                let mapped = sqlite_error(e);
                conn.execute_batch("ROLLBACK").map_err(sqlite_error)?;
                Err(mapped)
            }
        },
        Err(e) => {
            conn.execute_batch("ROLLBACK").map_err(sqlite_error)?;
            Err(e)
        }
    }
}

impl SqliteStateRepository {
    /// Appends the immutable start row of an attempt. A repeated `attempt_id` is refused.
    pub fn append_attempt_started(&mut self, start: AttemptStart) -> Result<(), StateError> {
        write(self.connection(), |conn| {
            let duplicate: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM attempt_started WHERE attempt_id=?1",
                    [start.attempt_id.as_str()],
                    |r| r.get(0),
                )
                .map_err(sqlite_error)?;
            if duplicate != 0 {
                return Err(failure(Code::DuplicateStart, None));
            }
            conn.execute(
                "INSERT INTO attempt_started(attempt_id,project_id,graph_id,node_id,attempt_number,role,runtime_ref,workspace_id,start_sha,started_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![start.attempt_id.as_str(), start.project_id, start.graph_id, start.node_id, start.attempt_number,
                    start.role.as_str(), start.runtime_ref, start.workspace_id, start.start_sha, start.started_at.as_str()],
            )
            .map_err(sqlite_error)?;
            Ok(())
        })
    }

    /// Appends the immutable finish row of a started attempt. A finish without
    /// a start, or a second finish (identical or different), is refused and
    /// leaves the stored rows unchanged.
    pub fn append_attempt_finished(&mut self, finish: AttemptFinish) -> Result<(), StateError> {
        write(self.connection(), |conn| {
            let started: Option<String> = conn
                .query_row(
                    "SELECT started_at FROM attempt_started WHERE attempt_id=?1",
                    [finish.attempt_id.as_str()],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sqlite_error)?;
            let Some(started) = started else {
                return Err(failure(Code::UnknownAttempt, Some(Field::AttemptId)));
            };
            let same: Option<bool> = conn
                .query_row(
                    "SELECT ended_at IS ?2 AND end_sha IS ?3 AND outcome_ref IS ?4 AND failure_class IS ?5 FROM attempt_finished WHERE attempt_id=?1",
                    params![finish.attempt_id.as_str(), finish.ended_at.as_str(), finish.end_sha, finish.outcome_ref, finish.failure_class],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sqlite_error)?;
            if let Some(same) = same {
                return Err(failure(
                    if same {
                        Code::DuplicateFinish
                    } else {
                        Code::ConflictingFinish
                    },
                    None,
                ));
            }
            let started = CanonicalTimestampV1::parse(&started).map_err(|_| corrupt())?;
            if finish.ended_at.cmp(&started) == Ordering::Less {
                return Err(invalid(Field::EndedAt));
            }
            conn.execute(
                "INSERT INTO attempt_finished(attempt_id,ended_at,end_sha,outcome_ref,failure_class) VALUES (?1,?2,?3,?4,?5)",
                params![finish.attempt_id.as_str(), finish.ended_at.as_str(), finish.end_sha, finish.outcome_ref, finish.failure_class],
            )
            .map_err(sqlite_error)?;
            Ok(())
        })
    }

    /// Reads one attempt by identity, or `None` when it was never started.
    pub fn find_attempt(
        &self,
        attempt_id: &AttemptId,
    ) -> Result<Option<AttemptRecord>, StateError> {
        read(self.connection(), |conn| {
            conn.query_row(
                &format!("{SELECT_JOINED} WHERE s.attempt_id=?1"),
                [attempt_id.as_str()],
                |r| Ok(decode(r)),
            )
            .optional()
            .map_err(sqlite_error)?
            .transpose()
        })
    }

    /// Lists one node's attempts by numeric attempt number, then attempt identity.
    pub fn list_attempts_for_node(
        &self,
        project_id: &str,
        graph_id: &str,
        node_id: &str,
    ) -> Result<Vec<AttemptRecord>, StateError> {
        checked(id(project_id), Field::ProjectId)?;
        checked(id(graph_id), Field::GraphId)?;
        checked(id(node_id), Field::NodeId)?;
        read(self.connection(), |conn| {
            let mut statement = conn
                .prepare(&format!(
                    "{SELECT_JOINED} WHERE s.project_id=?1 AND s.graph_id=?2 AND s.node_id=?3 ORDER BY length(s.attempt_number), s.attempt_number, s.attempt_id"
                ))
                .map_err(sqlite_error)?;
            let rows = statement
                .query_map(params![project_id, graph_id, node_id], |r| Ok(decode(r)))
                .map_err(sqlite_error)?;
            rows.map(|row| row.map_err(sqlite_error)?).collect()
        })
    }
}

fn read<T>(
    conn: &Connection,
    work: impl FnOnce(&Connection) -> Result<T, StateError>,
) -> Result<T, StateError> {
    let tx = conn.unchecked_transaction().map_err(sqlite_error)?;
    let outcome = verify_store(&tx).and_then(|()| work(&tx));
    match outcome {
        Ok(value) => {
            tx.commit().map_err(sqlite_error)?;
            Ok(value)
        }
        Err(e) => {
            tx.rollback().map_err(sqlite_error)?;
            Err(e)
        }
    }
}
