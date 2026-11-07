//! `WorktreeCreate` semantics (WE-W1-5, P-12). Host hook installation and
//! translation are Host-owned plumbing outside this crate.
//!
//! * **Task context:** a fresh request runs hardened provisioning, which
//!   records the exact task identity in protected Git administrative metadata
//!   (see [`crate::worktree_binding`]), and returns the new `PROVISIONED`
//!   handle. A repeated request attaches idempotently only to the exact
//!   verified registered worktree: the branch must be checked out there and
//!   nowhere else, the recorded repository/path/branch/original base/workspace
//!   /task binding must be present, authenticated and equal to the request,
//!   and the base must still be an ancestor of (or equal to) the branch. A
//!   different (even ancestral) base, task or workspace, a missing, corrupt
//!   or unauthenticated binding, or a worktree that was created without task
//!   context refuses; no binding is adopted or repaired from ancestry. Edited
//!   work is never reset; the response reports the actual current HEAD and
//!   cleanliness and carries no new handle.
//! * **No task context:** ordinary Git worktree semantics only (a new branch
//!   at the base, or idempotent attach to the identical worktree). No task
//!   admission, handle or controller privilege is granted, and a task-bound
//!   worktree is refused rather than attached as a plain one.
//!
//! No fetch or other remote operation ever happens; every Git call uses the
//! hardened boundary. The returned path is the canonical absolute path.
//! Limit: plain (no-task) attach has no recorded base, so for plain worktrees
//! the original base is still verified by ancestry only.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::error::WorkspaceError;
use crate::git;
use crate::handle::{CommitSha, WorkspaceHandle};
use crate::provision::{
    WorkspaceProvisionRequest, branch_target, canonical_target, preflight, validate_branch_name,
    validate_identifier,
};
use crate::teardown::{list_registered_worktrees, record_matches};
use crate::worktree_binding::{self, TaskBinding};

/// Optional orchestrator task context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCreateTask {
    pub workspace_id: String,
    pub task_id: String,
}

/// One validated create request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCreateRequest {
    repository_root: PathBuf,
    worktree_path: PathBuf,
    branch: String,
    base_sha: CommitSha,
    task: Option<WorktreeCreateTask>,
}

/// How the response was produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeCreateMode {
    /// Fresh hardened task provisioning; the response carries the handle.
    TaskProvisioned,
    /// Idempotent attach to the verified existing task worktree.
    TaskAttached,
    /// Fresh ordinary no-task Git worktree.
    PlainCreated,
    /// Idempotent attach to the identical ordinary worktree.
    PlainAttached,
}

/// Actual observed result; nothing here is fabricated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeCreateResponse {
    path: PathBuf,
    mode: WorktreeCreateMode,
    head_sha: CommitSha,
    clean: bool,
    handle: Option<WorkspaceHandle>,
}

impl WorktreeCreateResponse {
    /// Canonical absolute worktree path for the host.
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn mode(&self) -> WorktreeCreateMode {
        self.mode
    }
    /// The actual current HEAD.
    pub fn head_sha(&self) -> &CommitSha {
        &self.head_sha
    }
    /// Whether `git status --porcelain` was empty when observed.
    pub fn clean(&self) -> bool {
        self.clean
    }
    /// Present only for [`WorktreeCreateMode::TaskProvisioned`].
    pub fn handle(&self) -> Option<&WorkspaceHandle> {
        self.handle.as_ref()
    }
}

impl WorktreeCreateRequest {
    pub fn new(
        repository_root: impl Into<PathBuf>,
        worktree_path: impl Into<PathBuf>,
        branch: &str,
        base_sha: &str,
        task: Option<WorktreeCreateTask>,
    ) -> Result<Self, WorkspaceError> {
        let worktree_path = worktree_path.into();
        if !worktree_path.is_absolute() {
            return Err(WorkspaceError::InvalidWorktreePath {
                detail: "worktree path must be absolute".into(),
            });
        }
        validate_branch_name(branch).map_err(|reason| WorkspaceError::BranchNameInvalid {
            branch: branch.to_string(),
            reason,
        })?;
        let base_sha = CommitSha::parse(base_sha)?;
        if let Some(task) = &task {
            validate_identifier("workspace_id", &task.workspace_id)
                .map_err(|detail| WorkspaceError::InvalidWorkspaceId { detail })?;
            validate_identifier("task_id", &task.task_id)
                .map_err(|detail| WorkspaceError::InvalidTaskId { detail })?;
        }
        Ok(Self {
            repository_root: repository_root.into(),
            worktree_path,
            branch: branch.to_string(),
            base_sha,
            task,
        })
    }

    /// Creates or idempotently attaches. Never fetches, resets or forces.
    pub fn create(&self) -> Result<WorktreeCreateResponse, WorkspaceError> {
        let root = std::fs::canonicalize(&self.repository_root).map_err(|error| {
            WorkspaceError::RepositoryRootUnresolvable {
                detail: format!("{:?}: {error}", self.repository_root.display()),
            }
        })?;
        let target = canonical_target(&self.worktree_path)?;
        let registered = list_registered_worktrees(&root, "list registered worktrees", |detail| {
            WorkspaceError::WorktreeCreateConflict { detail }
        })?;
        let expected_ref = format!("refs/heads/{}", self.branch);
        let at_target: Vec<_> = registered
            .iter()
            .filter(|record| record_matches(record, &target))
            .collect();
        let with_branch = registered
            .iter()
            .filter(|record| record.branch_ref.as_deref() == Some(expected_ref.as_bytes()))
            .count();
        match at_target.as_slice() {
            [] => self.create_fresh(&root, &target),
            [record]
                if record.branch_ref.as_deref() == Some(expected_ref.as_bytes())
                    && with_branch == 1 =>
            {
                self.attach(&root, &target)
            }
            _ => Err(WorkspaceError::WorktreeCreateConflict {
                detail: "the target is registered with a different or ambiguous branch".into(),
            }),
        }
    }

