//! Local Git branch/worktree provisioning for task workspaces.
//!
//! [`WorkspaceProvisionRequest::provision`] executes the frozen flow:
//! validate inputs, resolve the repository root to its canonical (realpath)
//! location, verify the base commit exists, create the branch from that
//! exact commit, add the worktree, resolve the created worktree directory
//! canonically, require the resulting HEAD to equal the base SHA, require a
//! clean `git status --porcelain`, and only then return a
//! [`WorkspaceHandle`](crate::handle::WorkspaceHandle) in the `PROVISIONED`
//! state. All Git execution goes through the hardened argv-only boundary in
//! [`crate::git`]: absolute resolved executable, canonical working
//! directories, and an allowlisted child environment.
//!
//! No remote operation is performed at any step: no credential handling,
//! no fetch, no push, no remote publication, and no force-push exist here.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::error::WorkspaceError;
use crate::git;
use crate::handle::{CommitSha, WorkspaceHandle};
use crate::remote_publish_policy::WorkspaceRemotePublishPolicy;

/// Maximum accepted length for caller-supplied identifiers, in bytes.
pub(crate) const MAX_IDENTIFIER_LENGTH: usize = 256;

/// Maximum accepted length for branch names, in bytes.
pub(crate) const MAX_BRANCH_NAME_LENGTH: usize = 256;

/// A fully validated request to provision one local task worktree.
///
/// Construction performs all structural validation (identifiers, branch
/// name, worktree path, exact base-SHA syntax), so an invalid request can
/// never exist. Only Git-level failures remain for
/// [`provision`](Self::provision) to surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceProvisionRequest {
    repository_root: PathBuf,
    workspace_id: String,
    task_id: Option<String>,
    branch: String,
    worktree_path: PathBuf,
    base_sha: CommitSha,
    remote_publish_policy: Option<WorkspaceRemotePublishPolicy>,
}

impl WorkspaceProvisionRequest {
    /// Validates and assembles a provisioning request.
    ///
    /// `base_sha` must be an exact lowercase 40-character hexadecimal SHA;
    /// its existence as a commit is verified later, during provisioning.
    pub fn new(
        repository_root: impl Into<PathBuf>,
        workspace_id: &str,
        task_id: Option<&str>,
        branch: &str,
        worktree_path: impl Into<PathBuf>,
        base_sha: &str,
    ) -> Result<Self, WorkspaceError> {
        let base_sha = CommitSha::parse(base_sha)?;
        let repository_root = repository_root.into();
        let worktree_path = worktree_path.into();

        validate_identifier("workspace_id", workspace_id).map_err(|detail| {
            WorkspaceError::InvalidWorkspaceId {
                detail: detail.to_string(),
            }
        })?;
        if let Some(task_id) = task_id {
            validate_identifier("task_id", task_id).map_err(|detail| {
                WorkspaceError::InvalidTaskId {
                    detail: detail.to_string(),
                }
            })?;
        }
        validate_branch_name(branch).map_err(|reason| WorkspaceError::BranchNameInvalid {
            branch: branch.to_string(),
            reason,
        })?;
        validate_worktree_path(&worktree_path)?;

        Ok(Self {
            repository_root,
            workspace_id: workspace_id.to_string(),
            task_id: task_id.map(str::to_string),
            branch: branch.to_string(),
            worktree_path,
            base_sha,
            remote_publish_policy: None,
        })
    }

    /// Stores an explicit policy without executing or authorizing remote operations.
    pub fn with_remote_publish_policy(mut self, policy: WorkspaceRemotePublishPolicy) -> Self {
        self.remote_publish_policy = Some(policy);
        self
    }

