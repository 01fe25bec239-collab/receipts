//! Nine-check gate controls over real repositories, real Workspace
//! worktrees/temporary merges, the real argv runner and a real State store.
//! Reviews come from the controller with a scripted (synthetic) reviewer.

use crate::acceptance_gate::*;
use crate::controller::{AttemptFinished, ReviewController, ReviewDisposition};
use crate::ports::ImplementerIdentity;
use crate::reproduction::CheckRunnerConfig;
use crate::test_repo::*;
use crate::{
    A3HandoffCheck, A3HandoffCheckResult, A3HandoffNonTemporalCore, A4ReviewNonTemporalCore,
    AssuranceProfile, IntegrationDecisionNullableString, IntegrationDecisionOutcome,
    IntegrationRequestA4Verdict, IntegrationRequestAttestations, IntegrationRequestGateLevel,
    IntegrationRequestNonTemporalCore, IntegrationRequestTask, ReviewCapsuleCheckResult,
    ReviewCapsuleNonTemporalCore, ReviewRequest,
};
use receipts_state::{
    DecisionRecordKind, DecisionRecordV1, EventType, ProvenanceLinkKind, StateReader,
    StateWriteOperationV1, StateWriteSubjectV1, TrustedCoreStore,
};
use receipts_workspace_execution::{
    WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
};
use std::path::PathBuf;
use std::time::Duration;

struct Scenario {
    repo: Repo,
    task_wt: PathBuf,
    implementation: String,
    request: ReviewRequest,
    capsule: ReviewCapsuleNonTemporalCore,
    handoff: A3HandoffNonTemporalCore,
    review: A4ReviewNonTemporalCore,
    ireq: IntegrationRequestNonTemporalCore,
    scratch: TempDir,
}

fn handoff(base: &str, implementation: &str, timed_out: bool) -> A3HandoffNonTemporalCore {
    let check = A3HandoffCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            argv(&["true"]),
            0,
            sha(implementation),
            Some(timed_out),
            None,
        )
        .unwrap(),
        Some(A3HandoffCheckResult::Pass),
    );
    A3HandoffNonTemporalCore::new(
        "task-1".into(),
        "attempt-1".into(),
        base.into(),
        implementation.into(),
        "task/1".into(),
        vec!["src/status.txt".into()],
        vec![check],
        true,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap()
}

fn ireq(implementation: &str) -> IntegrationRequestNonTemporalCore {
    IntegrationRequestNonTemporalCore::new(
        "ireq-1".into(),
        IntegrationRequestGateLevel::A2Acceptance,
        "task/1".into(),
        implementation.into(),
        "main".into(),
        vec![
            IntegrationRequestTask::new(
                "task-1".into(),
                implementation.into(),
                implementation.into(),
                IntegrationRequestA4Verdict::Pass,
                None,
                IntegrationDecisionNullableString::Omitted,
            )
            .unwrap(),
        ],
        None,
        None,
        None,
        None,
        // Every attestation is false: the gate must never read them.
        Some(IntegrationRequestAttestations::new(
            Some(false),
            Some(false),
            Some(false),
            Some(false),
            Some(false),
            Some(false),
        )),
    )
    .unwrap()
}

