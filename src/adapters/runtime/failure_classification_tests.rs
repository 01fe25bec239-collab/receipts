//! Real Workspace processes; provider-like prose is data, never a class.
use std::{fs, path::PathBuf, time::Duration};

use receipts_workspace_execution::execution::{
    ExecutionError, LiveProcessAttemptError, LiveProcessOutcome, LiveProcessStartError,
    ProcessRunRequest, ProcessTimeoutPolicy, start_live_process_attempt,
};

use crate::codex_task_execution::{TaskCaptureSnapshot, execute_with_runner};
use crate::{
    CancelReason, CodexTaskExecutionRequest, CodexTaskSandboxMode, FailureChannel, FailureClass,
    RawFailure, RawFailureEvidence, RawFailureSource, RuntimeFailureOrigin, RuntimeId,
    classify_codex_task_execution_result, classify_live_outcome,
};
use receipts_workspace_execution::execution::ProcessTermination;

const ALL: [FailureClass; 12] = [
    FailureClass::RateLimited,
    FailureClass::SessionExhausted,
    FailureClass::AuthRequired,
    FailureClass::ProviderDown,
    FailureClass::Timeout,
    FailureClass::SandboxDenied,
    FailureClass::SafetyCheckPending,
    FailureClass::PolicyBlocked,
    FailureClass::RuntimeCrash,
    FailureClass::InvalidOutput,
    FailureClass::UserCancelled,
    FailureClass::Unknown,
];
const SAFETY: &str =
    "Safety check pending: this request was flagged for review. Please try again later. 429";
const POLICY: &str = "Request blocked by usage policy. Rate limit exceeded? No: refused by policy. 429 Too Many Requests";

