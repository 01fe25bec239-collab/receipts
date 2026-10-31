//! Runtime-keyed, type-erased adapter registry. Registration and dispatch grant
//! no admission, routing, credential or execution authority: the core dispatch
//! gate must still authorize every `start`. Any `RuntimeAdapter` with the
//! vendor-neutral bindings below registers without a core edit. Handles are
//! bound to the `DynAdapter` instance that issued them, not to a runtime ID.

use std::{
    any::Any,
    collections::BTreeMap,
    error::Error,
    fmt,
    future::{Future, poll_fn},
    pin::Pin,
    sync::Arc,
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

/// Identity of one registered adapter instance: this allocation. Every handle
/// it issues holds a strong reference, so the address cannot be reused while a
/// handle is live, and moving the owning `DynAdapter` or registry keeps it.
struct Issuer {
    runtime_id: Box<str>,
}

/// Non-cloneable erased handle bound to the `DynAdapter` that issued it.
/// Dropping it drops the concrete handle, so the adapter's own Drop cleanup
/// still runs.
pub struct DynAttemptHandle {
    issuer: Arc<Issuer>,
    inner: Box<dyn Any + Send + Sync>,
}
impl DynAttemptHandle {
    pub fn runtime_id(&self) -> &str {
        &self.issuer.runtime_id
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

/// The handle was not issued by this adapter instance, even if the runtime ID
/// and handle type match; nothing was invoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandleMismatch;
impl fmt::Display for HandleMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("attempt handle was not issued by this runtime adapter")
    }
}
impl Error for HandleMismatch {}

/// Object-safe frozen surface, private so every handle is issued and checked by
/// `DynAdapter`. Implemented for every `RuntimeAdapter` whose policy,
/// observation and cancellation bindings are the vendor-neutral types.
trait ErasedAdapter: Send + Sync {
    fn health(&self) -> RuntimeObservation;
    fn authenticate_status(&self) -> RuntimeAuthStatus;
    fn capabilities(&self) -> RuntimeCapabilities;
    fn models(&self) -> RuntimeObservation;
    fn start(
        &self,
        issuer: &Arc<Issuer>,
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
    fn resume(&self, issuer: &Arc<Issuer>, attempt_id: &AttemptId) -> Option<DynAttemptHandle>;
}

fn erase<H: Send + Sync + 'static>(issuer: &Arc<Issuer>, handle: H) -> DynAttemptHandle {
    DynAttemptHandle {
        issuer: Arc::clone(issuer),
        inner: Box::new(handle),
    }
}
fn typed<H: 'static>(handle: &DynAttemptHandle) -> Result<&H, HandleMismatch> {
    handle.downcast_ref().ok_or(HandleMismatch)
}

impl<A> ErasedAdapter for A
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
        issuer: &Arc<Issuer>,
        task: &RuntimeCapsuleFamily,
        workspace: &WorkspaceHandle,
        policy: &RuntimeExecutionPolicy,
    ) -> Result<DynAttemptHandle, FailureClass> {
        RuntimeAdapter::start(self, task, workspace, policy).map(|handle| erase(issuer, handle))
    }
    fn stream_events<'a>(
        &'a self,
        handle: &'a DynAttemptHandle,
    ) -> Result<DynEventStream<'a>, HandleMismatch> {
        let stream = RuntimeAdapter::stream_events(self, typed::<A::AttemptHandle>(handle)?);
        Ok(DynEventStream(Box::pin(Erased(Box::pin(stream)))))
    }
    fn collect_result(
        &self,
        handle: &DynAttemptHandle,
    ) -> Result<DynAttemptResult, HandleMismatch> {
        let result = RuntimeAdapter::collect_result(self, typed::<A::AttemptHandle>(handle)?);
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
        RuntimeAdapter::cancel(self, typed::<A::AttemptHandle>(handle)?, reason);
        Ok(())
    }
    fn classify_failure(&self, error: &RawFailure) -> FailureClass {
        RuntimeAdapter::classify_failure(self, error)
    }
    fn resume(&self, issuer: &Arc<Issuer>, attempt_id: &AttemptId) -> Option<DynAttemptHandle> {
        RuntimeAdapter::resume(self, attempt_id).map(|handle| erase(issuer, handle))
    }
}

