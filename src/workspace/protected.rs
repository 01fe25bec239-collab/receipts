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
//! * **One owner per resource.** Admission first reserves the actual Git
//!   common directory (see `protected_custody`): a second admission of the
//!   same worktree, of another linked worktree of the same repository or from
//!   another launcher or process refuses before any ownership or mode change.
//!   The reservation and its attributable state marker survive crashes and
//!   failed setup or release; only a verified complete release deletes them.
//!   Unrelated repositories proceed. Limit: one protected attempt per
//!   repository at a time. This coordinates admitted trusted controllers; it is
//!   not protection against root or controller compromise.
//! * **Release barrier.** Both channels share one lifecycle gate. Releasing
//!   custody closes admission of new operations, waits for every admitted
//!   operation (and its Git children, which each operation reaps) to finish,
//!   and only then restores ownership. Requests during or after release
//!   refuse before execution; unknown cleanup quarantines custody.
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::execution::{
    ExecutionError, LiveProcessAttempt, LiveProcessAttemptError, LiveProcessCancelAcceptance,
    LiveProcessOutcome, LiveProcessOutput, LiveProcessStartError, LiveProcessTerminalCause,
    ProcessRunRequest, ProcessTimeoutPolicy,
};
use crate::protected_custody as custody;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Open,
    Releasing,
    Released,
    Incomplete,
}

#[derive(Debug)]
struct GateState {
    phase: Phase,
    active: usize,
}

/// Shared lifecycle barrier of one admission: every controller operation on
/// either channel holds an [`Operation`] for its whole duration; releasing
/// custody closes admission and waits until none remain. No lock is held
/// while an operation or the restoration runs.
#[derive(Debug)]
pub(crate) struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
    /// A confined controller run whose cleanup was not proven.
    controller_unproven: AtomicBool,
    /// Next controller cgroup index of this admission, shared by both
    /// channels and every controller launch (see `create_controller_cgroup`).
    controller_cgroups: AtomicU64,
}

/// An admitted controller operation; dropping it ends the operation.
pub(crate) struct Operation<'a>(&'a Gate);

impl Drop for Operation<'_> {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        state.active -= 1;
        if state.active == 0 {
            self.0.changed.notify_all();
        }
    }
}

impl Gate {
    fn new() -> Self {
        Self {
            state: Mutex::new(GateState {
                phase: Phase::Open,
                active: 0,
            }),
            changed: Condvar::new(),
            controller_unproven: AtomicBool::new(false),
            controller_cgroups: AtomicU64::new(0),
        }
    }

