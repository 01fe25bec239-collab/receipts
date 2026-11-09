//! Nine-check gate controls over real repositories, real Workspace
//! worktrees/temporary merges, the real argv runner and a real State store.
//! Reviews come from the controller with a scripted (synthetic) reviewer.

use crate::acceptance_gate::*;
use crate::controller::{AttemptFinished, ReviewController, ReviewDisposition};
use crate::dispositions::FindingDisposition;
use crate::ports::ImplementerIdentity;
use crate::ports::ReviewRecordProposal;
use crate::reproduction::CheckRunnerConfig;
use crate::test_repo::*;
use crate::{
    A3HandoffCheck, A3HandoffCheckResult, A3HandoffNonTemporalCore, A4ReviewNonTemporalCore,
    A4ReviewReproduction, A4ReviewReproductionCheck, A4ReviewReproductionLimitation,
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
    /// The package the controller produced for `review`, as the trusted core
    /// persisted it (decision record and REVIEW_DISPATCHED).
    package: ReviewRecordProposal,
    /// Keeps the State store's directory alive for the scenario.
    _state_dir: TempDir,
    state: StateReader,
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
    scenario_prepared(profile, verdict, blocking, |_| {})
}

/// `prepare` may add commits to `main` before the task branch is cut; the
/// request baseline is then main's resulting head.
fn scenario_prepared(
    profile: AssuranceProfile,
    verdict: &'static str,
    blocking: &'static [&'static str],
    prepare: impl FnOnce(&mut Repo),
) -> Scenario {
    scenario_json(profile, prepare, move |cap| {
        verdict_json(cap, verdict, blocking)
    })
}

/// As [`scenario_prepared`] with the reviewer's exact answer supplied.
fn scenario_json(
    profile: AssuranceProfile,
    prepare: impl FnOnce(&mut Repo),
    json: impl Fn(&ReviewCapsuleNonTemporalCore) -> String + 'static,
) -> Scenario {
    let mut repo = Repo::new();
    prepare(&mut repo);
    repo.base = git(&repo.root(), &["rev-parse", "HEAD"]);
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
    let reviewer = ScriptedReviewer::new("runtime-b", move |cap| Ok(output(&json(cap))));
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
            generation: 1,
        },
        &mut MemorySink::default(),
    );
    let ReviewDisposition::Reviewed(review) = &ticket.disposition else {
        panic!("setup review failed: {:?}", ticket.disposition);
    };
    let package = ticket.proposal.clone().expect("a sealed package");
    let state_dir = TempDir::new("gate-state");
    let state = state_with_packages(&state_dir, &[&package], |_| {});
    Scenario {
        package,
        _state_dir: state_dir,
        state,
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
    supersessions: Vec<DependencySupersession>,
    dispositions: Vec<FindingDisposition>,
}

