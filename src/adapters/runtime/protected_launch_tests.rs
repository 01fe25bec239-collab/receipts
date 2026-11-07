//! Protected-launch integration of BOTH concrete adapters.
//!
//! Portable tests prove ordinary labeling, binding refusal, unsupported-host
//! refusal and the observation helpers below; they never mint admission. The
//! `hosted_*` tests use real public Workspace admission and launch. They are
//! `#[ignore]`d and run ONLY inside the hosted disposable-VM diagnostic
//! (`.github/scripts/ra-w1-completion-linux`) as root, with
//! `RECEIPTS_RA_HOSTED_CGROUP` naming a delegated cgroup v2 directory,
//! `RECEIPTS_RA_HOSTED_WORKER` naming the native worker that `prepare` built
//! unprivileged, and the host custody anchors administered. Never run them
//! with sudo on a workstation. The worker is the synthetic native
//! `protected_launch_worker.rs`: these results are integration controls, never
//! Claude/Codex provider evidence.
use super::*;
use crate::registry::{DynAdapter, DynAttemptEvent, DynAttemptHandle};
use receipts_runtime_bindings::*;
use receipts_workspace_execution::execution::{
    ChildEnvAuthMode, ChildEnvPolicy, ChildEnvProvider, LiveProcessTerminalCause,
    ProcessTimeoutPolicy,
};
use receipts_workspace_execution::{
    ControllerChannel, ControllerOperation, ControllerRequest, ControllerResponse, ControllerRole,
    ProtectedLaunchConfig, ProtectedLaunchError, ProtectedLauncher, WorkspaceHandle,
    WorkspaceProvisionRequest,
};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::{
    ffi::OsString,
    fs,
    future::Future,
    path::{Path, PathBuf},
    pin::pin,
    process::Command,
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

const CONTROLLER_ID: u32 = 61001;
const WORKER_GID: u32 = 61002;
const WORKER_RANGE: std::ops::RangeInclusive<u32> = 62000..=62063;
const FIRST: &[u8] = b"{\"type\":\"future\",\"n\":1}\n";
const SECOND: &[u8] = b"{\"type\":\"future\",\"n\":2}\n";
const WORKER_SOURCE: &str = include_str!("protected_launch_worker.rs");

unsafe extern "C" {
    fn getuid() -> u32;
}

/// The native worker. Hosted: the binary `prepare` built unprivileged (root
/// only copies it). Portable: compiled once here, unprivileged; compiling as
/// root is refused.
fn native_worker() -> &'static Path {
    static WORKER: OnceLock<PathBuf> = OnceLock::new();
    WORKER.get_or_init(|| {
        if let Some(prebuilt) = std::env::var_os("RECEIPTS_RA_HOSTED_WORKER") {
            let prebuilt = PathBuf::from(prebuilt);
            assert!(
                prebuilt.is_absolute(),
                "hosted worker path must be absolute"
            );
            return prebuilt;
        }
        // SAFETY: getuid has no preconditions and cannot fail.
        assert_ne!(unsafe { getuid() }, 0, "never compile the worker as root");
        let dir = std::env::temp_dir().join(format!("receipts-ra-worker-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let source = dir.join("protected_launch_worker.rs");
        fs::write(&source, WORKER_SOURCE).unwrap();
        let binary = dir.join("worker");
        let output = Command::new("rustc")
            .arg("--edition=2024")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "worker compile: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        binary
    })
}

fn launcher() -> ProtectedLauncher {
    let cgroup = std::env::var_os("RECEIPTS_RA_HOSTED_CGROUP").expect("hosted diagnostic only");
    let config = ProtectedLaunchConfig::new(
        CONTROLLER_ID,
        CONTROLLER_ID,
        WORKER_GID,
        WORKER_RANGE,
        cgroup,
    )
    .unwrap();
    ProtectedLauncher::new(config).expect("hosted admission must succeed")
}

pub(crate) struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    pub(crate) workspace: WorkspaceHandle,
    executable: PathBuf,
}
impl Fixture {
    pub(crate) fn new(task: Option<&str>) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ra-w1-native-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let repo = root.join("repo");
        fs::create_dir(&repo).unwrap();
        fs::set_permissions(&repo, fs::Permissions::from_mode(0o755)).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("/usr/bin/git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success(), "fixture git {args:?}");
            String::from_utf8(out.stdout).unwrap()
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["config", "user.name", "fixture"]);
        git(&["config", "user.email", "fixture@receipts.invalid"]);
        fs::write(repo.join("tracked.txt"), "original\n").unwrap();
        git(&["add", "tracked.txt"]);
        git(&["commit", "-q", "-m", "base"]);
        let base = git(&["rev-parse", "HEAD"]);
        let workspace = WorkspaceProvisionRequest::new(
            &repo,
            "ws-runtime-native",
            task,
            "runtime-a3/task",
            root.join("wt"),
            base.trim(),
        )
        .unwrap()
        .provision()
        .unwrap();
        let executable = root.join("worker");
        fs::copy(native_worker(), &executable).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            root,
            repo,
            workspace,
            executable,
        }
    }
    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.workspace.worktree_path().join(name)
    }
    fn read(&self, name: &str) -> Vec<u8> {
        fs::read(self.path(name)).unwrap()
    }
    fn ready(&self) {
        let start = Instant::now();
        while !self.path("ready").exists() {
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "worker never ready"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn release(&self) {
        fs::write(self.path("release"), "").unwrap();
    }
    fn reset(&self) {
        for name in [
            "pid",
            "uid",
            "env-observed",
            "argv",
            "received",
            "ready",
            "release",
        ] {
            let _ = fs::remove_file(self.path(name));
        }
    }
    fn task(&self) -> RuntimeCapsuleFamily {
        self.task_with_objective("objective -- \"quoted\"\n- literal")
    }
    pub(crate) fn task_with_objective(&self, objective: &str) -> RuntimeCapsuleFamily {
        RuntimeCapsuleFamily::Task(
            TaskCapsule::try_new(
                "task".into(),
                "stream".into(),
                None,
                1,
                TaskType::Implementation,
                objective.into(),
                vec![
                    Criterion::try_new(
                        "criterion".into(),
                        "description".into(),
                        CriterionKind::Semantic,
                        None,
                        Some("reason".into()),
                    )
                    .unwrap(),
                ],
                Some(vec!["non goal".into()]),
                self.workspace.base_sha().as_str().into(),
                self.workspace.base_sha().as_str().into(),
                vec!["src/**".into()],
                vec!["private/**".into()],
                None,
                None,
                None,
                None,
                QualityFloor::Frontier,
                vec![],
                None,
                vec![
                    VerificationPlanEntryV1::try_new(vec!["cargo".into(), "test".into()], 0)
                        .unwrap(),
                ],
                None,
                AssuranceProfileSelector::Standard,
                None,
                None,
                None,
                self.workspace.branch().into(),
                self.workspace.worktree_path().to_str().unwrap().into(),
                None,
                vec!["stop on collision".into()],
                None,
                7,
            )
            .unwrap(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn timeout() -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(Duration::from_secs(60), Duration::from_secs(1)).unwrap()
}
fn ordinary(read_only: bool) -> RuntimeExecutionPolicy {
    RuntimeExecutionPolicy::new(timeout(), read_only)
}
fn protected(read_only: bool, launcher: &ProtectedLauncher, repo: &Path) -> RuntimeExecutionPolicy {
    ordinary(read_only).with_protected_launch(ProtectedLaunch::new(launcher.clone(), repo))
}

/// Both concrete adapters, through the registry's erased lifecycle.
fn adapter(codex: bool, exe: &Path, env: Option<ChildEnvPolicy>) -> DynAdapter {
    if codex {
        let a = CodexAdapter::new(exe);
        DynAdapter::new(match env {
            Some(env) => a.with_env_policy(env),
            None => a,
        })
    } else {
        let a = ClaudeAdapter::new(exe);
        DynAdapter::new(match env {
            Some(env) => a.with_env_policy(env),
            None => a,
        })
    }
}
fn env_policy(codex: bool) -> ChildEnvPolicy {
    let provider = if codex {
        ChildEnvProvider::Codex
    } else {
        ChildEnvProvider::Claude
    };
    ChildEnvPolicy::new(provider, ChildEnvAuthMode::Subscription).unwrap()
}
fn start(a: &DynAdapter, f: &Fixture, p: &RuntimeExecutionPolicy) -> DynAttemptHandle {
    a.start(&f.task(), &f.workspace, p)
        .unwrap_or_else(|class| panic!("{} start refused: {class:?}", a.runtime_id()))
}
fn is_protected(h: &DynAttemptHandle) -> bool {
    h.downcast_ref::<ClaudeAttemptHandle>()
        .map(ClaudeAttemptHandle::protected)
        .or_else(|| {
            h.downcast_ref::<CodexAttemptHandle>()
                .map(CodexAttemptHandle::protected)
        })
        .unwrap()
}
fn channel(h: &DynAttemptHandle) -> Option<&ControllerChannel> {
    match h.downcast_ref::<ClaudeAttemptHandle>() {
        Some(h) => h.controller_channel(),
        None => h
            .downcast_ref::<CodexAttemptHandle>()
            .unwrap()
            .controller_channel(),
    }
}
fn release(h: &DynAttemptHandle) -> Option<Result<(), ProtectedLaunchError>> {
    match h.downcast_ref::<ClaudeAttemptHandle>() {
        Some(h) => h.release_custody(),
        None => h
            .downcast_ref::<CodexAttemptHandle>()
            .unwrap()
            .release_custody(),
    }
}
/// (terminal cause, exit code, protected label) of the collected result.
fn collected(
    a: &DynAdapter,
    h: &DynAttemptHandle,
) -> (
    LiveProcessTerminalCause,
    Option<i32>,
    bool,
    Option<FailureClass>,
) {
    let result = a.collect_result(h).unwrap();
    let failure = result.failure();
    if let Some(r) = result.downcast_ref::<Result<ClaudeAttemptResult, FailureClass>>() {
        let r = r
            .as_ref()
            .unwrap_or_else(|c| panic!("claude collect: {c:?}"));
        let p = r.outcome.process();
        return (p.terminal_cause(), p.exit_code(), r.protected(), failure);
    }
    let r = result
        .downcast_ref::<Result<CodexAttemptResult, FailureClass>>()
        .unwrap()
        .as_ref()
        .unwrap_or_else(|c| panic!("codex collect: {c:?}"));
    let p = r.outcome.process();
    (p.terminal_cause(), p.exit_code(), r.protected(), failure)
}

/// Waker that records notifications in a flag under a lock. A wake that lands
/// while the future is still being polled (before it returns Pending) stays
/// recorded, so it is never lost.
#[derive(Default)]
struct Notify {
    woken: Mutex<bool>,
    changed: Condvar,
}
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        *self.woken.lock().unwrap() = true;
        self.changed.notify_all();
    }
}
/// No waker notification arrived within the limit after the last Pending.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NoWakeup {
    pub(crate) polls: u32,
}
/// Repolls ONLY after an observed waker notification; there is no timer
/// repoll. Expiry of `limit` (total) without one is a failure.
pub(crate) fn poll_on_wakeups<T>(
    future: impl Future<Output = T>,
    limit: Duration,
) -> Result<T, NoWakeup> {
    let mut future = pin!(future);
    let notify = Arc::new(Notify::default());
    let waker = Waker::from(notify.clone());
    let mut cx = Context::from_waker(&waker);
    let deadline = Instant::now() + limit;
    let mut polls = 0;
    loop {
        polls += 1;
        if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
            return Ok(result);
        }
        let mut woken = notify.woken.lock().unwrap();
        while !*woken {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(NoWakeup { polls });
            }
            woken = notify.changed.wait_timeout(woken, left).unwrap().0;
        }
        *woken = false;
    }
}
pub(crate) fn block_on<T>(future: impl Future<Output = T>) -> T {
    poll_on_wakeups(future, Duration::from_secs(30))
        .unwrap_or_else(|e| panic!("no wakeup observed within 30 s: {e:?}"))
}
fn record(event: Option<DynAttemptEvent>) -> Vec<u8> {
    match event {
        Some(DynAttemptEvent::Record(bytes)) => bytes,
        Some(DynAttemptEvent::Unavailable(_)) => panic!("event unavailable"),
        None => panic!("stream ended early"),
    }
}
fn request(
    ch: &ControllerChannel,
    sequence: u64,
    operation: ControllerOperation,
) -> ControllerRequest {
    let c = ch.credentials();
    ControllerRequest {
        attempt_id: c.attempt_id,
        task_id: c.task_id,
        token: c.token,
        sequence,
        operation,
    }
}
fn binding_refused(result: Result<ControllerResponse, ProtectedLaunchError>) -> bool {
    matches!(result, Err(ProtectedLaunchError::Binding(_)))
}
/// One procfs observation of a PID. Only `Absent` (the kernel reports no such
/// PID) shows the worker was reaped; a zombie is exited-but-unreaped and any
/// read/parse error, or a host without procfs, is `Unknown`. An observation,
/// not cleanup authority: Workspace collection/cgroup proof and the
/// diagnostic's fixture-identity check remain the cleanup barriers.
#[derive(Debug, PartialEq, Eq)]
enum PidState {
    Absent,
    Zombie,
    Present,
    Unknown(String),
}
fn pid_state(stat: std::io::Result<String>) -> PidState {
    let stat = match stat {
        Ok(stat) => stat,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return PidState::Absent,
        Err(e) => return PidState::Unknown(format!("read: {e}")),
    };
    // The state follows the LAST ')': the command name may contain one.
    match stat
        .rfind(')')
        .and_then(|i| stat[i + 1..].split_whitespace().next())
    {
        Some("Z") => PidState::Zombie,
        Some(s) if s.len() == 1 && "RSDTtWPIK".contains(s) => PidState::Present,
        other => PidState::Unknown(format!("state {other:?}")),
    }
}
fn observe_pid(pid: &str) -> PidState {
    let Ok(pid) = pid.trim().parse::<u32>() else {
        return PidState::Unknown(format!("pid {pid:?}"));
    };
    // Without a readable procfs, NotFound below would not mean absence.
    if let Err(e) = fs::read_to_string("/proc/self/stat") {
        return PidState::Unknown(format!("procfs unavailable: {e}"));
    }
    pid_state(fs::read_to_string(format!("/proc/{pid}/stat")))
}
/// Sorted Debug of the exact (name, value) pairs, as the worker writes them.
fn env_snapshot(mut entries: Vec<(OsString, OsString)>) -> String {
    entries.sort();
    format!("{entries:?}")
}
/// What the child must observe: nothing by default, otherwise exactly the
/// admitted names with this (parent) process's values the policy snapshotted.
fn expected_env(env: Option<&ChildEnvPolicy>) -> String {
    env_snapshot(env.map_or_else(Vec::new, |env| {
        env.names()
            .map(|n| (n.into(), std::env::var_os(n).unwrap()))
            .collect()
    }))
}

