//! Safe local Git worktree teardown for task workspaces.
//!
//! [`WorkspaceTeardownRequest::teardown`] executes the frozen cleanup
//! contract for an accepted workspace: the worktree checkout is removed,
//! while the task branch is retained — together with every commit it
//! carries — until workstream integration consumes it. Whether a workspace
//! should be torn down at all is a higher-level policy decision made by the
//! caller; this module only performs the narrow, fully verified local
//! removal once that decision has been made.
//!
//! The successful flow is strictly ordered:
//!
//! 1. validate the supplied [`WorkspaceHandle`](crate::handle::WorkspaceHandle)
//!    state (only `PROVISIONED` is supported by this slice);
//! 2. canonicalize the repository root;
//! 3. canonicalize the existing worktree checkout path;
//! 4. prove the worktree is registered to exactly this repository via
//!    `git worktree list --porcelain`;
//! 5. prove the checked-out branch equals the handle's branch;
//! 6. read the exact current HEAD and require it to equal the handle's
//!    verified head evidence (a `PROVISIONED` handle carries its base
//!    commit as verified evidence, so any post-provisioning change fails
//!    closed);
//! 7. require a clean `git status --porcelain` immediately before removal;
//! 8. remove the registered worktree without any force flag;
//! 9. independently verify the worktree is no longer registered and that
//!    the retained branch still exists and still resolves to the exact
//!    previously observed commit;
//! 10. return a handle in the `TORN_DOWN` state preserving all immutable
//!     identity fields.
//!
//! Every verification failure leaves the repository untouched: a dirty,
//! stale-evidence, mismatched, or unregistered worktree is never removed.
//! Dirty or inconsistent workspaces remain available for later recovery,
//! which is a separate milestone outside this slice. No remote operation is
//! performed at any step: no credential handling, no fetch, no push, no
//! remote publication, and no force-push exist here. All Git execution goes
//! through the hardened argv-only boundary in [`crate::git`]: absolute
//! resolved executable, canonical working directories, and an allowlisted
//! child environment.
//!
//! A Git worktree provides workspace isolation only. It is NOT a security
//! sandbox, and removing one is ordinary workspace cleanup — nothing about
//! this flow contains, confines, or isolates processes or access.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::error::WorkspaceError;
use crate::git;
use crate::handle::{CommitSha, WorkspaceHandle, WorkspaceState};

/// A request to tear down one provisioned local task worktree.
///
/// The only caller-supplied input is the local repository root whose
/// registered worktrees are operated on; everything else comes from the
/// immutable [`WorkspaceHandle`] being retired. There is nothing meaningful
/// to validate before execution — an unusable root or unregistered worktree
/// surfaces as a typed failure from [`teardown`](Self::teardown) itself —
/// so construction is infallible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceTeardownRequest {
    repository_root: PathBuf,
}

impl WorkspaceTeardownRequest {
    /// Assembles a teardown request for worktrees registered under
    /// `repository_root`.
    pub fn new(repository_root: impl Into<PathBuf>) -> Self {
        Self {
            repository_root: repository_root.into(),
        }
    }

    /// The local Git repository whose registered worktrees are operated on.
    pub fn repository_root(&self) -> &Path {
        &self.repository_root
    }

