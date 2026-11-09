//! The nine-check acceptance gate over State written only through the real
//! Core sink, and its IntegrationDecision published through the same sink.
//!
//! The review runs through `CoreReviewSink` (a synthetic reviewer, not
//! provider proof). The gate then reads the A4Review record and the
//! REVIEW_DISPATCHED event back from that State, evaluates the nine checks on
//! real repositories and worktrees, and records its decision through the same
//! claim → seal → publish path.

mod common;

use common::*;
use receipts_review_integration::runtime::FailureClass;
use receipts_review_integration::state::{
    DecisionRecordKind, EventType, ReviewClaimStateV1 as S, StateReader,
};
use receipts_review_integration::{
    A3HandoffCheck, A3HandoffCheckResult, A3HandoffNonTemporalCore, AcceptanceCheckId,
    AcceptanceGate, AcceptanceInput, AssuranceProfile, AttemptFinished, CheckRunnerConfig,
    IntegrationDecisionNullableString, IntegrationDecisionOutcome, IntegrationRequestA4Verdict,
    IntegrationRequestAttestations, IntegrationRequestGateLevel, IntegrationRequestNonTemporalCore,
    IntegrationRequestTask, Recorded, ReviewCapsuleCheck, ReviewCapsuleCheckResult,
    ReviewCapsuleCriterion, ReviewCapsuleCriterionKind, ReviewCapsuleReviewScope, ReviewController,
    ReviewRequest, ReviewRequestNonNegativeInteger, ReviewRequestNonTemporalCore,
};
use receipts_workspace_execution::execution::ProcessTimeoutPolicy;
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
};
use std::path::PathBuf;
use std::time::Duration;

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn first_existing(candidates: &[&str]) -> PathBuf {
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("none of {candidates:?} exists"))
}

fn runner() -> CheckRunnerConfig {
    CheckRunnerConfig {
        executables: vec![
            (
                "true".into(),
                first_existing(&["/usr/bin/true", "/bin/true"]),
            ),
            (
                "grep".into(),
                first_existing(&["/usr/bin/grep", "/bin/grep"]),
            ),
        ],
        timeout: ProcessTimeoutPolicy::new(Duration::from_secs(20), Duration::from_millis(500))
            .unwrap(),
        env: None,
    }
}

struct Scenario {
    dir: Dir,
    root: PathBuf,
    task_wt: PathBuf,
    db: PathBuf,
    base: String,
    implementation: String,
    request: ReviewRequest,
    checks: Vec<ReviewCapsuleCheck>,
}

fn scenario(tag: &str) -> Scenario {
    let Fixture { dir, root, db, .. } = fixture(tag);
    let base = git(&root, &["rev-parse", "HEAD"]);
    // A real task branch checked out in its own worktree, one commit past base.
    let task_wt = dir.0.join("task-wt");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "task/1",
            task_wt.to_str().unwrap(),
            &base,
        ],
    );
    std::fs::create_dir_all(task_wt.join("src")).unwrap();
    std::fs::write(task_wt.join("src/status.txt"), "ok\n").unwrap();
    git(&task_wt, &["add", "-A"]);
    git(&task_wt, &["commit", "-q", "-m", "impl"]);
    let implementation = git(&task_wt, &["rev-parse", "HEAD"]);
    let criteria = vec![
        ReviewCapsuleCriterion::new(
            "c1".into(),
            "status says ok".into(),
            ReviewCapsuleCriterionKind::Deterministic,
            Some(argv(&["grep", "-q", "ok", "src/status.txt"])),
            None,
        )
        .unwrap(),
    ];
    let request = ReviewRequest::new(
        ReviewRequestNonTemporalCore::new(
            "request-1".into(),
            "task-1".into(),
            "attempt-1".into(),
            implementation.clone(),
            base.clone(),
            "make status ok".into(),
            criteria,
            vec!["src/**".into()],
            AssuranceProfile::Standard,
            ReviewCapsuleReviewScope::Full,
            ReviewRequestNonNegativeInteger::from_decimal("3").unwrap(),
            Some("ws-1".into()),
            Some("task/1".into()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap(),
        None,
    );
    let checks = vec![ReviewCapsuleCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            argv(&["true"]),
            0,
            CommitSha::parse(&implementation).unwrap(),
            Some(false),
            None,
        )
        .unwrap(),
        Some(ReviewCapsuleCheckResult::Pass),
    )];
    Scenario {
        db,
        dir,
        root,
        task_wt,
        base,
        implementation,
        request,
        checks,
    }
}

