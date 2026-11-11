use std::collections::BTreeMap;

use receipts_runtime_adapters::FailureClass;
use receipts_state::{AvailabilityLifecycleStatusV1, DecisionRecordKind, EventProposalOriginV1};

use super::failover::{FailoverError, fail_over};
use super::*;
use crate::router::{RouteContext, RoutedDecision};
use crate::router_tests::World;
use crate::test_state::*;
use crate::*;

fn policy() -> BackoffPolicy {
    BackoffPolicy::try_new(10, 120, 3).unwrap()
}

/// Jitter pinned to the top of its range: windows equal the exponential
/// ceilings (`10, 20, 40, …`), keeping the expectations below exact.
fn manager() -> AvailabilityManager {
    AvailabilityManager::new(policy()).with_jitter(std::sync::Arc::new(FixedJitter(u64::MAX)))
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
    let mut m = manager();
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
    let mut m = manager();
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
    let mut m2 = manager();
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
    let mut m = manager();
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
        let mut m = manager();
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
        manager().assess(&other),
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
    let mut m = AvailabilityManager::new(p).with_jitter(std::sync::Arc::new(FixedJitter(u64::MAX)));
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
    let mut m = manager();
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

// ---- repair: refusal axes, huge windows, quota binding (F2, F3, F4) -------------

fn base(kind: AvailabilityStateKind, at: &str) -> AvailabilityState {
    availability("prov-a", "model-a", "rt-a", kind, at)
}

fn limited_manager(retry: Option<u64>) -> AvailabilityManager {
    let mut m = manager();
    let effect = m
        .assess(&observation(
            "model-a",
            FailureClass::RateLimited,
            retry,
            T1,
        ))
        .unwrap();
    m.commit(&effect);
    m
}

#[test]
fn active_limit_never_overwrites_a_newer_refusal_axis() {
    let m = limited_manager(Some(30));
    let reason = |w: &World, wait: u64| {
        let overrides = view(w, &m, T1_PLUS_5);
        let routed = w
            .route_with(&request(), wait, &overrides, "decision-hold", None)
            .unwrap();
        let alternative = routed
            .decision
            .alternative_candidates()
            .unwrap()
            .iter()
            .find(|a| a.model_id() == "model-a")
            .unwrap()
            .rejection_reason()
            .to_owned();
        (routed.wait_after_seconds, alternative)
    };
    for (kind, expected) in [
        (
            AvailabilityStateKind::SafetyCheckPending,
            "SAFETY_OR_POLICY_HOLD:SAFETY_CHECK_PENDING",
        ),
        (
            AvailabilityStateKind::PolicyBlocked,
            "SAFETY_OR_POLICY_HOLD:POLICY_BLOCKED",
        ),
        (
            AvailabilityStateKind::AuthRequired,
            "AVAILABILITY:AUTH_REQUIRED",
        ),
    ] {
        // The hold is newer than the committed limit and the limit is active.
        let held = base(kind, "2026-11-08T10:00:02Z");
        assert_eq!(
            m.effective(&held, &ts(T1_PLUS_5)).unwrap(),
            held,
            "{kind:?} keeps its axis, reason and observation"
        );
        // Routing on it keeps the refusal: never a WAIT, never RATE_LIMITED.
        let mut w = crate::router_tests::world(&[Spec::frontier("prov-a", "model-a", "rt-a")]);
        w.availability
            .insert(("prov-a".into(), "model-a".into(), "rt-a".into()), held);
        assert_eq!(reason(&w, 3600), (None, expected.to_owned()), "{kind:?}");
    }
    // Control: the same limit over an AVAILABLE base is a transient WAIT.
    let w = crate::router_tests::world(&[Spec::frontier("prov-a", "model-a", "rt-a")]);
    assert_eq!(
        reason(&w, 3600),
        (Some(25), "AVAILABILITY:RATE_LIMITED".to_owned())
    );
}

#[test]
fn huge_retry_window_stays_active_and_never_elapses() {
    for retry in [u64::MAX, u64::MAX / 2, i64::MAX as u64, i64::MAX as u64 + 1] {
        let m = limited_manager(Some(retry));
        for now in [
            T1,
            T1_PLUS_5,
            "2036-11-08T10:00:00Z",
            "9000-01-01T00:00:00Z",
        ] {
            let e = m
                .effective(&base(AvailabilityStateKind::Available, T1), &ts(now))
                .unwrap();
            assert_eq!(
                e.core().state(),
                AvailabilityStateKind::RateLimited,
                "retry_after={retry} at {now}"
            );
            let left = e.core().retry_after_seconds().flatten().unwrap();
            assert!(left > 0 && left <= retry, "{retry} at {now}: left={left}");
        }
        // Routing sees an active limit far beyond any wait budget: ASK.
        let w = crate::router_tests::world(&[Spec::frontier("prov-a", "model-a", "rt-a")]);
        let overrides = view(&w, &m, T1_PLUS_5);
        let routed = w
            .route_with(&request(), 3600, &overrides, "decision-huge", None)
            .unwrap();
        assert_eq!(
            routed.decision.outcome(),
            RoutingDecisionOutcome::UserInputRequired,
            "retry_after={retry}"
        );
        assert_eq!(routed.wait_after_seconds, None);
    }
}

#[test]
fn quota_evidence_must_bind_the_observed_model() {
    let quota = |model: Option<&str>, scope: QuotaScope| {
        let core = QuotaStateRequiredCore::new(ids("prov-a", "x", "y").0, scope, ts(T1));
        QuotaState::try_new(
            core,
            model.map(str::to_owned),
            None,
            None,
            None,
            None,
            Some(QuotaVisibility::Headers),
        )
        .unwrap()
    };
    let scope_of = |q: QuotaState| {
        let mut obs = observation("model-a", FailureClass::RateLimited, Some(30), T1);
        obs.quota = Some(q);
        manager().assess(&obs)
    };
    // Another model's proof is refused, whatever scope it claims.
    for scope in [QuotaScope::Model, QuotaScope::Provider, QuotaScope::Unknown] {
        assert_eq!(
            scope_of(quota(Some("different-model"), scope)).unwrap_err(),
            AvailabilityError::QuotaModelMismatch,
            "{scope:?}"
        );
    }
    // MODEL scope without a model binding is missing proof: stays UNKNOWN.
    let ObservationEffect::Limited { quota_scope, .. } =
        scope_of(quota(None, QuotaScope::Model)).unwrap()
    else {
        panic!()
    };
    assert_eq!(quota_scope, QuotaScope::Unknown);
    // The matching model proves MODEL scope; provider scope needs no model.
    let ObservationEffect::Limited { quota_scope, .. } =
        scope_of(quota(Some("model-a"), QuotaScope::Model)).unwrap()
    else {
        panic!()
    };
    assert_eq!(quota_scope, QuotaScope::Model);
    let ObservationEffect::Limited { quota_scope, .. } =
        scope_of(quota(None, QuotaScope::Provider)).unwrap()
    else {
        panic!()
    };
    assert_eq!(quota_scope, QuotaScope::Provider);
}

// ---- repair: atomic publication of availability effects (F1) ---------------------

fn limit_effect(
    m: &AvailabilityManager,
    model: &str,
    class: FailureClass,
    at: &str,
) -> ObservationEffect {
    m.assess(&observation(model, class, Some(30), at)).unwrap()
}

#[test]
fn availability_publication_is_atomic_exactly_retryable_and_conflict_safe() {
    use crate::persistence::RoutingPersistError;
    let mut core = CoreHarness::new("availability-atomic");
    let scope = CoreHarness::scope();
    let mut m = manager();
    let effect = limit_effect(&m, "model-a", FailureClass::RateLimited, T1);
    let demotion_sequence = |core: &CoreHarness| {
        core.reader()
            .availability_lifecycle(PROJECT, "prov-a", "rt-a", "model-a")
            .unwrap()
            .latest_sequence
    };

    // A failed publication leaves no record, event or demotion, live or after
    // a restart, and the manager has not advanced.
    core.arm(Fault::Rollback);
    assert!(matches!(
        m.persist_and_commit(&mut core, &scope, &effect),
        Err(RoutingPersistError::Port(_))
    ));
    assert!(m.records().is_empty());
    assert_eq!(core.records(DecisionRecordKind::AvailabilityObservation), 0);
    assert_eq!((core.events(&[]), demotion_sequence(&core)), (0, 0));
    core.reopen();
    assert!(
        AvailabilityManager::reload(core.reader(), PROJECT, policy())
            .unwrap()
            .records()
            .is_empty()
    );

    // The acknowledgement is lost after the commit: the manager is behind
    // the store, and the identical retry recovers it with the ORIGINAL
    // receipts and without a second demotion fact or duplicate events.
    core.arm(Fault::AckLost);
    assert!(m.persist_and_commit(&mut core, &scope, &effect).is_err());
    assert!(m.records().is_empty());
    assert_eq!(core.records(DecisionRecordKind::AvailabilityObservation), 2);
    assert_eq!((core.events(&[]), demotion_sequence(&core)), (2, 1));
    let retried = m.persist_and_commit(&mut core, &scope, &effect).unwrap();
    assert!(retried.replayed);
    assert_eq!(retried.events.len(), 2);
    assert_eq!(retried.demotion.unwrap().sequence, 1);
    assert_eq!(core.records(DecisionRecordKind::AvailabilityObservation), 2);
    assert_eq!((core.events(&[]), demotion_sequence(&core)), (2, 1));
    assert_eq!(m.records().len(), 2, "live now equals the store");
    core.reopen();
    assert_eq!(
        AvailabilityManager::reload(core.reader(), PROJECT, policy())
            .unwrap()
            .records(),
        m.records(),
        "live equals restarted"
    );

    // A different effect aimed at the already-used record ids is refused and
    // writes nothing.
    let mut stale = manager();
    let other = limit_effect(&stale, "model-a", FailureClass::ProviderDown, T1_PLUS_5);
    assert!(matches!(
        stale.persist_and_commit(&mut core, &scope, &other),
        Err(RoutingPersistError::Port(HarnessError::Conflict))
    ));
    assert!(stale.records().is_empty());
    assert_eq!(core.records(DecisionRecordKind::AvailabilityObservation), 2);
    assert_eq!((core.events(&[]), demotion_sequence(&core)), (2, 1));

    // A genuinely next effect appends after the first.
    let next = limit_effect(&m, "model-a", FailureClass::SessionExhausted, T1_PLUS_5);
    m.persist_and_commit(&mut core, &scope, &next).unwrap();
    assert_eq!(demotion_sequence(&core), 2);
    assert_eq!(
        core.records(DecisionRecordKind::AvailabilityObservation),
        3,
        "SESSION_EXHAUSTED marks only the exact candidate"
    );
}

// ---- repair: jittered backoff per (provider, model) (F8) -----------------------

fn jittered(draw: u64) -> AvailabilityManager {
    AvailabilityManager::new(policy()).with_jitter(std::sync::Arc::new(FixedJitter(draw)))
}

fn limit(effect: &ObservationEffect) -> (u64, u32, String) {
    let ObservationEffect::Limited {
        window_seconds,
        attempt,
        records,
        ..
    } = effect
    else {
        panic!("{effect:?}")
    };
    (
        *window_seconds,
        *attempt,
        records[0].core().note().unwrap().to_owned(),
    )
}

#[test]
fn backoff_is_capped_exponential_with_bounded_jitter_and_recorded_provenance() {
    let p = policy();
    assert_eq!(
        (1..=8).map(|n| p.window_range(n)).collect::<Vec<_>>(),
        [
            (5, 10),
            (10, 20),
            (20, 40),
            (40, 80),
            (60, 120),
            (60, 120),
            (60, 120),
            (60, 120)
        ]
    );
    assert_eq!(p.window_range(u32::MAX), (60, 120), "no overflow");
    for (draw, window, drawn) in [(0, 5, 0), (3, 8, 3), (5, 10, 5), (u64::MAX, 10, 5)] {
        let m = jittered(draw);
        let (got, attempt, note) = limit(
            &m.assess(&observation("model-a", FailureClass::RateLimited, None, T1))
                .unwrap(),
        );
        assert_eq!((got, attempt), (window, 1), "draw {draw}");
        assert!(
            note.contains("window_source=BACKOFF;backoff_ceiling=10;")
                && note.ends_with(&format!("jitter_seconds={drawn}")),
            "{note}"
        );
    }
    // A provider's retry-after is authoritative and is not jittered.
    let (got, _, note) = limit(
        &jittered(3)
            .assess(&observation(
                "model-a",
                FailureClass::RateLimited,
                Some(30),
                T1,
            ))
            .unwrap(),
    );
    assert_eq!(got, 30);
    assert!(note.ends_with("window_source=RETRY_AFTER"), "{note}");
    // The widest policy stays inside u64 whatever the source says.
    let wide = BackoffPolicy::try_new(u64::MAX, u64::MAX, 1).unwrap();
    for draw in [0, u64::MAX / 3, u64::MAX] {
        let m = AvailabilityManager::new(wide).with_jitter(std::sync::Arc::new(FixedJitter(draw)));
        let (got, _, _) = limit(
            &m.assess(&observation("model-a", FailureClass::RateLimited, None, T1))
                .unwrap(),
        );
        assert!((u64::MAX - u64::MAX / 2..=u64::MAX).contains(&got), "{got}");
    }
    // The production source varies inside its bound and never overflows.
    let draws: std::collections::BTreeSet<_> = (0..200)
        .map(|_| RandomJitter.pick("prov-a", "model-a", 1, 1000))
        .collect();
    assert!(
        draws.len() > 20 && draws.iter().all(|d| *d <= 1000),
        "{draws:?}"
    );
    let _ = RandomJitter.pick("prov-a", "model-a", 1, u64::MAX);
    assert_eq!(RandomJitter.pick("prov-a", "model-a", 1, 0), 0);
}

#[test]
fn backoff_history_belongs_to_provider_and_model_not_runtime_and_survives_restart() {
    let other_runtime = |model: &str, runtime: &str, at: &str| {
        let mut o = observation(model, FailureClass::RateLimited, None, at);
        o.runtime_id = ids("prov-a", model, runtime).2;
        o
    };
    let mut core = CoreHarness::new("backoff-history");
    let scope = CoreHarness::scope();
    let mut m = jittered(u64::MAX);
    for (n, (runtime, at)) in [
        ("rt-a", T1),
        ("rt-b", T1_PLUS_5),
        ("rt-c", "2026-11-08T10:00:10Z"),
    ]
    .into_iter()
    .enumerate()
    {
        let effect = m.assess(&other_runtime("model-a", runtime, at)).unwrap();
        let (_, attempt, _) = limit(&effect);
        assert_eq!(
            attempt,
            n as u32 + 1,
            "{runtime}: runtime change keeps history"
        );
        m.persist_and_commit(&mut core, &scope, &effect).unwrap();
    }
    // A sibling model of the same provider has its own history.
    let (_, attempt, _) = limit(
        &m.assess(&other_runtime("model-a2", "rt-a", "2026-11-08T10:00:11Z"))
            .unwrap(),
    );
    assert_eq!(attempt, 1);
    // A restart does not reset it.
    core.reopen();
    let reloaded = AvailabilityManager::reload(core.reader(), PROJECT, policy())
        .unwrap()
        .with_jitter(std::sync::Arc::new(FixedJitter(u64::MAX)));
    let (window, attempt, _) = limit(
        &reloaded
            .assess(&other_runtime("model-a", "rt-d", "2026-11-08T10:00:15Z"))
            .unwrap(),
    );
    assert_eq!((attempt, window), (4, 80));
}
