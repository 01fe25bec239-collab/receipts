//! Fail-closed error surface for Workspace-Execution provisioning.

use std::fmt;

/// Errors produced by the Workspace-Execution foundation.
///
/// Every failure mode of provisioning a task worktree surfaces as an
/// explicit error; a failed or ambiguous operation is never converted into
/// a successful [`WorkspaceHandle`](crate::handle::WorkspaceHandle).
#[derive(Debug)]
#[non_exhaustive]
pub enum WorkspaceError {
    /// A commit SHA was not exactly 40 lowercase hexadecimal characters.
    ///
    /// `base_sha` and any populated `head_sha` must be exact lowercase
    /// 40-character hexadecimal Git commit SHAs.
    CommitShaInvalid {
        /// The rejected value, preserved exactly for diagnostics.
        value: String,
    },
    /// A `workspace_id` failed structural validation.
    InvalidWorkspaceId {
        /// What is wrong with the identifier.
        detail: String,
    },
    /// An optional `task_id` failed structural validation.
    InvalidTaskId {
        /// What is wrong with the identifier.
        detail: String,
    },
    /// A branch name failed structural validation before any Git command.
    BranchNameInvalid {
        /// The rejected branch name.
        branch: String,
        /// Which frozen constraint was violated.
        reason: &'static str,
    },
    /// A worktree path failed structural validation before any Git command.
    InvalidWorktreePath {
        /// What is wrong with the path.
        detail: String,
    },
    /// The requested base could not be verified as an existing Git commit.
    ///
    /// This also covers the case where the repository root is not a usable
    /// Git repository at all; `detail` carries the Git-reported reason.
    BaseCommitVerificationFailed {
        /// The requested base SHA, already syntax-validated.
        base_sha: String,
        /// Git-reported failure detail.
        detail: String,
    },
    /// The `git` executable could not be spawned.
    ///
    /// Distinct from [`WorkspaceError::GitCommandFailed`]: the process never
    /// ran, so no exit status or stderr exists.
    GitSpawnFailed {
        /// Provisioning step that attempted the invocation.
        operation: &'static str,
        /// Underlying spawn failure detail.
        detail: String,
    },
    /// A Git command exited unsuccessfully.
    GitCommandFailed {
        /// Provisioning step whose Git command failed.
        operation: &'static str,
        /// Exit status and stderr captured from the failed command.
        detail: String,
    },
    /// The resulting worktree HEAD could not be read after provisioning.
    WorktreeHeadUnavailable {
        /// Exit status and stderr captured from the failed read.
        detail: String,
    },
    /// The resulting worktree HEAD did not equal the requested base SHA.
    ///
    /// Provisioning requires `HEAD == base_sha` exactly; any other value
    /// fails closed and never yields a handle.
    WorktreeHeadMismatch {
        /// The requested base SHA.
        expected: String,
        /// The HEAD actually observed in the new worktree.
        observed: String,
    },
    /// The newly provisioned worktree was not clean per `git status
    /// --porcelain`.
    WorktreeNotClean {
        /// The non-empty porcelain status output.
        status: String,
    },
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WorkspaceError::CommitShaInvalid { value } => write!(
                f,
                "commit SHA {value:?} is not an exact lowercase 40-character hexadecimal Git commit SHA"
            ),
            WorkspaceError::InvalidWorkspaceId { detail } => {
                write!(f, "invalid workspace_id: {detail}")
            }
            WorkspaceError::InvalidTaskId { detail } => write!(f, "invalid task_id: {detail}"),
            WorkspaceError::BranchNameInvalid { branch, reason } => {
                write!(f, "invalid branch name {branch:?}: {reason}")
            }
            WorkspaceError::InvalidWorktreePath { detail } => {
                write!(f, "invalid worktree path: {detail}")
            }
            WorkspaceError::BaseCommitVerificationFailed { base_sha, detail } => write!(
                f,
                "base commit {base_sha} could not be verified as an existing Git commit: {detail}"
            ),
            WorkspaceError::GitSpawnFailed { operation, detail } => {
                write!(f, "failed to spawn git for {operation}: {detail}")
            }
            WorkspaceError::GitCommandFailed { operation, detail } => {
                write!(f, "git command for {operation} failed: {detail}")
            }
            WorkspaceError::WorktreeHeadUnavailable { detail } => {
                write!(f, "resulting worktree HEAD could not be read: {detail}")
            }
            WorkspaceError::WorktreeHeadMismatch { expected, observed } => write!(
                f,
                "resulting worktree HEAD {observed} does not equal the requested base SHA {expected}; refusing to return a workspace handle"
            ),
            WorkspaceError::WorktreeNotClean { status } => write!(
                f,
                "newly provisioned worktree is not clean according to git status --porcelain: {status}"
            ),
        }
    }
}

impl std::error::Error for WorkspaceError {}
