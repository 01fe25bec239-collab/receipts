//! Runtime-keyed, type-erased adapter registry. Registration and dispatch grant
//! no admission, routing, credential or execution authority: the core dispatch
//! gate must still authorize every `start`. Any `RuntimeAdapter` with the
//! vendor-neutral bindings below registers without a core edit.

use std::{
    any::Any,
    collections::BTreeMap,
    error::Error,
    fmt,
    future::{Future, poll_fn},
    pin::Pin,
    task::{Context, Poll},
};

use receipts_runtime_bindings::RuntimeCapsuleFamily;
use receipts_workspace_execution::WorkspaceHandle;

use crate::{
    AttemptId, CancelReason, FailureClass, RawFailure, RuntimeAdapter, RuntimeAuthStatus,
    RuntimeCapabilities, RuntimeExecutionPolicy, RuntimeObservation,
};

/// Asynchronous attempt events. Implementations must wake the registered task
/// when a later poll can make progress; returning `Pending` without arranging
/// a wakeup is a contract violation.
pub trait RuntimeEventStream {
    type Event;
    fn poll_event(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Event>>;
}

/// Vendor-neutral failure view of an adapter's collected result.
pub trait AttemptFailure {
    /// `None` means no failure observed, never Receipts acceptance.
    fn failure(&self) -> Option<FailureClass>;
}
impl<T: AttemptFailure> AttemptFailure for Result<T, FailureClass> {
    fn failure(&self) -> Option<FailureClass> {
        match self {
            Ok(result) => result.failure(),
            Err(class) => Some(*class),
        }
    }
}

/// Erased event: exact record bytes, or the adapter's own payload-free
/// unavailability error retained for explicit downcast.
pub enum DynAttemptEvent {
    Record(Vec<u8>),
    Unavailable(Box<dyn Any + Send>),
}

/// Non-cloneable erased handle. Dropping it drops the concrete handle, so the
/// adapter's own Drop cleanup still runs.
pub struct DynAttemptHandle {
    runtime_id: Box<str>,
    inner: Box<dyn Any + Send + Sync>,
}
impl DynAttemptHandle {
    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.inner.downcast_ref()
    }
}

/// Erased collected result; Debug is omitted because results carry payloads.
pub struct DynAttemptResult {
    failure: Option<FailureClass>,
    inner: Box<dyn Any + Send>,
}
impl DynAttemptResult {
    pub fn failure(&self) -> Option<FailureClass> {
        self.failure
    }
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.inner.downcast_ref()
    }
}

/// Erased event stream borrowing its adapter and handle.
pub struct DynEventStream<'a>(Pin<Box<dyn RuntimeEventStream<Event = DynAttemptEvent> + 'a>>);
impl DynEventStream<'_> {
    pub fn next_event(&mut self) -> impl Future<Output = Option<DynAttemptEvent>> + '_ {
        poll_fn(|cx| self.0.as_mut().poll_event(cx))
    }
}
impl RuntimeEventStream for DynEventStream<'_> {
    type Event = DynAttemptEvent;
    fn poll_event(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<DynAttemptEvent>> {
        self.get_mut().0.as_mut().poll_event(cx)
    }
}

struct Erased<S>(Pin<Box<S>>);
impl<S: RuntimeEventStream> RuntimeEventStream for Erased<S>
where
    S::Event: Into<DynAttemptEvent>,
{
    type Event = DynAttemptEvent;
    fn poll_event(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<DynAttemptEvent>> {
        self.get_mut()
            .0
            .as_mut()
            .poll_event(cx)
            .map(|event| event.map(Into::into))
    }
}

/// The handle belongs to another runtime or adapter type; nothing was invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleMismatch;
impl fmt::Display for HandleMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("attempt handle does not belong to this runtime adapter")
    }
}
impl Error for HandleMismatch {}

/// Object-safe frozen surface. Implemented for every `RuntimeAdapter` whose
/// policy, observation and cancellation bindings are the vendor-neutral types.
pub trait DynRuntimeAdapter: Send + Sync {
    fn runtime_id(&self) -> &str;
    fn health(&self) -> RuntimeObservation;
    fn authenticate_status(&self) -> RuntimeAuthStatus;
    fn capabilities(&self) -> RuntimeCapabilities;
    fn models(&self) -> RuntimeObservation;
    fn start(
        &self,
        task: &RuntimeCapsuleFamily,
        workspace: &WorkspaceHandle,
        policy: &RuntimeExecutionPolicy,
    ) -> Result<DynAttemptHandle, FailureClass>;
    fn stream_events<'a>(
        &'a self,
        handle: &'a DynAttemptHandle,
    ) -> Result<DynEventStream<'a>, HandleMismatch>;
    fn collect_result(&self, handle: &DynAttemptHandle)
    -> Result<DynAttemptResult, HandleMismatch>;
    fn cancel(
        &self,
        handle: &DynAttemptHandle,
        reason: &CancelReason,
    ) -> Result<(), HandleMismatch>;
    fn classify_failure(&self, error: &RawFailure) -> FailureClass;
    fn resume(&self, attempt_id: &AttemptId) -> Option<DynAttemptHandle>;
}

