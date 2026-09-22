//! Review-owned, in-process IntegrationRequest data with optional check timestamps.
//! Authority: build-control/orchestrator-architecture/schemas/IntegrationRequest.schema.json
//! at f49d621ee510705939394f7df4996223a73fdcb7.
//! Evidence submitted to an acceptance/integration gate establishes no acceptance,
//! integration, gate PASS, merge authorization, CI success, or BUILD-A1 approval.
//! Pure shape validation and data access only; no wire serialization or persistence.

use crate::ReviewDateTimeV1;
use crate::{
    A4ReviewFindingCategory, A4ReviewFindingConfidence, A4ReviewFindingSeverity,
    A4ReviewFindingSource, AssuranceProfile, IntegrationDecisionNullableString,
    ReviewCapsuleCheckResult,
};
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointRef,
};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationRequestConstructionError {
    InvalidIdentifier(&'static str),
    MalformedSha(&'static str),
    EmptyTasksIncluded,
    EmptyCommand,
    EmptyFindingDescription,
    InvalidDecimalInteger,
    NonPositiveInteger,
}

impl fmt::Display for IntegrationRequestConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier(field) => write!(f, "{field} must contain 1..=200 characters"),
            Self::MalformedSha(field) => {
                write!(f, "{field} must be 40 lowercase ASCII hex characters")
            }
            Self::EmptyTasksIncluded => {
                f.write_str("tasks_included must contain at least one item")
            }
            Self::EmptyCommand => f.write_str("command must contain at least one argv item"),
            Self::EmptyFindingDescription => f.write_str("finding description is empty"),
            Self::InvalidDecimalInteger => {
                f.write_str("expected decimal integer digits with an optional sign")
            }
            Self::NonPositiveInteger => f.write_str("integer value must be at least one"),
        }
    }
}
impl std::error::Error for IntegrationRequestConstructionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationRequestGateLevel {
    A2Acceptance,
    A1Integration,
}

impl IntegrationRequestGateLevel {
    pub const ALL: [Self; 2] = [Self::A2Acceptance, Self::A1Integration];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::A2Acceptance => "A2_ACCEPTANCE",
            Self::A1Integration => "A1_INTEGRATION",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationRequestA4Verdict {
    Pass,
    PassWithNonblockingFindings,
}

impl IntegrationRequestA4Verdict {
    pub const ALL: [Self; 2] = [Self::Pass, Self::PassWithNonblockingFindings];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::PassWithNonblockingFindings => "PASS_WITH_NONBLOCKING_FINDINGS",
        }
    }
}

/// Arbitrary-magnitude signed integer VALUE, with a canonical decimal magnitude.
/// No finite application bound. Storage and inspection are not a wire format.
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestSignedInteger) {
/// value.digits.clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestSignedInteger) {
/// let _: &mut str = value.decimal_digits();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationRequestSignedInteger {
    negative: bool,
    digits: String,
}

impl IntegrationRequestSignedInteger {
    /// Convenience constructor for the mathematical integer value domain.
    /// Optional sign and leading zeroes canonicalize: 0, +0, -0 and 000 are equal;
    /// 00123 and +123 are equal, as are -00123 and -123.
    /// Fractions, exponents, whitespace and non-ASCII digits are not decimal-domain
    /// spellings accepted here; this imposes no wire lexical rules.
    pub fn from_decimal(value: &str) -> Result<Self, IntegrationRequestConstructionError> {
        let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(IntegrationRequestConstructionError::InvalidDecimalInteger);
        }
        let magnitude = digits.trim_start_matches('0');
        Ok(Self {
            negative: value.starts_with('-') && !magnitude.is_empty(),
            digits: if magnitude.is_empty() { "0" } else { magnitude }.into(),
        })
    }

    pub fn is_negative(&self) -> bool {
        self.negative
    }
    /// Canonical magnitude, without a sign; zero is exactly "0".
    pub fn decimal_digits(&self) -> &str {
        &self.digits
    }
}

