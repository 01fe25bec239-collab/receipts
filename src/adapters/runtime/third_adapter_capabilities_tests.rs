//! A test-only third adapter returns identified capabilities through the existing
//! `RuntimeAdapter::capabilities()` type. It uses only crate-root public items; no
//! core file knows its name. This is not a conformance claim for a real adapter.

use std::convert::Infallible;

use receipts_runtime_bindings::RuntimeCapsuleFamily;
use receipts_workspace_execution::WorkspaceHandle;

use crate::{
    CapabilityStatus, ExtensionCapabilityReport, FailureClass, RawFailure, RuntimeAdapter,
    RuntimeAuthStatus, RuntimeCapabilities,
};

struct ThirdAdapter;

impl RuntimeAdapter for ThirdAdapter {
    type HealthReport = ();
    type Models = ();
    type ExecutionPolicy = ();
    // Uninhabited: `start` can only refuse, so no fake attempt can exist.
    type AttemptHandle = Infallible;
    type AttemptEvent = ();
    type EventStream<'a> = ();
    type AttemptResult = ();
    type CancelReason = ();

    fn runtime_id(&self) -> &str {
        "third-adapter.test"
    }

    fn health(&self) {}

    fn authenticate_status(&self) -> RuntimeAuthStatus {
        RuntimeAuthStatus::Unknown
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        let report = ExtensionCapabilityReport::new(
            self.runtime_id(),
            [
                ("streaming", CapabilityStatus::Supported),
                ("resume", CapabilityStatus::Unsupported),
                ("structured-output", CapabilityStatus::Unknown),
                (" spaced name ", CapabilityStatus::Supported),
            ],
        );
        RuntimeCapabilities::Extension(report.unwrap())
    }

    fn models(&self) {}

    fn start(
        &self,
        _task: &RuntimeCapsuleFamily,
        _workspace: &WorkspaceHandle,
        _policy: &(),
    ) -> Result<Infallible, FailureClass> {
        Err(FailureClass::PolicyBlocked)
    }

    fn stream_events<'a>(&'a self, handle: &'a Infallible) {
        match *handle {}
    }

    fn collect_result(&self, handle: &Infallible) {
        match *handle {}
    }

    fn cancel(&self, handle: &Infallible, _reason: &()) {
        match *handle {}
    }

    fn classify_failure(&self, _error: &RawFailure) -> FailureClass {
        FailureClass::Unknown
    }
}

/// Generic consumer: no adapter or capability name appears in its control flow.
fn observed<A: RuntimeAdapter>(adapter: &A) -> Option<(String, Vec<(String, CapabilityStatus)>)> {
    match adapter.capabilities() {
        RuntimeCapabilities::Extension(report) => Some((
            report.runtime_id().to_owned(),
            report
                .entries()
                .map(|(name, status)| (name.to_owned(), status))
                .collect(),
        )),
        RuntimeCapabilities::Codex(_)
        | RuntimeCapabilities::Claude(_)
        | RuntimeCapabilities::Unknown => None,
    }
}

#[test]
fn third_adapter_capabilities_flow_through_the_existing_trait_return() {
    let _: fn(&ThirdAdapter) -> RuntimeCapabilities = ThirdAdapter::capabilities;

    let (id, entries) = observed(&ThirdAdapter).expect("extension report");
    assert_eq!(id, ThirdAdapter.runtime_id());
    assert_ne!(id, "codex");
    assert_ne!(id, "claude");
    assert_eq!(
        entries,
        [
            (" spaced name ".to_owned(), CapabilityStatus::Supported),
            ("resume".to_owned(), CapabilityStatus::Unsupported),
            ("streaming".to_owned(), CapabilityStatus::Supported),
            ("structured-output".to_owned(), CapabilityStatus::Unknown),
        ]
    );

    let RuntimeCapabilities::Extension(report) = ThirdAdapter.capabilities() else {
        panic!("third adapter did not return the generic report");
    };
    assert_eq!(report.status("streaming"), CapabilityStatus::Supported);
    assert_eq!(report.status("resume"), CapabilityStatus::Unsupported);
    assert_eq!(
        report.get("structured-output"),
        Some(CapabilityStatus::Unknown)
    );
    assert_eq!(report.get("never-observed"), None);
    assert_eq!(report.status("never-observed"), CapabilityStatus::Unknown);
    assert_eq!(
        ThirdAdapter.capabilities(),
        RuntimeCapabilities::Extension(report)
    );
}

#[test]
fn start_only_refuses_and_can_never_yield_an_attempt() {
    // The handle type is uninhabited, so `Ok` is unrepresentable for this adapter.
    let _: fn(
        &ThirdAdapter,
        &RuntimeCapsuleFamily,
        &WorkspaceHandle,
        &(),
    ) -> Result<Infallible, FailureClass> = ThirdAdapter::start;
    assert_eq!(
        ThirdAdapter.resume(&crate::AttemptId::new("a").unwrap()),
        None
    );
}
