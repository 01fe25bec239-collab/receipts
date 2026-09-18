//! Test-only proof that the [`HostAdapter`] boundary is implementable in
//! full using the canonical event, report, and `std` facilities, for all
//! three host identities, without any concrete host behavior.
//!
//! The dummy below is deliberately fake: every placeholder slot binds a
//! unit struct that carries no semantics. Capabilities returns a fixed
//! structural canonical report; no physical evidence is observed. This proves
//! the nine semantic operations are representable and callable, nothing more.

use std::collections::HashSet;
use std::future::{Ready, ready};
use std::task::{Context, Poll, Waker};

use super::*;

/// Test-only stand-in bound to every placeholder slot. It is not an
/// implementation of any frozen contract; it exists so the interface can
/// be exercised end to end.
struct Placeholder;

/// Minimal test-only adapter: identity in, translation boundary out.
struct TestAdapter {
    id: HostId,
}

impl HostAdapter for TestAdapter {
    type DetectOutcome = Placeholder;
    type InstallPlan = Placeholder;
    type InstallOutcome = Placeholder;
    type CoreHandle = Placeholder;
    type EmitOutcome = Placeholder;
    type CoreView = Placeholder;
    type PresentOutcome = Placeholder;
    type UserPrompt = Placeholder;
    type UserResponse = Placeholder;
    type UserInputPending = Ready<Placeholder>;
    type ShutdownReason = Placeholder;
    type ShutdownOutcome = Placeholder;

    fn id(&self) -> HostId {
        self.id
    }

    fn detect(&self) -> Placeholder {
        Placeholder
    }

    fn install(&self, _plan: &Placeholder) -> Placeholder {
        Placeholder
    }

    fn start(&self) -> Placeholder {
        Placeholder
    }

    fn emit(&self, _event: &NormalizedHostEvent) -> Placeholder {
        Placeholder
    }

    fn present(&self, _view: &Placeholder) -> Placeholder {
        Placeholder
    }

    fn request_user_input(&mut self, _prompt: Placeholder) -> Ready<Placeholder> {
        ready(Placeholder)
    }

    fn capabilities(&self) -> HostCapabilityReport {
        canonical_report()
    }

    fn shutdown(self, _reason: Placeholder) -> Placeholder {
        Placeholder
    }
}

/// Drives all nine operations through one generic code path. Because the
/// path is generic over `A: HostAdapter`, identical code serves every host
/// identity, proving the boundary requires no concrete host behavior.
fn exercise_all_operations<A: HostAdapter>(
    mut adapter: A,
    plan: A::InstallPlan,
    event: NormalizedHostEvent,
    view: A::CoreView,
    prompt: A::UserPrompt,
    reason: A::ShutdownReason,
) -> HostId {
    let reported_id = adapter.id();

    let _detected = adapter.detect();
    let _installed = adapter.install(&plan);
    let _started = adapter.start();
    let _emitted = adapter.emit(&event);
    let _presented = adapter.present(&view);

    // The pending user-input future is generic and may not be Unpin, so it
    // is pinned with std facilities only and polled once to completion via
    // the no-op waker — no async runtime, no external dependency.
    let mut pending = Box::pin(adapter.request_user_input(prompt));
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(
        matches!(pending.as_mut().poll(&mut context), Poll::Ready(_)),
        "the pending user-input future must complete"
    );

    let report: HostCapabilityReport = adapter.capabilities();
    assert_eq!(report, canonical_report());
    let _shutdown = adapter.shutdown(reason);

    reported_id
}

/// The complete interface is implementable, all nine operations are
/// represented, and all three host identities are usable through the same
/// generic driver.
#[test]
fn complete_interface_is_implementable_for_every_host_identity() {
    let expected = [
        (HostId::ClaudeCode, "CLAUDE_CODE"),
        (HostId::Codex, "CODEX"),
        (HostId::Headless, "HEADLESS"),
    ];

    let mut distinct_reported: HashSet<HostId> = HashSet::new();
    for (id, diagnostic) in expected {
        let adapter = TestAdapter { id };
        let reported = exercise_all_operations(
            adapter,
            Placeholder,
            canonical_event(),
            Placeholder,
            Placeholder,
            Placeholder,
        );
        assert_eq!(reported, id, "{diagnostic} identity must round-trip");
        assert_eq!(reported.as_str(), diagnostic);
        distinct_reported.insert(reported);
    }
    assert_eq!(distinct_reported.len(), 3);
}

