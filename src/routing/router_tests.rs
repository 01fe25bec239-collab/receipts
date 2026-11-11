use std::collections::BTreeMap;

use crate::policy_eligibility::{PolicyStatus, ProviderPolicyEligibility};
use crate::registry::{CapabilityValue, ModelIntelligenceService};
use crate::router::{RouteContext, RouteError, RoutedDecision, route};
use crate::test_state::*;
use crate::wire::{decode_routing_decision, encode_routing_decision};
use crate::*;

fn request(
    class: RoutingTaskClass,
    floor: RoutingQualityFloor,
    preferred: &[&str],
    constraints: Option<RoutingRequestConstraints>,
) -> RoutingRequest {
    RoutingRequest::new(
        RoutingRequestNonTemporalCore::try_new(
            "request-1".into(),
            Some("task-1".into()),
            RoutingRequestRole::Implementer,
            class,
            floor,
            vec!["coding".into()],
            (!preferred.is_empty()).then(|| preferred.iter().map(|s| (*s).into()).collect()),
            None,
            None,
            constraints,
            None,
        )
        .unwrap(),
        None,
    )
}

fn frontier_request() -> RoutingRequest {
    request(
        RoutingTaskClass::FrontierImplementation,
        RoutingQualityFloor::Frontier,
        &[],
        None,
    )
}

type Key = (String, String, String);

pub(crate) struct World {
    pub service: ModelIntelligenceService,
    pub availability: BTreeMap<Key, AvailabilityState>,
    pub policy: BTreeMap<Key, ProviderPolicyEligibility>,
}

pub(crate) fn world(specs: &[Spec]) -> World {
    let service = build(specs);
    let mut w = World {
        service,
        availability: BTreeMap::new(),
        policy: BTreeMap::new(),
    };
    for s in specs {
        w.set_availability(s.p, s.m, s.r, AvailabilityStateKind::Available);
        w.policy.insert(
            key(s.p, s.m, s.r),
            policy(s.p, s.r, PolicyStatus::VerifiedAllowed),
        );
    }
    w
}

fn key(p: &str, m: &str, r: &str) -> Key {
    (p.into(), m.into(), r.into())
}

impl World {
    pub(crate) fn set_availability(&mut self, p: &str, m: &str, r: &str, s: AvailabilityStateKind) {
        self.availability
            .insert(key(p, m, r), availability(p, m, r, s, FRESH));
    }

    pub(crate) fn route_with(
        &self,
        request: &RoutingRequest,
        wait_budget_seconds: u64,
        override_availability: &BTreeMap<Key, AvailabilityState>,
        decision_id: &str,
        fallback_from: Option<String>,
    ) -> Result<RoutedDecision, RouteError> {
        let registry = self.service.registry();
        let candidates = enumerate_registry_candidates(registry);
        let evidence: Vec<_> = candidates
            .iter()
            .map(|c| {
                let k = key(
                    c.provider_id().as_str(),
                    c.model_id().as_str(),
                    c.runtime_id().as_str(),
                );
                BoundedPreScoreCandidateEvidence::new(
                    c.clone(),
                    override_availability
                        .get(&k)
                        .unwrap_or_else(|| &self.availability[&k]),
                    &self.policy[&k],
                    None,
                )
            })
            .collect();
        let (now, cutoff) = (ts(NOW), ts(CUTOFF));
        route(
            request,
            &RouteContext {
                registry,
                evidence: &evidence,
                requested_execution_context: Some(CONTEXT),
                now: &now,
                freshness_cutoff: &cutoff,
                mode: RoutingDecisionMode::AskOnUncertainty,
                wait_budget_seconds,
            },
            decision_id.into(),
            fallback_from,
        )
    }

    pub(crate) fn route(&self, request: &RoutingRequest) -> RoutingDecisionNonTemporalCore {
        self.route_with(request, 0, &BTreeMap::new(), "decision-1", None)
            .unwrap()
            .decision
    }
}

fn reasons(d: &RoutingDecisionNonTemporalCore, model: &str) -> String {
    d.alternative_candidates()
        .unwrap()
        .iter()
        .find(|a| a.model_id() == model)
        .unwrap_or_else(|| panic!("{model} missing from alternatives"))
        .rejection_reason()
        .to_owned()
}

