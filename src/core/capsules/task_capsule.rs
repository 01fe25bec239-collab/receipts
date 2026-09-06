//! Complete immutable pre-dispatch task data. Integers use signed 64-bit storage.

use super::*;
use crate::graph::CapabilityName;

/// Complete task data, validated atomically by [`Self::try_new`].
/// All 31 fields are supplied explicitly; optional properties use `Option`.
/// Required capabilities may be empty without granting dispatch authority.
///
/// Required fields cannot be omitted from construction:
/// ```compile_fail
/// use receipts_orchestration::capsules::TaskCapsule;
/// let capsule = TaskCapsule::try_new();
/// ```
/// Fields cannot be mutated after validation:
/// ```compile_fail
/// use receipts_orchestration::capsules::TaskCapsule;
/// fn invalidate(capsule: &mut TaskCapsule) {
///     capsule.objective = String::new();
/// }
/// ```
/// Accessors expose no mutable collection bypass:
/// ```compile_fail
/// use receipts_orchestration::capsules::TaskCapsule;
/// fn invalidate(capsule: &mut TaskCapsule) {
///     capsule.verification_plan().clear();
/// }
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCapsule {
    task_id: String,
    workstream_id: String,
    parent_task_id: Option<String>,
    attempt_number: i64,
    task_type: TaskType,
    objective: String,
    acceptance_criteria: Vec<Criterion>,
    non_goals: Option<Vec<String>>,
    baseline_sha: String,
    start_sha: String,
    allowed_write_paths: Vec<String>,
    forbidden_write_paths: Vec<String>,
    relevant_context_refs: Option<Vec<Ref>>,
    architecture_refs: Option<Vec<Ref>>,
    contract_refs: Option<Vec<Ref>>,
    dependencies: Option<Vec<String>>,
    quality_floor: QualityFloor,
    required_capabilities: Vec<CapabilityName>,
    preferred_capabilities: Option<Vec<CapabilityName>>,
    verification_plan: Vec<VerificationPlanEntryV1>,
    review_policy: Option<ReviewPolicy>,
    assurance_profile: AssuranceProfileSelector,
    cost_policy: Option<CostPolicy>,
    time_budget_seconds: Option<i64>,
    turn_budget: Option<i64>,
    branch: String,
    worktree: String,
    remote_publish_policy: Option<RemotePublishPolicy>,
    stop_conditions: Vec<String>,
    handoff_schema: Option<Ref>,
    context_epoch: i64,
}

