//! The typed [`WorkspaceHandle`] and its frozen vocabulary.

use std::path::Path;

use crate::error::WorkspaceError;

/// An exact lowercase 40-character hexadecimal Git commit SHA.
///
/// Construction is the only admission path: any other shape — uppercase
/// digits, wrong length, non-hexadecimal characters, abbreviations — fails
/// closed, so a valid `CommitSha` guarantees the frozen representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSha {
    value: String,
}

impl CommitSha {
    /// Parses only the one frozen SHA representation: exactly 40 lowercase
    /// hexadecimal characters.
    pub fn parse(value: &str) -> Result<Self, WorkspaceError> {
        let exact_shape = value.len() == 40
            && value
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
        if !exact_shape {
            return Err(WorkspaceError::CommitShaInvalid {
                value: value.to_string(),
            });
        }
        Ok(Self {
            value: value.to_string(),
        })
    }

    /// Returns the exact validated representation.
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

/// Frozen WorkspaceHandle state vocabulary.
///
/// This slice creates only `PROVISIONED` handles; every transition out of
/// that state belongs to later milestones outside this crate's current
/// scope. No new values may ever be invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WorkspaceState {
    /// `PROVISIONED`: branch and worktree created, HEAD verified against
    /// the requested base, worktree verified clean.
    Provisioned,
    /// `ACTIVE`.
    Active,
    /// `CHECKPOINTED`.
    Checkpointed,
    /// `ORPHANED`.
    Orphaned,
    /// `RECOVERED`.
    Recovered,
    /// `TORN_DOWN`.
    TornDown,
}

impl WorkspaceState {
    /// The frozen storage/contract representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            WorkspaceState::Provisioned => "PROVISIONED",
            WorkspaceState::Active => "ACTIVE",
            WorkspaceState::Checkpointed => "CHECKPOINTED",
            WorkspaceState::Orphaned => "ORPHANED",
            WorkspaceState::Recovered => "RECOVERED",
            WorkspaceState::TornDown => "TORN_DOWN",
        }
    }
}

/// Frozen isolation semantics for provisioned workspaces.
///
/// A Git worktree provides workspace isolation only. It is NOT a security
/// sandbox: nothing about a provisioned worktree contains, confines, or
/// isolates processes, filesystem access beyond the worktree directory, or
/// network access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WorkspaceIsolation {
    /// `WORKSPACE_ISOLATION`: Git-worktree-level workspace isolation.
    WorkspaceIsolation,
}

impl WorkspaceIsolation {
    /// The frozen contract representation.
    pub fn as_str(self) -> &'static str {
        "WORKSPACE_ISOLATION"
    }
}

/// Typed proof that a task workspace was successfully provisioned.
///
/// A handle exists only after every provisioning validation has succeeded:
/// the base commit was verified to exist, the branch was created from that
/// exact commit, the worktree was created at the requested path, the
/// resulting HEAD equaled the requested base SHA, and the worktree was
/// verified clean. There is no way to construct a handle around a failed
/// or ambiguous operation.
///
/// Fields are private and read-only through accessors; handles are
/// immutable evidence, never mutable configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceHandle {
    workspace_id: String,
    branch: String,
    worktree_path: Box<Path>,
    base_sha: CommitSha,
    state: WorkspaceState,
    task_id: Option<String>,
    head_sha: Option<CommitSha>,
    isolation: WorkspaceIsolation,
}

impl WorkspaceHandle {
    /// Creates the only handle shape this slice may produce: a
    /// `PROVISIONED` handle whose verified head equals its base.
    pub(crate) fn provisioned(
        workspace_id: String,
        task_id: Option<String>,
        branch: String,
        worktree_path: Box<Path>,
        base_sha: CommitSha,
    ) -> Self {
        Self {
            workspace_id,
            branch,
            worktree_path,
            head_sha: Some(base_sha.clone()),
            base_sha,
            state: WorkspaceState::Provisioned,
            isolation: WorkspaceIsolation::WorkspaceIsolation,
            task_id,
        }
    }

    /// The durable workspace identity supplied by the caller.
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    /// The optional task identity supplied by the caller.
    pub fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }

    /// The task branch backing this workspace.
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// The local worktree path holding the workspace checkout.
    pub fn worktree_path(&self) -> &Path {
        &self.worktree_path
    }

    /// The exact base commit the branch was created from.
    pub fn base_sha(&self) -> &CommitSha {
        &self.base_sha
    }

    /// The verified worktree head; equals `base_sha` after provisioning.
    pub fn head_sha(&self) -> Option<&CommitSha> {
        self.head_sha.as_ref()
    }

    /// The frozen lifecycle state, always `PROVISIONED` in this slice.
    pub fn state(&self) -> WorkspaceState {
        self.state
    }

    /// The frozen isolation semantics of the backing worktree.
    pub fn isolation(&self) -> WorkspaceIsolation {
        self.isolation
    }
}