fn factor(d: &RoutingDecisionNonTemporalCore) -> &str {
    d.selection_reason().unwrap().decisive_factor().unwrap()
}

#[test]
fn model_not_in_registry_never_selected() {
    let w = world(&[Spec::frontier("prov-a", "model-a", "rt-a")]);
    let pinned = RoutingRequestConstraints::try_new(
        None,
        None,
        Some("model-not-in-registry".into()),
        None,
        None,
    )
    .unwrap();
    let d = w.route(&request(
        RoutingTaskClass::FrontierImplementation,
        RoutingQualityFloor::Frontier,
        &[],
        Some(pinned),
    ));
    assert_eq!(d.outcome(), RoutingDecisionOutcome::NoEligibleCandidate);
    assert_eq!(d.selected_model(), None);
    assert_eq!(reasons(&d, "model-a"), "CONSTRAINT:PINNED_MODEL_MISMATCH");

    // Evidence for a triple the registry does not hold cannot even bind.
    let other = world(&[Spec::frontier("prov-a", "model-z", "rt-a")]);
    let foreign = &other.availability[&key("prov-a", "model-z", "rt-a")];
    let mut overrides = BTreeMap::new();
    overrides.insert(key("prov-a", "model-a", "rt-a"), foreign.clone());
    let registry = other.service.registry();
    let candidate = enumerate_registry_candidates(registry).remove(0);
    let ev = [BoundedPreScoreCandidateEvidence::new(
        candidate,
        foreign,
        &other.policy[&key("prov-a", "model-z", "rt-a")],
        None,
    )];
    let (now, cutoff) = (ts(NOW), ts(CUTOFF));
    let ctx = RouteContext {
        registry: w.service.registry(),
        evidence: &ev,
        requested_execution_context: Some(CONTEXT),
        now: &now,
        freshness_cutoff: &cutoff,
        mode: RoutingDecisionMode::AutoCurrent,
        wait_budget_seconds: 0,
    };
    assert!(matches!(
        route(&frontier_request(), &ctx, "d".into(), None),
        Err(RouteError::Binding(_))
    ));
}

#[test]
fn floor_never_violated_even_if_cheaper() {
    // The balanced model ranks better on every evidence feature (more
    // preferred capabilities) but is below the frontier floor.
    let mut balanced =
        Spec::frontier("prov-a", "model-balanced", "rt-a").tier(RoutingQualityFloor::Balanced);
    balanced.caps.push("long_context");
    let frontier = Spec::frontier("prov-b", "model-frontier", "rt-b");
    let w = world(&[balanced.clone(), frontier]);
    // The request floor is ECONOMY; the task class floor (FRONTIER) wins.
    let req = request(
        RoutingTaskClass::FrontierImplementation,
        RoutingQualityFloor::Economy,
        &["long_context"],
        None,
    );
    let d = w.route(&req);
    assert_eq!(d.selected_model(), Some("model-frontier"));
    assert_eq!(
        d.selection_reason().unwrap().quality_floor_applied(),
        Some(RoutingQualityFloor::Frontier)
    );
    assert_eq!(
        reasons(&d, "model-balanced"),
        "QUALITY_FLOOR_UNMET:FRONTIER(FRONTIER=NO_EVIDENCE)"
    );
    // Without the frontier candidate there is no downgrade.
    let only = world(&[balanced]);
    let d = only.route(&req);
    assert_eq!(d.outcome(), RoutingDecisionOutcome::NoEligibleCandidate);
    assert_eq!(d.selected_model(), None);
    // A balanced floor accepts frontier and balanced tier evidence.
    let d = only.route(&request(
        RoutingTaskClass::BalancedReasoning,
        RoutingQualityFloor::Balanced,
        &[],
        None,
    ));
    assert_eq!(d.selected_model(), Some("model-balanced"));
}