    /// The local Git repository the branch and worktree are created in.
    pub fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    /// The durable workspace identity supplied by the caller.
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    /// The optional task identity supplied by the caller.
    pub fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }

    /// The task branch to create from the base commit.
    pub fn branch(&self) -> &str {
        &self.branch
    }

    /// The absolute path at which the worktree will be created.
    pub fn worktree_path(&self) -> &Path {
        &self.worktree_path
    }

    /// The syntax-validated base commit the branch must be created from.
    pub fn base_sha(&self) -> &CommitSha {
        &self.base_sha
    }

    /// Executes the provisioning flow and returns the typed handle.
    ///
    /// Every step fails closed with a typed error. A failure after partial
    /// creation (for example the branch exists but the worktree cannot be
    /// added) leaves the already-created Git artifacts in place; teardown
    /// and recovery are outside this slice's scope. A failed or ambiguous
    /// operation is never converted into a successful handle.
    pub fn provision(&self) -> Result<WorkspaceHandle, WorkspaceError> {
        // Step 0: resolve the requested repository root to its canonical
        // (realpath) filesystem location before any Git command runs, so
        // every subprocess operates from the repository's true location
        // rather than any caller-supplied alias. Failure fails closed.
        let root = std::fs::canonicalize(&self.repository_root).map_err(|error| {
            WorkspaceError::RepositoryRootUnresolvable {
                detail: format!(
                    "{:?} could not be canonicalized: {error}",
                    self.repository_root.display()
                ),
            }
        })?;
        let root = &root;

        // Step 1: the requested base must identify an existing Git commit.
        // This also proves the repository root is a usable Git repository.
        // The resolved object name must equal the requested SHA exactly;
        // any other resolution fails closed.
        let commitish = format!("{}^{{commit}}", self.base_sha.as_str());
        let verified = git::capture(
            root,
            "verify base commit",
            &[
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new(commitish.as_str()),
            ],
        )?;
        if !verified.success {
            return Err(WorkspaceError::BaseCommitVerificationFailed {
                base_sha: self.base_sha.as_str().to_string(),
                detail: format!(
                    "{}; stderr: {}",
                    verified.exit_status,
                    verified.stderr.trim()
                ),
            });
        }
        let resolved = verified.stdout.trim();
        if resolved != self.base_sha.as_str() {
            return Err(WorkspaceError::BaseCommitVerificationFailed {
                base_sha: self.base_sha.as_str().to_string(),
                detail: format!("rev-parse resolved the base to {resolved:?} instead"),
            });
        }

        // Step 2: create the task branch from that exact base commit. Git
        // refuses to overwrite an existing branch, which surfaces here as a
        // typed command failure rather than any overwrite or merge.
        git::capture(
            root,
            "create branch",
            &[
                OsStr::new("branch"),
                OsStr::new(self.branch.as_str()),
                OsStr::new(self.base_sha.as_str()),
            ],
        )?
        .require_success("create branch")?;

        // Step 3: create the worktree checking out the new branch. Paths
        // travel as argv values, never through a shell.
        git::capture(
            root,
            "add worktree",
            &[
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new(self.worktree_path.as_os_str()),
                OsStr::new(self.branch.as_str()),
            ],
        )?
        .require_success("add worktree")?;

        // Step 4: resolve the freshly created worktree directory to its
        // canonical (realpath) location and run all subsequent verification
        // from there, so HEAD/status inspection is anchored to the true
        // filesystem target rather than any lexical alias of it. Failure
        // fails closed and never yields a handle.
        let created_worktree = std::fs::canonicalize(&self.worktree_path).map_err(|error| {
            WorkspaceError::CreatedWorktreeUnresolvable {
                detail: format!(
                    "{:?} could not be canonicalized: {error}",
                    self.worktree_path.display()
                ),
            }
        })?;

        // Step 5: read the resulting worktree HEAD from its canonical path
        // and require it to equal the requested base SHA exactly.
        let head = git::capture(
            &created_worktree,
            "read worktree HEAD",
            &[OsStr::new("rev-parse"), OsStr::new("HEAD")],
        )?;
        if !head.success {
            return Err(WorkspaceError::WorktreeHeadUnavailable {
                detail: format!("{}; stderr: {}", head.exit_status, head.stderr.trim()),
            });
        }
        let observed_head = head.stdout.trim();
        if observed_head != self.base_sha.as_str() {
            return Err(WorkspaceError::WorktreeHeadMismatch {
                expected: self.base_sha.as_str().to_string(),
                observed: observed_head.to_string(),
            });
        }

        // Step 6: the newly provisioned worktree must be clean per Git
        // porcelain status, inspected from its canonical path. Any output —
        // including untracked entries — fails closed.
        let status = git::capture(
            &created_worktree,
            "inspect worktree status",
            &[OsStr::new("status"), OsStr::new("--porcelain")],
        )?;
        let status = status.require_success("inspect worktree status")?;
        if !status.stdout.trim().is_empty() {
            return Err(WorkspaceError::WorktreeNotClean {
                status: status.stdout.trim().to_string(),
            });
        }

        // All validation succeeded; only now may a handle exist. The frozen
        // handle contract carries the validated absolute path the caller
        // requested; canonical (realpath) resolution above is an execution-
        // boundary detail, not a change to observable handle semantics.
        Ok(WorkspaceHandle::provisioned(
            self.workspace_id.clone(),
            self.task_id.clone(),
            self.branch.clone(),
            self.worktree_path.clone().into_boxed_path(),
            self.base_sha.clone(),
            self.remote_publish_policy,
        ))
    }
}

