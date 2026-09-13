//! Review-owned, in-process, non-temporal ReviewRequest data only.
//! Physical authority: `schemas/ReviewRequest.schema.json` at architecture
//! f49d621ee510705939394f7df4996223a73fdcb7 (SHA-256
//! 8d66615fc53b74b2a2f4e919fae4e5914caa35f0756720c7837e4d42028ace55).
//! REQUESTED_AT_STATUS: DEFERRED_NO_AUTHORIZED_REVIEW_TEMPORAL_TYPE
//! FULL_TEMPORAL_REVIEWREQUEST_CLAIMED: NO
//! No wire/serde, persistence, dispatch, capsule construction, or policy execution.

use crate::{
    AssuranceProfile, DistinctProviderPolicy, ReviewCapsuleCriterion, ReviewCapsuleReviewScope,
};
use receipts_workspace_execution::{CommitSha, WorkspaceCheckpointRef};
use std::fmt;

/// Construction failures for the request and its bounded integer carriers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRequestConstructionError {
    InvalidIdentifier(&'static str),
    MalformedBaselineSha,
    MalformedImplementationSha,
    EmptyObjective,
    EmptyAcceptanceCriteria,
    EmptyAllowedWritePaths,
    InvalidDecimalInteger,
    NegativeInteger,
    ZeroAttemptNumber,
}

impl fmt::Display for ReviewRequestConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier(field) => write!(f, "{field} must contain 1..=200 characters"),
            Self::MalformedBaselineSha => {
                f.write_str("baseline_sha must be 40 lowercase ASCII hex characters")
            }
            Self::MalformedImplementationSha => {
                f.write_str("implementation_sha must be 40 lowercase ASCII hex characters")
            }
            Self::EmptyObjective => f.write_str("objective is empty"),
            Self::EmptyAcceptanceCriteria => f.write_str("acceptance_criteria is empty"),
            Self::EmptyAllowedWritePaths => f.write_str("allowed_write_paths is empty"),
            Self::InvalidDecimalInteger => {
                f.write_str("expected decimal integer digits with an optional sign")
            }
            Self::NegativeInteger => f.write_str("integer value must be non-negative"),
            Self::ZeroAttemptNumber => f.write_str("attempt_number must be at least one"),
        }
    }
}
impl std::error::Error for ReviewRequestConstructionError {}

/// Dependency-free, arbitrary-magnitude non-negative integer VALUE carrier.
///
/// Decimal digits are internal storage, not a schema string or a JSON number
/// representation. Every non-negative integer has a finite decimal expansion;
/// no fixed native integer or application maximum limits the domain. Equivalent
/// signed/zero-padded decimal inputs compare equal. No wire lexical rule is
/// implied: a future wire adapter must map integer values to this carrier.
///
/// Used as REVIEWREQUEST_CONTEXT_EPOCH_FIELD_REPRESENTATION, without ownership
/// of State's ContextEpoch aggregate, epoch advancement, or State lookup.
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequestNonNegativeInteger) {
/// value.digits.clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequestNonNegativeInteger) {
/// let _: &mut str = value.decimal_digits();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRequestNonNegativeInteger {
    digits: String,
}

impl ReviewRequestNonNegativeInteger {
    /// Constructs from a decimal-domain spelling, NOT from JSON syntax.
    /// Accepts optional `+`/`-` and leading zeroes; rejects negative values.
    /// `-0` denotes zero. Fractions, exponents and whitespace are not parsed by
    /// this convenience constructor; this does not exclude any integer VALUE.
    pub fn from_decimal(value: &str) -> Result<Self, ReviewRequestConstructionError> {
        let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ReviewRequestConstructionError::InvalidDecimalInteger);
        }
        let magnitude = digits.trim_start_matches('0');
        if value.starts_with('-') && !magnitude.is_empty() {
            return Err(ReviewRequestConstructionError::NegativeInteger);
        }
        Ok(Self {
            digits: if magnitude.is_empty() { "0" } else { magnitude }.into(),
        })
    }

    /// Canonical decimal magnitude for in-process inspection, not serialization.
    pub fn decimal_digits(&self) -> &str {
        &self.digits
    }
}

/// Positive integer value for `attempt_number`, with no finite semantic maximum.
/// Shares the non-negative carrier's decimal storage semantics; not wire data.
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequestAttemptNumber) {
/// value.magnitude = receipts_review_integration::ReviewRequestNonNegativeInteger::from_decimal("0").unwrap();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRequestAttemptNumber {
    magnitude: ReviewRequestNonNegativeInteger,
}

