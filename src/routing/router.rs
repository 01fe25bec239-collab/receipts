//! Router v1 (MR-W2-1): registry-only, deterministic, explained selection.
//!
//! Pipeline, every stage evaluated for every candidate so each rejection
//! keeps all its reasons:
//! 1. candidates are exactly the registry's (provider, model, runtime)
//!    associations (I-4); caller evidence must bind to them positionally;
//! 2. request identity constraints (distinct/avoid/pins);
//! 3. the existing bounded hard filters: lifecycle ROUTABLE, every required
//!    capability Confirmed on the runtime path, availability AVAILABLE or
//!    DEGRADED, recorded technical status CONNECTED and provider policy;
//! 4. the quality floor (I-9): the stricter of the request floor and the
//!    task class floor (QUALITY_COST_POLICY) must be met by registry tier
//!    evidence — capability `quality_tier.<TIER>` on the MODEL subject,
//!    established `true` by the latest-instant resolver with verified
//!    confidence (OFFICIAL/INDEPENDENT/LOCAL_EMPIRICAL). UNKNOWN, conflicted,
//!    missing, false or unverified tier evidence never passes;
//! 5. evidence-only ranking: AVAILABLE before DEGRADED, fresh before stale
//!    tier evidence, more Confirmed preferred capabilities, stronger tier
//!    confidence, then canonical (provider, model, runtime) order. There is
//!    no vendor constant and no cost term: cost components stay explicit null
//!    (UNKNOWN) until the W5 estimator exists, so cost never moves the floor.
//!
//! No eligible candidate never downgrades: the decision is
//! NO_ELIGIBLE_CANDIDATE (WAIT when an otherwise-eligible candidate is only
//! rate/session limited within the wait budget) or USER_INPUT_REQUIRED (ASK)
//! when that wait would exceed the budget.

use crate::policy_eligibility::{
    ModelRoutingDateTimeComparisonError, ModelRoutingDateTimeV1, elapsed_whole_seconds,
};
use crate::registry::{
    CapabilityEvidence, CapabilityId, CapabilityOutcome, CapabilitySubject, CapabilityValue,
    Compatibility, Freshness, Registry, ResolveError,
};
use crate::{
    AlternativeCandidate, AvailabilityStateKind, BoundedPreScoreCandidateEvidence,
    BoundedPreScoreEvidenceBindingError, CandidateEligibilityRejection,
    CandidateIdentityConstraintOutcome, CandidateIdentityConstraintRejection,
    CapabilityEvidenceNonTemporalCore, DecisionConfidence, EstimatedCostClass, EvidenceConfidence,
    RegistryCandidateIdentity, RegistryFreshness, RoutingDecisionCoreError, RoutingDecisionMode,
    RoutingDecisionNonTemporalCore, RoutingDecisionOutcome, RoutingQualityFloor, RoutingRequest,
    RoutingScoreComponents, RoutingTaskClass, SelectionReason,
    evaluate_bounded_pre_score_candidate_set,
};

/// Registry capability naming one quality tier for a model.
pub fn quality_tier_capability(tier: RoutingQualityFloor) -> CapabilityId {
    CapabilityId::try_new(format!("quality_tier.{}", tier.as_str())).expect("non-empty")
}

/// QUALITY_COST_POLICY task-class floor.
pub const fn task_class_floor(class: RoutingTaskClass) -> RoutingQualityFloor {
    use RoutingTaskClass::*;
    match class {
        FrontierImplementation | FrontierReview | FrontierArchitecture | SecurityCriticalCode => {
            RoutingQualityFloor::Frontier
        }
        BalancedReasoning => RoutingQualityFloor::Balanced,
        EconomyDocs | EconomySummary | EconomyStatus | PresentationOnly => {
            RoutingQualityFloor::Economy
        }
    }
}

const fn floor_rank(f: RoutingQualityFloor) -> u8 {
    match f {
        RoutingQualityFloor::Frontier => 2,
        RoutingQualityFloor::Balanced => 1,
        RoutingQualityFloor::Economy => 0,
    }
}

/// Tiers that meet `floor`, strongest first.
fn tiers_meeting(floor: RoutingQualityFloor) -> impl Iterator<Item = RoutingQualityFloor> {
    RoutingQualityFloor::ALL
        .into_iter()
        .filter(move |t| floor_rank(*t) >= floor_rank(floor))
}

