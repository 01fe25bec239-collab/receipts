//! Non-temporal, in-process A4 review records. Constructors validate shape only.
use crate::{
    A4ReviewDimension, A4ReviewDimensionAssessment, A4ReviewFindingCategory,
    A4ReviewFindingConfidence, A4ReviewFindingSeverity, A4ReviewFindingSource,
    A4ReviewRecommendedAction, A4ReviewVerdict,
};
use receipts_workspace_execution::{WorkspaceCheckpointCheckSource, WorkspaceCheckpointRef};
use std::{fmt, num::NonZeroU64};

/// Result evidence for `A4Review.reproduction.checks[].result`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A4ReviewReproductionCheckResult {
    Pass,
    Fail,
    Error,
    Skipped,
    Unknown,
}
impl A4ReviewReproductionCheckResult {
    pub const ALL: [Self; 5] = [
        Self::Pass,
        Self::Fail,
        Self::Error,
        Self::Skipped,
        Self::Unknown,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Error => "ERROR",
            Self::Skipped => "SKIPPED",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Preserves the optional and nullable limitation property without policy meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A4ReviewReproductionLimitation {
    Omitted,
    Null,
    Text(String),
}

/// Deterministic schema-shape construction failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A4ReviewConstructionError {
    EmptyReviewId,
    ReviewIdTooLong,
    EmptyTaskId,
    TaskIdTooLong,
    MalformedReviewedSha,
    EmptyFindingId,
    FindingIdTooLong,
    EmptyFindingDescription,
    EmptyDimensions,
    EmptyReproductionCommand,
    MalformedReproductionCodeSha,
    EmptyReviewerProviderId,
    EmptyReviewerModelId,
    EmptyReviewerRuntimeId,
}
impl fmt::Display for A4ReviewConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyReviewId => f.write_str("review_id is empty"),
            Self::ReviewIdTooLong => f.write_str("review_id exceeds 200 characters"),
            Self::EmptyTaskId => f.write_str("task_id is empty"),
            Self::TaskIdTooLong => f.write_str("task_id exceeds 200 characters"),
            Self::MalformedReviewedSha => f.write_str(
                "reviewed_sha must be exactly 40 lowercase ASCII hexadecimal characters",
            ),
            Self::EmptyFindingId => f.write_str("finding_id is empty"),
            Self::FindingIdTooLong => f.write_str("finding_id exceeds 200 characters"),
            Self::EmptyFindingDescription => f.write_str("finding description is empty"),
            Self::EmptyDimensions => f.write_str("dimensions is empty"),
            Self::EmptyReproductionCommand => f.write_str("reproduction command is empty"),
            Self::MalformedReproductionCodeSha => f.write_str(
                "reproduction code_sha must be exactly 40 lowercase ASCII hexadecimal characters",
            ),
            Self::EmptyReviewerProviderId => f.write_str("reviewer provider_id is empty"),
            Self::EmptyReviewerModelId => f.write_str("reviewer model_id is empty"),
            Self::EmptyReviewerRuntimeId => f.write_str("reviewer runtime_id is empty"),
        }
    }
}
impl std::error::Error for A4ReviewConstructionError {}

/// Optional opaque reviewer identity; even an entirely absent identity is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A4ReviewReviewer {
    provider_id: Option<String>,
    model_id: Option<String>,
    runtime_id: Option<String>,
    session_ref: Option<String>,
}
impl A4ReviewReviewer {
    pub fn new(
        provider_id: Option<String>,
        model_id: Option<String>,
        runtime_id: Option<String>,
        session_ref: Option<String>,
    ) -> Result<Self, A4ReviewConstructionError> {
        if provider_id.as_deref() == Some("") {
            return Err(A4ReviewConstructionError::EmptyReviewerProviderId);
        }
        if model_id.as_deref() == Some("") {
            return Err(A4ReviewConstructionError::EmptyReviewerModelId);
        }
        if runtime_id.as_deref() == Some("") {
            return Err(A4ReviewConstructionError::EmptyReviewerRuntimeId);
        }
        Ok(Self {
            provider_id,
            model_id,
            runtime_id,
            session_ref,
        })
    }
    pub fn provider_id(&self) -> Option<&str> {
        self.provider_id.as_deref()
    }
    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }
    pub fn runtime_id(&self) -> Option<&str> {
        self.runtime_id.as_deref()
    }
    pub fn session_ref(&self) -> Option<&str> {
        self.session_ref.as_deref()
    }
}

