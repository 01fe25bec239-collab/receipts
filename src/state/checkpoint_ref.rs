//! Schema-shaped checkpoint metadata; nested references are opaque evidence.
use crate::error::{GraphFailureCode as Code, GraphPhase as Phase, StateError};
use crate::graph_node_result::{date_time, invalid, present};
use crate::graph_repository::{self as graph, hex, id};
use crate::repository::SqliteStateRepository;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::fmt;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    ref_type: String,
    target: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    digest: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    section: Option<String>,
}
impl Reference {
    fn valid(&self) -> bool {
        matches!(
            self.ref_type.as_str(),
            "REPO_PATH" | "STATE_QUERY" | "ARTIFACT_ID" | "URL"
        ) && !self.target.is_empty()
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Check {
    source: String,
    command: Vec<String>,
    exit_code: Box<RawValue>,
    code_sha: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    timed_out: Option<bool>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    started_at: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    finished_at: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    output_ref: Option<Reference>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    result: Option<String>,
}
impl Check {
    fn valid(&self) -> bool {
        matches!(
            self.source.as_str(),
            "WORKER_EXECUTION" | "BROKER_EXECUTION" | "REVIEW_EXECUTION" | "GIT_PROVENANCE"
        ) && !self.command.is_empty()
            && crate::json_integer::is_integer(&self.exit_code)
            && hex(&self.code_sha, 40)
            && [&self.started_at, &self.finished_at]
                .iter()
                .all(|s| s.as_deref().is_none_or(date_time))
            && self.output_ref.as_ref().is_none_or(Reference::valid)
            && self
                .result
                .as_deref()
                .is_none_or(|s| matches!(s, "PASS" | "FAIL" | "ERROR" | "SKIPPED" | "UNKNOWN"))
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    checkpoint_id: String,
    workspace_id: String,
    head_sha: String,
    captured_at: String,
    kind: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    task_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    attempt_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    base_sha: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    dirty_diff_ref: Option<Reference>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    modified_files: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    untracked_files: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    executed_checks: Option<Vec<Check>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    crash_classification: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    recovery_decision: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    recovery_rationale: Option<String>,
}
/// Validated complete checkpoint schema with an external caller partition.
pub struct CheckpointRecord {
    project_id: String,
    wire: Wire,
}
impl fmt::Debug for CheckpointRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CheckpointRecord")
    }
}
impl CheckpointRecord {
    pub fn parse_json(project_id: &str, text: &str) -> Result<Self, StateError> {
        let wire: Wire = serde_json::from_str(text).map_err(|_| invalid())?;
        if !id(project_id)
            || !id(&wire.checkpoint_id)
            || !id(&wire.workspace_id)
            || !hex(&wire.head_sha, 40)
            || !date_time(&wire.captured_at)
            || !matches!(
                wire.kind.as_str(),
                "PROGRESS" | "PRE_TERMINATION" | "RECOVERY_CAPTURE"
            )
            || [&wire.task_id, &wire.attempt_id]
                .iter()
                .any(|s| s.as_deref().is_some_and(|s| !id(s)))
            || wire.base_sha.as_deref().is_some_and(|s| !hex(s, 40))
            || wire.dirty_diff_ref.as_ref().is_some_and(|r| !r.valid())
            || wire
                .executed_checks
                .as_ref()
                .is_some_and(|cs| cs.iter().any(|c| !c.valid()))
            || wire.crash_classification.as_deref().is_some_and(|s| {
                !matches!(
                    s,
                    "RATE_LIMITED"
                        | "SESSION_EXHAUSTED"
                        | "AUTH_REQUIRED"
                        | "PROVIDER_DOWN"
                        | "TIMEOUT"
                        | "SANDBOX_DENIED"
                        | "SAFETY_CHECK_PENDING"
                        | "POLICY_BLOCKED"
                        | "RUNTIME_CRASH"
                        | "INVALID_OUTPUT"
                        | "USER_CANCELLED"
                        | "UNKNOWN"
                )
            })
            || wire.recovery_decision.as_deref().is_some_and(|s| {
                !matches!(
                    s,
                    "RESET_TO_LAST_ACCEPTED" | "CONTINUE_FROM_CHECKPOINT" | "INSPECT_AND_SALVAGE"
                )
            })
        {
            return Err(invalid());
        }
        Ok(Self {
            project_id: project_id.into(),
            wire,
        })
    }
    pub fn to_json(&self) -> Result<String, StateError> {
        serde_json::to_string(&self.wire).map_err(|_| invalid())
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn checkpoint_id(&self) -> &str {
        &self.wire.checkpoint_id
    }
    pub fn workspace_id(&self) -> &str {
        &self.wire.workspace_id
    }
    pub fn captured_at(&self) -> &str {
        &self.wire.captured_at
    }
}

// Validated RFC3339 evidence can have unlimited fractions and a leap second.
// Keep leap seconds between :59 and the next minute instead of collapsing them.
fn instant(text: &str) -> (i64, bool, &str) {
    let number = |a: usize, z: usize| text[a..z].parse::<i64>().expect("validated date");
    let y = number(0, 4);
    let m = number(5, 7);
    let d = number(8, 10);
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let days = 365 * y + (y + 3) / 4 - (y + 99) / 100
        + (y + 399) / 400
        + months[..(m - 1) as usize].iter().sum::<i64>()
        + d
        - 1;
    let second = number(17, 19);
    let mut end = 19;
    let fraction = if text.as_bytes()[end] == b'.' {
        end += 1;
        let start = end;
        while text.as_bytes()[end].is_ascii_digit() {
            end += 1;
        }
        text[start..end].trim_end_matches('0')
    } else {
        ""
    };
    let offset = if matches!(text.as_bytes()[end], b'Z' | b'z') {
        0
    } else {
        let minutes = number(end + 1, end + 3) * 60 + number(end + 4, end + 6);
        if text.as_bytes()[end] == b'+' {
            minutes
        } else {
            -minutes
        }
    };
    (
        days * 86400 + number(11, 13) * 3600 + number(14, 16) * 60 + second.min(59) - offset * 60,
        second == 60,
        fraction,
    )
}
fn decode(row: &rusqlite::Row<'_>) -> Result<CheckpointRecord, StateError> {
    let text = |i| {
        row.get::<_, String>(i)
            .map_err(|e| graph::sqlite_error(Phase::Read, e))
    };
    let value = CheckpointRecord::parse_json(&text(0)?, &text(3)?).map_err(|_| graph::corrupt())?;
    if value.checkpoint_id() != text(1)? || value.workspace_id() != text(2)? {
        return Err(graph::corrupt());
    }
    Ok(value)
}
impl SqliteStateRepository {
    pub fn append_checkpoint(&mut self, value: CheckpointRecord) -> Result<(), StateError> {
        graph::write_transaction(self.connection(), |conn| {
            let exists: bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM workspace_checkpoints WHERE project_id=?1 AND checkpoint_id=?2)",params![value.project_id(),value.checkpoint_id()], |r|r.get(0)).map_err(|e|graph::sqlite_error(Phase::Write,e))?;
            if exists {
                return Err(graph::failure(Phase::Write, Code::AlreadyExists, None));
            }
            conn.execute("INSERT INTO workspace_checkpoints(project_id,checkpoint_id,workspace_id,checkpoint_json) VALUES (?1,?2,?3,?4)", params![value.project_id(),value.checkpoint_id(),value.workspace_id(),value.to_json()?]).map_err(|e|graph::sqlite_error(Phase::Write,e))?;
            Ok(())
        })
    }
    pub fn find_checkpoint(
        &self,
        project_id: &str,
        checkpoint_id: &str,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        if !id(project_id) || !id(checkpoint_id) {
            return Err(invalid());
        }
        crate::graph_history::read_transaction(self.connection(), |conn| {
            conn.query_row("SELECT project_id,checkpoint_id,workspace_id,checkpoint_json FROM workspace_checkpoints WHERE project_id=?1 AND checkpoint_id=?2",params![project_id,checkpoint_id],|r|Ok(decode(r))).optional().map_err(|e|graph::sqlite_error(Phase::Read,e))?.transpose()
        })
    }
    /// Ordered by exact UTC instant and then binary checkpoint identity.
    pub fn list_checkpoints(
        &self,
        project_id: &str,
        workspace_id: &str,
    ) -> Result<Vec<CheckpointRecord>, StateError> {
        if !id(project_id) || !id(workspace_id) {
            return Err(invalid());
        }
        crate::graph_history::read_transaction(self.connection(), |conn| {
            let mut statement=conn.prepare("SELECT project_id,checkpoint_id,workspace_id,checkpoint_json FROM workspace_checkpoints WHERE project_id=?1 AND workspace_id=?2").map_err(|e|graph::sqlite_error(Phase::Read,e))?;
            let rows = statement
                .query_map(params![project_id, workspace_id], |r| Ok(decode(r)))
                .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
            let mut values = rows
                .map(|r| r.map_err(|e| graph::sqlite_error(Phase::Read, e))?)
                .collect::<Result<Vec<_>, _>>()?;
            values.sort_by(|a, b| {
                instant(a.captured_at())
                    .cmp(&instant(b.captured_at()))
                    .then_with(|| {
                        a.checkpoint_id()
                            .as_bytes()
                            .cmp(b.checkpoint_id().as_bytes())
                    })
            });
            Ok(values)
        })
    }
    pub fn latest_checkpoint(
        &self,
        project_id: &str,
        workspace_id: &str,
    ) -> Result<Option<CheckpointRecord>, StateError> {
        Ok(self.list_checkpoints(project_id, workspace_id)?.pop())
    }
}
