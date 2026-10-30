//! The typed [`WorkspaceHandle`] and its frozen vocabulary.

use std::path::Path;

use crate::date_time::WorkspaceDateTimeV1;
use crate::error::WorkspaceError;
use crate::remote_publish_policy::WorkspaceRemotePublishPolicy;

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

/// Frozen WorkspaceHandle state vocabulary with one checked transition oracle
/// ([`WorkspaceState::may_transition_to`]). Operational transitions require
/// observed evidence (see [`crate::lifecycle`]); a state value alone, or a
/// decoded wire record, never moves a handle. No new values may be invented.
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
    /// All six frozen states.
    pub const ALL: [Self; 6] = [
        Self::Provisioned,
        Self::Active,
        Self::Checkpointed,
        Self::Orphaned,
        Self::Recovered,
        Self::TornDown,
    ];

    /// The complete legal transition relation (no self-transitions):
    ///
    /// * `PROVISIONED`/`CHECKPOINTED`/`RECOVERED` → `ACTIVE`: an attempt start
    ///   was observed;
    /// * `ACTIVE` → `CHECKPOINTED`: a checkpoint capture succeeded;
    /// * `PROVISIONED`/`ACTIVE`/`CHECKPOINTED` → `ORPHANED`: lifecycle
    ///   evidence shows the owning attempt's cleanup is unknown;
    /// * `ORPHANED` → `RECOVERED`: recovery evidence (recovery execution is a
    ///   later wave; no public producer exists yet);
    /// * `PROVISIONED`/`CHECKPOINTED`/`RECOVERED` → `TORN_DOWN`: verified
    ///   non-force teardown. `TORN_DOWN` is terminal.
    pub const fn may_transition_to(self, to: Self) -> bool {
        use WorkspaceState::*;
        matches!(
            (self, to),
            (Provisioned | Checkpointed | Recovered, Active)
                | (Active, Checkpointed)
                | (Provisioned | Active | Checkpointed, Orphaned)
                | (Orphaned, Recovered)
                | (Provisioned | Checkpointed | Recovered, TornDown)
        )
    }

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
    remote_publish_policy: Option<WorkspaceRemotePublishPolicy>,
    created_at: Option<WorkspaceDateTimeV1>,
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
        remote_publish_policy: Option<WorkspaceRemotePublishPolicy>,
        created_at: Option<WorkspaceDateTimeV1>,
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
            remote_publish_policy,
            created_at,
        }
    }

    /// The only state-changing constructor: applies one checked transition,
    /// preserving every identity field. `head` replaces the verified head
    /// only when supplied as observed evidence by the caller.
    pub(crate) fn transitioned(
        &self,
        to: WorkspaceState,
        head: Option<CommitSha>,
    ) -> Result<Self, WorkspaceError> {
        if !self.state.may_transition_to(to) {
            return Err(WorkspaceError::IllegalStateTransition {
                from: self.state.as_str(),
                to: to.as_str(),
            });
        }
        let mut next = self.clone();
        next.state = to;
        if let Some(head) = head {
            next.head_sha = Some(head);
        }
        Ok(next)
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

    /// The verified worktree head: `base_sha` after provisioning, the
    /// captured head after a checkpoint, and the exact commit observed
    /// immediately before removal after teardown.
    pub fn head_sha(&self) -> Option<&CommitSha> {
        self.head_sha.as_ref()
    }

    /// The current checked lifecycle state.
    pub fn state(&self) -> WorkspaceState {
        self.state
    }

    /// The frozen isolation semantics of the backing worktree.
    pub fn isolation(&self) -> WorkspaceIsolation {
        self.isolation
    }

    /// The explicitly supplied policy, stored without granting remote authority.
    pub fn remote_publish_policy(&self) -> Option<WorkspaceRemotePublishPolicy> {
        self.remote_publish_policy
    }

    /// The caller-supplied creation timestamp, preserved exactly; `None` means absent.
    pub fn created_at(&self) -> Option<&WorkspaceDateTimeV1> {
        self.created_at.as_ref()
    }
}