/// Validates a caller-supplied identifier (`workspace_id` / `task_id`).
fn validate_identifier(field: &'static str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.len() > MAX_IDENTIFIER_LENGTH {
        return Err(format!(
            "{field} exceeds the {MAX_IDENTIFIER_LENGTH}-byte maximum"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(format!("{field} must not contain control characters"));
    }
    Ok(())
}

/// Validates a branch name against the conservative subset of Git refname
/// rules this slice enforces before invoking Git; Git itself still enforces
/// its complete rules and any residual violation surfaces as a typed
/// command failure.
pub fn validate_branch_name(branch: &str) -> Result<(), &'static str> {
    if branch.is_empty() {
        return Err("branch name must not be empty");
    }
    if branch.len() > MAX_BRANCH_NAME_LENGTH {
        return Err("branch name exceeds the 256-byte maximum");
    }
    let bytes = branch.as_bytes();
    match bytes[0] {
        b'-' => return Err("branch name must not start with '-'"),
        b'.' | b'/' => return Err("branch name must not start with '.' or '/'"),
        _ => {}
    }
    if branch.ends_with('/') || branch.ends_with('.') {
        return Err("branch name must not end with '/' or '.'");
    }
    if branch.ends_with(".lock") {
        return Err("branch name must not end with '.lock'");
    }
    for &byte in bytes {
        match byte {
            0x00..=0x1f | 0x7f => return Err("branch name must not contain control characters"),
            b' ' | b'~' | b'^' | b':' | b'?' | b'*' | b'[' | b'\\' => {
                return Err("branch name contains a character forbidden by Git refnames");
            }
            _ => {}
        }
    }
    if branch.contains("..") {
        return Err("branch name must not contain \"..\"");
    }
    if branch.contains("//") {
        return Err("branch name must not contain \"//\"");
    }
    if branch.contains("@{") {
        return Err("branch name must not contain \"@{\"");
    }
    Ok(())
}

/// Validates the worktree target path before any Git command runs: it must
/// be absolute and must not collide with existing filesystem content (an
/// existing empty directory is permitted, matching Git's own behavior).
fn validate_worktree_path(path: &Path) -> Result<(), WorkspaceError> {
    if !path.is_absolute() {
        return Err(WorkspaceError::InvalidWorktreePath {
            detail: format!(
                "worktree path {:?} must be absolute",
                path.display().to_string()
            ),
        });
    }
    match std::fs::metadata(path) {
        // A missing target includes paths whose parent components exist but
        // are not directories: the target itself does not exist, and any
        // residual problem surfaces later as a typed Git command failure.
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(WorkspaceError::InvalidWorktreePath {
            detail: format!(
                "worktree path {:?} could not be inspected: {error}",
                path.display()
            ),
        }),
        Ok(metadata) if metadata.is_dir() => {
            let occupied = std::fs::read_dir(path)
                .map(|mut entries| entries.next().is_some())
                .unwrap_or(true);
            if occupied {
                Err(WorkspaceError::InvalidWorktreePath {
                    detail: format!(
                        "worktree path {:?} already exists and is not empty",
                        path.display()
                    ),
                })
            } else {
                Ok(())
            }
        }
        Ok(_) => Err(WorkspaceError::InvalidWorktreePath {
            detail: format!(
                "worktree path {:?} already exists and is not a directory",
                path.display()
            ),
        }),
    }
}
