//! Protected Workspace launches: the exported interface for Runtime.
//!
//! A [`ProtectedLauncher`] exists only after the host passes platform
//! admission. On Linux x86_64 that requires an administrator-provisioned
//! launcher (effective UID 0), a delegated cgroup v2 directory with
//! `cgroup.kill`, Landlock ABI >= 3, seccomp filters and protected
//! hardlinks/symlinks. Every other platform (Darwin, Windows, Android, other
//! architectures) refuses with [`ProtectedLaunchError::Unsupported`]; the
//! standalone runner and lifecycle APIs are never a fallback for a protected
//! request.
//!
//! Model (from the PR185 qualified mechanisms):
//!
//! * **Custody.** Admission transfers the repository's Git metadata to the
//!   controller identity, strips group/other write, verifies every ancestor
//!   is root/controller-owned and not worker-writable, makes the worktree
//!   root controller-owned, worker-group, sticky `01770`, gives existing
//!   payload to the attempt's fresh worker UID, and refuses filter/include
//!   configuration. Worker identities can edit/delete payload but cannot
//!   modify, replace or rename metadata, its config or the worktree `.git`
//!   pointer, through any descendant, until the cgroup is proven empty.
//! * **Worker.** Fresh noncolliding UID (never reused by this launcher),
//!   worker GID, no supplementary groups, no capabilities (bounding set
//!   dropped), no-new-privs, private mount namespace with Git metadata
//!   bind-mounted read-only, all inherited FDs close-on-exec, and a seccomp
//!   filter refusing namespace/mount/ptrace/handle syscalls. Placed in its
//!   own cgroup before exec.
//! * **Controller.** Fixed Git argv only (no worker argv/config/ref), run as
//!   the controller identity holding only `CAP_DAC_READ_SEARCH` (to read
//!   worker `0600` payload without chmod/chown), confined by Landlock to the
//!   task repository, read/exec tool trees and a few named files, the same
//!   seccomp filter (which refuses `open_by_handle_at`), and its own cgroup.
//! * **Lifecycle.** The existing live-attempt deadline/grace/reap/EOF
//!   lifecycle is reused; cgroup kill and emptiness proof run as a cleanup
//!   step before reader verification, identity release or teardown. Unknown
//!   cleanup refuses success and retains the cgroup, UID and evidence.
//! * **Channel.** An opaque, attempt-scoped [`ProtectedAdmission`] and a
//!   task-bound [`ControllerChannel`] (worker: identity/status/stage/commit/
//!   checkpoint; reviewer: identity/status only). Requests must carry the
//!   channel's attempt, task, secret token and next sequence number, so
//!   forged, cross-task and replayed requests refuse.
//!
//! Limits: this is a qualified design, not a whole-platform sandbox. Root or
//! controller compromise, kernel bugs, non-filesystem Landlock scope and
//! side channels are outside admission assumptions. Linux enforcement is NOT
//! verified by local Darwin tests; the hosted diagnostic is separate.

// The Linux mechanism plans (custody, BPF, Landlock) are built and unit
// tested on every platform but only consumed by the Linux launcher.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::execution::{
    ExecutionError, LiveProcessAttempt, LiveProcessAttemptError, LiveProcessCancelAcceptance,
    LiveProcessOutcome, LiveProcessOutput, LiveProcessStartError, LiveProcessTerminalCause,
    ProcessRunRequest, ProcessTimeoutPolicy,
};
use crate::{
    CommitSha, WorkspaceCheckpointKind, WorkspaceCheckpointLifecycleInput,
    WorkspaceCheckpointTemporalCaptureCore, WorkspaceDateTimeV1, WorkspaceError, WorkspaceHandle,
    WorkspaceLifecycleError, WorkspaceState, git, validate_write_path,
};

/// Administrator-provisioned identities and cgroup delegation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedLaunchConfig {
    pub(crate) controller_uid: u32,
    pub(crate) controller_gid: u32,
    pub(crate) worker_gid: u32,
    worker_uids: std::ops::RangeInclusive<u32>,
    cgroup_root: PathBuf,
}

impl ProtectedLaunchConfig {
    /// Validates identity separation. The worker UID range must be dedicated
    /// to this launcher: UIDs are allocated monotonically and never reused.
    pub fn new(
        controller_uid: u32,
        controller_gid: u32,
        worker_gid: u32,
        worker_uids: std::ops::RangeInclusive<u32>,
        cgroup_root: impl Into<PathBuf>,
    ) -> Result<Self, ProtectedLaunchError> {
        let invalid = |detail: &str| Err(ProtectedLaunchError::InvalidConfig(detail.into()));
        let cgroup_root = cgroup_root.into();
        if controller_uid == 0 || controller_gid == 0 || worker_gid == 0 {
            return invalid("controller and worker identities must not be root");
        }
        if worker_uids.is_empty() || *worker_uids.start() == 0 {
            return invalid("worker UID range must be nonempty and exclude root");
        }
        if worker_uids.contains(&controller_uid) {
            return invalid("controller UID must not be a worker UID");
        }
        if worker_gid == controller_gid {
            return invalid("worker GID must differ from the controller GID");
        }
        if !cgroup_root.is_absolute() {
            return invalid("cgroup root must be absolute");
        }
        Ok(Self {
            controller_uid,
            controller_gid,
            worker_gid,
            worker_uids,
            cgroup_root,
        })
    }
}

