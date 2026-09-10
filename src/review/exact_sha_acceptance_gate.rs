//! Pure acceptance-provenance foundation, not an IntegrationDecision or full gate.
//!
//! Authority: f49d621ee510705939394f7df4996223a73fdcb7,
//! INTEGRATION_GATE_ARCHITECTURE, REVIEW_VERIFICATION_PROVENANCE,
//! REVIEW_CAPSULE_SPEC and the A3Handoff/A4Review/ReviewCapsule machine schemas.
//! Caller-supplied current SHAs represent Git provenance; nothing is looked up.

use crate::{
    A3HandoffCheckResult, A3HandoffNonTemporalCore, A4ReviewNonTemporalCore,
    A4ReviewReproductionCheckResult, DeterministicRepairCycleControlCore, RepairCycleControlError,
    RepairCycleDisposition, ReviewCapsuleCheckResult, ReviewCapsuleNonTemporalCore,
};
use receipts_workspace_execution::CommitSha;
use std::fmt;

/// IN_PROCESS, REVIEW_LOCAL, NON_WIRE, NON_PERSISTED.
/// NOT A DEPENDENCY ACCEPTANCE CONTRACT: only a caller-supplied whole-tree link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyShaFreshnessLink {
    pub evidence_whole_tree_sha: CommitSha,
    pub current_authoritative_sha: CommitSha,
}

/// Borrowed IN_PROCESS, REVIEW_LOCAL, NON_WIRE, NON_PERSISTED inputs.
/// Histories are initial..current; their final pair is the current attempt.
/// Empty histories and absent records/identities explicitly represent missing evidence.
pub struct ExactShaAcceptanceGateInput<'a> {
    pub handoffs: &'a [A3HandoffNonTemporalCore],
    pub reviews: &'a [A4ReviewNonTemporalCore],
    pub completed_repair_attempts: usize,
    pub capsule: Option<&'a ReviewCapsuleNonTemporalCore>,
    pub accepted_candidate_sha: Option<&'a str>,
    pub current_baseline_sha: Option<&'a str>,
    pub dependency_freshness: &'a [DependencyShaFreshnessLink],
}

/// Physical record or check channel, for deterministic failure attribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptanceEvidenceRecord {
    A3Handoff,
    ReviewCapsule,
    A4Review,
}

/// Indices are zero-based. Repair history errors retain A3-006 attribution,
/// including exact-SHA and independence failures, without duplicating its rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExactShaAcceptanceGateError {
    MissingA3Handoff,
    MissingA4Review,
    MissingReviewCapsule,
    MissingCandidateSha,
    MissingCurrentBaselineSha,
    MalformedCandidateSha,
    MalformedCurrentBaselineSha,
    RepairHistory(RepairCycleControlError),
    RepairCycleNotClear(RepairCycleDisposition),
    TaskIdMismatch(AcceptanceEvidenceRecord),
    AttemptIdMismatch,
    ReviewIdMismatch,
    CandidateShaMismatch(AcceptanceEvidenceRecord),
    /// Whole-tree evidence is stale against the caller's current merge target.
    BaselineShaMismatch(AcceptanceEvidenceRecord),
    BlockingFindingsPresent,
    WriteScopeEvidenceMissing,
    UnauthorizedFileChangesPresent,
    RequiredChecksMissing(AcceptanceEvidenceRecord),
    EvidenceShaMismatch {
        record: AcceptanceEvidenceRecord,
        check_index: usize,
    },
    EvidenceResultMissing {
        record: AcceptanceEvidenceRecord,
        check_index: usize,
    },
    EvidenceResultNotPassing {
        record: AcceptanceEvidenceRecord,
        check_index: usize,
    },
    RequiredReproductionMissing,
    ReproductionNotPerformed,
    DependencyEvidenceStale {
        link_index: usize,
    },
}

impl fmt::Display for ExactShaAcceptanceGateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "exact-SHA acceptance foundation failed: {self:?}")
    }
}
impl std::error::Error for ExactShaAcceptanceGateError {}

/// Only evaluation constructs this narrow eligibility proof. It does not mean
/// integrated, correct, accepted by A2/A1, or that all M3 gates have completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactShaAcceptanceGatePass {
    candidate_sha: CommitSha,
}
impl ExactShaAcceptanceGatePass {
    pub fn candidate_sha(&self) -> &CommitSha {
        &self.candidate_sha
    }
}

pub struct ExactShaAcceptanceGate;

