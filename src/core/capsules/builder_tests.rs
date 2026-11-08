//! Capsule builders: graph-derived fields, State-current epoch, workstream
//! write binding, credential heuristic and N4 repair invariants.

use super::*;
use crate::GraphNodeState::{Accepted, Planned, Rejected};
use crate::scheduler::WorkstreamOwnershipV1;
use crate::test_support::*;
use crate::{
    CapabilityName, ControlKind, ExecutionGraph, ExecutionGraphEnvelope,
    ExecutionGraphEnvelopeFields, GraphEdge, GraphNode, GraphNodeAttemptNumber, GraphNodeKind,
    GraphNodeState, GraphResultLedgerV1, GraphVersionV1, PrecedenceKind,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const SHA2: &str = "89abcdef0123456789abcdef0123456789abcdef";

fn ownership() -> Vec<WorkstreamOwnershipV1> {
    vec![
        WorkstreamOwnershipV1 {
            workstream_id: "ws-a".into(),
            owned_paths: paths(&["src/core/**"]),
        },
        WorkstreamOwnershipV1 {
            workstream_id: "ws-b".into(),
            owned_paths: paths(&["src/state/**"]),
        },
    ]
}

fn criterion() -> Criterion {
    Criterion::try_new(
        "c1".into(),
        "scheduler tests pass".into(),
        CriterionKind::Deterministic,
        Some(paths(&["cargo", "test"])),
        None,
    )
    .unwrap()
}

fn request(node: &str) -> TaskCapsuleRequestV1 {
    TaskCapsuleRequestV1 {
        node_id: node.into(),
        task_type: TaskType::Implementation,
        objective: "Implement the scheduler.".into(),
        acceptance_criteria: vec![criterion()],
        non_goals: None,
        baseline_sha: SHA.into(),
        start_sha: SHA.into(),
        allowed_write_paths: paths(&["src/core/scheduler/**"]),
        forbidden_write_paths: paths(&["src/core/graph/**"]),
        relevant_context_refs: Some(vec![
            Ref::try_new(
                RefType::RepoPath,
                "docs/orchestration/README.md".into(),
                None,
                None,
            )
            .unwrap(),
        ]),
        architecture_refs: None,
        contract_refs: None,
        quality_floor: QualityFloor::Balanced,
        preferred_capabilities: None,
        verification_plan: vec![
            VerificationPlanEntryV1::try_new(paths(&["cargo", "test"]), 0).unwrap(),
        ],
        review_policy: None,
        assurance_profile: AssuranceProfileSelector::Standard,
        cost_policy: None,
        time_budget_seconds: Some(600),
        turn_budget: None,
        remote_publish_policy: None,
        stop_conditions: paths(&["tests fail twice"]),
        handoff_schema: None,
        context_epoch: EPOCH.into(),
    }
}

fn task_env() -> ExecutionGraphEnvelope {
    let mut env = envelope(
        &[
            ("dep", "TASK", Accepted, Some("ws-a")),
            ("t", "TASK", Planned, Some("ws-a")),
            ("waiting", "TASK", Planned, Some("ws-a")),
            ("blocker", "TASK", Planned, Some("ws-a")),
            ("rev", "REVIEW", Planned, Some("ws-a")),
            ("loose", "TASK", Planned, None),
        ],
        &[("e1", "dep", "t"), ("e2", "blocker", "waiting")],
    );
    // `t` requires a capability; the builder copies it from the graph.
    let mut graph = env.graph().clone();
    let t = graph.node("t").unwrap().clone();
    let mut rebuilt = ExecutionGraph::new(GRAPH).unwrap();
    for node in graph.nodes() {
        if node.node_id() == "t" {
            rebuilt
                .add_node(
                    t.clone().with_required_capabilities(vec![
                        CapabilityName::new("graph.core").unwrap(),
                    ]),
                )
                .unwrap();
        } else {
            rebuilt.add_node(node.clone()).unwrap();
        }
    }
    for edge in graph.edges() {
        rebuilt.add_edge(edge.clone()).unwrap();
    }
    graph = rebuilt;
    env = ExecutionGraphEnvelope::bind(&graph, fields()).unwrap();
    env
}

fn fields() -> ExecutionGraphEnvelopeFields {
    ExecutionGraphEnvelopeFields::new(
        "p",
        "goal",
        GraphVersionV1::try_new("1").unwrap(),
        time("2026-11-08T00:00:00Z"),
        "policy",
    )
}

fn build(
    env: &ExecutionGraphEnvelope,
    writer: &crate::orchestration::CoreStateWriter,
    request: TaskCapsuleRequestV1,
) -> Result<TaskCapsule, CapsuleBuildError> {
    let ownership = ownership();
    let context = CapsuleContextV1 {
        envelope: env,
        interface: None,
        state: writer.reader(),
        ownership: &ownership,
        worktree: "/work/t",
    };
    build_task_capsule_v1(&context, &PORT, request)
}

#[test]
fn task_capsule_takes_graph_fields_and_the_current_state_epoch() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["6", "7"]);
    let capsule = build(&task_env(), &writer, request("t")).unwrap();
    assert_eq!(capsule.task_id(), "t");
    assert_eq!(capsule.workstream_id(), "ws-a");
    assert_eq!(capsule.attempt_number(), 1);
    assert_eq!(capsule.dependencies(), Some(&["dep".to_string()][..]));
    assert_eq!(
        capsule.required_capabilities(),
        &[CapabilityName::new("graph.core").unwrap()]
    );
    assert_eq!(capsule.branch(), "runtime-a3/t");
    assert_eq!(capsule.worktree(), "/work/t");
    assert_eq!(capsule.context_epoch(), 7);
    // Other workstreams' ownership is an explicit trap.
    assert_eq!(
        capsule.forbidden_write_paths(),
        &paths(&["src/core/graph/**", "src/state/**"])[..]
    );
    // Deterministic.
    assert_eq!(build(&task_env(), &writer, request("t")).unwrap(), capsule);
}