/// Everything a routing call reads besides the request.
pub struct RouteContext<'a> {
    pub registry: &'a Registry,
    /// Exactly one entry per registry candidate, in canonical order.
    pub evidence: &'a [BoundedPreScoreCandidateEvidence<'a>],
    pub requested_execution_context: Option<&'a str>,
    /// Trusted decision time (also the decision's `occurred_at`).
    pub now: &'a ModelRoutingDateTimeV1,
    /// Tier evidence observed before this instant is stale.
    pub freshness_cutoff: &'a ModelRoutingDateTimeV1,
    pub mode: RoutingDecisionMode,
    /// Longest retry window the caller is willing to WAIT for; beyond it the
    /// decision ASKs (USER_INPUT_REQUIRED).
    pub wait_budget_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteError {
    Binding(BoundedPreScoreEvidenceBindingError),
    Resolve(ResolveError),
    Time(ModelRoutingDateTimeComparisonError),
    Decision(RoutingDecisionCoreError),
}

/// The immutable decision plus the WAIT window it names, if any.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutedDecision {
    pub decision: RoutingDecisionNonTemporalCore,
    /// Shortest retry window among otherwise-eligible limited candidates when
    /// the decision is a WAIT (NO_ELIGIBLE_CANDIDATE within the budget).
    pub wait_after_seconds: Option<u64>,
}

struct TierPass {
    fresh: bool,
    confidence_rank: u8,
    evidence: Vec<CapabilityEvidence>,
    age_seconds: i64,
}

struct Assessed {
    candidate: RegistryCandidateIdentity,
    reasons: Vec<String>,
    tier: Option<TierPass>,
    available: bool,
    preferred: usize,
    /// Rate/session limited with a known window and nothing else wrong.
    waitable_after: Option<u64>,
}

impl Assessed {
    /// Higher is better; ties fall back to canonical order (stable sort).
    fn key(&self) -> [(u64, &'static str); 4] {
        let tier = self
            .tier
            .as_ref()
            .expect("eligible candidates passed the floor");
        [
            (u64::from(self.available), "AVAILABILITY"),
            (u64::from(tier.fresh), "EVIDENCE_FRESHNESS"),
            (self.preferred as u64, "PREFERRED_CAPABILITIES"),
            (u64::from(tier.confidence_rank), "EVIDENCE_CONFIDENCE"),
        ]
    }
}

fn decisive(winner: &Assessed, other: &Assessed) -> &'static str {
    winner
        .key()
        .iter()
        .zip(other.key())
        .find(|(w, o)| w.0 != o.0)
        .map_or("CANONICAL_IDENTITY_TIEBREAK", |(w, _)| w.1)
}

fn confidence_rank(c: EvidenceConfidence) -> Option<u8> {
    match c {
        EvidenceConfidence::OfficialVerified => Some(2),
        EvidenceConfidence::IndependentVerified => Some(1),
        EvidenceConfidence::LocalEmpirical => Some(0),
        EvidenceConfidence::UserDeclared | EvidenceConfidence::Unverified => None,
    }
}

fn identity_reason(r: CandidateIdentityConstraintRejection) -> &'static str {
    use CandidateIdentityConstraintRejection::*;
    match r {
        DistinctProviderFromViolation => "CONSTRAINT:DISTINCT_PROVIDER_FROM",
        AvoidedProvider => "CONSTRAINT:AVOIDED_PROVIDER",
        PinnedProviderMismatch => "CONSTRAINT:PINNED_PROVIDER_MISMATCH",
        PinnedModelMismatch => "CONSTRAINT:PINNED_MODEL_MISMATCH",
    }
}

