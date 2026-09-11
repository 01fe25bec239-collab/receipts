//! Read-only mechanical evidence for an exact final review anchor.
//! This grants no review verdict, acceptance, integration or mutation authority.

use std::path::{Component, Path, PathBuf};

use crate::checkpoint_evidence_capture::{observe, observe_state, parse_head, parse_root};
use crate::execution::{LiveProcessOutcome, LiveProcessTerminalCause};
use crate::{
    CommitSha, WorkspaceCheckpointCaptureCore, WorkspaceCheckpointEvidenceCaptureError,
    WorkspaceCheckpointGitObservation as Observation, WorkspaceHandle, WriteScopeVerification,
    WriteScopeVerificationError, WriteScopeVerificationStatus, verify_write_scope,
};

/// Caller-authoritative inert context. Start must equal the handle's provisioning
/// base. The checkpoint binds identities, without imposing temporal SHA semantics.
/// Execution has no workspace/attempt IDs: the caller owns that association.
#[derive(Debug)]
pub struct WorkspaceAttemptFinalizationRequest<'a> {
    pub handle: &'a WorkspaceHandle,
    pub workspace_id: &'a str,
    pub task_id: Option<&'a str>,
    pub attempt_id: Option<&'a str>,
    pub expected_worktree: &'a Path,
    pub expected_branch: &'a str,
    pub start_sha: &'a str,
    pub final_sha: &'a str,
    pub allowed_write_paths: &'a [String],
    pub forbidden_write_paths: &'a [String],
    pub checkpoint: &'a WorkspaceCheckpointCaptureCore,
    pub terminal: &'a LiveProcessOutcome,
}

/// Immutable, bounded mechanical proof only. Git-visible cleanliness excludes
/// ignored files. This does not establish provisioning-time physical continuity.
#[derive(Debug)]
pub struct WorkspaceAttemptFinalizationEvidence {
    handle: WorkspaceHandle,
    checkpoint: WorkspaceCheckpointCaptureCore,
    canonical_root: PathBuf,
    write_scope: WriteScopeVerification,
    modified_files: Vec<String>,
    untracked_files: Vec<String>,
    terminal_cause: LiveProcessTerminalCause,
    exit_code: Option<i32>,
}

impl WorkspaceAttemptFinalizationEvidence {
    pub fn handle(&self) -> &WorkspaceHandle {
        &self.handle
    }
    pub fn workspace_id(&self) -> &str {
        self.handle.workspace_id()
    }
    pub fn task_id(&self) -> Option<&str> {
        self.handle.task_id()
    }
    pub fn attempt_id(&self) -> Option<&str> {
        self.checkpoint.attempt_id()
    }
    pub fn checkpoint(&self) -> &WorkspaceCheckpointCaptureCore {
        &self.checkpoint
    }
    pub fn canonical_root(&self) -> &Path {
        &self.canonical_root
    }
    pub fn branch(&self) -> &str {
        self.handle.branch()
    }
    pub fn start_sha(&self) -> &CommitSha {
        self.write_scope.baseline_sha()
    }
    pub fn final_sha(&self) -> &CommitSha {
        self.write_scope.candidate_sha()
    }
    pub fn write_scope(&self) -> &WriteScopeVerification {
        &self.write_scope
    }
    pub fn changed_paths(&self) -> &[String] {
        self.write_scope.changed_paths()
    }
    pub fn modified_files(&self) -> &[String] {
        &self.modified_files
    }
    pub fn untracked_files(&self) -> &[String] {
        &self.untracked_files
    }
    pub fn terminal_cause(&self) -> LiveProcessTerminalCause {
        self.terminal_cause
    }
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
}

#[derive(Debug)]
pub enum WorkspaceAttemptFinalizationError {
    WorkspaceMismatch,
    TaskMismatch,
    CheckpointWorkspaceMismatch,
    CheckpointTaskMismatch,
    CheckpointAttemptMismatch,
    WorktreeRootMismatch,
    TargetIo(std::io::Error),
    BranchMismatch,
    DetachedHead,
    InvalidStartSha,
    InvalidFinalSha,
    StartAuthorityMismatch,
    CurrentHeadMismatch,
    DirtyTracked(Vec<String>),
    DirtyUntracked(Vec<String>),
    RepositoryChangedDuringCapture(Box<Self>),
    TerminalUnacceptable {
        cause: LiveProcessTerminalCause,
        exit_code: Option<i32>,
    },
    Evidence(WorkspaceCheckpointEvidenceCaptureError),
    WriteScopeTechnical(WriteScopeVerificationError),
    WriteScopeFailed(Box<WriteScopeVerification>),
}

use WorkspaceAttemptFinalizationError as E;
impl std::fmt::Display for E {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Evidence(source) => write!(f, "attempt finalization: {source}"),
            Self::WriteScopeTechnical(source) => write!(f, "attempt finalization: {source}"),
            Self::RepositoryChangedDuringCapture(source) => {
                write!(f, "repository changed during finalization: {source}")
            }
            _ => write!(f, "attempt finalization: {self:?}"),
        }
    }
}
impl std::error::Error for E {}
impl From<WorkspaceCheckpointEvidenceCaptureError> for E {
    fn from(source: WorkspaceCheckpointEvidenceCaptureError) -> Self {
        Self::Evidence(source)
    }
}

