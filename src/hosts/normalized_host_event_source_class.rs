//! Frozen, host-neutral source-class vocabulary for normalized host events.
//!
//! This module defines words only. It does not derive a source class from an
//! event type, host, hook, worker, capability, confidence, or fallback posture.

/// The source category for a normalized host event.
///
/// The set is closed by contract: exactly these four source classes exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NormalizedHostEventSourceClass {
    /// The host's native hook system emitted the event.
    HostHook,
    /// An externally dispatched worker emitted the event.
    WorkerDispatch,
    /// A direct user request/response turn emitted the event.
    Elicitation,
    /// The core computed the event without a host signal.
    CoreDriven,
}

impl NormalizedHostEventSourceClass {
    /// Every source class in the frozen vocabulary.
    pub const ALL: [NormalizedHostEventSourceClass; 4] = [
        NormalizedHostEventSourceClass::HostHook,
        NormalizedHostEventSourceClass::WorkerDispatch,
        NormalizedHostEventSourceClass::Elicitation,
        NormalizedHostEventSourceClass::CoreDriven,
    ];

    /// Canonical external string for this source class.
    pub fn as_str(self) -> &'static str {
        match self {
            NormalizedHostEventSourceClass::HostHook => "HOST_HOOK",
            NormalizedHostEventSourceClass::WorkerDispatch => "WORKER_DISPATCH",
            NormalizedHostEventSourceClass::Elicitation => "ELICITATION",
            NormalizedHostEventSourceClass::CoreDriven => "CORE_DRIVEN",
        }
    }
}
