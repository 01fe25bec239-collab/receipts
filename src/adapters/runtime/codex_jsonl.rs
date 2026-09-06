//! Interpretation of completed, bounded Codex stdout; never task acceptance.
//!
//! Evidence: codex-cli 0.153.4, read-only live probe (2026-09-07):
//! thread.started -> turn.started -> item.completed (agent_message.text)
//! -> turn.completed (usage). Process exit 0; stderr was an input notice.
//! Official reference: openai/codex at 52e12e0cb506e7bb2c9e406fc84d922e274a0e40,
//! codex-rs/exec/src/exec_events.rs. These observations are not a closed schema.
//!
//! Only the event envelope and fields used here receive validation/meaning.
//! Item types and statuses remain open strings, with no status classification.
//! Raw records include their original line endings; parsed values are a view,
//! not a replacement for exact evidence. Payload-bearing types omit Debug.

use std::{error::Error, fmt};

use serde_json::Value;

use crate::CodexTaskExecutionResult;

/// Recognized top-level names only. Unknown names remain in `event_type()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexJsonlEventKind {
    ThreadStarted,
    TurnStarted,
    TurnCompleted,
    TurnFailed,
    ItemStarted,
    ItemUpdated,
    ItemCompleted,
    Error,
    Unknown,
}

/// Protocol termination only, independent of the process exit code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexProtocolTermination {
    Completed,
    Failed,
    Indeterminate,
}

/// A safe error category, never a FailureClass or provider-prose diagnosis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexJsonlErrorKind {
    InvalidUtf8,
    InvalidJson,
    IncompleteJson,
    InvalidEventShape,
}

/// No payload or underlying parser error is retained in diagnostic surfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexJsonlError {
    pub line: usize,
    pub kind: CodexJsonlErrorKind,
}

impl fmt::Display for CodexJsonlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Codex JSONL {:?} at line {}", self.kind, self.line)
    }
}

impl Error for CodexJsonlError {}

/// Immutable ordered evidence. Explicit payload access may reveal sensitive data.
pub struct CodexJsonlEvent<'a> {
    raw_record: &'a [u8],
    value: Value,
    kind: CodexJsonlEventKind,
}

impl CodexJsonlEvent<'_> {
    pub fn raw_record(&self) -> &[u8] {
        self.raw_record
    }

    pub fn value(&self) -> &Value {
        &self.value
    }

    pub fn kind(&self) -> CodexJsonlEventKind {
        self.kind
    }

    /// Exact decoded type, including unknown names and Unicode.
    pub fn event_type(&self) -> &str {
        // Validated before an event can be constructed.
        self.value["type"].as_str().expect("validated event type")
    }

    /// Item evidence only for the three recognized item envelope types.
    pub fn item(&self) -> Option<&Value> {
        matches!(
            self.kind,
            CodexJsonlEventKind::ItemStarted
                | CodexJsonlEventKind::ItemUpdated
                | CodexJsonlEventKind::ItemCompleted
        )
        .then(|| &self.value["item"])
    }

    pub fn item_type(&self) -> Option<&str> {
        self.item()?.get("type")?.as_str()
    }

    /// An uninterpreted provider string. Absence never defaults to completed.
    pub fn item_status(&self) -> Option<&str> {
        self.item()?.get("status")?.as_str()
    }

    /// Text from an explicitly completed agent-message item, not task success.
    /// A future status-bearing agent-message form has no recognized meaning here.
    pub fn completed_agent_message(&self) -> Option<&str> {
        if self.kind != CodexJsonlEventKind::ItemCompleted
            || self.item_type() != Some("agent_message")
            || self.item()?.get("status").is_some()
        {
            return None;
        }
        self.item()?.get("text")?.as_str()
    }
}

/// Bound to the original process result; stdout's existing capture limit applies.
/// No public raw-byte constructor or alternate unbounded input path exists.
pub struct CodexJsonlInterpretation<'a> {
    execution: &'a CodexTaskExecutionResult,
    events: Vec<CodexJsonlEvent<'a>>,
}