fn knobs<'a>(s: &'a Scenario) -> Knobs<'a> {
    Knobs {
        floor: AssuranceProfile::Light,
        forbidden: vec![],
        dependencies: vec![],
        state: Some(&s.state),
        runner: runner(time_limit()),
        suite: argv(&["grep", "-q", "ok", "src/status.txt"]),
        handoffs: vec![s.handoff.clone()],
        reviews: vec![s.review.clone()],
        repairs: 0,
        supersessions: vec![],
        dispositions: vec![],
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
        supersessions: &k.supersessions,
        finding_dispositions: &k.dispositions,
        checks: &k.runner,
        component_suite: &k.suite,
        scratch: s.scratch.path(),
        clock: &clock,
        generation: 1,
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

fn state_with_dependency(
    s: &Scenario,
    dir: &TempDir,
    dep_sha: &str,
    accepted: bool,
) -> StateReader {
    state_with_dependency_range(s, dir, dep_sha, dep_sha, accepted)
}

/// Appends `task` as implemented over `start..implementation` and, when
/// `accepted`, with an ACCEPT IntegrationDecision, to `store`.
fn record_task(
    store: &mut TrustedCoreStore,
    task: &str,
    start: &str,
    implementation: &str,
    accepted: bool,
) {
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
    for (kind, value) in [
        (ProvenanceLinkKind::ImplementationSha, implementation),
        (ProvenanceLinkKind::StartSha, start),
    ] {
        store
            .append_provenance_link(
                admit(store, StateWriteOperationV1::ProvenanceLink),
                "project",
                task,
                kind,
                value,
            )
            .unwrap();
    }
    if accepted {
        let decision_id = format!("{task}-decision");
        let json = format!(
            r#"{{"decision_id":"{decision_id}","request_id":"{task}-req","outcome":"ACCEPT","checks_evaluated":[{{"check":"c","result":"PASS"}}],"provenance_chain":[{{"task_id":"{task}","start_sha":"{start}","implementation_sha":"{implementation}","review_sha":"{implementation}"}}],"decided_at":"2026-11-08T00:00:00Z"}}"#
        );
        let record = DecisionRecordV1::parse(
            "project",
            DecisionRecordKind::IntegrationDecision,
            &decision_id,
            &json,
        )
        .unwrap();
        store
            .append_decision_record(admit(store, StateWriteOperationV1::DecisionRecord), &record)
            .unwrap();
        store
            .append_provenance_link(
                admit(store, StateWriteOperationV1::ProvenanceLink),
                "project",
                task,
                ProvenanceLinkKind::AcceptanceRef,
                &decision_id,
            )
            .unwrap();
    }
}

/// The scenario's persisted review package plus `dep-1` implemented over
/// `start_sha..dep_sha`.
fn state_with_dependency_range(
    s: &Scenario,
    dir: &TempDir,
    start_sha: &str,
    dep_sha: &str,
    accepted: bool,
) -> StateReader {
    state_with_packages(dir, &[&s.package], |store| {
        record_task(store, "dep-1", start_sha, dep_sha, accepted);
    })
}

#[test]
fn check7_dependencies_accepted_and_not_reverted() {
    let s = scenario();
    let dir = TempDir::new("state");
    // Accepted dependency whose implementation is in the candidate.
    let reader = state_with_dependency(&s, &dir, &s.repo.base, true);
    let mut k = knobs(&s);
    k.dependencies = vec!["dep-1".into()];
    k.state = Some(&reader);
    assert_eq!(evaluate(&s, &k).outcome, IntegrationDecisionOutcome::Accept);
    // Not accepted.
    let dir2 = TempDir::new("state");
    let reader2 = state_with_dependency(&s, &dir2, &s.repo.base, false);
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
    let reader3 = state_with_dependency(&s, &dir3, &elsewhere, true);
    k.state = Some(&reader3);
    let o = evaluate(&s, &k);
    only_failed(
        &o,
        AcceptanceCheckId::Dependencies,
        IntegrationDecisionOutcome::Blocked,
    );
    assert!(o.checks[6].detail.contains("reverted"));
    // Missing reader is never a pass (origin and dependencies need State).
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

// ---- F4: contradictory evidence is refused (red control, pre-fix API).

#[test]
fn pass_result_with_nonzero_exit_is_contradictory_evidence() {
    let s = scenario();
    let mut k = knobs(&s);
    let contradictory = A3HandoffCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            argv(&["true"]),
            17,
            sha(&s.implementation),
            Some(false),
            None,
        )
        .unwrap(),
        Some(A3HandoffCheckResult::Pass),
    );
    let h = &s.handoff;
    k.handoffs = vec![
        A3HandoffNonTemporalCore::new(
            h.task_id().into(),
            h.attempt_id().into(),
            h.start_sha().as_str().into(),
            h.final_sha().as_str().into(),
            h.branch().into(),
            h.files_changed().to_vec(),
            vec![contradictory],
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
        .unwrap(),
    ];
    let o = evaluate(&s, &k);
    assert_ne!(
        o.outcome,
        IntegrationDecisionOutcome::Accept,
        "{:#?}",
        o.checks
    );
    assert!(
        o.failed().contains(&AcceptanceCheckId::RequiredChecks)
            || o.failed().contains(&AcceptanceCheckId::VerdictAndFindings),
        "{:#?}",
        o.checks
    );
}

// ---- F5: an actually reverted dependency is not effective (red control).

#[test]
fn reverted_dependency_with_preserved_ancestry_is_not_accepted() {
    let mut dep = String::new();
    let mut dep_start = String::new();
    let s = scenario_prepared(AssuranceProfile::Standard, "PASS", &[], |repo| {
        dep_start = git(&repo.root(), &["rev-parse", "HEAD"]);
        dep = repo.commit(&[("src/dep.txt", "dependency effect\n")], "dependency");
        git(&repo.root(), &["revert", "--no-edit", "HEAD"]);
    });
    assert!(!s.repo.root().join("src/dep.txt").exists());
    let dir = TempDir::new("state");
    let reader = state_with_dependency_range(&s, &dir, &dep_start, &dep, true);
    let mut k = knobs(&s);
    k.dependencies = vec!["dep-1".into()];
    k.state = Some(&reader);
    let o = evaluate(&s, &k);
    assert_ne!(
        o.outcome,
        IntegrationDecisionOutcome::Accept,
        "{:#?}",
        o.checks
    );
    assert!(
        o.failed().contains(&AcceptanceCheckId::Dependencies),
        "{:#?}",
        o.checks
    );
}

// ---- F4 (continued): contradictory and unadmitted evidence.

fn handoff_with_exit(s: &Scenario, exit_code: i64) -> A3HandoffNonTemporalCore {
    let h = &s.handoff;
    let check = A3HandoffCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            argv(&["true"]),
            exit_code,
            sha(&s.implementation),
            Some(false),
            None,
        )
        .unwrap(),
        Some(A3HandoffCheckResult::Pass),
    );
    A3HandoffNonTemporalCore::new(
        h.task_id().into(),
        h.attempt_id().into(),
        h.start_sha().as_str().into(),
        h.final_sha().as_str().into(),
        h.branch().into(),
        h.files_changed().to_vec(),
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

#[test]
fn worker_pass_with_nonzero_exit_is_refused_with_a_named_contradiction() {
    let s = scenario();
    let mut k = knobs(&s);
    k.handoffs = vec![handoff_with_exit(&s, 17)];
    let o = evaluate(&s, &k);
    assert_ne!(
        o.outcome,
        IntegrationDecisionOutcome::Accept,
        "{:#?}",
        o.checks
    );
    assert!(
        o.checks[4].detail.contains("contradicts itself") && o.checks[4].detail.contains("17"),
        "{}",
        o.checks[4].detail
    );
    // LIGHT does not authorize contradictory evidence either.
    assert!(o.failed().contains(&AcceptanceCheckId::RequiredChecks));
    // The consistent record still accepts.
    k.handoffs = vec![handoff_with_exit(&s, 0)];
    assert_eq!(evaluate(&s, &k).outcome, IntegrationDecisionOutcome::Accept);
}

#[test]
fn reproduction_pass_with_nonzero_exit_is_refused() {
    let s = scenario();
    let r = &s.review;
    let repro = r.reproduction().unwrap();
    let checks: Vec<_> = repro
        .checks()
        .unwrap()
        .iter()
        .map(|c| {
            A4ReviewReproductionCheck::new(
                c.source(),
                c.command().to_vec(),
                9,
                c.code_sha().to_owned(),
                c.timed_out(),
                None,
                c.result(),
            )
            .unwrap()
        })
        .collect();
    let forged = A4ReviewNonTemporalCore::new(
        r.review_id().into(),
        r.task_id().into(),
        r.reviewed_sha().into(),
        r.verdict(),
        true,
        vec![],
        vec![],
        r.dimensions().to_vec(),
        r.reviewer().cloned(),
        Some(A4ReviewReproduction::new(
            Some(true),
            Some(checks),
            A4ReviewReproductionLimitation::Null,
        )),
        Some(vec![]),
        None,
        None,
    )
    .unwrap();
    let mut k = knobs(&s);
    k.reviews = vec![forged];
    let o = evaluate(&s, &k);
    assert_ne!(
        o.outcome,
        IntegrationDecisionOutcome::Accept,
        "{:#?}",
        o.checks
    );
    // A record that differs from the persisted one is also an origin failure.
    assert!(
        o.checks[3]
            .detail
            .contains("differs from the persisted record")
    );
}

#[test]
fn records_must_come_from_the_admitted_origin() {
    // No State reader: origin cannot be verified.
    let s = scenario();
    let mut k = knobs(&s);
    k.state = None;
    let o = evaluate(&s, &k);
    assert!(o.failed().contains(&AcceptanceCheckId::VerdictAndFindings));
    assert!(o.checks[3].detail.contains("origin cannot be verified"));
    // A State that never persisted this review.
    let dir = TempDir::new("empty-state");
    let empty = state_with_packages(&dir, &[], |_| {});
    k.state = Some(&empty);
    let o = evaluate(&s, &k);
    assert!(
        o.checks[3].detail.contains("not a persisted record"),
        "{}",
        o.checks[3].detail
    );
    assert_eq!(o.outcome, IntegrationDecisionOutcome::Blocked);
    // A persisted review whose capsule was never dispatched.
    let mut half = s.package.clone();
    let spec_json = String::from_utf8(half.to_journal()).unwrap();
    assert!(spec_json.contains("REVIEW_DISPATCHED"));
    half.events.clear();
    half.event_keys.clear();
    let dir = TempDir::new("no-dispatch-state");
    let partial = state_with_packages(&dir, &[&half], |_| {});
    k.state = Some(&partial);
    let o = evaluate(&s, &k);
    assert!(
        o.checks[3].detail.contains("REVIEW_DISPATCHED"),
        "{}",
        o.checks[3].detail
    );
    // A handoff check the dispatched capsule does not carry.
    let mut k = knobs(&s);
    k.handoffs = vec![handoff_with_exit(&s, 0)];
    let altered = A3HandoffCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            argv(&["true", "--other"]),
            0,
            sha(&s.implementation),
            Some(false),
            None,
        )
        .unwrap(),
        Some(A3HandoffCheckResult::Pass),
    );
    let h = &s.handoff;
    k.handoffs = vec![
        A3HandoffNonTemporalCore::new(
            h.task_id().into(),
            h.attempt_id().into(),
            h.start_sha().as_str().into(),
            h.final_sha().as_str().into(),
            h.branch().into(),
            h.files_changed().to_vec(),
            vec![altered],
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
        .unwrap(),
    ];
    let o = evaluate(&s, &k);
    assert!(
        o.checks[4]
            .detail
            .contains("not in the dispatched review capsule")
    );
}

