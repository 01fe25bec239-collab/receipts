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
    repair_env_named("i1", "i2", judged_sha, r1_state, repair_parent)
}

fn repair_env_named(
    original: &str,
    repair: &str,
    judged_sha: Option<&str>,
    r1_state: GraphNodeState,
    repair_parent: &str,
) -> ExecutionGraphEnvelope {
    let mut graph = ExecutionGraph::new(GRAPH).unwrap();
    let mut i1 = node(original, GraphNodeKind::IMPLEMENTATION, Accepted);
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
            node(repair, GraphNodeKind::REPAIR, Planned)
                .with_parent_node_id(repair_parent)
                .unwrap()
                .with_attempt_number(GraphNodeAttemptNumber::try_new("2").unwrap()),
        )
        .unwrap();
    graph
        .add_node(node("r2", GraphNodeKind::REVIEW, Planned))
        .unwrap();
    for edge in [
        GraphEdge::precedence(
            "e1",
            GRAPH,
            original,
            "r1",
            PrecedenceKind::RequiresAccepted,
        ),
        GraphEdge::control("e2", GRAPH, "r1", repair, ControlKind::OnReject),
        GraphEdge::precedence("e3", GRAPH, repair, "r2", PrecedenceKind::RequiresAccepted),
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
    original_full(
        "i1",
        &r.objective,
        r.acceptance_criteria,
        allowed,
        forbidden,
    )
}

