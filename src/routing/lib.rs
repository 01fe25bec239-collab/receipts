mod availability_state_core;
mod availability_vocabulary;

#[cfg(test)]
mod availability_state_core_tests;
#[cfg(test)]
mod availability_vocabulary_tests;

pub use availability_state_core::{AvailabilityStateCoreError, AvailabilityStateNonTemporalCore};
pub use availability_vocabulary::{AvailabilitySignalSource, AvailabilityStateKind};
