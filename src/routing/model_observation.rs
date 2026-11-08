//! Complete in-process ModelObservation (one local calibration datum). This
//! is MR's own contract; W2 stores and codes it only. Calibration, blending
//! and cost-to-acceptance estimation are later (MR-W5-2).

use crate::policy_eligibility::ModelRoutingDateTimeV1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelObservationOutcome {
    AcceptedFirstPass,
    AcceptedAfterRepair,
    Rejected,
    Timeout,
    RuntimeFailure,
    Cancelled,
    UserOverride,
}

impl ModelObservationOutcome {
    pub const ALL: [Self; 7] = [
        Self::AcceptedFirstPass,
        Self::AcceptedAfterRepair,
        Self::Rejected,
        Self::Timeout,
        Self::RuntimeFailure,
        Self::Cancelled,
        Self::UserOverride,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AcceptedFirstPass => "ACCEPTED_FIRST_PASS",
            Self::AcceptedAfterRepair => "ACCEPTED_AFTER_REPAIR",
            Self::Rejected => "REJECTED",
            Self::Timeout => "TIMEOUT",
            Self::RuntimeFailure => "RUNTIME_FAILURE",
            Self::Cancelled => "CANCELLED",
            Self::UserOverride => "USER_OVERRIDE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DifficultyHint {
    Low,
    Medium,
    High,
    Unknown,
}

impl DifficultyHint {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Unknown];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "LOW",
            Self::Medium => "MEDIUM",
            Self::High => "HIGH",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Every schema field. Nullable numbers distinguish absent (`None`) from
/// explicit null (`Some(None)`); f64/i64/u64 are bounded carriers.
/// `task_class` is an open string in the schema and is kept exact.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelObservation {
    observation_id: String,
    provider_id: String,
    model_id: String,
    runtime_id: String,
    task_id: Option<String>,
    task_class: String,
    outcome: ModelObservationOutcome,
    repair_count: Option<u64>,
    cost: Option<Option<f64>>,
    tokens: Option<Option<i64>>,
    latency_seconds: Option<Option<f64>>,
    cost_to_accepted_result: Option<Option<f64>>,
    difficulty_hint: Option<DifficultyHint>,
    excluded_from_calibration: Option<bool>,
    observed_at: ModelRoutingDateTimeV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelObservationError {
    InvalidObservationId,
    EmptyProviderId,
    EmptyModelId,
    EmptyRuntimeId,
    InvalidTaskId,
    NonFiniteNumber,
}

fn bounded_id(value: &str) -> bool {
    (1..=200).contains(&value.chars().count())
}

impl ModelObservation {
    /// Validates only physical schema constraints and preserves accepted input.
    #[allow(clippy::too_many_arguments)] // Explicit schema fields; no hidden defaults.
    pub fn try_new(
        observation_id: String,
        provider_id: String,
        model_id: String,
        runtime_id: String,
        task_id: Option<String>,
        task_class: String,
        outcome: ModelObservationOutcome,
        repair_count: Option<u64>,
        cost: Option<Option<f64>>,
        tokens: Option<Option<i64>>,
        latency_seconds: Option<Option<f64>>,
        cost_to_accepted_result: Option<Option<f64>>,
        difficulty_hint: Option<DifficultyHint>,
        excluded_from_calibration: Option<bool>,
        observed_at: ModelRoutingDateTimeV1,
    ) -> Result<Self, ModelObservationError> {
        use ModelObservationError::*;
        if !bounded_id(&observation_id) {
            return Err(InvalidObservationId);
        }
        if provider_id.is_empty() {
            return Err(EmptyProviderId);
        }
        if model_id.is_empty() {
            return Err(EmptyModelId);
        }
        if runtime_id.is_empty() {
            return Err(EmptyRuntimeId);
        }
        if task_id.as_deref().is_some_and(|id| !bounded_id(id)) {
            return Err(InvalidTaskId);
        }
        if [cost, latency_seconds, cost_to_accepted_result]
            .into_iter()
            .flatten()
            .flatten()
            .any(|v| !v.is_finite())
        {
            return Err(NonFiniteNumber);
        }
        Ok(Self {
            observation_id,
            provider_id,
            model_id,
            runtime_id,
            task_id,
            task_class,
            outcome,
            repair_count,
            cost,
            tokens,
            latency_seconds,
            cost_to_accepted_result,
            difficulty_hint,
            excluded_from_calibration,
            observed_at,
        })
    }

    pub fn observation_id(&self) -> &str {
        &self.observation_id
    }
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }
    pub fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }
    pub fn task_class(&self) -> &str {
        &self.task_class
    }
    pub fn outcome(&self) -> ModelObservationOutcome {
        self.outcome
    }
    pub fn repair_count(&self) -> Option<u64> {
        self.repair_count
    }
    pub fn cost(&self) -> Option<Option<f64>> {
        self.cost
    }
    pub fn tokens(&self) -> Option<Option<i64>> {
        self.tokens
    }
    pub fn latency_seconds(&self) -> Option<Option<f64>> {
        self.latency_seconds
    }
    pub fn cost_to_accepted_result(&self) -> Option<Option<f64>> {
        self.cost_to_accepted_result
    }
    pub fn difficulty_hint(&self) -> Option<DifficultyHint> {
        self.difficulty_hint
    }
    pub fn excluded_from_calibration(&self) -> Option<bool> {
        self.excluded_from_calibration
    }
    pub fn observed_at(&self) -> &ModelRoutingDateTimeV1 {
        &self.observed_at
    }
}
