//! Whole-event semantic source compatibility and validated adapter emission.
//! This composition provides no live host event producer or delivery guarantee.

use crate::{
    HostAdapter, NormalizedHostEvent, NormalizedHostEventSourceClass, NormalizedHostEventType,
    source_class_allowed,
};

/// The supplied source class is not semantically permitted for this event type.
/// Only bounded vocabulary values are retained, never event contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizedHostEventEmissionSourceError {
    pub event_type: NormalizedHostEventType,
    pub source_class: NormalizedHostEventSourceClass,
}

impl std::fmt::Display for NormalizedHostEventEmissionSourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "source class {} is not semantically permitted for event type {}",
            self.source_class.as_str(),
            self.event_type.as_str()
        )
    }
}

impl std::error::Error for NormalizedHostEventEmissionSourceError {}

/// Validates the explicit source class, then calls [`HostAdapter::emit`] exactly once.
///
/// Rejection returns the existing validation error without calling the adapter.
/// Acceptance passes the original event reference and returns the adapter's
/// opaque outcome unchanged. Compatibility establishes no authenticity, trust,
/// confidence correctness, payload correctness, persistence, or delivery.
/// This bridge does not produce or normalize live host events.
pub fn emit_validated_normalized_host_event<A: HostAdapter>(
    adapter: &A,
    source_class: NormalizedHostEventSourceClass,
    event: &NormalizedHostEvent,
) -> Result<A::EmitOutcome, NormalizedHostEventEmissionSourceError> {
    validate_normalized_host_event_source(source_class, event)?;
    Ok(adapter.emit(event))
}

/// Validates only semantic source-class compatibility through [`source_class_allowed`].
///
/// Success establishes only that the supplied class is permitted for the event
/// type. It proves no physical source, emission, observation, authenticity,
/// trust, confidence, payload completeness, parser correctness, persistence,
/// or delivery. This read-only guard performs no I/O and does not invoke
/// `HostAdapter::emit`.
pub fn validate_normalized_host_event_source(
    source_class: NormalizedHostEventSourceClass,
    event: &NormalizedHostEvent,
) -> Result<(), NormalizedHostEventEmissionSourceError> {
    if source_class_allowed(event.event_type(), source_class) {
        Ok(())
    } else {
        Err(NormalizedHostEventEmissionSourceError {
            event_type: event.event_type(),
            source_class,
        })
    }
}