/// Task worktree on `task/1` with one commit; main stays at base.
fn scenario_with(
    profile: AssuranceProfile,
    verdict: &'static str,
    blocking: &'static [&'static str],
) -> Scenario {
    let repo = Repo::new();
    let task_wt = repo.scratch("task-wt");
    git(
        &repo.root(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task/1",
            task_wt.to_str().unwrap(),
            &repo.base,
        ],
    );
    std::fs::write(task_wt.join("src/status.txt"), "ok\n").unwrap();
    git(&task_wt, &["add", "-A"]);
    git(&task_wt, &["commit", "-q", "-m", "impl"]);
    let implementation = git(&task_wt, &["rev-parse", "HEAD"]);
    let request = request(&repo.base, &implementation, profile, None);
    let checks = vec![worker_check(
        &["true"],
        &implementation,
        ReviewCapsuleCheckResult::Pass,
    )];
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = ScriptedReviewer::new("runtime-b", move |cap| {
        Ok(output(&verdict_json(cap, verdict, blocking)))
    });
    let runner = runner(time_limit());
    let controller = ReviewController {
        routing: &routing,
        runtimes: &[&reviewer],
        checks: &runner,
        clock: &clock,
    };
    let ticket = controller.on_attempt_finished(
        AttemptFinished {
            project_id: "project",
            request: &request,
            repository_root: &repo.root(),
            implementer: &ImplementerIdentity {
                provider_id: "provider-a".into(),
                runtime_id: "runtime-a".into(),
                model_id: None,
                session_ref: None,
            },
            worker_checks: &checks,
            review_worktree: &repo.scratch("review-wt"),
        },
        &mut MemorySink::default(),
    );
    let ReviewDisposition::Reviewed(review) = &ticket.disposition else {
        panic!("setup review failed: {:?}", ticket.disposition);
    };
    Scenario {
        handoff: handoff(&repo.base, &implementation, false),
        review: review.core().clone(),
        capsule: ticket.capsule.clone().unwrap(),
        ireq: ireq(&implementation),
        request,
        implementation,
        task_wt,
        repo,
        scratch: TempDir::new("gate-scratch"),
    }
}

fn scenario() -> Scenario {
    scenario_with(AssuranceProfile::Standard, "PASS", &[])
}

struct Knobs<'a> {
    floor: AssuranceProfile,
    forbidden: Vec<String>,
    dependencies: Vec<String>,
    state: Option<&'a StateReader>,
    runner: CheckRunnerConfig,
    suite: Vec<String>,
    handoffs: Vec<A3HandoffNonTemporalCore>,
    reviews: Vec<A4ReviewNonTemporalCore>,
    repairs: usize,
}

fn knobs<'a>(s: &Scenario) -> Knobs<'a> {
    Knobs {
        floor: AssuranceProfile::Light,
        forbidden: vec![],
        dependencies: vec![],
        state: None,
        runner: runner(time_limit()),
        suite: argv(&["grep", "-q", "ok", "src/status.txt"]),
        handoffs: vec![s.handoff.clone()],
        reviews: vec![s.review.clone()],
        repairs: 0,
    }
}

fn evaluate(s: &Scenario, k: &Knobs<'_>) -> GateOutcome {
    AcceptanceGate::evaluate(&AcceptanceInput {
        project_id: "project",
        integration_request: &s.ireq,
        review_request: &s.request,
        profile_floor: k.floor,
        capsule: &s.capsule,
        handoffs: &k.handoffs,
        reviews: &k.reviews,
        completed_repair_attempts: k.repairs,
        repository_root: &s.repo.root(),
        task_worktree: &s.task_wt,
        merge_target_worktree: &s.repo.root(),
        forbidden_write_paths: &k.forbidden,
        dependencies: &k.dependencies,
        state: k.state,
        checks: &k.runner,
        component_suite: &k.suite,
        scratch: s.scratch.path(),
        clock: &clock,
    })
}

fn only_failed(o: &GateOutcome, id: AcceptanceCheckId, outcome: IntegrationDecisionOutcome) {
    assert_eq!(o.failed(), [id], "{:#?}", o.checks);
    assert_eq!(o.outcome, outcome, "{:#?}", o.checks);
}

fn refs(s: &Scenario) -> (String, String, String) {
    (
        git(
            &s.repo.root(),
            &["for-each-ref", "--format=%(refname) %(objectname)"],
        ),
        git(&s.repo.root(), &["worktree", "list", "--porcelain"]),
        git(
            &s.task_wt,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        ),
    )
}