// ---- F13: nonblocking findings need explicit dispositions at the gate.

fn nonblocking_style(cap: &ReviewCapsuleNonTemporalCore) -> String {
    format!(
        r#"{{"review_id":"{}","task_id":"{}","reviewed_sha":"{}","independence_attested":true,"verdict":"PASS_WITH_NONBLOCKING_FINDINGS","blocking_findings":[],"nonblocking_findings":[{{"finding_id":"n0","severity":"LOW","category":"STYLE","description":"d","blocking":false}}],"dimensions":[{}]}}"#,
        cap.review_id(),
        cap.task_id(),
        cap.implementation_sha().as_str(),
        all_dimensions_json()
    )
}

#[test]
fn pass_with_nonblocking_findings_needs_explicit_dispositions_to_be_accepted() {
    use crate::FindingDispositionKind as Kind;
    let s = scenario_json(AssuranceProfile::Standard, |_| {}, nonblocking_style);
    assert_eq!(
        s.review.verdict(),
        crate::A4ReviewVerdict::PassWithNonblockingFindings
    );
    let mut k = knobs(&s);
    let o = evaluate(&s, &k);
    assert_eq!(
        o.outcome,
        IntegrationDecisionOutcome::Blocked,
        "{:#?}",
        o.checks
    );
    assert!(
        o.checks[3].detail.contains("not dispositioned"),
        "{}",
        o.checks[3].detail
    );
    k.dispositions = vec![FindingDisposition {
        finding_id: "n0".into(),
        kind: Kind::Defer {
            issue_id: "ISSUE-1".into(),
            revisit_point: "wave 3".into(),
        },
        decided_by: "a2".into(),
    }];
    assert_eq!(evaluate(&s, &k).outcome, IntegrationDecisionOutcome::Accept);
    // A disposition naming another finding does not cover this one.
    k.dispositions[0].finding_id = "other".into();
    assert_eq!(
        evaluate(&s, &k).outcome,
        IntegrationDecisionOutcome::Blocked
    );
}