#[test]
#[ignore = "privileged hosted diagnostic"]
fn hosted_protected_adapters_collect_events_fidelity_and_channels() {
    let launcher = launcher();
    for codex in [false, true] {
        for read_only in [false, true] {
            let f = Fixture::new(Some("task"));
            // Env policy on the write case, default-empty env on the read-only case.
            let env = (!read_only).then(|| env_policy(codex));
            let a = adapter(codex, &f.executable, env.clone());

            // Ordinary, non-authoritative baseline of the SAME request before custody.
            let h = start(&a, &f, &ordinary(read_only));
            assert!(!is_protected(&h) && channel(&h).is_none() && release(&h).is_none());
            f.ready();
            let baseline: Vec<_> = ["received", "argv", "env-observed"]
                .map(|n| f.read(n))
                .into();
            assert_eq!(f.read("uid"), b"0\n");
            f.release();
            let (cause, code, labeled, _) = collected(&a, &h);
            assert_eq!(
                (cause, code, labeled),
                (LiveProcessTerminalCause::Completed, Some(0), false)
            );
            drop(h);
            f.reset();

            let h = start(&a, &f, &protected(read_only, &launcher, &f.repo));
            assert!(is_protected(&h));
            let mut stream = a.stream_events(&h).unwrap();
            assert_eq!(record(block_on(stream.next_event())), FIRST);
            // Exact stdin, argv and environment equal the ordinary request.
            for (name, expected) in ["received", "argv", "env-observed"].iter().zip(&baseline) {
                assert_eq!(&f.read(name), expected, "{name} differs under protection");
            }
            // Exact initial exec environment, order-independent: empty by
            // default, otherwise exactly the admitted entries.
            let observed = String::from_utf8(f.read("env-observed")).unwrap();
            assert_eq!(
                observed,
                expected_env(env.as_ref()),
                "exec environment differs from the admitted snapshot"
            );
            let uid: u32 = String::from_utf8(f.read("uid"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            assert!(WORKER_RANGE.contains(&uid), "worker ran as {uid}");
            assert_eq!(
                fs::metadata(f.workspace.worktree_path()).unwrap().uid(),
                CONTROLLER_ID
            );

            // Typed channel of the trusted owner; token never reaches the worker.
            let ch = channel(&h).expect("protected channel");
            let role = if read_only {
                ControllerRole::Reviewer
            } else {
                ControllerRole::Worker
            };
            assert_eq!(ch.role(), role);
            let token = ch.credentials().token;
            for name in ["received", "argv", "env-observed"] {
                let bytes = f.read(name);
                assert!(!bytes.windows(token.len()).any(|w| w == token.as_bytes()));
            }
            assert!(!format!("{ch:?}").contains(&token));
            match ch.handle(request(ch, 0, ControllerOperation::Identity)) {
                Ok(ControllerResponse::Identity {
                    task_id, worktree, ..
                }) => {
                    assert_eq!(task_id, "task");
                    assert_eq!(worktree, f.workspace.worktree_path());
                }
                other => panic!("identity: {other:?}"),
            }
            assert!(binding_refused(ch.handle(request(
                ch,
                0,
                ControllerOperation::Identity
            ))));
            let mut forged = request(ch, 1, ControllerOperation::Status);
            forged.token = "0".repeat(token.len());
            assert!(binding_refused(ch.handle(forged)));
            let mut cross = request(ch, 1, ControllerOperation::Status);
            cross.task_id = "other-task".into();
            assert!(binding_refused(ch.handle(cross)));
            assert!(matches!(
                ch.handle(request(ch, 1, ControllerOperation::Status)),
                Ok(ControllerResponse::Status { .. })
            ));
            let stage = ControllerOperation::Stage {
                paths: vec!["tracked.txt".into()],
            };
            let staged = ch.handle(request(ch, 2, stage));
            if read_only {
                assert!(binding_refused(staged), "reviewer mutation must refuse");
            } else {
                assert!(matches!(staged, Ok(ControllerResponse::Staged { .. })));
            }

            // Release before collection is refused and keeps custody.
            assert!(matches!(
                release(&h),
                Some(Err(ProtectedLaunchError::CleanupUnknown(_)))
            ));
            f.release();
            assert_eq!(record(block_on(stream.next_event())), SECOND);
            assert!(block_on(stream.next_event()).is_none());
            drop(stream);
            let (cause, code, labeled, _) = collected(&a, &h);
            assert_eq!(
                (cause, code, labeled),
                (LiveProcessTerminalCause::Completed, Some(0), true)
            );
            // Single collection.
            assert_eq!(
                a.collect_result(&h).unwrap().failure(),
                Some(FailureClass::Unknown)
            );
            assert!(matches!(release(&h), Some(Ok(()))));
            assert!(binding_refused(ch.handle(request(
                ch,
                3,
                ControllerOperation::Status
            ))));
            assert_eq!(fs::metadata(f.workspace.worktree_path()).unwrap().uid(), 0);
            assert_eq!(fs::metadata(f.repo.join(".git")).unwrap().uid(), 0);
            drop(h);
            println!(
                "PASS runtime={} read_only={read_only} protected collect/events/fidelity/channel/release",
                a.runtime_id()
            );
        }
    }
}

#[test]
#[ignore = "privileged hosted diagnostic"]
fn hosted_protected_cancel_drop_and_refusals_never_fall_back() {
    let launcher = launcher();
    for codex in [false, true] {
        // Accepted cancellation through the adapter.
        let f = Fixture::new(Some("task"));
        let a = adapter(codex, &f.executable, None);
        let h = start(&a, &f, &protected(false, &launcher, &f.repo));
        f.ready();
        let pid = String::from_utf8(f.read("pid")).unwrap();
        a.cancel(&h, &CancelReason::UserRequested).unwrap();
        let (cause, _, labeled, failure) = collected(&a, &h);
        assert_eq!(cause, LiveProcessTerminalCause::Cancelled);
        assert!(labeled);
        assert_eq!(failure, Some(FailureClass::UserCancelled));
        assert_eq!(
            observe_pid(&pid),
            PidState::Absent,
            "cancelled worker not observed absent after collection"
        );
        assert!(matches!(release(&h), Some(Ok(()))));
        drop(h);

        // Uncollected Drop: Workspace cleanup, then proven release.
        let f = Fixture::new(Some("task"));
        let a = adapter(codex, &f.executable, None);
        let h = start(&a, &f, &protected(false, &launcher, &f.repo));
        f.ready();
        let pid = String::from_utf8(f.read("pid")).unwrap();
        assert_eq!(observe_pid(&pid), PidState::Present);
        drop(h);
        assert_eq!(
            observe_pid(&pid),
            PidState::Absent,
            "dropped worker not observed absent after Drop collection"
        );
        assert_eq!(fs::metadata(f.workspace.worktree_path()).unwrap().uid(), 0);
        assert_eq!(fs::metadata(f.repo.join(".git")).unwrap().uid(), 0);

        // Refusals: no protected worker and never an ordinary fallback run.
        let f = Fixture::new(Some("task"));
        let other = Fixture::new(Some("task"));
        let a = adapter(codex, &f.executable, None);
        let foreign = protected(false, &launcher, &other.repo);
        assert!(matches!(
            a.start(&f.task(), &f.workspace, &foreign),
            Err(FailureClass::PolicyBlocked)
        ));
        let taskless = Fixture::new(None);
        let b = adapter(codex, &taskless.executable, None);
        assert!(matches!(
            b.start(
                &taskless.task(),
                &taskless.workspace,
                &protected(false, &launcher, &taskless.repo)
            ),
            Err(FailureClass::PolicyBlocked)
        ));
        std::thread::sleep(Duration::from_millis(200));
        for g in [&f, &taskless] {
            assert!(!g.path("pid").exists(), "a refused protected request ran");
            assert_eq!(fs::metadata(g.workspace.worktree_path()).unwrap().uid(), 0);
        }
        // The refused admissions left custody free: the same workspace launches.
        let h = start(&a, &f, &protected(false, &launcher, &f.repo));
        f.ready();
        f.release();
        let (cause, code, labeled, _) = collected(&a, &h);
        assert_eq!(
            (cause, code, labeled),
            (LiveProcessTerminalCause::Completed, Some(0), true)
        );
        assert!(matches!(release(&h), Some(Ok(()))));
        drop(h);
        println!(
            "PASS runtime={} protected cancel/drop/refusal/no-fallback",
            a.runtime_id()
        );
    }
}

#[test]
fn ordinary_adapters_are_labeled_without_custody_or_channel() {
    for codex in [false, true] {
        let f = Fixture::new(Some("task"));
        let a = adapter(codex, &f.executable, None);
        let h = start(&a, &f, &ordinary(false));
        assert!(!is_protected(&h));
        assert!(channel(&h).is_none());
        assert!(release(&h).is_none());
        f.ready();
        f.release();
        let (cause, code, labeled, _) = collected(&a, &h);
        assert_eq!(
            (cause, code, labeled),
            (LiveProcessTerminalCause::Completed, Some(0), false)
        );
    }
}

#[test]
fn capsule_task_must_match_a_task_bound_workspace() {
    for codex in [false, true] {
        let f = Fixture::new(Some("another-task"));
        let a = adapter(codex, &f.executable, None);
        assert!(matches!(
            a.start(&f.task(), &f.workspace, &ordinary(false)),
            Err(FailureClass::PolicyBlocked)
        ));
        assert!(!f.path("pid").exists(), "refused request launched");
        // A task-less ordinary workspace is still usable (non-authoritative).
        let g = Fixture::new(None);
        let b = adapter(codex, &g.executable, None);
        let h = start(&b, &g, &ordinary(false));
        g.ready();
        g.release();
        assert_eq!(collected(&b, &h).0, LiveProcessTerminalCause::Completed);
    }
}

#[test]
fn protected_launch_is_refused_before_any_worker_on_unadmitted_hosts() {
    let config = ProtectedLaunchConfig::new(
        CONTROLLER_ID,
        CONTROLLER_ID,
        WORKER_GID,
        WORKER_RANGE,
        "/sys/fs/cgroup/receipts-ra-portable-absent",
    )
    .unwrap();
    match ProtectedLauncher::new(config) {
        Err(ProtectedLaunchError::Unsupported(_)) => {}
        // An admitted Linux host is exercised only by the hosted diagnostic.
        Ok(_) if cfg!(target_os = "linux") => {}
        other => panic!("unexpected admission outcome: {other:?}"),
    }
}

#[test]
fn protected_refusals_classify_conservatively() {
    use crate::protected_launch::classify_protected_for_test as classify;
    for refusal in [
        ProtectedLaunchError::Unsupported("x".into()),
        ProtectedLaunchError::InvalidConfig("x".into()),
        ProtectedLaunchError::Custody("x".into()),
        ProtectedLaunchError::Binding("x"),
        ProtectedLaunchError::IdentityExhausted,
    ] {
        assert_eq!(classify(&refusal), FailureClass::PolicyBlocked);
    }
    assert_eq!(
        classify(&ProtectedLaunchError::CleanupUnknown("x".into())),
        FailureClass::Unknown
    );
}

#[test]
fn protected_composition_adds_no_runtime_process_control() {
    // Supplemental structural guard; hosted tests prove the behavior.
    let source = include_str!("protected_launch.rs");
    for forbidden in [
        "std::process",
        "Command",
        "Child",
        "libc::",
        "nix::",
        "SIGTERM",
        "SIGKILL",
        "waitpid",
        "process_group",
        "setpgid",
        "killpg",
        "setuid",
        "std::thread",
        "Instant",
        "sleep",
        "AtomicBool",
        "Mutex",
        "unenforced",
        "impl Drop",
        "mem::forget",
        "ManuallyDrop",
    ] {
        assert!(
            !source.contains(forbidden),
            "protected_launch.rs uses {forbidden}"
        );
    }
}

#[test]
fn native_worker_observes_exact_initial_exec_environment() {
    // Ordinary path, same observation the hosted cases compare: the worker
    // reads its environment itself, so no shell-added entry (PWD, SHLVL, _)
    // can appear, and default-empty really is empty.
    for codex in [false, true] {
        for env in [None, Some(env_policy(codex))] {
            let f = Fixture::new(Some("task"));
            let a = adapter(codex, &f.executable, env.clone());
            let h = start(&a, &f, &ordinary(false));
            f.ready();
            let observed = String::from_utf8(f.read("env-observed")).unwrap();
            assert_eq!(observed, expected_env(env.as_ref()), "codex={codex}");
            if env.is_none() {
                assert_eq!(observed, "[]");
            } else {
                assert_ne!(observed, "[]", "admitted entries must arrive");
            }
            assert!(!observed.contains("\"PWD\""));
            f.release();
            assert_eq!(collected(&a, &h).0, LiveProcessTerminalCause::Completed);
        }
    }
    // Comparison is order-independent and exact.
    let a = (OsString::from("A"), OsString::from("1"));
    let b = (OsString::from("B"), OsString::from("2"));
    assert_eq!(
        env_snapshot(vec![b.clone(), a.clone()]),
        env_snapshot(vec![a.clone(), b.clone()])
    );
    assert_ne!(env_snapshot(vec![a.clone()]), env_snapshot(vec![a, b]));
}

#[test]
fn wakeup_helper_repolls_only_after_an_observed_notification() {
    // Never woken: Pending once, Ready if ever repolled. A timer repoll would
    // return Ok; the helper must fail at expiry after exactly one poll.
    let mut polls = 0;
    let start = Instant::now();
    let never = poll_on_wakeups(
        std::future::poll_fn(|_| {
            polls += 1;
            if polls == 1 {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }),
        Duration::from_millis(300),
    );
    assert_eq!(never, Err(NoWakeup { polls: 1 }));
    assert_eq!(polls, 1);
    assert!(start.elapsed() >= Duration::from_millis(300));

    // A valid wake from another thread makes progress.
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut spawned = false;
    let woken = poll_on_wakeups(
        std::future::poll_fn(|cx| {
            if ready.load(Ordering::SeqCst) {
                return Poll::Ready(());
            }
            if !spawned {
                spawned = true;
                let (ready, waker) = (ready.clone(), cx.waker().clone());
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(50));
                    ready.store(true, Ordering::SeqCst);
                    waker.wake();
                });
            }
            Poll::Pending
        }),
        Duration::from_secs(10),
    );
    assert_eq!(woken, Ok(()));

    // A wake that races ahead of Pending (issued during the poll) is kept.
    let mut polls = 0;
    let start = Instant::now();
    let raced = poll_on_wakeups(
        std::future::poll_fn(|cx| {
            polls += 1;
            if polls == 1 {
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }),
        Duration::from_secs(10),
    );
    assert_eq!(raced, Ok(()));
    assert_eq!(polls, 2);
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "racing wake was lost"
    );
}

