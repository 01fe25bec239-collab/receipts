//! Open Claude stdout records and conservative provider-result interpretation.
use std::{collections::BTreeSet, error::Error, fmt};

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

/// Provider evidence only; never process completion or Receipts acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeProviderResult {
    Success,
    Error,
    Unknown,
}

/// Only documented result subtypes are interpreted. Other fields remain raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeResultSubtype {
    Success,
    ErrorDuringExecution,
    ErrorMaxTurns,
    ErrorMaxBudgetUsd,
    ErrorMaxStructuredOutputRetries,
    Unknown,
}

/// Main-loop token counts, not whole-tree usage, cost or quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeResultUsage {
    Unknown,
    Known {
        input_tokens: u64,
        output_tokens: u64,
    },
}

/// Payload-free refusal to interpret; raw protocol records remain available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeResultError {
    MissingResult,
    MultipleResults,
    InvalidResult,
    InvalidUsage,
}
impl fmt::Display for ClaudeResultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Claude result unavailable: {self:?}")
    }
}
impl Error for ClaudeResultError {}

// Value has already erased duplicate members. Walk validated raw JSON, letting
// serde_json decode strings so escaped equivalent keys compare identically.
fn require_unique_keys(raw: &[u8]) -> Result<(), ClaudeResultError> {
    let invalid = ClaudeResultError::InvalidResult;
    let mut objects = Vec::<BTreeSet<String>>::new();
    let mut index = 0;
    while index < raw.len() {
        match raw[index] {
            b'{' => objects.push(BTreeSet::new()),
            b'}' => {
                objects.pop().ok_or(invalid)?;
            }
            b'"' => {
                let mut strings =
                    serde_json::Deserializer::from_slice(&raw[index..]).into_iter::<String>();
                let string = strings.next().ok_or(invalid)?.map_err(|_| invalid)?;
                index += strings.byte_offset();
                if raw[index..].iter().find(|b| !b.is_ascii_whitespace()) == Some(&b':')
                    && !objects.last_mut().ok_or(invalid)?.insert(string)
                {
                    return Err(invalid);
                }
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    Ok(())
}

/// Borrowed interpretation. Payload access is explicit; Debug is omitted.
/// Session ID is opaque data and grants no resume, identity or consent authority.
pub struct ClaudeResult<'a> {
    record: &'a Value,
    subtype: ClaudeResultSubtype,
    is_error: bool,
    usage: ClaudeResultUsage,
}
impl ClaudeResult<'_> {
    pub fn subtype(&self) -> ClaudeResultSubtype {
        self.subtype
    }
    pub fn is_error(&self) -> bool {
        self.is_error
    }
    pub fn provider_result(&self) -> ClaudeProviderResult {
        match self.subtype {
            ClaudeResultSubtype::Unknown => ClaudeProviderResult::Unknown,
            ClaudeResultSubtype::Success if !self.is_error => ClaudeProviderResult::Success,
            _ => ClaudeProviderResult::Error,
        }
    }
    pub fn usage(&self) -> ClaudeResultUsage {
        self.usage
    }
    pub fn session_id(&self) -> Option<&str> {
        self.record.get("session_id").and_then(Value::as_str)
    }
    /// Final text or API error text on the success subtype; never error diagnosis.
    pub fn text(&self) -> Option<&str> {
        (self.subtype == ClaudeResultSubtype::Success)
            .then(|| self.record.get("result").and_then(Value::as_str))
            .flatten()
    }
    /// Provider error strings, accessed as data without a FailureClass mapping.
    pub fn errors(&self) -> Option<&[Value]> {
        self.record
            .get("errors")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
    }
}

impl ClaudeStreamJson<'_> {
    /// Interpret exactly one result in the entire validated observation. Live
    /// snapshots can contain a result before process completion. Trailing events
    /// are allowed, but any second result (including unknown subtypes) is refused.
    /// Duplicate object keys anywhere in the observation refuse interpretation;
    /// raw records and their existing Value projections remain available.
    ///
    /// Sources: <https://code.claude.com/docs/en/agent-sdk/typescript#sdkresultmessage>,
    /// <https://code.claude.com/docs/en/agent-sdk/agent-loop#handle-the-result>.
    /// The published @anthropic-ai/claude-agent-sdk 0.3.301 sdk.d.ts documents
    /// subtype success + is_error true as API error text, not provider success.
    pub fn result(&self) -> Result<ClaudeResult<'_>, ClaudeResultError> {
        // Check every observation before selection: a duplicate type can hide
        // an error result in an otherwise successful stream.
        for record in &self.records {
            require_unique_keys(record.raw)?;
        }
        let mut results = self.records.iter().filter(|r| r.value["type"] == "result");
        let record = &results
            .next()
            .ok_or(ClaudeResultError::MissingResult)?
            .value;
        if results.next().is_some() {
            return Err(ClaudeResultError::MultipleResults);
        }
        let invalid = ClaudeResultError::InvalidResult;
        let subtype = match record
            .get("subtype")
            .and_then(Value::as_str)
            .ok_or(invalid)?
        {
            "success" => ClaudeResultSubtype::Success,
            "error_during_execution" => ClaudeResultSubtype::ErrorDuringExecution,
            "error_max_turns" => ClaudeResultSubtype::ErrorMaxTurns,
            "error_max_budget_usd" => ClaudeResultSubtype::ErrorMaxBudgetUsd,
            "error_max_structured_output_retries" => {
                ClaudeResultSubtype::ErrorMaxStructuredOutputRetries
            }
            _ => ClaudeResultSubtype::Unknown,
        };
        let is_error = record
            .get("is_error")
            .and_then(Value::as_bool)
            .ok_or(invalid)?;
        if record.get("session_id").is_some_and(|v| !v.is_string()) {
            return Err(invalid);
        }
        match subtype {
            ClaudeResultSubtype::Success => {
                if !record.get("result").is_some_and(Value::is_string)
                    || record.get("errors").is_some()
                {
                    return Err(invalid);
                }
            }
            ClaudeResultSubtype::Unknown => {}
            _ => {
                if !record
                    .get("errors")
                    .and_then(Value::as_array)
                    .is_some_and(|errors| errors.iter().all(Value::is_string))
                    || record.get("result").is_some()
                {
                    return Err(invalid);
                }
            }
        }
        let usage = match record.get("usage") {
            None | Some(Value::Null) => ClaudeResultUsage::Unknown,
            Some(Value::Object(usage)) => {
                for key in ["input_tokens", "output_tokens"] {
                    if usage.get(key).is_some_and(|v| v.as_u64().is_none()) {
                        return Err(ClaudeResultError::InvalidUsage);
                    }
                }
                match (
                    usage.get("input_tokens").and_then(Value::as_u64),
                    usage.get("output_tokens").and_then(Value::as_u64),
                ) {
                    (Some(input_tokens), Some(output_tokens)) => ClaudeResultUsage::Known {
                        input_tokens,
                        output_tokens,
                    },
                    _ => ClaudeResultUsage::Unknown,
                }
            }
            _ => return Err(ClaudeResultError::InvalidUsage),
        };
        Ok(ClaudeResult {
            record,
            subtype,
            is_error,
            usage,
        })
    }
}