impl ReviewRequestAttemptNumber {
    /// Same decimal-domain input as `ReviewRequestNonNegativeInteger::from_decimal`.
    pub fn from_decimal(value: &str) -> Result<Self, ReviewRequestConstructionError> {
        let magnitude = ReviewRequestNonNegativeInteger::from_decimal(value)?;
        if magnitude.decimal_digits() == "0" {
            return Err(ReviewRequestConstructionError::ZeroAttemptNumber);
        }
        Ok(Self { magnitude })
    }

    pub fn decimal_digits(&self) -> &str {
        self.magnitude.decimal_digits()
    }
}

/// Request policy vocabulary; deliberately distinct from the two-value
/// AssuranceReviewerQualityFloor used by the assurance matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRequestReviewerFloor {
    Frontier,
    Balanced,
    Economy,
}

impl ReviewRequestReviewerFloor {
    pub const ALL: [Self; 3] = [Self::Frontier, Self::Balanced, Self::Economy];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Frontier => "FRONTIER",
            Self::Balanced => "BALANCED",
            Self::Economy => "ECONOMY",
        }
    }
}

/// Optional policy data only. All members may be absent; none implies a default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRequestPolicy {
    required: Option<bool>,
    distinct_provider: Option<DistinctProviderPolicy>,
    reviewer_floor: Option<ReviewRequestReviewerFloor>,
    excluded_session_refs: Option<Vec<String>>,
}

impl ReviewRequestPolicy {
    pub fn new(
        required: Option<bool>,
        distinct_provider: Option<DistinctProviderPolicy>,
        reviewer_floor: Option<ReviewRequestReviewerFloor>,
        excluded_session_refs: Option<Vec<String>>,
    ) -> Self {
        Self {
            required,
            distinct_provider,
            reviewer_floor,
            excluded_session_refs,
        }
    }

    pub fn required(&self) -> Option<bool> {
        self.required
    }
    pub fn distinct_provider(&self) -> Option<DistinctProviderPolicy> {
        self.distinct_provider
    }
    pub fn reviewer_floor(&self) -> Option<ReviewRequestReviewerFloor> {
        self.reviewer_floor
    }
    pub fn excluded_session_refs(&self) -> Option<&[String]> {
        self.excluded_session_refs.as_deref()
    }
}

/// Immutable ReviewRequest structured core; all non-temporal schema fields.
/// REQUESTED_AT_STATUS: DEFERRED_NO_AUTHORIZED_REVIEW_TEMPORAL_TYPE
/// FULL_TEMPORAL_REVIEWREQUEST_CLAIMED: NO
///
/// Required constructor arguments have no defaults. Validated fields and nested
/// values expose shared references only; caller-owned clones preserve validity.
///
/// ```compile_fail
/// let _ = receipts_review_integration::ReviewRequestNonTemporalCore::new();
/// ```
/// ```compile_fail
/// # fn forbidden(value: &receipts_review_integration::ReviewRequestNonTemporalCore) {
/// value.requested_at();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequestNonTemporalCore) {
/// value.request_id = String::new();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequestNonTemporalCore) {
/// let _: &mut receipts_workspace_execution::CommitSha = value.implementation_sha();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequestNonTemporalCore) {
/// let _: &mut receipts_workspace_execution::CommitSha = value.baseline_sha();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequestNonTemporalCore) {
/// value.acceptance_criteria()[0].id = String::new();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequestNonTemporalCore) {
/// value.allowed_write_paths()[0].clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequestNonTemporalCore) {
/// let _: &mut receipts_workspace_execution::WorkspaceCheckpointRef = value.a3_handoff_ref().unwrap();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewRequestNonTemporalCore) {
/// value.review_policy().unwrap().excluded_session_refs().unwrap()[0].clear();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRequestNonTemporalCore {
    request_id: String,
    task_id: String,
    attempt_id: String,
    implementation_sha: CommitSha,
    baseline_sha: CommitSha,
    objective: String,
    acceptance_criteria: Vec<ReviewCapsuleCriterion>,
    allowed_write_paths: Vec<String>,
    assurance_profile: AssuranceProfile,
    review_scope: ReviewCapsuleReviewScope,
    context_epoch: ReviewRequestNonNegativeInteger,
    workstream_id: Option<String>,
    branch: Option<String>,
    non_goals: Option<Vec<String>>,
    architecture_refs: Option<Vec<WorkspaceCheckpointRef>>,
    contract_refs: Option<Vec<WorkspaceCheckpointRef>>,
    a3_handoff_ref: Option<WorkspaceCheckpointRef>,
    review_policy: Option<ReviewRequestPolicy>,
    prior_review_ids: Option<Vec<String>>,
    attempt_number: Option<ReviewRequestAttemptNumber>,
}

