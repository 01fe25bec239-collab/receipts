//! Reusable lifecycle controls. Real CLI eligibility is a caller prerequisite;
//! a synthetic pass is never provider isolation or production qualification.
use crate::registry::{AttemptFailure, DynAttemptEvent, RuntimeEventStream};
use crate::{CancelReason, FailureClass, RawFailure, RuntimeAdapter, RuntimeExecutionPolicy};
use receipts_runtime_bindings::RuntimeCapsuleFamily;
use receipts_workspace_execution::WorkspaceHandle;
use std::{future::poll_fn, pin::pin};

pub struct ConformanceCase<'a> {
    pub task: &'a RuntimeCapsuleFamily,
    pub workspace: &'a WorkspaceHandle,
    pub policy: &'a RuntimeExecutionPolicy,
    pub cancel: Option<CancelReason>,
    pub expected_failure: Option<FailureClass>,
    pub refused_start: bool,
    pub classification: Option<(&'a RawFailure, FailureClass)>,
}
#[derive(Debug, PartialEq, Eq)]
pub struct ConformanceReport {
    pub start: bool,
    pub stream: bool,
    pub collection: bool,
    pub classification: bool,
    pub isolation_and_cleanup: bool,
}
impl ConformanceReport {
    pub fn passed(&self) -> bool {
        self.start
            && self.stream
            && self.collection
            && self.classification
            && self.isolation_and_cleanup
    }
}
/// `verify` runs after the handle is dropped. The fixture/qualification owner
/// must inspect actual filesystem/process facts, including reviewer writes;
/// Runtime cannot infer those facts from provider prose or an exit code.
/// The caller must also bound this future when qualifying an unknown adapter.
pub async fn run_conformance<A>(
    adapter: &A,
    case: ConformanceCase<'_>,
    verify: impl FnOnce() -> bool,
) -> ConformanceReport
where
    A: RuntimeAdapter<ExecutionPolicy = RuntimeExecutionPolicy, CancelReason = CancelReason>,
    A::AttemptResult: AttemptFailure,
    A::AttemptEvent: Into<DynAttemptEvent>,
    for<'a> A::EventStream<'a>: RuntimeEventStream<Event = A::AttemptEvent>,
{
    let classification = case
        .classification
        .is_none_or(|(raw, expected)| adapter.classify_failure(raw) == expected);
    let handle = match adapter.start(case.task, case.workspace, case.policy) {
        Ok(handle) => handle,
        Err(failure) => {
            return ConformanceReport {
                start: case.refused_start && Some(failure) == case.expected_failure,
                stream: case.refused_start,
                collection: case.refused_start,
                classification,
                isolation_and_cleanup: verify(),
            };
        }
    };
    if let Some(reason) = case.cancel {
        adapter.cancel(&handle, &reason);
    }
    let mut valid = true;
    let mut records = 0;
    {
        let mut stream = pin!(adapter.stream_events(&handle));
        while let Some(event) = poll_fn(|cx| stream.as_mut().poll_event(cx)).await {
            match event.into() {
                DynAttemptEvent::Record(_) => records += 1,
                DynAttemptEvent::Unavailable(_) => valid = false,
            }
        }
        // Terminal delivery must be stable, with no later records.
        valid &= poll_fn(|cx| stream.as_mut().poll_event(cx)).await.is_none();
    }
    let result = adapter.collect_result(&handle);
    let collection = result.failure() == case.expected_failure;
    drop(handle);
    ConformanceReport {
        start: !case.refused_start,
        stream: valid && (case.cancel.is_some() || records > 0),
        collection,
        classification,
        isolation_and_cleanup: verify(),
    }
}