#[test]
fn stale_or_missing_epoch_capsule_is_not_built() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["6", "7"]);
    let mut stale = request("t");
    stale.context_epoch = "6".into();
    assert_eq!(
        build(&task_env(), &writer, stale),
        Err(CapsuleBuildError::StaleEpoch)
    );
    let empty_dir = Dir::new();
    let empty = writer_with_epochs(&empty_dir, "policy", &[]);
    assert_eq!(
        build(&task_env(), &empty, request("t")),
        Err(CapsuleBuildError::EpochUnavailable)
    );
    let huge_dir = Dir::new();
    let huge = writer_with_epochs(&huge_dir, "policy", &["99999999999999999999"]);
    let mut big = request("t");
    big.context_epoch = "99999999999999999999".into();
    assert_eq!(
        build(&task_env(), &huge, big),
        Err(CapsuleBuildError::EpochOutOfRange)
    );
}

#[test]
fn writes_must_stay_inside_the_nodes_workstream() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let env = task_env();
    let with = |allowed: &[&str]| {
        let mut r = request("t");
        r.allowed_write_paths = paths(allowed);
        build(&env, &writer, r)
    };
    for wildcard in ["**", "*/x.rs", "**/lib.rs", "/abs/**"] {
        assert!(
            matches!(
                with(&[wildcard]),
                Err(CapsuleBuildError::PermissiveWildcard { .. })
            ),
            "{wildcard}"
        );
    }
    assert!(matches!(
        with(&["docs/**"]),
        Err(CapsuleBuildError::OutsideWorkstream { .. })
    ));
    // Inside ws-a but into an explicitly forbidden path.
    assert!(matches!(
        with(&["src/core/graph/lib.rs"]),
        Err(CapsuleBuildError::WriteScopeCollision { .. })
    ));
    assert!(matches!(
        with(&["src/core/sch?duler/**"]),
        Err(CapsuleBuildError::WriteScopeUndetermined { .. })
    ));
    // An ownership overlap with another workstream is refused.
    let mut overlapping = ownership();
    overlapping[1].owned_paths = paths(&["src/core/scheduler/**"]);
    let context = CapsuleContextV1 {
        envelope: &env,
        interface: None,
        state: writer.reader(),
        ownership: &overlapping,
        worktree: "/work/t",
    };
    assert!(matches!(
        build_task_capsule_v1(&context, &PORT, request("t")),
        Err(CapsuleBuildError::WriteScopeCollision { .. })
    ));
    // No ownership for the node's workstream at all.
    let context = CapsuleContextV1 {
        ownership: &overlapping[1..],
        ..context
    };
    assert_eq!(
        build_task_capsule_v1(&context, &PORT, request("t")),
        Err(CapsuleBuildError::OwnershipMissing)
    );
}

