//! Deterministic scheduling: priority, D11 consumption (S22/S44), stale epoch,
//! budgets, concurrency/backpressure and write collision (S10) controls.

use std::collections::BTreeMap;

use super::*;
use crate::GraphNodeState::{Accepted, Planned, Running};
use crate::orchestration::FeatureAdmissionOutcome;
use crate::test_support::*;

fn limits(global: usize) -> ConcurrencyLimitsV1 {
    ConcurrencyLimitsV1 {
        global,
        per_workstream: BTreeMap::from([("ws-a".into(), 10), ("ws-b".into(), 10)]),
        per_provider: BTreeMap::from([("openai".into(), 10), ("anthropic".into(), 10)]),
    }
}

const BUDGET: DispatchBudgetV1 = DispatchBudgetV1 {
    remaining_worker_starts: 100,
    remaining_cost: None,
};

fn candidate(node: &str, write: &[&str]) -> DispatchCandidateV1 {
    DispatchCandidateV1 {
        node_id: node.into(),
        write_paths: paths(write),
        cost_estimate: Some(1),
        ready_sequence: 0,
        user_priority: 0,
        admission: passing(node, "openai"),
    }
}

fn run(
    envelope: &crate::ExecutionGraphEnvelope,
    limits: &ConcurrencyLimitsV1,
    budget: DispatchBudgetV1,
    in_flight: &[InFlightWriterV1],
    candidates: &[DispatchCandidateV1],
) -> DispatchScheduleV1 {
    let context = SchedulingContextV1 {
        envelope,
        interface: None,
        current_context_epoch: EPOCH,
        limits,
        budget,
        in_flight,
    };
    schedule_dispatch_v1(&context, candidates, &PORT)
}

/// Five independent ready tasks a..e in workstream ws-a.
fn flat() -> crate::ExecutionGraphEnvelope {
    envelope(
        &[
            ("a", "TASK", Planned, Some("ws-a")),
            ("b", "TASK", Planned, Some("ws-a")),
            ("c", "TASK", Planned, Some("ws-a")),
            ("d", "TASK", Planned, Some("ws-b")),
            ("e", "TASK", Planned, Some("ws-b")),
        ],
        &[],
    )
}

fn deferral(schedule: &DispatchScheduleV1, node: &str) -> DeferralV1 {
    match schedule.decision(node) {
        Some(SchedulingDecisionV1::Deferred { reason, .. }) => reason.clone(),
        other => panic!("{node}: expected a deferral, got {other:?}"),
    }
}

fn refusal(schedule: &DispatchScheduleV1, node: &str) -> SchedulingRefusalV1 {
    match schedule.decision(node) {
        Some(SchedulingDecisionV1::Refused(r)) => r.clone(),
        other => panic!("{node}: expected a refusal, got {other:?}"),
    }
}

#[test]
fn priority_is_critical_path_first_then_blocking_age_user_and_id() {
    // a -> b -> c (a heads a 3-chain); e -> f; d alone; x and y equal.
    let env = envelope(
        &[
            ("a", "TASK", Planned, Some("ws-a")),
            ("b", "TASK", Planned, Some("ws-a")),
            ("c", "TASK", Planned, Some("ws-a")),
            ("d", "TASK", Planned, Some("ws-a")),
            ("e", "TASK", Planned, Some("ws-a")),
            ("f", "TASK", Planned, Some("ws-a")),
            ("g1", "TASK", Planned, Some("ws-a")),
            ("g2", "TASK", Planned, Some("ws-a")),
            ("w", "TASK", Planned, Some("ws-b")),
            ("x", "TASK", Planned, Some("ws-b")),
            ("y", "TASK", Planned, Some("ws-b")),
            ("z", "TASK", Planned, Some("ws-b")),
        ],
        &[
            ("e1", "a", "b"),
            ("e2", "b", "c"),
            ("e3", "e", "f"),
            // d blocks two nodes at depth 2, e blocks one: blocking count breaks the tie.
            ("e4", "d", "g1"),
            ("e5", "d", "g2"),
        ],
    );
    let mut cands: Vec<_> = ["z", "y", "x", "w", "e", "d", "a"]
        .iter()
        .enumerate()
        .map(|(i, n)| candidate(n, &[&format!("p{i}/**")]))
        .collect();
    // w, x, y, z all have critical path 1 and no dependents.
    cands[3].ready_sequence = 1; // w is younger than x, y, z
    cands[1].user_priority = 5; // y outranks x and z on user priority
    let schedule = run(&env, &limits(10), BUDGET, &[], &cands);
    assert_eq!(schedule.admitted(), ["a", "d", "e", "y", "x", "z", "w"]);
    let p = schedule.decisions[0].priority.unwrap();
    assert_eq!((p.critical_path, p.blocking_count), (3, 2));
    // Determinism: input order never changes the result.
    let mut reversed = cands.clone();
    reversed.reverse();
    assert_eq!(run(&env, &limits(10), BUDGET, &[], &reversed), schedule);
}

