//! In-process ReviewCapsule data with optional check timestamps. Constructors validate schema shape only.

use crate::{ReviewDateTimeV1, ReviewRequestNonNegativeInteger};
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
    WorkspaceCheckpointRef,
};
use std::fmt;

/// Closed vocabulary for `acceptance_criteria[].kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCapsuleCriterionKind {
    Deterministic,
    Semantic,
}

impl ReviewCapsuleCriterionKind {
    pub const ALL: [Self; 2] = [Self::Deterministic, Self::Semantic];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Deterministic => "DETERMINISTIC",
            Self::Semantic => "SEMANTIC",
        }
    }
}

/// One structured acceptance criterion. Optional empty arrays and strings are preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCapsuleCriterion {
    id: String,
    description: String,
    kind: ReviewCapsuleCriterionKind,
    check_command: Option<Vec<String>>,
    rationale: Option<String>,
}

impl ReviewCapsuleCriterion {
    pub fn new(
        id: String,
        description: String,
        kind: ReviewCapsuleCriterionKind,
        check_command: Option<Vec<String>>,
        rationale: Option<String>,
    ) -> Result<Self, ReviewCapsuleConstructionError> {
        validate_id(
            &id,
            ReviewCapsuleConstructionError::EmptyCriterionId,
            ReviewCapsuleConstructionError::CriterionIdTooLong,
        )?;
        if description.is_empty() {
            return Err(ReviewCapsuleConstructionError::EmptyCriterionDescription);
        }
        Ok(Self {
            id,
            description,
            kind,
            check_command,
            rationale,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub const fn kind(&self) -> ReviewCapsuleCriterionKind {
        self.kind
    }

    pub fn check_command(&self) -> Option<&[String]> {
        self.check_command.as_deref()
    }

    pub fn rationale(&self) -> Option<&str> {
        self.rationale.as_deref()
    }
}

/// Result evidence for the independent `ReviewCapsule.checks[].result` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCapsuleCheckResult {
    Pass,
    Fail,
    Error,
    Skipped,
    Unknown,
}

impl ReviewCapsuleCheckResult {
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

/// Supplied check evidence; timestamps are independently optional, immutable, and never ordered.
/// Existing `new` omits both timestamps; `new_with_timestamps` preserves supplied values.
///
/// ```
/// use receipts_review_integration::{ReviewCapsuleCheck, ReviewDateTimeV1};
/// fn timestamps(value: &ReviewCapsuleCheck) -> (Option<&ReviewDateTimeV1>, Option<&ReviewDateTimeV1>) {
///     (value.started_at(), value.finished_at())
/// }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewCapsuleCheck) {
/// let _: Option<&mut receipts_review_integration::ReviewDateTimeV1> = value.started_at();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: receipts_review_integration::ReviewCapsuleCheck) {
/// let _ = receipts_review_integration::ReviewCapsuleCheck { started_at: None, ..value };
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewCapsuleCheck) {
/// value.set_started_at(None);
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(core: receipts_workspace_execution::WorkspaceCheckpointExecutedCheckCore) {
/// let _ = receipts_review_integration::ReviewCapsuleCheck::new_with_timestamps(
///     core, None, Some(None), None,
/// );
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(core: receipts_workspace_execution::WorkspaceCheckpointExecutedCheckCore) {
/// let _ = receipts_review_integration::ReviewCapsuleCheck::new_with_timestamps(
///     core, None, Some(String::from("2026-09-08T00:00:00Z")), None,
/// );
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewCapsuleCheck) {
/// let _: Option<&mut receipts_review_integration::ReviewDateTimeV1> = value.finished_at();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: receipts_review_integration::ReviewCapsuleCheck) {
/// let _ = receipts_review_integration::ReviewCapsuleCheck { finished_at: None, ..value };
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewCapsuleCheck) {
/// value.set_finished_at(None);
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(core: receipts_workspace_execution::WorkspaceCheckpointExecutedCheckCore) {
/// let _ = receipts_review_integration::ReviewCapsuleCheck::new_with_timestamps(
///     core, None, None, Some(None),
/// );
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(core: receipts_workspace_execution::WorkspaceCheckpointExecutedCheckCore) {
/// let _ = receipts_review_integration::ReviewCapsuleCheck::new_with_timestamps(
///     core, None, None, Some(String::from("2026-09-08T00:00:00Z")),
/// );
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCapsuleCheck {
    core: WorkspaceCheckpointExecutedCheckCore,
    result: Option<ReviewCapsuleCheckResult>,
    started_at: Option<ReviewDateTimeV1>,
    finished_at: Option<ReviewDateTimeV1>,
}

impl ReviewCapsuleCheck {
    pub const fn new(
        core: WorkspaceCheckpointExecutedCheckCore,
        result: Option<ReviewCapsuleCheckResult>,
    ) -> Self {
        Self::new_with_timestamps(core, result, None, None)
    }

    /// Stores independently optional caller timestamps without chronology policy.
    pub const fn new_with_timestamps(
        core: WorkspaceCheckpointExecutedCheckCore,
        result: Option<ReviewCapsuleCheckResult>,
        started_at: Option<ReviewDateTimeV1>,
        finished_at: Option<ReviewDateTimeV1>,
    ) -> Self {
        Self {
            core,
            result,
            started_at,
            finished_at,
        }
    }

    pub fn started_at(&self) -> Option<&ReviewDateTimeV1> {
        self.started_at.as_ref()
    }

    pub fn finished_at(&self) -> Option<&ReviewDateTimeV1> {
        self.finished_at.as_ref()
    }

    pub const fn source(&self) -> WorkspaceCheckpointCheckSource {
        self.core.source()
    }

    pub fn command(&self) -> &[String] {
        self.core.command()
    }

    pub const fn exit_code(&self) -> i64 {
        self.core.exit_code()
    }

    pub fn code_sha(&self) -> &CommitSha {
        self.core.code_sha()
    }

    pub const fn timed_out(&self) -> Option<bool> {
        self.core.timed_out()
    }

    pub fn output_ref(&self) -> Option<&WorkspaceCheckpointRef> {
        self.core.output_ref()
    }

    pub const fn result(&self) -> Option<ReviewCapsuleCheckResult> {
        self.result
    }

    pub const fn core(&self) -> &WorkspaceCheckpointExecutedCheckCore {
        &self.core
    }
}

/// Closed vocabulary for the requested review scope. It has no runtime behavior here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCapsuleReviewScope {
    Full,
    Security,
    Regression,
    RepairVerification,
}

impl ReviewCapsuleReviewScope {
    pub const ALL: [Self; 4] = [
        Self::Full,
        Self::Security,
        Self::Regression,
        Self::RepairVerification,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "FULL",
            Self::Security => "SECURITY",
            Self::Regression => "REGRESSION",
            Self::RepairVerification => "REPAIR_VERIFICATION",
        }
    }
}

/// Machine-schema severity policy. Category strings are deliberately opaque.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCapsuleSeverityPolicy {
    blocking_categories: Vec<String>,
    nonblocking_categories: Option<Vec<String>>,
}

impl ReviewCapsuleSeverityPolicy {
    pub fn new(
        blocking_categories: Vec<String>,
        nonblocking_categories: Option<Vec<String>>,
    ) -> Result<Self, ReviewCapsuleConstructionError> {
        if blocking_categories.is_empty() {
            return Err(ReviewCapsuleConstructionError::EmptyBlockingCategories);
        }
        Ok(Self {
            blocking_categories,
            nonblocking_categories,
        })
    }

    pub fn blocking_categories(&self) -> &[String] {
        &self.blocking_categories
    }

    pub fn nonblocking_categories(&self) -> Option<&[String]> {
        self.nonblocking_categories.as_deref()
    }
}

/// Deterministic machine-schema construction failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewCapsuleConstructionError {
    EmptyReviewId,
    ReviewIdTooLong,
    EmptyTaskId,
    TaskIdTooLong,
    EmptyAttemptId,
    AttemptIdTooLong,
    MalformedBaselineSha,
    MalformedImplementationSha,
    EmptyObjective,
    EmptyAcceptanceCriteria,
    EmptyCriterionId,
    CriterionIdTooLong,
    EmptyCriterionDescription,
    EmptyAllowedWritePaths,
    EmptyBlockingCategories,
    NegativeContextEpoch,
}

impl fmt::Display for ReviewCapsuleConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyReviewId => f.write_str("review_id is empty"),
            Self::ReviewIdTooLong => f.write_str("review_id exceeds 200 characters"),
            Self::EmptyTaskId => f.write_str("task_id is empty"),
            Self::TaskIdTooLong => f.write_str("task_id exceeds 200 characters"),
            Self::EmptyAttemptId => f.write_str("attempt_id is empty"),
            Self::AttemptIdTooLong => f.write_str("attempt_id exceeds 200 characters"),
            Self::MalformedBaselineSha => f.write_str(
                "baseline_sha must be exactly 40 lowercase ASCII hexadecimal characters",
            ),
            Self::MalformedImplementationSha => f.write_str(
                "implementation_sha must be exactly 40 lowercase ASCII hexadecimal characters",
            ),
            Self::EmptyObjective => f.write_str("objective is empty"),
            Self::EmptyAcceptanceCriteria => f.write_str("acceptance_criteria is empty"),
            Self::EmptyCriterionId => f.write_str("criterion id is empty"),
            Self::CriterionIdTooLong => f.write_str("criterion id exceeds 200 characters"),
            Self::EmptyCriterionDescription => f.write_str("criterion description is empty"),
            Self::EmptyAllowedWritePaths => f.write_str("allowed_write_paths is empty"),
            Self::EmptyBlockingCategories => f.write_str("blocking_categories is empty"),
            Self::NegativeContextEpoch => f.write_str("context_epoch is negative"),
        }
    }
}