#[test]
fn unknown_capability_not_treated_as_pass() {
    use EvidenceConfidence::*;
    let mut unknown = Spec::frontier("prov-a", "model-unknown", "rt-a");
    unknown.tiers[0].1 = Some(CapabilityValue::Unknown);
    let mut omitted = Spec::frontier("prov-a", "model-omitted", "rt-a");
    omitted.tiers[0].1 = None;
    let mut conflicted = Spec::frontier("prov-a", "model-conflicted", "rt-a");
    conflicted.tiers.push((
        RoutingQualityFloor::Frontier,
        Some(CapabilityValue::Boolean(false)),
        OfficialVerified,
        FRESH,
    ));
    let mut marketing = Spec::frontier("prov-a", "model-marketing", "rt-a");
    marketing.tiers[0].2 = Unverified;
    let mut declared = Spec::frontier("prov-a", "model-declared", "rt-a");
    declared.tiers[0].2 = UserDeclared;
    let mut false_tier = Spec::frontier("prov-a", "model-false", "rt-a");
    false_tier.tiers[0].1 = Some(CapabilityValue::Boolean(false));
    // Required capability never confirmed on the runtime path.
    let mut no_caps = Spec::frontier("prov-a", "model-nocaps", "rt-a");
    no_caps.caps.clear();
    no_caps.routable = false;
    let w = world(&[
        unknown, omitted, conflicted, marketing, declared, false_tier, no_caps,
    ]);
    let d = w.route(&frontier_request());
    assert_eq!(d.outcome(), RoutingDecisionOutcome::NoEligibleCandidate);
    let floor = |outcome: &str| format!("QUALITY_FLOOR_UNMET:FRONTIER(FRONTIER={outcome})");
    assert_eq!(reasons(&d, "model-unknown"), floor("UNKNOWN"));
    assert_eq!(reasons(&d, "model-omitted"), floor("UNKNOWN"));
    assert_eq!(reasons(&d, "model-conflicted"), floor("CONFLICTED"));
    assert_eq!(
        reasons(&d, "model-marketing"),
        floor("UNVERIFIED_CONFIDENCE")
    );
    assert_eq!(
        reasons(&d, "model-declared"),
        floor("UNVERIFIED_CONFIDENCE")
    );
    assert_eq!(reasons(&d, "model-false"), floor("ASSERTED_FALSE"));
    let nocaps = reasons(&d, "model-nocaps");
    assert!(nocaps.contains("LIFECYCLE:DISCOVERED"), "{nocaps}");
    assert!(
        nocaps.contains("REQUIRED_CAPABILITY_UNKNOWN:coding"),
        "{nocaps}"
    );
}

#[test]
fn no_candidate_returns_failed_not_downgrade() {
    let w = world(&[
        Spec::frontier("prov-a", "model-econ", "rt-a").tier(RoutingQualityFloor::Economy),
        Spec::frontier("prov-b", "model-bal", "rt-b").tier(RoutingQualityFloor::Balanced),
    ]);
    let d = w.route(&frontier_request());
    assert_eq!(d.outcome(), RoutingDecisionOutcome::NoEligibleCandidate);
    assert_eq!(factor(&d), "NO_ELIGIBLE_CANDIDATE");
    assert_eq!(
        (
            d.selected_provider(),
            d.selected_model(),
            d.selected_runtime()
        ),
        (None, None, None)
    );
    assert_eq!(d.alternative_candidates().unwrap().len(), 2);
    assert_eq!(d.confidence(), None);

    let empty = world(&[]);
    let d = empty.route(&frontier_request());
    assert_eq!(d.outcome(), RoutingDecisionOutcome::NoEligibleCandidate);
    assert_eq!(d.alternative_candidates(), Some(&[][..]));
}