#[test]
fn gate_accepts_complete_exact_evidence_and_has_no_write_capability() {
    let s = scenario();
    let before = refs(&s);
    let o = evaluate(&s, &knobs(&s));
    assert_eq!(
        o.outcome,
        IntegrationDecisionOutcome::Accept,
        "{:#?}",
        o.checks
    );
    assert_eq!(o.checks.len(), 9);
    assert!(o.failed().is_empty());
    // No ref, worktree or task checkout changed (temporary worktrees owned
    // and removed; the gate edits and merges nothing).
    assert_eq!(refs(&s), before);
    let d = o.decision.core();
    assert_eq!(d.checks_evaluated().len(), 9);
    let entry = &d.provenance_chain().unwrap()[0];
    assert_eq!(
        entry.acceptance_sha(),
        &IntegrationDecisionNullableString::String(s.implementation.clone())
    );
    let p = o.proposal.unwrap();
    assert_eq!(
        p.decision_records[0].kind(),
        DecisionRecordKind::IntegrationDecision
    );
    assert_eq!(p.events[0].event_type(), EventType::AcceptanceEvaluated);
}

#[test]
fn check1_review_sha_must_equal_implementation_sha() {
    let s = scenario();
    let mut s2 = s;
    s2.ireq = ireq(&s2.repo.base);
    only_failed(
        &evaluate(&s2, &knobs(&s2)),
        AcceptanceCheckId::ExactReviewedHead,
        IntegrationDecisionOutcome::Blocked,
    );
}

#[test]
fn check2_no_commit_after_review() {
    let s = scenario();
    std::fs::write(s.task_wt.join("src/later.txt"), "x\n").unwrap();
    let o = evaluate(&s, &knobs(&s));
    only_failed(
        &o,
        AcceptanceCheckId::NoCommitAfterReview,
        IntegrationDecisionOutcome::Blocked,
    );
    assert!(o.checks[1].detail.contains("uncommitted"));
}

#[test]
fn commit_after_review_blocks_acceptance() {
    let s = scenario();
    git(&s.task_wt, &["add", "-A"]);
    std::fs::write(s.task_wt.join("src/later.txt"), "x\n").unwrap();
    git(&s.task_wt, &["add", "-A"]);
    git(&s.task_wt, &["commit", "-q", "-m", "after review"]);
    let o = evaluate(&s, &knobs(&s));
    assert!(o.failed().contains(&AcceptanceCheckId::NoCommitAfterReview));
    assert!(
        o.checks[1].detail.contains("1 commit(s) after review"),
        "{}",
        o.checks[1].detail
    );
    assert_ne!(o.outcome, IntegrationDecisionOutcome::Accept);
    assert_eq!(o.outcome, IntegrationDecisionOutcome::Blocked);
}

#[test]
fn check3_criteria_rerun_through_the_runner() {
    let s = scenario();
    let mut k = knobs(&s);
    // The gate's own runner cannot run grep: the criterion cannot pass.
    k.runner.executables.retain(|(n, _)| n != "grep");
    k.suite = argv(&["true"]);
    only_failed(
        &evaluate(&s, &k),
        AcceptanceCheckId::CriteriaReproduced,
        IntegrationDecisionOutcome::Repair,
    );
}

#[test]
fn check4_reject_verdict_needs_repair() {
    let s = scenario_with(AssuranceProfile::Standard, "REJECT", &["CORRECTNESS"]);
    let o = evaluate(&s, &knobs(&s));
    assert!(o.failed().contains(&AcceptanceCheckId::VerdictAndFindings));
    assert_eq!(
        o.outcome,
        IntegrationDecisionOutcome::Repair,
        "{:#?}",
        o.checks
    );
}

#[test]
fn check5_required_checks_at_sha() {
    let s = scenario();
    let mut k = knobs(&s);
    k.handoffs = vec![handoff(&s.repo.base, &s.repo.base, false)];
    let o = evaluate(&s, &k);
    assert!(o.failed().contains(&AcceptanceCheckId::RequiredChecks));
    assert_eq!(o.outcome, IntegrationDecisionOutcome::Blocked);
}

#[test]
fn pass_plus_timeout_not_accepted() {
    let s = scenario();
    let mut k = knobs(&s);
    k.handoffs = vec![handoff(&s.repo.base, &s.implementation, true)];
    let o = evaluate(&s, &k);
    assert!(
        o.failed().contains(&AcceptanceCheckId::RequiredChecks),
        "{:#?}",
        o.checks
    );
    assert_ne!(o.outcome, IntegrationDecisionOutcome::Accept);
    // Post-merge suite timeout is not accepted either.
    let mut k = knobs(&s);
    k.runner = runner(Duration::from_millis(300));
    k.suite = argv(&["sleep", "5"]);
    let o = evaluate(&s, &k);
    assert!(o.failed().contains(&AcceptanceCheckId::PostMergeTests));
    assert!(o.checks[8].detail.contains("timed out"));
}