impl std::error::Error for ReviewCapsuleConstructionError {}

/// Immutable, exact-SHA-bound, in-process structured ReviewCapsule core.
///
/// This bounded type does not claim complete wire-format closure. Nested checks
/// preserve optional Review timestamps. `test_results` is omitted under the BUILD-A1
/// machine-schema reconciliation. `context_epoch` is the embedded
/// `REVIEWCAPSULE_CONTEXT_EPOCH_FIELD_REPRESENTATION`, not ownership of the
/// State `ContextEpoch` aggregate.
///
/// Required constructor arguments are enforced by the function signature,
/// and private fields prevent construction or mutation around validation.
///
/// ```compile_fail
/// use receipts_review_integration::ReviewCapsuleNonTemporalCore;
/// let _ = ReviewCapsuleNonTemporalCore::new();
/// ```
///
/// No reconciled-away or aggregate-level check APIs exist:
///
/// ```compile_fail
/// # fn inspect(value: &receipts_review_integration::ReviewCapsuleNonTemporalCore) {
/// let _ = value.test_results();
/// let _ = value.started_at();
/// let _ = value.finished_at();
/// # }
/// ```
///
/// The shared integer carrier cannot be replaced or mutated through this API:
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewCapsuleNonTemporalCore) {
/// value.context_epoch = receipts_review_integration::ReviewRequestNonNegativeInteger::from_decimal("0").unwrap();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewCapsuleNonTemporalCore) {
/// let _: &mut receipts_review_integration::ReviewRequestNonNegativeInteger = value.context_epoch();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewCapsuleNonTemporalCore {
    review_id: String,
    task_id: String,
    attempt_id: String,
    baseline_sha: CommitSha,
    implementation_sha: CommitSha,
    objective: String,
    acceptance_criteria: Vec<ReviewCapsuleCriterion>,
    non_goals: Option<Vec<String>>,
    architecture_refs: Option<Vec<WorkspaceCheckpointRef>>,
    contract_refs: Option<Vec<WorkspaceCheckpointRef>>,
    diff: WorkspaceCheckpointRef,
    allowed_write_paths: Vec<String>,
    checks: Option<Vec<ReviewCapsuleCheck>>,
    security_requirements: Option<Vec<String>>,
    review_scope: ReviewCapsuleReviewScope,
    severity_policy: ReviewCapsuleSeverityPolicy,
    reproduction_required: bool,
    structured_output_schema: Option<WorkspaceCheckpointRef>,
    context_epoch: ReviewRequestNonNegativeInteger,
}

