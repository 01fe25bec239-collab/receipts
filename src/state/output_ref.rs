//! Immutable metadata only. Caller partition keys and references confer no authority.
use crate::error::{GraphFailureCode as Code, GraphPhase as Phase, StateError};
use crate::graph_node_result::invalid;
use crate::graph_repository::{self as graph, hex, id};
use crate::repository::SqliteStateRepository;
use rusqlite::{Connection, OptionalExtension, params};
use std::fmt;

/// An opaque caller reference with supplied metadata; contents are never accessed.
#[derive(Clone, PartialEq, Eq)]
pub struct OutputRef {
    project_id: String,
    output_ref: String,
    sha256: String,
    storage_uri: String,
    byte_length: String,
}
impl fmt::Debug for OutputRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OutputRef")
    }
}
impl OutputRef {
    pub fn new(
        project_id: &str,
        output_ref: &str,
        sha256: &str,
        storage_uri: &str,
        byte_length: &str,
    ) -> Result<Self, StateError> {
        if !id(project_id)
            || output_ref.is_empty()
            || !hex(sha256, 64)
            || storage_uri.is_empty()
            || !(byte_length == "0" || graph::positive(byte_length))
        {
            return Err(invalid());
        }
        Ok(Self {
            project_id: project_id.into(),
            output_ref: output_ref.into(),
            sha256: sha256.into(),
            storage_uri: storage_uri.into(),
            byte_length: byte_length.into(),
        })
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn output_ref(&self) -> &str {
        &self.output_ref
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    pub fn storage_uri(&self) -> &str {
        &self.storage_uri
    }
    pub fn byte_length(&self) -> &str {
        &self.byte_length
    }
}

pub(crate) fn schema_matches(conn: &Connection) -> Result<bool, rusqlite::Error> {
    for statement in crate::migrations::v0016_checkpoint_refs::SQL.split(";\n") {
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
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('workspace_checkpoints','output_refs')", [], |r|r.get(0))?;
    Ok(count == 6)
}
fn decode(row: &rusqlite::Row<'_>) -> Result<OutputRef, StateError> {
    let text = |i| {
        row.get::<_, String>(i)
            .map_err(|e| graph::sqlite_error(Phase::Read, e))
    };
    OutputRef::new(&text(0)?, &text(1)?, &text(2)?, &text(3)?, &text(4)?)
        .map_err(|_| graph::corrupt())
}
impl SqliteStateRepository {
    pub fn append_output_ref(&mut self, value: OutputRef) -> Result<(), StateError> {
        graph::write_transaction(self.connection(), |conn| {
            let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM output_refs WHERE project_id=?1 AND output_ref=?2)", params![value.project_id,value.output_ref], |r|r.get(0)).map_err(|e|graph::sqlite_error(Phase::Write,e))?;
            if exists {
                return Err(graph::failure(Phase::Write, Code::AlreadyExists, None));
            }
            conn.execute("INSERT INTO output_refs(project_id,output_ref,sha256,storage_uri,byte_length) VALUES (?1,?2,?3,?4,?5)",params![value.project_id,value.output_ref,value.sha256,value.storage_uri,value.byte_length]).map_err(|e|graph::sqlite_error(Phase::Write,e))?;
            Ok(())
        })
    }
    pub fn find_output_ref(
        &self,
        project_id: &str,
        output_ref: &str,
    ) -> Result<Option<OutputRef>, StateError> {
        if !id(project_id) || output_ref.is_empty() {
            return Err(invalid());
        }
        crate::graph_history::read_transaction(self.connection(), |conn| {
            conn.query_row("SELECT project_id,output_ref,sha256,storage_uri,byte_length FROM output_refs WHERE project_id=?1 AND output_ref=?2", params![project_id,output_ref], |r|Ok(decode(r))).optional().map_err(|e|graph::sqlite_error(Phase::Read,e))?.transpose()
        })
    }
}