#[test]
fn each_axis_failure_is_denied_naming_only_that_axis_s44() {
    use super::DispatchAdmissionFailingAxis as Axis;
    let env = flat();
    for axis in super::PRIMARY_FAILURE_PRECEDENCE {
        let mut c = candidate("a", &["a/**"]);
        let r = &mut c.admission;
        match axis {
            Axis::Safety => {
                let s = r.safety.as_mut().unwrap();
                s.state = SafetyStateV1::Pending;
                s.safety_interruption_id = Some("si".into());
            }
            Axis::Entitlement => {
                r.entitlement.as_mut().unwrap().outcome =
                    FeatureAdmissionOutcome::EntitlementExpired
            }
            Axis::ProviderPolicy => {
                r.provider_policy.as_mut().unwrap().status =
                    DispatchAdmissionProviderPolicyStatus::NeedsReview
            }
            Axis::ProviderAuth => {
                r.provider_auth.as_mut().unwrap().connection = ProviderConnectionV1::Unknown
            }
            Axis::ProviderAvailability => {
                r.provider_availability.as_mut().unwrap().observation =
                    AvailabilityObservationV1::ProvenDown
            }
            Axis::QualityFloor => {
                r.quality_floor.as_mut().unwrap().result = QualityFloorResultV1::Unsatisfied
            }
            Axis::None => unreachable!(),
        }
        let schedule = run(&env, &limits(10), BUDGET, &[], &[c]);
        assert!(schedule.admitted().is_empty(), "{axis:?}");
        let Some(SchedulingDecisionV1::Denied { derivation }) = schedule.decision("a") else {
            panic!("{axis:?}: expected a denial");
        };
        assert_eq!(derivation.decision().core().failing_axis(), axis);
        assert_eq!(
            derivation.failures().len(),
            1,
            "{axis:?}: only that axis fails"
        );
    }
}

#[test]
fn free_invoking_pro_capability_dispatches_nothing_s22() {
    let env = flat();
    let mut c = candidate("a", &["a/**"]);
    c.admission.entitlement.as_mut().unwrap().outcome = FeatureAdmissionOutcome::LockedRequiresPro;
    let schedule = run(&env, &limits(10), BUDGET, &[], &[c]);
    assert!(schedule.admitted().is_empty());
    let Some(SchedulingDecisionV1::Denied { derivation }) = schedule.decision("a") else {
        panic!("expected a denial");
    };
    assert_eq!(
        derivation.decision().core().denial_reason(),
        Some(DispatchAdmissionDenialReason::LockedRequiresPro)
    );
    assert_eq!(derivation.decision().core().selected_provider(), None);
}

#[test]
fn missing_or_stale_evidence_never_admits_and_multi_failure_follows_precedence() {
    let env = flat();
    let mut missing = candidate("a", &["a/**"]);
    missing.admission.provider_auth = None;
    let mut stale = candidate("b", &["b/**"]);
    stale.admission.quality_floor.as_mut().unwrap().freshness = EvidenceFreshnessV1::Stale;
    let mut multi = candidate("c", &["c/**"]);
    multi.admission.quality_floor = None;
    multi.admission.entitlement = None;
    multi.admission.provider_availability = None;
    let schedule = run(&env, &limits(10), BUDGET, &[], &[missing, stale, multi]);
    assert!(schedule.admitted().is_empty());
    let axis = |node| match schedule.decision(node) {
        Some(SchedulingDecisionV1::Denied { derivation }) => (
            derivation.decision().core().failing_axis(),
            derivation.failures().len(),
        ),
        other => panic!("{node}: {other:?}"),
    };
    use DispatchAdmissionFailingAxis as Axis;
    assert_eq!(axis("a"), (Axis::ProviderAuth, 1));
    assert_eq!(axis("b"), (Axis::QualityFloor, 1));
    assert_eq!(axis("c"), (Axis::Entitlement, 3));
}

