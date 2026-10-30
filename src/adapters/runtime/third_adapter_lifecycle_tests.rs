//! A third runtime runs start/events/collect/cancel/classify through public
//! extension points and the dynamic registry. Its code lives only here; no core
//! file names it. Synthetic worker: proves extension surfaces, not any provider.

use std::{
    fs,
    path::PathBuf,
    pin::Pin,
    process::Command,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use receipts_runtime_bindings::*;
use receipts_workspace_execution::execution::{
    LiveProcessAttempt, LiveProcessCancelAcceptance, LiveProcessOutcome, LiveProcessStartError,
    ProcessRunRequest, ProcessStdin, ProcessTimeoutPolicy, start_live_process_attempt,
};
use receipts_workspace_execution::{WorkspaceHandle, WorkspaceProvisionRequest};

use crate::registry::{
    AttemptFailure, DynAdapter, DynAttemptEvent, HandleMismatch, RegistryError,
    RuntimeAdapterRegistry, RuntimeEventStream,
};
use crate::{
    CancelReason, CapabilityStatus, ClaudeAdapter, CodexAdapter, ExtensionCapabilityReport,
    FailureChannel, FailureClass, RawFailure, RawFailureSource, RuntimeAdapter, RuntimeAuthStatus,
    RuntimeCapabilities, RuntimeExecutionPolicy, RuntimeFailureOrigin, RuntimeId,
    RuntimeObservation, classify_live_outcome,
};

const ID: &str = "line-echo.test";

// ---- the third adapter: only public items ----

struct LineAdapter {
    id: &'static str,
    executable: PathBuf,
}
struct LineHandle {
    attempt: LiveProcessAttempt,
    cancel: Mutex<Option<CancelReason>>,
    collected: AtomicBool,
}
struct LineResult {
    outcome: LiveProcessOutcome,
    cancel: Option<CancelReason>,
}
impl AttemptFailure for LineResult {
    fn failure(&self) -> Option<FailureClass> {
        classify_live_outcome(&self.outcome, self.cancel)
    }
}
enum LineEvent {
    Line(Vec<u8>),
    Unavailable(RawFailure),
}
impl From<LineEvent> for DynAttemptEvent {
    fn from(event: LineEvent) -> Self {
        match event {
            LineEvent::Line(line) => Self::Record(line),
            LineEvent::Unavailable(raw) => Self::Unavailable(Box::new(raw)),
        }
    }
}
impl LineAdapter {
    fn runtime(&self) -> RuntimeId {
        RuntimeId::new(self.id).expect("nonempty runtime id")
    }
}

/// An observer thread wakes a registered task every few milliseconds.
struct LineEvents<'a> {
    adapter: &'a LineAdapter,
    handle: &'a LineHandle,
    cursor: usize,
    ended: bool,
    waker: Arc<Mutex<(bool, Option<Waker>)>>,
    observer: Option<JoinHandle<()>>,
}
impl RuntimeEventStream for LineEvents<'_> {
    type Event = LineEvent;
    fn poll_event(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<LineEvent>> {
        let this = self.get_mut();
        if this.ended {
            return Poll::Ready(None);
        }
        let collected = this.handle.collected.load(Ordering::Acquire);
        let snapshot = match this.handle.attempt.snapshot_output() {
            Ok(snapshot) => snapshot,
            Err(error) => {
                this.ended = true;
                let raw = RawFailure::live_snapshot(this.adapter.runtime(), &error);
                return Poll::Ready(Some(LineEvent::Unavailable(raw)));
            }
        };
        let stdout = snapshot.stdout();
        let bytes = [stdout.head(), stdout.tail()].concat();
        if let Some(end) = bytes[this.cursor..].iter().position(|b| *b == b'\n') {
            let line = bytes[this.cursor..this.cursor + end + 1].to_vec();
            this.cursor += end + 1;
            return Poll::Ready(Some(LineEvent::Line(line)));
        }
        if collected {
            this.ended = true;
            return Poll::Ready(None);
        }
        this.waker.lock().unwrap().1 = Some(cx.waker().clone());
        Poll::Pending
    }
}
impl Drop for LineEvents<'_> {
    fn drop(&mut self) {
        self.waker.lock().unwrap().0 = true;
        if let Some(observer) = self.observer.take() {
            observer.join().unwrap();
        }
    }
}

