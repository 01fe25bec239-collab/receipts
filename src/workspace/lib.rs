//! Workspace-Execution foundation for the Receipts orchestration core.
//!
//! This crate implements the first bounded slice of `M-WORK-1`: local Git
//! branch/worktree provisioning for task workspaces and the typed
//! [`WorkspaceHandle`] that proves a successful provisioning.
//!
//! The successful provisioning flow is strictly ordered:
//!
//! 1. validate the provisioning inputs (exact base SHA syntax, branch name,
//!    absolute unused worktree path);
//! 2. verify the requested base identifies an existing Git commit;
//! 3. create the requested task branch from that exact base commit;
//! 4. create the requested Git worktree at the requested path;
//! 5. read the resulting worktree HEAD and require it to equal the
//!    requested base SHA exactly;
//! 6. verify the newly provisioned worktree is clean via `git status
//!    --porcelain`;
//! 7. return a [`WorkspaceHandle`] in the `PROVISIONED` state.
//!
//! Every failure mode — invalid SHA, missing commit, failed Git command,
//! unexpected HEAD, dirty resulting worktree — surfaces as an explicit
//! [`WorkspaceError`]. A failed or ambiguous operation is never converted
//! into a successful handle. Failed provisioning leaves any partially
//! created Git artifacts in place: teardown, orphan detection, and crash
//! recovery are later milestones outside this slice.
//!
//! Isolation semantics are frozen as [`WorkspaceIsolation`]: a Git worktree
//! provides workspace isolation only. It is NOT a security sandbox.
//!
//! All Git execution uses explicit argv through [`std::process::Command`].
//! No shell command string exists anywhere in this crate; untrusted values
//! travel only as individual process arguments or path values. No remote
//! operation is implemented: no credential handling, no fetch, no push,
//! no remote publication, no force-push.

pub mod error;
pub mod git;
pub mod handle;
pub mod provision;

#[cfg(test)]
mod provision_tests;

pub use error::WorkspaceError;
pub use handle::{CommitSha, WorkspaceHandle, WorkspaceIsolation, WorkspaceState};
pub use provision::{WorkspaceProvisionRequest, validate_branch_name};