fn run(script: &str, timeout_ms: u64, cancel: bool) -> LiveProcessOutcome {
    let dir = std::env::temp_dir().join(format!(
        "ra-w1-3-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let dir = fs::canonicalize(dir).unwrap();
    // Workspace refuses shell interpreters as executables; run a script file.
    let worker = dir.join("worker");
    fs::write(&worker, format!("#!/bin/sh\n{script}\n")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&worker, fs::Permissions::from_mode(0o700)).unwrap();
    let request = ProcessRunRequest::new(worker, [] as [&str; 0], &dir, &dir).unwrap();
    let policy = ProcessTimeoutPolicy::new(
        Duration::from_millis(timeout_ms),
        Duration::from_millis(100),
    )
    .unwrap();
    let attempt =
        start_live_process_attempt(&request, &policy).unwrap_or_else(|_| panic!("start failed"));
    if cancel {
        attempt.cancel();
    }
    let outcome = attempt.wait_collect().unwrap();
    let _ = fs::remove_dir_all(&dir);
    outcome
}

#[test]
fn each_failure_class_has_recorded_fixture_or_documented_unknown() {
    let doc = include_str!("../../../docs/runtime-adapters/failure-classes.md");
    for class in ALL {
        let row = format!("| `{}` |", class.as_str());
        let rows: Vec<_> = doc.lines().filter(|l| l.starts_with(&row)).collect();
        assert_eq!(
            rows.len(),
            1,
            "{} must have exactly one row",
            class.as_str()
        );
        let basis = rows[0]
            .trim_end_matches('|')
            .rsplit('|')
            .next()
            .unwrap()
            .trim();
        assert!(!basis.is_empty(), "{} has no basis", class.as_str());
        // No real recorded CLI failure capture exists yet; none may be cited.
        assert!(
            !basis.contains("fixtures/"),
            "{} cites a fixture",
            class.as_str()
        );
    }
}

#[test]
fn safety_message_never_rate_limited() {
    for exit in [0, 1, 75] {
        let script = format!("printf '%s' '{SAFETY}'; printf '%s' '{SAFETY}' >&2; exit {exit}");
        let outcome = run(&script, 10_000, false);
        let class = classify_live_outcome(&outcome, None);
        assert_eq!(class, (exit != 0).then_some(FailureClass::Unknown));
        assert_ne!(class, Some(FailureClass::RateLimited));
        assert_ne!(class, Some(FailureClass::SafetyCheckPending));
    }
    let result = codex_result(SAFETY);
    assert_eq!(
        classify_codex_task_execution_result(&result),
        Some(FailureClass::Unknown)
    );
}

#[test]
fn policy_blocked_never_rate_limited() {
    let outcome = run(
        &format!("printf '%s' '{POLICY}' >&2; exit 1"),
        10_000,
        false,
    );
    assert_eq!(
        classify_live_outcome(&outcome, None),
        Some(FailureClass::Unknown)
    );
    let result = codex_result(POLICY);
    assert_ne!(
        classify_codex_task_execution_result(&result),
        Some(FailureClass::RateLimited)
    );
}

#[test]
fn rate_limit_text_without_recorded_evidence_stays_unknown() {
    let text = "429 Too Many Requests: rate limit reached, retry after 60s";
    let outcome = run(&format!("printf '%s' '{text}'; exit 1"), 10_000, false);
    assert_eq!(
        classify_live_outcome(&outcome, None),
        Some(FailureClass::Unknown)
    );
    assert_eq!(
        classify_codex_task_execution_result(&codex_result(text)),
        Some(FailureClass::Unknown)
    );
}

#[test]
fn lifecycle_evidence_alone_selects_timeout_and_user_cancelled() {
    let completed = run("exit 0", 10_000, false);
    assert_eq!(classify_live_outcome(&completed, None), None);
    // A cancel reason cannot turn a completed process into a cancellation.
    assert_eq!(
        classify_live_outcome(&completed, Some(CancelReason::UserRequested)),
        None
    );
    let timed_out = run("exec /bin/sleep 30", 300, false);
    assert_eq!(
        classify_live_outcome(&timed_out, Some(CancelReason::UserRequested)),
        Some(FailureClass::Timeout)
    );
    let cancelled = run("exec /bin/sleep 30", 10_000, true);
    assert_eq!(
        classify_live_outcome(&cancelled, Some(CancelReason::UserRequested)),
        Some(FailureClass::UserCancelled)
    );
    // Cancellation without an accepted caller reason is not attributed to the user.
    assert_eq!(
        classify_live_outcome(&cancelled, None),
        Some(FailureClass::Unknown)
    );
}

#[test]
fn open_origin_accepts_any_runtime_id_and_keeps_typed_classification() {
    assert!(RuntimeId::new("").is_err());
    for id in ["gemini", " spaced id ", "試行.runtime", "x"] {
        let runtime_id = RuntimeId::new(id).unwrap();
        assert_eq!(runtime_id.as_str(), id);
        let origin = |channel| {
            RawFailureSource::Runtime(RuntimeFailureOrigin {
                runtime_id,
                channel,
            })
        };
        let error = ExecutionError::ProcessSpawnFailed {
            detail: "sk-secret-detail".into(),
        };
        for raw in [
            RawFailure::live_snapshot(runtime_id, &error),
            RawFailure::live_collect(
                runtime_id,
                &LiveProcessAttemptError::Execution(ExecutionError::ProcessSpawnFailed {
                    detail: "sk-secret-detail".into(),
                }),
            ),
            RawFailure::live_start(
                runtime_id,
                &LiveProcessStartError::Execution(ExecutionError::ProcessSpawnFailed {
                    detail: "sk-secret-detail".into(),
                }),
            ),
        ] {
            assert!(matches!(
                raw.evidence(),
                RawFailureEvidence::WorkspaceExecution(_)
            ));
            assert_eq!(raw.classify_failure(), FailureClass::Unknown);
            assert!(!format!("{raw:?} {raw}").contains("sk-secret"));
        }
        assert_eq!(
            RawFailure::live_snapshot(runtime_id, &error).origin(),
            origin(FailureChannel::Events)
        );
        let controller =
            RawFailure::live_collect(runtime_id, &LiveProcessAttemptError::ControllerFailed);
        assert_eq!(controller.origin(), origin(FailureChannel::Collect));
        assert_eq!(controller.evidence(), RawFailureEvidence::ControllerFailed);
        assert_ne!(controller.classify_failure(), FailureClass::RuntimeCrash);
        let start = RawFailure::live_start(runtime_id, &LiveProcessStartError::ControllerFailed);
        assert_eq!(start.origin(), origin(FailureChannel::Start));
        assert_eq!(start.classify_failure(), FailureClass::Unknown);
    }
    // The open origin keeps the accepted fixed-size, drop-free carrier.
    fn copy<T: Copy + Send + Sync + 'static>() {}
    copy::<RawFailure>();
    assert!(!std::mem::needs_drop::<RawFailure>());
    assert!(std::mem::size_of::<RawFailure>() <= 64);
}

#[test]
fn start_timeout_before_ready_maps_timeout_under_open_origin() {
    // Stdin is never read and the pipe buffer is exceeded, so READY cannot commit.
    let dir = std::env::temp_dir().join(format!("ra-w1-3-ready-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let dir = fs::canonicalize(dir).unwrap();
    let request = ProcessRunRequest::new(PathBuf::from("/bin/sleep"), ["30"], &dir, &dir)
        .unwrap()
        .with_stdin(
            receipts_workspace_execution::execution::ProcessStdin::bytes(vec![b'x'; 1 << 20])
                .unwrap(),
        );
    let policy =
        ProcessTimeoutPolicy::new(Duration::from_millis(300), Duration::from_millis(100)).unwrap();
    let error = match start_live_process_attempt(&request, &policy) {
        Err(error) => error,
        Ok(_) => panic!("unread stdin reached READY"),
    };
    let runtime_id = RuntimeId::new("third.runtime").unwrap();
    let raw = RawFailure::live_start(runtime_id, &error);
    assert!(matches!(
        raw.evidence(),
        RawFailureEvidence::LiveTerminalBeforeReady { .. }
    ));
    assert_eq!(raw.classify_failure(), FailureClass::Timeout);
    let _ = fs::remove_dir_all(&dir);
}

fn codex_result(stderr: &str) -> crate::CodexTaskExecutionResult {
    let request = CodexTaskExecutionRequest::new(
        "/tmp/fake-codex-bin",
        "/tmp",
        "/tmp",
        ProcessTimeoutPolicy::new(Duration::from_secs(30), Duration::from_secs(3)).unwrap(),
        CodexTaskSandboxMode::ReadOnly,
        "test",
    );
    execute_with_runner(&request, |_, _| {
        Ok(TaskCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(1),
            stderr.as_bytes().to_vec(),
            Vec::new(),
            false,
            stderr.as_bytes().to_vec(),
            Vec::new(),
            false,
        ))
    })
    .unwrap()
}
