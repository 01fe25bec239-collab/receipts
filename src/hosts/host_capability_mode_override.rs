//! Structural value object for a host capability mode override.

use crate::HostCapabilityModeOverrideSource;

/// Records the source and non-empty reason for a host capability mode override.
///
/// This value records provenance only. Its source does not grant user, admin,
/// or debug authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilityModeOverride {
    source: HostCapabilityModeOverrideSource,
    reason: String,
}

impl HostCapabilityModeOverride {
    /// Creates an override with the source and reason stored exactly as provided.
    ///
    /// Returns [`HostCapabilityModeOverrideError::EmptyReason`] when `reason` is
    /// empty. Every non-empty string is structurally valid.
    pub fn new(
        source: HostCapabilityModeOverrideSource,
        reason: String,
    ) -> Result<Self, HostCapabilityModeOverrideError> {
        if reason.is_empty() {
            return Err(HostCapabilityModeOverrideError::EmptyReason);
        }

        Ok(Self { source, reason })
    }

    /// Returns the recorded provenance source without granting its authority.
    pub fn source(&self) -> HostCapabilityModeOverrideSource {
        self.source
    }

    /// Returns the non-empty reason exactly as provided at construction.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// Structural validation errors for [`HostCapabilityModeOverride`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapabilityModeOverrideError {
    /// The supplied reason was empty; reasons must contain at least one byte.
    EmptyReason,
}