/// One finding, used unchanged in either findings array. Line uses a positive integer; no application maximum is imposed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A4ReviewFinding {
    finding_id: String,
    severity: A4ReviewFindingSeverity,
    category: A4ReviewFindingCategory,
    description: String,
    blocking: bool,
    path: Option<String>,
    line: Option<NonZeroU64>,
    evidence_ref: Option<WorkspaceCheckpointRef>,
    confidence: Option<A4ReviewFindingConfidence>,
    source: Option<A4ReviewFindingSource>,
}
impl A4ReviewFinding {
    // The constructor follows the frozen record fields, as do checkpoint records.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        finding_id: String,
        severity: A4ReviewFindingSeverity,
        category: A4ReviewFindingCategory,
        description: String,
        blocking: bool,
        path: Option<String>,
        line: Option<NonZeroU64>,
        evidence_ref: Option<WorkspaceCheckpointRef>,
        confidence: Option<A4ReviewFindingConfidence>,
        source: Option<A4ReviewFindingSource>,
    ) -> Result<Self, A4ReviewConstructionError> {
        validate_id(
            &finding_id,
            A4ReviewConstructionError::EmptyFindingId,
            A4ReviewConstructionError::FindingIdTooLong,
        )?;
        if description.is_empty() {
            return Err(A4ReviewConstructionError::EmptyFindingDescription);
        }
        Ok(Self {
            finding_id,
            severity,
            category,
            description,
            blocking,
            path,
            line,
            evidence_ref,
            confidence,
            source,
        })
    }
    pub fn finding_id(&self) -> &str {
        &self.finding_id
    }
    pub fn severity(&self) -> A4ReviewFindingSeverity {
        self.severity
    }
    pub fn category(&self) -> A4ReviewFindingCategory {
        self.category
    }
    pub fn description(&self) -> &str {
        &self.description
    }
    pub fn blocking(&self) -> bool {
        self.blocking
    }
    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }
    pub fn line(&self) -> Option<NonZeroU64> {
        self.line
    }
    pub fn evidence_ref(&self) -> Option<&WorkspaceCheckpointRef> {
        self.evidence_ref.as_ref()
    }
    pub fn confidence(&self) -> Option<A4ReviewFindingConfidence> {
        self.confidence
    }
    pub fn source(&self) -> Option<A4ReviewFindingSource> {
        self.source
    }
}

/// One assessment; duplicate dimensions and empty notes remain representable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A4ReviewDimensionReview {
    dimension: A4ReviewDimension,
    assessment: A4ReviewDimensionAssessment,
    note: Option<String>,
}
impl A4ReviewDimensionReview {
    pub fn new(
        dimension: A4ReviewDimension,
        assessment: A4ReviewDimensionAssessment,
        note: Option<String>,
    ) -> Self {
        Self {
            dimension,
            assessment,
            note,
        }
    }
    pub fn dimension(&self) -> A4ReviewDimension {
        self.dimension
    }
    pub fn assessment(&self) -> A4ReviewDimensionAssessment {
        self.assessment
    }
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

/// Stored reproduction evidence only. Temporal started_at and finished_at are deferred.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A4ReviewReproductionCheck {
    source: WorkspaceCheckpointCheckSource,
    command: Vec<String>,
    exit_code: i64,
    code_sha: String,
    timed_out: Option<bool>,
    output_ref: Option<WorkspaceCheckpointRef>,
    result: Option<A4ReviewReproductionCheckResult>,
}
impl A4ReviewReproductionCheck {
    pub fn new(
        source: WorkspaceCheckpointCheckSource,
        command: Vec<String>,
        exit_code: i64,
        code_sha: String,
        timed_out: Option<bool>,
        output_ref: Option<WorkspaceCheckpointRef>,
        result: Option<A4ReviewReproductionCheckResult>,
    ) -> Result<Self, A4ReviewConstructionError> {
        if command.is_empty() {
            return Err(A4ReviewConstructionError::EmptyReproductionCommand);
        }
        validate_sha(
            &code_sha,
            A4ReviewConstructionError::MalformedReproductionCodeSha,
        )?;
        Ok(Self {
            source,
            command,
            exit_code,
            code_sha,
            timed_out,
            output_ref,
            result,
        })
    }
    pub fn source(&self) -> WorkspaceCheckpointCheckSource {
        self.source
    }
    pub fn command(&self) -> &[String] {
        &self.command
    }
    pub fn exit_code(&self) -> i64 {
        self.exit_code
    }
    pub fn code_sha(&self) -> &str {
        &self.code_sha
    }
    pub fn timed_out(&self) -> Option<bool> {
        self.timed_out
    }
    pub fn output_ref(&self) -> Option<&WorkspaceCheckpointRef> {
        self.output_ref.as_ref()
    }
    pub fn result(&self) -> Option<A4ReviewReproductionCheckResult> {
        self.result
    }
}

