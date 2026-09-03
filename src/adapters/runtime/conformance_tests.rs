use std::cell::RefCell;

use crate::{FailureClass, RuntimeAdapter, RuntimeAuthStatus};

#[derive(Debug, PartialEq, Eq)]
struct FixtureHealthReport(&'static str);
#[derive(Debug, PartialEq, Eq)]
struct FixtureRuntimeCapabilities(&'static str);
#[derive(Debug, PartialEq, Eq)]
struct FixtureModels(&'static str);
struct FixtureCapsule;
struct FixtureWorkspaceHandle;
struct FixtureExecutionPolicy;
#[derive(Debug, PartialEq, Eq)]
struct FixtureAttemptHandle(&'static str);
struct FixtureAttemptEvent;
#[derive(Debug, PartialEq, Eq)]
struct OpaqueEventStream;
#[derive(Debug, PartialEq, Eq)]
struct FixtureAttemptResult(&'static str);
struct FixtureRawFailure;
struct FixtureAttemptId;
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
    type WorkspaceHandle = FixtureWorkspaceHandle;
    type ExecutionPolicy = FixtureExecutionPolicy;
    type AttemptHandle = FixtureAttemptHandle;
    type AttemptEvent = FixtureAttemptEvent;
    type EventStream<'a> = OpaqueEventStream;
    type AttemptResult = FixtureAttemptResult;
    type RawFailure = FixtureRawFailure;
    type AttemptId = FixtureAttemptId;
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
        _workspace: &FixtureWorkspaceHandle,
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

    fn classify_failure(&self, _error: &FixtureRawFailure) -> FailureClass {
        self.record(9);
        FailureClass::Unknown
    }

    fn resume(&self, _attempt_id: &FixtureAttemptId) -> Option<FixtureAttemptHandle> {
        self.record(10);
        Some(FixtureAttemptHandle("resumed-attempt"))
    }
}

struct DefaultResumeRuntimeAdapter;

impl RuntimeAdapter for DefaultResumeRuntimeAdapter {
    type HealthReport = FixtureHealthReport;
    type RuntimeCapabilities = FixtureRuntimeCapabilities;
    type Models = FixtureModels;
    type Capsule = FixtureCapsule;
    type WorkspaceHandle = FixtureWorkspaceHandle;
    type ExecutionPolicy = FixtureExecutionPolicy;
    type AttemptHandle = FixtureAttemptHandle;
    type AttemptEvent = FixtureAttemptEvent;
    type EventStream<'a> = OpaqueEventStream;
    type AttemptResult = FixtureAttemptResult;
    type RawFailure = FixtureRawFailure;
    type AttemptId = FixtureAttemptId;
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
        _workspace: &FixtureWorkspaceHandle,
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

    fn classify_failure(&self, _error: &FixtureRawFailure) -> FailureClass {
        FailureClass::Unknown
    }
}

#[test]
fn frozen_surface_is_exercised_once_with_deterministic_results() {
    let adapter = TrackingRuntimeAdapter::new();
    let started = adapter.start(
        &FixtureCapsule,
        &FixtureWorkspaceHandle,
        &FixtureExecutionPolicy,
    );

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
        adapter.classify_failure(&FixtureRawFailure),
        FailureClass::Unknown
    );
    assert_eq!(
        adapter.resume(&FixtureAttemptId),
        Some(FixtureAttemptHandle("resumed-attempt"))
    );
    assert_eq!(*adapter.calls.borrow(), [1; 11]);
}

#[test]
fn omitted_resume_override_means_unsupported() {
    let adapter = DefaultResumeRuntimeAdapter;

    assert_eq!(adapter.resume(&FixtureAttemptId), None);
}
