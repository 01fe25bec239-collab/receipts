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

use crate::date_time::WorkspaceDateTimeV1;
use crate::error::WorkspaceError;
use crate::git;
use crate::handle::{CommitSha, WorkspaceHandle};
use crate::remote_publish_policy::WorkspaceRemotePublishPolicy;
use crate::teardown::{list_registered_worktrees, record_matches};

/// Maximum accepted length for newly provisioned identifiers, in Unicode
/// characters (the wire schema's `maxLength: 200`). Existing observed
/// handles are never rewritten by this cap.
pub(crate) const MAX_IDENTIFIER_CHARACTERS: usize = 200;

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
    created_at: Option<WorkspaceDateTimeV1>,
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
            created_at: None,
        })
    }

    /// Stores an explicit policy without executing or authorizing remote operations.
    pub fn with_remote_publish_policy(mut self, policy: WorkspaceRemotePublishPolicy) -> Self {
        self.remote_publish_policy = Some(policy);
        self
    }

    /// Stores an already validated creation timestamp without changing its spelling.
    /// Omission means absent; explicit null is not represented.
    ///
    /// Raw strings cannot bypass canonical validation:
    /// ```compile_fail
    /// use receipts_workspace_execution::WorkspaceProvisionRequest;
    /// fn invalid(request: WorkspaceProvisionRequest) {
    ///     request.with_created_at(String::from("invalid"));
    /// }
    /// ```
    /// Explicit null is not an admitted value:
    /// ```compile_fail
    /// use receipts_workspace_execution::WorkspaceProvisionRequest;
    /// fn invalid(request: WorkspaceProvisionRequest) {
    ///     request.with_created_at(None);
    /// }
    /// ```
    pub fn with_created_at(mut self, created_at: WorkspaceDateTimeV1) -> Self {
        self.created_at = Some(created_at);
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
    /// Order: canonicalize the repository root and the target (canonical
    /// existing parent plus final name); preflight (Git >= 2.36, no refused
    /// filter/include configuration, base commit exists, branch free, target
    /// not registered); create the branch; add the worktree; verify HEAD and
    /// cleanliness from the canonical worktree. The handle stores the
    /// canonical worktree path (N2).
    ///
    /// Any failure after creation starts runs owned rollback, which removes
    /// only what this attempt demonstrably created: the clean registered
    /// worktree it added (non-force) and its new branch, deleted only if it
    /// still points at the base (conditional `update-ref -d`). Preexisting
    /// targets, drift or unknown state are retained and reported as
    /// [`WorkspaceError::ProvisionRollbackIncomplete`]. A complete rollback
    /// returns the original cause and lets a retry with the same identity
    /// succeed.
    pub fn provision(&self) -> Result<WorkspaceHandle, WorkspaceError> {
        let root = std::fs::canonicalize(&self.repository_root).map_err(|error| {
            WorkspaceError::RepositoryRootUnresolvable {
                detail: format!(
                    "{:?} could not be canonicalized: {error}",
                    self.repository_root.display()
                ),
            }
        })?;
        let target = canonical_target(&self.worktree_path)?;
        preflight(&root, &target, &self.branch, &self.base_sha)?;

        let mut created = Created {
            target_preexisted: target.exists(),
            ..Created::default()
        };
        match self.create_and_verify(&root, &target, &mut created) {
            Ok(handle) => Ok(handle),
            Err(cause) => Err(rollback(
                &root,
                &target,
                &self.branch,
                &self.base_sha,
                created,
                cause,
            )),
        }
    }

    fn create_and_verify(
        &self,
        root: &Path,
        target: &Path,
        created: &mut Created,
    ) -> Result<WorkspaceHandle, WorkspaceError> {
        git::capture(
            root,
            "create branch",
            &[
                OsStr::new("branch"),
                OsStr::new("--no-track"),
                OsStr::new(self.branch.as_str()),
                OsStr::new(self.base_sha.as_str()),
            ],
        )?
        .require_success("create branch")?;
        created.branch = true;
        test_fault(ProvisionFault::Branch)?;

        created.worktree = true;
        git::capture(
            root,
            "add worktree",
            &[
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new(target.as_os_str()),
                OsStr::new(self.branch.as_str()),
            ],
        )?
        .require_success("add worktree")?;
        test_fault(ProvisionFault::Worktree)?;
        #[cfg(test)]
        if PROVISION_FAULT.get() == Some(ProvisionFault::DirtyWorktree) {
            std::fs::write(target.join("worker-data"), "keep").expect("fixture write");
        }
        test_fault(ProvisionFault::DirtyWorktree)?;

        let created_worktree = std::fs::canonicalize(target).map_err(|error| {
            WorkspaceError::CreatedWorktreeUnresolvable {
                detail: format!("{:?} could not be canonicalized: {error}", target.display()),
            }
        })?;
        if created_worktree != target {
            return Err(WorkspaceError::CreatedWorktreeUnresolvable {
                detail: "created worktree resolved to a different canonical path".to_string(),
            });
        }

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

        let status = git::capture(
            &created_worktree,
            "inspect worktree status",
            &[
                OsStr::new("status"),
                OsStr::new("--porcelain"),
                OsStr::new("--untracked-files=all"),
                OsStr::new("--ignore-submodules=none"),
            ],
        )?
        .require_success("inspect worktree status")?;
        if !status.stdout.trim().is_empty() {
            return Err(WorkspaceError::WorktreeNotClean {
                status: status.stdout.trim().to_string(),
            });
        }

        // A task-bound worktree records its exact identity in protected Git
        // administrative metadata before any handle exists. `create_new`
        // never overwrites a marker; failure rolls this attempt back.
        if let Some(task_id) = &self.task_id {
            test_fault(ProvisionFault::Binding)?;
            #[cfg(test)]
            if PROVISION_FAULT.get() == Some(ProvisionFault::BindingBlocked) {
                use std::os::unix::fs::PermissionsExt;
                PROVISION_FAULT.set(None);
                let dir = crate::worktree_binding::admin_dir(root, &created_worktree)
                    .expect("fixture admin dir");
                std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o500))
                    .expect("fixture chmod");
            }
            crate::worktree_binding::record(
                root,
                &crate::worktree_binding::TaskBinding {
                    repository: root.to_path_buf(),
                    worktree: created_worktree.clone(),
                    branch: self.branch.clone(),
                    base_sha: self.base_sha.clone(),
                    workspace_id: self.workspace_id.clone(),
                    task_id: task_id.clone(),
                },
            )?;
        }

        Ok(WorkspaceHandle::provisioned(
            self.workspace_id.clone(),
            self.task_id.clone(),
            self.branch.clone(),
            created_worktree.into_boxed_path(),
            self.base_sha.clone(),
            self.remote_publish_policy,
            self.created_at.clone(),
        ))
    }
}

