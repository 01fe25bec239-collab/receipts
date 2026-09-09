//! Current-target, read-only recovery preparation. No recovery is executed.

use std::path::{Component, Path, PathBuf};

use crate::checkpoint_evidence_capture::{observe, parse_head, parse_root};
use crate::{
    CommitSha, WorkspaceCheckpointCaptureCore, WorkspaceCheckpointEvidenceCaptureError,
    WorkspaceCheckpointEvidenceCaptureRequest, WorkspaceCheckpointGitObservation as Observation,
    WorkspaceCheckpointKind, WorkspaceHandle, WorkspaceRecoveryDecision,
    capture_workspace_checkpoint_evidence,
};

/// Caller-selected recovery context. The handle is the only source of a target
/// path. `last_accepted_sha` is required exclusively for ResetToLastAccepted;
/// it is never inferred from the handle, checkpoint, or higher-level policy.
#[derive(Debug)]
pub struct WorkspaceRecoveryPreparationRequest<'a> {
    pub handle: &'a WorkspaceHandle,
    pub checkpoint: &'a WorkspaceCheckpointCaptureCore,
    pub decision: WorkspaceRecoveryDecision,
    pub pre_recovery_checkpoint_id: String,
    pub attempt_id: Option<String>,
    pub last_accepted_sha: Option<&'a str>,
}

/// Immutable evidence that bounded, read-only CURRENT-target checks succeeded.
///
/// Validates the current workspace/Git target against identities retained by
/// the handle and selected checkpoint. Does NOT prove filesystem or repository
/// instance continuity from provisioning time and grants NO destructive
/// mutation authority. Identically reconstructed targets may pass.
///
/// Partial work remains unaudited; independent review is still required.
/// Neither process quiescence nor physical checkpoint materializability is
/// established. The caller must exclude concurrent writers for a consistent
/// snapshot: these observations are not a transaction or a lasting guarantee.
///
/// Future blocker WORKSPACE-DESTRUCTIVE-RECOVERY-TARGET-CONTINUITY-001:
/// before destructive recovery, BUILD-A1/A0 must decide whether current
/// evidence suffices or provisioning-time physical identity must be retained.
#[derive(Debug)]
pub struct WorkspaceRecoveryPreparation<'a> {
    handle: &'a WorkspaceHandle,
    checkpoint: &'a WorkspaceCheckpointCaptureCore,
    canonical_target: PathBuf,
    decision: WorkspaceRecoveryDecision,
    current_head: CommitSha,
    pre_recovery_capture: WorkspaceCheckpointCaptureCore,
    last_accepted_sha: Option<CommitSha>,
}

impl<'a> WorkspaceRecoveryPreparation<'a> {
    pub fn handle(&self) -> &'a WorkspaceHandle {
        self.handle
    }
    pub fn checkpoint(&self) -> &'a WorkspaceCheckpointCaptureCore {
        self.checkpoint
    }
    pub fn canonical_target(&self) -> &Path {
        &self.canonical_target
    }
    pub fn decision(&self) -> WorkspaceRecoveryDecision {
        self.decision
    }
    pub fn current_head(&self) -> &CommitSha {
        &self.current_head
    }
    pub fn pre_recovery_capture(&self) -> &WorkspaceCheckpointCaptureCore {
        &self.pre_recovery_capture
    }
    pub fn last_accepted_sha(&self) -> Option<&CommitSha> {
        self.last_accepted_sha.as_ref()
    }
}

/// Failures return no partial preparation. Git diagnostics retain the existing
/// capture engine's physical bounds; caller input is not echoed into errors.
#[derive(Debug)]
pub enum WorkspaceRecoveryPreparationError {
    WorkspaceMismatch,
    TaskMismatch,
    MissingResetTarget,
    ExtraneousResetTarget,
    InvalidResetTarget,
    InvalidTargetPath,
    TargetIo(std::io::Error),
    WorktreeRootMismatch,
    BranchMismatch,
    HeadChanged,
    CommitNotCommit,
    Evidence(WorkspaceCheckpointEvidenceCaptureError),
}

impl std::fmt::Display for WorkspaceRecoveryPreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Self::Evidence(source) = self {
            write!(f, "recovery preparation: {source}")
        } else {
            write!(f, "recovery preparation: {self:?}")
        }
    }
}
impl std::error::Error for WorkspaceRecoveryPreparationError {}
impl From<WorkspaceCheckpointEvidenceCaptureError> for WorkspaceRecoveryPreparationError {
    fn from(source: WorkspaceCheckpointEvidenceCaptureError) -> Self {
        Self::Evidence(source)
    }
}
use WorkspaceRecoveryPreparationError as E;

