use std::cell::RefCell;

use receipts_runtime_bindings::RuntimeCapsuleFamily;
use receipts_workspace_execution::WorkspaceHandle;

use crate::{
    AttemptId, CodexTaskExecutionError, FailureClass, RawFailure, RuntimeAdapter,
    RuntimeAuthStatus, RuntimeCapabilities,
};

#[derive(Debug, PartialEq, Eq)]
struct FixtureHealthReport(&'static str);
#[derive(Debug, PartialEq, Eq)]
struct FixtureModels(&'static str);
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
    type Models = FixtureModels;
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

    fn capabilities(&self) -> RuntimeCapabilities {
        self.record(3);
        RuntimeCapabilities::Unknown
    }

    fn models(&self) -> FixtureModels {
        self.record(4);
        FixtureModels("models-fixture")
    }

    fn start(
        &self,
        _task: &RuntimeCapsuleFamily,
        _workspace: &WorkspaceHandle,
        _policy: &FixtureExecutionPolicy,
    ) -> Result<FixtureAttemptHandle, FailureClass> {
        self.record(5);
        Ok(FixtureAttemptHandle("started-attempt"))
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
    type Models = FixtureModels;
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

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities::Unknown
    }

    fn models(&self) -> FixtureModels {
        FixtureModels("unused")
    }

    fn start(
        &self,
        _task: &RuntimeCapsuleFamily,
        _workspace: &WorkspaceHandle,
        _policy: &FixtureExecutionPolicy,
    ) -> Result<FixtureAttemptHandle, FailureClass> {
        Ok(FixtureAttemptHandle("unused"))
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
fn safely_invocable_frozen_surface_has_deterministic_results() {
    let adapter = TrackingRuntimeAdapter::new();
    let handle = FixtureAttemptHandle("started-attempt");

    assert_eq!(adapter.runtime_id(), "tracking-fixture");
    assert_eq!(adapter.health(), FixtureHealthReport("healthy-fixture"));
    assert_eq!(adapter.authenticate_status(), RuntimeAuthStatus::Unknown);
    assert_eq!(adapter.capabilities(), RuntimeCapabilities::Unknown);
    assert_eq!(adapter.models(), FixtureModels("models-fixture"));
    assert_eq!(adapter.stream_events(&handle), OpaqueEventStream);
    assert_eq!(
        adapter.collect_result(&handle),
        FixtureAttemptResult("collected-result")
    );
    adapter.cancel(&handle, &FixtureCancelReason);
    assert_eq!(
        adapter.classify_failure(&RawFailure::from(&CodexTaskExecutionError::EmptyPrompt)),
        FailureClass::PolicyBlocked
    );
    assert_eq!(
        adapter.resume(&AttemptId::new(" exact-試行-e\u{301} ").unwrap()),
        Some(FixtureAttemptHandle("resumed-attempt"))
    );
    assert_eq!(*adapter.calls.borrow(), [1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1]);
}

#[test]
fn start_uses_the_three_variant_canonical_capsule_family() {
    let _: fn(
        &TrackingRuntimeAdapter,
        &RuntimeCapsuleFamily,
        &WorkspaceHandle,
        &FixtureExecutionPolicy,
    ) -> Result<FixtureAttemptHandle, FailureClass> = TrackingRuntimeAdapter::start;

    let _: fn(RuntimeCapsuleFamily) = |family| match family {
        RuntimeCapsuleFamily::Task(_) => {}
        RuntimeCapsuleFamily::Repair(_) => {}
        RuntimeCapsuleFamily::Review(_) => {}
    };
}

#[test]
fn omitted_resume_override_means_unsupported() {
    let adapter = DefaultResumeRuntimeAdapter;

    assert_eq!(
        adapter.resume(&AttemptId::new(" exact-試行-e\u{301} ").unwrap()),
        None
    );
}

// Synthetic Result-carrier checks only: no start invocation, provider
// execution, trusted capsule/workspace construction or enforcement is claimed.
type StartResult = Result<FixtureAttemptHandle, FailureClass>;

const ALL_FAILURE_CLASSES: [FailureClass; 12] = [
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

fn assert_failure_class_exhaustive(value: FailureClass) {
    match value {
        FailureClass::RateLimited
        | FailureClass::SessionExhausted
        | FailureClass::AuthRequired
        | FailureClass::ProviderDown
        | FailureClass::Timeout
        | FailureClass::SandboxDenied
        | FailureClass::SafetyCheckPending
        | FailureClass::PolicyBlocked
        | FailureClass::RuntimeCrash
        | FailureClass::InvalidOutput
        | FailureClass::UserCancelled
        | FailureClass::Unknown => {}
    }
}

#[test]
fn synthetic_start_result_preserves_accepted_handle() {
    let _: fn(
        &TrackingRuntimeAdapter,
        &RuntimeCapsuleFamily,
        &WorkspaceHandle,
        &FixtureExecutionPolicy,
    ) -> StartResult = TrackingRuntimeAdapter::start;

    let result: StartResult = Ok(FixtureAttemptHandle("started-attempt"));

    assert_eq!(result, Ok(FixtureAttemptHandle("started-attempt")));
    assert_eq!(result.err(), None);
}

#[test]
fn synthetic_start_result_preserves_every_failure_without_handle() {
    for (index, class) in ALL_FAILURE_CLASSES.into_iter().enumerate() {
        assert_failure_class_exhaustive(class);
        let result: StartResult = Err(class);

        assert_eq!(result, Err(ALL_FAILURE_CLASSES[index]));
        assert_eq!(result.as_ref().ok(), None, "{class:?} supplied a handle");
        assert_eq!(result.err().map(FailureClass::as_str), Some(class.as_str()));
        for other in &ALL_FAILURE_CLASSES[index + 1..] {
            assert_ne!(Err::<FixtureAttemptHandle, _>(class), Err(*other));
        }
    }
}

#[test]
fn synthetic_start_result_keeps_unknown_and_named_distinctions() {
    let unknown: StartResult = Err(FailureClass::Unknown);
    assert_eq!(unknown, Err(FailureClass::Unknown));
    assert_eq!(unknown.err().map(FailureClass::as_str), Some("UNKNOWN"));

    for (left, right) in [
        (FailureClass::RateLimited, FailureClass::PolicyBlocked),
        (
            FailureClass::SafetyCheckPending,
            FailureClass::PolicyBlocked,
        ),
        (FailureClass::AuthRequired, FailureClass::ProviderDown),
    ] {
        let left_result: StartResult = Err(left);
        let right_result: StartResult = Err(right);
        assert_ne!(left_result, right_result);
        assert_eq!(left_result, Err(left));
        assert_eq!(right_result, Err(right));
    }
}