#[test]
fn capsule_with_token_like_value_is_rejected() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let env = task_env();
    let mut objective = request("t");
    objective.objective = "Use ghp_0123456789abcdefABCDEF0123 to push".into();
    let mut command = request("t");
    command.verification_plan = vec![
        VerificationPlanEntryV1::try_new(paths(&["curl", "-H", "Authorization: Bearer x"]), 0)
            .unwrap(),
    ];
    let mut reference = request("t");
    reference.relevant_context_refs = Some(vec![
        Ref::try_new(RefType::Url, "https://x/?token=abc".into(), None, None).unwrap(),
    ]);
    for (request, field) in [
        (objective, "objective"),
        (command, "verification_plan"),
        (reference, "relevant_context_refs"),
    ] {
        assert_eq!(
            build(&env, &writer, request),
            Err(CapsuleBuildError::CredentialShapedValue { field })
        );
    }
}

#[test]
fn unready_unknown_and_wrong_kind_nodes_are_not_built() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let env = task_env();
    assert_eq!(
        build(&env, &writer, request("waiting")),
        Err(CapsuleBuildError::NotReady)
    );
    assert_eq!(
        build(&env, &writer, request("ghost")),
        Err(CapsuleBuildError::UnknownNode)
    );
    assert_eq!(
        build(&env, &writer, request("rev")),
        Err(CapsuleBuildError::WrongNodeKind)
    );
    assert_eq!(
        build(&env, &writer, request("loose")),
        Err(CapsuleBuildError::NodeWithoutWorkstream)
    );
    let mut repair = request("t");
    repair.task_type = TaskType::Repair;
    assert_eq!(
        build(&env, &writer, repair),
        Err(CapsuleBuildError::RepairNeedsRepairBuilder)
    );
}

// --- repairs -----------------------------------------------------------------

fn node(id: &str, kind: GraphNodeKind, state: GraphNodeState) -> GraphNode {
    GraphNode::new(id, GRAPH, kind, state)
        .unwrap()
        .with_workstream_id("ws-a")
        .unwrap()
}

