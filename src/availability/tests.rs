use std::collections::BTreeMap;

use receipts_runtime_adapters::FailureClass;
use receipts_state::{AvailabilityLifecycleStatusV1, EventProposalOriginV1};

use super::failover::{FailoverError, fail_over};
use super::*;
use crate::router::{RouteContext, RoutedDecision};
use crate::router_tests::World;
use crate::test_state::*;
use crate::*;

fn policy() -> BackoffPolicy {
    BackoffPolicy::try_new(10, 120, 3).unwrap()
}

fn observation(
    m: &str,
    failure: FailureClass,
    retry: Option<u64>,
    at: &str,
) -> AvailabilityObservation {
    let (p, m, r) = ids("prov-a", m, "rt-a");
    AvailabilityObservation {
        provider_id: p,
        model_id: m,
        runtime_id: r,
        failure,
        retry_after_seconds: retry,
        signal_source: AvailabilitySignalSource::StderrClassification,
        observed_at: ts(at),
        quota: None,
    }
}

fn request() -> RoutingRequest {
    RoutingRequest::new(
        RoutingRequestNonTemporalCore::try_new(
            "request-1".into(),
            Some("task-1".into()),
            RoutingRequestRole::Implementer,
            RoutingTaskClass::FrontierImplementation,
            RoutingQualityFloor::Frontier,
            vec!["coding".into()],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap(),
        None,
    )
}

fn three() -> World {
    crate::router_tests::world(&[
        Spec::frontier("prov-a", "model-a", "rt-a"),
        Spec::frontier("prov-a", "model-a2", "rt-a"),
        Spec::frontier("prov-b", "model-b", "rt-b"),
    ])
}

/// The manager's view of every candidate at `now`.
fn view(
    w: &World,
    m: &AvailabilityManager,
    now: &str,
) -> BTreeMap<(String, String, String), AvailabilityState> {
    w.availability
        .iter()
        .map(|(k, base)| (k.clone(), m.effective(base, &ts(now)).unwrap()))
        .collect()
}

fn reroute(
    w: &World,
    m: &AvailabilityManager,
    now: &str,
    previous: &RoutingDecisionNonTemporalCore,
    effect: &ObservationEffect,
    prior: u32,
    budget: u64,
) -> Result<RoutedDecision, FailoverError> {
    let overrides = view(w, m, now);
    let registry = w.service.registry();
    let evidence: Vec<_> = enumerate_registry_candidates(registry)
        .into_iter()
        .map(|c| {
            let k = (
                c.provider_id().as_str().to_owned(),
                c.model_id().as_str().to_owned(),
                c.runtime_id().as_str().to_owned(),
            );
            BoundedPreScoreCandidateEvidence::new(c, &overrides[&k], &w.policy[&k], None)
        })
        .collect();
    let (now, cutoff) = (ts(now), ts(CUTOFF));
    let ctx = RouteContext {
        registry,
        evidence: &evidence,
        requested_execution_context: Some(CONTEXT),
        now: &now,
        freshness_cutoff: &cutoff,
        mode: RoutingDecisionMode::AskOnUncertainty,
        wait_budget_seconds: budget,
    };
    fail_over(
        &request(),
        previous,
        effect,
        prior,
        5,
        &ctx,
        format!("decision-{}", prior + 2),
    )
}

const T1: &str = "2026-11-08T10:00:00Z";
const T1_PLUS_5: &str = "2026-11-08T10:00:05Z";
const T1_PLUS_40: &str = "2026-11-08T10:00:40Z";

#[test]
fn rate_limited_provider_filtered_and_failover_selects_other_frontier() {
    let w = three();
    let first = w.route(&request());
    assert_eq!(first.selected_model(), Some("model-a"));
    let mut m = AvailabilityManager::new(policy());
    let effect = m
        .assess(&observation(
            "model-a",
            FailureClass::RateLimited,
            Some(30),
            T1,
        ))
        .unwrap();
    let ObservationEffect::Limited {
        state,
        window_seconds,
        quota_scope,
        records,
        ..
    } = &effect
    else {
        panic!("{effect:?}");
    };
    assert_eq!(*state, AvailabilityStateKind::RateLimited);
    assert_eq!(*window_seconds, 30, "retry_after drives the window");
    assert_eq!(*quota_scope, QuotaScope::Unknown);
    assert_eq!(records.len(), 2, "exact limit plus provider DEGRADED");
    m.commit(&effect);

    let v = view(&w, &m, T1_PLUS_5);
    let state =
        |model: &str, p: &str, r: &str| v[&(p.into(), model.into(), r.into())].core().state();
    assert_eq!(
        state("model-a", "prov-a", "rt-a"),
        AvailabilityStateKind::RateLimited
    );
    assert_eq!(
        v[&("prov-a".into(), "model-a".into(), "rt-a".into())]
            .core()
            .retry_after_seconds(),
        Some(Some(25)),
        "remaining window"
    );
    // UNKNOWN quota scope never limits a sibling model; it only degrades.
    assert_eq!(
        state("model-a2", "prov-a", "rt-a"),
        AvailabilityStateKind::Degraded
    );
    assert_eq!(
        state("model-b", "prov-b", "rt-b"),
        AvailabilityStateKind::Available
    );

    let next = reroute(&w, &m, T1_PLUS_5, &first, &effect, 0, 60)
        .unwrap()
        .decision;
    assert_eq!(next.outcome(), RoutingDecisionOutcome::Selected);
    assert_eq!(next.selected_model(), Some("model-b"));
    assert_eq!(next.fallback_from(), Some("decision-1"));
    assert_eq!(
        next.task_id(),
        Some("task-1"),
        "task identity survives failover"
    );
    assert_eq!(next.request_id(), "request-1");
    let rejected = next
        .alternative_candidates()
        .unwrap()
        .iter()
        .find(|a| a.model_id() == "model-a")
        .unwrap()
        .rejection_reason()
        .to_owned();
    assert_eq!(rejected, "AVAILABILITY:RATE_LIMITED");
}

#[test]
fn all_frontier_limited_wait_then_ask() {
    let w = crate::router_tests::world(&[Spec::frontier("prov-a", "model-a", "rt-a")]);
    let first = w.route(&request());
    let mut m = AvailabilityManager::new(policy());
    let effect = m
        .assess(&observation("model-a", FailureClass::RateLimited, None, T1))
        .unwrap();
    m.commit(&effect);
    let wait = reroute(&w, &m, T1_PLUS_5, &first, &effect, 0, 60).unwrap();
    assert_eq!(
        wait.decision.outcome(),
        RoutingDecisionOutcome::NoEligibleCandidate
    );
    assert_eq!(wait.wait_after_seconds, Some(5), "backoff 10s, 5s elapsed");
    assert_eq!(
        wait.decision.selected_model(),
        None,
        "no downgrade while waiting"
    );
    // Beyond the wait budget the router asks.
    let ask = reroute(&w, &m, T1_PLUS_5, &first, &effect, 0, 1).unwrap();
    assert_eq!(
        ask.decision.outcome(),
        RoutingDecisionOutcome::UserInputRequired
    );
    // Repeated limits exhaust the bounded backoff, then ask.
    let mut at = 0u64;
    let mut last = effect;
    for _ in 0..3 {
        at += 5;
        last = m
            .assess(&observation(
                "model-a",
                FailureClass::RateLimited,
                None,
                &format!("2026-11-08T10:00:{at:02}Z"),
            ))
            .unwrap();
        m.commit(&last);
    }
    let ObservationEffect::Limited {
        attempt, exhausted, ..
    } = &last
    else {
        panic!()
    };
    assert_eq!((*attempt, *exhausted), (4, true));
    let ask = reroute(&w, &m, "2026-11-08T10:00:16Z", &first, &last, 0, 3600).unwrap();
    assert_eq!(
        ask.decision.outcome(),
        RoutingDecisionOutcome::UserInputRequired
    );
    assert_eq!(
        ask.decision.selection_reason().unwrap().decisive_factor(),
        Some("ASK_BACKOFF_ATTEMPTS_EXHAUSTED")
    );
    assert_eq!(ask.decision.fallback_from(), Some("decision-1"));
    // The failover chain itself is bounded too.
    let w2 = three();
    let first = w2.route(&request());
    let mut m2 = AvailabilityManager::new(policy());
    let e = m2
        .assess(&observation(
            "model-a",
            FailureClass::ProviderDown,
            None,
            T1,
        ))
        .unwrap();
    m2.commit(&e);
    let bounded = reroute(&w2, &m2, T1_PLUS_5, &first, &e, 5, 60).unwrap();
    assert_eq!(
        bounded.decision.outcome(),
        RoutingDecisionOutcome::UserInputRequired
    );
    assert_eq!(
        bounded
            .decision
            .selection_reason()
            .unwrap()
            .decisive_factor(),
        Some("ASK_FAILOVER_BOUND_REACHED")
    );
}

#[test]
fn safety_pending_does_not_mark_rate_limited() {
    let w = three();
    let first = w.route(&request());
    let mut m = AvailabilityManager::new(policy());
    for class in [
        FailureClass::SafetyCheckPending,
        FailureClass::PolicyBlocked,
    ] {
        let effect = m
            .assess(&observation("model-a", class, Some(30), T1))
            .unwrap();
        assert_eq!(effect, ObservationEffect::SafetyOrPolicyHold(class));
        m.commit(&effect);
        assert!(m.records().is_empty(), "availability unchanged");
        let v = view(&w, &m, T1_PLUS_5);
        assert!(
            v.values()
                .all(|a| a.core().state() == AvailabilityStateKind::Available)
        );
        // No provider-shopping: the task is BLOCKED, not re-routed.
        let blocked = reroute(&w, &m, T1_PLUS_5, &first, &effect, 0, 60)
            .unwrap()
            .decision;
        assert_eq!(blocked.outcome(), RoutingDecisionOutcome::Blocked);
        assert_eq!(blocked.selected_model(), None);
        assert_eq!(
            blocked.alternative_candidates(),
            None,
            "no other provider assessed"
        );
        assert_eq!(
            blocked.selection_reason().unwrap().decisive_factor(),
            Some(class.as_str())
        );
        assert_eq!(blocked.fallback_from(), Some("decision-1"));
    }
    for class in [
        FailureClass::Timeout,
        FailureClass::RuntimeCrash,
        FailureClass::InvalidOutput,
        FailureClass::AuthRequired,
        FailureClass::SandboxDenied,
        FailureClass::UserCancelled,
        FailureClass::Unknown,
    ] {
        let effect = m.assess(&observation("model-a", class, None, T1)).unwrap();
        assert_eq!(effect, ObservationEffect::NotAnAvailabilitySignal(class));
        assert_eq!(
            reroute(&w, &m, T1_PLUS_5, &first, &effect, 0, 60),
            Err(FailoverError::NotAnAvailabilitySignal)
        );
    }
}

#[test]
fn quota_scope_unknown_unless_verified() {
    let w = three();
    let quota = |visibility| {
        let core =
            QuotaStateRequiredCore::new(ids("prov-a", "x", "y").0, QuotaScope::Provider, ts(T1));
        QuotaState::try_new(core, None, None, None, None, None, Some(visibility)).unwrap()
    };
    for (visibility, sibling) in [
        (QuotaVisibility::Headers, AvailabilityStateKind::RateLimited),
        (
            QuotaVisibility::LocalApprox,
            AvailabilityStateKind::Degraded,
        ),
        (QuotaVisibility::Unknown, AvailabilityStateKind::Degraded),
    ] {
        let mut m = AvailabilityManager::new(policy());
        let mut obs = observation("model-a", FailureClass::RateLimited, Some(30), T1);
        obs.quota = Some(quota(visibility));
        let effect = m.assess(&obs).unwrap();
        m.commit(&effect);
        let v = view(&w, &m, T1_PLUS_5);
        assert_eq!(
            v[&("prov-a".into(), "model-a2".into(), "rt-a".into())]
                .core()
                .state(),
            sibling,
            "{visibility:?}"
        );
        assert_eq!(
            v[&("prov-b".into(), "model-b".into(), "rt-b".into())]
                .core()
                .state(),
            AvailabilityStateKind::Available
        );
    }
    let mut other = observation("model-a", FailureClass::RateLimited, None, T1);
    let core = QuotaStateRequiredCore::new(ids("prov-b", "x", "y").0, QuotaScope::Provider, ts(T1));
    other.quota = Some(QuotaState::try_new(core, None, None, None, None, None, None).unwrap());
    assert_eq!(
        AvailabilityManager::new(policy()).assess(&other),
        Err(AvailabilityError::QuotaProviderMismatch)
    );
}

#[test]
fn backoff_is_bounded_exponential_and_elapse_is_not_recovery() {
    let p = policy();
    assert_eq!(
        (1..=6).map(|n| p.delay_seconds(n)).collect::<Vec<_>>(),
        [10, 20, 40, 80, 120, 120]
    );
    assert_eq!(p.delay_seconds(200), 120, "no overflow past the cap");
    assert!(BackoffPolicy::try_new(0, 10, 1).is_none());
    assert!(BackoffPolicy::try_new(20, 10, 1).is_none());
    assert!(BackoffPolicy::try_new(1, 10, 0).is_none());

    let w = three();
    let mut m = AvailabilityManager::new(p);
    let e = m
        .assess(&observation(
            "model-a",
            FailureClass::SessionExhausted,
            None,
            T1,
        ))
        .unwrap();
    m.commit(&e);
    // Consecutive limit → attempt 2 → 20s window.
    let e2 = m
        .assess(&observation(
            "model-a",
            FailureClass::SessionExhausted,
            None,
            T1_PLUS_5,
        ))
        .unwrap();
    let ObservationEffect::Limited {
        attempt: 2,
        window_seconds: 20,
        ..
    } = e2
    else {
        panic!("{e2:?}")
    };
    m.commit(&e2);
    // A quiet period of two windows resets the attempt.
    let e3 = m
        .assess(&observation(
            "model-a",
            FailureClass::SessionExhausted,
            None,
            "2026-11-08T10:01:00Z",
        ))
        .unwrap();
    let ObservationEffect::Limited {
        attempt: 1,
        window_seconds: 10,
        ..
    } = e3
    else {
        panic!("{e3:?}")
    };
    // Older than the latest record for the candidate: refused.
    assert_eq!(
        m.assess(&observation(
            "model-a",
            FailureClass::RateLimited,
            None,
            "2026-11-08T09:59:00Z"
        )),
        Err(AvailabilityError::OutOfOrder)
    );
    // After the window: retryable but DEGRADED, never AVAILABLE by elapse.
    let key = ("prov-a".to_owned(), "model-a".to_owned(), "rt-a".to_owned());
    let after = view(&w, &m, T1_PLUS_40);
    assert_eq!(after[&key].core().state(), AvailabilityStateKind::Degraded);
    // Base evidence newer than the limit is honoured.
    let newer = availability(
        "prov-a",
        "model-a",
        "rt-a",
        AvailabilityStateKind::Available,
        T1_PLUS_40,
    );
    assert_eq!(
        m.effective(&newer, &ts(T1_PLUS_40)).unwrap().core().state(),
        AvailabilityStateKind::Available
    );
}

#[test]
fn availability_records_events_and_demotion_persist_and_reload() {
    let mut core = CoreHarness::new("availability");
    let scope = CoreHarness::scope();
    let mut m = AvailabilityManager::new(policy());
    let e = m
        .assess(&observation(
            "model-a",
            FailureClass::RateLimited,
            Some(30),
            T1,
        ))
        .unwrap();
    let out = m.persist_and_commit(&mut core, &scope, &e).unwrap();
    assert_eq!(
        out.events.len(),
        2,
        "RATE_LIMIT_OBSERVED + PROVIDER_DEGRADED"
    );
    assert!(
        out.events
            .iter()
            .all(|r| r.origin == EventProposalOriginV1::Routing)
    );
    let demotion = out.demotion.unwrap();
    assert_eq!(demotion.sequence, 1);
    assert_eq!(demotion.status, AvailabilityLifecycleStatusV1::Demoted);
    let e2 = m
        .assess(&observation(
            "model-a",
            FailureClass::ProviderDown,
            None,
            T1_PLUS_5,
        ))
        .unwrap();
    m.persist_and_commit(&mut core, &scope, &e2).unwrap();
    // Holds persist nothing.
    let hold = m
        .assess(&observation(
            "model-a",
            FailureClass::PolicyBlocked,
            None,
            T1_PLUS_5,
        ))
        .unwrap();
    let none = m.persist_and_commit(&mut core, &scope, &hold).unwrap();
    assert!(none.events.is_empty() && none.demotion.is_none());

    core.reopen();
    let reloaded = AvailabilityManager::reload(core.reader(), PROJECT, policy()).unwrap();
    assert_eq!(reloaded.records(), m.records(), "restart does not stampede");
    let lifecycle = core
        .reader()
        .availability_lifecycle(PROJECT, "prov-a", "rt-a", "model-a")
        .unwrap();
    assert_eq!(lifecycle.status, AvailabilityLifecycleStatusV1::Demoted);
    assert_eq!(lifecycle.latest_sequence, 2);
    // Each event payload is the exact persisted record.
    let event = core
        .reader()
        .find_event_receipt(&out.events[0].event_id)
        .unwrap()
        .unwrap();
    assert_eq!(event.origin, EventProposalOriginV1::Routing);
}