/// Typed refusal; none of these is a protected success.
#[derive(Debug)]
#[non_exhaustive]
pub enum ProtectedLaunchError {
    /// Platform, architecture, kernel, filesystem or setup cannot provide the
    /// required enforcement. Nothing was launched.
    Unsupported(String),
    InvalidConfig(String),
    /// Repository custody could not be established or verified.
    Custody(String),
    /// Forged, cross-task, replayed or role-forbidden channel request.
    Binding(&'static str),
    /// The dedicated worker UID range has no unused identity left.
    IdentityExhausted,
    Workspace(WorkspaceError),
    Lifecycle(WorkspaceLifecycleError),
    Start(LiveProcessStartError),
    /// Cleanup was not proven; cgroup, identity and evidence are retained.
    CleanupUnknown(String),
}

impl fmt::Display for ProtectedLaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(reason) => write!(f, "protected launch unsupported: {reason}"),
            Self::InvalidConfig(reason) => write!(f, "invalid protected launch config: {reason}"),
            Self::Custody(reason) => write!(f, "repository custody refused: {reason}"),
            Self::Binding(reason) => write!(f, "controller request refused: {reason}"),
            Self::IdentityExhausted => f.write_str("no unused worker identity remains"),
            Self::Workspace(error) => error.fmt(f),
            Self::Lifecycle(error) => error.fmt(f),
            Self::Start(error) => write!(f, "protected attempt start failed: {error}"),
            Self::CleanupUnknown(detail) => write!(f, "cleanup not proven: {detail}"),
        }
    }
}
impl std::error::Error for ProtectedLaunchError {}

impl From<WorkspaceError> for ProtectedLaunchError {
    fn from(error: WorkspaceError) -> Self {
        Self::Workspace(error)
    }
}

#[derive(Debug, Default)]
struct Allocator {
    next_offset: u64,
    live_or_retained: std::collections::BTreeSet<u32>,
}

/// Whether launches are enforced. `Unenforced` exists only for this crate's
/// tests of binding/channel logic; it is never reachable from public APIs.
#[derive(Debug)]
enum Enforcement {
    #[cfg(target_os = "linux")]
    Linux(crate::protected_linux::LinuxSupport),
    /// Constructed only by `unenforced_for_test`; outside tests every use
    /// fails closed (see [`unenforced`]).
    #[cfg_attr(not(test), allow(dead_code))]
    Unenforced,
}

fn unenforced() -> Result<(), ProtectedLaunchError> {
    if cfg!(test) {
        Ok(())
    } else {
        Err(ProtectedLaunchError::Unsupported(
            "unenforced launcher outside crate tests".into(),
        ))
    }
}

#[derive(Debug)]
struct LauncherInner {
    config: ProtectedLaunchConfig,
    enforcement: Enforcement,
    allocator: Mutex<Allocator>,
}

/// Admitted protected launcher.
#[derive(Debug, Clone)]
pub struct ProtectedLauncher {
    inner: Arc<LauncherInner>,
}

impl ProtectedLauncher {
    /// Performs platform admission. Unsupported hosts refuse before any
    /// protected worker can exist.
    pub fn new(config: ProtectedLaunchConfig) -> Result<Self, ProtectedLaunchError> {
        #[cfg(target_os = "linux")]
        {
            let support = crate::protected_linux::probe(&config.cgroup_root)
                .map_err(ProtectedLaunchError::Unsupported)?;
            Ok(Self::with(config, Enforcement::Linux(support)))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = config;
            Err(ProtectedLaunchError::Unsupported(format!(
                "no admitted protected-launch mechanism on {}/{}",
                std::env::consts::OS,
                std::env::consts::ARCH
            )))
        }
    }

