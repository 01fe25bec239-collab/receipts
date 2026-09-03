use std::fmt;

use crate::{
    CommitSha, WorkspaceCheckpointExecutedCheckCore, WorkspaceCheckpointKind,
    WorkspaceCheckpointRef, WorkspaceRecoveryDecision,
};

/// Bounded inert captured-state value object for one workspace checkpoint.
///
/// This is intentionally `WorkspaceCheckpointCaptureCore`, not the full
/// `WorkspaceCheckpoint` schema record: `captured_at` and
/// `crash_classification` are deliberately deferred and must not be added
/// here. All supplied values are already-observed evidence and are stored
/// exactly; no filesystem discovery, Git inspection, check execution,
/// reference resolution, digest calculation, recovery execution, or
/// persistence is performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCheckpointCaptureCore {
    checkpoint_id: String,
    workspace_id: String,
    task_id: Option<String>,
    attempt_id: Option<String>,
    kind: WorkspaceCheckpointKind,
    head_sha: CommitSha,
    base_sha: Option<CommitSha>,
    dirty_diff_ref: Option<WorkspaceCheckpointRef>,
    modified_files: Vec<String>,
    untracked_files: Vec<String>,
    executed_checks: Vec<WorkspaceCheckpointExecutedCheckCore>,
    recovery_decision: Option<WorkspaceRecoveryDecision>,
    recovery_rationale: Option<String>,
}

impl WorkspaceCheckpointCaptureCore {
    /// Captures the supplied already-observed checkpoint values.
    ///
    /// Only the four ID length contracts are validated, using Unicode
    /// character count (`chars().count()`): required IDs must be 1..=200
    /// characters, optional IDs when `Some` must be 1..=200 characters.
    /// Everything else is already typed or schema-unconstrained at this
    /// bounded layer and is stored exactly.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        checkpoint_id: impl Into<String>,
        workspace_id: impl Into<String>,
        task_id: Option<String>,
        attempt_id: Option<String>,
        kind: WorkspaceCheckpointKind,
        head_sha: CommitSha,
        base_sha: Option<CommitSha>,
        dirty_diff_ref: Option<WorkspaceCheckpointRef>,
        modified_files: Vec<String>,
        untracked_files: Vec<String>,
        executed_checks: Vec<WorkspaceCheckpointExecutedCheckCore>,
        recovery_decision: Option<WorkspaceRecoveryDecision>,
        recovery_rationale: Option<String>,
    ) -> Result<Self, WorkspaceCheckpointCaptureCoreError> {
        let checkpoint_id = checkpoint_id.into();
        let workspace_id = workspace_id.into();
        validate_required_id(&checkpoint_id, true)?;
        validate_required_id(&workspace_id, false)?;
        validate_optional_id(task_id.as_deref(), true)?;
        validate_optional_id(attempt_id.as_deref(), false)?;

        Ok(Self {
            checkpoint_id,
            workspace_id,
            task_id,
            attempt_id,
            kind,
            head_sha,
            base_sha,
            dirty_diff_ref,
            modified_files,
            untracked_files,
            executed_checks,
            recovery_decision,
            recovery_rationale,
        })
    }

    pub fn checkpoint_id(&self) -> &str {
        &self.checkpoint_id
    }

    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    pub fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }

    pub fn attempt_id(&self) -> Option<&str> {
        self.attempt_id.as_deref()
    }

    pub const fn kind(&self) -> WorkspaceCheckpointKind {
        self.kind
    }

    pub fn head_sha(&self) -> &CommitSha {
        &self.head_sha
    }

    pub fn base_sha(&self) -> Option<&CommitSha> {
        self.base_sha.as_ref()
    }

    pub fn dirty_diff_ref(&self) -> Option<&WorkspaceCheckpointRef> {
        self.dirty_diff_ref.as_ref()
    }

    pub fn modified_files(&self) -> &[String] {
        &self.modified_files
    }

    pub fn untracked_files(&self) -> &[String] {
        &self.untracked_files
    }

    pub fn executed_checks(&self) -> &[WorkspaceCheckpointExecutedCheckCore] {
        &self.executed_checks
    }

    pub const fn recovery_decision(&self) -> Option<WorkspaceRecoveryDecision> {
        self.recovery_decision
    }

    pub fn recovery_rationale(&self) -> Option<&str> {
        self.recovery_rationale.as_deref()
    }
}

fn validate_required_id(
    value: &str,
    is_checkpoint: bool,
) -> Result<(), WorkspaceCheckpointCaptureCoreError> {
    let len = value.chars().count();
    if len == 0 {
        return Err(if is_checkpoint {
            WorkspaceCheckpointCaptureCoreError::EmptyCheckpointId
        } else {
            WorkspaceCheckpointCaptureCoreError::EmptyWorkspaceId
        });
    }
    if len > 200 {
        return Err(if is_checkpoint {
            WorkspaceCheckpointCaptureCoreError::CheckpointIdTooLong
        } else {
            WorkspaceCheckpointCaptureCoreError::WorkspaceIdTooLong
        });
    }
    Ok(())
}

fn validate_optional_id(
    value: Option<&str>,
    is_task: bool,
) -> Result<(), WorkspaceCheckpointCaptureCoreError> {
    let Some(value) = value else {
        return Ok(());
    };
    let len = value.chars().count();
    if len == 0 {
        return Err(if is_task {
            WorkspaceCheckpointCaptureCoreError::EmptyTaskId
        } else {
            WorkspaceCheckpointCaptureCoreError::EmptyAttemptId
        });
    }
    if len > 200 {
        return Err(if is_task {
            WorkspaceCheckpointCaptureCoreError::TaskIdTooLong
        } else {
            WorkspaceCheckpointCaptureCoreError::AttemptIdTooLong
        });
    }
    Ok(())
}

/// Narrow typed constructor failure for [`WorkspaceCheckpointCaptureCore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCheckpointCaptureCoreError {
    EmptyCheckpointId,
    CheckpointIdTooLong,
    EmptyWorkspaceId,
    WorkspaceIdTooLong,
    EmptyTaskId,
    TaskIdTooLong,
    EmptyAttemptId,
    AttemptIdTooLong,
}

impl fmt::Display for WorkspaceCheckpointCaptureCoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCheckpointId => {
                f.write_str("workspace checkpoint capture checkpoint_id is empty")
            }
            Self::CheckpointIdTooLong => {
                f.write_str("workspace checkpoint capture checkpoint_id exceeds 200 characters")
            }
            Self::EmptyWorkspaceId => {
                f.write_str("workspace checkpoint capture workspace_id is empty")
            }
            Self::WorkspaceIdTooLong => {
                f.write_str("workspace checkpoint capture workspace_id exceeds 200 characters")
            }
            Self::EmptyTaskId => f.write_str("workspace checkpoint capture task_id is empty"),
            Self::TaskIdTooLong => {
                f.write_str("workspace checkpoint capture task_id exceeds 200 characters")
            }
            Self::EmptyAttemptId => f.write_str("workspace checkpoint capture attempt_id is empty"),
            Self::AttemptIdTooLong => {
                f.write_str("workspace checkpoint capture attempt_id exceeds 200 characters")
            }
        }
    }
}

impl std::error::Error for WorkspaceCheckpointCaptureCoreError {}