#[test]
fn pid_observation_keeps_absent_zombie_present_and_unknown_distinct() {
    use std::io::{Error, ErrorKind};
    let stat = |s: &str| pid_state(Ok(s.into()));
    assert_eq!(
        pid_state(Err(Error::from(ErrorKind::NotFound))),
        PidState::Absent
    );
    for kind in [
        ErrorKind::PermissionDenied,
        ErrorKind::InvalidData,
        ErrorKind::Other,
    ] {
        assert!(matches!(
            pid_state(Err(Error::from(kind))),
            PidState::Unknown(_)
        ));
    }
    assert_eq!(stat("42 (worker) Z 1 42"), PidState::Zombie);
    assert_eq!(stat("42 (worker) S 1 42"), PidState::Present);
    assert_eq!(stat("42 (worker) R 1 42"), PidState::Present);
    // The state follows the last ')', even when the name contains ") Z ".
    assert_eq!(stat("42 (a) Z (b) S 1"), PidState::Present);
    assert_eq!(stat("42 (a) S (b) Z 1"), PidState::Zombie);
    for bad in [
        "",
        "42",
        "42 (worker)",
        "42 (worker) X 1",
        "42 (worker) ?? 1",
    ] {
        assert!(matches!(stat(bad), PidState::Unknown(_)), "{bad:?}");
    }
    assert!(matches!(observe_pid("not-a-pid"), PidState::Unknown(_)));
    // This process: Present with procfs; without procfs never Absent.
    let own = std::process::id().to_string();
    match observe_pid(&own) {
        PidState::Present => assert!(Path::new("/proc/self/stat").exists()),
        PidState::Unknown(_) => assert!(!Path::new("/proc/self/stat").exists()),
        other => panic!("own pid observed as {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn pid_observation_sees_real_zombie_then_absence_after_reaping() {
    let mut child = Command::new("/bin/true").spawn().unwrap();
    let pid = child.id().to_string();
    let start = Instant::now();
    // Exited but unreaped: a zombie, which is not absence.
    while observe_pid(&pid) != PidState::Zombie {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{:?}",
            observe_pid(&pid)
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    child.wait().unwrap();
    assert_eq!(observe_pid(&pid), PidState::Absent);
}