    fn create_fresh(
        &self,
        root: &Path,
        target: &Path,
    ) -> Result<WorktreeCreateResponse, WorkspaceError> {
        if let Some(task) = &self.task {
            let handle = WorkspaceProvisionRequest::new(
                root,
                &task.workspace_id,
                Some(&task.task_id),
                &self.branch,
                target,
                self.base_sha.as_str(),
            )?
            .provision()?;
            return Ok(WorktreeCreateResponse {
                path: handle.worktree_path().to_path_buf(),
                mode: WorktreeCreateMode::TaskProvisioned,
                head_sha: handle.base_sha().clone(),
                clean: true,
                handle: Some(handle),
            });
        }
        // Ordinary Git semantics: same refusals as provisioning, no handle.
        preflight(root, target, &self.branch, &self.base_sha)?;
        git::capture(
            root,
            "add plain worktree",
            &[
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("-b"),
                OsStr::new(self.branch.as_str()),
                OsStr::new(target.as_os_str()),
                OsStr::new(self.base_sha.as_str()),
            ],
        )?
        .require_success("add plain worktree")?;
        self.observe(target, WorktreeCreateMode::PlainCreated)
    }

    fn attach(&self, root: &Path, target: &Path) -> Result<WorktreeCreateResponse, WorkspaceError> {
        git::refuse_untrusted_repository_config(root, "inspect repository configuration")?;
        if branch_target(root, &self.branch)?.is_none() {
            return Err(WorkspaceError::WorktreeCreateConflict {
                detail: "the registered branch no longer exists".into(),
            });
        }
        let ancestry = git::capture(
            root,
            "verify original base",
            &[
                OsStr::new("merge-base"),
                OsStr::new("--is-ancestor"),
                OsStr::new(self.base_sha.as_str()),
                OsStr::new(&format!("refs/heads/{}", self.branch)),
            ],
        )?;
        if !ancestry.success {
            return Err(WorkspaceError::WorktreeCreateConflict {
                detail: "the requested base is not an ancestor of the existing branch".into(),
            });
        }
        // Exact identity, never ancestry alone: a task request needs the
        // recorded repository/path/branch/ORIGINAL base/workspace/task to
        // match exactly; a no-task request may not attach a task-bound
        // worktree. Nothing is created, repaired or promoted here.
        let conflict = |detail: String| WorkspaceError::WorktreeCreateConflict { detail };
        let mode = match &self.task {
            Some(task) => {
                worktree_binding::verify(
                    root,
                    &TaskBinding {
                        repository: root.to_path_buf(),
                        worktree: target.to_path_buf(),
                        branch: self.branch.clone(),
                        base_sha: self.base_sha.clone(),
                        workspace_id: task.workspace_id.clone(),
                        task_id: task.task_id.clone(),
                    },
                )
                .map_err(conflict)?;
                WorktreeCreateMode::TaskAttached
            }
            None => {
                if worktree_binding::is_task_bound(root, target).map_err(conflict)? {
                    return Err(conflict(
                        "the worktree is bound to a task; a no-task request cannot attach it"
                            .into(),
                    ));
                }
                WorktreeCreateMode::PlainAttached
            }
        };
        self.observe(target, mode)
    }

    fn observe(
        &self,
        target: &Path,
        mode: WorktreeCreateMode,
    ) -> Result<WorktreeCreateResponse, WorkspaceError> {
        let path = std::fs::canonicalize(target).map_err(|error| {
            WorkspaceError::CreatedWorktreeUnresolvable {
                detail: format!("{:?}: {error}", target.display()),
            }
        })?;
        git::refuse_untrusted_repository_config(&path, "inspect target configuration")?;
        let head = git::capture(
            &path,
            "read worktree HEAD",
            &[OsStr::new("rev-parse"), OsStr::new("HEAD")],
        )?
        .require_success("read worktree HEAD")?;
        let head_sha = CommitSha::parse(head.stdout.trim()).map_err(|_| {
            WorkspaceError::WorktreeHeadUnavailable {
                detail: "HEAD is not an exact commit SHA".into(),
            }
        })?;
        let status = git::capture(
            &path,
            "inspect worktree status",
            &[
                OsStr::new("--no-optional-locks"),
                OsStr::new("status"),
                OsStr::new("--porcelain"),
                OsStr::new("--untracked-files=all"),
                OsStr::new("--ignore-submodules=none"),
            ],
        )?
        .require_success("inspect worktree status")?;
        Ok(WorktreeCreateResponse {
            path,
            mode,
            head_sha,
            clean: status.raw_stdout().is_empty(),
            handle: None,
        })
    }
}

#[cfg(test)]
#[path = "worktree_create_tests.rs"]
mod tests;
