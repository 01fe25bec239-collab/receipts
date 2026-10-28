//! Synthetic executable through real Git worktrees and the Workspace live runner.
use super::*;
use receipts_runtime_bindings::*;
use receipts_workspace_execution::execution::{LiveProcessTerminalCause, ProcessTimeoutPolicy};
use receipts_workspace_execution::{WorkspaceHandle, WorkspaceProvisionRequest};
use std::{
    fs,
    future::Future,
    path::PathBuf,
    pin::pin,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
    workspace: WorkspaceHandle,
    executable: PathBuf,
}
impl Fixture {
    fn new(prefix: &str, suffix: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ra-w1-1b-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let repo = root.join("repo");
        fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let result = Command::new("/usr/bin/git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(result.status.success());
            result.stdout
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
        fs::write(workspace.worktree_path().join("prefix"), prefix).unwrap();
        fs::write(workspace.worktree_path().join("suffix"), suffix).unwrap();
        let executable = root.join("worker");
        fs::write(&executable, "#!/bin/sh\nprintf '%s\\n' \"$@\" > argv\n/bin/cat > received\n/bin/cat prefix\n: > ready\nwhile [ ! -f release ]; do /bin/sleep 0.01; done\n/bin/cat suffix\nwhile [ -f hold ] && [ ! -f finish ]; do /bin/sleep 0.01; done\nif [ -f fail ]; then exit 9; fi\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        Self {
            root,
            workspace,
            executable,
        }
    }
    fn task(&self) -> RuntimeCapsuleFamily {
        self.task_with_budgets(None, None)
    }
    fn task_with_budgets(&self, time: Option<i64>, turns: Option<i64>) -> RuntimeCapsuleFamily {
        RuntimeCapsuleFamily::Task(
            TaskCapsule::try_new(
                "task".into(),
                "stream".into(),
                None,
                1,
                TaskType::Implementation,
                "objective 世界".into(),
                vec![criterion()],
                Some(vec!["non goal".into()]),
                self.workspace.base_sha().as_str().into(),
                self.workspace.base_sha().as_str().into(),
                vec!["src/**".into()],
                vec!["private/**".into()],
                Some(vec![
                    Ref::try_new(
                        RefType::RepoPath,
                        "README.md".into(),
                        None,
                        Some("Context".into()),
                    )
                    .unwrap(),
                ]),
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
                time,
                turns,
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
    fn ready(&self) {
        let start = Instant::now();
        while !self.workspace.worktree_path().join("ready").exists() {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn release(&self) {
        fs::write(self.workspace.worktree_path().join("release"), "").unwrap();
    }
    fn adapter(&self) -> ClaudeAdapter {
        ClaudeAdapter::new(&self.executable)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn criterion() -> Criterion {
    Criterion::try_new(
        "criterion".into(),
        "preserve criteria".into(),
        CriterionKind::Semantic,
        None,
        Some("reason".into()),
    )
    .unwrap()
}
fn policy(read_only: bool) -> RuntimeExecutionPolicy {
    RuntimeExecutionPolicy::new(
        ProcessTimeoutPolicy::new(Duration::from_secs(15), Duration::from_millis(100)).unwrap(),
        read_only,
    )
}
struct Notify(std::thread::Thread);
impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(Notify(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let start = Instant::now();
    loop {
        if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
            return result;
        }
        assert!(start.elapsed() < Duration::from_secs(20));
        std::thread::park_timeout(Duration::from_millis(100));
    }
}
fn start(f: &Fixture, p: &RuntimeExecutionPolicy) -> ClaudeAttemptHandle {
    f.adapter()
        .start(&f.task(), &f.workspace, p)
        .unwrap_or_else(|_| panic!("start refused"))
}
const RESULT: &str =
    "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ok\"}\n";

#[test]
fn trait_start_async_delivery_fidelity_and_single_collection() {
    let f = Fixture::new("{\"type\":\"future\",\"n\":1}\n", RESULT);
    let adapter = f.adapter();
    let handle = start(&f, &policy(true));
    f.ready();
    let mut stream = adapter.stream_events(&handle);
    let first = {
        let mut next = pin!(stream.next());
        let waker = Waker::from(Arc::new(Notify(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        match next.as_mut().poll(&mut cx) {
            Poll::Ready(x) => x,
            Poll::Pending => block_on(next),
        }
    };
    assert!(matches!(first, Some(ClaudeAttemptEvent::Record(_))));
    assert!(handle.attempt.terminal_cause().is_none());
    let input: serde_json::Value =
        serde_json::from_slice(&fs::read(f.workspace.worktree_path().join("received")).unwrap())
            .unwrap();
    let prompt = input["message"]["content"].as_str().unwrap();
    for text in [
        "objective 世界",
        "src/**",
        "private/**",
        "preserve criteria",
        "README.md",
        "Context",
        "non goal",
        "cargo",
        "stop on collision",
        "context_epoch: 7",
    ] {
        assert!(prompt.contains(text), "missing {text}");
    }
    let argv = fs::read_to_string(f.workspace.worktree_path().join("argv")).unwrap();
    assert!(argv.contains("\nplan\n"));
    f.release();
    let result = adapter
        .collect_result(&handle)
        .unwrap_or_else(|_| panic!("collect failed"));
    assert!(result.reduced_isolation);
    assert_eq!(result.provider_result(), ClaudeProviderResult::Success);
    assert_eq!(result.failure(), None);
    assert!(adapter.collect_result(&handle).is_err());
    assert!(matches!(
        block_on(stream.next()),
        Some(ClaudeAttemptEvent::Record(_))
    ));
    assert!(block_on(stream.next()).is_none());
    assert_eq!(adapter.authenticate_status(), RuntimeAuthStatus::Unknown);
    assert!(matches!(
        adapter.capabilities(),
        RuntimeCapabilities::Unknown
    ));
}

#[test]
fn partial_records_wait_without_duplicates_and_final_incomplete_is_error() {
    let f = Fixture::new("{\"type\":\"future\"", "}\n{\"type\":");
    fs::write(f.workspace.worktree_path().join("hold"), "").unwrap();
    let adapter = f.adapter();
    let handle = start(&f, &policy(false));
    f.ready();
    let mut stream = adapter.stream_events(&handle);
    let waker = Waker::from(Arc::new(Notify(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    for _ in 0..3 {
        assert!(matches!(
            std::pin::Pin::new(&mut stream).poll_next(&mut cx),
            Poll::Pending
        ));
    }
    f.release();
    assert!(matches!(
        block_on(stream.next()),
        Some(ClaudeAttemptEvent::Record(_))
    ));
    assert!(handle.attempt.terminal_cause().is_none());
    fs::write(f.workspace.worktree_path().join("finish"), "").unwrap();
    let result = adapter
        .collect_result(&handle)
        .unwrap_or_else(|_| panic!("collect failed"));
    assert_eq!(result.provider_result(), ClaudeProviderResult::Unknown);

    assert!(matches!(
        block_on(stream.next()),
        Some(ClaudeAttemptEvent::Unavailable(ClaudeEventError::Protocol(
            _
        )))
    ));
    assert!(block_on(stream.next()).is_none());
}

#[test]
fn truncation_is_never_a_complete_stream() {
    let f = Fixture::new(&"x".repeat(1_048_577), "");
    let adapter = f.adapter();
    let handle = start(&f, &policy(false));
    f.ready();
    let mut stream = adapter.stream_events(&handle);
    assert!(matches!(
        block_on(stream.next()),
        Some(ClaudeAttemptEvent::Unavailable(ClaudeEventError::Protocol(
            ClaudeStreamJsonError::StdoutTruncated
        )))
    ));
    f.release();
    let result = adapter
        .collect_result(&handle)
        .unwrap_or_else(|_| panic!("collect failed"));
    assert_eq!(result.provider_result(), ClaudeProviderResult::Unknown);
}

#[test]
fn cancellation_timeout_drop_and_policy_refusal() {
    for timeout in [false, true] {
        let f = Fixture::new("", "");
        let adapter = f.adapter();
        let mut p = policy(false);
        if timeout {
            p.timeout =
                ProcessTimeoutPolicy::new(Duration::from_millis(500), Duration::from_millis(100))
                    .unwrap();
        }
        let handle = start(&f, &p);
        if !timeout {
            f.ready();
            adapter.cancel(&handle, &CancelReason::UserRequested);
            adapter.cancel(&handle, &CancelReason::UserRequested);
        }
        let result = adapter
            .collect_result(&handle)
            .unwrap_or_else(|_| panic!("collect failed"));
        assert_eq!(
            result.outcome.process().terminal_cause(),
            if timeout {
                LiveProcessTerminalCause::TimedOut
            } else {
                LiveProcessTerminalCause::Cancelled
            }
        );
        assert!(adapter.collect_result(&handle).is_err());
    }
    let f = Fixture::new("", "");
    let adapter = f.adapter();
    for axis in 0..5 {
        let mut p = policy(false);
        match axis {
            0 => p.network_access = Some(false),
            1 => p.filesystem_scope = Some(vec![]),
            2 => p.sandbox_mode = Some("required".into()),
            3 => p.turn_budget = Some(1),
            _ => p.structured_output_schema = Some("secret schema".into()),
        }
        assert!(matches!(
            adapter.start(&f.task(), &f.workspace, &p),
            Err(FailureClass::PolicyBlocked)
        ));
    }
    let handle = start(&f, &policy(false));
    f.ready();
    let before = Instant::now();
    drop(handle);
    assert!(before.elapsed() < Duration::from_secs(5));
}

#[test]
fn canonical_repair_and_review_are_explicitly_refused_without_launch() {
    use receipts_workspace_execution::{WorkspaceCheckpointRef, WorkspaceCheckpointRefType};
    let f = Fixture::new("", "");
    let repair = RepairCapsule::try_new(
        "repair".into(),
        "task".into(),
        None,
        RepairAttemptNumberV1::try_new("2").unwrap(),
        f.workspace.base_sha().as_str().into(),
        "original objective".into(),
        vec![criterion()],
        None,
        vec![
            RepairCapsuleFindingV1::try_new(
                "finding".into(),
                RepairFindingSeverityV1::High,
                RepairFindingCategoryV1::Correctness,
                "repair instruction".into(),
                true,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
        ],
        None,
        "repair scope".into(),
        None,
        None,
        vec!["src/**".into()],
        Some(vec!["private/**".into()]),
        QualityFloor::Frontier,
        f.workspace.branch().into(),
        f.workspace.worktree_path().to_str().unwrap().into(),
        RepairContextEpochV1::try_new("7").unwrap(),
        QualityFloor::Frontier,
    )
    .unwrap();
    let review = ReviewCapsuleNonTemporalCore::new(
        "review".into(),
        "task".into(),
        "attempt".into(),
        f.workspace.base_sha().as_str().into(),
        f.workspace.base_sha().as_str().into(),
        "review objective".into(),
        vec![
            ReviewCapsuleCriterion::new(
                "criterion".into(),
                "review criteria".into(),
                ReviewCapsuleCriterionKind::Semantic,
                None,
                None,
            )
            .unwrap(),
        ],
        Some(vec!["non goal".into()]),
        None,
        None,
        WorkspaceCheckpointRef::new(WorkspaceCheckpointRefType::RepoPath, "diff", None, None)
            .unwrap(),
        vec!["src/**".into()],
        None,
        None,
        ReviewCapsuleReviewScope::Full,
        ReviewCapsuleSeverityPolicy::new(vec!["HIGH".into()], None).unwrap(),
        true,
        None,
        7,
    )
    .unwrap();
    let adapter = f.adapter();
    for capsule in [
        RuntimeCapsuleFamily::Repair(repair),
        RuntimeCapsuleFamily::Review(review),
    ] {
        assert!(matches!(
            adapter.start(&capsule, &f.workspace, &policy(true)),
            Err(FailureClass::PolicyBlocked)
        ));
    }
    assert!(!f.workspace.worktree_path().join("ready").exists());
}

#[test]
fn process_and_provider_evidence_remain_independent_and_errors_hide_payloads() {
    for (records, nonzero, expected) in [
        (RESULT, true, ClaudeProviderResult::Success),
        ("", false, ClaudeProviderResult::Unknown),
        (
            "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":true,\"result\":\"private provider payload\"}\n",
            false,
            ClaudeProviderResult::Error,
        ),
    ] {
        let f = Fixture::new("", records);
        let adapter = f.adapter();
        if nonzero {
            fs::write(f.workspace.worktree_path().join("fail"), "").unwrap();
        }
        let handle = start(&f, &policy(false));
        f.ready();
        f.release();
        let result = adapter
            .collect_result(&handle)
            .unwrap_or_else(|_| panic!("collect failed"));
        assert_eq!(result.provider_result(), expected);
        assert_eq!(result.failure(), Some(FailureClass::Unknown));
        assert_eq!(
            result.outcome.process().exit_code(),
            Some(if nonzero { 9 } else { 0 })
        );
    }
    let f = Fixture::new("", "");
    let adapter = ClaudeAdapter::new(f.root.join("private-secret-executable"));
    let failure = match adapter.start(&f.task(), &f.workspace, &policy(false)) {
        Err(e) => e,
        Ok(_) => panic!("missing executable started"),
    };
    assert!(!format!("{failure:?}").contains("private-secret"));
    assert!(matches!(failure, FailureClass::Unknown));
    let other = Fixture::new("", "");
    assert!(matches!(
        f.adapter()
            .start(&other.task(), &f.workspace, &policy(false)),
        Err(FailureClass::PolicyBlocked)
    ));
}

#[test]
fn capsule_mandatory_budgets_refuse_before_start() {
    let f = Fixture::new("", "");
    let adapter = f.adapter();
    for capsule in [
        f.task_with_budgets(None, Some(1)),
        f.task_with_budgets(Some(1), None),
    ] {
        assert!(matches!(
            adapter.start(&capsule, &f.workspace, &policy(false)),
            Err(FailureClass::PolicyBlocked)
        ));
    }
    assert!(!f.workspace.worktree_path().join("ready").exists());
}