/// Resources this provisioning attempt created (or attempted to create).
#[derive(Debug, Default, Clone, Copy)]
struct Created {
    branch: bool,
    worktree: bool,
    target_preexisted: bool,
}

/// Canonical form of a not-yet-existing target: canonical parent + name.
/// The parent must exist; `.`/`..` final components are refused.
pub(crate) fn canonical_target(path: &Path) -> Result<PathBuf, WorkspaceError> {
    let invalid = |detail: String| WorkspaceError::InvalidWorktreePath { detail };
    let name = match path.components().next_back() {
        Some(std::path::Component::Normal(name)) => name,
        _ => {
            return Err(invalid(format!(
                "worktree path {:?} must end in a normal name",
                path.display()
            )));
        }
    };
    let parent = path
        .parent()
        .ok_or_else(|| invalid("worktree path has no parent".to_string()))?;
    let parent = std::fs::canonicalize(parent).map_err(|error| {
        invalid(format!(
            "worktree parent {:?} could not be canonicalized: {error}",
            parent.display()
        ))
    })?;
    Ok(parent.join(name))
}

/// Minimum Git supporting `worktree list --porcelain -z` (teardown identity).
pub(crate) const MIN_GIT_VERSION: (u32, u32) = (2, 36);

/// Read-only preflight. Nothing is created and nothing is cleaned up here.
pub(crate) fn preflight(
    root: &Path,
    target: &Path,
    branch: &str,
    base_sha: &CommitSha,
) -> Result<(), WorkspaceError> {
    let version = git::capture(root, "read git version", &[OsStr::new("version")])?
        .require_success("read git version")?;
    match git::git_version_at_least(&version.stdout, MIN_GIT_VERSION.0, MIN_GIT_VERSION.1) {
        Some(true) => {}
        _ => {
            return Err(WorkspaceError::GitVersionUnsupported {
                detail: format!(
                    "{:?}; at least {}.{} is required",
                    version.stdout.trim(),
                    MIN_GIT_VERSION.0,
                    MIN_GIT_VERSION.1
                ),
            });
        }
    }
    git::refuse_untrusted_repository_config(root, "inspect repository configuration")?;

    let commitish = format!("{}^{{commit}}", base_sha.as_str());
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
            base_sha: base_sha.as_str().to_string(),
            detail: format!(
                "{}; stderr: {}",
                verified.exit_status,
                verified.stderr.trim()
            ),
        });
    }
    let resolved = verified.stdout.trim();
    if resolved != base_sha.as_str() {
        return Err(WorkspaceError::BaseCommitVerificationFailed {
            base_sha: base_sha.as_str().to_string(),
            detail: format!("rev-parse resolved the base to {resolved:?} instead"),
        });
    }

    if branch_target(root, branch)?.is_some() {
        return Err(WorkspaceError::BranchAlreadyExists {
            branch: branch.to_string(),
        });
    }

    let registered = list_registered_worktrees(root, "list registered worktrees", |detail| {
        WorkspaceError::InvalidWorktreePath { detail }
    })?;
    if registered
        .iter()
        .any(|record| record_matches(record, target))
    {
        return Err(WorkspaceError::InvalidWorktreePath {
            detail: format!("{:?} is already a registered worktree", target.display()),
        });
    }
    validate_worktree_path(target)
}

