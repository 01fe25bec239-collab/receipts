//! Workspace-Execution foundation for the Receipts orchestration core.
//!
//! This crate implements the first bounded slices of `M-WORK-1`: local Git
//! branch/worktree provisioning for task workspaces, the typed
//! [`WorkspaceHandle`] that proves a successful provisioning, and the
//! narrow verified teardown of a provisioned worktree.
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
//! created Git artifacts in place: orphan detection and crash recovery are
//! later milestones outside this slice.
//!
//! The teardown slice ([`WorkspaceTeardownRequest`]) performs the frozen
//! cleanup contract for an accepted workspace: after full identity,
//! evidence, and cleanliness verification the registered worktree checkout
//! is removed without any force flag while the task branch is retained —
//! with every commit it carries — until workstream integration consumes
//! it. Whether a workspace should be torn down is a caller-side policy
//! decision; this crate only executes the verified local removal.
//!
//! Isolation semantics are frozen as [`WorkspaceIsolation`]: a Git worktree
//! provides workspace isolation only. It is NOT a security sandbox.
//!
//! All Git execution uses explicit argv through [`std::process::Command`]
//! behind the hardened boundary in the `git` module: the child program is
//! one absolute, canonically resolved `git` executable (never an
//! unqualified `"git"` PATH lookup), every subprocess working directory is
//! a canonicalized (realpath) location, and children inherit nothing from
//! the parent environment except a fixed, documented allowlist of two
//! non-sensitive entries. No shell command string exists anywhere in this
//! crate; untrusted values travel only as individual process arguments or
//! path values. No remote operation is implemented: no credential handling,
//! no fetch, no push, no remote publication, no force-push.

pub mod error;
pub mod git;
pub mod handle;
pub mod provision;
pub mod teardown;

#[cfg(test)]
mod provision_tests;
#[cfg(test)]
mod teardown_tests;
#[cfg(test)]
mod test_support;

pub use error::WorkspaceError;
pub use handle::{CommitSha, WorkspaceHandle, WorkspaceIsolation, WorkspaceState};
pub use provision::{WorkspaceProvisionRequest, validate_branch_name};
pub use teardown::WorkspaceTeardownRequest;