/// Stable reason codes; each names its own axis (availability, auth,
/// provider policy, safety/policy hold) so one is never reported as another.
pub fn eligibility_reason(r: &CandidateEligibilityRejection) -> String {
    use CandidateEligibilityRejection::*;
    match r {
        RegistryModelMissing => "REGISTRY:MODEL_MISSING".into(),
        RegistryRuntimeAssociationMissing => "REGISTRY:RUNTIME_ASSOCIATION_MISSING".into(),
        LifecycleNotNormallyRoutable(s) => format!("LIFECYCLE:{}", s.as_str()),
        RequiredCapabilityUnsupported(c) => {
            format!("REQUIRED_CAPABILITY_UNSUPPORTED:{}", c.as_str())
        }
        RequiredCapabilityUnknown(c) => format!("REQUIRED_CAPABILITY_UNKNOWN:{}", c.as_str()),
        AvailabilityIdentityMismatch => "AVAILABILITY:EVIDENCE_IDENTITY_MISMATCH".into(),
        AvailabilityScopeInsufficient => "AVAILABILITY:EVIDENCE_SCOPE_INSUFFICIENT".into(),
        AvailabilityIneligible(
            k @ (AvailabilityStateKind::SafetyCheckPending | AvailabilityStateKind::PolicyBlocked),
        ) => format!("SAFETY_OR_POLICY_HOLD:{}", k.as_str()),
        AvailabilityIneligible(k) => format!("AVAILABILITY:{}", k.as_str()),
        ProviderPolicyIdentityMismatch => "PROVIDER_POLICY:EVIDENCE_IDENTITY_MISMATCH".into(),
        RecordedTechnicalStatusIneligible(t) => format!("AUTH:{}", t.as_str()),
        ProviderPolicyStatusIneligible(p) => format!("PROVIDER_POLICY:{}", p.as_str()),
        ExecutionContextNotProvenAllowed => "PROVIDER_POLICY:EXECUTION_CONTEXT_NOT_ALLOWED".into(),
        ReverificationEvidenceMissing => "PROVIDER_POLICY:REVERIFICATION_EVIDENCE_MISSING".into(),
        ReverificationDeadlinePassed => "PROVIDER_POLICY:REVERIFICATION_DEADLINE_PASSED".into(),
    }
}

fn assess_floor(
    registry: &Registry,
    candidate: &RegistryCandidateIdentity,
    floor: RoutingQualityFloor,
    now: &ModelRoutingDateTimeV1,
    cutoff: &ModelRoutingDateTimeV1,
) -> Result<Result<TierPass, String>, RouteError> {
    let subject = CapabilitySubject::Model {
        provider_id: candidate.provider_id().clone(),
        model_id: candidate.model_id().clone(),
    };
    let mut seen = Vec::new();
    for tier in tiers_meeting(floor) {
        let resolution = registry
            .resolve_capability(&subject, &quality_tier_capability(tier), now, cutoff)
            .map_err(RouteError::Resolve)?;
        let outcome = match &resolution.outcome {
            CapabilityOutcome::Established {
                value: true,
                evidence,
            } => {
                let ranks: Option<Vec<u8>> = evidence
                    .iter()
                    .map(|e| confidence_rank(e.observation.confidence))
                    .collect();
                match ranks {
                    Some(ranks) => {
                        let observed = &evidence[0].observation.observed_at;
                        return Ok(Ok(TierPass {
                            fresh: resolution.freshness == Some(Freshness::Fresh),
                            confidence_rank: ranks.into_iter().min().unwrap_or(0),
                            evidence: evidence.iter().map(|e| (*e).clone()).collect(),
                            age_seconds: elapsed_whole_seconds(observed, now)
                                .map_err(RouteError::Time)?,
                        }));
                    }
                    None => "UNVERIFIED_CONFIDENCE",
                }
            }
            CapabilityOutcome::Established { value: false, .. } => "ASSERTED_FALSE",
            CapabilityOutcome::Unknown { .. } => "UNKNOWN",
            CapabilityOutcome::Conflicted { .. } => "CONFLICTED",
            CapabilityOutcome::NoUsableEvidence => "NO_EVIDENCE",
        };
        seen.push(format!("{}={outcome}", tier.as_str()));
    }
    Ok(Err(format!(
        "QUALITY_FLOOR_UNMET:{}({})",
        floor.as_str(),
        seen.join(",")
    )))
}

fn evidence_core(e: &CapabilityEvidence) -> CapabilityEvidenceNonTemporalCore {
    let core = CapabilityEvidenceNonTemporalCore::new(
        e.observation.observed_at.clone(),
        e.capability.as_str().to_owned(),
        e.observation.confidence,
        e.observation.source_ref.clone(),
        e.sample_size,
    );
    match e.value {
        Some(CapabilityValue::Boolean(b)) => core.with_value(serde_json::Value::Bool(b)),
        Some(CapabilityValue::Unknown) => core.with_value(serde_json::Value::Null),
        None => core,
    }
}

