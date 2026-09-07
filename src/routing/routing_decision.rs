//! Frozen RoutingDecision physical vocabulary and non-temporal in-process storage only.
//! No routing, scoring, inference, registry access, or temporal parsing.

use crate::RoutingQualityFloor;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDecisionOutcome {
    Selected,
    NoEligibleCandidate,
    UserInputRequired,
    Blocked,
}

impl RoutingDecisionOutcome {
    pub const ALL: [Self; 4] = [
        Self::Selected,
        Self::NoEligibleCandidate,
        Self::UserInputRequired,
        Self::Blocked,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "SELECTED",
            Self::NoEligibleCandidate => "NO_ELIGIBLE_CANDIDATE",
            Self::UserInputRequired => "USER_INPUT_REQUIRED",
            Self::Blocked => "BLOCKED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceConfidence {
    OfficialVerified,
    IndependentVerified,
    LocalEmpirical,
    UserDeclared,
    Unverified,
}

impl EvidenceConfidence {
    pub const ALL: [Self; 5] = [
        Self::OfficialVerified,
        Self::IndependentVerified,
        Self::LocalEmpirical,
        Self::UserDeclared,
        Self::Unverified,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OfficialVerified => "OFFICIAL_VERIFIED",
            Self::IndependentVerified => "INDEPENDENT_VERIFIED",
            Self::LocalEmpirical => "LOCAL_EMPIRICAL",
            Self::UserDeclared => "USER_DECLARED",
            Self::Unverified => "UNVERIFIED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSourceRefType {
    RepoPath,
    StateQuery,
    ArtifactId,
    Url,
}

impl EvidenceSourceRefType {
    pub const ALL: [Self; 4] = [
        Self::RepoPath,
        Self::StateQuery,
        Self::ArtifactId,
        Self::Url,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RepoPath => "REPO_PATH",
            Self::StateQuery => "STATE_QUERY",
            Self::ArtifactId => "ARTIFACT_ID",
            Self::Url => "URL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionConfidence {
    High,
    Medium,
    Low,
}

impl DecisionConfidence {
    pub const ALL: [Self; 3] = [Self::High, Self::Medium, Self::Low];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "HIGH",
            Self::Medium => "MEDIUM",
            Self::Low => "LOW",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstimatedCostClass {
    Low,
    Medium,
    High,
    Unknown,
}

impl EstimatedCostClass {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDecisionMode {
    AutoCurrent,
    AskOnUncertainty,
    UserControlled,
}

impl RoutingDecisionMode {
    pub const ALL: [Self; 3] = [
        Self::AutoCurrent,
        Self::AskOnUncertainty,
        Self::UserControlled,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AutoCurrent => "AUTO_CURRENT",
            Self::AskOnUncertainty => "ASK_ON_UNCERTAINTY",
            Self::UserControlled => "USER_CONTROLLED",
        }
    }
}

/// Optional nullable numbers use `Option<Option<f64>>`: outer `None` is an
/// absent property, `Some(None)` is explicit null, and `Some(Some(value))` is
/// a number. The f64 values are bounded in-process carriers, not an
/// arbitrary-precision JSON number codec.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutingScoreComponents {
    expected_implementation_cost: Option<Option<f64>>,
    expected_rejection_probability: Option<Option<f64>>,
    expected_repair_cost: Option<Option<f64>>,
    expected_review_cost: Option<Option<f64>>,
    latency_estimate_seconds: Option<Option<f64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingScoreComponentsError {
    NonFiniteExpectedImplementationCost,
    NonFiniteExpectedRejectionProbability,
    ExpectedRejectionProbabilityOutOfRange,
    NonFiniteExpectedRepairCost,
    NonFiniteExpectedReviewCost,
    NonFiniteLatencyEstimateSeconds,
}

impl RoutingScoreComponents {
    pub fn try_new(
        expected_implementation_cost: Option<Option<f64>>,
        expected_rejection_probability: Option<Option<f64>>,
        expected_repair_cost: Option<Option<f64>>,
        expected_review_cost: Option<Option<f64>>,
        latency_estimate_seconds: Option<Option<f64>>,
    ) -> Result<Self, RoutingScoreComponentsError> {
        use RoutingScoreComponentsError::*;

        if matches!(expected_implementation_cost, Some(Some(value)) if !value.is_finite()) {
            return Err(NonFiniteExpectedImplementationCost);
        }
        if let Some(Some(value)) = expected_rejection_probability {
            if !value.is_finite() {
                return Err(NonFiniteExpectedRejectionProbability);
            }
            if !(0.0..=1.0).contains(&value) {
                return Err(ExpectedRejectionProbabilityOutOfRange);
            }
        }
        if matches!(expected_repair_cost, Some(Some(value)) if !value.is_finite()) {
            return Err(NonFiniteExpectedRepairCost);
        }
        if matches!(expected_review_cost, Some(Some(value)) if !value.is_finite()) {
            return Err(NonFiniteExpectedReviewCost);
        }
        if matches!(latency_estimate_seconds, Some(Some(value)) if !value.is_finite()) {
            return Err(NonFiniteLatencyEstimateSeconds);
        }
        Ok(Self {
            expected_implementation_cost,
            expected_rejection_probability,
            expected_repair_cost,
            expected_review_cost,
            latency_estimate_seconds,
        })
    }

