//! Fail-closed error surface for Workspace-Execution provisioning and
//! teardown.

use std::fmt;

/// Errors produced by the Workspace-Execution foundation.
///
/// Every failure mode of provisioning a task worktree — and every failure
/// mode of tearing a provisioned worktree down again — surfaces as an
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
    /// No executable `git` binary could be located at an absolute,
    /// canonical filesystem path.
    ///
    /// Provisioning never spawns an unqualified `git` lookup; resolution
    /// fails closed before any subprocess exists.
    GitExecutableUnavailable {
        /// Where the resolver searched and what went wrong.
        detail: String,
    },
    /// The repository root could not be resolved to its canonical
    /// (realpath) filesystem location before any Git command ran.
    RepositoryRootUnresolvable {
        /// The rejected root path and the underlying resolution failure.
        detail: String,
    },
    /// The newly created worktree directory could not be resolved to its
    /// canonical (realpath) location after `git worktree add` succeeded, so
    /// post-provision verification refused to proceed.
    CreatedWorktreeUnresolvable {
        /// The created-worktree path and the underlying resolution failure.
        detail: String,
    },
    /// A filesystem path about to become the working directory of a Git
    /// subprocess could not be canonicalized at the execution boundary.
    ///
    /// Defense in depth for the frozen realpath-CWD requirement: every
    /// production invocation passes through this check even if a future
    /// call site forwards a lexical path.
    SubprocessCwdUnresolvable {
        /// Provisioning step that attempted the invocation.
        operation: &'static str,
        /// The rejected path and the underlying resolution failure.
        detail: String,
    },
    /// Teardown was invoked on a handle whose lifecycle state is outside
    /// this slice's single frozen transition (`PROVISIONED` → `TORN_DOWN`).
    ///
    /// Teardown never silently succeeds for an already-torn-down or any
    /// other non-provisioned handle; such requests fail closed.
    TeardownUnsupportedState {
        /// The unsupported lifecycle state carried by the handle.
        state: &'static str,
    },
    /// A handle presented to teardown carries no verified HEAD evidence,
    /// so no exact expectation could be established.
    TeardownHeadEvidenceMissing,
    /// The registered worktree checkout path could not be resolved to its
    /// canonical (realpath) location before teardown verification, so the
    /// worktree's identity could not be proven.
    WorktreeUnresolvable {
        /// The rejected path and the underlying resolution failure.
        detail: String,
    },
    /// The supplied worktree is not a registered worktree of the supplied
    /// repository, or its registered identity disagrees with the handle.
    TeardownWorktreeNotRegistered {
        /// What was expected and what Git actually reports as registered.
        detail: String,
    },
    /// The worktree's checked-out branch differs from the handle's branch.
    TeardownBranchMismatch {
        /// The branch recorded in the workspace handle.
        expected_branch: String,
        /// The branch (or detached/no-branch marker) Git reports for the
        /// registered worktree.
        observed: String,
    },
    /// The worktree's current HEAD could not be read during teardown.
    TeardownHeadUnavailable {
        /// Exit status/stderr of the failed read, or a description of why
        /// the reported value is not an acceptable commit SHA.
        detail: String,
    },
    /// The worktree's current HEAD differs from the handle's verified head
    /// evidence. The worktree has changed since provisioning; it is left
    /// fully intact for later recovery rather than removed under stale
    /// evidence.
    TeardownHeadMismatch {
        /// The verified head evidence carried by the handle.
        expected: String,
        /// The HEAD actually observed in the worktree.
        observed: String,
    },
    /// The worktree was not clean per `git status --porcelain` immediately
    /// before removal. Tracked modifications, staged modifications, and
    /// untracked files all count. Nothing is removed in this case.
    TeardownWorktreeDirty {
        /// The non-empty porcelain status output.
        status: String,
    },
    /// After the removal command exited successfully, the worktree was
    /// still registered with the repository — removal did not take effect
    /// verifiably, so success is refused.
    TeardownRegistrationVerificationFailed {
        /// What remained registered after removal.
        detail: String,
    },
    /// The retained task branch disappeared during teardown. Branch
    /// retention is a required property; its absence fails closed instead
    /// of being recreated (which would hide evidence corruption).
    TeardownRetainedBranchMissing {
        /// The branch that must remain.
        branch: String,
    },
    /// The retained task branch no longer resolves to the exact commit
    /// observed and verified immediately before teardown.
    TeardownRetainedBranchShaMismatch {
        /// The retained branch whose target was re-verified.
        branch: String,
        /// The exact commit observed immediately before teardown.
        expected: String,
        /// What the branch resolves to now.
        observed: String,
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
            WorkspaceError::GitExecutableUnavailable { detail } => write!(
                f,
                "no executable git binary could be resolved at an absolute canonical path: {detail}"
            ),
            WorkspaceError::RepositoryRootUnresolvable { detail } => write!(
                f,
                "repository root could not be resolved to a canonical path: {detail}"
            ),
            WorkspaceError::CreatedWorktreeUnresolvable { detail } => write!(
                f,
                "created worktree could not be resolved to a canonical path for verification: {detail}"
            ),
            WorkspaceError::SubprocessCwdUnresolvable { operation, detail } => write!(
                f,
                "working directory for {operation} could not be canonicalized before execution: {detail}"
            ),
            WorkspaceError::TeardownUnsupportedState { state } => write!(
                f,
                "teardown supports only handles in the PROVISIONED state; the supplied handle is in the {state} state"
            ),
            WorkspaceError::TeardownHeadEvidenceMissing => write!(
                f,
                "the supplied workspace handle carries no verified HEAD evidence, so teardown could not establish an exact expectation"
            ),
            WorkspaceError::WorktreeUnresolvable { detail } => write!(
                f,
                "registered worktree could not be resolved to a canonical path for verification: {detail}"
            ),
            WorkspaceError::TeardownWorktreeNotRegistered { detail } => write!(
                f,
                "supplied worktree is not registered with the supplied repository: {detail}"
            ),
            WorkspaceError::TeardownBranchMismatch {
                expected_branch,
                observed,
            } => write!(
                f,
                "worktree checkout is on {observed:?} instead of the handle's branch {expected_branch:?}; refusing to remove it"
            ),
            WorkspaceError::TeardownHeadUnavailable { detail } => write!(
                f,
                "worktree HEAD could not be read during teardown: {detail}"
            ),
            WorkspaceError::TeardownHeadMismatch { expected, observed } => write!(
                f,
                "worktree HEAD {observed} does not equal the handle's verified head evidence {expected}; the worktree has changed since provisioning and is left intact"
            ),
            WorkspaceError::TeardownWorktreeDirty { status } => write!(
                f,
                "worktree is not clean according to git status --porcelain immediately before removal: {status}"
            ),
            WorkspaceError::TeardownRegistrationVerificationFailed { detail } => write!(
                f,
                "worktree removal did not verifiably take effect: {detail}"
            ),
            WorkspaceError::TeardownRetainedBranchMissing { branch } => write!(
                f,
                "retained branch {branch:?} disappeared during teardown; failing closed instead of recreating it"
            ),
            WorkspaceError::TeardownRetainedBranchShaMismatch {
                branch,
                expected,
                observed,
            } => write!(
                f,
                "retained branch {branch:?} resolves to {observed} instead of the verified commit {expected}"
            ),
        }
    }
}

impl std::error::Error for WorkspaceError {}
