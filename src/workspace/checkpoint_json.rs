//! Schema-shaped JSON export of an already-captured temporal checkpoint.

use std::fmt;
use std::io::Write;

use serde::Serialize;

use crate::{
    WorkspaceCheckpointExecutedCheckCore, WorkspaceCheckpointRef,
    WorkspaceCheckpointTemporalCaptureCore,
};

/// Serialization failed. The cause is deliberately dropped: it could echo
/// captured paths, argv or reference targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspaceCheckpointJsonExportError;

impl fmt::Display for WorkspaceCheckpointJsonExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("workspace checkpoint JSON export failed")
    }
}

impl std::error::Error for WorkspaceCheckpointJsonExportError {}

/// Encodes the typed checkpoint as `WorkspaceCheckpoint.schema.json`-shaped JSON.
///
/// Every captured value is copied unchanged: strings and argv byte-for-byte,
/// `exit_code` as an exact `i64`, timestamps with their supplied spelling
/// (no normalization; the schema `date-time` format is an annotation, not
/// asserted). `None` optionals are omitted, never `null`; the three arrays are
/// always present, even when empty. Nothing is observed, resolved, decided or
/// persisted, and the input is not modified. This is an encoder only: there is
/// no decoder and no admission of arbitrary wire input.
pub fn workspace_checkpoint_to_json(
    checkpoint: &WorkspaceCheckpointTemporalCaptureCore,
) -> Result<String, WorkspaceCheckpointJsonExportError> {
    let mut out = Vec::new();
    write_workspace_checkpoint_json(checkpoint, &mut out)?;
    // serde_json emits valid UTF-8 only.
    String::from_utf8(out).map_err(|_| WorkspaceCheckpointJsonExportError)
}

pub(crate) fn write_workspace_checkpoint_json<W: Write>(
    checkpoint: &WorkspaceCheckpointTemporalCaptureCore,
    writer: W,
) -> Result<(), WorkspaceCheckpointJsonExportError> {
    serde_json::to_writer(writer, &CheckpointFields::from(checkpoint))
        .map_err(|_| WorkspaceCheckpointJsonExportError)
}

impl<'a> From<&'a WorkspaceCheckpointTemporalCaptureCore> for CheckpointFields<'a> {
    fn from(checkpoint: &'a WorkspaceCheckpointTemporalCaptureCore) -> Self {
        let core = checkpoint.core();
        Self {
            checkpoint_id: core.checkpoint_id(),
            workspace_id: core.workspace_id(),
            task_id: core.task_id(),
            attempt_id: core.attempt_id(),
            kind: core.kind().as_str(),
            head_sha: core.head_sha().as_str(),
            base_sha: core.base_sha().map(|sha| sha.as_str()),
            dirty_diff_ref: core.dirty_diff_ref().map(RefFields::from),
            modified_files: core.modified_files(),
            untracked_files: core.untracked_files(),
            executed_checks: core
                .executed_checks()
                .iter()
                .map(CheckFields::from)
                .collect(),
            crash_classification: core.crash_classification().map(|value| value.as_str()),
            recovery_decision: core.recovery_decision().map(|value| value.as_str()),
            recovery_rationale: core.recovery_rationale(),
            captured_at: checkpoint.captured_at().as_str(),
        }
    }
}

#[derive(Serialize)]
struct CheckpointFields<'a> {
    checkpoint_id: &'a str,
    workspace_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attempt_id: Option<&'a str>,
    kind: &'static str,
    head_sha: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_sha: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dirty_diff_ref: Option<RefFields<'a>>,
    modified_files: &'a [String],
    untracked_files: &'a [String],
    executed_checks: Vec<CheckFields<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    crash_classification: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_decision: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_rationale: Option<&'a str>,
    captured_at: &'a str,
}

#[derive(Serialize)]
struct CheckFields<'a> {
    source: &'static str,
    command: &'a [String],
    exit_code: i64,
    code_sha: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    timed_out: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    started_at: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    finished_at: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_ref: Option<RefFields<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<&'static str>,
}

impl<'a> From<&'a WorkspaceCheckpointExecutedCheckCore> for CheckFields<'a> {
    fn from(check: &'a WorkspaceCheckpointExecutedCheckCore) -> Self {
        Self {
            source: check.source().as_str(),
            command: check.command(),
            exit_code: check.exit_code(),
            code_sha: check.code_sha().as_str(),
            timed_out: check.timed_out(),
            started_at: check.started_at().map(|time| time.as_str()),
            finished_at: check.finished_at().map(|time| time.as_str()),
            output_ref: check.output_ref().map(RefFields::from),
            result: check.result().map(|value| value.as_str()),
        }
    }
}

#[derive(Serialize)]
struct RefFields<'a> {
    ref_type: &'static str,
    target: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    digest: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    section: Option<&'a str>,
}

impl<'a> From<&'a WorkspaceCheckpointRef> for RefFields<'a> {
    fn from(reference: &'a WorkspaceCheckpointRef) -> Self {
        Self {
            ref_type: reference.ref_type().as_str(),
            target: reference.target(),
            digest: reference.digest(),
            section: reference.section(),
        }
    }
}