// ---- F5: a dependency's effect must still be present (or verifiably superseded).

fn state_with_tasks(
    s: &Scenario,
    dir: &TempDir,
    tasks: &[(&str, &str, &str, bool)],
) -> StateReader {
    state_with_packages(dir, &[&s.package], |store| {
        for (task, start, implementation, accepted) in tasks {
            record_task(store, task, start, implementation, *accepted);
        }
    })
}

fn dep_gate(
    s: &Scenario,
    reader: &StateReader,
    supersessions: Vec<DependencySupersession>,
) -> GateOutcome {
    let mut k = knobs(s);
    k.dependencies = vec!["dep-1".into()];
    k.state = Some(reader);
    k.supersessions = supersessions;
    evaluate(s, &k)
}

fn claim(path: &str, by: &str) -> DependencySupersession {
    DependencySupersession {
        dependency_task_id: "dep-1".into(),
        path: path.into(),
        superseding_task_id: by.into(),
    }
}

#[test]
fn dependency_whose_effect_is_intact_is_accepted() {
    let mut range = (String::new(), String::new());
    let s = scenario_prepared(AssuranceProfile::Standard, "PASS", &[], |repo| {
        let start = git(&repo.root(), &["rev-parse", "HEAD"]);
        let dep = repo.commit(&[("src/dep.txt", "effect\n")], "dependency");
        range = (start, dep);
    });
    let dir = TempDir::new("state");
    let reader = state_with_tasks(&s, &dir, &[("dep-1", &range.0, &range.1, true)]);
    let o = dep_gate(&s, &reader, vec![]);
    assert_eq!(
        o.outcome,
        IntegrationDecisionOutcome::Accept,
        "{:#?}",
        o.checks
    );
}