#[test]
fn profile_floor_cannot_be_lowered() {
    // STANDARD request, HIGH_ASSURANCE floor: raised, and HIGH's broker
    // re-execution is not available → HUMAN_REQUIRED, never ACCEPT.
    let s = scenario();
    let mut k = knobs(&s);
    k.floor = AssuranceProfile::HighAssurance;
    let o = evaluate(&s, &k);
    only_failed(
        &o,
        AcceptanceCheckId::RequiredChecks,
        IntegrationDecisionOutcome::HumanRequired,
    );
    // A LIGHT floor never lowers the request's STANDARD requirements: with
    // reproduction evidence removed the STANDARD requirement still fails.
    let mut k = knobs(&s);
    k.floor = AssuranceProfile::Light;
    let r = &s.review;
    k.reviews = vec![
        A4ReviewNonTemporalCore::new(
            r.review_id().into(),
            r.task_id().into(),
            r.reviewed_sha().into(),
            r.verdict(),
            true,
            vec![],
            vec![],
            r.dimensions().to_vec(),
            r.reviewer().cloned(),
            None,
            Some(vec![]),
            None,
            None,
        )
        .unwrap(),
    ];
    let o = evaluate(&s, &k);
    assert!(
        o.failed().contains(&AcceptanceCheckId::RequiredChecks),
        "{:#?}",
        o.checks
    );
}

#[test]
fn check6_write_scope() {
    let s = scenario();
    let mut k = knobs(&s);
    k.forbidden = vec!["src/status.txt".into()];
    only_failed(
        &evaluate(&s, &k),
        AcceptanceCheckId::WriteScope,
        IntegrationDecisionOutcome::Repair,
    );
}

fn state_with_dependency(dir: &TempDir, dep_sha: &str, accepted: bool) -> StateReader {
    use std::os::unix::fs::PermissionsExt;
    let state_dir = dir.path().join("state");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let db = state_dir.join("state.db");
    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    let admit = |store: &TrustedCoreStore, op| {
        store.admit(StateWriteSubjectV1 {
            operation: op,
            project_id: "project".into(),
            graph_id: None,
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        })
    };
    store
        .append_provenance_link(
            admit(&store, StateWriteOperationV1::ProvenanceLink),
            "project",
            "dep-1",
            ProvenanceLinkKind::ImplementationSha,
            dep_sha,
        )
        .unwrap();
    if accepted {
        let json = format!(
            r#"{{"decision_id":"dep-decision","request_id":"dep-req","outcome":"ACCEPT","checks_evaluated":[{{"check":"c","result":"PASS"}}],"provenance_chain":[{{"task_id":"dep-1","start_sha":"{dep_sha}","implementation_sha":"{dep_sha}","review_sha":"{dep_sha}"}}],"decided_at":"2026-11-08T00:00:00Z"}}"#
        );
        let record = DecisionRecordV1::parse(
            "project",
            DecisionRecordKind::IntegrationDecision,
            "dep-decision",
            &json,
        )
        .unwrap();
        store
            .append_decision_record(
                admit(&store, StateWriteOperationV1::DecisionRecord),
                &record,
            )
            .unwrap();
        store
            .append_provenance_link(
                admit(&store, StateWriteOperationV1::ProvenanceLink),
                "project",
                "dep-1",
                ProvenanceLinkKind::AcceptanceRef,
                "dep-decision",
            )
            .unwrap();
    }
    drop(store);
    StateReader::open_existing(&db).unwrap()
}

