//! Review-owned, in-process, structured, non-temporal IntegrationDecision data only.
//! Authority: build-control/orchestrator-architecture/schemas/IntegrationDecision.schema.json
//! at f49d621ee510705939394f7df4996223a73fdcb7.
//! DECIDED_AT_STATUS: DEFERRED_NO_AUTHORIZED_REVIEW_TEMPORAL_TYPE
//! FULL_TEMPORAL_INTEGRATIONDECISION_CLAIMED: NO
//! Construction validates shape only; no gate evaluation, wire serde or persistence.

use receipts_workspace_execution::CommitSha;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationDecisionOutcome {
    Accept,
    Repair,
    Blocked,
    HumanRequired,
}

impl IntegrationDecisionOutcome {
    pub const ALL: [Self; 4] = [
        Self::Accept,
        Self::Repair,
        Self::Blocked,
        Self::HumanRequired,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accept => "ACCEPT",
            Self::Repair => "REPAIR",
            Self::Blocked => "BLOCKED",
            Self::HumanRequired => "HUMAN_REQUIRED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationDecisionCheckResult {
    Pass,
    Fail,
    NotApplicable,
}

impl IntegrationDecisionCheckResult {
    pub const ALL: [Self; 3] = [Self::Pass, Self::Fail, Self::NotApplicable];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::NotApplicable => "NOT_APPLICABLE",
        }
    }
}

/// Optional nullable string data. Strings are opaque, including empty and non-SHA values.
/// This in-process representation neither parses nor serializes wire data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrationDecisionNullableString {
    Omitted,
    Null,
    String(String),
}

/// Check evidence supplied by the caller, with no outcome inference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationDecisionCheck {
    check: String,
    result: IntegrationDecisionCheckResult,
    detail: Option<String>,
}

impl IntegrationDecisionCheck {
    pub fn new(
        check: String,
        result: IntegrationDecisionCheckResult,
        detail: Option<String>,
    ) -> Self {
        Self {
            check,
            result,
            detail,
        }
    }

    pub fn check(&self) -> &str {
        &self.check
    }
    pub fn result(&self) -> IntegrationDecisionCheckResult {
        self.result
    }
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationDecisionConstructionError {
    InvalidIdentifier(&'static str),
    EmptyChecksEvaluated,
    MalformedStartSha,
    MalformedImplementationSha,
    MalformedReviewSha,
}

impl fmt::Display for IntegrationDecisionConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier(field) => write!(f, "{field} must contain 1..=200 characters"),
            Self::EmptyChecksEvaluated => {
                f.write_str("checks_evaluated must contain at least one item")
            }
            Self::MalformedStartSha => {
                f.write_str("start_sha must be 40 lowercase ASCII hex characters")
            }
            Self::MalformedImplementationSha => {
                f.write_str("implementation_sha must be 40 lowercase ASCII hex characters")
            }
            Self::MalformedReviewSha => {
                f.write_str("review_sha must be 40 lowercase ASCII hex characters")
            }
        }
    }
}
impl std::error::Error for IntegrationDecisionConstructionError {}

/// Immutable provenance data; only the three required SHAs have a SHA constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationDecisionProvenance {
    task_id: String,
    start_sha: CommitSha,
    implementation_sha: CommitSha,
    review_sha: CommitSha,
    acceptance_sha: IntegrationDecisionNullableString,
    integration_sha: IntegrationDecisionNullableString,
}

