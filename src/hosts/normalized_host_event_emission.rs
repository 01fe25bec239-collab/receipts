//! Pure whole-event semantic source compatibility; no physical emission.

use crate::{
    NormalizedHostEvent, NormalizedHostEventSourceClass, NormalizedHostEventType,
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