fn original_full(
    id: &str,
    objective: &str,
    criteria: Vec<Criterion>,
    allowed: &[&str],
    forbidden: &[&str],
) -> TaskCapsule {
    let r = request("i1");
    TaskCapsule::try_new(
        id.into(),
        "ws-a".into(),
        None,
        1,
        TaskType::Implementation,
        objective.into(),
        criteria,
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
        format!("runtime-a3/{id}"),
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

// --- credential screening of every emitted field, and refusal diagnostics ------
//
// Values are synthetic. Assertions never print a built capsule or a refusal:
// a failure names only the field under test.

use serde_json::json;

use crate::scheduler::{WriteScopeOverlapV1, WriteScopePort};
use crate::wire::GraphMutationWireV1;
use crate::{GraphMutationScopeV1, ScopedActorV1, apply_graph_mutation_v1};

/// Recognised by the heuristic (`ghp_` prefix, long tail).
const TOKEN: &str = "ghp_zq7Kx2mNvB9rTc4LpWs8YdEf";
/// The same shape as a capability id (`[a-z0-9_]` components).
const CAP_TOKEN: &str = "ghp_zq7kx2mnvb9rtc4lpws8ydef.core";
/// Case-insensitive marker of either value above.
const MARK: &str = "zq7kx2mnvb9r";

fn leaks(error: &CapsuleBuildError) -> bool {
    [format!("{error}"), format!("{error:?}")]
        .iter()
        .any(|text| text.to_ascii_lowercase().contains(MARK))
}

/// Why `result` is not a credential-shaped refusal with leak-free text; names
/// only `label`, never the value.
fn screening_failure<T>(result: Result<T, CapsuleBuildError>, label: &str) -> Option<String> {
    match result {
        Ok(_) => Some(format!("{label}: built")),
        Err(error) if leaks(&error) => Some(format!("{label}: refusal exposes the value")),
        Err(CapsuleBuildError::CredentialShapedValue { .. }) => None,
        Err(_) => Some(format!("{label}: not refused as credential-shaped")),
    }
}

/// A reference whose `target`/`section`/`digest` is credential-shaped only when
/// `field` is `<pos>.<part>` and `secret`.
fn ref_with(field: &str, pos: &str, secret: bool) -> Ref {
    let value = |part: &str, plain: &str| {
        if secret && field == format!("{pos}.{part}") {
            TOKEN.to_string()
        } else {
            plain.to_string()
        }
    };
    Ref::try_new(
        RefType::RepoPath,
        value("target", "docs/orchestration/README.md"),
        Some(value("digest", "sha256:00ab")),
        Some(value("section", "intro")),
    )
    .unwrap()
}

fn criterion_with(
    id: &str,
    description: &str,
    command: &str,
    rationale: Option<String>,
) -> Criterion {
    Criterion::try_new(
        id.into(),
        description.into(),
        CriterionKind::Deterministic,
        Some(paths(&["cargo", command])),
        rationale,
    )
    .unwrap()
}

fn task_graph(task: &str, dep: &str, ws: &str, capability: &str) -> ExecutionGraphEnvelope {
    let node = |id: &str, state| {
        GraphNode::new(id, GRAPH, GraphNodeKind::TASK, state)
            .unwrap()
            .with_workstream_id(ws)
            .unwrap()
    };
    let mut graph = ExecutionGraph::new(GRAPH).unwrap();
    graph.add_node(node(dep, Accepted)).unwrap();
    graph
        .add_node(
            node(task, Planned)
                .with_required_capabilities(vec![CapabilityName::new(capability).unwrap()]),
        )
        .unwrap();
    graph
        .add_edge(
            GraphEdge::precedence("e1", GRAPH, dep, task, PrecedenceKind::RequiresAccepted)
                .unwrap(),
        )
        .unwrap();
    ExecutionGraphEnvelope::bind(&graph, fields()).unwrap()
}

/// Every emitted TaskCapsule text family; see the repair evidence inventory.
const TASK_FIELDS: [&str; 36] = [
    "task_id",
    "workstream_id",
    "dependencies",
    "required_capabilities",
    "preferred_capabilities",
    "criterion_id",
    "criterion_description",
    "criterion_command",
    "criterion_rationale",
    "objective",
    "non_goals",
    "stop_conditions",
    "worktree",
    "verification_plan",
    "allowed_write_paths",
    "request_forbidden_path",
    "synthesized_forbidden_path",
    "relevant_context_refs.target",
    "relevant_context_refs.section",
    "relevant_context_refs.digest",
    "architecture_refs.target",
    "architecture_refs.section",
    "architecture_refs.digest",
    "contract_refs.target",
    "contract_refs.section",
    "contract_refs.digest",
    "handoff_schema.target",
    "handoff_schema.section",
    "handoff_schema.digest",
    // Early scope refusals must not carry the value either.
    "outside_workstream_path",
    "permissive_wildcard_path",
    "undetermined_path",
    "collision_path",
    "plain_a",
    "plain_b",
    "plain_c",
];

fn task_case(field: &str, secret: bool) -> Result<TaskCapsule, CapsuleBuildError> {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let hit = |name: &str| secret && field == name;
    let value = |name: &str, plain: &str| {
        if hit(name) {
            TOKEN.to_string()
        } else {
            plain.to_string()
        }
    };
    let task = value("task_id", "t");
    let dep = value("dependencies", "dep");
    let ws = value("workstream_id", "ws-a");
    let capability = if hit("required_capabilities") {
        CAP_TOKEN
    } else {
        "graph.core"
    };
    let env = task_graph(&task, &dep, &ws, capability);
    let ownership = vec![
        WorkstreamOwnershipV1 {
            workstream_id: ws,
            owned_paths: paths(&["src/core/**"]),
        },
        WorkstreamOwnershipV1 {
            workstream_id: "ws-b".into(),
            owned_paths: vec![value("synthesized_forbidden_path", "src/state/**")],
        },
    ];
    let worktree = value("worktree", "/work/t");
    let context = CapsuleContextV1 {
        envelope: &env,
        interface: None,
        state: writer.reader(),
        ownership: &ownership,
        worktree: &worktree,
    };
    let mut r = request(&task);
    r.objective = value("objective", "Implement the scheduler.");
    r.acceptance_criteria = vec![criterion_with(
        &value("criterion_id", "c1"),
        &value("criterion_description", "scheduler tests pass"),
        &value("criterion_command", "test"),
        Some(value("criterion_rationale", "needed")),
    )];
    r.non_goals = Some(vec![value("non_goals", "no routing")]);
    r.stop_conditions = vec![value("stop_conditions", "tests fail twice")];
    r.verification_plan = vec![
        VerificationPlanEntryV1::try_new(paths(&["cargo", &value("verification_plan", "test")]), 0)
            .unwrap(),
    ];
    r.allowed_write_paths = vec![if hit("allowed_write_paths") {
        format!("src/core/{TOKEN}")
    } else {
        "src/core/scheduler/**".to_string()
    }];
    r.forbidden_write_paths = vec![value("request_forbidden_path", "src/core/graph/**")];
    r.relevant_context_refs = Some(vec![ref_with(field, "relevant_context_refs", secret)]);
    r.architecture_refs = Some(vec![ref_with(field, "architecture_refs", secret)]);
    r.contract_refs = Some(vec![ref_with(field, "contract_refs", secret)]);
    r.handoff_schema = Some(ref_with(field, "handoff_schema", secret));
    r.preferred_capabilities = Some(vec![
        CapabilityName::new(if hit("preferred_capabilities") {
            CAP_TOKEN
        } else {
            "graph.extra"
        })
        .unwrap(),
    ]);
    // Scope refusals that would otherwise echo the supplied pattern.
    for (name, path) in [
        ("outside_workstream_path", format!("docs/{TOKEN}")),
        ("permissive_wildcard_path", format!("*/{TOKEN}")),
        ("undetermined_path", format!("src/core/{TOKEN}?")),
        ("collision_path", format!("src/core/graph/{TOKEN}")),
    ] {
        if hit(name) {
            r.allowed_write_paths = vec![path];
        }
    }
    build_task_capsule_v1(&context, &PORT, r)
}

#[test]
fn every_task_text_family_is_screened_and_plain_values_still_build() {
    let mut failures = Vec::new();
    for field in TASK_FIELDS {
        if task_case(field, false).is_err() {
            failures.push(format!("{field}: plain control refused"));
        }
        if !field.starts_with("plain_") {
            failures.extend(screening_failure(task_case(field, true), field));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every emitted RepairCapsule text family, including inherited values.
const REPAIR_FIELDS: [&str; 41] = [
    "task_id",
    "original_task_id",
    "parent_attempt_id",
    "original_objective",
    "original_criterion_id",
    "original_criterion_description",
    "original_criterion_command",
    "original_criterion_rationale",
    "a4_finding_id",
    "blocking_finding_id",
    "finding_description",
    "finding_path",
    "repair_scope",
    "prior_attempt_summary",
    "failed_check_command",
    "worktree",
    "allowed_write_paths",
    "original_forbidden_path",
    "request_forbidden_path",
    "synthesized_forbidden_path",
    "relevant_context_refs.target",
    "relevant_context_refs.section",
    "relevant_context_refs.digest",
    "finding_evidence.target",
    "finding_evidence.section",
    "finding_evidence.digest",
    "a4_finding_evidence.target",
    "a4_finding_evidence.section",
    "a4_finding_evidence.digest",
    "failed_check_output.target",
    "failed_check_output.section",
    "failed_check_output.digest",
    // Early refusals must not carry the value either.
    "widened_path",
    "non_blocking_finding_id",
    "plain_a",
    "plain_b",
    "plain_c",
    "plain_d",
    "plain_e",
    "plain_f",
    "plain_g",
];

/// The real ledger holding an admitted attempt `attempt` of node `i1`.
fn ledger_with_attempt(attempt: &str) -> GraphResultLedgerV1 {
    let env = envelope(&[("i1", "IMPLEMENTATION", Planned, Some("ws-a"))], &[]);
    let step = |from: &str, to: &str, cause: serde_json::Value| json!({"op":"SET_NODE_STATE","node_id":"i1","detail":{"from":from,"to":to,"cause":cause}});
    let text = json!({
        "mutation_id": "m", "graph_id": GRAPH, "parent_version": 1, "resulting_version": 2,
        "actor": {"role":"RUNTIME_A2","role_id":"a2"}, "reason": "test",
        "created_at": "2026-10-29T01:02:03Z",
        "operations": [
            step("PLANNED","READY",json!({"kind":"DEPENDENCIES_SATISFIED"})),
            step("READY","ADMITTED",json!({"kind":"ADMISSION","admission_decision_id":"ad"})),
            step("ADMITTED","DISPATCHED",json!({"kind":"DISPATCH","attempt_id":attempt})),
        ],
    });
    let mutation = GraphMutationWireV1::from_json(&text.to_string())
        .unwrap()
        .to_mutation()
        .unwrap();
    let actor = ScopedActorV1::RuntimeA2 {
        role_id: Some("a2".into()),
        workstream_id: "ws-a".into(),
    };
    let scope = GraphMutationScopeV1::new(
        env.project_id(),
        env.graph_id(),
        env.graph_version().clone(),
        actor,
    );
    apply_graph_mutation_v1(&env, &GraphResultLedgerV1::new(), &mutation, &scope, None)
        .unwrap()
        .results
}

fn finding_with(id: &str, blocking: bool, evidence: Option<Ref>) -> RepairCapsuleFindingV1 {
    RepairCapsuleFindingV1::try_new(
        id.into(),
        RepairFindingSeverityV1::High,
        RepairFindingCategoryV1::Correctness,
        "deadline is not enforced".into(),
        blocking,
        Some("src/core/scheduler/dispatch_scheduler.rs".into()),
        Some(RepairFindingLineV1::try_new("42").unwrap()),
        evidence,
        None,
        None,
    )
    .unwrap()
}

fn repair_case(field: &str, secret: bool) -> Result<RepairCapsule, CapsuleBuildError> {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let hit = |name: &str| secret && field == name;
    let value = |name: &str, plain: &str| {
        if hit(name) {
            TOKEN.to_string()
        } else {
            plain.to_string()
        }
    };
    let original_id = value("original_task_id", "i1");
    let repair_id = value("task_id", "i2");
    let env = repair_env_named(&original_id, &repair_id, Some(SHA2), Rejected, &original_id);
    let ledger = if hit("parent_attempt_id") {
        // Same project/graph/node key as `env`; the attempt id is the value.
        ledger_with_attempt(TOKEN)
    } else {
        GraphResultLedgerV1::new()
    };
    let ownership = vec![
        WorkstreamOwnershipV1 {
            workstream_id: "ws-a".into(),
            owned_paths: paths(&["src/core/**"]),
        },
        WorkstreamOwnershipV1 {
            workstream_id: "ws-b".into(),
            owned_paths: vec![value("synthesized_forbidden_path", "src/state/**")],
        },
    ];
    let worktree = value("worktree", "/work/i2");
    let context = CapsuleContextV1 {
        envelope: &env,
        interface: None,
        state: writer.reader(),
        ownership: &ownership,
        worktree: &worktree,
    };
    let allowed = if hit("allowed_write_paths") {
        format!("src/core/{TOKEN}")
    } else {
        "src/core/scheduler/**".to_string()
    };
    let criteria = vec![criterion_with(
        &value("original_criterion_id", "c1"),
        &value("original_criterion_description", "scheduler tests pass"),
        &value("original_criterion_command", "test"),
        Some(value("original_criterion_rationale", "needed")),
    )];
    // The original is a public physical type: nothing says its builder ran.
    let original = original_full(
        &original_id,
        &value("original_objective", "Implement the scheduler."),
        criteria,
        &[&allowed],
        &[
            "src/core/graph/**",
            &value("original_forbidden_path", "src/state/**"),
        ],
    );
    let evidence = |pos: &str| Some(ref_with(field, pos, secret));
    let mut blocking = finding_with(
        &value("blocking_finding_id", "f1"),
        true,
        evidence("finding_evidence"),
    );
    if hit("finding_description") || hit("finding_path") {
        blocking = RepairCapsuleFindingV1::try_new(
            "f1".into(),
            RepairFindingSeverityV1::High,
            RepairFindingCategoryV1::Correctness,
            value("finding_description", "deadline is not enforced"),
            true,
            Some(value("finding_path", "src/core/scheduler/x.rs")),
            None,
            None,
            None,
            None,
        )
        .unwrap();
    }
    let a4 = vec![
        blocking.clone(),
        finding_with(
            &value("a4_finding_id", "f2"),
            false,
            evidence("a4_finding_evidence"),
        ),
    ];
    let failed = RepairCapsuleFailedCheckV1::try_new(
        RepairCheckSourceV1::WorkerExecution,
        paths(&["cargo", &value("failed_check_command", "test")]),
        RepairExitCodeV1::try_new("1").unwrap(),
        SHA.into(),
        None,
        None,
        None,
        evidence("failed_check_output"),
        None,
    )
    .unwrap();
    let mut r = repair_request();
    r.repair_node_id = repair_id;
    r.original_objective = original.objective().into();
    r.original_acceptance_criteria = original.acceptance_criteria().to_vec();
    r.a4_findings = Some(a4);
    r.blocking_findings = vec![blocking];
    r.failed_checks = Some(vec![failed]);
    r.repair_scope = value(
        "repair_scope",
        "Enforce the deadline in the scheduler only.",
    );
    r.prior_attempt_summary = Some(value("prior_attempt_summary", "Attempt 1 ignored it."));
    r.relevant_context_refs = Some(vec![ref_with(field, "relevant_context_refs", secret)]);
    r.allowed_write_paths = vec![allowed];
    r.forbidden_write_paths = Some(vec![value("request_forbidden_path", "src/other/**")]);
    if hit("widened_path") {
        r.allowed_write_paths = vec![format!("docs/{TOKEN}")];
    }
    if hit("non_blocking_finding_id") {
        r.blocking_findings = vec![finding_with(TOKEN, false, None)];
    }
    build_repair_capsule_v1(&context, &PORT, &ledger, &original, 3, r)
}

#[test]
fn every_repair_text_family_is_screened_and_plain_values_still_build() {
    let mut failures = Vec::new();
    for field in REPAIR_FIELDS {
        if repair_case(field, false).is_err() {
            failures.push(format!("{field}: plain control refused"));
        }
        if !field.starts_with("plain_") {
            failures.extend(screening_failure(repair_case(field, true), field));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Reports the matcher-produced witness without trusting it.
struct LeakyPort;

impl WriteScopePort for LeakyPort {
    fn overlap(&self, _: &[String], _: &[String]) -> WriteScopeOverlapV1 {
        WriteScopeOverlapV1::Overlaps {
            witness: format!("src/core/{TOKEN}"),
        }
    }
}

#[test]
fn a_matcher_witness_cannot_carry_a_credential_into_a_refusal() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let env = task_env();
    let ownership = ownership();
    let context = CapsuleContextV1 {
        envelope: &env,
        interface: None,
        state: writer.reader(),
        ownership: &ownership,
        worktree: "/work/t",
    };
    match build_task_capsule_v1(&context, &LeakyPort, request("t")) {
        Err(error @ CapsuleBuildError::WriteScopeCollision { .. }) => {
            assert!(!leaks(&error), "task witness exposed");
        }
        _ => panic!("task: expected a collision refusal"),
    }
    let env = repair_env(Some(SHA2), Rejected, "i1");
    let context = CapsuleContextV1 {
        envelope: &env,
        worktree: "/work/i2",
        ..context
    };
    match build_repair_capsule_v1(
        &context,
        &LeakyPort,
        &GraphResultLedgerV1::new(),
        &original(),
        3,
        repair_request(),
    ) {
        Err(error @ CapsuleBuildError::WriteScopeCollision { .. }) => {
            assert!(!leaks(&error), "repair witness exposed");
        }
        _ => panic!("repair: expected a collision refusal"),
    }
}

#[test]
fn ordinary_scope_refusals_keep_their_typed_payloads() {
    let dir = Dir::new();
    let writer = writer_with_epochs(&dir, "policy", &["7"]);
    let env = task_env();
    let with = |allowed: &[&str]| {
        let mut r = request("t");
        r.allowed_write_paths = paths(allowed);
        build(&env, &writer, r)
    };
    assert_eq!(
        with(&["docs/**"]),
        Err(CapsuleBuildError::OutsideWorkstream {
            pattern: "docs/**".into()
        })
    );
    assert_eq!(
        with(&["src/core/graph/lib.rs"]),
        Err(CapsuleBuildError::WriteScopeCollision {
            pattern: "src/core/graph/lib.rs".into(),
            witness: "src/core/graph/lib.rs".into()
        })
    );
    let shown = with(&["**"]).unwrap_err().to_string();
    assert!(shown.contains("PermissiveWildcard") && shown.contains("**"));
    let rejected = repair_env(Some(SHA2), Rejected, "i1");
    let mut widened = repair_request();
    widened.allowed_write_paths = paths(&["src/core/**"]);
    assert_eq!(
        repair(&rejected, &writer, 3, widened),
        Err(CapsuleBuildError::RepairScopeWidened {
            pattern: "src/core/**".into()
        })
    );
    let mut mislabelled = repair_request();
    mislabelled.blocking_findings = vec![finding("f2", false)];
    assert_eq!(
        repair(&rejected, &writer, 3, mislabelled),
        Err(CapsuleBuildError::NonBlockingFindingListedAsBlocking {
            finding_id: "f2".into()
        })
    );
}