/// Establishes fresh exact local commit, target, cleanliness and write-scope
/// evidence. Never commits or repairs. Pre/post observations detect observable
/// changes; this is not a Git transaction and cannot defeat an adversarial writer.
/// Callers must exclude concurrent writers for stronger consistency guarantees.
pub fn finalize_workspace_attempt(
    request: WorkspaceAttemptFinalizationRequest<'_>,
) -> Result<WorkspaceAttemptFinalizationEvidence, E> {
    let r = &request;
    let start = CommitSha::parse(r.start_sha).map_err(|_| E::InvalidStartSha)?;
    let final_sha = CommitSha::parse(r.final_sha).map_err(|_| E::InvalidFinalSha)?;
    if r.workspace_id != r.handle.workspace_id() {
        return Err(E::WorkspaceMismatch);
    }
    if r.task_id != r.handle.task_id() {
        return Err(E::TaskMismatch);
    }
    if &start != r.handle.base_sha() {
        return Err(E::StartAuthorityMismatch);
    }
    if r.expected_worktree != r.handle.worktree_path() {
        return Err(E::WorktreeRootMismatch);
    }
    if r.expected_branch != r.handle.branch() {
        return Err(E::BranchMismatch);
    }
    if r.checkpoint.workspace_id() != r.workspace_id {
        return Err(E::CheckpointWorkspaceMismatch);
    }
    if r.checkpoint.task_id() != r.task_id {
        return Err(E::CheckpointTaskMismatch);
    }
    if r.checkpoint.attempt_id() != r.attempt_id {
        return Err(E::CheckpointAttemptMismatch);
    }
    if r.terminal.terminal_cause() != LiveProcessTerminalCause::Completed || !r.terminal.success() {
        return Err(E::TerminalUnacceptable {
            cause: r.terminal.terminal_cause(),
            exit_code: r.terminal.exit_code(),
        });
    }
    let (root, head) = observe_identity(r.handle)?;
    clean_state(&root)?;
    let write_scope = verify_write_scope(
        &root,
        r.start_sha,
        r.final_sha,
        r.allowed_write_paths,
        r.forbidden_write_paths,
    )
    .map_err(E::WriteScopeTechnical)?;
    // The verifier freshly proves both exact local commit objects before diffing.
    if head != final_sha {
        return Err(E::CurrentHeadMismatch);
    }
    if write_scope.verification() != WriteScopeVerificationStatus::Pass {
        return Err(E::WriteScopeFailed(Box::new(write_scope)));
    }
    #[cfg(test)]
    run_capture_gate();
    let post = || -> Result<_, E> {
        let (post_root, post_head) = observe_identity(r.handle)?;
        if post_root != root {
            return Err(E::WorktreeRootMismatch);
        }
        if post_head != final_sha {
            return Err(E::CurrentHeadMismatch);
        }
        let state = clean_state(&post_root)?;
        // Also bracket the final status queries with identity observations.
        if observe_identity(r.handle)? != (root.clone(), final_sha) {
            return Err(E::CurrentHeadMismatch);
        }
        Ok(state)
    };
    let (modified_files, untracked_files) =
        post().map_err(|e| E::RepositoryChangedDuringCapture(Box::new(e)))?;
    Ok(WorkspaceAttemptFinalizationEvidence {
        handle: r.handle.clone(),
        checkpoint: r.checkpoint.clone(),
        canonical_root: root,
        write_scope,
        modified_files,
        untracked_files,
        terminal_cause: r.terminal.terminal_cause(),
        exit_code: r.terminal.exit_code(),
    })
}

fn clean_state(root: &Path) -> Result<(Vec<String>, Vec<String>), E> {
    let (modified, untracked) = observe_state(root)?;
    if !modified.is_empty() {
        return Err(E::DirtyTracked(modified));
    }
    if !untracked.is_empty() {
        return Err(E::DirtyUntracked(untracked));
    }
    Ok((modified, untracked))
}

fn observe_identity(handle: &WorkspaceHandle) -> Result<(PathBuf, CommitSha), E> {
    let target = handle.worktree_path();
    if !target.is_absolute()
        || target
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(E::WorktreeRootMismatch);
    }
    let root = std::fs::canonicalize(target).map_err(E::TargetIo)?;
    if root != target {
        return Err(E::WorktreeRootMismatch);
    }
    let observed = parse_root(observe(
        &root,
        Observation::WorktreeRoot,
        &["rev-parse", "--show-toplevel"],
    )?)?;
    if std::fs::canonicalize(observed).map_err(E::TargetIo)? != root {
        return Err(E::WorktreeRootMismatch);
    }
    let branch = match observe(
        &root,
        Observation::RecoveryBranch,
        &["symbolic-ref", "--quiet", "HEAD"],
    ) {
        Err(WorkspaceCheckpointEvidenceCaptureError::GitCommandFailed {
            status: Some(1), ..
        }) => return Err(E::DetachedHead),
        other => other?,
    }
    .require_complete(Observation::RecoveryBranch)?;
    if branch != format!("refs/heads/{}\n", handle.branch()).as_bytes() {
        return Err(E::BranchMismatch);
    }
    let head = parse_head(observe(
        &root,
        Observation::Head,
        &["rev-parse", "--verify", "HEAD"],
    )?)?;
    Ok((root, head))
}

#[cfg(test)]
thread_local! {
    static CAPTURE_GATE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
fn run_capture_gate() {
    CAPTURE_GATE.with(|gate| {
        if let Some(action) = gate.borrow_mut().take() {
            action();
        }
    });
}
#[cfg(test)]
#[path = "attempt_finalization_tests.rs"]
mod tests;
