//! Test-only proof that the [`HostAdapter`] boundary is implementable in
//! full with `std` only, for all three host identities, without any
//! concrete Claude Code, Codex, or headless behavior.
//!
//! The dummy below is deliberately fake: every placeholder slot binds a
//! unit struct that carries no semantics, and every operation returns it
//! untouched. This demonstrates exactly what the interface slice claims —
//! the nine semantic operations are representable and callable — and
//! nothing more.

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
    type NormalizedHostEvent = Placeholder;
    type EmitOutcome = Placeholder;
    type CoreView = Placeholder;
    type PresentOutcome = Placeholder;
    type UserPrompt = Placeholder;
    type UserResponse = Placeholder;
    type UserInputPending = Ready<Placeholder>;
    type HostCapabilityReport = Placeholder;
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

    fn emit(&self, _event: &Placeholder) -> Placeholder {
        Placeholder
    }

    fn present(&self, _view: &Placeholder) -> Placeholder {
        Placeholder
    }

    fn request_user_input(&mut self, _prompt: Placeholder) -> Ready<Placeholder> {
        ready(Placeholder)
    }

    fn capabilities(&self) -> Placeholder {
        Placeholder
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
    event: A::NormalizedHostEvent,
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

    let _report = adapter.capabilities();
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
            Placeholder,
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
    let event = Placeholder;
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

    let _capabilities = adapter.capabilities();
    let _shutdown = adapter.shutdown(Placeholder);
}

/// The interface is implementable without importing anything beyond
/// `std`: this module itself compiles with zero external dependencies,
/// mirroring the crate manifest's empty dependency list.
#[test]
fn interface_requires_only_std() {
    // Compile-time statement: exercising the whole boundary needs only the
    // items re-exported from this crate plus std task/future facilities
    // imported above. Nothing else is available or required.
    fn assert_host_adapter<A: HostAdapter>(_: &A) {}
    let adapter = TestAdapter { id: HostId::Codex };
    assert_host_adapter(&adapter);
}