/// Validates explicit recovery inputs, observes the current target, then uses
/// the integrated capture engine for a fresh RecoveryCapture. Carried paths,
/// references and executed checks are inert: they are neither opened nor run.
/// No recovery choice is selected automatically and no handle state changes.
pub fn prepare_workspace_checkpoint_recovery(
    request: WorkspaceRecoveryPreparationRequest<'_>,
) -> Result<WorkspaceRecoveryPreparation<'_>, E> {
    let WorkspaceRecoveryPreparationRequest {
        handle,
        checkpoint,
        decision,
        pre_recovery_checkpoint_id,
        attempt_id,
        last_accepted_sha,
    } = request;
    if checkpoint.workspace_id() != handle.workspace_id() {
        return Err(E::WorkspaceMismatch);
    }
    if checkpoint.task_id() != handle.task_id() {
        return Err(E::TaskMismatch);
    }
    let last_accepted_sha = match (decision, last_accepted_sha) {
        (WorkspaceRecoveryDecision::ResetToLastAccepted, Some(sha)) => {
            Some(CommitSha::parse(sha).map_err(|_| E::InvalidResetTarget)?)
        }
        (WorkspaceRecoveryDecision::ResetToLastAccepted, None) => {
            return Err(E::MissingResetTarget);
        }
        (_, Some(_)) => return Err(E::ExtraneousResetTarget),
        (_, None) => None,
    };
    let root = validate_target(handle)?;
    let current_head = current_head(&root)?;
    verify_commit(&root, checkpoint.head_sha())?;
    if let Some(base) = checkpoint.base_sha() {
        verify_commit(&root, base)?;
    }
    if let Some(target) = &last_accepted_sha {
        verify_commit(&root, target)?;
    }
    let pre_recovery_capture =
        capture_workspace_checkpoint_evidence(WorkspaceCheckpointEvidenceCaptureRequest {
            directory: &root,
            checkpoint_id: pre_recovery_checkpoint_id,
            workspace_id: handle.workspace_id().into(),
            task_id: handle.task_id().map(str::to_owned),
            attempt_id,
            kind: WorkspaceCheckpointKind::RecoveryCapture,
            base_sha: None,
            dirty_diff_ref: None,
            executed_checks: Vec::new(),
        })?;
    // Detect observable target/branch/HEAD changes across capture, without
    // pretending to exclude arbitrary concurrent writers or prove continuity.
    if validate_target(handle)? != root {
        return Err(E::WorktreeRootMismatch);
    }
    if pre_recovery_capture.head_sha() != &current_head
        || self::current_head(&root)? != current_head
    {
        return Err(E::HeadChanged);
    }
    Ok(WorkspaceRecoveryPreparation {
        handle,
        checkpoint,
        canonical_target: root,
        decision,
        current_head,
        pre_recovery_capture,
        last_accepted_sha,
    })
}

fn validate_target(handle: &WorkspaceHandle) -> Result<PathBuf, E> {
    let target = handle.worktree_path();
    if !target.is_absolute()
        || target
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(E::InvalidTargetPath);
    }
    let canonical = std::fs::canonicalize(target).map_err(E::TargetIo)?;
    // Handle paths are retained verbatim by provisioning. Reject aliases in
    // this preparation boundary rather than allowing a symlink to redirect it.
    if canonical != target {
        return Err(E::InvalidTargetPath);
    }
    let observed = parse_root(observe(
        &canonical,
        Observation::WorktreeRoot,
        &["rev-parse", "--show-toplevel"],
    )?)?;
    if std::fs::canonicalize(observed).map_err(E::TargetIo)? != canonical {
        return Err(E::WorktreeRootMismatch);
    }
    let branch = observe(
        &canonical,
        Observation::RecoveryBranch,
        &["symbolic-ref", "--quiet", "HEAD"],
    )?
    .require_complete(Observation::RecoveryBranch)?;
    let branch = std::str::from_utf8(&branch).map_err(|_| E::BranchMismatch)?;
    if branch.strip_suffix('\n') != Some(format!("refs/heads/{}", handle.branch()).as_str()) {
        return Err(E::BranchMismatch);
    }
    Ok(canonical)
}

fn current_head(root: &Path) -> Result<CommitSha, E> {
    let head = parse_head(observe(
        root,
        Observation::Head,
        &["rev-parse", "--verify", "HEAD"],
    )?)?;
    verify_commit(root, &head)?;
    Ok(head)
}

fn verify_commit(root: &Path, sha: &CommitSha) -> Result<(), E> {
    let output = observe(
        root,
        Observation::RecoveryCommit,
        &["cat-file", "-t", sha.as_str()],
    )?
    .require_complete(Observation::RecoveryCommit)?;
    if output != b"commit\n" {
        return Err(E::CommitNotCommit);
    }
    Ok(())
}