fn handoff(s: &Scenario) -> A3HandoffNonTemporalCore {
    let check = A3HandoffCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            argv(&["true"]),
            0,
            CommitSha::parse(&s.implementation).unwrap(),
            Some(false),
            None,
        )
        .unwrap(),
        Some(A3HandoffCheckResult::Pass),
    );
    A3HandoffNonTemporalCore::new(
        "task-1".into(),
        "attempt-1".into(),
        s.base.clone(),
        s.implementation.clone(),
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

fn integration_request(s: &Scenario) -> IntegrationRequestNonTemporalCore {
    IntegrationRequestNonTemporalCore::new(
        "ireq-1".into(),
        IntegrationRequestGateLevel::A2Acceptance,
        "task/1".into(),
        s.implementation.clone(),
        "main".into(),
        vec![
            IntegrationRequestTask::new(
                "task-1".into(),
                s.implementation.clone(),
                s.implementation.clone(),
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

/// Synthetic PASS bound to the capsule.
struct Pass;
impl receipts_review_integration::ReviewerRuntime for Pass {
    fn runtime_id(&self) -> &str {
        "synthetic"
    }
    fn run_review(
        &self,
        c: &receipts_review_integration::ReviewCapsuleNonTemporalCore,
        w: &receipts_workspace_execution::WorkspaceHandle,
    ) -> Result<receipts_review_integration::ReviewerRunOutput, FailureClass> {
        Synthetic::default().run_review(c, w)
    }
}

fn review_through_core(
    s: &Scenario,
    sink: &mut dyn receipts_review_integration::ReviewRecordSink,
) -> receipts_review_integration::ReviewTicket {
    let checks = runner();
    let controller = ReviewController {
        routing: &Pick::default(),
        runtimes: &[&Pass],
        checks: &checks,
        clock: &now,
    };
    controller.on_attempt_finished(
        AttemptFinished {
            project_id: PROJECT,
            request: &s.request,
            repository_root: &s.root,
            implementer: &implementer(),
            worker_checks: &s.checks,
            review_worktree: &s.dir.0.join("review-wt"),
            generation: 1,
        },
        sink,
    )
}

fn evaluate(
    s: &Scenario,
    ticket: &receipts_review_integration::ReviewTicket,
    state: &StateReader,
    generation: u32,
) -> receipts_review_integration::GateOutcome {
    let review = ticket.review().unwrap().core().clone();
    let ireq = integration_request(s);
    AcceptanceGate::evaluate(&AcceptanceInput {
        project_id: PROJECT,
        integration_request: &ireq,
        review_request: &s.request,
        profile_floor: AssuranceProfile::Light,
        capsule: ticket.capsule.as_ref().unwrap(),
        handoffs: &[handoff(s)],
        reviews: &[review],
        completed_repair_attempts: 0,
        repository_root: &s.root,
        task_worktree: &s.task_wt,
        merge_target_worktree: &s.root,
        forbidden_write_paths: &[],
        dependencies: &[],
        state: Some(state),
        supersessions: &[],
        finding_dispositions: &[],
        checks: &runner(),
        component_suite: &argv(&["grep", "-q", "ok", "src/status.txt"]),
        scratch: &s.dir.0.join("gate-scratch"),
        clock: &now,
        generation,
    })
}

#[test]
fn the_gate_reads_core_written_state_and_its_decision_publishes_through_the_core_sink() {
    let s = scenario("accept");
    std::fs::create_dir(s.dir.0.join("gate-scratch")).unwrap();
    let clock = Clock::default();
    let mut sink = open_sink(&s.db, &clock);
    let ticket = review_through_core(&s, &mut sink);
    assert!(
        ticket.passed(),
        "{:?} {:?}",
        ticket.disposition,
        ticket.persisted
    );
    drop(sink);

    // The gate's origin binding reads the Core-published record and event.
    let state = StateReader::open_existing(&s.db).unwrap();
    let outcome = evaluate(&s, &ticket, &state, 1);
    assert_eq!(
        outcome.outcome,
        IntegrationDecisionOutcome::Accept,
        "{:#?}",
        outcome.checks
    );
    assert!(outcome.failed().is_empty(), "{:#?}", outcome.checks);
    drop(state);

    let mut sink = open_sink(&s.db, &clock);
    assert_eq!(outcome.record(&mut sink), Ok(Recorded::Published));
    let b = outcome.proposal.as_ref().unwrap().binding().clone();
    assert_eq!(
        b.operation(),
        receipts_review_integration::ReviewOperation::Acceptance
    );
    let claim = image(&sink, &b, "x").claim.unwrap();
    assert_eq!(claim.state, S::Completed);
    assert_eq!(claim.outcome_kind.as_deref(), Some("ACCEPTANCE_EVALUATED"));
    let decision_id = claim.outcome_ref.clone().unwrap();
    // Exact replay is idempotent; the review claim is a different claim.
    let before = image(&sink, &b, "x");
    assert_eq!(outcome.record(&mut sink), Ok(Recorded::Published));
    assert_eq!(image(&sink, &b, "x"), before);
    assert_ne!(b.key(), ticket.proposal.as_ref().unwrap().claim_key());
    drop(sink);

    let reader = StateReader::open_existing(&s.db).unwrap();
    assert!(
        reader
            .find_decision_record(
                PROJECT,
                DecisionRecordKind::IntegrationDecision,
                &decision_id
            )
            .unwrap()
            .is_some()
    );
    let types = stored_event_types(&s.db);
    assert!(types.contains(&EventType::AcceptanceEvaluated), "{types:?}");
    assert_eq!(types.len(), 3, "dispatch, verdict, acceptance: {types:?}");
}

#[test]
fn a_gate_refuses_a_review_that_was_never_published_through_the_core() {
    let s = scenario("accept-unpublished");
    std::fs::create_dir(s.dir.0.join("gate-scratch")).unwrap();
    let clock = Clock::default();
    // Run the review with a sink that loses the publication: the review ran,
    // the package is sealed, but State holds no A4Review record yet.
    let mut lossy = Faulty::open(&s.db, &clock);
    lossy.fault = Fault::PublishLost;
    let ticket = review_through_core(&s, &mut lossy);
    assert!(ticket.persisted.is_err());
    drop(lossy);
    let state = StateReader::open_existing(&s.db).unwrap();
    let outcome = evaluate(&s, &ticket, &state, 1);
    assert_ne!(outcome.outcome, IntegrationDecisionOutcome::Accept);
    assert!(
        outcome
            .failed()
            .contains(&AcceptanceCheckId::VerdictAndFindings),
        "the unpublished review is an unadmitted origin: {:#?}",
        outcome.checks
    );
}
