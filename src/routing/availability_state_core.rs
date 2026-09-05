use crate::{AvailabilitySignalSource, AvailabilityStateKind};

/// In-process, non-temporal availability storage, not the complete wire AvailabilityState.
/// Retry seconds use a bounded non-negative carrier, not an arbitrary-precision JSON codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailabilityStateNonTemporalCore {
    provider_id: String,
    model_id: Option<String>,
    runtime_id: Option<String>,
    state: AvailabilityStateKind,
    retry_after_seconds: Option<u64>,
    signal_source: Option<AvailabilitySignalSource>,
    note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityStateCoreError {
    EmptyProviderId,
    EmptyModelId,
    EmptyRuntimeId,
}

impl AvailabilityStateNonTemporalCore {
    /// Stores values unchanged, rejecting only empty identifiers when present.
    pub fn try_new(
        provider_id: String,
        model_id: Option<String>,
        runtime_id: Option<String>,
        state: AvailabilityStateKind,
        retry_after_seconds: Option<u64>,
        signal_source: Option<AvailabilitySignalSource>,
        note: Option<String>,
    ) -> Result<Self, AvailabilityStateCoreError> {
        if provider_id.is_empty() {
            return Err(AvailabilityStateCoreError::EmptyProviderId);
        }
        if model_id.as_deref() == Some("") {
            return Err(AvailabilityStateCoreError::EmptyModelId);
        }
        if runtime_id.as_deref() == Some("") {
            return Err(AvailabilityStateCoreError::EmptyRuntimeId);
        }
        Ok(Self {
            provider_id,
            model_id,
            runtime_id,
            state,
            retry_after_seconds,
            signal_source,
            note,
        })
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }

    pub fn runtime_id(&self) -> Option<&str> {
        self.runtime_id.as_deref()
    }

    pub fn state(&self) -> AvailabilityStateKind {
        self.state
    }

    pub fn retry_after_seconds(&self) -> Option<u64> {
        self.retry_after_seconds
    }

    pub fn signal_source(&self) -> Option<AvailabilitySignalSource> {
        self.signal_source
    }

    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}