    pub fn expected_implementation_cost(&self) -> Option<Option<f64>> {
        self.expected_implementation_cost
    }

    pub fn expected_rejection_probability(&self) -> Option<Option<f64>> {
        self.expected_rejection_probability
    }

    pub fn expected_repair_cost(&self) -> Option<Option<f64>> {
        self.expected_repair_cost
    }

    pub fn expected_review_cost(&self) -> Option<Option<f64>> {
        self.expected_review_cost
    }

    pub fn latency_estimate_seconds(&self) -> Option<Option<f64>> {
        self.latency_estimate_seconds
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectionReason {
    quality_floor_applied: Option<RoutingQualityFloor>,
    hard_filters_passed: Option<Vec<String>>,
    score_components: Option<RoutingScoreComponents>,
    decisive_factor: Option<String>,
}

impl SelectionReason {
    pub fn new(
        quality_floor_applied: Option<RoutingQualityFloor>,
        hard_filters_passed: Option<Vec<String>>,
        score_components: Option<RoutingScoreComponents>,
        decisive_factor: Option<String>,
    ) -> Self {
        Self {
            quality_floor_applied,
            hard_filters_passed,
            score_components,
            decisive_factor,
        }
    }

    pub fn quality_floor_applied(&self) -> Option<RoutingQualityFloor> {
        self.quality_floor_applied
    }

    pub fn hard_filters_passed(&self) -> Option<&[String]> {
        self.hard_filters_passed.as_deref()
    }

    pub fn score_components(&self) -> Option<&RoutingScoreComponents> {
        self.score_components.as_ref()
    }

    pub fn decisive_factor(&self) -> Option<&str> {
        self.decisive_factor.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSourceRef {
    ref_type: EvidenceSourceRefType,
    target: String,
    digest: Option<String>,
    section: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceSourceRefError {
    EmptyTarget,
}

impl EvidenceSourceRef {
    pub fn try_new(
        ref_type: EvidenceSourceRefType,
        target: String,
        digest: Option<String>,
        section: Option<String>,
    ) -> Result<Self, EvidenceSourceRefError> {
        if target.is_empty() {
            return Err(EvidenceSourceRefError::EmptyTarget);
        }
        Ok(Self {
            ref_type,
            target,
            digest,
            section,
        })
    }

    pub fn ref_type(&self) -> EvidenceSourceRefType {
        self.ref_type
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn digest(&self) -> Option<&str> {
        self.digest.as_deref()
    }

    pub fn section(&self) -> Option<&str> {
        self.section.as_deref()
    }
}

/// Non-temporal evidence storage. The unrestricted `value` property is
/// deferred without an authoritative any-JSON carrier, and `observed_at` is
/// deferred without an authoritative timestamp binding.
/// `sample_size` distinguishes absent/null/value and uses a bounded u64
/// carrier rather than an arbitrary-precision JSON integer codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityEvidenceNonTemporalCore {
    capability: String,
    confidence: EvidenceConfidence,
    source_ref: Option<EvidenceSourceRef>,
    sample_size: Option<Option<u64>>,
}

impl CapabilityEvidenceNonTemporalCore {
    pub fn new(
        capability: String,
        confidence: EvidenceConfidence,
        source_ref: Option<EvidenceSourceRef>,
        sample_size: Option<Option<u64>>,
    ) -> Self {
        Self {
            capability,
            confidence,
            source_ref,
            sample_size,
        }
    }

    pub fn capability(&self) -> &str {
        &self.capability
    }

    pub fn confidence(&self) -> EvidenceConfidence {
        self.confidence
    }

    pub fn source_ref(&self) -> Option<&EvidenceSourceRef> {
        self.source_ref.as_ref()
    }

    pub fn sample_size(&self) -> Option<Option<u64>> {
        self.sample_size
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlternativeCandidate {
    provider_id: String,
    model_id: String,
    runtime_id: String,
    score: Option<Option<f64>>,
    rejection_reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlternativeCandidateError {
    EmptyProviderId,
    EmptyModelId,
    EmptyRuntimeId,
    NonFiniteScore,
}

impl AlternativeCandidate {
    pub fn try_new(
        provider_id: String,
        model_id: String,
        runtime_id: String,
        score: Option<Option<f64>>,
        rejection_reason: String,
    ) -> Result<Self, AlternativeCandidateError> {
        if provider_id.is_empty() {
            return Err(AlternativeCandidateError::EmptyProviderId);
        }
        if model_id.is_empty() {
            return Err(AlternativeCandidateError::EmptyModelId);
        }
        if runtime_id.is_empty() {
            return Err(AlternativeCandidateError::EmptyRuntimeId);
        }
        if matches!(score, Some(Some(value)) if !value.is_finite()) {
            return Err(AlternativeCandidateError::NonFiniteScore);
        }
        Ok(Self {
            provider_id,
            model_id,
            runtime_id,
            score,
            rejection_reason,
        })
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

    pub fn score(&self) -> Option<Option<f64>> {
        self.score
    }

    pub fn rejection_reason(&self) -> &str {
        &self.rejection_reason
    }
}

/// Integer properties distinguish absent/null/value and use bounded i64
/// carriers, not an arbitrary-precision JSON integer codec. Negative values
/// are preserved because the physical schema gives these fields no minimum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryFreshness {
    model_list_age_seconds: Option<Option<i64>>,
    capability_age_seconds: Option<Option<i64>>,
    calibration_sample_size: Option<Option<i64>>,
    stale: bool,
}

impl RegistryFreshness {
    pub fn new(
        model_list_age_seconds: Option<Option<i64>>,
        capability_age_seconds: Option<Option<i64>>,
        calibration_sample_size: Option<Option<i64>>,
        stale: bool,
    ) -> Self {
        Self {
            model_list_age_seconds,
            capability_age_seconds,
            calibration_sample_size,
            stale,
        }
    }

    pub fn model_list_age_seconds(&self) -> Option<Option<i64>> {
        self.model_list_age_seconds
    }

    pub fn capability_age_seconds(&self) -> Option<Option<i64>> {
        self.capability_age_seconds
    }

    pub fn calibration_sample_size(&self) -> Option<Option<i64>> {
        self.calibration_sample_size
    }

    pub fn stale(&self) -> bool {
        self.stale
    }
}

/// In-process non-temporal core, NOT the complete wire RoutingDecision.
/// `occurred_at` and evidence `observed_at` are deferred without an
/// authoritative timestamp binding. Evidence `value` is deferred without an
/// authoritative dependency-free any-JSON representation.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutingDecisionNonTemporalCore {
    decision_id: String,
    request_id: String,
    task_id: Option<String>,
    outcome: RoutingDecisionOutcome,
    selected_provider: Option<String>,
    selected_model: Option<String>,
    selected_runtime: Option<String>,
    selection_reason: Option<SelectionReason>,
    capability_evidence: Option<Vec<CapabilityEvidenceNonTemporalCore>>,
    confidence: Option<DecisionConfidence>,
    availability_at_decision: Option<String>,
    estimated_cost_class: Option<EstimatedCostClass>,
    alternative_candidates: Option<Vec<AlternativeCandidate>>,
    registry_freshness: RegistryFreshness,
    mode: RoutingDecisionMode,
    user_involved: Option<bool>,
    user_pin_applied: Option<bool>,
    fallback_from: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDecisionCoreError {
    EmptyDecisionId,
    DecisionIdTooLong,
    EmptyRequestId,
    RequestIdTooLong,
    EmptyTaskId,
    TaskIdTooLong,
    EmptySelectedProvider,
    EmptySelectedModel,
    EmptySelectedRuntime,
    EmptyFallbackFrom,
    FallbackFromTooLong,
}

impl RoutingDecisionNonTemporalCore {
    /// Validates only physical schema invariants and preserves accepted input.
    #[allow(clippy::too_many_arguments)] // Explicit schema fields; no hidden defaults.
    pub fn try_new(
        decision_id: String,
        request_id: String,
        task_id: Option<String>,
        outcome: RoutingDecisionOutcome,
        selected_provider: Option<String>,
        selected_model: Option<String>,
        selected_runtime: Option<String>,
        selection_reason: Option<SelectionReason>,
        capability_evidence: Option<Vec<CapabilityEvidenceNonTemporalCore>>,
        confidence: Option<DecisionConfidence>,
        availability_at_decision: Option<String>,
        estimated_cost_class: Option<EstimatedCostClass>,
        alternative_candidates: Option<Vec<AlternativeCandidate>>,
        registry_freshness: RegistryFreshness,
        mode: RoutingDecisionMode,
        user_involved: Option<bool>,
        user_pin_applied: Option<bool>,
        fallback_from: Option<String>,
    ) -> Result<Self, RoutingDecisionCoreError> {
        use RoutingDecisionCoreError::*;

        validate_bounded_id(&decision_id, EmptyDecisionId, DecisionIdTooLong)?;
        validate_bounded_id(&request_id, EmptyRequestId, RequestIdTooLong)?;
        if let Some(value) = &task_id {
            validate_bounded_id(value, EmptyTaskId, TaskIdTooLong)?;
        }
        if selected_provider.as_deref() == Some("") {
            return Err(EmptySelectedProvider);
        }
        if selected_model.as_deref() == Some("") {
            return Err(EmptySelectedModel);
        }
        if selected_runtime.as_deref() == Some("") {
            return Err(EmptySelectedRuntime);
        }
        if let Some(value) = &fallback_from {
            validate_bounded_id(value, EmptyFallbackFrom, FallbackFromTooLong)?;
        }
        Ok(Self {
            decision_id,
            request_id,
            task_id,
            outcome,
            selected_provider,
            selected_model,
            selected_runtime,
            selection_reason,
            capability_evidence,
            confidence,
            availability_at_decision,
            estimated_cost_class,
            alternative_candidates,
            registry_freshness,
            mode,
            user_involved,
            user_pin_applied,
            fallback_from,
        })
    }

    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }
    pub fn outcome(&self) -> RoutingDecisionOutcome {
        self.outcome
    }
    pub fn selected_provider(&self) -> Option<&str> {
        self.selected_provider.as_deref()
    }
    pub fn selected_model(&self) -> Option<&str> {
        self.selected_model.as_deref()
    }
    pub fn selected_runtime(&self) -> Option<&str> {
        self.selected_runtime.as_deref()
    }
    pub fn selection_reason(&self) -> Option<&SelectionReason> {
        self.selection_reason.as_ref()
    }
    pub fn capability_evidence(&self) -> Option<&[CapabilityEvidenceNonTemporalCore]> {
        self.capability_evidence.as_deref()
    }
    pub fn confidence(&self) -> Option<DecisionConfidence> {
        self.confidence
    }
    pub fn availability_at_decision(&self) -> Option<&str> {
        self.availability_at_decision.as_deref()
    }
    pub fn estimated_cost_class(&self) -> Option<EstimatedCostClass> {
        self.estimated_cost_class
    }
    pub fn alternative_candidates(&self) -> Option<&[AlternativeCandidate]> {
        self.alternative_candidates.as_deref()
    }
    pub fn registry_freshness(&self) -> &RegistryFreshness {
        &self.registry_freshness
    }
    pub fn mode(&self) -> RoutingDecisionMode {
        self.mode
    }
    pub fn user_involved(&self) -> Option<bool> {
        self.user_involved
    }
    pub fn user_pin_applied(&self) -> Option<bool> {
        self.user_pin_applied
    }
    pub fn fallback_from(&self) -> Option<&str> {
        self.fallback_from.as_deref()
    }
}

fn validate_bounded_id(
    value: &str,
    empty: RoutingDecisionCoreError,
    too_long: RoutingDecisionCoreError,
) -> Result<(), RoutingDecisionCoreError> {
    if value.is_empty() {
        return Err(empty);
    }
    if value.chars().count() > 200 {
        return Err(too_long);
    }
    Ok(())
}