/// i1 (accepted, SHA) -> r1 (REJECTED review) -ON_REJECT-> i2 (repair, attempt 2) -> r2.
fn repair_env(
    judged_sha: Option<&str>,
    r1_state: GraphNodeState,
    repair_parent: &str,
) -> ExecutionGraphEnvelope {
    let mut graph = ExecutionGraph::new(GRAPH).unwrap();
    let mut i1 = node("i1", GraphNodeKind::IMPLEMENTATION, Accepted);
    if let Some(sha) = judged_sha {
        i1 = i1.with_code_sha(sha).unwrap();
    }
    graph.add_node(i1).unwrap();
    graph
        .add_node(node("other", GraphNodeKind::TASK, Accepted))
        .unwrap();
    graph
        .add_node(node("r1", GraphNodeKind::REVIEW, r1_state))
        .unwrap();
    graph
        .add_node(
            node("i2", GraphNodeKind::REPAIR, Planned)
                .with_parent_node_id(repair_parent)
                .unwrap()
                .with_attempt_number(GraphNodeAttemptNumber::try_new("2").unwrap()),
        )
        .unwrap();
    graph
        .add_node(node("r2", GraphNodeKind::REVIEW, Planned))
        .unwrap();
    for edge in [
        GraphEdge::precedence("e1", GRAPH, "i1", "r1", PrecedenceKind::RequiresAccepted),
        GraphEdge::control("e2", GRAPH, "r1", "i2", ControlKind::OnReject),
        GraphEdge::precedence("e3", GRAPH, "i2", "r2", PrecedenceKind::RequiresAccepted),
    ] {
        graph.add_edge(edge.unwrap()).unwrap();
    }
    ExecutionGraphEnvelope::bind(&graph, fields()).unwrap()
}

fn original() -> TaskCapsule {
    original_with(
        &["src/core/scheduler/**", "src/core/dag/**"],
        &["src/core/graph/**", "src/state/**"],
    )
}

fn original_with(allowed: &[&str], forbidden: &[&str]) -> TaskCapsule {
    let r = request("i1");
    TaskCapsule::try_new(
        "i1".into(),
        "ws-a".into(),
        None,
        1,
        TaskType::Implementation,
        r.objective,
        r.acceptance_criteria,
        None,
        SHA.into(),
        SHA.into(),
        paths(allowed),
        paths(forbidden),
        None,
        None,
        None,
        None,
        QualityFloor::Balanced,
        Vec::new(),
        None,
        r.verification_plan,
        None,
        AssuranceProfileSelector::Standard,
        None,
        None,
        None,
        "runtime-a3/i1".into(),
        "/work/i1".into(),
        None,
        r.stop_conditions,
        None,
        7,
    )
    .unwrap()
}

fn finding(id: &str, blocking: bool) -> RepairCapsuleFindingV1 {
    RepairCapsuleFindingV1::try_new(
        id.into(),
        RepairFindingSeverityV1::High,
        RepairFindingCategoryV1::Correctness,
        "deadline is not enforced".into(),
        blocking,
        Some("src/core/scheduler/dispatch_scheduler.rs".into()),
        Some(RepairFindingLineV1::try_new("42").unwrap()),
        None,
        None,
        None,
    )
    .unwrap()
}

fn repair_request() -> RepairCapsuleRequestV1 {
    let o = original();
    RepairCapsuleRequestV1 {
        repair_node_id: "i2".into(),
        original_objective: o.objective().into(),
        original_acceptance_criteria: o.acceptance_criteria().to_vec(),
        a4_findings: Some(vec![finding("f1", true), finding("f2", false)]),
        blocking_findings: vec![finding("f1", true)],
        failed_checks: None,
        repair_scope: "Enforce the deadline in the scheduler only.".into(),
        prior_attempt_summary: Some("Attempt 1 ignored the deadline.".into()),
        relevant_context_refs: None,
        allowed_write_paths: paths(&["src/core/scheduler/**"]),
        forbidden_write_paths: None,
        required_quality_floor: QualityFloor::Balanced,
        context_epoch: EPOCH.into(),
    }
}

fn repair(
    env: &ExecutionGraphEnvelope,
    writer: &crate::orchestration::CoreStateWriter,
    max: u64,
    request: RepairCapsuleRequestV1,
) -> Result<RepairCapsule, CapsuleBuildError> {
    let ownership = ownership();
    let context = CapsuleContextV1 {
        envelope: env,
        interface: None,
        state: writer.reader(),
        ownership: &ownership,
        worktree: "/work/i2",
    };
    build_repair_capsule_v1(
        &context,
        &PORT,
        &GraphResultLedgerV1::new(),
        &original(),
        max,
        request,
    )
}