#[test]
fn stale_epoch_binding_not_ready_and_malformed_candidates_are_refused() {
    let env = envelope(
        &[
            ("a", "TASK", Planned, Some("ws-a")),
            ("b", "TASK", Planned, Some("ws-a")),
            ("done", "TASK", Accepted, Some("ws-a")),
            ("run", "TASK", Running, Some("ws-a")),
            ("loose", "TASK", Planned, None),
            ("ok", "TASK", Planned, Some("ws-a")),
        ],
        &[("e1", "a", "b")],
    );
    let mut stale = candidate("a", &["a/**"]);
    stale.admission.binding.context_epoch = "6".into();
    let mut foreign = candidate("ok", &["ok/**"]);
    foreign.admission.binding.graph_id = Some("other".into());
    let mut evidence = candidate("ok", &["ok/**"]);
    evidence.node_id = "ok".into();
    let mut empty = candidate("ok", &[]);
    empty.node_id = "ok".into();
    let schedule = run(
        &env,
        &limits(10),
        BUDGET,
        &[],
        &[
            stale,
            candidate("b", &["b/**"]),
            candidate("done", &["d/**"]),
            candidate("run", &["r/**"]),
            candidate("loose", &["l/**"]),
            candidate("ghost", &["g/**"]),
            foreign,
        ],
    );
    assert!(schedule.admitted().is_empty());
    assert_eq!(
        refusal(&schedule, "a"),
        SchedulingRefusalV1::StaleContextEpoch
    );
    assert_eq!(refusal(&schedule, "b"), SchedulingRefusalV1::NotReady);
    assert_eq!(refusal(&schedule, "done"), SchedulingRefusalV1::NotReady);
    assert_eq!(refusal(&schedule, "run"), SchedulingRefusalV1::NotReady);
    assert_eq!(
        refusal(&schedule, "loose"),
        SchedulingRefusalV1::NodeWithoutWorkstream
    );
    assert_eq!(
        refusal(&schedule, "ghost"),
        SchedulingRefusalV1::UnknownNode
    );
    assert_eq!(
        refusal(&schedule, "ok"),
        SchedulingRefusalV1::BindingMismatch
    );

    let schedule = run(&env, &limits(10), BUDGET, &[], &[empty]);
    assert_eq!(
        refusal(&schedule, "ok"),
        SchedulingRefusalV1::EmptyWriteScope
    );
    let dup = run(
        &env,
        &limits(10),
        BUDGET,
        &[],
        &[candidate("ok", &["ok/**"]), evidence],
    );
    assert_eq!(dup.admitted(), ["ok"]);
    assert_eq!(
        dup.decisions[1].decision,
        SchedulingDecisionV1::Refused(SchedulingRefusalV1::DuplicateCandidate)
    );

    // Evidence bound to another epoch is a typed D11 refusal, not a denial.
    let mut wrong = candidate("ok", &["ok/**"]);
    wrong
        .admission
        .safety
        .as_mut()
        .unwrap()
        .binding
        .context_epoch = "6".into();
    let schedule = run(&env, &limits(10), BUDGET, &[], &[wrong]);
    assert!(matches!(
        refusal(&schedule, "ok"),
        SchedulingRefusalV1::Evidence(DispatchAdmissionRefusalV1::EvidenceBindingMismatch { .. })
    ));
}

#[test]
fn global_workstream_and_provider_limits_apply_backpressure() {
    let env = flat();
    let all: Vec<_> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|n| candidate(n, &[&format!("{n}/**")]))
        .collect();
    // Global 2 with one in flight: only one more.
    let running = InFlightWriterV1 {
        node_id: "r".into(),
        workstream_id: "ws-a".into(),
        provider_id: "openai".into(),
        write_paths: paths(&["r/**"]),
    };
    let schedule = run(
        &env,
        &limits(2),
        BUDGET,
        std::slice::from_ref(&running),
        &all,
    );
    assert_eq!(schedule.admitted(), ["a"]);
    for node in ["b", "c", "d", "e"] {
        assert_eq!(deferral(&schedule, node), DeferralV1::GlobalConcurrency);
    }

    // Per-workstream 1 (ws-a has one running): ws-a waits, ws-b proceeds once.
    let mut l = limits(10);
    l.per_workstream = BTreeMap::from([("ws-a".into(), 1), ("ws-b".into(), 1)]);
    let schedule = run(&env, &l, BUDGET, std::slice::from_ref(&running), &all);
    assert_eq!(schedule.admitted(), ["d"]);
    assert_eq!(
        deferral(&schedule, "a"),
        DeferralV1::WorkstreamConcurrency {
            workstream_id: "ws-a".into()
        }
    );
    assert_eq!(
        deferral(&schedule, "e"),
        DeferralV1::WorkstreamConcurrency {
            workstream_id: "ws-b".into()
        }
    );

    // Per-provider 2 for openai (one running); anthropic work is unaffected.
    let mut l = limits(10);
    l.per_provider.insert("openai".into(), 2);
    let mut mixed = all.clone();
    mixed[4].admission = passing("e", "anthropic");
    let schedule = run(&env, &l, BUDGET, std::slice::from_ref(&running), &mixed);
    assert_eq!(schedule.admitted(), ["a", "e"]);
    assert_eq!(
        deferral(&schedule, "b"),
        DeferralV1::ProviderConcurrency {
            provider_id: "openai".into()
        }
    );

    // Unconfigured workstream/provider limits defer instead of passing.
    let mut l = limits(10);
    l.per_workstream.remove("ws-b");
    l.per_provider.remove("anthropic");
    let schedule = run(&env, &l, BUDGET, &[], &mixed);
    assert_eq!(
        deferral(&schedule, "d"),
        DeferralV1::WorkstreamLimitUnconfigured {
            workstream_id: "ws-b".into()
        }
    );
    let mut only_a = vec![candidate("a", &["a/**"])];
    only_a[0].admission = passing("a", "anthropic");
    let schedule = run(&env, &l, BUDGET, &[], &only_a);
    assert_eq!(
        deferral(&schedule, "a"),
        DeferralV1::ProviderLimitUnconfigured {
            provider_id: "anthropic".into()
        }
    );
}