fn unknown_costs() -> RoutingScoreComponents {
    RoutingScoreComponents::try_new(Some(None), Some(None), Some(None), Some(None), Some(None))
        .expect("nulls are valid")
}

pub const SELECTED_HARD_FILTERS: [&str; 7] = [
    "REGISTRY_CANDIDATE",
    "IDENTITY_CONSTRAINTS",
    "LIFECYCLE_ROUTABLE",
    "REQUIRED_CAPABILITIES_CONFIRMED",
    "AVAILABILITY_ELIGIBLE",
    "AUTH_AND_PROVIDER_POLICY_ELIGIBLE",
    "QUALITY_FLOOR_MET",
];

/// Routes one request. Deterministic: equal inputs give equal decisions.
pub fn route(
    request: &RoutingRequest,
    ctx: &RouteContext<'_>,
    decision_id: String,
    fallback_from: Option<String>,
) -> Result<RoutedDecision, RouteError> {
    let core = request.core();
    let floor =
        if floor_rank(task_class_floor(core.task_class())) > floor_rank(core.quality_floor()) {
            task_class_floor(core.task_class())
        } else {
            core.quality_floor()
        };
    let assessments = evaluate_bounded_pre_score_candidate_set(
        core,
        ctx.registry,
        ctx.evidence,
        ctx.requested_execution_context,
    )
    .map_err(RouteError::Binding)?;

    let mut assessed = Vec::with_capacity(assessments.len());
    for (a, bundle) in assessments.iter().zip(ctx.evidence) {
        let candidate = a.candidate().clone();
        let mut reasons = Vec::new();
        if let CandidateIdentityConstraintOutcome::Rejected(r) = a.identity_constraint_outcome() {
            reasons.push(identity_reason(r).to_owned());
        }
        let rejections = a.bounded_eligibility_outcome().rejections();
        reasons.extend(rejections.iter().map(eligibility_reason));
        let tier = match assess_floor(
            ctx.registry,
            &candidate,
            floor,
            ctx.now,
            ctx.freshness_cutoff,
        )? {
            Ok(pass) => Some(pass),
            Err(reason) => {
                reasons.push(reason);
                None
            }
        };
        let availability = bundle.availability().core();
        // WAIT applies only when the sole obstacle is a rate/session limit.
        let waitable_after = match rejections {
            [
                CandidateEligibilityRejection::AvailabilityIneligible(
                    AvailabilityStateKind::RateLimited | AvailabilityStateKind::SessionExhausted,
                ),
            ] if reasons.len() == 1 => availability.retry_after_seconds().flatten(),
            _ => None,
        };
        let preferred = core
            .preferred_capabilities()
            .unwrap_or_default()
            .iter()
            .filter_map(|c| CapabilityId::try_new(c.clone()).ok())
            .filter(|c| {
                ctx.registry.compatibility(
                    candidate.provider_id(),
                    candidate.model_id(),
                    candidate.runtime_id(),
                    c,
                ) == Compatibility::Confirmed
            })
            .count();
        assessed.push(Assessed {
            candidate,
            reasons,
            tier,
            available: availability.state() == AvailabilityStateKind::Available,
            preferred,
            waitable_after,
        });
    }

    let mut eligible: Vec<&Assessed> = assessed.iter().filter(|a| a.reasons.is_empty()).collect();
    // Stable: equal keys keep canonical registry order.
    eligible.sort_by_key(|a| std::cmp::Reverse(a.key().map(|k| k.0)));
    let winner = eligible.first().copied();

    let alternatives = assessed
        .iter()
        .filter(|a| winner.is_none_or(|w| !std::ptr::eq(*a, w)))
        .map(|a| {
            let reason = if a.reasons.is_empty() {
                format!(
                    "RANKED_BELOW_SELECTED:{}",
                    decisive(winner.expect("an eligible alternative implies a winner"), a)
                )
            } else {
                a.reasons.join("; ")
            };
            AlternativeCandidate::try_new(
                a.candidate.provider_id().as_str().to_owned(),
                a.candidate.model_id().as_str().to_owned(),
                a.candidate.runtime_id().as_str().to_owned(),
                Some(None),
                reason,
            )
            .expect("registry identities are non-empty")
        })
        .collect::<Vec<_>>();

    let pinned = core
        .constraints()
        .is_some_and(|c| c.user_pinned_model().is_some() || c.user_pinned_provider().is_some());
    let wait = assessed.iter().filter_map(|a| a.waitable_after).min();
    let (outcome, factor, wait_after) = match (winner, wait) {
        (Some(w), _) => (
            RoutingDecisionOutcome::Selected,
            eligible
                .get(1)
                .map_or("ONLY_ELIGIBLE_CANDIDATE", |second| decisive(w, second)),
            None,
        ),
        (None, Some(s)) if s <= ctx.wait_budget_seconds => (
            RoutingDecisionOutcome::NoEligibleCandidate,
            "WAIT_RETRY_AFTER",
            Some(s),
        ),
        (None, Some(_)) => (
            RoutingDecisionOutcome::UserInputRequired,
            "ASK_WAIT_EXCEEDS_BUDGET",
            None,
        ),
        (None, None) => (
            RoutingDecisionOutcome::NoEligibleCandidate,
            "NO_ELIGIBLE_CANDIDATE",
            None,
        ),
    };

    let selected = winner.map(|w| {
        let tier = w.tier.as_ref().expect("winner passed the floor");
        let mut evidence: Vec<_> = tier.evidence.iter().map(evidence_core).collect();
        for required in core.required_capabilities() {
            let subject = CapabilitySubject::ModelRuntimePair {
                provider_id: w.candidate.provider_id().clone(),
                model_id: w.candidate.model_id().clone(),
                runtime_id: w.candidate.runtime_id().clone(),
            };
            evidence.extend(
                ctx.registry
                    .capability_evidence(&subject, required)
                    .unwrap_or_default()
                    .iter()
                    .filter(|e| {
                        e.observation.confidence == EvidenceConfidence::OfficialVerified
                            && e.value == Some(CapabilityValue::Boolean(true))
                    })
                    .map(evidence_core),
            );
        }
        let confidence = if !tier.fresh {
            DecisionConfidence::Low
        } else if w.available && tier.confidence_rank == 2 {
            DecisionConfidence::High
        } else {
            DecisionConfidence::Medium
        };
        (w, tier, evidence, confidence)
    });

    let mut filters = SELECTED_HARD_FILTERS.map(str::to_owned).to_vec();
    filters[6] = format!("QUALITY_FLOOR_MET:{}", floor.as_str());
    let decision = RoutingDecisionNonTemporalCore::try_new(
        ctx.now.clone(),
        decision_id,
        core.request_id().to_owned(),
        core.task_id().map(str::to_owned),
        outcome,
        selected
            .as_ref()
            .map(|s| s.0.candidate.provider_id().as_str().to_owned()),
        selected
            .as_ref()
            .map(|s| s.0.candidate.model_id().as_str().to_owned()),
        selected
            .as_ref()
            .map(|s| s.0.candidate.runtime_id().as_str().to_owned()),
        Some(SelectionReason::new(
            Some(floor),
            selected.as_ref().map(|_| filters),
            Some(unknown_costs()),
            Some(factor.to_owned()),
        )),
        selected.as_ref().map(|s| s.2.clone()),
        selected.as_ref().map(|s| s.3),
        selected.as_ref().map(|s| {
            if s.0.available {
                AvailabilityStateKind::Available
            } else {
                AvailabilityStateKind::Degraded
            }
            .as_str()
            .to_owned()
        }),
        Some(EstimatedCostClass::Unknown),
        Some(alternatives),
        RegistryFreshness::new(
            Some(None),
            Some(selected.as_ref().map(|s| s.1.age_seconds)),
            Some(None),
            selected.as_ref().is_some_and(|s| !s.1.fresh),
        ),
        ctx.mode,
        Some(false),
        Some(pinned && selected.is_some()),
        fallback_from,
    )
    .map_err(RouteError::Decision)?;
    Ok(RoutedDecision {
        decision,
        wait_after_seconds: wait_after,
    })
}