/// Positive arbitrary-magnitude integer VALUE for finding.line, with no maximum.
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestPositiveInteger) {
/// value.value = receipts_review_integration::IntegrationRequestSignedInteger::from_decimal("0").unwrap();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestPositiveInteger) {
/// let _: &mut str = value.decimal_digits();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationRequestPositiveInteger {
    value: IntegrationRequestSignedInteger,
}

impl IntegrationRequestPositiveInteger {
    /// Decimal-domain convenience constructor, not a wire parser. Accepts the
    /// signed carrier's spellings only when the mathematical value is positive.
    pub fn from_decimal(value: &str) -> Result<Self, IntegrationRequestConstructionError> {
        let value = IntegrationRequestSignedInteger::from_decimal(value)?;
        if value.is_negative() || value.decimal_digits() == "0" {
            return Err(IntegrationRequestConstructionError::NonPositiveInteger);
        }
        Ok(Self { value })
    }
    pub fn decimal_digits(&self) -> &str {
        self.value.decimal_digits()
    }
}

/// Supplied task evidence; implementation and review SHAs need not be equal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationRequestTask {
    task_id: String,
    implementation_sha: CommitSha,
    review_sha: CommitSha,
    a4_verdict: IntegrationRequestA4Verdict,
    findings_disposition: Option<String>,
    merge_commit: IntegrationDecisionNullableString,
}