impl<A> DynRuntimeAdapter for A
where
    A: RuntimeAdapter<
            HealthReport = RuntimeObservation,
            Models = RuntimeObservation,
            ExecutionPolicy = RuntimeExecutionPolicy,
            CancelReason = CancelReason,
        > + Send
        + Sync
        + 'static,
    A::AttemptHandle: Send + Sync + 'static,
    A::AttemptResult: AttemptFailure + Send + 'static,
    A::AttemptEvent: Into<DynAttemptEvent>,
    for<'a> A::EventStream<'a>: RuntimeEventStream<Event = A::AttemptEvent>,
{
    fn runtime_id(&self) -> &str {
        RuntimeAdapter::runtime_id(self)
    }
    fn health(&self) -> RuntimeObservation {
        RuntimeAdapter::health(self)
    }
    fn authenticate_status(&self) -> RuntimeAuthStatus {
        RuntimeAdapter::authenticate_status(self)
    }
    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeAdapter::capabilities(self)
    }
    fn models(&self) -> RuntimeObservation {
        RuntimeAdapter::models(self)
    }
    fn start(
        &self,
        task: &RuntimeCapsuleFamily,
        workspace: &WorkspaceHandle,
        policy: &RuntimeExecutionPolicy,
    ) -> Result<DynAttemptHandle, FailureClass> {
        let handle = RuntimeAdapter::start(self, task, workspace, policy)?;
        Ok(self.erase(handle))
    }
    fn stream_events<'a>(
        &'a self,
        handle: &'a DynAttemptHandle,
    ) -> Result<DynEventStream<'a>, HandleMismatch> {
        let stream = RuntimeAdapter::stream_events(self, self.typed(handle)?);
        Ok(DynEventStream(Box::pin(Erased(Box::pin(stream)))))
    }
    fn collect_result(
        &self,
        handle: &DynAttemptHandle,
    ) -> Result<DynAttemptResult, HandleMismatch> {
        let result = RuntimeAdapter::collect_result(self, self.typed(handle)?);
        Ok(DynAttemptResult {
            failure: result.failure(),
            inner: Box::new(result),
        })
    }
    fn cancel(
        &self,
        handle: &DynAttemptHandle,
        reason: &CancelReason,
    ) -> Result<(), HandleMismatch> {
        RuntimeAdapter::cancel(self, self.typed(handle)?, reason);
        Ok(())
    }
    fn classify_failure(&self, error: &RawFailure) -> FailureClass {
        RuntimeAdapter::classify_failure(self, error)
    }
    fn resume(&self, attempt_id: &AttemptId) -> Option<DynAttemptHandle> {
        RuntimeAdapter::resume(self, attempt_id).map(|handle| self.erase(handle))
    }
}

trait Erase: RuntimeAdapter {
    fn erase(&self, handle: Self::AttemptHandle) -> DynAttemptHandle;
    fn typed<'a>(
        &self,
        handle: &'a DynAttemptHandle,
    ) -> Result<&'a Self::AttemptHandle, HandleMismatch>;
}
impl<A: RuntimeAdapter> Erase for A
where
    A::AttemptHandle: Send + Sync + 'static,
{
    fn erase(&self, handle: A::AttemptHandle) -> DynAttemptHandle {
        DynAttemptHandle {
            runtime_id: RuntimeAdapter::runtime_id(self).into(),
            inner: Box::new(handle),
        }
    }
    fn typed<'a>(
        &self,
        handle: &'a DynAttemptHandle,
    ) -> Result<&'a A::AttemptHandle, HandleMismatch> {
        if *handle.runtime_id != *RuntimeAdapter::runtime_id(self) {
            return Err(HandleMismatch);
        }
        handle.downcast_ref().ok_or(HandleMismatch)
    }
}

/// Registration refusal; nothing is retained or replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    EmptyRuntimeId,
    DuplicateRuntimeId,
}
impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyRuntimeId => "runtime identifier must not be empty",
            Self::DuplicateRuntimeId => "runtime identifier is already registered",
        })
    }
}
impl Error for RegistryError {}

/// Exact runtime-id → adapter map. No defaults, fallback, health or routing.
#[derive(Default)]
pub struct RuntimeAdapterRegistry {
    adapters: BTreeMap<Box<str>, Box<dyn DynRuntimeAdapter>>,
}
impl RuntimeAdapterRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    /// Refuses empty and duplicate identifiers; an existing entry is never replaced.
    pub fn register(&mut self, adapter: Box<dyn DynRuntimeAdapter>) -> Result<(), RegistryError> {
        let id = adapter.runtime_id();
        if id.is_empty() {
            return Err(RegistryError::EmptyRuntimeId);
        }
        if self.adapters.contains_key(id) {
            return Err(RegistryError::DuplicateRuntimeId);
        }
        self.adapters.insert(id.into(), adapter);
        Ok(())
    }
    /// Exact byte match; no normalization.
    pub fn get(&self, runtime_id: &str) -> Option<&dyn DynRuntimeAdapter> {
        self.adapters.get(runtime_id).map(|adapter| &**adapter)
    }
    /// The adapter that issued this handle, if still registered.
    pub fn for_handle(&self, handle: &DynAttemptHandle) -> Option<&dyn DynRuntimeAdapter> {
        self.get(handle.runtime_id())
    }
    /// Registered identifiers in byte order.
    pub fn runtime_ids(&self) -> impl Iterator<Item = &str> {
        self.adapters.keys().map(|id| &**id)
    }
}