impl IntegrationDecisionProvenance {
    pub fn new(
        task_id: String,
        start_sha: String,
        implementation_sha: String,
        review_sha: String,
        acceptance_sha: IntegrationDecisionNullableString,
        integration_sha: IntegrationDecisionNullableString,
    ) -> Result<Self, IntegrationDecisionConstructionError> {
        validate_id(&task_id, "task_id")?;
        let start_sha = CommitSha::parse(&start_sha)
            .map_err(|_| IntegrationDecisionConstructionError::MalformedStartSha)?;
        let implementation_sha = CommitSha::parse(&implementation_sha)
            .map_err(|_| IntegrationDecisionConstructionError::MalformedImplementationSha)?;
        let review_sha = CommitSha::parse(&review_sha)
            .map_err(|_| IntegrationDecisionConstructionError::MalformedReviewSha)?;
        Ok(Self {
            task_id,
            start_sha,
            implementation_sha,
            review_sha,
            acceptance_sha,
            integration_sha,
        })
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn start_sha(&self) -> &CommitSha {
        &self.start_sha
    }
    pub fn implementation_sha(&self) -> &CommitSha {
        &self.implementation_sha
    }
    pub fn review_sha(&self) -> &CommitSha {
        &self.review_sha
    }
    pub fn acceptance_sha(&self) -> &IntegrationDecisionNullableString {
        &self.acceptance_sha
    }
    pub fn integration_sha(&self) -> &IntegrationDecisionNullableString {
        &self.integration_sha
    }
}

/// All frozen non-temporal fields, with required explicit inputs and read-only access.
/// Optional arrays preserve absence, present emptiness, ordering and duplicates.
/// DECIDED_AT_STATUS: DEFERRED_NO_AUTHORIZED_REVIEW_TEMPORAL_TYPE
/// FULL_TEMPORAL_INTEGRATIONDECISION_CLAIMED: NO
///
/// ```compile_fail
/// let _ = receipts_review_integration::IntegrationDecisionNonTemporalCore::new();
/// ```
/// ```compile_fail
/// # fn forbidden(value: &receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// value.decided_at();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// value.evaluate();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// value.decision_id = String::new();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// let _: &mut str = value.request_id();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// value.checks_evaluated().clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// value.checks_evaluated()[0].check = String::new();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// value.provenance_chain().unwrap()[0].task_id = String::new();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// let _: &mut receipts_workspace_execution::CommitSha = value.provenance_chain().unwrap()[0].start_sha();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// let _: &mut receipts_workspace_execution::CommitSha = value.provenance_chain().unwrap()[0].implementation_sha();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// let _: &mut receipts_workspace_execution::CommitSha = value.provenance_chain().unwrap()[0].review_sha();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// *value.provenance_chain().unwrap()[0].acceptance_sha() = receipts_review_integration::IntegrationDecisionNullableString::Null;
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// *value.provenance_chain().unwrap()[0].integration_sha() = receipts_review_integration::IntegrationDecisionNullableString::Null;
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// *value.integration_sha() = receipts_review_integration::IntegrationDecisionNullableString::Null;
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationDecisionNonTemporalCore) {
/// value.unmet_conditions().unwrap()[0].clear();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationDecisionNonTemporalCore {
    decision_id: String,
    request_id: String,
    outcome: IntegrationDecisionOutcome,
    checks_evaluated: Vec<IntegrationDecisionCheck>,
    provenance_chain: Option<Vec<IntegrationDecisionProvenance>>,
    unmet_conditions: Option<Vec<String>>,
    integration_sha: IntegrationDecisionNullableString,
}

impl IntegrationDecisionNonTemporalCore {
    pub fn new(
        decision_id: String,
        request_id: String,
        outcome: IntegrationDecisionOutcome,
        checks_evaluated: Vec<IntegrationDecisionCheck>,
        provenance_chain: Option<Vec<IntegrationDecisionProvenance>>,
        unmet_conditions: Option<Vec<String>>,
        integration_sha: IntegrationDecisionNullableString,
    ) -> Result<Self, IntegrationDecisionConstructionError> {
        validate_id(&decision_id, "decision_id")?;
        validate_id(&request_id, "request_id")?;
        if checks_evaluated.is_empty() {
            return Err(IntegrationDecisionConstructionError::EmptyChecksEvaluated);
        }
        Ok(Self {
            decision_id,
            request_id,
            outcome,
            checks_evaluated,
            provenance_chain,
            unmet_conditions,
            integration_sha,
        })
    }

    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn outcome(&self) -> IntegrationDecisionOutcome {
        self.outcome
    }
    pub fn checks_evaluated(&self) -> &[IntegrationDecisionCheck] {
        &self.checks_evaluated
    }
    pub fn provenance_chain(&self) -> Option<&[IntegrationDecisionProvenance]> {
        self.provenance_chain.as_deref()
    }
    pub fn unmet_conditions(&self) -> Option<&[String]> {
        self.unmet_conditions.as_deref()
    }
    pub fn integration_sha(&self) -> &IntegrationDecisionNullableString {
        &self.integration_sha
    }
}

fn validate_id(
    value: &str,
    field: &'static str,
) -> Result<(), IntegrationDecisionConstructionError> {
    if !(1..=200).contains(&value.chars().count()) {
        return Err(IntegrationDecisionConstructionError::InvalidIdentifier(
            field,
        ));
    }
    Ok(())
}
