use std::{cell::RefCell, path::PathBuf, process::Command};

use receipts_workspace_execution::{WorkspaceHandle, WorkspaceProvisionRequest};

use crate::{
    AttemptId, CodexTaskExecutionError, FailureClass, RawFailure, RuntimeAdapter, RuntimeAuthStatus,
};

#[derive(Debug, PartialEq, Eq)]
struct FixtureHealthReport(&'static str);
#[derive(Debug, PartialEq, Eq)]
struct FixtureRuntimeCapabilities(&'static str);
#[derive(Debug, PartialEq, Eq)]
struct FixtureModels(&'static str);
struct FixtureCapsule;
struct FixtureExecutionPolicy;
#[derive(Debug, PartialEq, Eq)]
struct FixtureAttemptHandle(&'static str);
struct FixtureAttemptEvent;
#[derive(Debug, PartialEq, Eq)]
struct OpaqueEventStream;
#[derive(Debug, PartialEq, Eq)]
struct FixtureAttemptResult(&'static str);
struct FixtureCancelReason;

struct TrackingRuntimeAdapter {
    calls: RefCell<[u8; 11]>,
}

impl TrackingRuntimeAdapter {
    fn new() -> Self {
        Self {
            calls: RefCell::new([0; 11]),
        }
    }

    fn record(&self, operation: usize) {
        self.calls.borrow_mut()[operation] += 1;
    }
}

impl RuntimeAdapter for TrackingRuntimeAdapter {
    type HealthReport = FixtureHealthReport;
    type RuntimeCapabilities = FixtureRuntimeCapabilities;
    type Models = FixtureModels;
    type Capsule = FixtureCapsule;
    type ExecutionPolicy = FixtureExecutionPolicy;
    type AttemptHandle = FixtureAttemptHandle;
    type AttemptEvent = FixtureAttemptEvent;
    type EventStream<'a> = OpaqueEventStream;
    type AttemptResult = FixtureAttemptResult;
    type CancelReason = FixtureCancelReason;

    fn runtime_id(&self) -> &str {
        self.record(0);
        "tracking-fixture"
    }

    fn health(&self) -> FixtureHealthReport {
        self.record(1);
        FixtureHealthReport("healthy-fixture")
    }

    fn authenticate_status(&self) -> RuntimeAuthStatus {
        self.record(2);
        RuntimeAuthStatus::Unknown
    }

    fn capabilities(&self) -> FixtureRuntimeCapabilities {
        self.record(3);
        FixtureRuntimeCapabilities("capabilities-fixture")
    }

    fn models(&self) -> FixtureModels {
        self.record(4);
        FixtureModels("models-fixture")
    }

    fn start(
        &self,
        _task: &FixtureCapsule,
        _workspace: &WorkspaceHandle,
        _policy: &FixtureExecutionPolicy,
    ) -> FixtureAttemptHandle {
        self.record(5);
        FixtureAttemptHandle("started-attempt")
    }

    fn stream_events<'a>(&'a self, _handle: &'a FixtureAttemptHandle) -> OpaqueEventStream {
        self.record(6);
        OpaqueEventStream
    }

    fn collect_result(&self, _handle: &FixtureAttemptHandle) -> FixtureAttemptResult {
        self.record(7);
        FixtureAttemptResult("collected-result")
    }

    fn cancel(&self, _handle: &FixtureAttemptHandle, _reason: &FixtureCancelReason) {
        self.record(8);
    }

    fn classify_failure(&self, error: &RawFailure) -> FailureClass {
        self.record(9);
        assert_eq!(error.evidence(), crate::RawFailureEvidence::EmptyPrompt);
        assert_eq!(error.classify_failure(), FailureClass::Unknown);
        // Fixture choice proves adapter control without changing RawFailure mapping.
        FailureClass::PolicyBlocked
    }

    fn resume(&self, attempt_id: &AttemptId) -> Option<FixtureAttemptHandle> {
        self.record(10);
        assert_eq!(attempt_id.as_str(), " exact-試行-e\u{301} ");
        Some(FixtureAttemptHandle("resumed-attempt"))
    }
}

struct DefaultResumeRuntimeAdapter;

impl RuntimeAdapter for DefaultResumeRuntimeAdapter {
    type HealthReport = FixtureHealthReport;
    type RuntimeCapabilities = FixtureRuntimeCapabilities;
    type Models = FixtureModels;
    type Capsule = FixtureCapsule;
    type ExecutionPolicy = FixtureExecutionPolicy;
    type AttemptHandle = FixtureAttemptHandle;
    type AttemptEvent = FixtureAttemptEvent;
    type EventStream<'a> = OpaqueEventStream;
    type AttemptResult = FixtureAttemptResult;
    type CancelReason = FixtureCancelReason;

    fn runtime_id(&self) -> &str {
        "default-resume-fixture"
    }

    fn health(&self) -> FixtureHealthReport {
        FixtureHealthReport("unused")
    }

    fn authenticate_status(&self) -> RuntimeAuthStatus {
        RuntimeAuthStatus::Unknown
    }

    fn capabilities(&self) -> FixtureRuntimeCapabilities {
        FixtureRuntimeCapabilities("unused")
    }

    fn models(&self) -> FixtureModels {
        FixtureModels("unused")
    }

    fn start(
        &self,
        _task: &FixtureCapsule,
        _workspace: &WorkspaceHandle,
        _policy: &FixtureExecutionPolicy,
    ) -> FixtureAttemptHandle {
        FixtureAttemptHandle("unused")
    }

    fn stream_events<'a>(&'a self, _handle: &'a FixtureAttemptHandle) -> OpaqueEventStream {
        OpaqueEventStream
    }

    fn collect_result(&self, _handle: &FixtureAttemptHandle) -> FixtureAttemptResult {
        FixtureAttemptResult("unused")
    }

    fn cancel(&self, _handle: &FixtureAttemptHandle, _reason: &FixtureCancelReason) {}

    fn classify_failure(&self, _error: &RawFailure) -> FailureClass {
        FailureClass::Unknown
    }
}

#[test]
fn frozen_surface_is_exercised_once_with_deterministic_results() {
    let repository = TestRepository::new();
    let workspace = WorkspaceProvisionRequest::new(
        &repository.0,
        "conformance-workspace",
        None,
        "conformance-task",
        repository.0.join("worktree"),
        &repository.git(&["rev-parse", "HEAD"]),
    )
    .unwrap()
    .provision()
    .unwrap();
    let adapter = TrackingRuntimeAdapter::new();
    let started = adapter.start(&FixtureCapsule, &workspace, &FixtureExecutionPolicy);

    assert_eq!(adapter.runtime_id(), "tracking-fixture");
    assert_eq!(adapter.health(), FixtureHealthReport("healthy-fixture"));
    assert_eq!(adapter.authenticate_status(), RuntimeAuthStatus::Unknown);
    assert_eq!(
        adapter.capabilities(),
        FixtureRuntimeCapabilities("capabilities-fixture")
    );
    assert_eq!(adapter.models(), FixtureModels("models-fixture"));
    assert_eq!(started, FixtureAttemptHandle("started-attempt"));
    assert_eq!(adapter.stream_events(&started), OpaqueEventStream);
    assert_eq!(
        adapter.collect_result(&started),
        FixtureAttemptResult("collected-result")
    );
    adapter.cancel(&started, &FixtureCancelReason);
    assert_eq!(
        adapter.classify_failure(&RawFailure::from(&CodexTaskExecutionError::EmptyPrompt)),
        FailureClass::PolicyBlocked
    );
    assert_eq!(
        adapter.resume(&AttemptId::new(" exact-試行-e\u{301} ").unwrap()),
        Some(FixtureAttemptHandle("resumed-attempt"))
    );
    assert_eq!(*adapter.calls.borrow(), [1; 11]);
}

#[test]
fn omitted_resume_override_means_unsupported() {
    let adapter = DefaultResumeRuntimeAdapter;

    assert_eq!(
        adapter.resume(&AttemptId::new(" exact-試行-e\u{301} ").unwrap()),
        None
    );
}

// A throwaway repository keeps the eleven-operation call test intact without
// fabricating Workspace-owned evidence or adding production lifecycle behavior.
struct TestRepository(PathBuf);

impl TestRepository {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "receipts-runtime-conformance-{}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        let repository = Self(path);
        repository.git(&["init", "--quiet"]);
        repository.git(&["commit", "--quiet", "--allow-empty", "-m", "seed"]);
        repository
    }

    fn git(&self, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(&self.0)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args([
                "-c",
                "user.name=Runtime Tests",
                "-c",
                "user.email=runtime@receipts.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {:?}", output);
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }
}

impl Drop for TestRepository {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
