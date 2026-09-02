//! The frozen [`RuntimeAdapter`] semantic interface.
//!
//! This module declares vocabulary only. It implements no runtime behavior,
//! grants no execution authority, and defines no externally owned contracts.

use crate::{FailureClass, RuntimeAuthStatus};

/// Interface shared by every agent runtime.
pub trait RuntimeAdapter {
    /// Unbound placeholder for the frozen health report.
    type HealthReport;

    /// Unbound placeholder for the frozen runtime capabilities.
    type RuntimeCapabilities;

    /// Unbound placeholder for the frozen `ModelRef[] | UNKNOWN` result.
    ///
    /// This declaration defines neither `ModelRef` nor model-registry behavior.
    type Models;

    /// Physical binding placeholder for the frozen capsule family.
    ///
    /// `Capsule` is not a new Receipts contract. Its eventual binding must
    /// preserve the authority of `TaskCapsule`, `RepairCapsule`, and
    /// `ReviewCapsule`; this crate defines none of those contracts' shapes.
    /// The one frozen `start` semantic remains one operation.
    type Capsule;

    /// Unbound placeholder for the externally owned frozen `WorkspaceHandle`.
    type WorkspaceHandle;

    /// Unbound placeholder for the frozen execution policy.
    ///
    /// Its eventual binding must preserve the established execution-policy
    /// semantics, including A4 read-only behavior. This declaration neither
    /// defines that shape nor claims read-only enforcement.
    type ExecutionPolicy;

    /// Unbound placeholder for the frozen attempt handle.
    type AttemptHandle;

    /// Unbound placeholder for the frozen `AttemptEvent` semantic.
    type AttemptEvent;

    /// Physical placeholder for the frozen asynchronous event stream.
    ///
    /// The frozen contract is asynchronous event streaming. This interface-only
    /// associated type does not claim synchronous iteration or weaken delivery
    /// semantics, and it has no `Iterator` or `IntoIterator` bound. A later
    /// concrete implementation/conformance slice must bind the actual
    /// asynchronous stream whose events have the frozen `AttemptEvent` semantic.
    type EventStream<'a>
    where
        Self: 'a;

    /// Unbound placeholder for the frozen attempt result.
    type AttemptResult;

    /// Unbound placeholder for a raw runtime failure.
    type RawFailure;

    /// Unbound placeholder for the frozen attempt identifier.
    type AttemptId;

    /// Unbound placeholder for the frozen cancellation reason.
    type CancelReason;

    fn runtime_id(&self) -> &str;

    fn health(&self) -> Self::HealthReport;

    fn authenticate_status(&self) -> RuntimeAuthStatus;

    fn capabilities(&self) -> Self::RuntimeCapabilities;

    fn models(&self) -> Self::Models;

    /// Starts one frozen capsule-family operation after core dispatch admission.
    ///
    /// This interface operation grants no execution authority. The orchestrator
    /// dispatch gate must authorize dispatch before this boundary is reached.
    fn start(
        &self,
        task: &Self::Capsule,
        workspace: &Self::WorkspaceHandle,
        policy: &Self::ExecutionPolicy,
    ) -> Self::AttemptHandle;

    fn stream_events<'a>(&'a self, handle: &'a Self::AttemptHandle) -> Self::EventStream<'a>;

    fn collect_result(&self, handle: &Self::AttemptHandle) -> Self::AttemptResult;

    fn cancel(&self, handle: &Self::AttemptHandle, reason: &Self::CancelReason);

    fn classify_failure(&self, error: &Self::RawFailure) -> FailureClass;

    /// Attempts to resume a prior attempt when this adapter supports resume.
    ///
    /// `None` means only that this adapter does not support resume. It never
    /// means success without a handle, a swallowed failure, or a missing
    /// provider session. Supporting adapters must deliberately override this
    /// default, and no core operation may depend on resume support.
    fn resume(&self, _attempt_id: &Self::AttemptId) -> Option<Self::AttemptHandle> {
        None
    }
}
