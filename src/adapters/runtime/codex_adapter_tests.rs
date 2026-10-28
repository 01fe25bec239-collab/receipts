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
            "ra-w1-2-{}-{}",
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
        fs::write(&executable, "#!/bin/sh\nprintf '%s' \"$$\" > pid\n/bin/ps -o pgid= -p \"$$\" > pgid\nprintf '%s\\n' \"$@\" > argv\n/bin/cat > received\n/bin/cat prefix\n: > ready\nwhile [ ! -f release ]; do /bin/sleep 0.01; done\n/bin/cat suffix\nwhile [ -f hold ] && [ ! -f finish ]; do /bin/sleep 0.01; done\nif [ -f fail ]; then exit 9; fi\n").unwrap();
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
                OBJECTIVE.into(),
                vec![criterion()],
                Some(vec!["non goal".into()]),
                self.workspace.base_sha().as_str().into(),
                self.workspace.base_sha().as_str().into(),
                vec![ALLOWED.into()],
                vec![FORBIDDEN.into()],
                Some(vec![
                    Ref::try_new(
                        RefType::RepoPath,
                        CONTEXT.into(),
                        None,
                        Some(SECTION.into()),
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
    fn adapter(&self) -> CodexAdapter {
        CodexAdapter::new(&self.executable)
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
        DESCRIPTION.into(),
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
fn start(f: &Fixture, p: &RuntimeExecutionPolicy) -> CodexAttemptHandle {
    f.adapter()
        .start(&f.task(), &f.workspace, p)
        .unwrap_or_else(|_| panic!("start refused"))
}
const RESULT: &str = "{\"type\":\"turn.started\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"agent_message\",\"text\":\"ok\"}}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}\n";

#[test]
fn trait_start_async_delivery_fidelity_and_single_collection() {
    let f = Fixture::new(
        "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n",
        RESULT,
    );
    let adapter = f.adapter();
    let handle = start(&f, &policy(true));
    f.ready();
    let mut stream = adapter.stream_events(&handle);
    let first = {
        let mut next = pin!(stream.next_event());
        let waker = Waker::from(Arc::new(Notify(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        match next.as_mut().poll(&mut cx) {
            Poll::Ready(x) => x,
            Poll::Pending => block_on(next),
        }
    };
    assert!(matches!(first, Some(CodexAttemptEvent::Record(_))));
    assert!(handle.attempt.terminal_cause().is_none());
    let prompt = fs::read_to_string(f.workspace.worktree_path().join("received")).unwrap();
    let received: serde_json::Value =
        serde_json::from_str(prompt.split_once('\n').unwrap().1).unwrap();
    let expected = serde_json::json!({
        "task_id":"task", "workstream_id":"stream", "parent_task_id":null,
        "attempt_number":1, "task_type":"IMPLEMENTATION", "objective":OBJECTIVE,
        "acceptance_criteria":[{"id":"criterion", "description":DESCRIPTION,
            "kind":"SEMANTIC", "check_command":null, "rationale":"reason"}],
        "non_goals":["non goal"], "baseline_sha":f.workspace.base_sha().as_str(),
        "start_sha":f.workspace.base_sha().as_str(), "allowed_write_paths":[ALLOWED],
        "forbidden_write_paths":[FORBIDDEN],
        "relevant_context_refs":[{"ref_type":"REPO_PATH", "target":CONTEXT, "digest":null, "section":SECTION}],
        "architecture_refs":null, "contract_refs":null, "dependencies":null,
        "quality_floor":"FRONTIER", "required_capabilities":[], "preferred_capabilities":null,
        "verification_plan":[{"command":["cargo","test"], "expected_exit_code":0}],
        "review_policy":null, "assurance_profile":"STANDARD", "cost_policy":null,
        "time_budget_seconds":null, "turn_budget":null, "branch":f.workspace.branch(),
        "worktree":f.workspace.worktree_path().to_str().unwrap(), "remote_publish_policy":null,
        "stop_conditions":["stop on collision"], "handoff_schema":null, "context_epoch":7
    });
    assert_eq!(received, expected);
    // Controls reject field-boundary changes and pre-escaped values.
    let mut wrong = expected.clone();
    wrong["objective"] = serde_json::json!(DESCRIPTION);
    assert_ne!(received, wrong);
    let mut wrong = expected;
    wrong["objective"] = serde_json::json!(format!("{OBJECTIVE:?}"));
    assert_ne!(received, wrong);
    let argv = fs::read_to_string(f.workspace.worktree_path().join("argv")).unwrap();
    assert_eq!(argv, "exec\n--json\n--sandbox\nread-only\n--\n-\n");
    f.release();
    let result = adapter
        .collect_result(&handle)
        .unwrap_or_else(|_| panic!("collect failed"));
    assert_eq!(
        result.protocol_termination(),
        CodexProtocolTermination::Completed
    );
    assert_eq!(result.failure(), None);
    assert!(adapter.collect_result(&handle).is_err());
    assert!(matches!(
        block_on(stream.next_event()),
        Some(CodexAttemptEvent::Record(_))
    ));
    for _ in 0..2 {
        assert!(matches!(
            block_on(stream.next_event()),
            Some(CodexAttemptEvent::Record(_))
        ));
    }
    assert!(block_on(stream.next_event()).is_none());
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
        block_on(stream.next_event()),
        Some(CodexAttemptEvent::Record(_))
    ));
    assert!(handle.attempt.terminal_cause().is_none());
    fs::write(f.workspace.worktree_path().join("finish"), "").unwrap();
    let result = adapter
        .collect_result(&handle)
        .unwrap_or_else(|_| panic!("collect failed"));
    assert_eq!(
        result.protocol_termination(),
        CodexProtocolTermination::Indeterminate
    );

    assert!(matches!(
        block_on(stream.next_event()),
        Some(CodexAttemptEvent::Unavailable(CodexEventError::Protocol(_)))
    ));
    assert!(block_on(stream.next_event()).is_none());
}

#[test]
fn truncation_is_never_a_complete_stream() {
    let f = Fixture::new(&"x".repeat(1_048_577), "");
    let adapter = f.adapter();
    let handle = start(&f, &policy(false));
    f.ready();
    let mut stream = adapter.stream_events(&handle);
    assert!(matches!(
        block_on(stream.next_event()),
        Some(CodexAttemptEvent::Unavailable(CodexEventError::Protocol(
            CodexLiveProtocolError::StdoutTruncated
        )))
    ));
    f.release();
    let result = adapter
        .collect_result(&handle)
        .unwrap_or_else(|_| panic!("collect failed"));
    assert_eq!(
        result.protocol_termination(),
        CodexProtocolTermination::Indeterminate
    );
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
        (RESULT, true, CodexProtocolTermination::Completed),
        ("", false, CodexProtocolTermination::Indeterminate),
        (
            "{\"type\":\"turn.failed\",\"error\":{\"message\":\"private provider payload\"}}\n",
            false,
            CodexProtocolTermination::Failed,
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
        assert_eq!(result.protocol_termination(), expected);
        assert_eq!(result.failure(), Some(FailureClass::Unknown));
        assert_eq!(
            result.outcome.process().exit_code(),
            Some(if nonzero { 9 } else { 0 })
        );
    }
    let f = Fixture::new("", "");
    let adapter = CodexAdapter::new(f.root.join("private-secret-executable"));
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

const OBJECTIVE: &str = "objective \"quotes\" \\ literal\nnewline 世界 e\u{301}";
const DESCRIPTION: &str = "criterion \"objective\": \\ path\n🦀";
const ALLOWED: &str = "src/\"allowed\"/\\/\n世界/**";
const FORBIDDEN: &str = "private/\"trap\"/\\/\n🦀/**";
const CONTEXT: &str = "context/\"file\"/\\/\n文.md";
const SECTION: &str = "section \"quoted\" \\ \nend";

#[test]
fn trait_start_child_environment_is_empty_or_explicitly_admitted() {
    use receipts_workspace_execution::execution::{
        ChildEnvAuthMode, ChildEnvPolicy, ChildEnvProvider, ProjectEnvConfirmation,
    };
    const TEST: &str =
        "codex_adapter_tests::trait_start_child_environment_is_empty_or_explicitly_admitted";
    if std::env::var_os("RA_CODEX_ENV_BOUNDARY_CHILD").is_none() {
        // Mutate only this subprocess's environment, never the parallel test runner.
        let compiler = Command::new("rustup")
            .args(["which", "rustc"])
            .output()
            .unwrap();
        assert!(compiler.status.success());
        let compiler = String::from_utf8(compiler.stdout).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", TEST, "--nocapture"])
            .env_clear()
            .env("RA_CODEX_ENV_BOUNDARY_CHILD", "1")
            .env("RA_TEST_RUSTC", compiler.trim())
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", "/synthetic/home")
            .env("LANG", "C")
            .env("CODEX_HOME", "/synthetic/codex")
            .env("OPENAI_API_KEY", "synthetic-sensitive-value")
            .env("ANTHROPIC_API_KEY", "excluded-other-provider")
            .env("HTTPS_PROXY", "excluded-proxy")
            .env("SECRET_TOKEN", "excluded-parent-secret")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated environment boundary test failed"
        );
        return;
    }
    let f = Fixture::new("", "");
    let source = f.root.join("environment_worker.rs");
    fs::write(&source, r#"
use std::{fs,io::Read,time::Duration};
fn main() {
    let mut env: Vec<_> = std::env::vars_os().collect(); env.sort();
    fs::write("observed-env", format!("{env:?}")).unwrap();
    let mut input = Vec::new(); std::io::stdin().read_to_end(&mut input).unwrap();
    fs::write("ready", "").unwrap();
    while !std::path::Path::new("release").exists() { std::thread::sleep(Duration::from_millis(2)); }
}
"#).unwrap();
    let executable = f.root.join("environment-worker");
    assert!(
        Command::new(std::env::var_os("RA_TEST_RUSTC").unwrap())
            .args(["--edition=2024"])
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .status()
            .unwrap()
            .success()
    );
    let project = f.root.join("project-env");
    let bytes = b"EXTRA=synthetic-extra=with-value\n";
    fs::write(&project, bytes).unwrap();
    let confirmation = ProjectEnvConfirmation::new(bytes.to_vec(), ["EXTRA"], false).unwrap();
    let env = ChildEnvPolicy::with_project_file(
        ChildEnvProvider::Codex,
        ChildEnvAuthMode::ApiKey,
        &project,
        &confirmation,
    )
    .unwrap();
    let mut expected: Vec<(std::ffi::OsString, std::ffi::OsString)> = env
        .names()
        .map(|name| {
            let value = if name == "EXTRA" {
                "synthetic-extra=with-value".into()
            } else {
                std::env::var_os(name).unwrap()
            };
            (name.into(), value)
        })
        .collect();
    expected.sort();
    for excluded in [
        "ANTHROPIC_API_KEY",
        "HTTPS_PROXY",
        "SECRET_TOKEN",
        "RA_CODEX_ENV_BOUNDARY_CHILD",
    ] {
        assert!(!env.names().any(|name| name == excluded));
    }
    for name in ["HOME", "LANG", "CODEX_HOME", "OPENAI_API_KEY", "EXTRA"] {
        assert!(env.names().any(|admitted| admitted == name));
    }
    assert!(!format!("{env:?}").contains("synthetic-sensitive-value"));
    for opted_in in [false, true] {
        let adapter = CodexAdapter::new(&executable);
        let adapter = if opted_in {
            adapter.with_env_policy(env.clone())
        } else {
            adapter
        };
        let handle = adapter
            .start(&f.task(), &f.workspace, &policy(false))
            .unwrap_or_else(|_| panic!("start refused"));
        f.ready();
        let observed = fs::read(f.workspace.worktree_path().join("observed-env")).unwrap();
        let correct = if opted_in {
            format!("{expected:?}")
        } else {
            "[]".into()
        };
        assert!(
            observed == correct.as_bytes(),
            "child environment differs from exact admitted snapshot"
        );
        // Negative controls: neither omission nor unrelated parent inheritance can pass.
        assert!(observed != b"[(\"SECRET_TOKEN\", \"excluded-parent-secret\")]");
        if opted_in {
            assert!(observed != b"[]");
        }
        f.release();
        assert!(adapter.collect_result(&handle).is_ok());
        fs::remove_file(f.workspace.worktree_path().join("ready")).unwrap();
        fs::remove_file(f.workspace.worktree_path().join("release")).unwrap();
    }
}

unsafe extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
    fn getpgrp() -> i32;
}
fn owned_identities(f: &Fixture) -> Option<[i32; 2]> {
    let number = |file| {
        fs::read_to_string(f.workspace.worktree_path().join(file))
            .ok()?
            .trim()
            .parse::<i32>()
            .ok()
    };
    let pid = number("pid")?;
    let pgid = number("pgid")?;
    if pid <= 1 || pgid != pid || pgid == unsafe { getpgrp() } {
        return None;
    }
    Some([pid, -pgid])
}
fn identities_absent(ids: [i32; 2]) -> bool {
    ids.into_iter().all(|id| {
        // Observation only, after fixture identity validation; Workspace owns signals/reaping.
        (unsafe { kill(id, 0) }) == -1 && std::io::Error::last_os_error().raw_os_error() == Some(3)
    })
}

#[test]
fn cancel_and_uncollected_drop_remove_owned_process_and_group() {
    for drop_without_collect in [false, true] {
        let f = Fixture::new("", "");
        let adapter = f.adapter();
        assert!(owned_identities(&f).is_none());
        for (pid, pgid) in [("1", "1"), ("42", "43")] {
            fs::write(f.workspace.worktree_path().join("pid"), pid).unwrap();
            fs::write(f.workspace.worktree_path().join("pgid"), pgid).unwrap();
            assert!(owned_identities(&f).is_none());
        }
        let handle = start(&f, &policy(false));
        f.ready();
        let ids = owned_identities(&f).expect("fixture must acknowledge leader and group");
        assert!(
            !identities_absent(ids),
            "live owned process must fail absence check"
        );
        assert!(handle.attempt.terminal_cause().is_none());
        if drop_without_collect {
            drop(handle);
        } else {
            adapter.cancel(&handle, &CancelReason::UserRequested);
            let result = adapter
                .collect_result(&handle)
                .unwrap_or_else(|_| panic!("cancel collection failed"));
            assert_eq!(
                result.outcome.process().terminal_cause(),
                LiveProcessTerminalCause::Cancelled
            );
        }
        assert!(
            identities_absent(ids),
            "owned leader or group remains observable after cleanup"
        );
    }
}

#[test]
fn unknown_event_is_ordered_evidence_but_never_success() {
    let unknown = "{\"type\":\"future\",\"n\":1}\n";
    let f = Fixture::new(unknown, RESULT);
    let adapter = f.adapter();
    let handle = start(&f, &policy(false));
    f.ready();
    let mut stream = adapter.stream_events(&handle);
    match block_on(stream.next_event()) {
        Some(CodexAttemptEvent::Record(record)) => assert_eq!(record, unknown.as_bytes()),
        _ => panic!("unknown event lost"),
    }
    f.release();
    let result = adapter
        .collect_result(&handle)
        .unwrap_or_else(|_| panic!("collect failed"));
    assert_eq!(
        result.protocol_termination(),
        CodexProtocolTermination::Completed
    );
    assert_eq!(result.failure(), Some(FailureClass::Unknown));
    for expected in RESULT.split_inclusive('\n') {
        match block_on(stream.next_event()) {
            Some(CodexAttemptEvent::Record(record)) => assert_eq!(record, expected.as_bytes()),
            _ => panic!("ordered record lost"),
        }
    }
    assert!(block_on(stream.next_event()).is_none());
}

#[test]
fn contradictory_terminal_events_are_explicitly_unavailable() {
    let failed = "{\"type\":\"turn.failed\",\"error\":{\"message\":\"synthetic\"}}\n";
    let f = Fixture::new(RESULT, failed);
    let adapter = f.adapter();
    let handle = start(&f, &policy(false));
    f.ready();
    let mut stream = adapter.stream_events(&handle);
    for expected in RESULT.split_inclusive('\n') {
        match block_on(stream.next_event()) {
            Some(CodexAttemptEvent::Record(record)) => assert_eq!(record, expected.as_bytes()),
            _ => panic!("ordered record lost"),
        }
    }
    f.release();
    let result = adapter
        .collect_result(&handle)
        .unwrap_or_else(|_| panic!("collect failed"));
    assert_eq!(
        result.protocol_termination(),
        CodexProtocolTermination::Indeterminate
    );
    assert_eq!(result.failure(), Some(FailureClass::Unknown));
    assert!(matches!(
        block_on(stream.next_event()),
        Some(CodexAttemptEvent::Unavailable(
            CodexEventError::ContradictoryTermination
        ))
    ));
    assert!(block_on(stream.next_event()).is_none());
}