impl RuntimeAdapter for LineAdapter {
    type HealthReport = RuntimeObservation;
    type Models = RuntimeObservation;
    type ExecutionPolicy = RuntimeExecutionPolicy;
    type AttemptHandle = LineHandle;
    type AttemptEvent = LineEvent;
    type EventStream<'a> = LineEvents<'a>;
    type AttemptResult = Result<LineResult, FailureClass>;
    type CancelReason = CancelReason;
    fn runtime_id(&self) -> &str {
        self.id
    }
    fn health(&self) -> RuntimeObservation {
        RuntimeObservation::Unknown
    }
    fn authenticate_status(&self) -> RuntimeAuthStatus {
        RuntimeAuthStatus::Unknown
    }
    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities::Extension(
            ExtensionCapabilityReport::new(self.id, [("line-events", CapabilityStatus::Supported)])
                .unwrap(),
        )
    }
    fn models(&self) -> RuntimeObservation {
        RuntimeObservation::Unknown
    }
    fn start(
        &self,
        task: &RuntimeCapsuleFamily,
        workspace: &WorkspaceHandle,
        policy: &RuntimeExecutionPolicy,
    ) -> Result<LineHandle, FailureClass> {
        let RuntimeCapsuleFamily::Task(task) = task else {
            return Err(FailureClass::PolicyBlocked);
        };
        if !policy.supported() {
            return Err(FailureClass::PolicyBlocked);
        }
        let stdin =
            ProcessStdin::bytes(task.objective()).map_err(|_| FailureClass::PolicyBlocked)?;
        let path = workspace.worktree_path();
        let attempt = ProcessRunRequest::new(&self.executable, [] as [&str; 0], path, path)
            .map_err(LiveProcessStartError::from)
            .and_then(|request| {
                start_live_process_attempt(&request.with_stdin(stdin), &policy.timeout)
            })
            .map_err(|error| {
                self.classify_failure(&RawFailure::live_start(self.runtime(), &error))
            })?;
        Ok(LineHandle {
            attempt,
            cancel: Mutex::new(None),
            collected: AtomicBool::new(false),
        })
    }
    fn stream_events<'a>(&'a self, handle: &'a LineHandle) -> LineEvents<'a> {
        let waker = Arc::new(Mutex::new((false, None::<Waker>)));
        let shared = waker.clone();
        let observer = thread::spawn(move || {
            loop {
                let pending = {
                    let mut state = shared.lock().unwrap();
                    if state.0 {
                        return;
                    }
                    state.1.take()
                };
                if let Some(waker) = pending {
                    waker.wake();
                }
                thread::sleep(Duration::from_millis(5));
            }
        });
        LineEvents {
            adapter: self,
            handle,
            cursor: 0,
            ended: false,
            waker,
            observer: Some(observer),
        }
    }
    fn collect_result(&self, handle: &LineHandle) -> Result<LineResult, FailureClass> {
        let outcome = handle.attempt.wait_collect();
        handle.collected.store(true, Ordering::Release);
        let outcome = outcome.map_err(|error| {
            self.classify_failure(&RawFailure::live_collect(self.runtime(), &error))
        })?;
        Ok(LineResult {
            outcome,
            cancel: *handle.cancel.lock().unwrap(),
        })
    }
    fn cancel(&self, handle: &LineHandle, reason: &CancelReason) {
        let mut accepted = handle.cancel.lock().unwrap();
        if handle.attempt.cancel() == LiveProcessCancelAcceptance::CancelAccepted {
            *accepted = Some(*reason);
        }
    }
    fn classify_failure(&self, error: &RawFailure) -> FailureClass {
        error.classify_failure()
    }
}

