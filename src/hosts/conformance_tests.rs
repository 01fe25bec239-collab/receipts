use std::cell::RefCell;
use std::future::{Future, Ready, ready};
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use super::{HostAdapter, HostId};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Id,
    Detect,
    Install,
    Start,
    Emit,
    Present,
    RequestUserInput,
    Capabilities,
    Shutdown,
}

struct FixtureDetectOutcome;
struct FixtureInstallPlan;
struct FixtureInstallOutcome;
struct FixtureCoreHandle;
struct FixtureNormalizedHostEvent;
struct FixtureEmitOutcome;
struct FixtureCoreView;
struct FixturePresentOutcome;
struct FixtureUserPrompt;
struct FixtureUserResponse;
struct FixtureHostCapabilityReport;
struct FixtureShutdownReason;
struct FixtureShutdownOutcome;

struct ConformanceAdapter {
    id: HostId,
    trace: Rc<RefCell<Vec<Operation>>>,
}

impl ConformanceAdapter {
    fn record(&self, operation: Operation) {
        self.trace.borrow_mut().push(operation);
    }
}

impl HostAdapter for ConformanceAdapter {
    type DetectOutcome = FixtureDetectOutcome;
    type InstallPlan = FixtureInstallPlan;
    type InstallOutcome = FixtureInstallOutcome;
    type CoreHandle = FixtureCoreHandle;
    type NormalizedHostEvent = FixtureNormalizedHostEvent;
    type EmitOutcome = FixtureEmitOutcome;
    type CoreView = FixtureCoreView;
    type PresentOutcome = FixturePresentOutcome;
    type UserPrompt = FixtureUserPrompt;
    type UserResponse = FixtureUserResponse;
    type UserInputPending = Ready<FixtureUserResponse>;
    type HostCapabilityReport = FixtureHostCapabilityReport;
    type ShutdownReason = FixtureShutdownReason;
    type ShutdownOutcome = FixtureShutdownOutcome;

    fn id(&self) -> HostId {
        self.record(Operation::Id);
        self.id
    }

    fn detect(&self) -> Self::DetectOutcome {
        self.record(Operation::Detect);
        FixtureDetectOutcome
    }

    fn install(&self, _plan: &Self::InstallPlan) -> Self::InstallOutcome {
        self.record(Operation::Install);
        FixtureInstallOutcome
    }

    fn start(&self) -> Self::CoreHandle {
        self.record(Operation::Start);
        FixtureCoreHandle
    }

    fn emit(&self, _event: &Self::NormalizedHostEvent) -> Self::EmitOutcome {
        self.record(Operation::Emit);
        FixtureEmitOutcome
    }

    fn present(&self, _view: &Self::CoreView) -> Self::PresentOutcome {
        self.record(Operation::Present);
        FixturePresentOutcome
    }

    fn request_user_input(&mut self, _prompt: Self::UserPrompt) -> Self::UserInputPending {
        self.record(Operation::RequestUserInput);
        ready(FixtureUserResponse)
    }

    fn capabilities(&self) -> Self::HostCapabilityReport {
        self.record(Operation::Capabilities);
        FixtureHostCapabilityReport
    }

    fn shutdown(self, _reason: Self::ShutdownReason) -> Self::ShutdownOutcome {
        self.record(Operation::Shutdown);
        FixtureShutdownOutcome
    }
}

fn exercise_all_operations<A: HostAdapter>(
    mut adapter: A,
    plan: A::InstallPlan,
    event: A::NormalizedHostEvent,
    view: A::CoreView,
    prompt: A::UserPrompt,
    reason: A::ShutdownReason,
) -> HostId {
    let id = <A as HostAdapter>::id(&adapter);
    let _ = <A as HostAdapter>::detect(&adapter);
    let _ = <A as HostAdapter>::install(&adapter, &plan);
    let _ = <A as HostAdapter>::start(&adapter);
    let _ = <A as HostAdapter>::emit(&adapter, &event);
    let _ = <A as HostAdapter>::present(&adapter, &view);

    let mut pending = Box::pin(<A as HostAdapter>::request_user_input(&mut adapter, prompt));
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(matches!(
        pending.as_mut().poll(&mut context),
        Poll::Ready(_)
    ));

    let _ = <A as HostAdapter>::capabilities(&adapter);
    let _ = <A as HostAdapter>::shutdown(adapter, reason);
    id
}

#[test]
fn real_trait_records_all_nine_operations_in_order() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let expected_id = HostId::Codex;
    let adapter = ConformanceAdapter {
        id: expected_id,
        trace: Rc::clone(&trace),
    };

    let actual_id = exercise_all_operations(
        adapter,
        FixtureInstallPlan,
        FixtureNormalizedHostEvent,
        FixtureCoreView,
        FixtureUserPrompt,
        FixtureShutdownReason,
    );

    assert_eq!(actual_id, expected_id);
    assert_eq!(
        trace.borrow().as_slice(),
        [
            Operation::Id,
            Operation::Detect,
            Operation::Install,
            Operation::Start,
            Operation::Emit,
            Operation::Present,
            Operation::RequestUserInput,
            Operation::Capabilities,
            Operation::Shutdown,
        ]
    );
}