impl TaskCapsule {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        task_id: String,
        workstream_id: String,
        parent_task_id: Option<String>,
        attempt_number: i64,
        task_type: TaskType,
        objective: String,
        acceptance_criteria: Vec<Criterion>,
        non_goals: Option<Vec<String>>,
        baseline_sha: String,
        start_sha: String,
        allowed_write_paths: Vec<String>,
        forbidden_write_paths: Vec<String>,
        relevant_context_refs: Option<Vec<Ref>>,
        architecture_refs: Option<Vec<Ref>>,
        contract_refs: Option<Vec<Ref>>,
        dependencies: Option<Vec<String>>,
        quality_floor: QualityFloor,
        required_capabilities: Vec<CapabilityName>,
        preferred_capabilities: Option<Vec<CapabilityName>>,
        verification_plan: Vec<VerificationPlanEntryV1>,
        review_policy: Option<ReviewPolicy>,
        assurance_profile: AssuranceProfileSelector,
        cost_policy: Option<CostPolicy>,
        time_budget_seconds: Option<i64>,
        turn_budget: Option<i64>,
        branch: String,
        worktree: String,
        remote_publish_policy: Option<RemotePublishPolicy>,
        stop_conditions: Vec<String>,
        handoff_schema: Option<Ref>,
        context_epoch: i64,
    ) -> Result<Self, CapsuleError> {
        validate_identifier("task_id", &task_id)?;
        validate_identifier("workstream_id", &workstream_id)?;
        if let Some(id) = &parent_task_id {
            validate_identifier("parent_task_id", id)?;
        }
        if task_type == TaskType::Repair && parent_task_id.is_none() {
            return Err(CapsuleError::RepairParentMissing);
        }
        if attempt_number < 1 {
            return Err(CapsuleError::IntegerBelowMinimum {
                field: "attempt_number",
                minimum: 1,
                value: attempt_number,
            });
        }
        if context_epoch < 0 {
            return Err(CapsuleError::IntegerBelowMinimum {
                field: "context_epoch",
                minimum: 0,
                value: context_epoch,
            });
        }
        if objective.is_empty() {
            return Err(CapsuleError::EmptyField { field: "objective" });
        }
        if acceptance_criteria.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "acceptance_criteria",
            });
        }
        if allowed_write_paths.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "allowed_write_paths",
            });
        }
        if worktree.is_empty() {
            return Err(CapsuleError::EmptyField { field: "worktree" });
        }
        if stop_conditions.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "stop_conditions",
            });
        }
        if baseline_sha.len() != 40
            || !baseline_sha
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(CapsuleError::InvalidSha {
                field: "baseline_sha",
            });
        }
        if start_sha.len() != 40
            || !start_sha
                .bytes()
                .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(CapsuleError::InvalidSha { field: "start_sha" });
        }
        if let Some(ids) = &dependencies {
            for id in ids {
                validate_identifier("dependencies", id)?;
            }
        }
        if verification_plan.is_empty()
            && !matches!(task_type, TaskType::Docs | TaskType::Investigation)
        {
            return Err(CapsuleError::VerificationPlanMissing { task_type });
        }
        if let Some(value) = time_budget_seconds
            && value < 1
        {
            return Err(CapsuleError::IntegerBelowMinimum {
                field: "time_budget_seconds",
                minimum: 1,
                value,
            });
        }
        if let Some(value) = turn_budget
            && value < 1
        {
            return Err(CapsuleError::IntegerBelowMinimum {
                field: "turn_budget",
                minimum: 1,
                value,
            });
        }
        if !branch.starts_with("runtime-a3/") {
            return Err(CapsuleError::InvalidBranch);
        }
        Ok(Self {
            task_id,
            workstream_id,
            parent_task_id,
            attempt_number,
            task_type,
            objective,
            acceptance_criteria,
            non_goals,
            baseline_sha,
            start_sha,
            allowed_write_paths,
            forbidden_write_paths,
            relevant_context_refs,
            architecture_refs,
            contract_refs,
            dependencies,
            quality_floor,
            required_capabilities,
            preferred_capabilities,
            verification_plan,
            review_policy,
            assurance_profile,
            cost_policy,
            time_budget_seconds,
            turn_budget,
            branch,
            worktree,
            remote_publish_policy,
            stop_conditions,
            handoff_schema,
            context_epoch,
        })
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn workstream_id(&self) -> &str {
        &self.workstream_id
    }
    pub fn parent_task_id(&self) -> Option<&str> {
        self.parent_task_id.as_deref()
    }
    pub fn attempt_number(&self) -> i64 {
        self.attempt_number
    }
    pub fn task_type(&self) -> TaskType {
        self.task_type
    }
    pub fn objective(&self) -> &str {
        &self.objective
    }
    pub fn acceptance_criteria(&self) -> &[Criterion] {
        &self.acceptance_criteria
    }
    pub fn non_goals(&self) -> Option<&[String]> {
        self.non_goals.as_deref()
    }
    pub fn baseline_sha(&self) -> &str {
        &self.baseline_sha
    }
    pub fn start_sha(&self) -> &str {
        &self.start_sha
    }
    pub fn allowed_write_paths(&self) -> &[String] {
        &self.allowed_write_paths
    }
    pub fn forbidden_write_paths(&self) -> &[String] {
        &self.forbidden_write_paths
    }
    pub fn relevant_context_refs(&self) -> Option<&[Ref]> {
        self.relevant_context_refs.as_deref()
    }
    pub fn architecture_refs(&self) -> Option<&[Ref]> {
        self.architecture_refs.as_deref()
    }
    pub fn contract_refs(&self) -> Option<&[Ref]> {
        self.contract_refs.as_deref()
    }
    pub fn dependencies(&self) -> Option<&[String]> {
        self.dependencies.as_deref()
    }
    pub fn quality_floor(&self) -> QualityFloor {
        self.quality_floor
    }
    pub fn required_capabilities(&self) -> &[CapabilityName] {
        &self.required_capabilities
    }
    pub fn preferred_capabilities(&self) -> Option<&[CapabilityName]> {
        self.preferred_capabilities.as_deref()
    }
    pub fn verification_plan(&self) -> &[VerificationPlanEntryV1] {
        &self.verification_plan
    }
    pub fn review_policy(&self) -> Option<&ReviewPolicy> {
        self.review_policy.as_ref()
    }
    pub fn assurance_profile(&self) -> AssuranceProfileSelector {
        self.assurance_profile
    }
    pub fn cost_policy(&self) -> Option<&CostPolicy> {
        self.cost_policy.as_ref()
    }
    pub fn time_budget_seconds(&self) -> Option<i64> {
        self.time_budget_seconds
    }
    pub fn turn_budget(&self) -> Option<i64> {
        self.turn_budget
    }
    pub fn branch(&self) -> &str {
        &self.branch
    }
    pub fn worktree(&self) -> &str {
        &self.worktree
    }
    pub fn remote_publish_policy(&self) -> Option<RemotePublishPolicy> {
        self.remote_publish_policy
    }
    pub fn stop_conditions(&self) -> &[String] {
        &self.stop_conditions
    }
    pub fn handoff_schema(&self) -> Option<&Ref> {
        self.handoff_schema.as_ref()
    }
    pub fn context_epoch(&self) -> i64 {
        self.context_epoch
    }
}
