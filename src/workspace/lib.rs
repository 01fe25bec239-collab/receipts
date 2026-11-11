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
//! into a successful handle. Preflight (Git version, refused filter/include
//! configuration, free branch, unregistered target) runs before creation;
//! a later failure runs owned rollback of only this attempt's resources,
//! reporting rollback-incomplete instead of claiming retry cleanliness.
//! Handles store canonical worktree paths and move only through the checked
//! six-state lifecycle in [`lifecycle`].
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

pub mod attempt_finalization;
pub mod checkpoint_capture_core;
pub mod checkpoint_evidence_capture;
pub mod checkpoint_evidence_vocabulary;
pub mod checkpoint_executed_check;
pub mod checkpoint_json;
pub mod checkpoint_reference;
pub mod checkpoint_vocabulary;
pub mod checkpoint_wire;
pub mod date_time;
pub mod error;
pub mod git;
pub mod handle;
pub mod handle_wire;
pub mod lifecycle;
pub mod plain_worktree;
pub mod protected;
mod protected_custody;
#[cfg(target_os = "linux")]
mod protected_linux;
pub mod provision;
pub use plain_worktree::OwnedPlainWorktree;
pub mod recovery_preparation;
pub mod remote_publish_policy;
pub mod review_evidence;
pub mod teardown;
mod worktree_binding;
pub mod worktree_create;
pub mod write_path_pattern;
pub mod write_scope;
pub mod write_scope_case;

/// Argv-only local process-runner foundation owned by the execution layer.
///
/// Wired through a path-qualified module so this package root stays
/// `src/workspace/lib.rs` (Cargo files are immutable) while ownership of
/// all execution source and tests remains under `src/execution/**`.
#[path = "../execution/lib.rs"]
pub mod execution;

#[cfg(test)]
mod checkpoint_capture_core_tests;
#[cfg(test)]
mod checkpoint_evidence_vocabulary_tests;
#[cfg(test)]
mod checkpoint_executed_check_tests;
#[cfg(test)]
mod checkpoint_json_tests;
#[cfg(test)]
mod checkpoint_reference_tests;
#[cfg(test)]
mod checkpoint_vocabulary_tests;
#[cfg(test)]
mod date_time_tests;
#[cfg(test)]
mod git_tests;
#[cfg(test)]
mod handle_wire_tests;
#[cfg(all(test, target_os = "linux"))]
mod protected_linux_tests;
#[cfg(test)]
mod provision_tests;
#[cfg(test)]
mod recovery_preparation_tests;
#[cfg(test)]
mod remote_publish_policy_tests;
#[cfg(test)]
mod review_evidence_tests;
#[cfg(test)]
mod teardown_tests;
#[cfg(test)]
mod test_support;

pub use checkpoint_capture_core::{
    WorkspaceCheckpointCaptureCore, WorkspaceCheckpointCaptureCoreError,
    WorkspaceCheckpointTemporalCaptureCore,
};
pub use checkpoint_evidence_capture::{
    WorkspaceCheckpointEvidenceCaptureError, WorkspaceCheckpointEvidenceCaptureRequest,
    WorkspaceCheckpointGitObservation, WorkspaceCheckpointGitStream,
    capture_workspace_checkpoint_evidence, capture_workspace_checkpoint_temporal_evidence,
};
pub use checkpoint_evidence_vocabulary::{
    WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckResult,
    WorkspaceCheckpointRefType,
};
pub use checkpoint_executed_check::{
    WorkspaceCheckpointExecutedCheckCore, WorkspaceCheckpointExecutedCheckCoreError,
};
pub use checkpoint_json::{WorkspaceCheckpointJsonExportError, workspace_checkpoint_to_json};
pub use checkpoint_reference::{WorkspaceCheckpointRef, WorkspaceCheckpointRefError};
pub use checkpoint_vocabulary::{
    WorkspaceCheckpointCrashClassification, WorkspaceCheckpointKind, WorkspaceRecoveryDecision,
};
pub use checkpoint_wire::{
    WorkspaceCheckpointWire, WorkspaceCheckpointWireConversionError,
    WorkspaceCheckpointWireExitCode,
};
pub use date_time::{WorkspaceDateTimeError, WorkspaceDateTimeV1};
pub use error::WorkspaceError;
pub use handle::{CommitSha, WorkspaceHandle, WorkspaceIsolation, WorkspaceState};
pub use handle_wire::{
    WorkspaceHandleExportError, WorkspaceHandleWire, WorkspaceHandleWireIsolation,
};
pub use lifecycle::{
    WorkspaceAttempt, WorkspaceCheckpointLifecycleInput, WorkspaceCollectionFailure,
    WorkspaceLifecycleError, checkpoint_workspace, orphan_workspace, start_workspace_attempt,
};
pub use protected::{
    ControllerChannel, ControllerCredentials, ControllerOperation, ControllerRequest,
    ControllerResponse, ControllerRole, ProtectedAdmission, ProtectedAttempt,
    ProtectedLaunchConfig, ProtectedLaunchError, ProtectedLauncher,
};
pub use provision::{WorkspaceProvisionRequest, validate_branch_name};
pub use recovery_preparation::{
    WorkspaceRecoveryPreparation, WorkspaceRecoveryPreparationError,
    WorkspaceRecoveryPreparationRequest, prepare_workspace_checkpoint_recovery,
};
pub use remote_publish_policy::WorkspaceRemotePublishPolicy;
pub use review_evidence::{
    MAX_REVIEW_RANGE_COMMITS, REVIEW_BRANCH_PREFIX, ReviewCommitRangeV1, ReviewDiffEvidenceV1,
    ReviewWorkspace, ReviewWorkspaceIntegrityV1, ReviewWorkspaceRequest, TemporaryMergeV1,
    provision_review_workspace, provision_temporary_merge, remove_review_workspace,
    review_commit_range, review_diff, verify_review_workspace,
};
pub use teardown::WorkspaceTeardownRequest;
pub use write_scope::{
    ForbiddenWriteMatch, WriteScopeGitOperation, WriteScopePatternSet, WriteScopeVerification,
    WriteScopeVerificationError, WriteScopeVerificationStatus, verify_write_scope,
};

pub use attempt_finalization::{
    WorkspaceAttemptFinalizationError, WorkspaceAttemptFinalizationEvidence,
    WorkspaceAttemptFinalizationRequest, finalize_workspace_attempt,
};

pub use worktree_create::{
    WorktreeCreateMode, WorktreeCreateRequest, WorktreeCreateResponse, WorktreeCreateTask,
};
pub use write_path_pattern::{
    WritePathIntersectionError, WritePathPattern, WritePathSet, intersects, validate_write_path,
};
pub use write_scope_case::{
    CaseAliasCollision, FilesystemCaseSensitivity, WriteScopeCaseAliasCheck, probe_case_sensitivity,
};
