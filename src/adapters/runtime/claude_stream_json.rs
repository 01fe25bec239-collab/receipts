//! Open Claude stdout records, with no provider success or failure taxonomy.
use std::{error::Error, fmt};

use receipts_workspace_execution::execution::CapturedStream;
use serde_json::Value;

/// Payload-free protocol availability. An incomplete live snapshot is only an
/// observation of current bytes; it does not change the process lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeStreamJsonError {
    StdoutTruncated,
    InvalidUtf8 { line: usize },
    InvalidJson { line: usize },
    IncompleteJson { line: usize },
    ExpectedObject { line: usize },
}
impl fmt::Display for ClaudeStreamJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Claude stream-json unavailable: {self:?}")
    }
}
impl Error for ClaudeStreamJsonError {}

/// One complete JSON object. Unknown types and fields remain explicit evidence.
/// Payload-bearing types deliberately omit Debug; callers opt into payload access.
pub struct ClaudeStreamJsonRecord<'a> {
    raw: &'a [u8],
    value: Value,
}
impl ClaudeStreamJsonRecord<'_> {
    pub fn raw_record(&self) -> &[u8] {
        self.raw
    }
    pub fn value(&self) -> &Value {
        &self.value
    }
}

/// Ordered records, not a Claude task verdict or a Receipts acceptance decision.
pub struct ClaudeStreamJson<'a> {
    records: Vec<ClaudeStreamJsonRecord<'a>>,
}
impl ClaudeStreamJson<'_> {
    pub fn records(&self) -> &[ClaudeStreamJsonRecord<'_>] {
        &self.records
    }
}

// Only bounded Workspace capture enters this module. Never join truncated head
// and tail. One copy per observation, never an accumulator across snapshots.
pub(crate) fn contiguous_stdout(stdout: &CapturedStream) -> Option<Vec<u8>> {
    (!stdout.truncated()).then(|| [stdout.head(), stdout.tail()].concat())
}
pub(crate) fn protocol(
    stdout: &Option<Vec<u8>>,
) -> Result<ClaudeStreamJson<'_>, ClaudeStreamJsonError> {
    let bytes = stdout
        .as_deref()
        .ok_or(ClaudeStreamJsonError::StdoutTruncated)?;
    let mut records = Vec::new();
    for record in crate::jsonl::records(bytes) {
        let record = record.map_err(|e| match e.kind {
            crate::jsonl::JsonlErrorKind::InvalidUtf8 => {
                ClaudeStreamJsonError::InvalidUtf8 { line: e.line }
            }
            crate::jsonl::JsonlErrorKind::InvalidJson => {
                ClaudeStreamJsonError::InvalidJson { line: e.line }
            }
            crate::jsonl::JsonlErrorKind::IncompleteJson => {
                ClaudeStreamJsonError::IncompleteJson { line: e.line }
            }
        })?;
        if !record.value.is_object() {
            return Err(ClaudeStreamJsonError::ExpectedObject { line: record.line });
        }
        records.push(ClaudeStreamJsonRecord {
            raw: record.raw,
            value: record.value,
        });
    }
    Ok(ClaudeStreamJson { records })
}
