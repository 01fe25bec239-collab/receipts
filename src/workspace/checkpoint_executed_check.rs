use std::fmt;

use crate::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckResult,
    WorkspaceCheckpointRef,
};

/// Bounded core of one `WorkspaceCheckpoint.executed_checks[]` entry.
///
/// Records evidence of an explicitly-invoked argv command and its observed
/// outcome. This is the deliberately bounded `Core` slice: `started_at`,
/// `finished_at` remain out of scope. Result is optional and caller supplied only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCheckpointExecutedCheckCore {
    source: WorkspaceCheckpointCheckSource,
    command: Vec<String>,
    exit_code: i64,
    code_sha: CommitSha,
    timed_out: Option<bool>,
    output_ref: Option<WorkspaceCheckpointRef>,
    result: Option<WorkspaceCheckpointExecutedCheckResult>,
}

impl WorkspaceCheckpointExecutedCheckCore {
    /// Records one executed check core.
    ///
    /// The only new validation performed here is the schema `minItems: 1`
    /// contract on `command`: an empty argv vector is rejected. Individual
    /// argv elements carry no length contract and are preserved exactly.
    pub fn new(
        source: WorkspaceCheckpointCheckSource,
        command: Vec<String>,
        exit_code: i64,
        code_sha: CommitSha,
        timed_out: Option<bool>,
        output_ref: Option<WorkspaceCheckpointRef>,
    ) -> Result<Self, WorkspaceCheckpointExecutedCheckCoreError> {
        if command.is_empty() {
            return Err(WorkspaceCheckpointExecutedCheckCoreError::EmptyCommand);
        }
        Ok(Self {
            source,
            command,
            exit_code,
            code_sha,
            timed_out,
            output_ref,
            result: None,
        })
    }

    /// Stores exactly the caller-supplied result, including absence.
    /// No other evidence field is interpreted or validated by this method.
    pub fn with_result(mut self, result: Option<WorkspaceCheckpointExecutedCheckResult>) -> Self {
        self.result = result;
        self
    }

    pub const fn result(&self) -> Option<WorkspaceCheckpointExecutedCheckResult> {
        self.result
    }

    pub const fn source(&self) -> WorkspaceCheckpointCheckSource {
        self.source
    }

    pub fn command(&self) -> &[String] {
        &self.command
    }

    pub const fn exit_code(&self) -> i64 {
        self.exit_code
    }

    pub fn code_sha(&self) -> &CommitSha {
        &self.code_sha
    }

    pub const fn timed_out(&self) -> Option<bool> {
        self.timed_out
    }

    pub fn output_ref(&self) -> Option<&WorkspaceCheckpointRef> {
        self.output_ref.as_ref()
    }
}

/// Narrow typed constructor failure for
/// [`WorkspaceCheckpointExecutedCheckCore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCheckpointExecutedCheckCoreError {
    EmptyCommand,
}

impl fmt::Display for WorkspaceCheckpointExecutedCheckCoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCommand => {
                f.write_str("workspace checkpoint executed-check command is empty")
            }
        }
    }
}

impl std::error::Error for WorkspaceCheckpointExecutedCheckCoreError {}