#[test]
fn repair_keeps_the_original_target_and_binds_the_rejected_sha() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let env = repair_env(Some(SHA2), Rejected, "i1");
    let capsule = repair(&env, &writer, 3, repair_request()).unwrap();
    assert_eq!(capsule.task_id(), "i2");
    assert_eq!(capsule.original_task_id(), "i1");
    assert_eq!(capsule.current_sha(), SHA2);
    assert_eq!(capsule.attempt_number().as_str(), "2");
    assert_eq!(capsule.original_objective(), original().objective());
    assert_eq!(
        capsule.original_acceptance_criteria(),
        original().acceptance_criteria()
    );
    assert_eq!(capsule.context_epoch().as_str(), "7");
    assert_eq!(capsule.branch(), "runtime-a3/i2");
    // The original task's traps are retained.
    assert_eq!(
        capsule.forbidden_write_paths(),
        Some(&paths(&["src/core/graph/**", "src/state/**"])[..])
    );
}

#[test]
fn repair_changing_objective_or_criteria_or_weakening_floor_is_rejected() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let env = repair_env(Some(SHA2), Rejected, "i1");
    let mut objective = repair_request();
    objective.original_objective = "Implement a simpler scheduler.".into();
    assert_eq!(
        repair(&env, &writer, 3, objective),
        Err(CapsuleBuildError::ObjectiveChanged)
    );
    let mut criteria = repair_request();
    criteria.original_acceptance_criteria = vec![];
    assert_eq!(
        repair(&env, &writer, 3, criteria),
        Err(CapsuleBuildError::CriteriaChanged)
    );
    let mut floor = repair_request();
    floor.required_quality_floor = QualityFloor::Economy;
    assert_eq!(
        repair(&env, &writer, 3, floor),
        Err(CapsuleBuildError::Capsule(
            CapsuleError::RepairQualityFloorWeakened
        ))
    );
    let mut widened = repair_request();
    widened.allowed_write_paths = paths(&["src/core/**"]);
    assert!(matches!(
        repair(&env, &writer, 3, widened),
        Err(CapsuleBuildError::RepairScopeWidened { .. })
    ));
    let mut mislabelled = repair_request();
    mislabelled.blocking_findings = vec![finding("f2", false)];
    assert!(matches!(
        repair(&env, &writer, 3, mislabelled),
        Err(CapsuleBuildError::NonBlockingFindingListedAsBlocking { .. })
    ));
    let mut empty = repair_request();
    empty.blocking_findings = vec![];
    assert!(matches!(
        repair(&env, &writer, 3, empty),
        Err(CapsuleBuildError::Capsule(CapsuleError::EmptyField { .. }))
    ));
    let mut secret = repair_request();
    secret.prior_attempt_summary = Some("worker set PASSWORD=hunter2".into());
    assert_eq!(
        repair(&env, &writer, 3, secret),
        Err(CapsuleBuildError::CredentialShapedValue {
            field: "prior_attempt_summary"
        })
    );
}

#[test]
fn repair_over_bound_without_rejection_sha_or_lineage_is_rejected() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let rejected = repair_env(Some(SHA2), Rejected, "i1");
    assert_eq!(
        repair(&rejected, &writer, 1, repair_request()),
        Err(CapsuleBuildError::AttemptBoundExceeded)
    );
    assert_eq!(
        repair(
            &repair_env(Some(SHA2), GraphNodeState::Passed, "i1"),
            &writer,
            3,
            repair_request()
        ),
        Err(CapsuleBuildError::NoRejectedReview)
    );
    assert_eq!(
        repair(
            &repair_env(None, Rejected, "i1"),
            &writer,
            3,
            repair_request()
        ),
        Err(CapsuleBuildError::CurrentShaUnavailable)
    );
    assert_eq!(
        repair(
            &repair_env(Some(SHA2), Rejected, "other"),
            &writer,
            3,
            repair_request()
        ),
        Err(CapsuleBuildError::ParentMismatch)
    );
    let mut not_repair = repair_request();
    not_repair.repair_node_id = "r2".into();
    assert_eq!(
        repair(&rejected, &writer, 3, not_repair),
        Err(CapsuleBuildError::NotReady)
    );
    let mut stale = repair_request();
    stale.context_epoch = "6".into();
    assert_eq!(
        repair(&rejected, &writer, 3, stale),
        Err(CapsuleBuildError::StaleEpoch)
    );
}

