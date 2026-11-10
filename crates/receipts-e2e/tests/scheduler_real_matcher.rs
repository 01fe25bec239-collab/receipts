//! `schedule_dispatch_v1` through the real Workspace write-path engine.
//!
//! Orchestration's scheduler consumes the `WriteScopePort`; here that port is
//! `WorkspaceWriteScopeMatcher` (the real `WritePathPattern` parser and
//! `intersects` engine), so every verdict below comes from the real language,
//! not a synthetic matcher. Admission inputs are built from the public D11
//! types: every axis PASSes, so only write collision decides.

use receipts_e2e::WorkspaceWriteScopeMatcher;
use receipts_orchestration::orchestration::{FeatureAdmissionOutcome, OrchestrationDateTimeV1};
use receipts_orchestration::scheduler::{
    AdmissionBindingV1, AvailabilityEvidenceV1, AvailabilityObservationV1, ConcurrencyLimitsV1,
    DeferralV1, DispatchAdmissionProviderPolicyStatus, DispatchAdmissionRequestV1,
    DispatchBudgetV1, DispatchCandidateV1, DispatchScheduleV1, EntitlementEvidenceV1,
    EvidenceFreshnessV1, InFlightWriterV1, ProviderAuthEvidenceV1, ProviderConnectionV1,
    ProviderPolicyEvidenceV1, QualityFloorEvidenceV1, QualityFloorResultV1, SafetyEvidenceV1,
    SafetyStateV1, SchedulingContextV1, SchedulingDecisionV1, WriteScopeOverlapV1, WriteScopePort,
    schedule_dispatch_v1,
};
use receipts_orchestration::{
    ExecutionGraph, ExecutionGraphEnvelope, ExecutionGraphEnvelopeFields, GraphNode, GraphNodeKind,
    GraphNodeState, GraphVersionV1,
};
use receipts_workspace_execution::WritePathPattern;
use std::collections::BTreeMap;

const GRAPH: &str = "g";
const EPOCH: &str = "7";

/// Independent ready TASK nodes `a`, `b` (ws-a) and `c` (ws-b).
fn envelope() -> ExecutionGraphEnvelope {
    let mut graph = ExecutionGraph::new(GRAPH).unwrap();
    for (id, workstream) in [("a", "ws-a"), ("b", "ws-a"), ("c", "ws-b")] {
        let node = GraphNode::new(
            id,
            GRAPH,
            GraphNodeKind::new("TASK".to_owned()).unwrap(),
            GraphNodeState::Planned,
        )
        .unwrap()
        .with_workstream_id(workstream)
        .unwrap();
        graph.add_node(node).unwrap();
    }
    ExecutionGraphEnvelope::bind(
        &graph,
        ExecutionGraphEnvelopeFields::new(
            "p",
            "goal",
            GraphVersionV1::try_new("1").unwrap(),
            OrchestrationDateTimeV1::try_new("2026-11-08T00:00:00Z").unwrap(),
            "policy",
        ),
    )
    .unwrap()
}

fn binding(node: &str) -> AdmissionBindingV1 {
    AdmissionBindingV1 {
        node_id: node.into(),
        graph_id: Some(GRAPH.into()),
        context_epoch: EPOCH.into(),
    }
}

/// Fresh ALLOW / Authenticated / VerifiedAllowed / Available / Clear /
/// Satisfied on every axis for `node`, selecting `provider`.
fn admission(node: &str, provider: &str) -> DispatchAdmissionRequestV1 {
    let fresh = EvidenceFreshnessV1::Fresh;
    DispatchAdmissionRequestV1 {
        decision_id: format!("da-{node}"),
        capability_id: Some("graph.core".into()),
        binding: binding(node),
        selected_provider_id: Some(provider.into()),
        entitlement: Some(EntitlementEvidenceV1 {
            binding: binding(node),
            freshness: fresh,
            feature_admission_decision_id: "fa".into(),
            outcome: FeatureAdmissionOutcome::Allow,
        }),
        provider_auth: Some(ProviderAuthEvidenceV1 {
            binding: binding(node),
            freshness: fresh,
            provider_id: provider.into(),
            connection: ProviderConnectionV1::Authenticated,
            technical_status: "CONNECTED".into(),
        }),
        provider_policy: Some(ProviderPolicyEvidenceV1 {
            binding: binding(node),
            freshness: fresh,
            provider_policy_eligibility_id: "ppe".into(),
            provider_id: provider.into(),
            status: DispatchAdmissionProviderPolicyStatus::VerifiedAllowed,
            execution_context: "PROGRAMMATIC_WORKER".into(),
        }),
        provider_availability: Some(AvailabilityEvidenceV1 {
            binding: binding(node),
            freshness: fresh,
            availability_state_ref: "as".into(),
            observation: AvailabilityObservationV1::Available,
            runtime_id: Some("runtime".into()),
        }),
        safety: Some(SafetyEvidenceV1 {
            binding: binding(node),
            freshness: fresh,
            state: SafetyStateV1::Clear,
            safety_interruption_id: None,
        }),
        quality_floor: Some(QualityFloorEvidenceV1 {
            binding: binding(node),
            freshness: fresh,
            routing_decision_id: "rd".into(),
            result: QualityFloorResultV1::Satisfied,
        }),
    }
}

