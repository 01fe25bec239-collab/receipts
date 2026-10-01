//! Safe local Git worktree teardown for task workspaces.
//!
//! [`WorkspaceTeardownRequest::teardown`] executes the frozen cleanup
//! contract for an accepted workspace: the worktree checkout is removed,
//! while the task branch is retained — together with every commit it
//! carries — until workstream integration consumes it. Whether a workspace
//! should be torn down at all is a higher-level policy decision made by the
//! caller; this module only performs the narrow, fully verified local
//! removal once that decision has been made.
//! [`WorkspaceTeardownRequest::teardown_finalized`] adds the same mechanical
//! removal using `evidence.handle()` for identity and `evidence.final_sha()`
//! for the expected HEAD. Finalization evidence conveys no policy approval.
//!
//! The successful flow is strictly ordered:
//!
//! 1. validate the supplied [`WorkspaceHandle`](crate::handle::WorkspaceHandle)
//!    state (only `PROVISIONED` is supported by this slice);
//! 2. canonicalize the repository root;
//! 3. canonicalize the existing worktree checkout path;
//! 4. prove the worktree is registered to exactly this repository, exactly
//!    once, via the raw NUL-delimited `git worktree list --porcelain -z`;
//! 5. prove the checked-out branch equals the handle's branch;
//! 6. read the exact current HEAD and require it to equal the handle's
//!    verified head evidence (a `PROVISIONED` handle carries its base
//!    commit as verified evidence, so any post-provisioning change fails
//!    closed), or the final SHA for the finalized-evidence entry point;
//! 7. require a clean `git status --porcelain` immediately before removal;
//! 8. remove the registered worktree without any force flag;
//! 9. independently verify the worktree is no longer registered and that
//!    the retained branch still exists and still resolves to the exact
//!    previously observed commit;
//! 10. return a handle in the `TORN_DOWN` state preserving all immutable
//!     identity fields.
//!
//! Every pre-removal verification failure leaves the repository untouched: a dirty,
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