impl CodexJsonlInterpretation<'_> {
    pub fn exit_code(&self) -> i32 {
        self.execution.exit_code()
    }

    pub fn events(&self) -> &[CodexJsonlEvent<'_>] {
        &self.events
    }

    pub fn turn_completed_observed(&self) -> bool {
        self.events
            .iter()
            .any(|e| e.kind == CodexJsonlEventKind::TurnCompleted)
    }

    pub fn turn_failed_observed(&self) -> bool {
        self.events
            .iter()
            .any(|e| e.kind == CodexJsonlEventKind::TurnFailed)
    }

    /// Only a recognized final record establishes termination. Later unknown or
    /// nonterminal records leave termination indeterminate; observed facts remain.
    /// Conflicting turn signals are not reconciled.
    pub fn termination(&self) -> CodexProtocolTermination {
        if self.turn_completed_observed() && self.turn_failed_observed() {
            return CodexProtocolTermination::Indeterminate;
        }
        match self.events.last().map(|e| e.kind) {
            Some(CodexJsonlEventKind::TurnCompleted) => CodexProtocolTermination::Completed,
            Some(CodexJsonlEventKind::TurnFailed) => CodexProtocolTermination::Failed,
            _ => CodexProtocolTermination::Indeterminate,
        }
    }

    /// The sole completed agent message in one recognized completed turn.
    /// Multiple messages/turns, unknown events, or stream errors leave selection
    /// unavailable. No concatenation, ranking, or process-success inference.
    pub fn final_agent_message(&self) -> Option<&str> {
        use CodexJsonlEventKind::*;
        if self.termination() != CodexProtocolTermination::Completed
            || self
                .events
                .iter()
                .any(|e| matches!(e.kind, Unknown | Error))
            || self.events.iter().filter(|e| e.kind == TurnStarted).count() != 1
            || self
                .events
                .iter()
                .filter(|e| e.kind == TurnCompleted)
                .count()
                != 1
        {
            return None;
        }
        let mut messages = self
            .events
            .iter()
            .enumerate()
            .filter(|(_, e)| e.kind == ItemCompleted && e.item_type() == Some("agent_message"));
        let (index, message) = messages.next()?;
        if messages.next().is_some() || !self.events[..index].iter().any(|e| e.kind == TurnStarted)
        {
            return None;
        }
        message.completed_agent_message()
    }
}

/// Parses each stdout line independently, all-or-error. Empty stdout gives no
/// events and indeterminate termination. Blank records (including interstitial
/// whitespace) fail; one ordinary final newline creates no extra record.
/// The caller still owns the execution result on error, including process truth.
pub fn interpret_codex_jsonl(
    execution: &CodexTaskExecutionResult,
) -> Result<CodexJsonlInterpretation<'_>, CodexJsonlError> {
    let mut events = Vec::new();
    for (index, raw_record) in execution
        .stdout()
        .split_inclusive(|b| *b == b'\n')
        .enumerate()
    {
        let error = |kind| CodexJsonlError {
            line: index + 1,
            kind,
        };
        let text =
            std::str::from_utf8(raw_record).map_err(|_| error(CodexJsonlErrorKind::InvalidUtf8))?;
        let value: Value = serde_json::from_str(text).map_err(|e| {
            error(if e.is_eof() {
                CodexJsonlErrorKind::IncompleteJson
            } else {
                CodexJsonlErrorKind::InvalidJson
            })
        })?;
        let kind =
            event_kind(&value).ok_or_else(|| error(CodexJsonlErrorKind::InvalidEventShape))?;
        events.push(CodexJsonlEvent {
            raw_record,
            value,
            kind,
        });
    }
    Ok(CodexJsonlInterpretation { execution, events })
}

fn event_kind(value: &Value) -> Option<CodexJsonlEventKind> {
    use CodexJsonlEventKind::*;
    let kind = match value.as_object()?.get("type")?.as_str()? {
        "thread.started" => {
            value.get("thread_id")?.as_str()?;
            ThreadStarted
        }
        "turn.started" => TurnStarted,
        "turn.completed" => {
            let usage = value.get("usage")?.as_object()?;
            // Stable usage fields observed in the existing protocol. Additional
            // fields remain raw evidence, not mandatory schema expansion.
            for field in ["input_tokens", "cached_input_tokens", "output_tokens"] {
                usage.get(field)?.as_i64()?;
            }
            TurnCompleted
        }
        "turn.failed" => {
            value.get("error")?.get("message")?.as_str()?;
            TurnFailed
        }
        name @ ("item.started" | "item.updated" | "item.completed") => {
            let item = value.get("item")?.as_object()?;
            let item_type = item.get("type")?.as_str()?;
            item.get("id")?.as_str()?;
            if let Some(status) = item.get("status") {
                status.as_str()?;
            }
            if item_type == "agent_message" {
                item.get("text")?.as_str()?;
            }
            match name {
                "item.started" => ItemStarted,
                "item.updated" => ItemUpdated,
                _ => ItemCompleted,
            }
        }
        "error" => {
            value.get("message")?.as_str()?;
            Error
        }
        _ => Unknown,
    };
    Some(kind)
}
