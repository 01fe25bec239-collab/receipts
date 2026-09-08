//! Machine-authoritative A3Handoff data only; temporal check fields are deferred.
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
    WorkspaceCheckpointRef,
};
use std::fmt;

/// ```compile_fail
/// let _ = receipts_review_integration::A3HandoffCheckResult::Other;
/// ```
/// ```compile_fail
/// let _: receipts_review_integration::A3HandoffCheckResult = "PASS".into();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A3HandoffCheckResult {
    Pass,
    Fail,
    Error,
    Skipped,
    Unknown,
}
impl A3HandoffCheckResult {
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

/// ```compile_fail
/// let _ = receipts_review_integration::A3HandoffEvidenceLabel::Other;
/// ```
/// ```compile_fail
/// let _: receipts_review_integration::A3HandoffEvidenceLabel = "PASS".into();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A3HandoffEvidenceLabel {
    Implemented,
    Tested,
    NotTested,
    Blocked,
    Assumed,
}
impl A3HandoffEvidenceLabel {
    pub const ALL: [Self; 5] = [
        Self::Implemented,
        Self::Tested,
        Self::NotTested,
        Self::Blocked,
        Self::Assumed,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Implemented => "IMPLEMENTED",
            Self::Tested => "TESTED",
            Self::NotTested => "NOT_TESTED",
            Self::Blocked => "BLOCKED",
            Self::Assumed => "ASSUMED",
        }
    }
}

/// `None` at the aggregate means omitted; this enum preserves null versus text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A3HandoffBlocker {
    ExplicitNull,
    Text(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A3HandoffConstructionError {
    EmptyTaskId,
    TaskIdTooLong,
    EmptyAttemptId,
    AttemptIdTooLong,
    MalformedStartSha,
    MalformedFinalSha,
    EmptyProviderId,
    EmptyModelId,
    EmptyRuntimeId,
    EmptyBindingId,
    BindingIdTooLong,
    EmptySubtaskRequest,
    SubtaskRequestTooLong,
}
impl fmt::Display for A3HandoffConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTaskId => f.write_str("task_id is empty"),
            Self::TaskIdTooLong => f.write_str("task_id exceeds 200 characters"),
            Self::EmptyAttemptId => f.write_str("attempt_id is empty"),
            Self::AttemptIdTooLong => f.write_str("attempt_id exceeds 200 characters"),
            Self::MalformedStartSha => {
                f.write_str("start_sha must be exactly 40 lowercase ASCII hexadecimal characters")
            }
            Self::MalformedFinalSha => {
                f.write_str("final_sha must be exactly 40 lowercase ASCII hexadecimal characters")
            }
            Self::EmptyProviderId => f.write_str("implementer provider_id is empty"),
            Self::EmptyModelId => f.write_str("implementer model_id is empty"),
            Self::EmptyRuntimeId => f.write_str("implementer runtime_id is empty"),
            Self::EmptyBindingId => f.write_str("implementer binding_id is empty"),
            Self::BindingIdTooLong => f.write_str("implementer binding_id exceeds 200 characters"),
            Self::EmptySubtaskRequest => f.write_str("subtask request id is empty"),
            Self::SubtaskRequestTooLong => f.write_str("subtask request id exceeds 200 characters"),
        }
    }
}
impl std::error::Error for A3HandoffConstructionError {}