    /// Executes the teardown flow for `handle` and returns the derived
    /// `TORN_DOWN` handle.
    ///
    /// Every step fails closed with a typed error and leaves the worktree,
    /// its registration, and the retained branch untouched unless full
    /// verification succeeded. Removal uses explicit argv with the
    /// canonical worktree path and never any force flag; Git performs the
    /// registered-worktree removal itself.
    pub fn teardown(&self, handle: &WorkspaceHandle) -> Result<WorkspaceHandle, WorkspaceError> {
        // Step 1: only the single frozen transition of this slice is
        // implemented. Any other state — including TORN_DOWN, for which
        // teardown is deliberately not idempotent — is rejected instead of
        // inventing future lifecycle semantics.
        if handle.state() != WorkspaceState::Provisioned {
            return Err(WorkspaceError::TeardownUnsupportedState {
                state: handle.state().as_str(),
            });
        }
        let expected_head = match handle.head_sha() {
            Some(verified) => verified.clone(),
            None => return Err(WorkspaceError::TeardownHeadEvidenceMissing),
        };

        // Step 2: resolve the requested repository root to its canonical
        // (realpath) location before any Git command runs, so registration
        // and retention checks operate on the repository's true location
        // rather than any caller-supplied alias.
        let root = std::fs::canonicalize(&self.repository_root).map_err(|error| {
            WorkspaceError::RepositoryRootUnresolvable {
                detail: format!(
                    "{:?} could not be canonicalized: {error}",
                    self.repository_root.display()
                ),
            }
        })?;
        let root = &root;

        // Step 3: resolve the existing worktree checkout to its canonical
        // (realpath) location. A missing or dangling checkout cannot have
        // its identity proven, so this fails closed before anything is
        // inspected or removed.
        let requested_path = handle.worktree_path();
        let canonical_worktree = std::fs::canonicalize(requested_path).map_err(|error| {
            WorkspaceError::WorktreeUnresolvable {
                detail: format!(
                    "{:?} could not be canonicalized: {error}",
                    requested_path.display()
                ),
            }
        })?;

        // Step 4: the worktree must be registered with exactly this
        // repository. Listing from the canonical root enumerates only that
        // repository's registrations, so a matching entry proves both
        // registration and membership.
        let registered = list_registered_worktrees(root, "list registered worktrees")?;
        let record = match registered
            .iter()
            .find(|record| record_matches(record, &canonical_worktree))
        {
            Some(record) => record,
            None => {
                return Err(WorkspaceError::TeardownWorktreeNotRegistered {
                    detail: format!(
                        "{:?} does not appear among the {} worktree(s) registered with {:?}",
                        requested_path.display(),
                        registered.len(),
                        root.display()
                    ),
                });
            }
        };

        // Step 5: the registered checkout must be on the handle's branch.
        let expected_ref = format!("refs/heads/{}", handle.branch());
        if record.branch_ref.as_deref() != Some(expected_ref.as_str()) {
            return Err(WorkspaceError::TeardownBranchMismatch {
                expected_branch: handle.branch().to_string(),
                observed: record
                    .branch_ref
                    .clone()
                    .unwrap_or_else(|| "(no branch: detached or non-branch checkout)".to_string()),
            });
        }

        // Step 6: read the exact current HEAD from the canonical worktree
        // path and require it to equal the handle's verified head evidence
        // exactly. Cleanliness alone proves nothing about staleness: a
        // worktree with freshly committed work would still be clean, and
        // removing it here would destroy partial results under outdated
        // evidence.
        let head = git::capture(
            &canonical_worktree,
            "read worktree HEAD",
            &[OsStr::new("rev-parse"), OsStr::new("HEAD")],
        )?;
        if !head.success {
            return Err(WorkspaceError::TeardownHeadUnavailable {
                detail: format!("{}; stderr: {}", head.exit_status, head.stderr.trim()),
            });
        }
        let observed = head.stdout.trim();
        let observed_head = CommitSha::parse(observed).map_err(|_| {
            WorkspaceError::TeardownHeadUnavailable {
                detail: format!(
                    "reported HEAD {observed:?} is not an exact lowercase 40-character hexadecimal commit SHA"
                ),
            }
        })?;
        if observed_head != expected_head {
            return Err(WorkspaceError::TeardownHeadMismatch {
                expected: expected_head.as_str().to_string(),
                observed: observed.to_string(),
            });
        }

        // Step 7: immediately before removal the worktree must be clean
        // per Git porcelain status, inspected from its canonical path. Any
        // output — tracked modifications, staged modifications, untracked
        // files — refuses removal so the evidence stays intact for later
        // recovery.
        let status = git::capture(
            &canonical_worktree,
            "inspect worktree status",
            &[OsStr::new("status"), OsStr::new("--porcelain")],
        )?;
        let status = status.require_success("inspect worktree status")?;
        if !status.stdout.trim().is_empty() {
            return Err(WorkspaceError::TeardownWorktreeDirty {
                status: status.stdout.trim().to_string(),
            });
        }

        // Step 8: perform the registered-worktree removal through the
        // hardened boundary. Paths travel as individual argv values; no
        // force flag exists anywhere in this flow, so Git's own refusal
        // semantics remain fully in effect.
        git::capture(
            root,
            "remove worktree",
            &[
                OsStr::new("worktree"),
                OsStr::new("remove"),
                OsStr::new(canonical_worktree.as_os_str()),
            ],
        )?
        .require_success("remove worktree")?;

        // Step 9a: success is not inferred from the removal command's exit
        // status alone — re-listing must show the checkout is no longer
        // registered.
        let remaining = list_registered_worktrees(root, "verify worktree deregistration")?;
        if remaining
            .iter()
            .any(|record| record_matches(record, &canonical_worktree))
        {
            return Err(WorkspaceError::TeardownRegistrationVerificationFailed {
                detail: format!(
                    "{:?} is still registered after the removal command reported success",
                    canonical_worktree.display()
                ),
            });
        }

        // Step 9b: the task branch must be retained — deletion would
        // destroy the workstream's evidence — and must still resolve to the
        // exact commit observed immediately before removal. A missing or
        // retargeted branch indicates corruption and fails closed rather
        // than being recreated.
        let retained_query = format!("{expected_ref}^{{commit}}");
        let retained = git::capture(
            root,
            "verify retained branch",
            &[
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new(retained_query.as_str()),
            ],
        )?;
        if !retained.success {
            return Err(WorkspaceError::TeardownRetainedBranchMissing {
                branch: handle.branch().to_string(),
            });
        }
        let retained_target = retained.stdout.trim();
        if retained_target != observed_head.as_str() {
            return Err(WorkspaceError::TeardownRetainedBranchShaMismatch {
                branch: handle.branch().to_string(),
                expected: observed_head.as_str().to_string(),
                observed: retained_target.to_string(),
            });
        }

        // Every verification succeeded; only now may the TORN_DOWN handle
        // exist. Identity fields are preserved verbatim and the verified
        // head is the exact commit observed immediately before removal.
        Ok(WorkspaceHandle::torn_down(
            handle,
            CommitSha::parse(observed).expect("head shape was validated above"),
        ))
    }
}