impl ExactShaAcceptanceGate {
    /// First checks record presence, then invokes A3-006 unchanged and requires
    /// NoRepairRequired (only independent PASS/PASS_WITH_NONBLOCKING_FINDINGS).
    /// Then parses caller SHAs with CommitSha and validates current links,
    /// findings, write scope, checks, reproduction, and dependency freshness.
    ///
    /// All physically supplied current checks are acceptance support (strategy A
    /// for capsule checks), requiring exact SHA and explicit PASS. Worker checks
    /// must be nonempty; capsule checks remain optional as in the machine schema.
    /// Required reproduction must be performed with nonempty checks. Even optional
    /// supplied reproduction checks are validated; nonempty checks also require
    /// performed=true. No result is inferred from exit_code, timed_out, or source.
    /// This does not establish criterion coverage or implement assurance profiles.
    pub fn evaluate(
        input: ExactShaAcceptanceGateInput<'_>,
    ) -> Result<ExactShaAcceptanceGatePass, ExactShaAcceptanceGateError> {
        use AcceptanceEvidenceRecord::{A3Handoff, A4Review, ReviewCapsule};
        use ExactShaAcceptanceGateError::*;

        let handoff = input.handoffs.last().ok_or(MissingA3Handoff)?;
        let review = input.reviews.last().ok_or(MissingA4Review)?;
        let capsule = input.capsule.ok_or(MissingReviewCapsule)?;
        let cycle = DeterministicRepairCycleControlCore::evaluate(
            input.handoffs,
            input.reviews,
            input.completed_repair_attempts,
        )
        .map_err(RepairHistory)?;
        if cycle.disposition() != RepairCycleDisposition::NoRepairRequired {
            return Err(RepairCycleNotClear(cycle.disposition()));
        }
        let candidate = CommitSha::parse(input.accepted_candidate_sha.ok_or(MissingCandidateSha)?)
            .map_err(|_| MalformedCandidateSha)?;
        let baseline = CommitSha::parse(
            input
                .current_baseline_sha
                .ok_or(MissingCurrentBaselineSha)?,
        )
        .map_err(|_| MalformedCurrentBaselineSha)?;

        for (record, task_id) in [(A3Handoff, handoff.task_id()), (A4Review, review.task_id())] {
            if task_id != capsule.task_id() {
                return Err(TaskIdMismatch(record));
            }
        }
        if handoff.attempt_id() != capsule.attempt_id() {
            return Err(AttemptIdMismatch);
        }
        if review.review_id() != capsule.review_id() {
            return Err(ReviewIdMismatch);
        }
        for (record, sha) in [
            (A3Handoff, handoff.final_sha().as_str()),
            (ReviewCapsule, capsule.implementation_sha().as_str()),
            (A4Review, review.reviewed_sha()),
        ] {
            if sha != candidate.as_str() {
                return Err(CandidateShaMismatch(record));
            }
        }
        for (record, sha) in [
            (A3Handoff, handoff.start_sha()),
            (ReviewCapsule, capsule.baseline_sha()),
        ] {
            if sha != &baseline {
                return Err(BaselineShaMismatch(record));
            }
        }
        if !review.blocking_findings().is_empty()
            || review
                .nonblocking_findings()
                .iter()
                .any(|finding| finding.blocking())
        {
            return Err(BlockingFindingsPresent);
        }
        if !review
            .unauthorized_file_changes()
            .ok_or(WriteScopeEvidenceMissing)?
            .is_empty()
        {
            return Err(UnauthorizedFileChangesPresent);
        }
        if handoff.checks().is_empty() {
            return Err(RequiredChecksMissing(A3Handoff));
        }
        for (index, check) in handoff.checks().iter().enumerate() {
            validate_check(
                A3Handoff,
                index,
                check.code_sha().as_str(),
                &candidate,
                check
                    .result()
                    .map(|result| result == A3HandoffCheckResult::Pass),
            )?;
        }
        for (index, check) in capsule.checks().unwrap_or_default().iter().enumerate() {
            validate_check(
                ReviewCapsule,
                index,
                check.code_sha().as_str(),
                &candidate,
                check
                    .result()
                    .map(|result| result == ReviewCapsuleCheckResult::Pass),
            )?;
        }
        if capsule.reproduction_required() && review.reproduction().is_none() {
            return Err(RequiredReproductionMissing);
        }
        if let Some(reproduction) = review.reproduction() {
            let checks = reproduction.checks().unwrap_or_default();
            if (capsule.reproduction_required() || !checks.is_empty())
                && reproduction.performed() != Some(true)
            {
                return Err(ReproductionNotPerformed);
            }
            if capsule.reproduction_required() && checks.is_empty() {
                return Err(RequiredChecksMissing(A4Review));
            }
            for (index, check) in checks.iter().enumerate() {
                validate_check(
                    A4Review,
                    index,
                    check.code_sha(),
                    &candidate,
                    check
                        .result()
                        .map(|result| result == A4ReviewReproductionCheckResult::Pass),
                )?;
            }
        }
        for (link_index, link) in input.dependency_freshness.iter().enumerate() {
            if link.evidence_whole_tree_sha != link.current_authoritative_sha {
                return Err(DependencyEvidenceStale { link_index });
            }
        }
        Ok(ExactShaAcceptanceGatePass {
            candidate_sha: candidate,
        })
    }
}

fn validate_check(
    record: AcceptanceEvidenceRecord,
    check_index: usize,
    code_sha: &str,
    candidate: &CommitSha,
    passing: Option<bool>,
) -> Result<(), ExactShaAcceptanceGateError> {
    use ExactShaAcceptanceGateError::*;
    if code_sha != candidate.as_str() {
        return Err(EvidenceShaMismatch {
            record,
            check_index,
        });
    }
    match passing {
        Some(true) => Ok(()),
        Some(false) => Err(EvidenceResultNotPassing {
            record,
            check_index,
        }),
        None => Err(EvidenceResultMissing {
            record,
            check_index,
        }),
    }
}