#[test]
fn revert_then_reapply_restores_the_effect_whatever_the_messages_say() {
    let mut range = (String::new(), String::new());
    let s = scenario_prepared(AssuranceProfile::Standard, "PASS", &[], |repo| {
        let start = git(&repo.root(), &["rev-parse", "HEAD"]);
        let dep = repo.commit(&[("src/dep.txt", "effect\n")], "dependency");
        git(&repo.root(), &["revert", "--no-edit", "HEAD"]);
        // Reapplied under a misleading message: only content matters.
        repo.commit(
            &[("src/dep.txt", "effect\n")],
            "Revert \"dependency\" (not really)",
        );
        range = (start, dep);
    });
    let dir = TempDir::new("state");
    let reader = state_with_tasks(&s, &dir, &[("dep-1", &range.0, &range.1, true)]);
    assert_eq!(
        dep_gate(&s, &reader, vec![]).outcome,
        IntegrationDecisionOutcome::Accept
    );
}

#[test]
fn revert_hidden_behind_an_innocent_message_is_still_refused() {
    let mut range = (String::new(), String::new());
    let s = scenario_prepared(AssuranceProfile::Standard, "PASS", &[], |repo| {
        let start = git(&repo.root(), &["rev-parse", "HEAD"]);
        let dep = repo.commit(&[("src/dep.txt", "effect\n")], "dependency");
        // Content undone by a plain edit with a harmless message.
        repo.commit(&[("src/dep.txt", "gone\n")], "tidy up");
        range = (start, dep);
    });
    let dir = TempDir::new("state");
    let reader = state_with_tasks(&s, &dir, &[("dep-1", &range.0, &range.1, true)]);
    let o = dep_gate(&s, &reader, vec![]);
    only_failed(
        &o,
        AcceptanceCheckId::Dependencies,
        IntegrationDecisionOutcome::Blocked,
    );
    assert!(
        o.checks[6].detail.contains("src/dep.txt"),
        "{}",
        o.checks[6].detail
    );
}