/// Optional reproduction properties are independent; absent checks differ from empty checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A4ReviewReproduction {
    performed: Option<bool>,
    checks: Option<Vec<A4ReviewReproductionCheck>>,
    limitation: A4ReviewReproductionLimitation,
}
impl A4ReviewReproduction {
    pub fn new(
        performed: Option<bool>,
        checks: Option<Vec<A4ReviewReproductionCheck>>,
        limitation: A4ReviewReproductionLimitation,
    ) -> Self {
        Self {
            performed,
            checks,
            limitation,
        }
    }
    pub fn performed(&self) -> Option<bool> {
        self.performed
    }
    pub fn checks(&self) -> Option<&[A4ReviewReproductionCheck]> {
        self.checks.as_deref()
    }
    pub fn limitation(&self) -> &A4ReviewReproductionLimitation {
        &self.limitation
    }
}

/// Structured in-process core, exact SHA-bound for its lifetime.
///
/// reviewed_at, reproduction.checks[].started_at, and
/// reproduction.checks[].finished_at are deferred. This is not a complete
/// wire contract and makes no wire/serde completeness claim. No acceptance
/// policy is applied: all caller-supplied evidence is preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A4ReviewNonTemporalCore {
    review_id: String,
    task_id: String,
    reviewed_sha: String,
    verdict: A4ReviewVerdict,
    independence_attested: bool,
    blocking_findings: Vec<A4ReviewFinding>,
    nonblocking_findings: Vec<A4ReviewFinding>,
    dimensions: Vec<A4ReviewDimensionReview>,
    reviewer: Option<A4ReviewReviewer>,
    reproduction: Option<A4ReviewReproduction>,
    unauthorized_file_changes: Option<Vec<String>>,
    recommended_action: Option<A4ReviewRecommendedAction>,
    rationale: Option<String>,
}
impl A4ReviewNonTemporalCore {
    // The constructor follows the frozen record fields, as do checkpoint records.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        review_id: String,
        task_id: String,
        reviewed_sha: String,
        verdict: A4ReviewVerdict,
        independence_attested: bool,
        blocking_findings: Vec<A4ReviewFinding>,
        nonblocking_findings: Vec<A4ReviewFinding>,
        dimensions: Vec<A4ReviewDimensionReview>,
        reviewer: Option<A4ReviewReviewer>,
        reproduction: Option<A4ReviewReproduction>,
        unauthorized_file_changes: Option<Vec<String>>,
        recommended_action: Option<A4ReviewRecommendedAction>,
        rationale: Option<String>,
    ) -> Result<Self, A4ReviewConstructionError> {
        validate_id(
            &review_id,
            A4ReviewConstructionError::EmptyReviewId,
            A4ReviewConstructionError::ReviewIdTooLong,
        )?;
        validate_id(
            &task_id,
            A4ReviewConstructionError::EmptyTaskId,
            A4ReviewConstructionError::TaskIdTooLong,
        )?;
        validate_sha(
            &reviewed_sha,
            A4ReviewConstructionError::MalformedReviewedSha,
        )?;
        if dimensions.is_empty() {
            return Err(A4ReviewConstructionError::EmptyDimensions);
        }
        Ok(Self {
            review_id,
            task_id,
            reviewed_sha,
            verdict,
            independence_attested,
            blocking_findings,
            nonblocking_findings,
            dimensions,
            reviewer,
            reproduction,
            unauthorized_file_changes,
            recommended_action,
            rationale,
        })
    }
    pub fn review_id(&self) -> &str {
        &self.review_id
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn reviewed_sha(&self) -> &str {
        &self.reviewed_sha
    }
    pub fn verdict(&self) -> A4ReviewVerdict {
        self.verdict
    }
    pub fn independence_attested(&self) -> bool {
        self.independence_attested
    }
    pub fn blocking_findings(&self) -> &[A4ReviewFinding] {
        &self.blocking_findings
    }
    pub fn nonblocking_findings(&self) -> &[A4ReviewFinding] {
        &self.nonblocking_findings
    }
    pub fn dimensions(&self) -> &[A4ReviewDimensionReview] {
        &self.dimensions
    }
    pub fn reviewer(&self) -> Option<&A4ReviewReviewer> {
        self.reviewer.as_ref()
    }
    pub fn reproduction(&self) -> Option<&A4ReviewReproduction> {
        self.reproduction.as_ref()
    }
    pub fn unauthorized_file_changes(&self) -> Option<&[String]> {
        self.unauthorized_file_changes.as_deref()
    }
    pub fn recommended_action(&self) -> Option<A4ReviewRecommendedAction> {
        self.recommended_action
    }
    pub fn rationale(&self) -> Option<&str> {
        self.rationale.as_deref()
    }
}

fn validate_id(
    value: &str,
    empty: A4ReviewConstructionError,
    long: A4ReviewConstructionError,
) -> Result<(), A4ReviewConstructionError> {
    let count = value.chars().count();
    if count == 0 {
        return Err(empty);
    }
    if count > 200 {
        return Err(long);
    }
    Ok(())
}
fn validate_sha(
    value: &str,
    error: A4ReviewConstructionError,
) -> Result<(), A4ReviewConstructionError> {
    if value.len() != 40
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(error);
    }
    Ok(())
}