    /// Creates the next controller cgroup `receipts-<attempt>-ctl-<index>`
    /// under `root`. The index comes from this admission-wide counter, so
    /// overlapping Git calls on either channel never choose the same name.
    /// An index is consumed even when creation fails and is never reused;
    /// exhaustion refuses instead of wrapping. An existing directory refuses:
    /// it is never adopted or cleaned up here.
    pub(crate) fn create_controller_cgroup(
        &self,
        root: &Path,
        attempt_id: &str,
    ) -> Result<PathBuf, String> {
        let index = self
            .controller_cgroups
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |index| {
                index.checked_add(1)
            })
            .map_err(|_| "controller cgroup identifiers exhausted".to_owned())?;
        let name = format!("receipts-{attempt_id}-ctl-{index}");
        let dir = root.join(&name);
        std::fs::create_dir(&dir).map_err(|error| format!("create cgroup {name}: {error}"))?;
        Ok(dir)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Admits one operation, or refuses once release began.
    fn enter(&self) -> Result<Operation<'_>, ProtectedLaunchError> {
        let mut state = self.lock();
        match state.phase {
            Phase::Open => {
                state.active += 1;
                Ok(Operation(self))
            }
            Phase::Releasing => Err(ProtectedLaunchError::Binding("custody release in progress")),
            Phase::Released => Err(ProtectedLaunchError::Binding("custody was released")),
            Phase::Incomplete => Err(ProtectedLaunchError::Binding("custody release incomplete")),
        }
    }

    /// Atomically closes admission, then waits for every admitted operation.
    fn close_and_drain(&self) -> Result<(), ProtectedLaunchError> {
        let mut state = self.lock();
        match state.phase {
            Phase::Open => {
                state.phase = Phase::Releasing;
                self.changed.notify_all();
            }
            Phase::Releasing => {
                return Err(ProtectedLaunchError::Custody(
                    "custody release already in progress".into(),
                ));
            }
            Phase::Released => {
                return Err(ProtectedLaunchError::Custody(
                    "custody was already released".into(),
                ));
            }
            Phase::Incomplete => {
                return Err(ProtectedLaunchError::CleanupUnknown(
                    "custody release was incomplete earlier; custody retained".into(),
                ));
            }
        }
        while state.active > 0 {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        Ok(())
    }

    fn settle(&self, released: bool) {
        self.lock().phase = if released {
            Phase::Released
        } else {
            Phase::Incomplete
        };
        self.changed.notify_all();
    }

    pub(crate) fn mark_controller_unproven(&self) {
        self.controller_unproven.store(true, Ordering::SeqCst);
    }

    fn controller_unproven(&self) -> bool {
        self.controller_unproven.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn phase_for_test(&self) -> Phase {
        self.lock().phase
    }

    /// Blocks until `phase` is reached (condition-based, no sleeping).
    #[cfg(test)]
    pub(crate) fn wait_for_phase_for_test(&self, phase: Phase) {
        let mut state = self.lock();
        while state.phase != phase {
            assert!(
                !(phase == Phase::Releasing
                    && matches!(state.phase, Phase::Released | Phase::Incomplete)),
                "release finished without ever waiting for in-flight operations"
            );
            let (next, timeout) = self
                .changed
                .wait_timeout(state, std::time::Duration::from_secs(30))
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state = next;
            assert!(!timeout.timed_out(), "phase {phase:?} never reached");
        }
    }
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

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn cap_last_cap_for_test(&self) -> u32 {
        match &self.inner.enforcement {
            #[cfg(target_arch = "x86_64")]
            Enforcement::Linux(support) => support.cap_last_cap,
            #[cfg(not(target_arch = "x86_64"))]
            Enforcement::Linux(_) => panic!("requires an admitted Linux architecture"),
            Enforcement::Unenforced => panic!("requires admitted Linux support"),
        }
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
        git::refuse_untrusted_repository_config(&worktree, "inspect target configuration")?;
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

        let attempt_id = random_hex(16)
            .map_err(|error| ProtectedLaunchError::Unsupported(format!("randomness: {error}")))?;

        // One protected custody owner per actual Git common directory: reserve
        // it before reading ownership or changing anything. A loser mutates
        // nothing; a refused setup below deletes only its own reservation.
        // The test registry is per contender thread, and each reservation
        // keeps its registry alive (it outlives the thread that admitted it).
        #[cfg(test)]
        let (reserve, test_registry) = match self.inner.enforcement {
            Enforcement::Unenforced => {
                thread_local! {
                    static REGISTRY: Arc<crate::test_support::TempDir> = {
                        let dir = crate::test_support::TempDir::new("custody-registry");
                        std::fs::write(dir.path().join("lock"), "").unwrap();
                        Arc::new(dir)
                    };
                }
                custody_event_for_test("pre-reservation checks passed".into());
                let registry = REGISTRY.with(Arc::clone);
                custody_event_for_test(format!("registry {}", registry.path().display()));
                let reserve = custody::Reservation::begin_for_test(
                    registry.path(),
                    &common_dir,
                    &attempt_id,
                    &worktree,
                );
                (
                    reserve,
                    Some(registry as Arc<dyn std::any::Any + Send + Sync>),
                )
            }
            #[cfg(target_os = "linux")]
            Enforcement::Linux(_) => (
                custody::Reservation::begin(&common_dir, &attempt_id, &worktree),
                None,
            ),
        };
        #[cfg(not(test))]
        let reserve = custody::Reservation::begin(&common_dir, &attempt_id, &worktree);
        let reservation = reserve.map_err(ProtectedLaunchError::Custody)?;
        #[cfg(test)]
        custody_event_for_test("reserved".into());
        let cancel = |error: ProtectedLaunchError| {
            let _ = reservation.finish();
            error
        };
        let original =
            original_custody(&common_dir, &worktree, &self.inner.config).map_err(cancel)?;
        let ids = custody::preflight_layout(&common_dir, &worktree)
            .map_err(|detail| cancel(ProtectedLaunchError::Custody(detail)))?;
        let worker_uid = self.allocate_worker_uid().map_err(cancel)?;
        reservation
            .record_identity(worker_uid, original, ids)
            .map_err(|error| {
                cancel(ProtectedLaunchError::Custody(format!(
                    "custody record could not be written: {error}"
                )))
            })?;
        #[cfg(test)]
        custody_event_for_test("identity recorded".into());
        let binding = Binding {
            attempt_id,
            task_id,
            workspace_id: handle.workspace_id().to_owned(),
            branch: handle.branch().to_owned(),
            common_dir,
            worktree,
            worker_uid,
            original,
            gate: Arc::new(Gate::new()),
            reservation,
            handle: handle.clone(),
            #[cfg(test)]
            hooks: TestHooks::default(),
            #[cfg(test)]
            _test_registry: test_registry,
        };
        match &self.inner.enforcement {
            #[cfg(target_os = "linux")]
            Enforcement::Linux(_) => {
                let config = &self.inner.config;
                match crate::protected_linux::establish_custody(config, &binding) {
                    Ok(()) => {}
                    Err(EstablishError::Refused(detail)) => {
                        // Nothing was changed: free only this reservation.
                        self.retain(worker_uid);
                        let _ = binding.reservation.finish();
                        return Err(ProtectedLaunchError::Custody(detail));
                    }
                    Err(EstablishError::Incomplete(detail)) => {
                        // Ownership may be half transferred: keep the
                        // reservation and its record; nothing looks free.
                        self.retain(worker_uid);
                        let _ = binding.reservation.set_phase(custody::Phase::Incomplete);
                        return Err(ProtectedLaunchError::Custody(format!(
                            "custody transfer incomplete; reservation retained: {detail}"
                        )));
                    }
                }
            }
            Enforcement::Unenforced => {
                if let Err(error) = unenforced() {
                    let _ = binding.reservation.finish();
                    return Err(error);
                }
                binding
                    .reservation
                    .set_phase(custody::Phase::Custody)
                    .map_err(|error| {
                        let _ = binding.reservation.finish();
                        ProtectedLaunchError::Custody(format!("custody record: {error}"))
                    })?;
            }
        }
        #[cfg(test)]
        custody_event_for_test("custody".into());
        Ok(ProtectedAdmission {
            binding: Arc::new(binding),
            launcher: self.clone(),
            consumed: false,
        })
    }

    /// Restores ownership under the shared release barrier's result. Unknown
    /// outcomes (including an unproven controller cleanup) quarantine custody:
    /// the reservation record stays and every channel refuses.
    fn settle_release(&self, binding: &Binding) -> Result<(), ProtectedLaunchError> {
        let outcome = if binding.gate.controller_unproven() {
            Err("a confined controller operation's cleanup was not proven".to_owned())
        } else {
            self.restore(binding)
        };
        match outcome {
            Ok(()) => {
                binding.gate.settle(true);
                Ok(())
            }
            Err(detail) => {
                let _ = binding.reservation.set_phase(custody::Phase::Incomplete);
                binding.gate.settle(false);
                // Ownership may be half restored; never claim either state.
                Err(ProtectedLaunchError::CleanupUnknown(format!(
                    "custody release incomplete: {detail}"
                )))
            }
        }
    }

    fn restore(&self, binding: &Binding) -> Result<(), String> {
        binding
            .reservation
            .set_phase(custody::Phase::Releasing)
            .map_err(|error| format!("custody record: {error}"))?;
        binding.reservation.verify_original(binding.original)?;
        #[cfg(test)]
        if binding.hooks.fail_restore.load(Ordering::SeqCst) {
            return Err("injected restoration failure".into());
        }
        match &self.inner.enforcement {
            #[cfg(target_os = "linux")]
            Enforcement::Linux(_) => crate::protected_linux::release_custody(binding)?,
            Enforcement::Unenforced => unenforced().map_err(|error| error.to_string())?,
        }
        binding.reservation.finish()
    }

    /// An admission that never reached a started worker gives custody back
    /// (nothing ran under the worker identity); failure keeps the quarantine.
    fn abandon(&self, binding: &Binding) -> Result<(), ProtectedLaunchError> {
        binding.gate.close_and_drain()?;
        self.settle_release(binding)
    }

    /// Setup failed after a worker may exist: keep custody and the record.
    fn retain_incomplete(&self, binding: &Binding) {
        let _ = binding.reservation.set_phase(custody::Phase::Incomplete);
        binding.gate.settle(false);
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
        mut admission: ProtectedAdmission,
        request: &ProcessRunRequest,
        policy: &ProcessTimeoutPolicy,
    ) -> Result<ProtectedAttempt, ProtectedLaunchError> {
        admission.consumed = true;
        let binding = admission.binding.clone();
        // The admission's own launcher established custody, so it restores it.
        let owner = &admission.launcher;
        // Refusals before any worker could exist give custody straight back.
        let refuse_unstarted = |error: ProtectedLaunchError| match owner.abandon(&binding) {
            Ok(()) => error,
            Err(release) => ProtectedLaunchError::CleanupUnknown(format!(
                "{error}; custody release also failed: {release}"
            )),
        };
        if !Arc::ptr_eq(&owner.inner, &self.inner) {
            return Err(refuse_unstarted(ProtectedLaunchError::Binding(
                "admission belongs to another launcher",
            )));
        }
        match std::fs::canonicalize(request.workspace_root()) {
            Ok(root) if root == binding.worktree => {}
            _ => {
                return Err(refuse_unstarted(ProtectedLaunchError::Binding(
                    "request workspace root is not the admitted worktree",
                )));
            }
        }
        let customization = match &self.inner.enforcement {
            #[cfg(target_os = "linux")]
            Enforcement::Linux(support) => {
                crate::protected_linux::worker_launch(support, &self.inner.config, &binding)
                    .map_err(|detail| refuse_unstarted(ProtectedLaunchError::Unsupported(detail)))?
            }
            Enforcement::Unenforced => {
                unenforced().map_err(refuse_unstarted)?;
                Default::default()
            }
        };
        // From here a worker may exist: failures keep custody and its record.
        let attempt =
            crate::execution::live_attempt_start_customized(request, policy, customization)
                .map_err(|error| {
                    self.retain_incomplete(&binding);
                    ProtectedLaunchError::Start(error)
                })?;
        let channels = binding
            .handle
            .transitioned(WorkspaceState::Active, None)
            .map_err(ProtectedLaunchError::Workspace)
            .and_then(|active| {
                let worker =
                    ControllerChannel::new(binding.clone(), ControllerRole::Worker, self.clone())?;
                let reviewer = ControllerChannel::new(
                    binding.clone(),
                    ControllerRole::Reviewer,
                    self.clone(),
                )?;
                Ok((active, worker, reviewer))
            })
            .inspect_err(|_| self.retain_incomplete(&binding))?;
        let (active, worker, reviewer) = channels;
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
    /// Set once `launch` took the admission.
    consumed: bool,
}

impl Drop for ProtectedAdmission {
    /// An admission dropped without a launch never ran a worker: give custody
    /// back. If that cannot be proven the reservation record stays.
    fn drop(&mut self) {
        if !self.consumed {
            let _ = self.launcher.abandon(&self.binding);
        }
    }
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
/// transferred; restored by [`ProtectedAttempt::release_custody`] and
/// persisted once in the custody reservation record.
pub(crate) type OriginalCustody = custody::OriginalOwner;

/// Why custody transfer stopped.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug)]
pub(crate) enum EstablishError {
    /// Refused before the first ownership or mode change.
    Refused(String),
    /// Changes began; ownership may be half transferred.
    Incomplete(String),
}

/// Test-only coordination points inside controller operations and release.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct TestHooks {
    pub(crate) before: Mutex<Option<ChannelHook>>,
    pub(crate) after: Mutex<Option<ChannelHook>>,
    pub(crate) fail_restore: AtomicBool,
    /// When set, unenforced controller operations take their per-call cgroup
    /// names from the production allocator, see [`CgroupNameSeam`].
    pub(crate) controller_cgroups: Mutex<Option<CgroupNameSeam>>,
}

#[cfg(test)]
pub(crate) type ChannelHook = Arc<dyn Fn(ControllerRole) + Send + Sync>;

/// Unprivileged filesystem stand-in for the delegated cgroup root. It proves
/// only controller cgroup NAME reservation through [`Gate::create_controller_cgroup`]
/// and the real confined-run path; no kernel cgroup or confinement exists.
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct CgroupNameSeam {
    pub(crate) root: PathBuf,
    /// Runs after each successful reservation, while its directory exists.
    pub(crate) reserved: ReservedHook,
}

#[cfg(test)]
pub(crate) type ReservedHook = Arc<dyn Fn(ControllerRole, &Path) + Send + Sync>;

#[cfg(test)]
type CustodyHook = Box<dyn Fn(&str)>;

#[cfg(test)]
struct SeamController {
    binding: Arc<Binding>,
    role: ControllerRole,
    seam: CgroupNameSeam,
}

#[cfg(test)]
struct SeamCgroup(PathBuf);

#[cfg(test)]
impl crate::execution::AttemptContainment for SeamCgroup {
    fn kill_and_verify_empty(&self) -> Result<(), String> {
        std::fs::remove_dir(&self.0).map_err(|error| format!("{}: {error}", self.0.display()))
    }
}

#[cfg(test)]
impl git::ControllerLaunch for SeamController {
    fn customization(&self) -> Result<crate::execution::LaunchCustomization, String> {
        let dir = self
            .binding
            .gate
            .create_controller_cgroup(&self.seam.root, &self.binding.attempt_id)?;
        (self.seam.reserved)(self.role, &dir);
        Ok(crate::execution::LaunchCustomization {
            pre_exec: None,
            containment: Some(Arc::new(SeamCgroup(dir))),
        })
    }
    fn cleanup_unproven(&self) {
        self.binding.gate.mark_controller_unproven();
    }
}

#[cfg(test)]
thread_local! {
    static CUSTODY_TRACE: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
    static CUSTODY_HOOK: std::cell::RefCell<Option<CustodyHook>> = const { std::cell::RefCell::new(None) };
}

/// Records one admission/reservation phase event of this thread and runs
/// this thread's hook, if any, at that point.
#[cfg(test)]
pub(crate) fn custody_event_for_test(event: String) {
    CUSTODY_TRACE.with(|trace| trace.borrow_mut().push(event.clone()));
    CUSTODY_HOOK.with(|hook| {
        if let Some(hook) = hook.borrow().as_ref() {
            hook(&event);
        }
    });
}

#[cfg(test)]
pub(crate) fn set_custody_hook_for_test(hook: impl Fn(&str) + 'static) {
    CUSTODY_HOOK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
pub(crate) fn take_custody_trace_for_test() -> Vec<String> {
    CUSTODY_TRACE.with(|trace| std::mem::take(&mut *trace.borrow_mut()))
}

#[cfg(test)]
impl fmt::Debug for TestHooks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TestHooks")
    }
}

#[cfg(test)]
impl TestHooks {
    fn fire(slot: &Mutex<Option<ChannelHook>>, role: ControllerRole) {
        let hook = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if let Some(hook) = hook {
            hook(role);
        }
    }
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
    /// Shared lifecycle barrier of both channels and the release.
    pub(crate) gate: Arc<Gate>,
    /// Exclusive reservation of the Git common directory and its record.
    pub(crate) reservation: custody::Reservation,
    handle: WorkspaceHandle,
    #[cfg(test)]
    pub(crate) hooks: TestHooks,
    /// Keeps this admission's test registry alive while its reservation is.
    #[cfg(test)]
    _test_registry: Option<Arc<dyn std::any::Any + Send + Sync>>,
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
        // Close admission on BOTH channels and drain every admitted operation
        // (each reaps its Git children) before ownership moves.
        binding.gate.close_and_drain()?;
        self.worker.launcher.settle_release(binding)
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
        // Admitted for the whole operation; refuses once release began.
        let _operation = self.binding.gate.enter()?;
        let mut next = self
            .next_sequence
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        #[cfg(test)]
        TestHooks::fire(&self.binding.hooks.before, self.role);
        let result = self
            .confine()
            .and_then(|_confined| self.perform(request.operation));
        #[cfg(test)]
        TestHooks::fire(&self.binding.hooks.after, self.role);
        result
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
            Enforcement::Unenforced => {
                unenforced()?;
                #[cfg(test)]
                if let Some(seam) = self
                    .binding
                    .hooks
                    .controller_cgroups
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
                {
                    return Ok(Some(git::ControllerScope::enter(Arc::new(
                        SeamController {
                            binding: self.binding.clone(),
                            role: self.role,
                            seam,
                        },
                    ))));
                }
                Ok(None)
            }
        }
    }

    fn perform(
        &self,
        operation: ControllerOperation,
    ) -> Result<ControllerResponse, ProtectedLaunchError> {
        use std::ffi::OsStr;
        let binding = &self.binding;
        let worktree = binding.worktree.as_path();
        // Custody keeps the actual target's metadata protected through every
        // child completion; root configuration alone does not cover config.worktree.
        if !matches!(operation, ControllerOperation::Identity) {
            git::refuse_untrusted_repository_config(
                worktree,
                "inspect controller target configuration",
            )?;
        }
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

/// Ownership and root mode recorded before custody transfer. An owner that
/// already looks like controller or worker custody is prior, unknown custody
/// and refuses (there is no automatic recovery).
#[cfg(unix)]
fn original_custody(
    common_dir: &Path,
    worktree: &Path,
    config: &ProtectedLaunchConfig,
) -> Result<OriginalCustody, ProtectedLaunchError> {
    use std::os::unix::fs::MetadataExt;
    let meta = |path: &Path| {
        std::fs::symlink_metadata(path)
            .map_err(|error| ProtectedLaunchError::Custody(format!("{}: {error}", path.display())))
    };
    let metadata = meta(common_dir)?;
    let root = meta(worktree)?;
    let looks_held = |uid: u32, gid: u32| {
        uid == config.controller_uid
            || config.worker_uids.contains(&uid)
            || gid == config.worker_gid
    };
    if looks_held(metadata.uid(), metadata.gid()) || looks_held(root.uid(), root.gid()) {
        return Err(ProtectedLaunchError::Custody(
            "repository ownership already matches controller or worker custody \
             (unknown prior custody); refusing without recovery"
                .into(),
        ));
    }
    Ok(OriginalCustody {
        uid: metadata.uid(),
        gid: metadata.gid(),
        worktree_mode: root.mode() & 0o7777,
    })
}

#[cfg(not(unix))]
fn original_custody(
    _: &Path,
    _: &Path,
    _: &ProtectedLaunchConfig,
) -> Result<OriginalCustody, ProtectedLaunchError> {
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

/// Every ancestor of both custody roots is root- or controller-owned and not
/// group/other writable (unless sticky). Read-only; also run before the first
/// mutation.
#[cfg(unix)]
pub(crate) fn verify_ancestors(
    common_dir: &Path,
    worktree: &Path,
    controller_uid: u32,
) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    for top in [common_dir, worktree] {
        for ancestor in top.ancestors().skip(1) {
            let metadata = std::fs::symlink_metadata(ancestor)
                .map_err(|error| format!("{}: {error}", ancestor.display()))?;
            let trusted = metadata.uid() == 0 || metadata.uid() == controller_uid;
            let mode = metadata.mode();
            if !trusted || (mode & 0o022 != 0 && mode & 0o1000 == 0) {
                return Err(format!("ancestor {} is not protected", ancestor.display()));
            }
        }
    }
    Ok(())
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
    verify_ancestors(common_dir, worktree, controller_uid)?;
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
