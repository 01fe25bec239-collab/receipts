//! Review-owned, in-process, structured, non-temporal SafetyInterruption data only.
//! Authority: build-control/orchestrator-architecture/schemas/SafetyInterruption.schema.json
//! at f49d621ee510705939394f7df4996223a73fdcb7, blob
//! 37505721fffce6246d79c373adbd7d369b2a2d6b (also at the authorized baseline).
//! OBSERVED_AT_STATUS: DEFERRED_NO_AUTHORIZED_REVIEW_TEMPORAL_TYPE
//! FULL_TEMPORAL_SAFETYINTERRUPTION_CLAIMED: NO
//! No policy detection, retry, routing, tooling execution, wire format or persistence.
//! Q-06 remains unresolved; tooling usage is caller-supplied data only.

use receipts_workspace_execution::WorkspaceCheckpointRef;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyInterruptionState {
    SafetyCheckPending,
    PolicyBlocked,
    Unknown,
}

impl SafetyInterruptionState {
    pub const ALL: [Self; 3] = [Self::SafetyCheckPending, Self::PolicyBlocked, Self::Unknown];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SafetyCheckPending => "SAFETY_CHECK_PENDING",
            Self::PolicyBlocked => "POLICY_BLOCKED",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyInterruptionDetectionConfidence {
    ExplicitProviderSignal,
    Heuristic,
    Unknown,
}

impl SafetyInterruptionDetectionConfidence {
    pub const ALL: [Self; 3] = [Self::ExplicitProviderSignal, Self::Heuristic, Self::Unknown];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitProviderSignal => "EXPLICIT_PROVIDER_SIGNAL",
            Self::Heuristic => "HEURISTIC",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyInterruptionTerminalOutcome {
    Resumed,
    HumanRequired,
    RoutedElsewhereCompliantly,
    Abandoned,
    Pending,
}

impl SafetyInterruptionTerminalOutcome {
    pub const ALL: [Self; 5] = [
        Self::Resumed,
        Self::HumanRequired,
        Self::RoutedElsewhereCompliantly,
        Self::Abandoned,
        Self::Pending,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resumed => "RESUMED",
            Self::HumanRequired => "HUMAN_REQUIRED",
            Self::RoutedElsewhereCompliantly => "ROUTED_ELSEWHERE_COMPLIANTLY",
            Self::Abandoned => "ABANDONED",
            Self::Pending => "PENDING",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafetyInterruptionConstructionError {
    InvalidIdentifier(&'static str),
    EmptyString(&'static str),
}

impl fmt::Display for SafetyInterruptionConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier(field) => write!(f, "{field} must contain 1..=200 characters"),
            Self::EmptyString(field) => write!(f, "{field} must contain at least one character"),
        }
    }
}
impl std::error::Error for SafetyInterruptionConstructionError {}

/// All fourteen non-temporal fields, with private validated state and shared access.
/// `None` means absent, never explicit null. No optional value is defaulted or inferred.
/// Evidence arrays retain absence, emptiness, order and duplicates. Construction
/// checks only machine shape, never cross-field policy; HUMAN_REQUIRED is data.
/// `observed_at` is deferred: this is not the full temporal contract.
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruptionNonTemporalCore) {
/// value.observed_at();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruptionNonTemporalCore) {
/// let _ = value.observed_at;
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruptionNonTemporalCore) {
/// value.retry();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruptionNonTemporalCore) {
/// value.detect_policy();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruptionNonTemporalCore) {
/// value.interruption_id = String::new();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruptionNonTemporalCore) {
/// let _: &mut str = value.interruption_id();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruptionNonTemporalCore) {
/// let _: &mut str = value.provider_id();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruptionNonTemporalCore) {
/// value.preserved_evidence_refs().unwrap().clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruptionNonTemporalCore) {
/// let _: &mut receipts_workspace_execution::WorkspaceCheckpointRef = &mut value.preserved_evidence_refs().unwrap()[0];
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::SafetyInterruptionNonTemporalCore) {
/// let _: &mut str = value.model_id().unwrap();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyInterruptionNonTemporalCore {
    interruption_id: String,
    task_id: String,
    attempt_id: String,
    provider_id: String,
    model_id: Option<String>,
    state: SafetyInterruptionState,
    detection_confidence: Option<SafetyInterruptionDetectionConfidence>,
    task_classified_defensive: Option<bool>,
    capsule_narrowed: Option<bool>,
    retry_attempted: Option<bool>,
    retry_provider_id: Option<String>,
    deterministic_tooling_used: Option<bool>,
    terminal_outcome: Option<SafetyInterruptionTerminalOutcome>,
    preserved_evidence_refs: Option<Vec<WorkspaceCheckpointRef>>,
}

impl SafetyInterruptionNonTemporalCore {
    // Arguments mirror the frozen shape, with observed_at explicitly deferred.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        interruption_id: String,
        task_id: String,
        attempt_id: String,
        provider_id: String,
        model_id: Option<String>,
        state: SafetyInterruptionState,
        detection_confidence: Option<SafetyInterruptionDetectionConfidence>,
        task_classified_defensive: Option<bool>,
        capsule_narrowed: Option<bool>,
        retry_attempted: Option<bool>,
        retry_provider_id: Option<String>,
        deterministic_tooling_used: Option<bool>,
        terminal_outcome: Option<SafetyInterruptionTerminalOutcome>,
        preserved_evidence_refs: Option<Vec<WorkspaceCheckpointRef>>,
    ) -> Result<Self, SafetyInterruptionConstructionError> {
        for (field, value) in [
            ("interruption_id", &interruption_id),
            ("task_id", &task_id),
            ("attempt_id", &attempt_id),
        ] {
            if !(1..=200).contains(&value.chars().count()) {
                return Err(SafetyInterruptionConstructionError::InvalidIdentifier(
                    field,
                ));
            }
        }
        for (field, value) in [
            ("provider_id", Some(provider_id.as_str())),
            ("model_id", model_id.as_deref()),
            ("retry_provider_id", retry_provider_id.as_deref()),
        ] {
            if value.is_some_and(str::is_empty) {
                return Err(SafetyInterruptionConstructionError::EmptyString(field));
            }
        }
        Ok(Self {
            interruption_id,
            task_id,
            attempt_id,
            provider_id,
            model_id,
            state,
            detection_confidence,
            task_classified_defensive,
            capsule_narrowed,
            retry_attempted,
            retry_provider_id,
            deterministic_tooling_used,
            terminal_outcome,
            preserved_evidence_refs,
        })
    }
    pub fn interruption_id(&self) -> &str {
        &self.interruption_id
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }
    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }
    pub fn state(&self) -> SafetyInterruptionState {
        self.state
    }
    pub fn detection_confidence(&self) -> Option<SafetyInterruptionDetectionConfidence> {
        self.detection_confidence
    }
    pub fn task_classified_defensive(&self) -> Option<bool> {
        self.task_classified_defensive
    }
    pub fn capsule_narrowed(&self) -> Option<bool> {
        self.capsule_narrowed
    }
    pub fn retry_attempted(&self) -> Option<bool> {
        self.retry_attempted
    }
    pub fn retry_provider_id(&self) -> Option<&str> {
        self.retry_provider_id.as_deref()
    }
    pub fn deterministic_tooling_used(&self) -> Option<bool> {
        self.deterministic_tooling_used
    }
    pub fn terminal_outcome(&self) -> Option<SafetyInterruptionTerminalOutcome> {
        self.terminal_outcome
    }
    pub fn preserved_evidence_refs(&self) -> Option<&[WorkspaceCheckpointRef]> {
        self.preserved_evidence_refs.as_deref()
    }
}
