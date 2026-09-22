use receipts_runtime_bindings::RuntimeCapsuleFamily;
use receipts_workspace_execution::WorkspaceHandle;

use crate::{
    AttemptId, CodexTaskExecutionError, FailureClass, RawFailure, RuntimeAdapter, RuntimeAuthStatus,
};

struct SurfaceWitness;
struct OpaqueEventStream;

impl RuntimeAdapter for SurfaceWitness {
    type HealthReport = ();
    type RuntimeCapabilities = ();
    type Models = ();
    type ExecutionPolicy = ();
    type AttemptHandle = ();
    type AttemptEvent = ();
    type EventStream<'a> = OpaqueEventStream;
    type AttemptResult = ();
    type CancelReason = ();

    fn runtime_id(&self) -> &str {
        "test"
    }

    fn health(&self) {}

    fn authenticate_status(&self) -> RuntimeAuthStatus {
        RuntimeAuthStatus::Unknown
    }

    fn capabilities(&self) {}

    fn models(&self) {}

    fn start(&self, _task: &RuntimeCapsuleFamily, _workspace: &WorkspaceHandle, _policy: &()) {}

    fn stream_events<'a>(&'a self, _handle: &'a ()) -> Self::EventStream<'a> {
        OpaqueEventStream
    }

    fn collect_result(&self, _handle: &()) {}

    fn cancel(&self, _handle: &(), _reason: &()) {}

    fn classify_failure(&self, _error: &RawFailure) -> FailureClass {
        FailureClass::Unknown
    }
}

#[test]
fn public_trait_preserves_typed_surface_and_optional_resume() {
    let adapter = SurfaceWitness;

    let auth: RuntimeAuthStatus = adapter.authenticate_status();
    let failure: FailureClass =
        adapter.classify_failure(&RawFailure::from(&CodexTaskExecutionError::EmptyPrompt));
    let _stream: OpaqueEventStream = adapter.stream_events(&());

    assert_eq!(auth, RuntimeAuthStatus::Unknown);
    assert_eq!(failure, FailureClass::Unknown);
    assert_eq!(
        adapter.resume(&AttemptId::new("surface-attempt").unwrap()),
        None,
        "None means unsupported resume"
    );
}

#[test]
fn declaration_has_exact_frozen_operation_set() {
    const EXPECTED: [&str; 11] = [
        "runtime_id",
        "health",
        "authenticate_status",
        "capabilities",
        "models",
        "start",
        "stream_events",
        "collect_result",
        "cancel",
        "classify_failure",
        "resume",
    ];

    let source = include_str!("adapter.rs");
    let observed: Vec<_> = source
        .lines()
        .filter_map(|line| {
            line.trim_start()
                .strip_prefix("fn ")?
                .split_once('(')
                .map(|(name, _)| name.split('<').next().unwrap_or(name))
        })
        .collect();

    assert_eq!(observed, EXPECTED);
}

#[test]
fn canonical_parameters_leave_only_the_other_associated_placeholders() {
    // These witnesses compile for every implementer without associated-type
    // equality constraints: no canonical parameter can be substituted by an adapter.
    fn parameters<A: RuntimeAdapter>() {
        let _: fn(
            &A,
            &RuntimeCapsuleFamily,
            &WorkspaceHandle,
            &A::ExecutionPolicy,
        ) -> A::AttemptHandle = A::start;
        let _: fn(&A, &crate::RawFailure) -> FailureClass = A::classify_failure;
        let _: fn(&A, &crate::AttemptId) -> Option<A::AttemptHandle> = A::resume;
    }
    parameters::<SurfaceWitness>();

    let source = include_str!("adapter.rs");
    let observed: Vec<_> = source
        .lines()
        .filter_map(|line| line.trim_start().strip_prefix("type "))
        .collect();
    assert_eq!(
        observed,
        [
            "HealthReport;",
            "RuntimeCapabilities;",
            "Models;",
            "ExecutionPolicy;",
            "AttemptHandle;",
            "AttemptEvent;",
            "EventStream<'a>",
            "AttemptResult;",
            "CancelReason;",
        ]
    );
    assert!(source.contains("fn classify_failure(&self, error: &RawFailure) -> FailureClass;"));
    assert!(
        source.contains("fn resume(&self, _attempt_id: &AttemptId) -> Option<Self::AttemptHandle>")
    );
}

#[test]
fn production_surface_has_no_shadow_external_contracts() {
    let source = include_str!("adapter.rs");
    let forbidden = [
        "struct WorkspaceHandle",
        "enum WorkspaceHandle",
        "struct TaskCapsule",
        "enum TaskCapsule",
        "struct RepairCapsule",
        "enum RepairCapsule",
        "struct ReviewCapsule",
        "enum ReviewCapsule",
        "struct DispatchAdmissionDecision",
        "enum DispatchAdmissionDecision",
        "receipts_m0_contracts",
        "start_task",
        "start_repair",
        "start_review",
    ];

    for declaration in forbidden {
        assert!(!source.contains(declaration), "found {declaration}");
    }
}