fn paths<S: AsRef<str>>(values: &[S]) -> Vec<String> {
    values.iter().map(|v| v.as_ref().to_owned()).collect()
}

fn candidate(node: &str, ready_sequence: u64, write: &[&str]) -> DispatchCandidateV1 {
    DispatchCandidateV1 {
        node_id: node.into(),
        write_paths: paths(write),
        cost_estimate: Some(1),
        ready_sequence,
        user_priority: 0,
        admission: admission(node, "openai"),
    }
}

fn in_flight(node: &str, write: &[&str]) -> InFlightWriterV1 {
    InFlightWriterV1 {
        node_id: node.into(),
        workstream_id: "ws-x".into(),
        provider_id: "anthropic".into(),
        write_paths: paths(write),
    }
}

fn round(running: &[InFlightWriterV1], candidates: &[DispatchCandidateV1]) -> DispatchScheduleV1 {
    round_capacity(running, candidates, 100)
}

fn round_capacity(
    running: &[InFlightWriterV1],
    candidates: &[DispatchCandidateV1],
    capacity: usize,
) -> DispatchScheduleV1 {
    let envelope = envelope();
    let limits = ConcurrencyLimitsV1 {
        global: capacity,
        per_workstream: BTreeMap::from([("ws-a".into(), 100), ("ws-b".into(), 100)]),
        per_provider: BTreeMap::from([("openai".into(), 100), ("anthropic".into(), 100)]),
    };
    let context = SchedulingContextV1 {
        envelope: &envelope,
        interface: None,
        current_context_epoch: EPOCH,
        limits: &limits,
        budget: DispatchBudgetV1 {
            remaining_worker_starts: capacity as u64,
            remaining_cost: Some(capacity as u64),
        },
        in_flight: running,
    };
    schedule_dispatch_v1(&context, candidates, &WorkspaceWriteScopeMatcher)
}

fn deferral(schedule: &DispatchScheduleV1, node: &str) -> DeferralV1 {
    match schedule.decision(node) {
        Some(SchedulingDecisionV1::Deferred { reason, .. }) => reason.clone(),
        other => panic!("{node}: expected a deferral, got {other:?}"),
    }
}

/// Both pattern sets accept `witness` under the real parser.
fn accepted_by_both(witness: &str, left: &[&str], right: &[&str]) {
    for pattern in left.iter().chain(right) {
        assert!(
            WritePathPattern::parse(pattern).unwrap().matches(witness),
            "{pattern} does not accept {witness}"
        );
    }
}

#[test]
fn lone_and_mixed_invalid_scopes_never_admit() {
    for write in [vec!["../escape/**"], vec!["docs/**", "../escape/**"]] {
        let schedule = round(&[], &[candidate("a", 0, &write)]);
        assert!(schedule.admitted().is_empty(), "{write:?}");
    }
    assert_eq!(
        round(&[], &[candidate("a", 0, &["docs/**"])]).admitted(),
        ["a"]
    );
}

#[test]
fn an_in_flight_overlap_defers_with_a_witness_both_patterns_accept() {
    let running = [in_flight("run-1", &["src/core/**"])];
    let schedule = round(&running, &[candidate("a", 0, &["src/core/scheduler.rs"])]);
    assert!(schedule.admitted().is_empty());
    let DeferralV1::WriteConflict { with, witness } = deferral(&schedule, "a") else {
        panic!("{:?}", schedule.decision("a"))
    };
    assert_eq!(with, "run-1");
    accepted_by_both(&witness, &["src/core/**"], &["src/core/scheduler.rs"]);
}

#[test]
fn a_candidate_disjoint_from_in_flight_work_is_admitted() {
    let running = [in_flight("run-1", &["src/core/**"])];
    let schedule = round(&running, &[candidate("a", 0, &["docs/**"])]);
    assert_eq!(schedule.admitted(), ["a"]);
}