#[test]
fn decision_lists_every_rejected_candidate_with_reason() {
    let mut multi =
        Spec::frontier("prov-c", "model-multi", "rt-c").tier(RoutingQualityFloor::Economy);
    multi.routable = false;
    let mut w = world(&[
        Spec::frontier("prov-a", "model-win", "rt-a"),
        Spec::frontier("prov-b", "model-lower", "rt-b"),
        multi,
    ]);
    w.set_availability(
        "prov-b",
        "model-lower",
        "rt-b",
        AvailabilityStateKind::Degraded,
    );
    w.set_availability(
        "prov-c",
        "model-multi",
        "rt-c",
        AvailabilityStateKind::ProviderDown,
    );
    w.policy.insert(
        key("prov-c", "model-multi", "rt-c"),
        policy("prov-c", "rt-c", PolicyStatus::NeedsReview),
    );
    let d = w.route(&frontier_request());
    assert_eq!(d.selected_model(), Some("model-win"));
    assert_eq!(factor(&d), "AVAILABILITY");
    let alternatives = d.alternative_candidates().unwrap();
    assert_eq!(
        alternatives.len(),
        2,
        "every non-selected candidate is listed"
    );
    assert_eq!(
        reasons(&d, "model-lower"),
        "RANKED_BELOW_SELECTED:AVAILABILITY"
    );
    assert_eq!(
        reasons(&d, "model-multi"),
        "LIFECYCLE:DISCOVERED; AVAILABILITY:PROVIDER_DOWN; PROVIDER_POLICY:NEEDS_REVIEW; \
         QUALITY_FLOOR_UNMET:FRONTIER(FRONTIER=NO_EVIDENCE)"
    );
    assert!(alternatives.iter().all(|a| a.score() == Some(None)));
    let reason = d.selection_reason().unwrap();
    assert_eq!(reason.hard_filters_passed().unwrap().len(), 7);
    assert_eq!(
        reason.hard_filters_passed().unwrap()[6],
        "QUALITY_FLOOR_MET:FRONTIER"
    );
    let score = reason.score_components().unwrap();
    assert_eq!(
        score.expected_implementation_cost(),
        Some(None),
        "UNKNOWN stays null"
    );
    assert_eq!(score.expected_rejection_probability(), Some(None));
    assert_eq!(d.estimated_cost_class(), Some(EstimatedCostClass::Unknown));
    assert_eq!(d.availability_at_decision(), Some("AVAILABLE"));
    assert_eq!(d.confidence(), Some(DecisionConfidence::High));
    let evidence = d.capability_evidence().unwrap();
    assert!(
        evidence
            .iter()
            .any(|e| e.capability() == "quality_tier.FRONTIER")
    );
    assert!(
        evidence.iter().any(|e| e.capability() == "coding"
            && e.confidence() == EvidenceConfidence::OfficialVerified)
    );
    assert_eq!(
        d.registry_freshness().capability_age_seconds(),
        Some(Some(7 * 86_400 + 36_000))
    );
    assert!(!d.registry_freshness().stale());
    // The emitted decision is schema-valid and State-admissible.
    let json = encode_routing_decision(&d);
    assert_eq!(decode_routing_decision(&json).unwrap(), d);
}

#[test]
fn routing_is_deterministic() {
    let a = Spec::frontier("prov-a", "model-a", "rt-a");
    let b = Spec::frontier("prov-b", "model-b", "rt-b");
    let forward = world(&[a.clone(), b.clone()]).route(&frontier_request());
    let reverse = world(&[b, a]).route(&frontier_request());
    assert_eq!(forward, reverse);
    assert_eq!(
        encode_routing_decision(&forward),
        encode_routing_decision(&reverse)
    );
    // A full tie is broken by canonical identity, and says so.
    assert_eq!(forward.selected_model(), Some("model-a"));
    assert_eq!(factor(&forward), "CANONICAL_IDENTITY_TIEBREAK");
    assert_eq!(
        reasons(&forward, "model-b"),
        "RANKED_BELOW_SELECTED:CANONICAL_IDENTITY_TIEBREAK"
    );
    for _ in 0..5 {
        assert_eq!(
            world(&[
                Spec::frontier("prov-b", "model-b", "rt-b"),
                Spec::frontier("prov-a", "model-a", "rt-a")
            ])
            .route(&frontier_request()),
            forward
        );
    }
}

#[test]
fn policy_blocked_provider_excluded_with_policy_reason_not_availability() {
    let mut w = world(&[
        Spec::frontier("prov-a", "model-disallowed", "rt-a"),
        Spec::frontier("prov-b", "model-held", "rt-b"),
        Spec::frontier("prov-c", "model-ok", "rt-c"),
    ]);
    w.policy.insert(
        key("prov-a", "model-disallowed", "rt-a"),
        policy("prov-a", "rt-a", PolicyStatus::VerifiedDisallowed),
    );
    w.set_availability(
        "prov-b",
        "model-held",
        "rt-b",
        AvailabilityStateKind::PolicyBlocked,
    );
    let d = w.route(&frontier_request());
    assert_eq!(d.selected_model(), Some("model-ok"));
    let disallowed = reasons(&d, "model-disallowed");
    assert_eq!(disallowed, "PROVIDER_POLICY:VERIFIED_DISALLOWED");
    assert!(!disallowed.contains("AVAILABILITY"));
    let held = reasons(&d, "model-held");
    assert_eq!(held, "SAFETY_OR_POLICY_HOLD:POLICY_BLOCKED");
    assert!(!held.contains("AVAILABILITY:"));
    // A hold is never waitable.
    let only = {
        let mut w = world(&[Spec::frontier("prov-b", "model-held", "rt-b")]);
        w.set_availability(
            "prov-b",
            "model-held",
            "rt-b",
            AvailabilityStateKind::SafetyCheckPending,
        );
        w
    };
    let r = only
        .route_with(&frontier_request(), 3600, &BTreeMap::new(), "d", None)
        .unwrap();
    assert_eq!(
        r.decision.outcome(),
        RoutingDecisionOutcome::NoEligibleCandidate
    );
    assert_eq!(r.wait_after_seconds, None);
}

