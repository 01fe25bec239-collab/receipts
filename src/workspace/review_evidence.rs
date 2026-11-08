//! Exact-SHA review workspaces and reviewer evidence over hardened Git.
//!
//! A review workspace is an ordinary hardened provisioned worktree on a
//! freshly generated, never pre-existing LOCAL branch
//! `receipts/review/<128-bit hex>` pinned at the reviewed implementation SHA.
//! It is not a detached HEAD: `WorkspaceHandle.branch` names that real
//! branch, and the handle is bound to the reviewed capsule's `task_id`, so
//! the Workspace protected launcher admits it like any task workspace. The
//! reviewer execution attempt is allocated by that admission and is distinct
//! from the reviewed implementation attempt.
//!
//! Every Git call goes through [`crate::git`] (absolute executable, cleared
//! environment, hooks/fsmonitor/signing/transports disabled, filter/include
//! configuration refused, bounded runs). There is no arbitrary-argv export.
//! Integrity checks (HEAD, branch, ref tip, clean status) detect movement;
//! they are not immutable-execution or OS read-only proof.
//!
//! Cleanup removes only what this module created: the worktree through the
//! verified teardown (clean, registered, expected HEAD) and then the review
//! branch through an atomic compare-and-delete at the pinned SHA, so a moved
//! or replaced ref is refused and left untouched. No remote operation exists.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::WorkspaceError;
use crate::git;
use crate::handle::{CommitSha, WorkspaceHandle};
use crate::provision::WorkspaceProvisionRequest;
use crate::teardown::WorkspaceTeardownRequest;

/// Prefix every generated review branch lives under.
pub const REVIEW_BRANCH_PREFIX: &str = "receipts/review/";
/// Maximum commits a review range may contain before it is refused.
pub const MAX_REVIEW_RANGE_COMMITS: usize = 10_000;

fn refused(operation: &'static str, detail: impl Into<String>) -> WorkspaceError {
    WorkspaceError::GitCommandFailed {
        operation,
        detail: detail.into(),
    }
}

fn os(value: &str) -> &OsStr {
    OsStr::new(value)
}

fn canonical_root(root: &Path) -> Result<PathBuf, WorkspaceError> {
    std::fs::canonicalize(root).map_err(|error| WorkspaceError::RepositoryRootUnresolvable {
        detail: format!("{:?} could not be canonicalized: {error}", root.display()),
    })
}

fn random_token() -> Result<String, WorkspaceError> {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| refused("generate review branch", e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn commit(root: &Path, operation: &'static str, sha: &CommitSha) -> Result<(), WorkspaceError> {
    let object = format!("{}^{{commit}}", sha.as_str());
    let found = git::capture(
        root,
        operation,
        &[os("rev-parse"), os("--verify"), os("--quiet"), os(&object)],
    )?
    .require_success(operation)?;
    if found.stdout.trim() != sha.as_str() {
        return Err(refused(operation, "commit does not resolve exactly"));
    }
    Ok(())
}

/// A provisioned, task-bound review workspace pinned at one exact commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewWorkspace {
    handle: WorkspaceHandle,
    reviewed_sha: CommitSha,
}

impl ReviewWorkspace {
    /// The PROVISIONED, task-bound handle (branch = the review branch).
    pub fn handle(&self) -> &WorkspaceHandle {
        &self.handle
    }
    /// The exact commit under review.
    pub fn reviewed_sha(&self) -> &CommitSha {
        &self.reviewed_sha
    }
    /// The generated local review branch.
    pub fn review_branch(&self) -> &str {
        self.handle.branch()
    }
}

/// What a review workspace needs: the repository, the canonical capsule's
/// task, a fresh workspace identity, the reviewed commit and a new path.
#[derive(Debug, Clone)]
pub struct ReviewWorkspaceRequest {
    pub repository_root: PathBuf,
    pub workspace_id: String,
    pub task_id: String,
    pub reviewed_sha: CommitSha,
    pub worktree_path: PathBuf,
}

/// Provisions a review workspace on a fresh local review branch at
/// `reviewed_sha`. An existing branch or target is never adopted.
pub fn provision_review_workspace(
    request: &ReviewWorkspaceRequest,
) -> Result<ReviewWorkspace, WorkspaceError> {
    let branch = format!("{REVIEW_BRANCH_PREFIX}{}", random_token()?);
    let handle = WorkspaceProvisionRequest::new(
        request.repository_root.clone(),
        &request.workspace_id,
        Some(&request.task_id),
        &branch,
        request.worktree_path.clone(),
        request.reviewed_sha.as_str(),
    )?
    .provision()?;
    Ok(ReviewWorkspace {
        handle,
        reviewed_sha: request.reviewed_sha.clone(),
    })
}

/// Freshly observed integrity of a review workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewWorkspaceIntegrityV1 {
    pub head_sha: CommitSha,
    pub branch_tip: CommitSha,
}