#[test]
fn same_round_overlap_admits_the_first_and_defers_the_second_against_it() {
    let schedule = round(
        &[],
        &[
            candidate("b", 1, &["docs/readme.md"]),
            candidate("a", 0, &["docs/**"]),
        ],
    );
    assert_eq!(
        schedule.admitted(),
        ["a"],
        "the older ready node goes first"
    );
    let DeferralV1::WriteConflict { with, witness } = deferral(&schedule, "b") else {
        panic!("{:?}", schedule.decision("b"))
    };
    assert_eq!(with, "a");
    accepted_by_both(&witness, &["docs/**"], &["docs/readme.md"]);
}

#[test]
fn same_round_disjoint_writers_are_both_admitted() {
    let schedule = round(
        &[],
        &[
            candidate("a", 0, &["src/state/**"]),
            candidate("b", 1, &["src/core/**"]),
        ],
    );
    assert_eq!(schedule.admitted(), ["a", "b"]);
}

#[test]
fn an_invalid_pattern_is_uncertainty_never_admission() {
    // Against in-flight work, in either position.
    let running = [in_flight("run-1", &["src/**"])];
    let schedule = round(&running, &[candidate("a", 0, &["../escape/**"])]);
    assert!(schedule.admitted().is_empty());
    assert_eq!(deferral(&schedule, "a"), DeferralV1::WriteScopeUnvalidated);
    let running = [in_flight("run-1", &["/absolute/**"])];
    let schedule = round(&running, &[candidate("a", 0, &["docs/**"])]);
    assert!(schedule.admitted().is_empty());
    assert_eq!(
        deferral(&schedule, "a"),
        DeferralV1::WriteOverlapUndetermined {
            with: "run-1".into()
        }
    );
    // Against an earlier writer of the same round: the invalid one defers.
    let schedule = round(
        &[],
        &[
            candidate("a", 0, &["docs/**"]),
            candidate("b", 1, &["../escape/**"]),
        ],
    );
    assert_eq!(schedule.admitted(), ["a"]);
    assert_eq!(deferral(&schedule, "b"), DeferralV1::WriteScopeUnvalidated);
    // An invalid first candidate leaves the sole worker/cost/concurrency slot free.
    let schedule = round_capacity(
        &[],
        &[
            candidate("a", 0, &["../escape/**"]),
            candidate("b", 1, &["docs/**"]),
        ],
        1,
    );
    assert_eq!(schedule.admitted(), ["b"]);
    assert_eq!(deferral(&schedule, "a"), DeferralV1::WriteScopeUnvalidated);
}

fn literals(prefix: &str, count: usize) -> Vec<String> {
    (0..count).map(|i| format!("{prefix}/x{i}.rs")).collect()
}

#[test]
fn engine_resource_exhaustion_is_uncertainty_against_in_flight_and_between_candidates() {
    let (left, right) = (literals("a", 400), literals("b", 400));
    // The sets are valid and pairwise disjoint: the uncertainty is the real
    // engine's work limit, not a parse failure. A small control proves it.
    for pattern in left.iter().chain(&right) {
        WritePathPattern::parse(pattern).unwrap();
    }
    let matcher = WorkspaceWriteScopeMatcher;
    assert_eq!(
        matcher.overlap(&left, &right),
        WriteScopeOverlapV1::Undetermined
    );
    assert_eq!(
        matcher.overlap(&literals("a", 20), &literals("b", 20)),
        WriteScopeOverlapV1::Disjoint
    );

    let big = |node: &str, seq: u64, set: &[String]| DispatchCandidateV1 {
        write_paths: set.to_vec(),
        ..candidate(node, seq, &["unused"])
    };
    // Against in-flight work.
    let running = [InFlightWriterV1 {
        write_paths: left.clone(),
        ..in_flight("run-1", &[])
    }];
    let schedule = round(&running, &[big("a", 0, &right)]);
    assert!(
        schedule.admitted().is_empty(),
        "the later writer is never admitted"
    );
    assert_eq!(
        deferral(&schedule, "a"),
        DeferralV1::WriteOverlapUndetermined {
            with: "run-1".into()
        }
    );
    // Between two candidates of one round.
    let schedule = round(&[], &[big("a", 0, &left), big("b", 1, &right)]);
    assert_eq!(schedule.admitted(), ["a"]);
    assert_eq!(
        deferral(&schedule, "b"),
        DeferralV1::WriteOverlapUndetermined { with: "a".into() }
    );
    // Control: the same two sets at a size the engine can decide are admitted.
    let schedule = round(
        &[],
        &[
            big("a", 0, &literals("a", 20)),
            big("b", 1, &literals("b", 20)),
        ],
    );
    assert_eq!(schedule.admitted(), ["a", "b"]);
}