/// Own `src/core/**`, another owner `docs/**`: nothing else is owned.
fn escape_ownership() -> Vec<WorkstreamOwnershipV1> {
    vec![
        WorkstreamOwnershipV1 {
            workstream_id: "ws-a".into(),
            owned_paths: paths(&["src/core/**"]),
        },
        WorkstreamOwnershipV1 {
            workstream_id: "ws-b".into(),
            owned_paths: paths(&["docs/**"]),
        },
    ]
}

#[test]
fn task_scope_must_be_contained_in_the_owned_paths_under_the_real_matcher() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let env = task_env();
    let ownership = escape_ownership();
    let context = CapsuleContextV1 {
        envelope: &env,
        interface: None,
        state: writer.reader(),
        ownership: &ownership,
        worktree: "/work/t",
    };
    let with = |allowed: &[&str]| {
        let mut r = request("t");
        r.allowed_write_paths = paths(allowed);
        r.forbidden_write_paths = Vec::new();
        build_task_capsule_v1(&context, &RealPort, r)
    };
    // F1: overlaps own, is disjoint from docs/**, yet reaches src/unowned/safe/x.
    assert!(matches!(
        with(&["src/*/safe/**"]),
        Err(CapsuleBuildError::OutsideWorkstream { .. })
    ));
    for escaping in ["src/co*/x.rs", "src/**/core/x.rs", "src/core*/x.rs"] {
        assert!(
            matches!(
                with(&[escaping]),
                Err(CapsuleBuildError::OutsideWorkstream { .. })
            ),
            "{escaping}"
        );
    }
    // Containment needs only one of several allowed patterns to fail.
    assert!(matches!(
        with(&["src/core/a.rs", "src/*/safe/**"]),
        Err(CapsuleBuildError::OutsideWorkstream { .. })
    ));
    // Provably contained scopes are still built.
    for inside in [
        "src/core/scheduler/**",
        "src/core/*/x.rs",
        "src/core/**/a/**",
        "src/core/x.rs",
        "src/core",
    ] {
        assert!(with(&[inside]).is_ok(), "{inside}");
    }
    // A pattern too large to decide is refused, never admitted.
    let huge = format!("src/core/{}", "a".repeat(5000));
    assert!(matches!(
        with(&[&huge]),
        Err(CapsuleBuildError::WriteScopeUndetermined { .. })
    ));
}

#[test]
fn repair_scope_must_be_contained_in_the_owned_paths_under_the_real_matcher() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let env = repair_env(Some(SHA2), Rejected, "i1");
    let ownership = escape_ownership();
    let context = CapsuleContextV1 {
        envelope: &env,
        interface: None,
        state: writer.reader(),
        ownership: &ownership,
        worktree: "/work/i2",
    };
    let build = |allowed: &[&str]| {
        let original = original_with(allowed, &[]);
        let mut r = repair_request();
        r.allowed_write_paths = paths(allowed);
        build_repair_capsule_v1(
            &context,
            &RealPort,
            &GraphResultLedgerV1::new(),
            &original,
            3,
            r,
        )
    };
    assert!(matches!(
        build(&["src/*/safe/**"]),
        Err(CapsuleBuildError::OutsideWorkstream { .. })
    ));
    assert!(build(&["src/core/scheduler/**"]).is_ok());
}