#[test]
fn ranking_uses_evidence_only_and_reports_the_decisive_factor() {
    let mut stale = Spec::frontier("prov-a", "model-stale", "rt-a");
    stale.tiers[0].3 = STALE;
    let fresh = Spec::frontier("prov-b", "model-fresh", "rt-b");
    let d = world(&[stale.clone(), fresh]).route(&frontier_request());
    assert_eq!(d.selected_model(), Some("model-fresh"));
    assert_eq!(factor(&d), "EVIDENCE_FRESHNESS");

    // Stale evidence alone still routes, visibly stale and LOW confidence.
    let d = world(&[stale]).route(&frontier_request());
    assert_eq!(d.selected_model(), Some("model-stale"));
    assert!(d.registry_freshness().stale());
    assert_eq!(d.confidence(), Some(DecisionConfidence::Low));
    assert_eq!(factor(&d), "ONLY_ELIGIBLE_CANDIDATE");

    let mut richer = Spec::frontier("prov-z", "model-richer", "rt-z");
    richer.caps.push("long_context");
    let d = world(&[Spec::frontier("prov-a", "model-plain", "rt-a"), richer]).route(&request(
        RoutingTaskClass::FrontierImplementation,
        RoutingQualityFloor::Frontier,
        &["long_context"],
        None,
    ));
    assert_eq!(d.selected_model(), Some("model-richer"));
    assert_eq!(factor(&d), "PREFERRED_CAPABILITIES");

    let mut independent = Spec::frontier("prov-a", "model-independent", "rt-a");
    independent.tiers[0].2 = EvidenceConfidence::IndependentVerified;
    let d = world(&[
        independent,
        Spec::frontier("prov-z", "model-official", "rt-z"),
    ])
    .route(&frontier_request());
    assert_eq!(d.selected_model(), Some("model-official"));
    assert_eq!(factor(&d), "EVIDENCE_CONFIDENCE");
}

#[test]
fn all_frontier_limited_wait_then_ask() {
    let w = world(&[Spec::frontier("prov-a", "model-a", "rt-a")]);
    let mut limited = BTreeMap::new();
    limited.insert(
        key("prov-a", "model-a", "rt-a"),
        AvailabilityState::new(
            AvailabilityStateNonTemporalCore::try_new(
                "prov-a".into(),
                Some("model-a".into()),
                Some("rt-a".into()),
                AvailabilityStateKind::RateLimited,
                Some(Some(30)),
                Some(AvailabilitySignalSource::RateLimitHeader),
                None,
            )
            .unwrap(),
            ts(NOW),
        ),
    );
    let wait = w
        .route_with(&frontier_request(), 60, &limited, "d-wait", None)
        .unwrap();
    assert_eq!(
        wait.decision.outcome(),
        RoutingDecisionOutcome::NoEligibleCandidate
    );
    assert_eq!(factor(&wait.decision), "WAIT_RETRY_AFTER");
    assert_eq!(wait.wait_after_seconds, Some(30));
    let ask = w
        .route_with(&frontier_request(), 10, &limited, "d-ask", None)
        .unwrap();
    assert_eq!(
        ask.decision.outcome(),
        RoutingDecisionOutcome::UserInputRequired
    );
    assert_eq!(factor(&ask.decision), "ASK_WAIT_EXCEEDS_BUDGET");
    assert_eq!(ask.wait_after_seconds, None);
}