/// Refuses unless the worktree is still on its review branch, HEAD and the
/// branch tip both equal the reviewed SHA, and status (including untracked
/// files) is empty. Detects movement; does not prevent it.
pub fn verify_review_workspace(
    workspace: &ReviewWorkspace,
) -> Result<ReviewWorkspaceIntegrityV1, WorkspaceError> {
    let worktree = workspace.handle.worktree_path();
    let op = "verify review workspace";
    let head = git::capture(worktree, op, &[os("rev-parse"), os("HEAD")])?.require_success(op)?;
    let head = CommitSha::parse(head.stdout.trim())?;
    let branch = git::capture(
        worktree,
        op,
        &[os("symbolic-ref"), os("--quiet"), os("HEAD")],
    )?
    .require_success(op)?;
    let expected_ref = format!("refs/heads/{}", workspace.review_branch());
    if branch.stdout.trim() != expected_ref {
        return Err(refused(op, "worktree left its review branch"));
    }
    let tip = git::capture(
        worktree,
        op,
        &[os("rev-parse"), os("--verify"), os(&expected_ref)],
    )?
    .require_success(op)?;
    let tip = CommitSha::parse(tip.stdout.trim())?;
    if head != workspace.reviewed_sha || tip != workspace.reviewed_sha {
        return Err(refused(
            op,
            "review HEAD or branch moved from the reviewed SHA",
        ));
    }
    let status = git::capture(
        worktree,
        op,
        &[
            os("status"),
            os("--porcelain=v1"),
            os("-z"),
            os("--untracked-files=all"),
        ],
    )?
    .require_success(op)?;
    if !status.raw_stdout().is_empty() {
        return Err(refused(op, "review worktree is not clean"));
    }
    Ok(ReviewWorkspaceIntegrityV1 {
        head_sha: head,
        branch_tip: tip,
    })
}

/// Removes a review workspace this module created: verified teardown of
/// the worktree, then compare-and-delete of the review branch at the pinned
/// SHA. A moved/replaced ref or a dirty worktree is refused and retained.
pub fn remove_review_workspace(
    repository_root: &Path,
    workspace: &ReviewWorkspace,
) -> Result<(), WorkspaceError> {
    let op = "remove review workspace";
    if !workspace.review_branch().starts_with(REVIEW_BRANCH_PREFIX) {
        return Err(refused(op, "not a generated review branch"));
    }
    WorkspaceTeardownRequest::new(repository_root).teardown(&workspace.handle)?;
    let root = canonical_root(repository_root)?;
    let reference = format!("refs/heads/{}", workspace.review_branch());
    git::capture(
        &root,
        op,
        &[
            os("update-ref"),
            os("-d"),
            os(&reference),
            os(workspace.reviewed_sha.as_str()),
        ],
    )?
    .require_success(op)?;
    Ok(())
}

/// The ordered commits `baseline..implementation`, with the baseline proven
/// to be an ancestor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCommitRangeV1 {
    pub baseline_sha: CommitSha,
    pub implementation_sha: CommitSha,
    /// Oldest first; excludes the baseline, ends at the implementation.
    pub commits: Vec<CommitSha>,
}

pub fn review_commit_range(
    repository_root: &Path,
    baseline: &CommitSha,
    implementation: &CommitSha,
) -> Result<ReviewCommitRangeV1, WorkspaceError> {
    let op = "read review commit range";
    let root = canonical_root(repository_root)?;
    git::refuse_untrusted_repository_config(&root, op)?;
    commit(&root, op, baseline)?;
    commit(&root, op, implementation)?;
    let ancestor = git::capture(
        &root,
        op,
        &[
            os("merge-base"),
            os("--is-ancestor"),
            os(baseline.as_str()),
            os(implementation.as_str()),
        ],
    )?;
    if !ancestor.success {
        return Err(refused(
            op,
            "baseline is not an ancestor of the implementation",
        ));
    }
    let range = format!("{}..{}", baseline.as_str(), implementation.as_str());
    let listed = git::capture(&root, op, &[os("rev-list"), os("--reverse"), os(&range)])?
        .require_success(op)?;
    let commits = listed
        .stdout
        .lines()
        .map(CommitSha::parse)
        .collect::<Result<Vec<_>, _>>()?;
    if commits.len() > MAX_REVIEW_RANGE_COMMITS {
        return Err(refused(op, "review range exceeds the commit bound"));
    }
    Ok(ReviewCommitRangeV1 {
        baseline_sha: baseline.clone(),
        implementation_sha: implementation.clone(),
        commits,
    })
}