impl ReviewRequestNonTemporalCore {
    // Arguments follow the frozen record; compatible nested values validate at construction.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: String,
        task_id: String,
        attempt_id: String,
        implementation_sha: String,
        baseline_sha: String,
        objective: String,
        acceptance_criteria: Vec<ReviewCapsuleCriterion>,
        allowed_write_paths: Vec<String>,
        assurance_profile: AssuranceProfile,
        review_scope: ReviewCapsuleReviewScope,
        context_epoch: ReviewRequestNonNegativeInteger,
        workstream_id: Option<String>,
        branch: Option<String>,
        non_goals: Option<Vec<String>>,
        architecture_refs: Option<Vec<WorkspaceCheckpointRef>>,
        contract_refs: Option<Vec<WorkspaceCheckpointRef>>,
        a3_handoff_ref: Option<WorkspaceCheckpointRef>,
        review_policy: Option<ReviewRequestPolicy>,
        prior_review_ids: Option<Vec<String>>,
        attempt_number: Option<ReviewRequestAttemptNumber>,
    ) -> Result<Self, ReviewRequestConstructionError> {
        validate_id(&request_id, "request_id")?;
        validate_id(&task_id, "task_id")?;
        validate_id(&attempt_id, "attempt_id")?;
        if let Some(id) = &workstream_id {
            validate_id(id, "workstream_id")?;
        }
        if let Some(ids) = &prior_review_ids {
            for id in ids {
                validate_id(id, "prior_review_ids item")?;
            }
        }
        let implementation_sha = CommitSha::parse(&implementation_sha)
            .map_err(|_| ReviewRequestConstructionError::MalformedImplementationSha)?;
        let baseline_sha = CommitSha::parse(&baseline_sha)
            .map_err(|_| ReviewRequestConstructionError::MalformedBaselineSha)?;
        if objective.is_empty() {
            return Err(ReviewRequestConstructionError::EmptyObjective);
        }
        if acceptance_criteria.is_empty() {
            return Err(ReviewRequestConstructionError::EmptyAcceptanceCriteria);
        }
        if allowed_write_paths.is_empty() {
            return Err(ReviewRequestConstructionError::EmptyAllowedWritePaths);
        }
        Ok(Self {
            request_id,
            task_id,
            attempt_id,
            implementation_sha,
            baseline_sha,
            objective,
            acceptance_criteria,
            allowed_write_paths,
            assurance_profile,
            review_scope,
            context_epoch,
            workstream_id,
            branch,
            non_goals,
            architecture_refs,
            contract_refs,
            a3_handoff_ref,
            review_policy,
            prior_review_ids,
            attempt_number,
        })
    }

    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }
    pub fn implementation_sha(&self) -> &CommitSha {
        &self.implementation_sha
    }
    pub fn baseline_sha(&self) -> &CommitSha {
        &self.baseline_sha
    }
    pub fn objective(&self) -> &str {
        &self.objective
    }
    pub fn acceptance_criteria(&self) -> &[ReviewCapsuleCriterion] {
        &self.acceptance_criteria
    }
    pub fn allowed_write_paths(&self) -> &[String] {
        &self.allowed_write_paths
    }
    pub fn assurance_profile(&self) -> AssuranceProfile {
        self.assurance_profile
    }
    pub fn review_scope(&self) -> ReviewCapsuleReviewScope {
        self.review_scope
    }
    pub fn context_epoch(&self) -> &ReviewRequestNonNegativeInteger {
        &self.context_epoch
    }
    pub fn workstream_id(&self) -> Option<&str> {
        self.workstream_id.as_deref()
    }
    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }
    pub fn non_goals(&self) -> Option<&[String]> {
        self.non_goals.as_deref()
    }
    pub fn architecture_refs(&self) -> Option<&[WorkspaceCheckpointRef]> {
        self.architecture_refs.as_deref()
    }
    pub fn contract_refs(&self) -> Option<&[WorkspaceCheckpointRef]> {
        self.contract_refs.as_deref()
    }
    pub fn a3_handoff_ref(&self) -> Option<&WorkspaceCheckpointRef> {
        self.a3_handoff_ref.as_ref()
    }
    pub fn review_policy(&self) -> Option<&ReviewRequestPolicy> {
        self.review_policy.as_ref()
    }
    pub fn prior_review_ids(&self) -> Option<&[String]> {
        self.prior_review_ids.as_deref()
    }
    pub fn attempt_number(&self) -> Option<&ReviewRequestAttemptNumber> {
        self.attempt_number.as_ref()
    }
}

fn validate_id(value: &str, field: &'static str) -> Result<(), ReviewRequestConstructionError> {
    if !(1..=200).contains(&value.chars().count()) {
        return Err(ReviewRequestConstructionError::InvalidIdentifier(field));
    }
    Ok(())
}
