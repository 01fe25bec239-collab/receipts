//! Bounded reference-bearing records for four dispatch-admission axes.
//!
//! Each record stores its supplied axis result and optional opaque reference
//! verbatim. The only validation is the frozen `1..=200` Unicode-character
//! bound on a reference when present.

use super::dispatch_admission_vocabulary::DispatchAdmissionAxisResult;

const MAX_REFERENCE_CHARACTERS: usize = 200;

/// A field-specific reference-length failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchAdmissionAxisReferenceError {
    FeatureAdmissionDecisionIdLengthOutOfRange { character_count: usize },
    AvailabilityStateRefLengthOutOfRange { character_count: usize },
    SafetyInterruptionIdLengthOutOfRange { character_count: usize },
    RoutingDecisionIdLengthOutOfRange { character_count: usize },
}

fn invalid_character_count(reference: &Option<String>) -> Option<usize> {
    let character_count = reference.as_deref()?.chars().count();
    (!(1..=MAX_REFERENCE_CHARACTERS).contains(&character_count)).then_some(character_count)
}

/// Entitlement-axis result with an optional `FeatureAdmissionDecision` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionEntitlementAxisResult {
    result: DispatchAdmissionAxisResult,
    feature_admission_decision_id: Option<String>,
}

impl DispatchAdmissionEntitlementAxisResult {
    pub fn try_new(
        result: DispatchAdmissionAxisResult,
        feature_admission_decision_id: Option<String>,
    ) -> Result<Self, DispatchAdmissionAxisReferenceError> {
        if let Some(character_count) = invalid_character_count(&feature_admission_decision_id) {
            return Err(
                DispatchAdmissionAxisReferenceError::FeatureAdmissionDecisionIdLengthOutOfRange {
                    character_count,
                },
            );
        }
        Ok(Self {
            result,
            feature_admission_decision_id,
        })
    }

    pub fn result(&self) -> DispatchAdmissionAxisResult {
        self.result
    }

    pub fn feature_admission_decision_id(&self) -> Option<&str> {
        self.feature_admission_decision_id.as_deref()
    }
}

/// Provider-availability result with an optional `AvailabilityState` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionProviderAvailabilityAxisResult {
    result: DispatchAdmissionAxisResult,
    availability_state_ref: Option<String>,
}

impl DispatchAdmissionProviderAvailabilityAxisResult {
    pub fn try_new(
        result: DispatchAdmissionAxisResult,
        availability_state_ref: Option<String>,
    ) -> Result<Self, DispatchAdmissionAxisReferenceError> {
        if let Some(character_count) = invalid_character_count(&availability_state_ref) {
            return Err(
                DispatchAdmissionAxisReferenceError::AvailabilityStateRefLengthOutOfRange {
                    character_count,
                },
            );
        }
        Ok(Self {
            result,
            availability_state_ref,
        })
    }

    pub fn result(&self) -> DispatchAdmissionAxisResult {
        self.result
    }

    pub fn availability_state_ref(&self) -> Option<&str> {
        self.availability_state_ref.as_deref()
    }
}

/// Safety-axis result with an optional `SafetyInterruption` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionSafetyAxisResult {
    result: DispatchAdmissionAxisResult,
    safety_interruption_id: Option<String>,
}

impl DispatchAdmissionSafetyAxisResult {
    pub fn try_new(
        result: DispatchAdmissionAxisResult,
        safety_interruption_id: Option<String>,
    ) -> Result<Self, DispatchAdmissionAxisReferenceError> {
        if let Some(character_count) = invalid_character_count(&safety_interruption_id) {
            return Err(
                DispatchAdmissionAxisReferenceError::SafetyInterruptionIdLengthOutOfRange {
                    character_count,
                },
            );
        }
        Ok(Self {
            result,
            safety_interruption_id,
        })
    }

    pub fn result(&self) -> DispatchAdmissionAxisResult {
        self.result
    }

    pub fn safety_interruption_id(&self) -> Option<&str> {
        self.safety_interruption_id.as_deref()
    }
}

/// Quality-floor result with an optional `RoutingDecision` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionQualityFloorAxisResult {
    result: DispatchAdmissionAxisResult,
    routing_decision_id: Option<String>,
}

impl DispatchAdmissionQualityFloorAxisResult {
    pub fn try_new(
        result: DispatchAdmissionAxisResult,
        routing_decision_id: Option<String>,
    ) -> Result<Self, DispatchAdmissionAxisReferenceError> {
        if let Some(character_count) = invalid_character_count(&routing_decision_id) {
            return Err(
                DispatchAdmissionAxisReferenceError::RoutingDecisionIdLengthOutOfRange {
                    character_count,
                },
            );
        }
        Ok(Self {
            result,
            routing_decision_id,
        })
    }

    pub fn result(&self) -> DispatchAdmissionAxisResult {
        self.result
    }

    pub fn routing_decision_id(&self) -> Option<&str> {
        self.routing_decision_id.as_deref()
    }
}