#[test]
fn worker_and_cost_budgets_are_prechecked_and_consumed() {
    let env = flat();
    let mut cands: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|n| candidate(n, &[&format!("{n}/**")]))
        .collect();
    let workers = DispatchBudgetV1 {
        remaining_worker_starts: 2,
        remaining_cost: None,
    };
    let schedule = run(&env, &limits(10), workers, &[], &cands);
    assert_eq!(schedule.admitted(), ["a", "b"]);
    assert_eq!(deferral(&schedule, "c"), DeferralV1::WorkerBudgetExhausted);

    cands[0].cost_estimate = Some(6);
    cands[1].cost_estimate = Some(5);
    cands[2].cost_estimate = None;
    let cost = DispatchBudgetV1 {
        remaining_worker_starts: 10,
        remaining_cost: Some(10),
    };
    let schedule = run(&env, &limits(10), cost, &[], &cands);
    assert_eq!(schedule.admitted(), ["a"]);
    assert_eq!(deferral(&schedule, "b"), DeferralV1::CostBudgetExhausted);
    assert_eq!(deferral(&schedule, "c"), DeferralV1::CostEstimateUnknown);

    // Budget is checked before concurrency: the first failing axis is named.
    let schedule = run(
        &env,
        &limits(0),
        DispatchBudgetV1 {
            remaining_worker_starts: 0,
            remaining_cost: None,
        },
        &[],
        &cands,
    );
    assert_eq!(deferral(&schedule, "a"), DeferralV1::WorkerBudgetExhausted);
}

#[test]
fn overlapping_writers_serialize_disjoint_run_parallel_unknown_defers_s10() {
    let env = flat();
    let running = InFlightWriterV1 {
        node_id: "r".into(),
        workstream_id: "ws-b".into(),
        provider_id: "openai".into(),
        write_paths: paths(&["src/core/**"]),
    };
    let cands = vec![
        candidate("a", &["src/core/scheduler.rs"]), // overlaps the running writer
        candidate("b", &["docs/**"]),               // disjoint
        candidate("c", &["docs/readme.md"]),        // overlaps b, admitted first this round
        candidate("d", &["src/st?te/**"]),          // undetermined
        candidate("e", &["src/state/**"]),          // disjoint from everything
    ];
    let schedule = run(
        &env,
        &limits(10),
        BUDGET,
        std::slice::from_ref(&running),
        &cands,
    );
    assert_eq!(schedule.admitted(), ["b", "e"]);
    assert_eq!(
        deferral(&schedule, "a"),
        DeferralV1::WriteConflict {
            with: "r".into(),
            witness: "src/core/scheduler.rs".into()
        }
    );
    assert_eq!(
        deferral(&schedule, "c"),
        DeferralV1::WriteConflict {
            with: "b".into(),
            witness: "docs/readme.md".into()
        }
    );
    assert_eq!(
        deferral(&schedule, "d"),
        DeferralV1::WriteOverlapUndetermined { with: "r".into() }
    );

    // A matcher resource limit is uncertainty: the second writer waits.
    let big: Vec<String> = (0..10).map(|i| format!("x{i}.rs")).collect();
    let mut a = candidate("a", &[]);
    a.write_paths = big.clone();
    let mut b = candidate("b", &[]);
    b.write_paths = (0..10).map(|i| format!("y{i}.rs")).collect();
    let schedule = run(&env, &limits(10), BUDGET, &[], &[a, b]);
    assert_eq!(schedule.admitted(), ["a"]);
    assert_eq!(
        deferral(&schedule, "b"),
        DeferralV1::WriteOverlapUndetermined { with: "a".into() }
    );
}