// ---- fixture and strict-wakeup driver ----

struct Fixture {
    root: PathBuf,
    workspace: WorkspaceHandle,
    executable: PathBuf,
}
impl Fixture {
    fn new(suffix: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ra-w1-4-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let repo = root.join("repo");
        fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("/usr/bin/git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success());
            out.stdout
        };
        git(&["init", "-q"]);
        git(&[
            "-c",
            "user.name=test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ]);
        let sha = String::from_utf8(git(&["rev-parse", "HEAD"])).unwrap();
        let workspace = WorkspaceProvisionRequest::new(
            &repo,
            "workspace",
            Some("task"),
            "runtime-a3/task",
            root.join("work"),
            sha.trim(),
        )
        .unwrap()
        .provision()
        .unwrap();
        fs::write(workspace.worktree_path().join("suffix"), suffix).unwrap();
        let executable = root.join("worker");
        fs::write(&executable, "#!/bin/sh\n/bin/cat > received\n: > ready\nwhile [ ! -f release ]; do /bin/sleep 0.01; done\n/bin/cat suffix\nif [ -f fail ]; then exit 9; fi\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            workspace,
            executable,
        }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.workspace.worktree_path().join(name)
    }
    fn ready(&self) {
        let start = Instant::now();
        while !self.path("ready").exists() {
            assert!(start.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(5));
        }
    }
    fn release_later(&self) -> JoinHandle<()> {
        let release = self.path("release");
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            fs::write(release, "").unwrap();
        })
    }
    fn task(&self) -> RuntimeCapsuleFamily {
        let sha = self.workspace.base_sha().as_str();
        RuntimeCapsuleFamily::Task(
            TaskCapsule::try_new(
                "task".into(),
                "stream".into(),
                None,
                1,
                TaskType::Implementation,
                "objective -- \"exact\" 世界".into(),
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
                None,
                sha.into(),
                sha.into(),
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
                vec!["stop".into()],
                None,
                7,
            )
            .unwrap(),
        )
    }
    fn line(&self) -> LineAdapter {
        LineAdapter {
            id: ID,
            executable: self.executable.clone(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn policy(timeout: Duration) -> RuntimeExecutionPolicy {
    RuntimeExecutionPolicy::new(
        ProcessTimeoutPolicy::new(timeout, Duration::from_millis(100)).unwrap(),
        false,
    )
}

#[derive(Default)]
struct Woken(Mutex<bool>, Condvar);
impl Wake for Woken {
    fn wake(self: Arc<Self>) {
        *self.0.lock().unwrap() = true;
        self.1.notify_all();
    }
}
/// Never re-polls on a timer: after `Pending` it waits only for a real wake.
/// Returns the event and whether any poll was `Pending`.
fn next<S: RuntimeEventStream + Unpin>(stream: &mut S) -> (Option<S::Event>, bool) {
    let woken = Arc::new(Woken::default());
    let waker = Waker::from(woken.clone());
    let mut cx = Context::from_waker(&waker);
    let mut pended = false;
    loop {
        if let Poll::Ready(event) = Pin::new(&mut *stream).poll_event(&mut cx) {
            return (event, pended);
        }
        pended = true;
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut flag = woken.0.lock().unwrap();
        while !*flag {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "stream returned Pending without waking");
            flag = woken.1.wait_timeout(flag, left).unwrap().0;
        }
        *flag = false;
    }
}
fn record(event: Option<DynAttemptEvent>) -> Vec<u8> {
    match event {
        Some(DynAttemptEvent::Record(record)) => record,
        _ => panic!("expected a record"),
    }
}

fn registered(f: &Fixture) -> RuntimeAdapterRegistry {
    let mut registry = RuntimeAdapterRegistry::new();
    registry.register(DynAdapter::new(f.line())).unwrap();
    registry
        .register(DynAdapter::new(ClaudeAdapter::new(&f.executable)))
        .unwrap();
    registry
        .register(DynAdapter::new(CodexAdapter::new(&f.executable)))
        .unwrap();
    registry
}

// ---- proofs ----

#[test]
fn registry_refuses_empty_and_duplicate_ids_without_replacing() {
    let f = Fixture::new("");
    let mut registry = registered(&f);
    assert_eq!(
        registry.runtime_ids().collect::<Vec<_>>(),
        ["claude", "codex", ID]
    );
    let other = LineAdapter {
        id: ID,
        executable: f.root.join("never-used"),
    };
    assert_eq!(
        registry.register(DynAdapter::new(other)),
        Err(RegistryError::DuplicateRuntimeId)
    );
    let empty = LineAdapter {
        id: "",
        executable: f.executable.clone(),
    };
    assert_eq!(
        registry.register(DynAdapter::new(empty)),
        Err(RegistryError::EmptyRuntimeId)
    );
    assert!(registry.get("Line-Echo.test").is_none());
    assert!(registry.get(" line-echo.test").is_none());
    let RuntimeCapabilities::Extension(report) = registry.get(ID).unwrap().capabilities() else {
        panic!("third adapter capabilities lost");
    };
    assert_eq!(report.status("line-events"), CapabilityStatus::Supported);
    // The original registration survived the duplicate attempt.
    let handle = registry
        .get(ID)
        .unwrap()
        .start(&f.task(), &f.workspace, &policy(Duration::from_secs(15)))
        .unwrap_or_else(|_| panic!("start refused"));
    f.ready();
    let typed: &LineHandle = handle.downcast_ref().unwrap();
    assert!(typed.attempt.terminal_cause().is_none());
}

#[test]
fn third_adapter_full_lifecycle_through_registry_with_real_wakeups() {
    let f = Fixture::new("line-1\nline-2\n");
    let registry = registered(&f);
    let adapter = registry.get(ID).unwrap();
    let handle = adapter
        .start(&f.task(), &f.workspace, &policy(Duration::from_secs(15)))
        .unwrap_or_else(|_| panic!("start refused"));
    assert_eq!(handle.runtime_id(), ID);
    f.ready();
    // Exact stdin delivery, empty argv.
    assert_eq!(
        fs::read(f.path("received")).unwrap(),
        "objective -- \"exact\" 世界".as_bytes()
    );
    let mut stream = registry
        .for_handle(&handle)
        .unwrap()
        .stream_events(&handle)
        .unwrap_or_else(|_| panic!("own handle refused"));
    let releaser = f.release_later();
    let (event, pended) = next(&mut stream);
    assert!(pended, "event arrived before release; wakeup not exercised");
    assert_eq!(record(event), b"line-1\n");
    releaser.join().unwrap();
    let result = adapter
        .collect_result(&handle)
        .unwrap_or_else(|_| panic!("own handle refused"));
    assert_eq!(result.failure(), None);
    let typed = result
        .downcast_ref::<Result<LineResult, FailureClass>>()
        .unwrap()
        .as_ref()
        .unwrap_or_else(|_| panic!("collect failed"));
    assert_eq!(typed.outcome.exit_code(), Some(0));
    assert_eq!(record(next(&mut stream).0), b"line-2\n");
    assert!(next(&mut stream).0.is_none());
    // A second collection is a typed refusal, classified through the registry.
    let second = adapter
        .collect_result(&handle)
        .unwrap_or_else(|_| panic!("own handle refused"));
    assert_eq!(second.failure(), Some(FailureClass::Unknown));
    // Foreign handles are refused before anything is invoked.
    for other in ["claude", "codex"] {
        let other = registry.get(other).unwrap();
        assert!(matches!(other.collect_result(&handle), Err(HandleMismatch)));
        assert!(matches!(
            other.cancel(&handle, &CancelReason::UserRequested),
            Err(HandleMismatch)
        ));
        assert!(other.stream_events(&handle).is_err());
    }
    assert!(
        adapter
            .resume(&crate::AttemptId::new("a").unwrap())
            .is_none()
    );
}

#[test]
fn third_adapter_cancel_timeout_and_failure_classify_from_lifecycle() {
    let id = RuntimeId::new(ID).unwrap();
    for case in ["cancel", "timeout", "nonzero"] {
        let f = Fixture::new("");
        let registry = registered(&f);
        let adapter = registry.get(ID).unwrap();
        let timeout = if case == "timeout" { 500 } else { 15_000 };
        if case == "nonzero" {
            fs::write(f.path("fail"), "").unwrap();
            fs::write(f.path("release"), "").unwrap();
        }
        let handle = adapter
            .start(
                &f.task(),
                &f.workspace,
                &policy(Duration::from_millis(timeout)),
            )
            .unwrap_or_else(|_| panic!("start refused"));
        if case == "cancel" {
            f.ready();
            adapter
                .cancel(&handle, &CancelReason::UserRequested)
                .unwrap();
            // Only the accepted cancellation records a reason.
            adapter
                .cancel(&handle, &CancelReason::UserRequested)
                .unwrap();
        }
        let result = adapter
            .collect_result(&handle)
            .unwrap_or_else(|_| panic!("refused"));
        assert_eq!(
            result.failure(),
            Some(match case {
                "cancel" => FailureClass::UserCancelled,
                "timeout" => FailureClass::Timeout,
                _ => FailureClass::Unknown,
            })
        );
    }
    // Missing executable: open-origin raw failure classified through the registry.
    let f = Fixture::new("");
    let registry = registered(&f);
    let missing = LineAdapter {
        id: ID,
        executable: f.root.join("missing-secret-path"),
    };
    let refused = match RuntimeAdapter::start(
        &missing,
        &f.task(),
        &f.workspace,
        &policy(Duration::from_secs(5)),
    ) {
        Err(class) => class,
        Ok(_) => panic!("missing executable started"),
    };
    assert_eq!(refused, FailureClass::Unknown);
    let raw = RawFailure::live_start(id, &LiveProcessStartError::ControllerFailed);
    assert_eq!(
        raw.origin(),
        RawFailureSource::Runtime(RuntimeFailureOrigin {
            runtime_id: id,
            channel: FailureChannel::Start
        })
    );
    assert_eq!(
        registry.get(ID).unwrap().classify_failure(&raw),
        FailureClass::Unknown
    );
    assert!(!format!("{raw:?}").contains("missing-secret"));
}

#[test]
fn policy_and_capsule_refusals_are_policy_blocked_never_rate_limited() {
    let f = Fixture::new("");
    let registry = registered(&f);
    for id in [ID, "claude", "codex"] {
        let adapter = registry.get(id).unwrap();
        for axis in 0..5 {
            let mut p = policy(Duration::from_secs(5));
            match axis {
                0 => p.network_access = Some(false),
                1 => p.filesystem_scope = Some(vec![]),
                2 => p.sandbox_mode = Some("required".into()),
                3 => p.turn_budget = Some(1),
                _ => p.structured_output_schema = Some("schema".into()),
            }
            let class = match adapter.start(&f.task(), &f.workspace, &p) {
                Err(class) => class,
                Ok(_) => panic!("{id} accepted an unsupported mandatory policy"),
            };
            assert_eq!(class, FailureClass::PolicyBlocked);
            assert_ne!(class, FailureClass::RateLimited);
        }
    }
    assert!(!f.path("ready").exists(), "a refused start launched");
}

#[test]
fn merged_adapters_run_through_registry_with_real_wakeups_and_user_cancel() {
    // Records valid for both protocols; both adapters keep their own semantics.
    let claude =
        "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ok\"}\n";
    let codex = "{\"type\":\"turn.started\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"agent_message\",\"text\":\"ok\"}}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}\n";
    for (id, records) in [("claude", claude), ("codex", codex)] {
        let f = Fixture::new(records);
        let registry = registered(&f);
        let adapter = registry.get(id).unwrap();
        let handle = adapter
            .start(&f.task(), &f.workspace, &policy(Duration::from_secs(15)))
            .unwrap_or_else(|_| panic!("{id} start refused"));
        f.ready();
        let mut stream = adapter
            .stream_events(&handle)
            .unwrap_or_else(|_| panic!("refused"));
        let releaser = f.release_later();
        let (event, pended) = next(&mut stream);
        assert!(pended, "{id} event arrived before release");
        assert_eq!(
            record(event),
            records.split_inclusive('\n').next().unwrap().as_bytes()
        );
        releaser.join().unwrap();
        let result = adapter
            .collect_result(&handle)
            .unwrap_or_else(|_| panic!("refused"));
        assert_eq!(result.failure(), None, "{id}");

        let f = Fixture::new("");
        let registry = registered(&f);
        let adapter = registry.get(id).unwrap();
        let handle = adapter
            .start(&f.task(), &f.workspace, &policy(Duration::from_secs(15)))
            .unwrap_or_else(|_| panic!("{id} start refused"));
        f.ready();
        adapter
            .cancel(&handle, &CancelReason::UserRequested)
            .unwrap();
        let result = adapter
            .collect_result(&handle)
            .unwrap_or_else(|_| panic!("refused"));
        assert_eq!(result.failure(), Some(FailureClass::UserCancelled), "{id}");
    }
}

#[test]
fn core_files_name_no_concrete_runtime() {
    let neutral = [
        ("registry.rs", include_str!("registry.rs")),
        (
            "failure_classification.rs",
            include_str!("failure_classification.rs"),
        ),
    ];
    for (name, source) in neutral {
        let lower = source.to_lowercase();
        for vendor in ["claude", "codex", "gemini", "line-echo", "lineadapter"] {
            assert!(!lower.contains(vendor), "{name} names {vendor}");
        }
    }
    for (name, source) in [
        ("adapter.rs", include_str!("adapter.rs")),
        ("raw_failure.rs", include_str!("raw_failure.rs")),
        ("failure.rs", include_str!("failure.rs")),
        (
            "runtime_capabilities.rs",
            include_str!("runtime_capabilities.rs"),
        ),
        ("policy.rs", include_str!("policy.rs")),
        ("lib.rs", include_str!("lib.rs")),
    ] {
        for third in ["line-echo", "LineAdapter", "LineHandle"] {
            assert!(!source.contains(third), "{name} names the third adapter");
        }
    }
}

// ---- handle ownership: same runtime ID and handle type, different instance ----

#[test]
fn foreign_same_id_instance_cannot_stream_cancel_or_collect_issuer_handles() {
    for id in [ID, "claude", "codex"] {
        let f = Fixture::new("line-1\nline-2\n");
        let issuer = registered(&f);
        let foreign = registered(&f);
        // Moving the registry must not change who issued a handle.
        let mut issuer = Box::new(issuer);
        let handle = issuer
            .get(id)
            .unwrap()
            .start(&f.task(), &f.workspace, &policy(Duration::from_secs(15)))
            .unwrap_or_else(|_| panic!("{id} start refused"));
        f.ready();
        issuer = Box::new(*issuer);

        let other = foreign.get(id).unwrap();
        assert!(other.stream_events(&handle).is_err(), "{id}");
        assert_eq!(
            other.cancel(&handle, &CancelReason::UserRequested),
            Err(HandleMismatch),
            "{id}"
        );
        assert!(matches!(other.collect_result(&handle), Err(HandleMismatch)));
        assert!(foreign.for_handle(&handle).is_none(), "{id}");
        assert!(!other.issued(&handle));
        let own = issuer.for_handle(&handle).expect("issuer lost its handle");
        assert!(own.issued(&handle) && std::ptr::eq(own, issuer.get(id).unwrap()));

        if id == ID {
            // Refused calls neither cancelled the process nor consumed events.
            let typed: &LineHandle = handle.downcast_ref().unwrap();
            assert!(typed.attempt.terminal_cause().is_none());
            let mut stream = own.stream_events(&handle).unwrap();
            f.release_later().join().unwrap();
            assert_eq!(record(next(&mut stream).0), b"line-1\n");
            assert_eq!(record(next(&mut stream).0), b"line-2\n");
        } else {
            f.release_later().join().unwrap();
        }
        // The single collection was not consumed; the issuer still gets the
        // real outcome, not a foreign cancellation.
        let result = own.collect_result(&handle).unwrap();
        assert_ne!(result.failure(), Some(FailureClass::UserCancelled), "{id}");
        if id == ID {
            assert_eq!(result.failure(), None);
        }
        let again = own.collect_result(&handle).unwrap();
        assert_eq!(again.failure(), Some(FailureClass::Unknown), "{id}");
    }
}

#[test]
fn handle_stays_bound_to_its_adapter_when_registered_after_start_and_after_drop() {
    let f = Fixture::new("");
    let adapter = DynAdapter::new(f.line());
    let handle = adapter
        .start(&f.task(), &f.workspace, &policy(Duration::from_secs(15)))
        .unwrap_or_else(|_| panic!("start refused"));
    f.ready();
    let mut registry = RuntimeAdapterRegistry::new();
    registry.register(adapter).unwrap();
    let own = registry.for_handle(&handle).expect("moved issuer lost");
    f.release_later().join().unwrap();
    assert_eq!(own.collect_result(&handle).unwrap().failure(), None);
    // A replacement with the same ID, in a registry or alone, is not the issuer.
    let replacement = registered(&f);
    assert!(replacement.for_handle(&handle).is_none());
    assert!(
        replacement
            .get(ID)
            .unwrap()
            .collect_result(&handle)
            .is_err()
    );
    // After the issuer is gone the handle is unowned, never adopted.
    drop(registry);
    assert!(replacement.for_handle(&handle).is_none());
    assert_eq!(handle.runtime_id(), ID);
}

/// Counts every call that reaches the concrete adapter; its handle comes only
/// from `resume`.
struct ResumeAdapter(Arc<std::sync::atomic::AtomicUsize>);
struct ResumeHandle;
struct ResumeResult;
struct NoEvents;
impl AttemptFailure for ResumeResult {
    fn failure(&self) -> Option<FailureClass> {
        None
    }
}
impl RuntimeEventStream for NoEvents {
    type Event = DynAttemptEvent;
    fn poll_event(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<DynAttemptEvent>> {
        Poll::Ready(None)
    }
}
impl RuntimeAdapter for ResumeAdapter {
    type HealthReport = RuntimeObservation;
    type Models = RuntimeObservation;
    type ExecutionPolicy = RuntimeExecutionPolicy;
    type AttemptHandle = ResumeHandle;
    type AttemptEvent = DynAttemptEvent;
    type EventStream<'a> = NoEvents;
    type AttemptResult = ResumeResult;
    type CancelReason = CancelReason;
    fn runtime_id(&self) -> &str {
        "resume.test"
    }
    fn health(&self) -> RuntimeObservation {
        RuntimeObservation::Unknown
    }
    fn authenticate_status(&self) -> RuntimeAuthStatus {
        RuntimeAuthStatus::Unknown
    }
    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities::Extension(
            ExtensionCapabilityReport::new("resume.test", [] as [(&str, CapabilityStatus); 0])
                .unwrap(),
        )
    }
    fn models(&self) -> RuntimeObservation {
        RuntimeObservation::Unknown
    }
    fn start(
        &self,
        _: &RuntimeCapsuleFamily,
        _: &WorkspaceHandle,
        _: &RuntimeExecutionPolicy,
    ) -> Result<ResumeHandle, FailureClass> {
        Err(FailureClass::PolicyBlocked)
    }
    fn stream_events<'a>(&'a self, _: &'a ResumeHandle) -> NoEvents {
        self.0.fetch_add(1, Ordering::SeqCst);
        NoEvents
    }
    fn collect_result(&self, _: &ResumeHandle) -> ResumeResult {
        self.0.fetch_add(1, Ordering::SeqCst);
        ResumeResult
    }
    fn cancel(&self, _: &ResumeHandle, _: &CancelReason) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn classify_failure(&self, error: &RawFailure) -> FailureClass {
        error.classify_failure()
    }
    fn resume(&self, _: &crate::AttemptId) -> Option<ResumeHandle> {
        Some(ResumeHandle)
    }
}

#[test]
fn resume_issued_handles_are_bound_to_the_resuming_instance() {
    let calls = || Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (issuer_calls, foreign_calls) = (calls(), calls());
    let mut issuer = RuntimeAdapterRegistry::new();
    issuer
        .register(DynAdapter::new(ResumeAdapter(issuer_calls.clone())))
        .unwrap();
    let mut foreign = RuntimeAdapterRegistry::new();
    foreign
        .register(DynAdapter::new(ResumeAdapter(foreign_calls.clone())))
        .unwrap();
    let attempt = crate::AttemptId::new("a").unwrap();
    let handle = issuer.get("resume.test").unwrap().resume(&attempt).unwrap();
    let issuer = [issuer];
    let other = foreign.get("resume.test").unwrap();
    assert!(other.stream_events(&handle).is_err());
    assert!(other.collect_result(&handle).is_err());
    assert!(other.cancel(&handle, &CancelReason::UserRequested).is_err());
    assert!(foreign.for_handle(&handle).is_none());
    assert_eq!(foreign_calls.load(Ordering::SeqCst), 0, "foreign delegated");
    assert_eq!(issuer_calls.load(Ordering::SeqCst), 0);
    let own = issuer[0].for_handle(&handle).unwrap();
    assert!(own.stream_events(&handle).is_ok());
    assert!(own.collect_result(&handle).is_ok());
    own.cancel(&handle, &CancelReason::UserRequested).unwrap();
    assert_eq!(issuer_calls.load(Ordering::SeqCst), 3);
    assert_eq!(foreign_calls.load(Ordering::SeqCst), 0);
}

// ---- identity redaction ----

#[test]
fn open_origin_runtime_identity_is_redacted_in_all_ordinary_formatting() {
    const MARKER: &str = "sk-test-NOT-A-REAL-CREDENTIAL-redaction-marker";
    let id = RuntimeId::new(MARKER).unwrap();
    let failures = [
        RawFailure::live_start(id, &LiveProcessStartError::ControllerFailed),
        RawFailure::live_snapshot(
            id,
            &receipts_workspace_execution::execution::ExecutionError::ProcessSpawnFailed {
                detail: MARKER.into(),
            },
        ),
        RawFailure::live_collect(
            id,
            &receipts_workspace_execution::execution::LiveProcessAttemptError::ControllerFailed,
        ),
    ];
    for raw in failures {
        let RawFailureSource::Runtime(origin) = raw.origin() else {
            panic!("open origin lost");
        };
        // Exact identity stays available through explicit access.
        assert_eq!(origin.runtime_id.as_str(), MARKER);
        let nested = (Some(raw), vec![raw.origin()], Ok::<_, RawFailure>(origin));
        for text in [
            format!("{raw}"),
            format!("{raw:?}"),
            format!("{raw:#?}"),
            format!("{origin:?}"),
            format!("{origin:#?}"),
            format!("{:?}", raw.origin()),
            format!("{:#?}", raw.origin()),
            format!("{:?}", origin.runtime_id),
            format!("{:#?}", origin.runtime_id),
            format!("{nested:?}"),
            format!("{nested:#?}"),
            format!("{:?}", Err::<(), _>(raw)),
        ] {
            assert!(!text.contains(MARKER), "identity leaked: {text}");
            assert!(!text.contains("redaction"), "identity fragment: {text}");
        }
    }
    // Redaction is structural, not a match on the value: any identity formats alike.
    let other = RuntimeId::new("other id 世界").unwrap();
    assert_eq!(format!("{id:?}"), format!("{other:?}"));
    assert_ne!(id, other);
}
