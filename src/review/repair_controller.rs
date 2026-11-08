//! RI-W2-4: bounded repair controller emitting `RepairRequest`.
//!
//! Progression comes from the existing pure
//! [`DeterministicRepairCycleControlCore`] (bound 3, exact SHA and attested
//! independence per pair). Escalation ladder: repair 1 at the same tier,
//! repairs 2–3 at the frontier floor, then HUMAN_REQUIRED. A policy or
//! safety interruption is never turned into a repair (that would re-route
//! the same capsule elsewhere): it escalates. Every rejected SHA in the
//! history is carried forward in order; the A4Review records that reject
//! them are immutable in State and this module deletes nothing.
//!
//! The `RepairRequest` is Review's output contract; building the
//! Orchestration RepairCapsule from it is later composition (OR-W2-3).

use crate::records::{ProposalBuilder, derived_id};
use crate::{
    A3HandoffNonTemporalCore, A4ReviewFinding, A4ReviewFindingCategory, A4ReviewNonTemporalCore,
    A4ReviewVerdict, DEFAULT_MAX_REPAIR_ATTEMPTS, DeterministicRepairCycleControlCore,
    RepairCycleControlError, RepairCycleDisposition, ReviewRequest, SafetyInterruption,
    is_assurance_profile_blocking_floor, ports::ReviewRecordProposal,
};
use receipts_state::{EventType, SubjectKind};
use receipts_workspace_execution::CommitSha;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairTier {
    /// First repair: the same routing tier as the rejected attempt.
    SameTier,
    /// Later repairs: at least the frontier floor.
    FrontierFloor,
}

impl RepairTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SameTier => "SAME_TIER",
            Self::FrontierFloor => "FRONTIER_FLOOR",
        }
    }
}

/// One bounded repair order for Orchestration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairRequest {
    pub repair_request_id: String,
    pub task_id: String,
    /// The rejected attempt being repaired.
    pub parent_attempt_id: String,
    pub rejected_sha: CommitSha,
    /// Every rejected SHA so far, oldest first; never shortened.
    pub rejected_shas: Vec<CommitSha>,
    /// Reviews the repair must close (REPAIR_VERIFICATION prior reviews).
    pub prior_review_ids: Vec<String>,
    /// Blocking findings first, then policy-floor findings, then the rest.
    pub findings: Vec<A4ReviewFinding>,
    /// 1-based repair ordinal, at most `bound`.
    pub attempt_n: usize,
    pub bound: usize,
    pub tier: RepairTier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairEscalation {
    BoundExhausted,
    /// Safety/policy refusal: no repair dispatch, no provider-shopping.
    SafetyInterruption,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairDecision {
    NotRequired,
    Issue(RepairRequest),
    HumanRequired {
        reason: RepairEscalation,
        rejected_shas: Vec<CommitSha>,
    },
}

pub struct RepairInput<'a> {
    pub project_id: &'a str,
    pub request: &'a ReviewRequest,
    pub handoffs: &'a [A3HandoffNonTemporalCore],
    pub reviews: &'a [A4ReviewNonTemporalCore],
    pub completed_repair_attempts: usize,
    /// A safety interruption for the current attempt, if any.
    pub interruption: Option<&'a SafetyInterruption>,
}

#[derive(Debug, Clone)]
pub struct RepairOutcome {
    pub decision: RepairDecision,
    pub proposal: Result<ReviewRecordProposal, String>,
}

fn rank(f: &A4ReviewFinding) -> u8 {
    let floor = is_assurance_profile_blocking_floor(f.category())
        || f.category() == A4ReviewFindingCategory::MissingRequiredNegativeTest;
    match (f.blocking(), floor) {
        (true, _) => 0,
        (false, true) => 1,
        (false, false) => 2,
    }
}

/// Findings of `review`, blocking first (stable within each group).
pub fn ordered_findings(review: &A4ReviewNonTemporalCore) -> Vec<A4ReviewFinding> {
    let mut all: Vec<A4ReviewFinding> = review
        .blocking_findings()
        .iter()
        .chain(review.nonblocking_findings())
        .cloned()
        .collect();
    all.sort_by_key(rank);
    all
}

fn rejected_shas(reviews: &[A4ReviewNonTemporalCore]) -> Vec<CommitSha> {
    reviews
        .iter()
        .filter(|r| r.verdict() == A4ReviewVerdict::Reject)
        .filter_map(|r| CommitSha::parse(r.reviewed_sha()).ok())
        .collect()
}

