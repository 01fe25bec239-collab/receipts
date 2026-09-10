//! Complete inert RepairCapsule physical contract, with no operational authority.

use super::repair_capsule_snapshots::validate_repair_sha;
use super::*;

/// Original task text and historical evidence are preserved exactly.
/// `parent_quality_floor` is validation-only evidence and is never stored.
///
/// Fields cannot bypass validation:
/// ```compile_fail
/// use receipts_orchestration::capsules::RepairCapsule;
/// fn invalidate(c: &mut RepairCapsule) { c.current_sha = String::new(); }
/// ```
/// Getters do not expose mutable collections:
/// ```compile_fail
/// use receipts_orchestration::capsules::RepairCapsule;
/// fn invalidate(c: &mut RepairCapsule) { c.blocking_findings().clear(); }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairCapsule {
    task_id: String,
    original_task_id: String,
    parent_attempt_id: Option<String>,
    attempt_number: RepairAttemptNumberV1,
    current_sha: String,
    original_objective: String,
    original_acceptance_criteria: Vec<Criterion>,
    a4_findings: Option<Vec<RepairCapsuleFindingV1>>,
    blocking_findings: Vec<RepairCapsuleFindingV1>,
    failed_checks: Option<Vec<RepairCapsuleFailedCheckV1>>,
    repair_scope: String,
    prior_attempt_summary: Option<String>,
    relevant_context_refs: Option<Vec<Ref>>,
    allowed_write_paths: Vec<String>,
    forbidden_write_paths: Option<Vec<String>>,
    required_quality_floor: QualityFloor,
    branch: String,
    worktree: String,
    context_epoch: RepairContextEpochV1,
}

impl RepairCapsule {
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        task_id: String,
        original_task_id: String,
        parent_attempt_id: Option<String>,
        attempt_number: RepairAttemptNumberV1,
        current_sha: String,
        original_objective: String,
        original_acceptance_criteria: Vec<Criterion>,
        a4_findings: Option<Vec<RepairCapsuleFindingV1>>,
        blocking_findings: Vec<RepairCapsuleFindingV1>,
        failed_checks: Option<Vec<RepairCapsuleFailedCheckV1>>,
        repair_scope: String,
        prior_attempt_summary: Option<String>,
        relevant_context_refs: Option<Vec<Ref>>,
        allowed_write_paths: Vec<String>,
        forbidden_write_paths: Option<Vec<String>>,
        required_quality_floor: QualityFloor,
        branch: String,
        worktree: String,
        context_epoch: RepairContextEpochV1,
        parent_quality_floor: QualityFloor,
    ) -> Result<Self, CapsuleError> {
        validate_identifier("task_id", &task_id)?;
        validate_identifier("original_task_id", &original_task_id)?;
        if let Some(id) = &parent_attempt_id {
            validate_identifier("parent_attempt_id", id)?;
        }
        validate_repair_sha("current_sha", &current_sha)?;
        if original_objective.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "original_objective",
            });
        }
        if original_acceptance_criteria.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "original_acceptance_criteria",
            });
        }
        if blocking_findings.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "blocking_findings",
            });
        }
        if repair_scope.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "repair_scope",
            });
        }
        if allowed_write_paths.is_empty() {
            return Err(CapsuleError::EmptyField {
                field: "allowed_write_paths",
            });
        }
        if !branch.starts_with("runtime-a3/") {
            return Err(CapsuleError::InvalidBranch);
        }
        // Explicit frozen order: FRONTIER > BALANCED > ECONOMY.
        let permitted = match parent_quality_floor {
            QualityFloor::Frontier => matches!(required_quality_floor, QualityFloor::Frontier),
            QualityFloor::Balanced => matches!(
                required_quality_floor,
                QualityFloor::Balanced | QualityFloor::Frontier
            ),
            QualityFloor::Economy => true,
        };
        if !permitted {
            return Err(CapsuleError::RepairQualityFloorWeakened);
        }
        Ok(Self {
            task_id,
            original_task_id,
            parent_attempt_id,
            attempt_number,
            current_sha,
            original_objective,
            original_acceptance_criteria,
            a4_findings,
            blocking_findings,
            failed_checks,
            repair_scope,
            prior_attempt_summary,
            relevant_context_refs,
            allowed_write_paths,
            forbidden_write_paths,
            required_quality_floor,
            branch,
            worktree,
            context_epoch,
        })
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn original_task_id(&self) -> &str {
        &self.original_task_id
    }
    pub fn parent_attempt_id(&self) -> Option<&str> {
        self.parent_attempt_id.as_deref()
    }
    pub fn attempt_number(&self) -> &RepairAttemptNumberV1 {
        &self.attempt_number
    }
    pub fn current_sha(&self) -> &str {
        &self.current_sha
    }
    pub fn original_objective(&self) -> &str {
        &self.original_objective
    }
    pub fn original_acceptance_criteria(&self) -> &[Criterion] {
        &self.original_acceptance_criteria
    }
    pub fn a4_findings(&self) -> Option<&[RepairCapsuleFindingV1]> {
        self.a4_findings.as_deref()
    }
    pub fn blocking_findings(&self) -> &[RepairCapsuleFindingV1] {
        &self.blocking_findings
    }
    pub fn failed_checks(&self) -> Option<&[RepairCapsuleFailedCheckV1]> {
        self.failed_checks.as_deref()
    }
    pub fn repair_scope(&self) -> &str {
        &self.repair_scope
    }
    pub fn prior_attempt_summary(&self) -> Option<&str> {
        self.prior_attempt_summary.as_deref()
    }
    pub fn relevant_context_refs(&self) -> Option<&[Ref]> {
        self.relevant_context_refs.as_deref()
    }
    pub fn allowed_write_paths(&self) -> &[String] {
        &self.allowed_write_paths
    }
    pub fn forbidden_write_paths(&self) -> Option<&[String]> {
        self.forbidden_write_paths.as_deref()
    }
    pub fn required_quality_floor(&self) -> QualityFloor {
        self.required_quality_floor
    }
    pub fn branch(&self) -> &str {
        &self.branch
    }
    pub fn worktree(&self) -> &str {
        &self.worktree
    }
    pub fn context_epoch(&self) -> &RepairContextEpochV1 {
        &self.context_epoch
    }
}