#[test]
fn adopted_supersession_by_an_accepted_task_keeps_the_dependency_effective() {
    let mut ranges = Vec::new();
    let s = scenario_prepared(AssuranceProfile::Standard, "PASS", &[], |repo| {
        let start = git(&repo.root(), &["rev-parse", "HEAD"]);
        let dep = repo.commit(&[("src/dep.txt", "v1\n")], "dependency");
        let sup = repo.commit(&[("src/dep.txt", "v2\n")], "superseding task");
        ranges = vec![start, dep, sup];
    });
    let (start, dep, sup) = (&ranges[0], &ranges[1], &ranges[2]);
    let dir = TempDir::new("state");
    let reader = state_with_tasks(
        &s,
        &dir,
        &[("dep-1", start, dep, true), ("sup-1", dep, sup, true)],
    );
    // Without the evidence the changed path is uncertainty.
    assert_eq!(
        dep_gate(&s, &reader, vec![]).outcome,
        IntegrationDecisionOutcome::Blocked
    );
    // With it, the dependency stays effective.
    let o = dep_gate(&s, &reader, vec![claim("src/dep.txt", "sup-1")]);
    assert_eq!(
        o.outcome,
        IntegrationDecisionOutcome::Accept,
        "{:#?}",
        o.checks
    );
    // A superseder State does not show as accepted is not evidence.
    let dir2 = TempDir::new("state");
    let unaccepted = state_with_tasks(
        &s,
        &dir2,
        &[("dep-1", start, dep, true), ("sup-1", dep, sup, false)],
    );
    let o = dep_gate(&s, &unaccepted, vec![claim("src/dep.txt", "sup-1")]);
    assert_eq!(o.outcome, IntegrationDecisionOutcome::Blocked);
    assert!(o.checks[6].detail.contains("sup-1: not accepted"));
    // A claimed superseder that never changed the path proves nothing.
    let dir3 = TempDir::new("state");
    let wrong = state_with_tasks(
        &s,
        &dir3,
        &[("dep-1", start, dep, true), ("sup-1", start, dep, true)],
    );
    let o = dep_gate(&s, &wrong, vec![claim("src/dep.txt", "sup-1")]);
    assert_eq!(
        o.outcome,
        IntegrationDecisionOutcome::Blocked,
        "{:#?}",
        o.checks
    );
    // Self-supersession and a claim for another path do not count.
    for bad in [
        claim("src/dep.txt", "dep-1"),
        claim("src/other.txt", "sup-1"),
    ] {
        assert_eq!(
            dep_gate(&s, &reader, vec![bad]).outcome,
            IntegrationDecisionOutcome::Blocked
        );
    }
}