/// One owned adapter instance behind the erased surface. It issues handles
/// bound to itself and refuses stream/collect/cancel for any handle another
/// instance issued, including one with the same runtime ID and handle type,
/// before the concrete adapter is invoked. Not `Clone`: one instance, one
/// identity. Local typed ownership only; this is no OS or dispatch boundary.
pub struct DynAdapter {
    issuer: Arc<Issuer>,
    adapter: Box<dyn ErasedAdapter>,
}
impl DynAdapter {
    pub fn new<A>(adapter: A) -> Self
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
        let runtime_id = RuntimeAdapter::runtime_id(&adapter).into();
        Self {
            issuer: Arc::new(Issuer { runtime_id }),
            adapter: Box::new(adapter),
        }
    }
    /// The identifier read from the adapter when it was wrapped, unnormalized.
    pub fn runtime_id(&self) -> &str {
        &self.issuer.runtime_id
    }
    /// True only for a handle this instance issued.
    pub fn issued(&self, handle: &DynAttemptHandle) -> bool {
        Arc::ptr_eq(&self.issuer, &handle.issuer)
    }
    fn own<'h>(
        &self,
        handle: &'h DynAttemptHandle,
    ) -> Result<&'h DynAttemptHandle, HandleMismatch> {
        self.issued(handle).then_some(handle).ok_or(HandleMismatch)
    }
    pub fn health(&self) -> RuntimeObservation {
        self.adapter.health()
    }
    pub fn authenticate_status(&self) -> RuntimeAuthStatus {
        self.adapter.authenticate_status()
    }
    pub fn capabilities(&self) -> RuntimeCapabilities {
        self.adapter.capabilities()
    }
    pub fn models(&self) -> RuntimeObservation {
        self.adapter.models()
    }
    pub fn start(
        &self,
        task: &RuntimeCapsuleFamily,
        workspace: &WorkspaceHandle,
        policy: &RuntimeExecutionPolicy,
    ) -> Result<DynAttemptHandle, FailureClass> {
        self.adapter.start(&self.issuer, task, workspace, policy)
    }
    pub fn stream_events<'a>(
        &'a self,
        handle: &'a DynAttemptHandle,
    ) -> Result<DynEventStream<'a>, HandleMismatch> {
        self.adapter.stream_events(self.own(handle)?)
    }
    pub fn collect_result(
        &self,
        handle: &DynAttemptHandle,
    ) -> Result<DynAttemptResult, HandleMismatch> {
        self.adapter.collect_result(self.own(handle)?)
    }
    pub fn cancel(
        &self,
        handle: &DynAttemptHandle,
        reason: &CancelReason,
    ) -> Result<(), HandleMismatch> {
        self.adapter.cancel(self.own(handle)?, reason)
    }
    pub fn classify_failure(&self, error: &RawFailure) -> FailureClass {
        self.adapter.classify_failure(error)
    }
    pub fn resume(&self, attempt_id: &AttemptId) -> Option<DynAttemptHandle> {
        self.adapter.resume(&self.issuer, attempt_id)
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
    adapters: BTreeMap<Box<str>, DynAdapter>,
}
impl RuntimeAdapterRegistry {
    pub fn new() -> Self {
        Self::default()
    }
    /// Refuses empty and duplicate identifiers; an existing entry is never replaced.
    pub fn register(&mut self, adapter: DynAdapter) -> Result<(), RegistryError> {
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
    pub fn get(&self, runtime_id: &str) -> Option<&DynAdapter> {
        self.adapters.get(runtime_id)
    }
    /// The registered instance that issued this handle; `None` for a handle
    /// from any other instance or registry, whatever its runtime ID.
    pub fn for_handle(&self, handle: &DynAttemptHandle) -> Option<&DynAdapter> {
        self.adapters
            .values()
            .find(|adapter| adapter.issued(handle))
    }
    /// Registered identifiers in byte order.
    pub fn runtime_ids(&self) -> impl Iterator<Item = &str> {
        self.adapters.keys().map(|id| &**id)
    }
}