use crate::WorkspaceAttemptFinalizationEvidence;
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
    /// Pre-removal failures leave the worktree, registration and branch
    /// untouched. Post-removal failures report a typed error without repair.
    /// Removal uses explicit argv with the
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

        self.teardown_verified_head(handle, &expected_head)
    }

    /// Removes a finalized worktree using only the evidence's handle and final SHA.
    ///
    /// Evidence is mechanical, not acceptance, review, integration, publication,
    /// security, deployment, or teardown policy approval. The caller decides
    /// whether removal should happen. Identity, HEAD and cleanliness are freshly
    /// checked; evidence is not a lock against concurrent writers.
    /// Post-removal verification errors do not restore or repair removed state.
    pub fn teardown_finalized(
        &self,
        evidence: &WorkspaceAttemptFinalizationEvidence,
    ) -> Result<WorkspaceHandle, WorkspaceError> {
        let handle = evidence.handle();
        if handle.state() != WorkspaceState::Provisioned {
            return Err(WorkspaceError::TeardownUnsupportedState {
                state: handle.state().as_str(),
            });
        }
        self.teardown_verified_head(handle, evidence.final_sha())
    }

    fn teardown_verified_head(
        &self,
        handle: &WorkspaceHandle,
        expected_head: &CommitSha,
    ) -> Result<WorkspaceHandle, WorkspaceError> {
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
        // repository, exactly once. Listing from the canonical root
        // enumerates only that repository's registrations, so one matching
        // entry proves both registration and membership; several matching
        // entries leave the identity ambiguous and refuse removal.
        let registered = list_registered_worktrees(root, "list registered worktrees", |detail| {
            WorkspaceError::TeardownWorktreeNotRegistered { detail }
        })?;
        let matching: Vec<&RegisteredWorktree> = registered
            .iter()
            .filter(|record| record_matches(record, &canonical_worktree))
            .collect();
        let [record] = matching.as_slice() else {
            return Err(WorkspaceError::TeardownWorktreeNotRegistered {
                detail: format!(
                    "{:?} matches {} of the {} worktree(s) registered with {:?}; exactly one is required",
                    requested_path.display(),
                    matching.len(),
                    registered.len(),
                    root.display()
                ),
            });
        };

        // Step 5: the registered checkout must be on the handle's branch,
        // compared as Git's exact ref bytes.
        let expected_ref = format!("refs/heads/{}", handle.branch());
        if record.branch_ref.as_deref() != Some(expected_ref.as_bytes()) {
            return Err(WorkspaceError::TeardownBranchMismatch {
                expected_branch: handle.branch().to_string(),
                // Display only; the comparison above is byte-exact.
                observed: record.branch_ref.as_deref().map_or_else(
                    || "(no branch: detached or non-branch checkout)".to_string(),
                    |branch_ref| String::from_utf8_lossy(branch_ref).into_owned(),
                ),
            });
        }

        // Step 6: read the exact current HEAD from the canonical worktree
        // path and require it to equal the entry point's verified head evidence
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
        if &observed_head != expected_head {
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
        // Explicit flags prevent repository configuration from hiding dirt;
        // optional index refresh writes are unnecessary for this observation.
        let status = git::capture(
            &canonical_worktree,
            "inspect worktree status",
            &[
                OsStr::new("--no-optional-locks"),
                OsStr::new("status"),
                OsStr::new("--porcelain"),
                OsStr::new("--untracked-files=all"),
                OsStr::new("--ignore-submodules=none"),
            ],
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

        #[cfg(test)]
        crate::teardown_tests::run_post_removal_gate();

        // Step 9a: success is not inferred from the removal command's exit
        // status alone — re-listing must show the checkout is no longer
        // registered.
        let remaining =
            list_registered_worktrees(root, "verify worktree deregistration", |detail| {
                WorkspaceError::TeardownRegistrationVerificationFailed { detail }
            })?;
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

/// One record of `git worktree list --porcelain -z` output, built from the
/// exact bytes Git reported.
#[derive(Debug, Default)]
pub(crate) struct RegisteredWorktree {
    /// The checkout path, from Git's raw path bytes.
    pub(crate) path: PathBuf,
    /// The exact full ref name of the checked-out branch, when the checkout
    /// has one (`None` for detached or bare records).
    pub(crate) branch_ref: Option<Vec<u8>>,
    head: bool,
    detached: bool,
    bare: bool,
}

/// Runs `git worktree list --porcelain -z` from the canonical repository
/// root and parses its complete raw stdout. Unparseable output becomes the
/// caller's typed error, carrying only constant detail.
fn list_registered_worktrees(
    root: &Path,
    operation: &'static str,
    malformed: fn(String) -> WorkspaceError,
) -> Result<Vec<RegisteredWorktree>, WorkspaceError> {
    let capture = git::capture(
        root,
        operation,
        &[
            OsStr::new("worktree"),
            OsStr::new("list"),
            OsStr::new("--porcelain"),
            OsStr::new("-z"),
        ],
    )?;
    let capture = capture.require_success(operation)?;
    parse_worktree_list(capture.raw_stdout()).map_err(|reason| {
        malformed(format!(
            "{operation}: refused `git worktree list --porcelain -z` output: {reason}"
        ))
    })
}

/// Parses complete `git worktree list --porcelain -z` output.
///
/// Every attribute is one NUL-terminated `label[ value]` field; a record
/// starts with its `worktree <absolute path>` field and ends with an empty
/// field, so complete output ends in two NULs. Paths and branch refs keep
/// Git's exact bytes: nothing is trimmed, unquoted or decoded. Valid
/// detached and bare records are accepted, and attributes without identity
/// meaning (`locked`, `prunable`, future additions) are ignored. Truncated,
/// malformed, repeated or contradictory identity evidence refuses the whole
/// listing with a constant reason that never echoes Git's bytes.
pub(crate) fn parse_worktree_list(stdout: &[u8]) -> Result<Vec<RegisteredWorktree>, &'static str> {
    let fields = stdout
        .strip_suffix(b"\0")
        .ok_or("output does not end with a NUL terminator")?;
    let mut records = Vec::new();
    let mut open: Option<RegisteredWorktree> = None;
    for field in fields.split(|&byte| byte == 0) {
        let Some(record) = open.as_mut() else {
            let path = field
                .strip_prefix(b"worktree ")
                .ok_or("record does not start with a worktree attribute")?;
            let path = native_path(path)?;
            if !path.is_absolute() {
                return Err("worktree path is not absolute");
            }
            open = Some(RegisteredWorktree {
                path,
                ..RegisteredWorktree::default()
            });
            continue;
        };
        if field.is_empty() {
            if record.head == record.bare {
                return Err("record must carry exactly one of HEAD or bare");
            }
            if record.branch_ref.is_some() && (record.detached || record.bare) {
                return Err("record names a branch but is detached or bare");
            }
            records.extend(open.take());
            continue;
        }
        let (label, value) = match field.iter().position(|&byte| byte == b' ') {
            Some(space) => (&field[..space], Some(&field[space + 1..])),
            None => (field, None),
        };
        let repeated = match (label, value) {
            (b"branch", Some(name)) if !name.is_empty() => {
                record.branch_ref.replace(name.to_vec()).is_some()
            }
            (b"HEAD", Some(oid)) if !oid.is_empty() => std::mem::replace(&mut record.head, true),
            (b"detached", None) => std::mem::replace(&mut record.detached, true),
            (b"bare", None) => std::mem::replace(&mut record.bare, true),
            (b"worktree" | b"branch" | b"HEAD" | b"detached" | b"bare", _) => {
                return Err("malformed or misplaced identity attribute");
            }
            _ => false,
        };
        if repeated {
            return Err("repeated identity attribute");
        }
    }
    if open.is_some() {
        return Err("final record is not terminated");
    }
    Ok(records)
}

/// Git's raw path bytes as a native Unix path, exactly.
#[cfg(unix)]
fn native_path(bytes: &[u8]) -> Result<PathBuf, &'static str> {
    use std::os::unix::ffi::OsStrExt;
    Ok(PathBuf::from(OsStr::from_bytes(bytes)))
}

/// Without Unix byte paths only strict UTF-8 is representable; anything
/// else fails closed instead of decoding lossily.
#[cfg(not(unix))]
fn native_path(bytes: &[u8]) -> Result<PathBuf, &'static str> {
    std::str::from_utf8(bytes)
        .map(PathBuf::from)
        .map_err(|_| "worktree path is not representable on this platform")
}

/// Canonicalizes a record's reported checkout path for identity comparison,
/// tolerating symlink indirection between what Git recorded and where the
/// checkout physically lives. A record whose reported path cannot be
/// canonicalized falls back to exact path comparison, so a matching
/// registration is never missed just because its checkout is mid-removal.
fn record_matches(record: &RegisteredWorktree, canonical: &Path) -> bool {
    match std::fs::canonicalize(&record.path) {
        Ok(resolved) => resolved == canonical,
        Err(_) => record.path == canonical,
    }
}