/// One parsed record of `git worktree list --porcelain` output.
struct RegisteredWorktree {
    /// The checkout path as Git reports it.
    path: PathBuf,
    /// The full ref name of the checked-out branch, when the checkout has
    /// one (`None` for detached, bare, or otherwise branchless records).
    branch_ref: Option<String>,
}

/// Runs `git worktree list --porcelain` from the canonical repository root
/// and parses the records out of its stdout.
fn list_registered_worktrees(
    root: &Path,
    operation: &'static str,
) -> Result<Vec<RegisteredWorktree>, WorkspaceError> {
    let capture = git::capture(
        root,
        operation,
        &[
            OsStr::new("worktree"),
            OsStr::new("list"),
            OsStr::new("--porcelain"),
        ],
    )?;
    let capture = capture.require_success(operation)?;
    Ok(parse_worktree_list(&capture.stdout))
}

/// Parses `git worktree list --porcelain` records: each record starts at a
/// `worktree <path>` line; `branch <ref>` lines carry the checked-out
/// branch; blank lines separate records; other attributes are ignored.
fn parse_worktree_list(stdout: &str) -> Vec<RegisteredWorktree> {
    let mut records = Vec::new();
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            records.push(RegisteredWorktree {
                path: PathBuf::from(unquote_if_needed(path)),
                branch_ref: None,
            });
        } else if let Some(branch) = line.strip_prefix("branch ")
            && let Some(record) = records.last_mut()
        {
            record.branch_ref = Some(branch.to_string());
        }
    }
    records
}

/// Resolves C-style quoting that Git applies to paths containing special
/// characters, so quoted and unquoted report forms compare identically.
fn unquote_if_needed(path: &str) -> String {
    if !(path.starts_with('"') && path.ends_with('"') && path.len() >= 2) {
        return path.to_string();
    }
    let inner = &path[1..path.len() - 1];
    let mut decoded = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        match chars.next() {
            Some('a') => decoded.push('\u{7}'),
            Some('b') => decoded.push('\u{8}'),
            Some('f') => decoded.push('\u{c}'),
            Some('n') => decoded.push('\n'),
            Some('r') => decoded.push('\r'),
            Some('t') => decoded.push('\t'),
            Some('v') => decoded.push('\u{b}'),
            Some('\\') => decoded.push('\\'),
            Some('"') => decoded.push('"'),
            Some(octal) => {
                // Octal byte escapes (\NNN): decode up to three octal
                // digits into one byte, then continue decoding lossily as
                // the rest of the crate treats Git text output.
                let mut value: u32 = 0;
                let mut digits = 0;
                let mut next = Some(octal);
                while digits < 3 {
                    match next.and_then(|c| c.to_digit(8)) {
                        Some(digit) => {
                            value = value * 8 + digit;
                            digits += 1;
                            next = chars.next();
                        }
                        None => break,
                    }
                }
                if let Some(returned) = next {
                    decoded.push(returned);
                }
                if let Ok(byte) = u8::try_from(value) {
                    decoded.push(byte as char);
                }
            }
            _ => decoded.push(character),
        }
    }
    decoded
}

/// Canonicalizes a record's reported checkout path for identity comparison,
/// tolerating symlink indirection between what Git recorded and where the
/// checkout physically lives. A record whose reported path cannot be
/// canonicalized falls back to direct textual comparison, so a matching
/// registration is never missed just because its checkout is mid-removal.
fn record_matches(record: &RegisteredWorktree, canonical: &Path) -> bool {
    match std::fs::canonicalize(&record.path) {
        Ok(resolved) => resolved == canonical,
        Err(_) => record.path == canonical,
    }
}