pub struct RepairController;

impl RepairController {
    pub fn decide(input: &RepairInput<'_>) -> Result<RepairOutcome, RepairCycleControlError> {
        let rejected = rejected_shas(input.reviews);
        let decision = if input.interruption.is_some() {
            RepairDecision::HumanRequired {
                reason: RepairEscalation::SafetyInterruption,
                rejected_shas: rejected,
            }
        } else {
            let cycle = DeterministicRepairCycleControlCore::evaluate(
                input.handoffs,
                input.reviews,
                input.completed_repair_attempts,
            )?;
            match (cycle.disposition(), cycle.next_repair_ordinal()) {
                (RepairCycleDisposition::NoRepairRequired, _) => RepairDecision::NotRequired,
                (RepairCycleDisposition::RepairRequired, Some(n)) => {
                    // evaluate() guarantees a non-empty history here.
                    let review = &input.reviews[input.reviews.len() - 1];
                    let handoff = &input.handoffs[input.handoffs.len() - 1];
                    RepairDecision::Issue(RepairRequest {
                        repair_request_id: derived_id(
                            "repair",
                            &[review.task_id(), review.reviewed_sha(), &n.to_string()],
                        ),
                        task_id: review.task_id().to_owned(),
                        parent_attempt_id: handoff.attempt_id().to_owned(),
                        rejected_sha: handoff.final_sha().clone(),
                        rejected_shas: rejected,
                        prior_review_ids: input
                            .reviews
                            .iter()
                            .map(|r| r.review_id().to_owned())
                            .collect(),
                        findings: ordered_findings(review),
                        attempt_n: n,
                        bound: DEFAULT_MAX_REPAIR_ATTEMPTS,
                        tier: if n == 1 {
                            RepairTier::SameTier
                        } else {
                            RepairTier::FrontierFloor
                        },
                    })
                }
                (_, _) => RepairDecision::HumanRequired {
                    reason: RepairEscalation::BoundExhausted,
                    rejected_shas: rejected,
                },
            }
        };
        let proposal = records(input, &decision);
        Ok(RepairOutcome { decision, proposal })
    }
}

/// Canonical JSON for event payload content.
pub fn repair_request_json(r: &RepairRequest) -> String {
    use serde_json::{Value, json};
    let findings: Vec<Value> = r
        .findings
        .iter()
        .map(|f| {
            json!({
                "finding_id": f.finding_id(),
                "severity": f.severity().as_str(),
                "category": f.category().as_str(),
                "description": f.description(),
                "blocking": f.blocking(),
            })
        })
        .collect();
    json!({
        "repair_request_id": r.repair_request_id,
        "task_id": r.task_id,
        "parent_attempt_id": r.parent_attempt_id,
        "rejected_sha": r.rejected_sha.as_str(),
        "rejected_shas": r.rejected_shas.iter().map(CommitSha::as_str).collect::<Vec<_>>(),
        "prior_review_ids": r.prior_review_ids,
        "findings": findings,
        "attempt_n": r.attempt_n,
        "bound": r.bound,
        "tier": r.tier.as_str(),
    })
    .to_string()
}

fn records(input: &RepairInput<'_>, d: &RepairDecision) -> Result<ReviewRecordProposal, String> {
    let core = input.request.core();
    let mut b = ProposalBuilder::new(
        input.project_id,
        core.task_id(),
        core.context_epoch().decimal_digits(),
    )?;
    let task = (SubjectKind::Task, core.task_id());
    match d {
        RepairDecision::NotRequired => {}
        RepairDecision::Issue(r) => {
            let json = repair_request_json(r);
            let payload = b.content(
                &format!("review:repair:{}", r.repair_request_id),
                json.as_bytes(),
            );
            b.event(EventType::RepairIssued, task, payload)?;
        }
        RepairDecision::HumanRequired {
            reason,
            rejected_shas,
        } => {
            let body = serde_json::json!({
                "task_id": core.task_id(),
                "escalation": format!("{reason:?}"),
                "rejected_shas": rejected_shas.iter().map(CommitSha::as_str).collect::<Vec<_>>(),
            })
            .to_string();
            let payload = b.content(
                &derived_id("review:escalation", &[core.task_id(), &body]),
                body.as_bytes(),
            );
            if *reason == RepairEscalation::BoundExhausted {
                b.event(EventType::RepairLimitReached, task, payload.clone())?;
            }
            b.event(EventType::HumanRequired, task, payload)?;
        }
    }
    Ok(b.finish())
}
