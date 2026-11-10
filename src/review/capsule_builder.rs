//! RI-W2-1: ReviewRequest → ReviewCapsule, bound to the exact subject.
//!
//! Inputs are the Orchestration-supplied request, the repository and the
//! worker's mechanical check evidence. The builder reads the commit range
//! and diff itself through Workspace's hardened Git evidence API; nothing
//! about the subject is accepted from the caller as a claim. There is no
//! input for implementer conversation, summaries, notes, questions or
//! self-assessment, so none can reach the reviewer:
//!
//! ```compile_fail
//! # use receipts_review_integration::capsule_builder::ReviewCapsuleBuildInput;
//! # fn forbidden(input: ReviewCapsuleBuildInput<'_>) {
//! let _ = ReviewCapsuleBuildInput { conversation: "…", ..input };
//! # }
//! ```
//! ```compile_fail
//! # use receipts_review_integration::capsule_builder::ReviewCapsuleBuildInput;
//! # fn forbidden(input: ReviewCapsuleBuildInput<'_>) {
//! let _ = ReviewCapsuleBuildInput { implementation_summary: "…", ..input };
//! # }
//! ```
//! ```compile_fail
//! # fn forbidden(c: &receipts_review_integration::ReviewCapsuleNonTemporalCore) {
//! let _ = c.a3_conversation();
//! # }
//! ```

use crate::records::derived_id;
use crate::{
    A4ReviewFindingCategory, ReviewCapsuleCheck, ReviewCapsuleCriterionKind,
    ReviewCapsuleNonTemporalCore, ReviewCapsuleSeverityPolicy, ReviewRequest,
    is_assurance_profile_blocking_floor,
};
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointRef, WorkspaceCheckpointRefType,
    review_commit_range, review_diff,
};
use std::path::Path;

/// Everything the builder accepts.
#[derive(Debug, Clone, Copy)]
pub struct ReviewCapsuleBuildInput<'a> {
    pub request: &'a ReviewRequest,
    pub repository_root: &'a Path,
    /// Worker-executed checks to reproduce (argv, exit code, SHA, result).
    pub worker_checks: &'a [ReviewCapsuleCheck],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapsuleBuildRefusal {
    /// Workspace evidence refused the subject (missing commit, baseline not
    /// an ancestor, unsafe repository configuration, oversized range).
    SubjectEvidence(String),
    /// A worker check is not bound to the exact implementation SHA.
    CheckNotAtImplementationSha {
        index: usize,
    },
    /// A worker check claims a non-worker source.
    CheckNotWorkerExecution {
        index: usize,
    },
    /// A deterministic criterion has no argv to reproduce.
    DeterministicCriterionWithoutCommand {
        criterion_id: String,
    },
    Construction(String),
}

/// Categories that always block: the assurance floor plus missing negative
/// tests and unmet acceptance criteria.
pub fn blocking_categories() -> Vec<String> {
    A4ReviewFindingCategory::ALL
        .into_iter()
        .filter(|c| {
            is_assurance_profile_blocking_floor(*c)
                || matches!(
                    c,
                    A4ReviewFindingCategory::MissingRequiredNegativeTest
                        | A4ReviewFindingCategory::AcceptanceCriterionUnmet
                )
        })
        .map(|c| c.as_str().to_owned())
        .collect()
}

pub fn build_review_capsule(
    input: ReviewCapsuleBuildInput<'_>,
) -> Result<ReviewCapsuleNonTemporalCore, CapsuleBuildRefusal> {
    use CapsuleBuildRefusal as R;
    let request = input.request.core();
    let baseline = request.baseline_sha();
    let implementation = request.implementation_sha();
    // The range proves both commits exist and the baseline is an ancestor.
    review_commit_range(input.repository_root, baseline, implementation)
        .map_err(|e| R::SubjectEvidence(e.to_string()))?;
    let diff = review_diff(input.repository_root, baseline, implementation)
        .map_err(|e| R::SubjectEvidence(e.to_string()))?;
    for criterion in request.acceptance_criteria() {
        if criterion.kind() == ReviewCapsuleCriterionKind::Deterministic
            && criterion.check_command().is_none_or(<[String]>::is_empty)
        {
            return Err(R::DeterministicCriterionWithoutCommand {
                criterion_id: criterion.id().to_owned(),
            });
        }
    }
    for (index, check) in input.worker_checks.iter().enumerate() {
        if check.code_sha() != implementation {
            return Err(R::CheckNotAtImplementationSha { index });
        }
        if check.source() != WorkspaceCheckpointCheckSource::WorkerExecution {
            return Err(R::CheckNotWorkerExecution { index });
        }
    }
    let diff_ref = WorkspaceCheckpointRef::new(
        WorkspaceCheckpointRefType::ArtifactId,
        diff_target(baseline, implementation),
        Some(format!("sha256:{}", diff.patch_sha256)),
        None,
    )
    .map_err(|e| R::Construction(e.to_string()))?;
    let schema = WorkspaceCheckpointRef::new(
        WorkspaceCheckpointRefType::RepoPath,
        "schemas/A4Review.schema.json",
        None,
        None,
    )
    .map_err(|e| R::Construction(e.to_string()))?;
    let severity = ReviewCapsuleSeverityPolicy::new(
        blocking_categories(),
        Some(
            [
                A4ReviewFindingCategory::Style,
                A4ReviewFindingCategory::Naming,
                A4ReviewFindingCategory::MinorPerformance,
                A4ReviewFindingCategory::DocGap,
            ]
            .map(|c| c.as_str().to_owned())
            .to_vec(),
        ),
    )
    .map_err(|e| R::Construction(e.to_string()))?;
    let reproduction_required = request
        .assurance_profile()
        .requirements()
        .a4_reproduction_required;
    ReviewCapsuleNonTemporalCore::new_with_context_epoch(
        review_id(input.request),
        request.task_id().to_owned(),
        request.attempt_id().to_owned(),
        baseline.as_str().to_owned(),
        implementation.as_str().to_owned(),
        request.objective().to_owned(),
        request.acceptance_criteria().to_vec(),
        request.non_goals().map(<[String]>::to_vec),
        request.architecture_refs().map(<[_]>::to_vec),
        request.contract_refs().map(<[_]>::to_vec),
        diff_ref,
        request.allowed_write_paths().to_vec(),
        Some(input.worker_checks.to_vec()),
        None,
        request.review_scope(),
        severity,
        reproduction_required,
        Some(schema),
        request.context_epoch().clone(),
    )
    .map_err(|e| R::Construction(e.to_string()))
}

/// Stable identity for the semantic request and exact subject (excluding requested_at).
pub fn review_id(request: &ReviewRequest) -> String {
    let c = request.core();
    derived_id(
        "review",
        &[
            c.request_id(),
            c.task_id(),
            c.attempt_id(),
            c.implementation_sha().as_str(),
            &crate::claims::request_identity_json(request),
        ],
    )
}

pub(crate) fn diff_target(baseline: &CommitSha, implementation: &CommitSha) -> String {
    format!(
        "git-diff:{}..{}",
        baseline.as_str(),
        implementation.as_str()
    )
}