impl IntegrationRequestTask {
    pub fn new(
        task_id: String,
        implementation_sha: String,
        review_sha: String,
        a4_verdict: IntegrationRequestA4Verdict,
        findings_disposition: Option<String>,
        merge_commit: IntegrationDecisionNullableString,
    ) -> Result<Self, IntegrationRequestConstructionError> {
        validate_id(&task_id, "task_id")?;
        let implementation_sha = CommitSha::parse(&implementation_sha)
            .map_err(|_| IntegrationRequestConstructionError::MalformedSha("implementation_sha"))?;
        let review_sha = CommitSha::parse(&review_sha)
            .map_err(|_| IntegrationRequestConstructionError::MalformedSha("review_sha"))?;
        Ok(Self {
            task_id,
            implementation_sha,
            review_sha,
            a4_verdict,
            findings_disposition,
            merge_commit,
        })
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn implementation_sha(&self) -> &CommitSha {
        &self.implementation_sha
    }
    pub fn review_sha(&self) -> &CommitSha {
        &self.review_sha
    }
    pub fn a4_verdict(&self) -> IntegrationRequestA4Verdict {
        self.a4_verdict
    }
    pub fn findings_disposition(&self) -> Option<&str> {
        self.findings_disposition.as_deref()
    }
    pub fn merge_commit(&self) -> &IntegrationDecisionNullableString {
        &self.merge_commit
    }
}

/// Supplied check evidence; timestamps are independently optional, immutable, and never ordered.
/// Existing `new` omits both timestamps; `new_with_timestamps` preserves supplied values.
///
/// ```
/// use receipts_review_integration::{IntegrationRequestPostMergeCheck, ReviewDateTimeV1};
/// fn timestamps(value: &IntegrationRequestPostMergeCheck) -> (Option<&ReviewDateTimeV1>, Option<&ReviewDateTimeV1>) {
///     (value.started_at(), value.finished_at())
/// }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestPostMergeCheck) {
/// let _: Option<&mut receipts_review_integration::ReviewDateTimeV1> = value.started_at();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: receipts_review_integration::IntegrationRequestPostMergeCheck) {
/// let _ = receipts_review_integration::IntegrationRequestPostMergeCheck { started_at: None, ..value };
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestPostMergeCheck) {
/// value.set_started_at(None);
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden() {
/// let _ = receipts_review_integration::IntegrationRequestPostMergeCheck::new_with_timestamps(
///     receipts_workspace_execution::WorkspaceCheckpointCheckSource::ReviewExecution, vec!["tool".into()], receipts_review_integration::IntegrationRequestSignedInteger::from_decimal("0").unwrap(), "a".repeat(40), None, None, None, Some(None), None,
/// );
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden() {
/// let _ = receipts_review_integration::IntegrationRequestPostMergeCheck::new_with_timestamps(
///     receipts_workspace_execution::WorkspaceCheckpointCheckSource::ReviewExecution, vec!["tool".into()], receipts_review_integration::IntegrationRequestSignedInteger::from_decimal("0").unwrap(), "a".repeat(40), None, None, None, Some(String::from("2026-09-08T00:00:00Z")), None,
/// );
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestPostMergeCheck) {
/// let _: Option<&mut receipts_review_integration::ReviewDateTimeV1> = value.finished_at();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: receipts_review_integration::IntegrationRequestPostMergeCheck) {
/// let _ = receipts_review_integration::IntegrationRequestPostMergeCheck { finished_at: None, ..value };
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestPostMergeCheck) {
/// value.set_finished_at(None);
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden() {
/// let _ = receipts_review_integration::IntegrationRequestPostMergeCheck::new_with_timestamps(
///     receipts_workspace_execution::WorkspaceCheckpointCheckSource::ReviewExecution, vec!["tool".into()], receipts_review_integration::IntegrationRequestSignedInteger::from_decimal("0").unwrap(), "a".repeat(40), None, None, None, None, Some(None),
/// );
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden() {
/// let _ = receipts_review_integration::IntegrationRequestPostMergeCheck::new_with_timestamps(
///     receipts_workspace_execution::WorkspaceCheckpointCheckSource::ReviewExecution, vec!["tool".into()], receipts_review_integration::IntegrationRequestSignedInteger::from_decimal("0").unwrap(), "a".repeat(40), None, None, None, None, Some(String::from("2026-09-08T00:00:00Z")),
/// );
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationRequestPostMergeCheck {
    source: WorkspaceCheckpointCheckSource,
    command: Vec<String>,
    exit_code: IntegrationRequestSignedInteger,
    code_sha: CommitSha,
    timed_out: Option<bool>,
    output_ref: Option<WorkspaceCheckpointRef>,
    result: Option<ReviewCapsuleCheckResult>,
    started_at: Option<ReviewDateTimeV1>,
    finished_at: Option<ReviewDateTimeV1>,
}

impl IntegrationRequestPostMergeCheck {
    pub fn new(
        source: WorkspaceCheckpointCheckSource,
        command: Vec<String>,
        exit_code: IntegrationRequestSignedInteger,
        code_sha: String,
        timed_out: Option<bool>,
        output_ref: Option<WorkspaceCheckpointRef>,
        result: Option<ReviewCapsuleCheckResult>,
    ) -> Result<Self, IntegrationRequestConstructionError> {
        Self::new_with_timestamps(
            source, command, exit_code, code_sha, timed_out, output_ref, result, None, None,
        )
    }

    /// Stores independently optional caller timestamps without chronology policy.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_timestamps(
        source: WorkspaceCheckpointCheckSource,
        command: Vec<String>,
        exit_code: IntegrationRequestSignedInteger,
        code_sha: String,
        timed_out: Option<bool>,
        output_ref: Option<WorkspaceCheckpointRef>,
        result: Option<ReviewCapsuleCheckResult>,
        started_at: Option<ReviewDateTimeV1>,
        finished_at: Option<ReviewDateTimeV1>,
    ) -> Result<Self, IntegrationRequestConstructionError> {
        if command.is_empty() {
            return Err(IntegrationRequestConstructionError::EmptyCommand);
        }
        let code_sha = CommitSha::parse(&code_sha)
            .map_err(|_| IntegrationRequestConstructionError::MalformedSha("code_sha"))?;
        Ok(Self {
            source,
            command,
            exit_code,
            code_sha,
            timed_out,
            output_ref,
            result,
            started_at,
            finished_at,
        })
    }
    pub fn started_at(&self) -> Option<&ReviewDateTimeV1> {
        self.started_at.as_ref()
    }

    pub fn finished_at(&self) -> Option<&ReviewDateTimeV1> {
        self.finished_at.as_ref()
    }

    pub fn source(&self) -> WorkspaceCheckpointCheckSource {
        self.source
    }
    pub fn command(&self) -> &[String] {
        &self.command
    }
    pub fn exit_code(&self) -> &IntegrationRequestSignedInteger {
        &self.exit_code
    }
    pub fn code_sha(&self) -> &CommitSha {
        &self.code_sha
    }
    pub fn timed_out(&self) -> Option<bool> {
        self.timed_out
    }
    pub fn output_ref(&self) -> Option<&WorkspaceCheckpointRef> {
        self.output_ref.as_ref()
    }
    pub fn result(&self) -> Option<ReviewCapsuleCheckResult> {
        self.result
    }
}

/// Supplied finding data: blocking=true is valid despite the collection name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationRequestOpenFinding {
    finding_id: String,
    severity: A4ReviewFindingSeverity,
    category: A4ReviewFindingCategory,
    description: String,
    blocking: bool,
    path: Option<String>,
    line: Option<IntegrationRequestPositiveInteger>,
    evidence_ref: Option<WorkspaceCheckpointRef>,
    confidence: Option<A4ReviewFindingConfidence>,
    source: Option<A4ReviewFindingSource>,
}

impl IntegrationRequestOpenFinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        finding_id: String,
        severity: A4ReviewFindingSeverity,
        category: A4ReviewFindingCategory,
        description: String,
        blocking: bool,
        path: Option<String>,
        line: Option<IntegrationRequestPositiveInteger>,
        evidence_ref: Option<WorkspaceCheckpointRef>,
        confidence: Option<A4ReviewFindingConfidence>,
        source: Option<A4ReviewFindingSource>,
    ) -> Result<Self, IntegrationRequestConstructionError> {
        validate_id(&finding_id, "finding_id")?;
        if description.is_empty() {
            return Err(IntegrationRequestConstructionError::EmptyFindingDescription);
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
    pub fn line(&self) -> Option<&IntegrationRequestPositiveInteger> {
        self.line.as_ref()
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

/// Optional caller declarations. No defaults, computation, or consistency inference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationRequestAttestations {
    all_tasks_accepted: Option<bool>,
    review_sha_equals_implementation_sha: Option<bool>,
    no_commit_after_review: Option<bool>,
    no_unresolved_blocking_finding: Option<bool>,
    no_frozen_artifact_modified: Option<bool>,
    no_out_of_scope_write: Option<bool>,
}

impl IntegrationRequestAttestations {
    pub fn new(
        all_tasks_accepted: Option<bool>,
        review_sha_equals_implementation_sha: Option<bool>,
        no_commit_after_review: Option<bool>,
        no_unresolved_blocking_finding: Option<bool>,
        no_frozen_artifact_modified: Option<bool>,
        no_out_of_scope_write: Option<bool>,
    ) -> Self {
        Self {
            all_tasks_accepted,
            review_sha_equals_implementation_sha,
            no_commit_after_review,
            no_unresolved_blocking_finding,
            no_frozen_artifact_modified,
            no_out_of_scope_write,
        }
    }
    pub fn all_tasks_accepted(&self) -> Option<bool> {
        self.all_tasks_accepted
    }
    pub fn review_sha_equals_implementation_sha(&self) -> Option<bool> {
        self.review_sha_equals_implementation_sha
    }
    pub fn no_commit_after_review(&self) -> Option<bool> {
        self.no_commit_after_review
    }
    pub fn no_unresolved_blocking_finding(&self) -> Option<bool> {
        self.no_unresolved_blocking_finding
    }
    pub fn no_frozen_artifact_modified(&self) -> Option<bool> {
        self.no_frozen_artifact_modified
    }
    pub fn no_out_of_scope_write(&self) -> Option<bool> {
        self.no_out_of_scope_write
    }
}

/// Closed immutable non-temporal request. Required inputs cannot be omitted.
/// Optional collections preserve absence, empty values, order and duplicates.
/// An absent attestation object differs from a present object with no members.
/// Structurally legal contradictory evidence remains representable.
///
/// ```compile_fail
/// let _ = receipts_review_integration::IntegrationRequestNonTemporalCore::new();
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// value.evaluate();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// value.request_id = String::new();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// let _: &mut str = value.request_id();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// value.tasks_included().clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// value.tasks_included()[0].task_id = String::new();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// let _: &mut receipts_workspace_execution::CommitSha = value.source_sha();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// *value.tasks_included()[0].merge_commit() = receipts_review_integration::IntegrationDecisionNullableString::Null;
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// value.post_merge_checks().unwrap()[0].command()[0].clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// value.open_nonblocking_findings().unwrap()[0].description = String::new();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// value.known_limitations().unwrap()[0].clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::IntegrationRequestNonTemporalCore) {
/// value.attestations().unwrap().all_tasks_accepted = Some(true);
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrationRequestNonTemporalCore {
    request_id: String,
    gate_level: IntegrationRequestGateLevel,
    source_branch: String,
    source_sha: CommitSha,
    target_ref: String,
    tasks_included: Vec<IntegrationRequestTask>,
    assurance_profile: Option<AssuranceProfile>,
    post_merge_checks: Option<Vec<IntegrationRequestPostMergeCheck>>,
    open_nonblocking_findings: Option<Vec<IntegrationRequestOpenFinding>>,
    known_limitations: Option<Vec<String>>,
    attestations: Option<IntegrationRequestAttestations>,
}

impl IntegrationRequestNonTemporalCore {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        request_id: String,
        gate_level: IntegrationRequestGateLevel,
        source_branch: String,
        source_sha: String,
        target_ref: String,
        tasks_included: Vec<IntegrationRequestTask>,
        assurance_profile: Option<AssuranceProfile>,
        post_merge_checks: Option<Vec<IntegrationRequestPostMergeCheck>>,
        open_nonblocking_findings: Option<Vec<IntegrationRequestOpenFinding>>,
        known_limitations: Option<Vec<String>>,
        attestations: Option<IntegrationRequestAttestations>,
    ) -> Result<Self, IntegrationRequestConstructionError> {
        validate_id(&request_id, "request_id")?;
        if tasks_included.is_empty() {
            return Err(IntegrationRequestConstructionError::EmptyTasksIncluded);
        }
        let source_sha = CommitSha::parse(&source_sha)
            .map_err(|_| IntegrationRequestConstructionError::MalformedSha("source_sha"))?;
        Ok(Self {
            request_id,
            gate_level,
            source_branch,
            source_sha,
            target_ref,
            tasks_included,
            assurance_profile,
            post_merge_checks,
            open_nonblocking_findings,
            known_limitations,
            attestations,
        })
    }
    pub fn request_id(&self) -> &str {
        &self.request_id
    }
    pub fn gate_level(&self) -> IntegrationRequestGateLevel {
        self.gate_level
    }
    pub fn source_branch(&self) -> &str {
        &self.source_branch
    }
    pub fn source_sha(&self) -> &CommitSha {
        &self.source_sha
    }
    pub fn target_ref(&self) -> &str {
        &self.target_ref
    }
    pub fn tasks_included(&self) -> &[IntegrationRequestTask] {
        &self.tasks_included
    }
    pub fn assurance_profile(&self) -> Option<AssuranceProfile> {
        self.assurance_profile
    }
    pub fn post_merge_checks(&self) -> Option<&[IntegrationRequestPostMergeCheck]> {
        self.post_merge_checks.as_deref()
    }
    pub fn open_nonblocking_findings(&self) -> Option<&[IntegrationRequestOpenFinding]> {
        self.open_nonblocking_findings.as_deref()
    }
    pub fn known_limitations(&self) -> Option<&[String]> {
        self.known_limitations.as_deref()
    }
    pub fn attestations(&self) -> Option<&IntegrationRequestAttestations> {
        self.attestations.as_ref()
    }
}

fn validate_id(
    value: &str,
    field: &'static str,
) -> Result<(), IntegrationRequestConstructionError> {
    if !(1..=200).contains(&value.chars().count()) {
        return Err(IntegrationRequestConstructionError::InvalidIdentifier(
            field,
        ));
    }
    Ok(())
}