/// The exact commit `refs/heads/<branch>` points at, or `None` if absent.
pub(crate) fn branch_target(root: &Path, branch: &str) -> Result<Option<String>, WorkspaceError> {
    let reference = format!("refs/heads/{branch}");
    let probe = git::capture(
        root,
        "inspect branch",
        &[
            OsStr::new("rev-parse"),
            OsStr::new("--verify"),
            OsStr::new("--quiet"),
            OsStr::new(reference.as_str()),
        ],
    )?;
    if probe.success {
        return Ok(Some(probe.stdout.trim().to_string()));
    }
    if probe.exit_status == "exit status: 1" && probe.raw_stdout().is_empty() {
        return Ok(None);
    }
    Err(WorkspaceError::GitCommandFailed {
        operation: "inspect branch",
        detail: format!("{}; stderr: {}", probe.exit_status, probe.stderr.trim()),
    })
}

/// Owned rollback. Never forces, prunes, resets or deletes data it did not
/// create; returns `cause` only when cleanup of every created resource is
/// proven, otherwise [`WorkspaceError::ProvisionRollbackIncomplete`].
fn rollback(
    root: &Path,
    target: &Path,
    branch: &str,
    base_sha: &CommitSha,
    created: Created,
    cause: WorkspaceError,
) -> WorkspaceError {
    let mut problems: Vec<String> = Vec::new();
    if created.worktree {
        match list_registered_worktrees(root, "rollback: list worktrees", |detail| {
            WorkspaceError::InvalidWorktreePath { detail }
        }) {
            Err(error) => problems.push(format!("worktree registration unknown: {error}")),
            Ok(records) => {
                let matching = records
                    .iter()
                    .filter(|record| record_matches(record, target))
                    .count();
                match (matching, created.target_preexisted) {
                    (0, true) => {}
                    (0, false) => {
                        // Git removes its own partial checkout on failed add; an
                        // empty leftover directory is ours, anything else is kept.
                        if target.exists() && std::fs::remove_dir(target).is_err() {
                            problems.push("unregistered target directory retained".into());
                        }
                    }
                    (1, true) => {
                        problems.push("registered worktree at a preexisting target retained".into())
                    }
                    (1, false) => {
                        // Non-force: Git refuses to remove modified or untracked content.
                        let removed = git::capture(
                            root,
                            "rollback: remove worktree",
                            &[
                                OsStr::new("worktree"),
                                OsStr::new("remove"),
                                OsStr::new(target.as_os_str()),
                            ],
                        );
                        match removed {
                            Ok(capture) if capture.success => {}
                            Ok(capture) => problems
                                .push(format!("worktree retained: {}", capture.stderr.trim())),
                            Err(error) => problems.push(format!("worktree retained: {error}")),
                        }
                    }
                    _ => problems.push("ambiguous worktree registration retained".into()),
                }
            }
        }
    }
    if created.branch {
        if !problems.is_empty() {
            problems.push("branch retained because worktree cleanup is incomplete".into());
        } else {
            let reference = format!("refs/heads/{branch}");
            match git::capture(
                root,
                "rollback: delete created branch",
                &[
                    OsStr::new("update-ref"),
                    OsStr::new("-d"),
                    OsStr::new(reference.as_str()),
                    OsStr::new(base_sha.as_str()),
                ],
            ) {
                Ok(capture) if capture.success => {}
                Ok(capture) => problems.push(format!(
                    "branch retained (moved or locked): {}",
                    capture.stderr.trim()
                )),
                Err(error) => problems.push(format!("branch retained: {error}")),
            }
        }
    }
    if problems.is_empty() {
        cause
    } else {
        WorkspaceError::ProvisionRollbackIncomplete {
            cause: Box::new(cause),
            detail: problems.join("; "),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProvisionFault {
    Branch,
    Worktree,
    /// Also leaves an untracked file in the new worktree, which non-force
    /// rollback must refuse to delete.
    DirtyWorktree,
    /// Fails before the task binding is written (clean rollback expected).
    Binding,
    /// Makes the worktree's administrative directory unwritable so the real
    /// binding write fails and non-force rollback cannot remove it.
    #[cfg(test)]
    BindingBlocked,
}

#[cfg(test)]
thread_local! {
    pub(crate) static PROVISION_FAULT: std::cell::Cell<Option<ProvisionFault>> = const { std::cell::Cell::new(None) };
}

fn test_fault(_point: ProvisionFault) -> Result<(), WorkspaceError> {
    #[cfg(test)]
    if PROVISION_FAULT.get() == Some(_point) {
        PROVISION_FAULT.set(None);
        return Err(WorkspaceError::GitCommandFailed {
            operation: "injected provisioning fault",
            detail: format!("{_point:?}"),
        });
    }
    Ok(())
}

/// Validates a caller-supplied identifier (`workspace_id` / `task_id`).
pub(crate) fn validate_identifier(field: &'static str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    if value.chars().count() > MAX_IDENTIFIER_CHARACTERS {
        return Err(format!(
            "{field} exceeds the {MAX_IDENTIFIER_CHARACTERS}-character maximum"
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
pub(crate) fn validate_worktree_path(path: &Path) -> Result<(), WorkspaceError> {
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