#[test]
fn check7_dependencies_accepted_and_not_reverted() {
    let s = scenario();
    let dir = TempDir::new("state");
    // Accepted dependency whose implementation is in the candidate.
    let reader = state_with_dependency(&dir, &s.repo.base, true);
    let mut k = knobs(&s);
    k.dependencies = vec!["dep-1".into()];
    k.state = Some(&reader);
    assert_eq!(evaluate(&s, &k).outcome, IntegrationDecisionOutcome::Accept);
    // Not accepted.
    let dir2 = TempDir::new("state");
    let reader2 = state_with_dependency(&dir2, &s.repo.base, false);
    k.state = Some(&reader2);
    only_failed(
        &evaluate(&s, &k),
        AcceptanceCheckId::Dependencies,
        IntegrationDecisionOutcome::Blocked,
    );
    // Accepted, but its implementation is not in the candidate (reverted).
    let elsewhere = {
        let other = s.repo.scratch("other-wt");
        git(
            &s.repo.root(),
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "other",
                other.to_str().unwrap(),
                &s.repo.base,
            ],
        );
        std::fs::write(other.join("o.txt"), "o\n").unwrap();
        git(&other, &["add", "-A"]);
        git(&other, &["commit", "-q", "-m", "dep"]);
        git(&other, &["rev-parse", "HEAD"])
    };
    let dir3 = TempDir::new("state");
    let reader3 = state_with_dependency(&dir3, &elsewhere, true);
    k.state = Some(&reader3);
    let o = evaluate(&s, &k);
    only_failed(
        &o,
        AcceptanceCheckId::Dependencies,
        IntegrationDecisionOutcome::Blocked,
    );
    assert!(o.checks[6].detail.contains("reverted"));
    // Missing reader with dependencies is never a pass.
    k.state = None;
    assert!(
        evaluate(&s, &k)
            .failed()
            .contains(&AcceptanceCheckId::Dependencies)
    );
}

#[test]
fn check8_freshness_against_merge_target() {
    let mut s = scenario();
    s.repo.commit(&[("README.md", "moved\n")], "target moved");
    let o = evaluate(&s, &knobs(&s));
    only_failed(
        &o,
        AcceptanceCheckId::Freshness,
        IntegrationDecisionOutcome::Repair,
    );
}

#[test]
fn check9_post_merge_tree_tests() {
    let s = scenario();
    let mut k = knobs(&s);
    k.suite = argv(&["false"]);
    only_failed(
        &evaluate(&s, &k),
        AcceptanceCheckId::PostMergeTests,
        IntegrationDecisionOutcome::Repair,
    );
    // A conflicting target refuses the merge and fails check 9.
    let mut s = scenario();
    s.repo
        .commit(&[("src/status.txt", "conflict\n")], "conflict");
    let o = evaluate(&s, &knobs(&s));
    assert!(o.failed().contains(&AcceptanceCheckId::PostMergeTests));
    assert!(
        o.checks[8].detail.contains("unavailable"),
        "{}",
        o.checks[8].detail
    );
}

#[test]
fn bound_exhausted_rejection_is_human_required() {
    let s = scenario_with(AssuranceProfile::Standard, "REJECT", &["CORRECTNESS"]);
    let mut k = knobs(&s);
    k.handoffs = vec![s.handoff.clone(); 4];
    k.reviews = vec![s.review.clone(); 4];
    k.repairs = 3;
    let o = evaluate(&s, &k);
    assert_eq!(
        o.outcome,
        IntegrationDecisionOutcome::HumanRequired,
        "{:#?}",
        o.checks
    );
}

#[test]
fn gate_has_no_write_capability() {
    // Even a failing evaluation that provisions criteria and post-merge
    // worktrees leaves every ref, worktree registration, the task checkout
    // and the scratch directory exactly as before.
    let s = scenario();
    let before = refs(&s);
    let mut k = knobs(&s);
    k.suite = argv(&["false"]);
    let o = evaluate(&s, &k);
    assert_eq!(o.outcome, IntegrationDecisionOutcome::Repair);
    assert_eq!(refs(&s), before);
    assert_eq!(std::fs::read_dir(s.scratch.path()).unwrap().count(), 0);
    assert_eq!(git(&s.task_wt, &["rev-parse", "HEAD"]), s.implementation);
}
