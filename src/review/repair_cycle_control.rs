//! Pure Review-local repair progression over immutable, caller-retained evidence.
//!
//! Authority: architecture f49d621ee510705939394f7df4996223a73fdcb7,
//! `A3_A4_REPAIR_LOOP.md` (bound/history), `RUNTIME_A4_LIFECYCLE.md`
//! (exact SHA/independence), and `schemas/A4Review.schema.json` (attestation).
//! No blocking-finding/verdict rule is added: the frozen lifecycle defines
//! blocking categories, but no repair-cycle disposition for contradictory
//! finding arrays. Closing blocking findings belongs to the acceptance gate
//! (`INTEGRATION_GATE_ARCHITECTURE.md`, A2 check 4). No readiness gate is added.
//! This module neither persists history nor executes any resulting action.

use crate::{A3HandoffNonTemporalCore, A4ReviewNonTemporalCore, A4ReviewVerdict};
use std::fmt;

pub const DEFAULT_MAX_REPAIR_ATTEMPTS: usize = 3;

/// Closed Review control meanings, not a wire contract or acceptance decision.
///
/// ```compile_fail
/// let _: receipts_review_integration::RepairCycleDisposition = Default::default();
/// ```
/// ```compile_fail
/// let _ = receipts_review_integration::RepairCycleDisposition::Unknown;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairCycleDisposition {
    /// NO_REPAIR_REQUIRED at this layer only; no A2/A1 acceptance is implied.
    NoRepairRequired,
    /// REPAIR_REQUIRED; the evaluator performs no repair or dispatch.
    RepairRequired,
    /// REPAIR_BOUND_EXHAUSTED; the evaluator performs no escalation.
    RepairBoundExhausted,
}

/// Immutable control output. Only evaluation can construct a next ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RepairCycleControlResult {
    disposition: RepairCycleDisposition,
    next_repair_ordinal: Option<usize>,
}

impl RepairCycleControlResult {
    pub const fn disposition(&self) -> RepairCycleDisposition {
        self.disposition
    }

    /// Some(1..=3) only for REPAIR_REQUIRED; otherwise None.
    pub const fn next_repair_ordinal(&self) -> Option<usize> {
        self.next_repair_ordinal
    }
}

/// Invalid evidence, never an ordinary repair disposition. Indices are zero-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairCycleControlError {
    EmptyAttemptHistory,
    HistoryLengthMismatch,
    RepairAttemptCountMismatch,
    RepairAttemptBoundExceeded,
    ShaMismatch { attempt_index: usize },
    IndependenceNotAttested { attempt_index: usize },
    PriorAttemptWasNotRejected { attempt_index: usize },
}

impl fmt::Display for RepairCycleControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAttemptHistory => f.write_str("attempt history is empty"),
            Self::HistoryLengthMismatch => f.write_str("handoff and review history lengths differ"),
            Self::RepairAttemptCountMismatch => {
                f.write_str("history pair count must equal completed repair attempts plus one")
            }
            Self::RepairAttemptBoundExceeded => f.write_str("automatic repair count exceeds three"),
            Self::ShaMismatch { attempt_index } => {
                write!(
                    f,
                    "attempt {attempt_index}: reviewed SHA differs from final SHA"
                )
            }
            Self::IndependenceNotAttested { attempt_index } => {
                write!(
                    f,
                    "attempt {attempt_index}: review independence is not attested"
                )
            }
            Self::PriorAttemptWasNotRejected { attempt_index } => {
                write!(
                    f,
                    "attempt {attempt_index}: repair continued after a passing review"
                )
            }
        }
    }
}

impl std::error::Error for RepairCycleControlError {}

/// Stateless evaluator; independence means only the frozen boolean attestation.
pub struct DeterministicRepairCycleControlCore;

impl DeterministicRepairCycleControlCore {
    /// Histories are ordered initial..current; the final pair is authoritative.
    /// `completed_repair_attempts`: initial=0, R1=1, R2=2, R3=3; >3 is invalid.
    /// Both lengths must equal this count + 1. Caller evidence is only borrowed;
    /// callers retain every rejected SHA and review in its original order.
    /// IDs, branches, readiness and global SHA uniqueness are not progression rules.
    /// Errors are checked in order: bound, lengths, empty, count, then each pair's
    /// exact SHA, independence and prior verdict, from initial to current.
    pub fn evaluate(
        handoffs: &[A3HandoffNonTemporalCore],
        reviews: &[A4ReviewNonTemporalCore],
        completed_repair_attempts: usize,
    ) -> Result<RepairCycleControlResult, RepairCycleControlError> {
        if completed_repair_attempts > DEFAULT_MAX_REPAIR_ATTEMPTS {
            return Err(RepairCycleControlError::RepairAttemptBoundExceeded);
        }
        if handoffs.len() != reviews.len() {
            return Err(RepairCycleControlError::HistoryLengthMismatch);
        }
        let Some(current_review) = reviews.last() else {
            return Err(RepairCycleControlError::EmptyAttemptHistory);
        };
        if handoffs.len() != completed_repair_attempts + 1 {
            return Err(RepairCycleControlError::RepairAttemptCountMismatch);
        }
        for (attempt_index, (handoff, review)) in handoffs.iter().zip(reviews).enumerate() {
            if handoff.final_sha().as_str() != review.reviewed_sha() {
                return Err(RepairCycleControlError::ShaMismatch { attempt_index });
            }
            if !review.independence_attested() {
                return Err(RepairCycleControlError::IndependenceNotAttested { attempt_index });
            }
            if attempt_index < completed_repair_attempts {
                match review.verdict() {
                    A4ReviewVerdict::Reject => {}
                    A4ReviewVerdict::Pass | A4ReviewVerdict::PassWithNonblockingFindings => {
                        return Err(RepairCycleControlError::PriorAttemptWasNotRejected {
                            attempt_index,
                        });
                    }
                }
            }
        }
        let (disposition, next_repair_ordinal) = match current_review.verdict() {
            A4ReviewVerdict::Pass | A4ReviewVerdict::PassWithNonblockingFindings => {
                (RepairCycleDisposition::NoRepairRequired, None)
            }
            A4ReviewVerdict::Reject => {
                if completed_repair_attempts < DEFAULT_MAX_REPAIR_ATTEMPTS {
                    (
                        RepairCycleDisposition::RepairRequired,
                        Some(completed_repair_attempts + 1),
                    )
                } else {
                    (RepairCycleDisposition::RepairBoundExhausted, None)
                }
            }
        };
        Ok(RepairCycleControlResult {
            disposition,
            next_repair_ordinal,
        })
    }
}
