//! RI-W2-4: bounded repair controller emitting `RepairRequest`.
//!
//! Progression comes from the existing pure
//! [`DeterministicRepairCycleControlCore`] (bound 3, exact SHA and attested
//! independence per pair). Escalation ladder: repair 1 at the same tier,
//! repairs 2–3 at the frontier floor, then ESCALATED_TO_A2 and
//! HUMAN_REQUIRED (the A1 escalation step is governance outside this
//! package). Finding-specific mandatory policy is applied BEFORE that
//! ordinary ladder and can only raise it (A3_A4_REPAIR_LOOP "Who repairs" and
//! "Who reviews the repair"): a security-boundary finding makes the frontier
//! floor mandatory from the first repair and escalates `distinct_provider` to
//! REQUIRED; a security, contract or architecture(invariant) finding bars
//! the reviewer that raised it (and its model) from grading the repair; a
//! contract or architecture finding prefers a different model. A review that
//! raised such a finding without a recorded reviewer identity cannot be
//! excluded, so the repair escalates instead of guessing. A policy or
//! safety interruption is never turned into a repair (that would re-route
//! the same capsule elsewhere): it escalates. Every rejected SHA in the
//! history is carried forward in order; the A4Review records that reject
//! them are immutable in State and this module deletes nothing.
//!
//! The `RepairRequest` is Review's output contract; building the
//! Orchestration RepairCapsule from it is later composition (OR-W2-3).

use crate::claims::{
    ReviewClaimBinding, ReviewClaimScope, ReviewOperation, interruption_identity_json,
    request_identity_json, request_sha256,
};
use crate::ports::ReviewRecordSink;
use crate::record::{Recorded, record_proposal};
use crate::records::{ProposalBuilder, derived_id};
use crate::wire::{encode_a3_handoff, encode_a4_review};
use crate::{
    A3HandoffNonTemporalCore, A4Review, A4ReviewFinding, A4ReviewFindingCategory,
    A4ReviewNonTemporalCore, A4ReviewVerdict, DEFAULT_MAX_REPAIR_ATTEMPTS,
    DeterministicRepairCycleControlCore, DistinctProviderPolicy, RepairCycleControlError,
    RepairCycleDisposition, ReviewRequest, SafetyInterruption, is_assurance_profile_blocking_floor,
    ports::ReviewRecordProposal,
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
    /// Effective tier: the ordinary ladder, raised by finding-specific policy.
    pub tier: RepairTier,
    /// Raise-only provider independence for the repair's executor/reviewer.
    pub distinct_provider: DistinctProviderPolicy,
    /// A different model is preferred (contract/architecture misreading).
    pub prefer_different_model: bool,
    /// Reviewers that raised a security, contract or invariant finding and
    /// so may not grade its repair.
    pub excluded_reviewers: ReviewerExclusions,
    /// Why the finding-specific policy applied, per finding.
    pub policy_triggers: Vec<RepairPolicyTrigger>,
}

/// Identities barred from reviewing a repair.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewerExclusions {
    pub session_refs: Vec<String>,
    /// `(provider, model)` pairs; a missing model excludes the provider's
    /// reviews of that runtime only through the session refs.
    pub provider_models: Vec<(String, Option<String>)>,
    /// The `review_id`s whose finders these are.
    pub review_ids: Vec<String>,
}

/// One finding that triggered a mandatory repair policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairPolicyTrigger {
    pub review_id: String,
    pub finding_id: String,
    pub category: A4ReviewFindingCategory,
    pub frontier_floor: bool,
    pub distinct_provider_required: bool,
    pub exclude_finder: bool,
    pub prefer_different_model: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairEscalation {
    BoundExhausted,
    /// Safety/policy refusal: no repair dispatch, no provider-shopping.
    SafetyInterruption,
    /// A review raised a security, contract or invariant finding without a
    /// recorded reviewer identity, so the finder cannot be excluded.
    FinderUnidentified,
}

// The issued request carries the policy detail; boxing would only churn callers.
#[allow(clippy::large_enum_variant)]
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
    /// Claim generation, supplied by the trusted composition (at least 1).
    pub generation: u32,
}

#[derive(Debug, Clone)]
pub struct RepairOutcome {
    pub decision: RepairDecision,
    pub proposal: Result<ReviewRecordProposal, String>,
}