/// Each of the nine semantic operations is individually present on the
/// trait surface and callable against the dummy implementation.
#[test]
fn all_nine_operations_are_individually_callable() {
    let mut adapter = TestAdapter {
        id: HostId::Headless,
    };

    let _id = adapter.id();
    let _detect = adapter.detect();
    let plan = Placeholder;
    let _install = adapter.install(&plan);
    let _start = adapter.start();
    let event = canonical_event();
    let _emit = adapter.emit(&event);
    let view = Placeholder;
    let _present = adapter.present(&view);

    let response = {
        let mut pending = Box::pin(adapter.request_user_input(Placeholder));
        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        match pending.as_mut().poll(&mut context) {
            Poll::Ready(response) => response,
            Poll::Pending => panic!("dummy user-input future must resolve immediately"),
        }
    };
    let _response_guard = response;

    let capabilities: HostCapabilityReport = adapter.capabilities();
    assert_eq!(capabilities, canonical_report());
    let _shutdown = adapter.shutdown(Placeholder);
}

/// The adapter implementation needs only Host contracts and std facilities.
#[test]
fn interface_uses_host_contracts_and_std() {
    fn assert_host_adapter<A: HostAdapter>(_: &A) {}
    let adapter = TestAdapter { id: HostId::Codex };
    assert_host_adapter(&adapter);
}

/// Fixed structural fixture only; no claim about a physical host event.
pub(super) fn canonical_event() -> NormalizedHostEvent {
    use receipts_orchestration::orchestration::{
        OrchestrationDateTimeV1, OrchestrationJsonObjectV1,
    };

    NormalizedHostEvent::try_new(NormalizedHostEventInputs {
        event_id: NormalizedHostEventId::try_new("Z123456789ABCDEFGHJKMNPQRS").unwrap(),
        event_type: NormalizedHostEventType::HostSessionStarted,
        host: HostId::Headless.into(),
        host_session_id: "session".to_owned(),
        project_id: None,
        occurred_at: OrchestrationDateTimeV1::try_new("2026-09-12T00:00:00Z").unwrap(),
        payload: OrchestrationJsonObjectV1::default(),
        raw_ref: None,
        confidence: NormalizedHostEventConfidence::Observed,
    })
    .unwrap()
}

/// Fixed caller-supplied structural evidence only; no capability truth,
/// fingerprint validity, timestamp freshness, or physical probing is claimed.
pub(super) fn canonical_report() -> HostCapabilityReport {
    use receipts_orchestration::orchestration::OrchestrationDateTimeV1;

    let core =
        HostCapabilityReportNonTemporalCore::new(HostCapabilityReportNonTemporalCoreInputs {
            host_id: "synthetic".to_owned(),
            host_version: None,
            probe_status: HostCapabilityProbeStatus::Partial,
            validity_fingerprint: None,
            hook_definition_digest: None,
            relevant_config_digest: None,
            stale_reason: HostCapabilityStaleReason::None,
            plugin_supported: None,
            plugin_installed: None,
            manifest_path: None,
            supports_skills: None,
            supports_commands: None,
            supports_subagents: None,
            supports_mcp: None,
            hooks_supported: None,
            hooks_configured: None,
            hook_trust_required: None,
            hooks_trusted: None,
            hooks_enabled: None,
            hooks_allowed_by_admin_policy: None,
            hook_events: None,
            blocking_hook_events: None,
            hook_coverage_class: HostCapabilityHookCoverageClass::Unknown,
            required_hook_coverage_satisfied: None,
            selected_mode: HostCapabilitySelectedMode::Supervised,
            mode_override: None,
            inactive_reason: None,
            plugin_data_path: None,
            sandbox_modes: None,
            evidence_label: None,
            source_claim_id: None,
        })
        .unwrap();
    HostCapabilityReport::new(
        core,
        OrchestrationDateTimeV1::try_new("2026-09-12T00:00:00Z").unwrap(),
        OrchestrationDateTimeV1::try_new("2026-09-12T00:00:00Z").unwrap(),
    )
}

#[test]
fn canonical_event_input_and_report_output_are_bound_on_adapter_surface() {
    let source = include_str!("adapter.rs");
    let surface = source.split("pub trait HostAdapter {").nth(1).unwrap();
    assert!(!surface.contains("type NormalizedHostEvent;"));
    assert!(!surface.contains("type HostCapabilityReport;"));
    assert!(surface.contains("fn capabilities(&self) -> HostCapabilityReport;"));
    assert!(surface.contains("fn emit(&self, event: &NormalizedHostEvent) -> Self::EmitOutcome;"));
    for name in [
        "DetectOutcome",
        "InstallPlan",
        "InstallOutcome",
        "CoreHandle",
        "EmitOutcome",
        "CoreView",
        "PresentOutcome",
        "UserPrompt",
        "UserResponse",
        "ShutdownReason",
        "ShutdownOutcome",
    ] {
        assert!(surface.contains(&format!("type {name};")));
    }
    assert!(surface.contains("type UserInputPending: Future<Output = Self::UserResponse>;"));
    assert!(!surface.contains("source_class"));
    assert!(!surface.contains("validate_normalized_host_event_source("));
    assert!(!surface.contains("source_class_allowed("));
}