/// Optional opaque identity; an empty object is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A3HandoffImplementer {
    provider_id: Option<String>,
    model_id: Option<String>,
    runtime_id: Option<String>,
    binding_id: Option<String>,
}
impl A3HandoffImplementer {
    pub fn new(
        provider_id: Option<String>,
        model_id: Option<String>,
        runtime_id: Option<String>,
        binding_id: Option<String>,
    ) -> Result<Self, A3HandoffConstructionError> {
        if provider_id.as_deref() == Some("") {
            return Err(A3HandoffConstructionError::EmptyProviderId);
        }
        if model_id.as_deref() == Some("") {
            return Err(A3HandoffConstructionError::EmptyModelId);
        }
        if runtime_id.as_deref() == Some("") {
            return Err(A3HandoffConstructionError::EmptyRuntimeId);
        }
        if let Some(value) = &binding_id {
            validate_id(
                value,
                A3HandoffConstructionError::EmptyBindingId,
                A3HandoffConstructionError::BindingIdTooLong,
            )?;
        }
        Ok(Self {
            provider_id,
            model_id,
            runtime_id,
            binding_id,
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
    pub fn binding_id(&self) -> Option<&str> {
        self.binding_id.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A3HandoffContractConsumed {
    contract_id: String,
    version: String,
}
impl A3HandoffContractConsumed {
    pub fn new(contract_id: String, version: String) -> Self {
        Self {
            contract_id,
            version,
        }
    }
    pub fn contract_id(&self) -> &str {
        &self.contract_id
    }
    pub fn version(&self) -> &str {
        &self.version
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A3HandoffLabeledEvidence {
    claim: String,
    label: A3HandoffEvidenceLabel,
}
impl A3HandoffLabeledEvidence {
    pub fn new(claim: String, label: A3HandoffEvidenceLabel) -> Self {
        Self { claim, label }
    }
    pub fn claim(&self) -> &str {
        &self.claim
    }
    pub fn label(&self) -> A3HandoffEvidenceLabel {
        self.label
    }
}

/// An A3Handoff check delegates its compatible physical fields to Workspace.
/// `started_at` and `finished_at` are deferred because no temporal type is authorized.
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffCheck) {
/// value.started_at();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffCheck) {
/// value.finished_at();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffCheck) {
/// value.set_started_at(String::new());
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffCheck) {
/// value.set_finished_at(String::new());
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A3HandoffCheck {
    core: WorkspaceCheckpointExecutedCheckCore,
    result: Option<A3HandoffCheckResult>,
}

impl A3HandoffCheck {
    pub const fn new(
        core: WorkspaceCheckpointExecutedCheckCore,
        result: Option<A3HandoffCheckResult>,
    ) -> Self {
        Self { core, result }
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

    pub const fn result(&self) -> Option<A3HandoffCheckResult> {
        self.result
    }

    pub const fn core(&self) -> &WorkspaceCheckpointExecutedCheckCore {
        &self.core
    }
}

/// Immutable non-temporal A3Handoff evidence. No readiness or verification policy.
///
/// Check `started_at` and `finished_at` are deferred because no generic datetime
/// physical type is authorized. This is not complete temporal or wire support.
/// Required fields cannot be omitted, and accepted evidence cannot be rebound.
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffNonTemporalCore) {
/// value.start_sha = value.final_sha().clone();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffNonTemporalCore) {
/// value.final_sha = value.start_sha().clone();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffNonTemporalCore) {
/// let _: &mut receipts_workspace_execution::CommitSha = value.start_sha();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffNonTemporalCore) {
/// let _: &mut receipts_workspace_execution::CommitSha = value.final_sha();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffNonTemporalCore) {
/// value.task_id = String::new();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffNonTemporalCore) {
/// value.attempt_id = String::new();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffNonTemporalCore) {
/// value.ready_for_a4 = true;
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffNonTemporalCore) {
/// value.dispatch_a4();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffNonTemporalCore) {
/// value.history();
/// # }
/// ```
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::A3HandoffNonTemporalCore) {
/// value.reasoning();
/// # }
/// ```
///
/// Missing `task_id`:
/// ```compile_fail
/// use receipts_review_integration::A3HandoffNonTemporalCore;
/// let _ = A3HandoffNonTemporalCore::new("attempt".into(), "a".repeat(40), "b".repeat(40), String::new(), vec![], vec![], true, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
/// ```
///
/// Missing `attempt_id`:
/// ```compile_fail
/// use receipts_review_integration::A3HandoffNonTemporalCore;
/// let _ = A3HandoffNonTemporalCore::new("task".into(), "a".repeat(40), "b".repeat(40), String::new(), vec![], vec![], true, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
/// ```
///
/// Missing `start_sha`:
/// ```compile_fail
/// use receipts_review_integration::A3HandoffNonTemporalCore;
/// let _ = A3HandoffNonTemporalCore::new("task".into(), "attempt".into(), "b".repeat(40), String::new(), vec![], vec![], true, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
/// ```
///
/// Missing `final_sha`:
/// ```compile_fail
/// use receipts_review_integration::A3HandoffNonTemporalCore;
/// let _ = A3HandoffNonTemporalCore::new("task".into(), "attempt".into(), "a".repeat(40), String::new(), vec![], vec![], true, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
/// ```
///
/// Missing `branch`:
/// ```compile_fail
/// use receipts_review_integration::A3HandoffNonTemporalCore;
/// let _ = A3HandoffNonTemporalCore::new("task".into(), "attempt".into(), "a".repeat(40), "b".repeat(40), vec![], vec![], true, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
/// ```
///
/// Missing `files_changed`:
/// ```compile_fail
/// use receipts_review_integration::A3HandoffNonTemporalCore;
/// let _ = A3HandoffNonTemporalCore::new("task".into(), "attempt".into(), "a".repeat(40), "b".repeat(40), String::new(), vec![], true, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
/// ```
///
/// Missing `checks`:
/// ```compile_fail
/// use receipts_review_integration::A3HandoffNonTemporalCore;
/// let _ = A3HandoffNonTemporalCore::new("task".into(), "attempt".into(), "a".repeat(40), "b".repeat(40), String::new(), vec![], true, None, None, None, None, None, None, None, None, None, None, None, None, None, None);
/// ```
///
/// Missing `ready_for_a4`:
/// ```compile_fail
/// use receipts_review_integration::A3HandoffNonTemporalCore;
/// let _ = A3HandoffNonTemporalCore::new("task".into(), "attempt".into(), "a".repeat(40), "b".repeat(40), String::new(), vec![], vec![], None, None, None, None, None, None, None, None, None, None, None, None, None, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A3HandoffNonTemporalCore {
    task_id: String,
    attempt_id: String,
    start_sha: CommitSha,
    final_sha: CommitSha,
    branch: String,
    files_changed: Vec<String>,
    checks: Vec<A3HandoffCheck>,
    ready_for_a4: bool,
    implementer: Option<A3HandoffImplementer>,
    files_created: Option<Vec<String>>,
    files_deleted: Option<Vec<String>>,
    contracts_consumed: Option<Vec<A3HandoffContractConsumed>>,
    implementation_summary: Option<String>,
    not_run: Option<Vec<String>>,
    evidence_labels: Option<Vec<A3HandoffLabeledEvidence>>,
    known_limitations: Option<Vec<String>>,
    assumptions: Option<Vec<String>>,
    security_notes: Option<String>,
    subtask_requests: Option<Vec<String>>,
    open_questions: Option<Vec<String>>,
    evidence_refs: Option<Vec<WorkspaceCheckpointRef>>,
    blocker: Option<A3HandoffBlocker>,
}
impl A3HandoffNonTemporalCore {
    // Constructor arguments follow the frozen record; optional fields accept None.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        task_id: String,
        attempt_id: String,
        start_sha: String,
        final_sha: String,
        branch: String,
        files_changed: Vec<String>,
        checks: Vec<A3HandoffCheck>,
        ready_for_a4: bool,
        implementer: Option<A3HandoffImplementer>,
        files_created: Option<Vec<String>>,
        files_deleted: Option<Vec<String>>,
        contracts_consumed: Option<Vec<A3HandoffContractConsumed>>,
        implementation_summary: Option<String>,
        not_run: Option<Vec<String>>,
        evidence_labels: Option<Vec<A3HandoffLabeledEvidence>>,
        known_limitations: Option<Vec<String>>,
        assumptions: Option<Vec<String>>,
        security_notes: Option<String>,
        subtask_requests: Option<Vec<String>>,
        open_questions: Option<Vec<String>>,
        evidence_refs: Option<Vec<WorkspaceCheckpointRef>>,
        blocker: Option<A3HandoffBlocker>,
    ) -> Result<Self, A3HandoffConstructionError> {
        validate_id(
            &task_id,
            A3HandoffConstructionError::EmptyTaskId,
            A3HandoffConstructionError::TaskIdTooLong,
        )?;
        validate_id(
            &attempt_id,
            A3HandoffConstructionError::EmptyAttemptId,
            A3HandoffConstructionError::AttemptIdTooLong,
        )?;
        let start_sha = CommitSha::parse(&start_sha)
            .map_err(|_| A3HandoffConstructionError::MalformedStartSha)?;
        let final_sha = CommitSha::parse(&final_sha)
            .map_err(|_| A3HandoffConstructionError::MalformedFinalSha)?;
        if let Some(requests) = &subtask_requests {
            for request in requests {
                validate_id(
                    request,
                    A3HandoffConstructionError::EmptySubtaskRequest,
                    A3HandoffConstructionError::SubtaskRequestTooLong,
                )?;
            }
        }
        Ok(Self {
            task_id,
            attempt_id,
            start_sha,
            final_sha,
            branch,
            files_changed,
            checks,
            ready_for_a4,
            implementer,
            files_created,
            files_deleted,
            contracts_consumed,
            implementation_summary,
            not_run,
            evidence_labels,
            known_limitations,
            assumptions,
            security_notes,
            subtask_requests,
            open_questions,
            evidence_refs,
            blocker,
        })
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }
    pub fn start_sha(&self) -> &CommitSha {
        &self.start_sha
    }
    pub fn final_sha(&self) -> &CommitSha {
        &self.final_sha
    }
    pub fn branch(&self) -> &str {
        &self.branch
    }
    pub fn files_changed(&self) -> &[String] {
        &self.files_changed
    }
    pub fn checks(&self) -> &[A3HandoffCheck] {
        &self.checks
    }
    pub fn ready_for_a4(&self) -> bool {
        self.ready_for_a4
    }
    pub fn implementer(&self) -> Option<&A3HandoffImplementer> {
        self.implementer.as_ref()
    }
    pub fn files_created(&self) -> Option<&[String]> {
        self.files_created.as_deref()
    }
    pub fn files_deleted(&self) -> Option<&[String]> {
        self.files_deleted.as_deref()
    }
    pub fn contracts_consumed(&self) -> Option<&[A3HandoffContractConsumed]> {
        self.contracts_consumed.as_deref()
    }
    pub fn implementation_summary(&self) -> Option<&str> {
        self.implementation_summary.as_deref()
    }
    pub fn not_run(&self) -> Option<&[String]> {
        self.not_run.as_deref()
    }
    pub fn evidence_labels(&self) -> Option<&[A3HandoffLabeledEvidence]> {
        self.evidence_labels.as_deref()
    }
    pub fn known_limitations(&self) -> Option<&[String]> {
        self.known_limitations.as_deref()
    }
    pub fn assumptions(&self) -> Option<&[String]> {
        self.assumptions.as_deref()
    }
    pub fn security_notes(&self) -> Option<&str> {
        self.security_notes.as_deref()
    }
    pub fn subtask_requests(&self) -> Option<&[String]> {
        self.subtask_requests.as_deref()
    }
    pub fn open_questions(&self) -> Option<&[String]> {
        self.open_questions.as_deref()
    }
    pub fn evidence_refs(&self) -> Option<&[WorkspaceCheckpointRef]> {
        self.evidence_refs.as_deref()
    }
    pub fn blocker(&self) -> Option<&A3HandoffBlocker> {
        self.blocker.as_ref()
    }
}

fn validate_id(
    value: &str,
    empty: A3HandoffConstructionError,
    long: A3HandoffConstructionError,
) -> Result<(), A3HandoffConstructionError> {
    match value.chars().count() {
        0 => Err(empty),
        201.. => Err(long),
        _ => Ok(()),
    }
}