impl ReviewCapsuleNonTemporalCore {
    /// Constructs from a legacy `i64` epoch; negative values report `NegativeContextEpoch`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        review_id: String,
        task_id: String,
        attempt_id: String,
        baseline_sha: String,
        implementation_sha: String,
        objective: String,
        acceptance_criteria: Vec<ReviewCapsuleCriterion>,
        non_goals: Option<Vec<String>>,
        architecture_refs: Option<Vec<WorkspaceCheckpointRef>>,
        contract_refs: Option<Vec<WorkspaceCheckpointRef>>,
        diff: WorkspaceCheckpointRef,
        allowed_write_paths: Vec<String>,
        checks: Option<Vec<ReviewCapsuleCheck>>,
        security_requirements: Option<Vec<String>>,
        review_scope: ReviewCapsuleReviewScope,
        severity_policy: ReviewCapsuleSeverityPolicy,
        reproduction_required: bool,
        structured_output_schema: Option<WorkspaceCheckpointRef>,
        context_epoch: i64,
    ) -> Result<Self, ReviewCapsuleConstructionError> {
        Self::build(
            review_id,
            task_id,
            attempt_id,
            baseline_sha,
            implementation_sha,
            objective,
            acceptance_criteria,
            non_goals,
            architecture_refs,
            contract_refs,
            diff,
            allowed_write_paths,
            checks,
            security_requirements,
            review_scope,
            severity_policy,
            reproduction_required,
            structured_output_schema,
            context_epoch,
        )
    }

    /// Constructs from the shared integer carrier without a native-width maximum.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_context_epoch(
        review_id: String,
        task_id: String,
        attempt_id: String,
        baseline_sha: String,
        implementation_sha: String,
        objective: String,
        acceptance_criteria: Vec<ReviewCapsuleCriterion>,
        non_goals: Option<Vec<String>>,
        architecture_refs: Option<Vec<WorkspaceCheckpointRef>>,
        contract_refs: Option<Vec<WorkspaceCheckpointRef>>,
        diff: WorkspaceCheckpointRef,
        allowed_write_paths: Vec<String>,
        checks: Option<Vec<ReviewCapsuleCheck>>,
        security_requirements: Option<Vec<String>>,
        review_scope: ReviewCapsuleReviewScope,
        severity_policy: ReviewCapsuleSeverityPolicy,
        reproduction_required: bool,
        structured_output_schema: Option<WorkspaceCheckpointRef>,
        context_epoch: ReviewRequestNonNegativeInteger,
    ) -> Result<Self, ReviewCapsuleConstructionError> {
        Self::build(
            review_id,
            task_id,
            attempt_id,
            baseline_sha,
            implementation_sha,
            objective,
            acceptance_criteria,
            non_goals,
            architecture_refs,
            contract_refs,
            diff,
            allowed_write_paths,
            checks,
            security_requirements,
            review_scope,
            severity_policy,
            reproduction_required,
            structured_output_schema,
            context_epoch,
        )
    }

    // Both public paths share validation, including the legacy error ordering.
    #[allow(clippy::too_many_arguments)]
    fn build(
        review_id: String,
        task_id: String,
        attempt_id: String,
        baseline_sha: String,
        implementation_sha: String,
        objective: String,
        acceptance_criteria: Vec<ReviewCapsuleCriterion>,
        non_goals: Option<Vec<String>>,
        architecture_refs: Option<Vec<WorkspaceCheckpointRef>>,
        contract_refs: Option<Vec<WorkspaceCheckpointRef>>,
        diff: WorkspaceCheckpointRef,
        allowed_write_paths: Vec<String>,
        checks: Option<Vec<ReviewCapsuleCheck>>,
        security_requirements: Option<Vec<String>>,
        review_scope: ReviewCapsuleReviewScope,
        severity_policy: ReviewCapsuleSeverityPolicy,
        reproduction_required: bool,
        structured_output_schema: Option<WorkspaceCheckpointRef>,
        context_epoch: impl TryInto<ReviewRequestNonNegativeInteger>,
    ) -> Result<Self, ReviewCapsuleConstructionError> {
        validate_id(
            &review_id,
            ReviewCapsuleConstructionError::EmptyReviewId,
            ReviewCapsuleConstructionError::ReviewIdTooLong,
        )?;
        validate_id(
            &task_id,
            ReviewCapsuleConstructionError::EmptyTaskId,
            ReviewCapsuleConstructionError::TaskIdTooLong,
        )?;
        validate_id(
            &attempt_id,
            ReviewCapsuleConstructionError::EmptyAttemptId,
            ReviewCapsuleConstructionError::AttemptIdTooLong,
        )?;
        let baseline_sha = CommitSha::parse(&baseline_sha)
            .map_err(|_| ReviewCapsuleConstructionError::MalformedBaselineSha)?;
        let implementation_sha = CommitSha::parse(&implementation_sha)
            .map_err(|_| ReviewCapsuleConstructionError::MalformedImplementationSha)?;
        if objective.is_empty() {
            return Err(ReviewCapsuleConstructionError::EmptyObjective);
        }
        if acceptance_criteria.is_empty() {
            return Err(ReviewCapsuleConstructionError::EmptyAcceptanceCriteria);
        }
        if allowed_write_paths.is_empty() {
            return Err(ReviewCapsuleConstructionError::EmptyAllowedWritePaths);
        }
        let context_epoch = context_epoch
            .try_into()
            .map_err(|_| ReviewCapsuleConstructionError::NegativeContextEpoch)?;
        Ok(Self {
            review_id,
            task_id,
            attempt_id,
            baseline_sha,
            implementation_sha,
            objective,
            acceptance_criteria,
            non_goals,
            architecture_refs,
            contract_refs,
            diff,
            allowed_write_paths,
            checks,
            security_requirements,
            review_scope,
            severity_policy,
            reproduction_required,
            structured_output_schema,
            context_epoch,
        })
    }

    pub fn review_id(&self) -> &str {
        &self.review_id
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }
    pub fn baseline_sha(&self) -> &CommitSha {
        &self.baseline_sha
    }
    pub fn implementation_sha(&self) -> &CommitSha {
        &self.implementation_sha
    }
    pub fn objective(&self) -> &str {
        &self.objective
    }
    pub fn acceptance_criteria(&self) -> &[ReviewCapsuleCriterion] {
        &self.acceptance_criteria
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
    pub fn diff(&self) -> &WorkspaceCheckpointRef {
        &self.diff
    }
    pub fn allowed_write_paths(&self) -> &[String] {
        &self.allowed_write_paths
    }
    pub fn checks(&self) -> Option<&[ReviewCapsuleCheck]> {
        self.checks.as_deref()
    }
    pub fn security_requirements(&self) -> Option<&[String]> {
        self.security_requirements.as_deref()
    }
    pub const fn review_scope(&self) -> ReviewCapsuleReviewScope {
        self.review_scope
    }
    pub fn severity_policy(&self) -> &ReviewCapsuleSeverityPolicy {
        &self.severity_policy
    }
    pub const fn reproduction_required(&self) -> bool {
        self.reproduction_required
    }
    pub fn structured_output_schema(&self) -> Option<&WorkspaceCheckpointRef> {
        self.structured_output_schema.as_ref()
    }
    pub const fn context_epoch(&self) -> &ReviewRequestNonNegativeInteger {
        &self.context_epoch
    }
}

fn validate_id(
    value: &str,
    empty: ReviewCapsuleConstructionError,
    long: ReviewCapsuleConstructionError,
) -> Result<(), ReviewCapsuleConstructionError> {
    match value.chars().count() {
        0 => Err(empty),
        201.. => Err(long),
        _ => Ok(()),
    }
}