    fn with(config: ProtectedLaunchConfig, enforcement: Enforcement) -> Self {
        Self {
            inner: Arc::new(LauncherInner {
                config,
                enforcement,
                allocator: Mutex::default(),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn unenforced_for_test(config: ProtectedLaunchConfig) -> Self {
        Self::with(config, Enforcement::Unenforced)
    }

    /// Establishes custody and allocates a fresh attempt identity for one
    /// `PROVISIONED`/`CHECKPOINTED`/`RECOVERED` handle of `repository_root`.
    pub fn admit(
        &self,
        handle: &WorkspaceHandle,
        repository_root: &Path,
    ) -> Result<ProtectedAdmission, ProtectedLaunchError> {
        if !handle.state().may_transition_to(WorkspaceState::Active) {
            return Err(ProtectedLaunchError::Workspace(
                WorkspaceError::IllegalStateTransition {
                    from: handle.state().as_str(),
                    to: WorkspaceState::Active.as_str(),
                },
            ));
        }
        let task_id = handle
            .task_id()
            .ok_or(ProtectedLaunchError::Custody(
                "protected launches require a task-bound handle".into(),
            ))?
            .to_owned();
        let repository = std::fs::canonicalize(repository_root)
            .map_err(|error| ProtectedLaunchError::Custody(format!("repository: {error}")))?;
        let worktree = std::fs::canonicalize(handle.worktree_path())
            .map_err(|error| ProtectedLaunchError::Custody(format!("worktree: {error}")))?;
        git::refuse_untrusted_repository_config(&repository, "inspect repository configuration")?;
        let common_dir = git_path(&worktree, "--git-common-dir")?;
        let repository_common = git_path(&repository, "--git-common-dir")?;
        if common_dir != repository_common {
            return Err(ProtectedLaunchError::Custody(
                "worktree does not belong to the supplied repository".into(),
            ));
        }
        let head = git::capture(
            &worktree,
            "read worktree HEAD",
            &[
                std::ffi::OsStr::new("rev-parse"),
                std::ffi::OsStr::new("HEAD"),
            ],
        )?
        .require_success("read worktree HEAD")?;
        if Some(head.stdout.trim()) != handle.head_sha().map(CommitSha::as_str) {
            return Err(ProtectedLaunchError::Custody(
                "worktree HEAD differs from the handle's verified head".into(),
            ));
        }
        let branch = git::capture(
            &worktree,
            "read worktree branch",
            &[
                std::ffi::OsStr::new("symbolic-ref"),
                std::ffi::OsStr::new("--quiet"),
                std::ffi::OsStr::new("HEAD"),
            ],
        )?
        .require_success("read worktree branch")?;
        if branch.stdout.trim() != format!("refs/heads/{}", handle.branch()) {
            return Err(ProtectedLaunchError::Custody(
                "worktree is not on the handle's task branch".into(),
            ));
        }

        let original = original_custody(&common_dir, &worktree)?;
        let worker_uid = self.allocate_worker_uid()?;
        let attempt_id = random_hex(16)
            .map_err(|error| ProtectedLaunchError::Unsupported(format!("randomness: {error}")))?;
        let binding = Binding {
            attempt_id,
            task_id,
            workspace_id: handle.workspace_id().to_owned(),
            branch: handle.branch().to_owned(),
            common_dir,
            worktree,
            worker_uid,
            original,
            released: AtomicBool::new(false),
            handle: handle.clone(),
        };
        match &self.inner.enforcement {
            #[cfg(target_os = "linux")]
            Enforcement::Linux(_) => {
                let config = &self.inner.config;
                if let Err(error) = crate::protected_linux::establish_custody(config, &binding) {
                    self.retain(worker_uid);
                    return Err(ProtectedLaunchError::Custody(error));
                }
            }
            Enforcement::Unenforced => unenforced()?,
        }
        Ok(ProtectedAdmission {
            binding: Arc::new(binding),
            launcher: self.clone(),
        })
    }

    fn allocate_worker_uid(&self) -> Result<u32, ProtectedLaunchError> {
        let config = &self.inner.config;
        let mut allocator = self
            .inner
            .allocator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let span = u64::from(*config.worker_uids.end() - *config.worker_uids.start()) + 1;
        while allocator.next_offset < span {
            let uid = *config.worker_uids.start() + allocator.next_offset as u32;
            allocator.next_offset += 1;
            if allocator.live_or_retained.contains(&uid) || uid_in_use(uid)? {
                continue;
            }
            allocator.live_or_retained.insert(uid);
            return Ok(uid);
        }
        Err(ProtectedLaunchError::IdentityExhausted)
    }

    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    fn retain(&self, uid: u32) {
        self.inner
            .allocator
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .live_or_retained
            .insert(uid);
    }

    /// Starts the admitted worker. `request.workspace_root()` must be the
    /// admitted worktree. Returns the `ACTIVE` handle, the live attempt and
    /// the task-bound channels.
    pub fn launch(
        &self,
        admission: ProtectedAdmission,
        request: &ProcessRunRequest,
        policy: &ProcessTimeoutPolicy,
    ) -> Result<ProtectedAttempt, ProtectedLaunchError> {
        if !Arc::ptr_eq(&admission.launcher.inner, &self.inner) {
            return Err(ProtectedLaunchError::Binding(
                "admission belongs to another launcher",
            ));
        }
        let binding = admission.binding;
        match std::fs::canonicalize(request.workspace_root()) {
            Ok(root) if root == binding.worktree => {}
            _ => {
                return Err(ProtectedLaunchError::Binding(
                    "request workspace root is not the admitted worktree",
                ));
            }
        }
        let customization = match &self.inner.enforcement {
            #[cfg(target_os = "linux")]
            Enforcement::Linux(support) => {
                crate::protected_linux::worker_launch(support, &self.inner.config, &binding)
                    .map_err(ProtectedLaunchError::Unsupported)?
            }
            Enforcement::Unenforced => {
                unenforced()?;
                Default::default()
            }
        };
        let attempt =
            crate::execution::live_attempt_start_customized(request, policy, customization)
                .map_err(ProtectedLaunchError::Start)?;
        let active = binding
            .handle
            .transitioned(WorkspaceState::Active, None)
            .map_err(ProtectedLaunchError::Workspace)?;
        let worker = ControllerChannel::new(binding.clone(), ControllerRole::Worker, self.clone())?;
        let reviewer =
            ControllerChannel::new(binding.clone(), ControllerRole::Reviewer, self.clone())?;
        Ok(ProtectedAttempt {
            handle: active,
            attempt,
            collected: AtomicBool::new(false),
            worker,
            reviewer,
        })
    }
}

/// Opaque, attempt-scoped admission. Not cloneable, not constructible
/// outside this crate, consumed by exactly one launch.
///
/// ```compile_fail
/// use receipts_workspace_execution::ProtectedAdmission;
/// fn forge() -> ProtectedAdmission { ProtectedAdmission { } }
/// ```
/// ```compile_fail
/// use receipts_workspace_execution::ProtectedAdmission;
/// fn copy(a: &ProtectedAdmission) -> ProtectedAdmission { a.clone() }
/// ```
#[derive(Debug)]
pub struct ProtectedAdmission {
    binding: Arc<Binding>,
    launcher: ProtectedLauncher,
}

impl ProtectedAdmission {
    pub fn attempt_id(&self) -> &str {
        &self.binding.attempt_id
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn worker_uid_for_test(&self) -> u32 {
        self.binding.worker_uid
    }
}

/// Ownership of the repository metadata and worktree root before custody was
/// transferred; restored by [`ProtectedAttempt::release_custody`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OriginalCustody {
    pub(crate) uid: u32,
    pub(crate) gid: u32,
    pub(crate) worktree_mode: u32,
}

#[derive(Debug)]
pub(crate) struct Binding {
    pub(crate) attempt_id: String,
    pub(crate) task_id: String,
    pub(crate) workspace_id: String,
    pub(crate) branch: String,
    pub(crate) common_dir: PathBuf,
    pub(crate) worktree: PathBuf,
    pub(crate) worker_uid: u32,
    pub(crate) original: OriginalCustody,
    /// Set once custody was returned; channels then refuse every request.
    released: AtomicBool,
    handle: WorkspaceHandle,
}

/// A running protected attempt.
#[derive(Debug)]
pub struct ProtectedAttempt {
    handle: WorkspaceHandle,
    attempt: LiveProcessAttempt,
    /// Set only when `wait_collect` succeeded: the existing deadline/grace/
    /// reap/EOF lifecycle and the cgroup kill/emptiness proof both completed.
    collected: AtomicBool,
    worker: ControllerChannel,
    reviewer: ControllerChannel,
}

/// Borrowed view of the live attempt. It offers the same operations as the
/// execution layer's handle and additionally records a successful collection,
/// the only evidence that permits [`ProtectedAttempt::release_custody`].
#[derive(Debug)]
pub struct ProtectedAttemptView<'a> {
    attempt: &'a LiveProcessAttempt,
    collected: &'a AtomicBool,
}

impl ProtectedAttemptView<'_> {
    /// See [`LiveProcessAttempt::snapshot_output`].
    pub fn snapshot_output(&self) -> Result<LiveProcessOutput, ExecutionError> {
        self.attempt.snapshot_output()
    }
    /// See [`LiveProcessAttempt::cancel`].
    pub fn cancel(&self) -> LiveProcessCancelAcceptance {
        self.attempt.cancel()
    }
    /// See [`LiveProcessAttempt::terminal_cause`].
    pub fn terminal_cause(&self) -> Option<LiveProcessTerminalCause> {
        self.attempt.terminal_cause()
    }
    /// See [`LiveProcessAttempt::wait_collect`]. A failed collection leaves
    /// cleanup unproven: custody, cgroup and identity stay retained.
    pub fn wait_collect(&self) -> Result<LiveProcessOutcome, LiveProcessAttemptError> {
        let outcome = self.attempt.wait_collect()?;
        self.collected.store(true, Ordering::SeqCst);
        Ok(outcome)
    }
}

impl ProtectedAttempt {
    /// The `ACTIVE` handle observed at start.
    pub fn handle(&self) -> &WorkspaceHandle {
        &self.handle
    }
    /// The live attempt (cancel/snapshot/collect).
    pub fn attempt(&self) -> ProtectedAttemptView<'_> {
        ProtectedAttemptView {
            attempt: &self.attempt,
            collected: &self.collected,
        }
    }
    /// The worker's task-bound channel.
    pub fn worker_channel(&self) -> &ControllerChannel {
        &self.worker
    }
    /// The query-only reviewer channel.
    pub fn reviewer_channel(&self) -> &ControllerChannel {
        &self.reviewer
    }

    /// Returns the repository metadata and worktree to their pre-admission
    /// owner so ordinary Workspace APIs (for example non-force teardown) run
    /// as that owner again. Allowed exactly once, only after
    /// [`ProtectedAttemptView::wait_collect`] succeeded (every attempt process
    /// reaped, its cgroup killed and proven empty) and with no live process
    /// left under the worker identity. Afterwards both channels refuse.
    /// Refusal retains custody. File modes are not widened again.
    pub fn release_custody(&self) -> Result<(), ProtectedLaunchError> {
        if !self.collected.load(Ordering::SeqCst) {
            return Err(ProtectedLaunchError::CleanupUnknown(
                "attempt collection and cgroup cleanup were not proven; custody retained".into(),
            ));
        }
        let binding = &self.worker.binding;
        if binding.released.swap(true, Ordering::SeqCst) {
            return Err(ProtectedLaunchError::Custody(
                "custody was already released".into(),
            ));
        }
        let result = match &self.worker.launcher.inner.enforcement {
            #[cfg(target_os = "linux")]
            Enforcement::Linux(_) => crate::protected_linux::release_custody(binding),
            Enforcement::Unenforced => unenforced().map_err(|error| error.to_string()),
        };
        if let Err(detail) = result {
            // Ownership may be half restored; never claim either state.
            return Err(ProtectedLaunchError::CleanupUnknown(format!(
                "custody release incomplete: {detail}"
            )));
        }
        Ok(())
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn controller_launch_for_test(&self) -> Arc<dyn git::ControllerLaunch> {
        let Enforcement::Linux(support) = &self.worker.launcher.inner.enforcement else {
            panic!("enforced launcher required");
        };
        crate::protected_linux::controller_launch(
            support,
            &self.worker.launcher.inner.config,
            &self.worker.binding,
        )
        .unwrap()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerRole {
    Worker,
    Reviewer,
}

/// What a peer must present; Host translation hands these to its peer.
#[derive(Clone, PartialEq, Eq)]
pub struct ControllerCredentials {
    pub attempt_id: String,
    pub task_id: String,
    pub token: String,
}

impl fmt::Debug for ControllerCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControllerCredentials")
            .field("attempt_id", &self.attempt_id)
            .field("task_id", &self.task_id)
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Typed operations; there is no argv, config, ref or path-to-repository
/// field, so a peer cannot select Git behavior.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerOperation {
    Identity,
    Status,
    /// Stage exact worktree-relative paths (A0 structural validation; no
    /// pathspec magic).
    Stage {
        paths: Vec<String>,
    },
    /// Commit staged changes on the task branch with a fixed identity.
    Commit {
        message: String,
    },
    /// Capture a checkpoint of the actual task worktree.
    Checkpoint {
        checkpoint_id: String,
        kind: WorkspaceCheckpointKind,
        captured_at: WorkspaceDateTimeV1,
    },
}

/// One peer request. `token` is a bearer credential: it stays an ordinary
/// public field for the protocol, but `Debug` (ordinary, pretty and nested)
/// never prints it.
#[derive(Clone, PartialEq, Eq)]
pub struct ControllerRequest {
    pub attempt_id: String,
    pub task_id: String,
    pub token: String,
    pub sequence: u64,
    pub operation: ControllerOperation,
}

impl fmt::Debug for ControllerRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControllerRequest")
            .field("attempt_id", &self.attempt_id)
            .field("task_id", &self.task_id)
            .field("token", &"<redacted>")
            .field("sequence", &self.sequence)
            .field("operation", &self.operation)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ControllerResponse {
    Identity {
        attempt_id: String,
        task_id: String,
        workspace_id: String,
        branch: String,
        worktree: PathBuf,
    },
    Status {
        head: CommitSha,
        porcelain: Vec<u8>,
    },
    Staged {
        paths: Vec<String>,
    },
    Committed {
        commit: CommitSha,
    },
    /// The checkpoint observed from the actual worktree and the
    /// `CHECKPOINTED` handle whose verified head is the captured head.
    Checkpointed {
        handle: Box<WorkspaceHandle>,
        checkpoint: Box<WorkspaceCheckpointTemporalCaptureCore>,
    },
}

/// Task-bound typed channel. Requests are serialized; each must present the
/// channel's attempt, task, token and exactly the next sequence number. The
/// token is only reachable through [`ControllerChannel::credentials`];
/// `Debug` never prints it.
pub struct ControllerChannel {
    binding: Arc<Binding>,
    role: ControllerRole,
    token: String,
    next_sequence: Mutex<u64>,
    launcher: ProtectedLauncher,
}

impl fmt::Debug for ControllerChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ControllerChannel")
            .field("attempt_id", &self.binding.attempt_id)
            .field("task_id", &self.binding.task_id)
            .field("role", &self.role)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl ControllerChannel {
    fn new(
        binding: Arc<Binding>,
        role: ControllerRole,
        launcher: ProtectedLauncher,
    ) -> Result<Self, ProtectedLaunchError> {
        Ok(Self {
            binding,
            role,
            token: random_hex(32).map_err(|error| {
                ProtectedLaunchError::Unsupported(format!("randomness: {error}"))
            })?,
            next_sequence: Mutex::new(0),
            launcher,
        })
    }

    pub fn role(&self) -> ControllerRole {
        self.role
    }

    pub fn credentials(&self) -> ControllerCredentials {
        ControllerCredentials {
            attempt_id: self.binding.attempt_id.clone(),
            task_id: self.binding.task_id.clone(),
            token: self.token.clone(),
        }
    }

    /// Verifies binding/replay/role, then performs the fixed operation as the
    /// confined controller identity.
    pub fn handle(
        &self,
        request: ControllerRequest,
    ) -> Result<ControllerResponse, ProtectedLaunchError> {
        let mut next = self
            .next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.binding.released.load(Ordering::SeqCst) {
            return Err(ProtectedLaunchError::Binding("custody was released"));
        }
        if request.task_id != self.binding.task_id {
            return Err(ProtectedLaunchError::Binding("cross-task request"));
        }
        if request.attempt_id != self.binding.attempt_id
            || !constant_time_eq(request.token.as_bytes(), self.token.as_bytes())
        {
            return Err(ProtectedLaunchError::Binding("forged request"));
        }
        if request.sequence != *next {
            return Err(ProtectedLaunchError::Binding(
                "replayed or out-of-order request",
            ));
        }
        let query_only = matches!(
            request.operation,
            ControllerOperation::Identity | ControllerOperation::Status
        );
        if self.role == ControllerRole::Reviewer && !query_only {
            return Err(ProtectedLaunchError::Binding(
                "reviewer channel is query-only",
            ));
        }
        // The sequence is consumed even if the operation fails: no replay.
        *next += 1;
        let _confined = self.confine()?;
        self.perform(request.operation)
    }

    fn confine(&self) -> Result<Option<git::ControllerScope>, ProtectedLaunchError> {
        match &self.launcher.inner.enforcement {
            #[cfg(target_os = "linux")]
            Enforcement::Linux(support) => crate::protected_linux::controller_launch(
                support,
                &self.launcher.inner.config,
                &self.binding,
            )
            .map(|launch| Some(git::ControllerScope::enter(launch)))
            .map_err(ProtectedLaunchError::Unsupported),
            Enforcement::Unenforced => unenforced().map(|()| None),
        }
    }

    fn perform(
        &self,
        operation: ControllerOperation,
    ) -> Result<ControllerResponse, ProtectedLaunchError> {
        use std::ffi::OsStr;
        let binding = &self.binding;
        let worktree = binding.worktree.as_path();
        match operation {
            ControllerOperation::Identity => Ok(ControllerResponse::Identity {
                attempt_id: binding.attempt_id.clone(),
                task_id: binding.task_id.clone(),
                workspace_id: binding.workspace_id.clone(),
                branch: binding.branch.clone(),
                worktree: binding.worktree.clone(),
            }),
            ControllerOperation::Status => {
                let head = self.verified_head()?;
                let status = git::capture(
                    worktree,
                    "controller status",
                    &[
                        OsStr::new("--no-optional-locks"),
                        OsStr::new("status"),
                        OsStr::new("--porcelain=v1"),
                        OsStr::new("-z"),
                        OsStr::new("--untracked-files=all"),
                        OsStr::new("--ignore-submodules=none"),
                    ],
                )?
                .require_success("controller status")?;
                Ok(ControllerResponse::Status {
                    head,
                    porcelain: status.raw_stdout().to_vec(),
                })
            }
            ControllerOperation::Stage { paths } => {
                if paths.is_empty() {
                    return Err(ProtectedLaunchError::Binding("stage requires paths"));
                }
                for path in &paths {
                    validate_write_path(path)
                        .map_err(|_| ProtectedLaunchError::Binding("invalid stage path"))?;
                }
                self.verified_head()?;
                // Worker payload may contain an embedded repository whose own
                // config/attributes name a filter. Once such a gitlink is
                // tracked, a later `status`/`diff` would enter it and run that
                // filter as the controller. Never stage one: remember the
                // index and restore it if Git reports that it added an
                // embedded repository. (Git also refuses to enter repositories
                // owned by another UID, which the distinct worker identity
                // provides; this is defense in depth.)
                let index_before = git::capture(
                    worktree,
                    "controller index snapshot",
                    &[OsStr::new("write-tree")],
                )?
                .require_success("controller index snapshot")?
                .stdout
                .trim()
                .to_owned();
                let mut args = vec![
                    OsStr::new("--literal-pathspecs"),
                    OsStr::new("add"),
                    OsStr::new("--all"),
                    OsStr::new("--"),
                ];
                args.extend(paths.iter().map(OsStr::new));
                let staged = git::capture(worktree, "controller stage", &args)?;
                if staged.stderr.contains("adding embedded git repository") {
                    git::capture(
                        worktree,
                        "controller index restore",
                        &[OsStr::new("read-tree"), OsStr::new(index_before.as_str())],
                    )?
                    .require_success("controller index restore")?;
                    return Err(ProtectedLaunchError::Binding(
                        "embedded repositories cannot be staged",
                    ));
                }
                staged.require_success("controller stage")?;
                Ok(ControllerResponse::Staged { paths })
            }
            ControllerOperation::Commit { message } => {
                if message.trim().is_empty() || message.contains('\0') {
                    return Err(ProtectedLaunchError::Binding("invalid commit message"));
                }
                self.verified_head()?;
                let author = format!("user.name=receipts-controller {}", binding.task_id);
                git::capture(
                    worktree,
                    "controller commit",
                    &[
                        OsStr::new("-c"),
                        OsStr::new(&author),
                        OsStr::new("-c"),
                        OsStr::new("user.email=controller@receipts.invalid"),
                        OsStr::new("commit"),
                        OsStr::new("--no-verify"),
                        OsStr::new("--no-edit"),
                        OsStr::new("--cleanup=verbatim"),
                        OsStr::new("-m"),
                        OsStr::new(&message),
                    ],
                )?
                .require_success("controller commit")?;
                Ok(ControllerResponse::Committed {
                    commit: self.verified_head()?,
                })
            }
            ControllerOperation::Checkpoint {
                checkpoint_id,
                kind,
                captured_at,
            } => {
                self.verified_head()?;
                let active = binding
                    .handle
                    .transitioned(WorkspaceState::Active, None)
                    .map_err(ProtectedLaunchError::Workspace)?;
                let (checkpointed, checkpoint) = crate::checkpoint_workspace(
                    &active,
                    WorkspaceCheckpointLifecycleInput {
                        checkpoint_id,
                        attempt_id: Some(binding.attempt_id.clone()),
                        kind,
                        dirty_diff_ref: None,
                        executed_checks: Vec::new(),
                        captured_at,
                    },
                )
                .map_err(ProtectedLaunchError::Lifecycle)?;
                Ok(ControllerResponse::Checkpointed {
                    handle: Box::new(checkpointed),
                    checkpoint: Box::new(checkpoint),
                })
            }
        }
    }

    /// Same repository and task branch throughout: HEAD must still be the
    /// task branch of the admitted worktree.
    fn verified_head(&self) -> Result<CommitSha, ProtectedLaunchError> {
        use std::ffi::OsStr;
        let worktree = self.binding.worktree.as_path();
        let branch = git::capture(
            worktree,
            "controller branch",
            &[
                OsStr::new("symbolic-ref"),
                OsStr::new("--quiet"),
                OsStr::new("HEAD"),
            ],
        )?;
        if !branch.success || branch.stdout.trim() != format!("refs/heads/{}", self.binding.branch)
        {
            return Err(ProtectedLaunchError::Custody("task branch changed".into()));
        }
        if git_path(worktree, "--git-common-dir")? != self.binding.common_dir {
            return Err(ProtectedLaunchError::Custody("repository changed".into()));
        }
        let head = git::capture(
            worktree,
            "controller head",
            &[OsStr::new("rev-parse"), OsStr::new("HEAD")],
        )?
        .require_success("controller head")?;
        CommitSha::parse(head.stdout.trim()).map_err(ProtectedLaunchError::Workspace)
    }
}

/// Ownership and root mode recorded before custody transfer.
#[cfg(unix)]
fn original_custody(
    common_dir: &Path,
    worktree: &Path,
) -> Result<OriginalCustody, ProtectedLaunchError> {
    use std::os::unix::fs::MetadataExt;
    let meta = |path: &Path| {
        std::fs::symlink_metadata(path)
            .map_err(|error| ProtectedLaunchError::Custody(format!("{}: {error}", path.display())))
    };
    let metadata = meta(common_dir)?;
    let root = meta(worktree)?;
    Ok(OriginalCustody {
        uid: metadata.uid(),
        gid: metadata.gid(),
        worktree_mode: root.mode() & 0o7777,
    })
}

#[cfg(not(unix))]
fn original_custody(_: &Path, _: &Path) -> Result<OriginalCustody, ProtectedLaunchError> {
    Err(ProtectedLaunchError::Unsupported(
        "custody requires Unix ownership".into(),
    ))
}

fn git_path(directory: &Path, flag: &str) -> Result<PathBuf, ProtectedLaunchError> {
    let output = git::capture(
        directory,
        "resolve git path",
        &[
            std::ffi::OsStr::new("rev-parse"),
            std::ffi::OsStr::new("--path-format=absolute"),
            std::ffi::OsStr::new(flag),
        ],
    )?
    .require_success("resolve git path")?;
    std::fs::canonicalize(output.stdout.trim_end_matches('\n'))
        .map_err(|error| ProtectedLaunchError::Custody(format!("git path: {error}")))
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_hex(bytes: usize) -> std::io::Result<String> {
    use std::io::Read;
    let mut buffer = vec![0u8; bytes];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut buffer)?;
    Ok(buffer.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Whether any live process runs with `uid` (Linux `/proc` scan). Unknown
/// states refuse the identity rather than assuming it is free.
fn uid_in_use(uid: u32) -> Result<bool, ProtectedLaunchError> {
    #[cfg(target_os = "linux")]
    {
        crate::protected_linux::uid_in_use(uid).map_err(ProtectedLaunchError::Unsupported)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = uid;
        Ok(false)
    }
}

/// Custody verification over filesystem metadata (no following of links).
/// Ancestors: owned by root or the controller and not group/other writable
/// unless sticky. Metadata tree: controller-owned, no group/other write.
/// Worktree root: controller-owned, worker group, exactly `01770`; its
/// `.git` pointer controller-owned without group/other write.
#[cfg(unix)]
pub(crate) fn verify_custody(
    common_dir: &Path,
    worktree: &Path,
    controller_uid: u32,
    worker_gid: u32,
) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let meta = |path: &Path| {
        std::fs::symlink_metadata(path).map_err(|error| format!("{}: {error}", path.display()))
    };
    for top in [common_dir, worktree] {
        for ancestor in top.ancestors().skip(1) {
            let metadata = meta(ancestor)?;
            let trusted = metadata.uid() == 0 || metadata.uid() == controller_uid;
            let mode = metadata.mode();
            if !trusted || (mode & 0o022 != 0 && mode & 0o1000 == 0) {
                return Err(format!("ancestor {} is not protected", ancestor.display()));
            }
        }
    }
    let mut pending = vec![common_dir.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = meta(&path)?;
        if metadata.uid() != controller_uid || metadata.mode() & 0o022 != 0 {
            return Err(format!(
                "metadata {} is not controller custody",
                path.display()
            ));
        }
        if metadata.is_dir() {
            for entry in std::fs::read_dir(&path).map_err(|error| error.to_string())? {
                pending.push(entry.map_err(|error| error.to_string())?.path());
            }
        }
    }
    let root = meta(worktree)?;
    if root.uid() != controller_uid || root.gid() != worker_gid || root.mode() & 0o7777 != 0o1770 {
        return Err("worktree root must be controller:worker 01770".into());
    }
    let pointer = meta(&worktree.join(".git"))?;
    if !pointer.is_file() || pointer.uid() != controller_uid || pointer.mode() & 0o022 != 0 {
        return Err("worktree .git pointer is not controller custody".into());
    }
    Ok(())
}

/// x86_64 syscall numbers refused with `EPERM` by the qualified filter.
pub(crate) const SECCOMP_DENIED_X86_64: [u32; 16] = [
    304, // open_by_handle_at
    308, // setns
    272, // unshare
    165, // mount
    166, // umount2
    155, // pivot_root
    161, // chroot
    428, // open_tree
    429, // move_mount
    430, // fsopen
    432, // fsmount
    442, // mount_setattr
    101, // ptrace
    310, // process_vm_readv
    311, // process_vm_writev
    438, // pidfd_getfd
];
const SYS_CLONE: u32 = 56;
const SYS_CLONE3: u32 = 435;
/// CLONE_NEWNS|NEWCGROUP|NEWUTS|NEWIPC|NEWUSER|NEWPID|NEWNET|NEWTIME.
pub(crate) const CLONE_NAMESPACE_FLAGS: u32 = 0x7E02_0080;
const AUDIT_ARCH_X86_64: u32 = 0xC000_003E;
const X32_SYSCALL_BIT: u32 = 0x4000_0000;
const RET_KILL_PROCESS: u32 = 0x8000_0000;
const RET_ERRNO: u32 = 0x0005_0000;
const RET_ALLOW: u32 = 0x7FFF_0000;
const EPERM: u32 = 1;
const ENOSYS: u32 = 38;

/// One classic-BPF instruction (`struct sock_filter`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SockFilter {
    pub(crate) code: u16,
    pub(crate) jt: u8,
    pub(crate) jf: u8,
    pub(crate) k: u32,
}

const fn stmt(code: u16, k: u32) -> SockFilter {
    SockFilter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}
const fn jump(code: u16, k: u32, jt: u8, jf: u8) -> SockFilter {
    SockFilter { code, jt, jf, k }
}
const LD_W_ABS: u16 = 0x20;
const JEQ_K: u16 = 0x15;
const JGE_K: u16 = 0x35;
const JSET_K: u16 = 0x45;
const RET_K: u16 = 0x06;

/// The qualified x86_64 filter: wrong architecture or x32 ABI kills; denied
/// calls and namespace-creating `clone` return `EPERM`; `clone3` returns
/// `ENOSYS` (its flags are behind a pointer, so libc falls back to `clone`);
/// everything else is allowed.
pub(crate) fn seccomp_program_x86_64() -> Vec<SockFilter> {
    let mut program = vec![
        stmt(LD_W_ABS, 4),
        jump(JEQ_K, AUDIT_ARCH_X86_64, 1, 0),
        stmt(RET_K, RET_KILL_PROCESS),
        stmt(LD_W_ABS, 0),
        jump(JGE_K, X32_SYSCALL_BIT, 0, 1),
        stmt(RET_K, RET_KILL_PROCESS),
    ];
    for number in SECCOMP_DENIED_X86_64 {
        program.push(jump(JEQ_K, number, 0, 1));
        program.push(stmt(RET_K, RET_ERRNO | EPERM));
    }
    program.push(jump(JEQ_K, SYS_CLONE3, 0, 1));
    program.push(stmt(RET_K, RET_ERRNO | ENOSYS));
    program.push(jump(JEQ_K, SYS_CLONE, 0, 3));
    // args[0] low 32 bits (little-endian) hold the clone flags.
    program.push(stmt(LD_W_ABS, 16));
    program.push(jump(JSET_K, CLONE_NAMESPACE_FLAGS, 0, 1));
    program.push(stmt(RET_K, RET_ERRNO | EPERM));
    program.push(stmt(RET_K, RET_ALLOW));
    program
}

/// Landlock ABI 3 filesystem rights handled by the controller ruleset.
pub(crate) const LANDLOCK_ACCESS_FS_ABI3: u64 = (1 << 15) - 1;
/// Read + execute + directory listing.
pub(crate) const LANDLOCK_READ_EXEC: u64 = 1 | (1 << 2) | (1 << 3);
/// Read and write an existing file (for `/dev/null`).
pub(crate) const LANDLOCK_READ_WRITE_FILE: u64 = (1 << 1) | (1 << 2);

/// The controller's Landlock plan: full rights beneath the repository and
/// worktree, read/exec beneath tool trees, read of a few named files and
/// read/write of `/dev/null`. Missing optional paths are skipped.
pub(crate) fn controller_landlock_plan(binding: &Binding) -> Vec<(PathBuf, u64)> {
    let mut plan = vec![
        (binding.common_dir.clone(), LANDLOCK_ACCESS_FS_ABI3),
        (binding.worktree.clone(), LANDLOCK_ACCESS_FS_ABI3),
        (PathBuf::from("/dev/null"), LANDLOCK_READ_WRITE_FILE),
    ];
    for tree in ["/usr", "/lib", "/lib64", "/bin"] {
        plan.push((PathBuf::from(tree), LANDLOCK_READ_EXEC));
    }
    for file in [
        "/etc/ld.so.cache",
        "/etc/localtime",
        "/etc/nsswitch.conf",
        "/etc/passwd",
        "/etc/group",
    ] {
        plan.push((PathBuf::from(file), 1 << 2));
    }
    plan
}

#[cfg(test)]
#[path = "protected_tests.rs"]
mod tests;