/// Exact diff evidence between two commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewDiffEvidenceV1 {
    /// Changed paths (rename detection off), as Git reports them.
    pub changed_paths: Vec<String>,
    /// SHA-256 of the full binary patch bytes.
    pub patch_sha256: String,
    pub patch_bytes: u64,
}

pub fn review_diff(
    repository_root: &Path,
    baseline: &CommitSha,
    implementation: &CommitSha,
) -> Result<ReviewDiffEvidenceV1, WorkspaceError> {
    let op = "read review diff";
    let root = canonical_root(repository_root)?;
    git::refuse_untrusted_repository_config(&root, op)?;
    commit(&root, op, baseline)?;
    commit(&root, op, implementation)?;
    let common = [
        os("diff"),
        os("--no-ext-diff"),
        os("--no-textconv"),
        os("--no-renames"),
        os("--no-color"),
    ];
    let mut names = common.to_vec();
    names.extend([
        os("--name-only"),
        os("-z"),
        os(baseline.as_str()),
        os(implementation.as_str()),
    ]);
    let listed = git::capture(&root, op, &names)?.require_success(op)?;
    let changed_paths = listed
        .raw_stdout()
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    let mut patch = common.to_vec();
    patch.extend([
        os("--binary"),
        os("--full-index"),
        os(baseline.as_str()),
        os(implementation.as_str()),
    ]);
    let patch = git::capture(&root, op, &patch)?.require_success(op)?;
    let digest = Sha256::digest(patch.raw_stdout());
    Ok(ReviewDiffEvidenceV1 {
        changed_paths,
        patch_sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
        patch_bytes: patch.raw_stdout().len() as u64,
    })
}

/// A temporary merge result checked out in its own review workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporaryMergeV1 {
    pub target_sha: CommitSha,
    pub candidate_sha: CommitSha,
    /// Deterministic merged tree object.
    pub merged_tree: String,
    /// Local, unreferenced-elsewhere merge commit (target, candidate parents).
    pub merge_sha: CommitSha,
    pub workspace: ReviewWorkspace,
}

/// Computes `target + candidate` without touching any worktree or ref
/// (`git merge-tree --write-tree`), records a local two-parent commit, and
/// provisions a review workspace at it for post-merge-tree checks. A
/// conflict is refused and creates nothing. Remove with
/// [`remove_review_workspace`].
pub fn provision_temporary_merge(
    request: &ReviewWorkspaceRequest,
    target: &CommitSha,
) -> Result<TemporaryMergeV1, WorkspaceError> {
    let op = "compute temporary merge";
    let root = canonical_root(&request.repository_root)?;
    git::refuse_untrusted_repository_config(&root, op)?;
    let version = git::capture(&root, op, &[os("version")])?.require_success(op)?;
    if git::git_version_at_least(&version.stdout, 2, 38) != Some(true) {
        return Err(refused(
            op,
            "git merge-tree --write-tree requires Git 2.38 or later",
        ));
    }
    let candidate = &request.reviewed_sha;
    commit(&root, op, target)?;
    commit(&root, op, candidate)?;
    let merged = git::capture(
        &root,
        op,
        &[
            os("merge-tree"),
            os("--write-tree"),
            os("--no-messages"),
            os(target.as_str()),
            os(candidate.as_str()),
        ],
    )?;
    if !merged.success {
        return Err(refused(op, "temporary merge has conflicts"));
    }
    let merged_tree = merged
        .stdout
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if merged_tree.len() != 40 || !merged_tree.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(refused(op, "unexpected merge-tree output"));
    }
    let made = git::capture(
        &root,
        op,
        &[
            os("-c"),
            os("user.name=Receipts temporary merge"),
            os("-c"),
            os("user.email=temporary-merge@receipts.invalid"),
            os("commit-tree"),
            os(&merged_tree),
            os("-p"),
            os(target.as_str()),
            os("-p"),
            os(candidate.as_str()),
            os("-m"),
            os("Receipts temporary review merge"),
        ],
    )?
    .require_success(op)?;
    let merge_sha = CommitSha::parse(made.stdout.trim())?;
    let workspace = provision_review_workspace(&ReviewWorkspaceRequest {
        reviewed_sha: merge_sha.clone(),
        ..request.clone()
    })?;
    Ok(TemporaryMergeV1 {
        target_sha: target.clone(),
        candidate_sha: candidate.clone(),
        merged_tree,
        merge_sha,
        workspace,
    })
}