#[test]
fn supersession_does_not_cover_a_later_change_to_the_path() {
    let mut ranges = Vec::new();
    let s = scenario_prepared(AssuranceProfile::Standard, "PASS", &[], |repo| {
        let start = git(&repo.root(), &["rev-parse", "HEAD"]);
        let dep = repo.commit(&[("src/dep.txt", "v1\n")], "dependency");
        let sup = repo.commit(&[("src/dep.txt", "v2\n")], "superseding task");
        repo.commit(&[("src/dep.txt", "v3\n")], "unaccepted later edit");
        ranges = vec![start, dep, sup];
    });
    let dir = TempDir::new("state");
    let reader = state_with_tasks(
        &s,
        &dir,
        &[
            ("dep-1", &ranges[0], &ranges[1], true),
            ("sup-1", &ranges[1], &ranges[2], true),
        ],
    );
    let o = dep_gate(&s, &reader, vec![claim("src/dep.txt", "sup-1")]);
    assert_eq!(o.outcome, IntegrationDecisionOutcome::Blocked);
    assert!(
        o.checks[6].detail.contains("changed again"),
        "{}",
        o.checks[6].detail
    );
}

#[test]
fn candidate_that_itself_rewrites_a_dependency_file_is_uncertain() {
    let mut range = (String::new(), String::new());
    let s = scenario_prepared(AssuranceProfile::Standard, "PASS", &[], |repo| {
        let start = git(&repo.root(), &["rev-parse", "HEAD"]);
        // The scenario's own candidate commit rewrites src/status.txt to "ok".
        let dep = repo.commit(&[("src/status.txt", "dependency\n")], "dependency");
        range = (start, dep);
    });
    let dir = TempDir::new("state");
    let reader = state_with_tasks(&s, &dir, &[("dep-1", &range.0, &range.1, true)]);
    let o = dep_gate(&s, &reader, vec![]);
    only_failed(
        &o,
        AcceptanceCheckId::Dependencies,
        IntegrationDecisionOutcome::Blocked,
    );
    assert!(o.checks[6].detail.contains("src/status.txt"));
}

#[test]
fn dependency_without_a_recorded_start_is_uncertain() {
    let s = scenario();
    let dir = TempDir::new("state");
    let reader = state_with_packages(&dir, &[&s.package], |store| {
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
                admit(store, StateWriteOperationV1::ProvenanceLink),
                "project",
                "dep-1",
                ProvenanceLinkKind::ImplementationSha,
                &s.repo.base,
            )
            .unwrap();
    });
    let o = dep_gate(&s, &reader, vec![]);
    assert_eq!(o.outcome, IntegrationDecisionOutcome::Blocked);
}

#[test]
fn sealed_acceptance_refuses_a_consistently_rebound_foreign_subject() {
    let s = scenario();
    let outcome = evaluate(&s, &knobs(&s));
    let proposal = outcome.proposal.unwrap();
    let sealed = crate::SealedReview::new(&proposal, "ACCEPTANCE_EVALUATED", true, 0);
    assert!(sealed.settlement().is_ok());
    let scope = proposal.binding().scope();
    let moved = crate::ReviewClaimBinding::new(
        crate::ReviewOperation::Acceptance,
        crate::ReviewClaimScope::new(
            scope.project_id(),
            scope.task_id(),
            Some(&"1".repeat(40)),
            scope.request_sha256(),
            scope.generation(),
        )
        .unwrap(),
    );
    let mut spec: serde_json::Value = serde_json::from_slice(&proposal.to_journal()).unwrap();
    spec["binding"]["subject_sha"] = "1".repeat(40).into();
    spec["binding"]["key"] = moved.key().into();
    // Rebuild event keys as well, so the refusal must come from scope validation.
    for (index, event) in spec["events"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        event["key"] = crate::records::derived_id(
            "revent",
            &[
                moved.key(),
                &index.to_string(),
                event["event_type"].as_str().unwrap(),
            ],
        )
        .into();
    }
    let rebound = crate::ReviewRecordProposal::from_journal(spec.to_string().as_bytes()).unwrap();
    assert!(
        crate::SealedReview::new(&rebound, "ACCEPTANCE_EVALUATED", true, 0)
            .settlement()
            .is_err()
    );
}