impl RepairOutcome {
    /// Records the decision durably through the sink: claim, seal, publish.
    /// `NotRequired` holds nothing to record and claims nothing.
    pub fn record(&self, sink: &mut dyn ReviewRecordSink) -> Result<Recorded, String> {
        let label = match &self.decision {
            RepairDecision::NotRequired => return Ok(Recorded::NothingToRecord),
            RepairDecision::Issue(_) => "REPAIR_ISSUED",
            RepairDecision::HumanRequired { .. } => "REPAIR_ESCALATED",
        };
        let proposal = self.proposal.as_ref().map_err(Clone::clone)?;
        record_proposal(sink, proposal, label)
    }
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

fn trigger_for(review_id: &str, f: &A4ReviewFinding) -> Option<RepairPolicyTrigger> {
    use A4ReviewFindingCategory as C;
    let (frontier, distinct, exclude, prefer) = match f.category() {
        C::SecurityBoundaryViolation => (true, true, true, false),
        C::ContractViolation | C::ArchitectureViolation => (false, false, true, true),
        _ => return None,
    };
    Some(RepairPolicyTrigger {
        review_id: review_id.to_owned(),
        finding_id: f.finding_id().to_owned(),
        category: f.category(),
        frontier_floor: frontier,
        distinct_provider_required: distinct,
        exclude_finder: exclude,
        prefer_different_model: prefer,
    })
}

/// Finding-specific mandatory policy over every rejecting review so far.
fn finding_policy(
    reviews: &[A4ReviewNonTemporalCore],
) -> Result<(Vec<RepairPolicyTrigger>, ReviewerExclusions), RepairEscalation> {
    let mut triggers = Vec::new();
    let mut exclusions = ReviewerExclusions::default();
    for review in reviews
        .iter()
        .filter(|r| r.verdict() == A4ReviewVerdict::Reject)
    {
        let found: Vec<RepairPolicyTrigger> = review
            .blocking_findings()
            .iter()
            .chain(review.nonblocking_findings())
            .filter_map(|f| trigger_for(review.review_id(), f))
            .collect();
        if found.is_empty() {
            continue;
        }
        if found.iter().any(|t| t.exclude_finder) {
            let Some(reviewer) = review.reviewer() else {
                return Err(RepairEscalation::FinderUnidentified);
            };
            if reviewer.session_ref().is_none() && reviewer.provider_id().is_none() {
                return Err(RepairEscalation::FinderUnidentified);
            }
            if let Some(session) = reviewer.session_ref() {
                exclusions.session_refs.push(session.to_owned());
            }
            if let Some(provider) = reviewer.provider_id() {
                exclusions
                    .provider_models
                    .push((provider.to_owned(), reviewer.model_id().map(str::to_owned)));
            }
            exclusions.review_ids.push(review.review_id().to_owned());
        }
        triggers.extend(found);
    }
    exclusions.session_refs.sort();
    exclusions.session_refs.dedup();
    exclusions.provider_models.sort();
    exclusions.provider_models.dedup();
    Ok((triggers, exclusions))
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
                    let (triggers, excluded_reviewers) = match finding_policy(input.reviews) {
                        Ok(p) => p,
                        Err(reason) => {
                            let outcome = RepairDecision::HumanRequired {
                                reason,
                                rejected_shas: rejected,
                            };
                            let proposal = records(input, &outcome);
                            return Ok(RepairOutcome {
                                decision: outcome,
                                proposal,
                            });
                        }
                    };
                    let profile_distinct = input
                        .request
                        .core()
                        .assurance_profile()
                        .requirements()
                        .distinct_provider;
                    let distinct_provider = if triggers.iter().any(|t| t.distinct_provider_required)
                    {
                        DistinctProviderPolicy::Required
                    } else {
                        profile_distinct
                    };
                    let floor_by_finding = triggers.iter().any(|t| t.frontier_floor);
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
                        tier: if n == 1 && !floor_by_finding {
                            RepairTier::SameTier
                        } else {
                            RepairTier::FrontierFloor
                        },
                        distinct_provider,
                        prefer_different_model: triggers.iter().any(|t| t.prefer_different_model),
                        excluded_reviewers,
                        policy_triggers: triggers,
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
        "distinct_provider": r.distinct_provider.as_str(),
        "prefer_different_model": r.prefer_different_model,
        "excluded_reviewers": {
            "session_refs": r.excluded_reviewers.session_refs,
            "provider_models": r.excluded_reviewers.provider_models.iter()
                .map(|(p, m)| json!({"provider_id": p, "model_id": m}))
                .collect::<Vec<Value>>(),
            "review_ids": r.excluded_reviewers.review_ids,
        },
        "policy_triggers": r.policy_triggers.iter().map(|t| json!({
            "review_id": t.review_id,
            "finding_id": t.finding_id,
            "category": t.category.as_str(),
            "frontier_floor": t.frontier_floor,
            "distinct_provider_required": t.distinct_provider_required,
            "exclude_finder": t.exclude_finder,
            "prefer_different_model": t.prefer_different_model,
        })).collect::<Vec<Value>>(),
    })
    .to_string()
}

/// Durable identity of one repair decision: project, task, the last
/// reviewed SHA, the digest of every input that shapes the decision (the
/// request without `requested_at`, each retained review and handoff, the
/// completed repair count and any interruption, deterministically encoded)
/// and the trusted composition's generation.
pub fn repair_claim_binding(input: &RepairInput<'_>) -> Result<ReviewClaimBinding, String> {
    let core = input.request.core();
    let mut parts = vec![
        request_identity_json(input.request),
        input.completed_repair_attempts.to_string(),
        input
            .interruption
            .map_or_else(String::new, interruption_identity_json),
        input.reviews.len().to_string(),
    ];
    parts.extend(
        input
            .reviews
            .iter()
            .map(|r| encode_a4_review(&A4Review::new(r.clone(), None))),
    );
    parts.extend(input.handoffs.iter().map(encode_a3_handoff));
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let scope = ReviewClaimScope::new(
        input.project_id,
        core.task_id(),
        input.reviews.last().map(|r| r.reviewed_sha()),
        &request_sha256(ReviewOperation::Repair, &parts),
        input.generation,
    )?;
    Ok(ReviewClaimBinding::new(ReviewOperation::Repair, scope))
}

fn records(input: &RepairInput<'_>, d: &RepairDecision) -> Result<ReviewRecordProposal, String> {
    let core = input.request.core();
    let mut b = ProposalBuilder::new(
        &repair_claim_binding(input)?,
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
                // The ladder's A2 step (specification re-examined). The A1
                // escalation step is governance outside this package.
                b.event(EventType::EscalatedToA2, task, payload.clone())?;
            }
            b.event(EventType::HumanRequired, task, payload)?;
        }
    }
    Ok(b.finish())
}
