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
    runtime: &dyn receipts_review_integration::ReviewerRuntime,
) -> receipts_review_integration::ReviewTicket {
    let checks = runner();
    let controller = ReviewController {
        routing: &Pick::default(),
        runtimes: &[runtime],
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
    dispositions: &[receipts_review_integration::FindingDisposition],
) -> receipts_review_integration::GateOutcome {
    evaluate_with_content(s, ticket, state, generation, dispositions, None)
}

fn evaluate_with_content(
    s: &Scenario,
    ticket: &receipts_review_integration::ReviewTicket,
    state: &StateReader,
    generation: u32,
    dispositions: &[receipts_review_integration::FindingDisposition],
    content: Option<&dyn receipts_review_integration::ReviewContentResolver>,
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
        finding_dispositions: dispositions,
        disposition_content: content,
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
    let ticket = review_through_core(&s, &mut sink, &Pass);
    assert!(
        ticket.passed(),
        "{:?} {:?}",
        ticket.disposition,
        ticket.persisted
    );
    drop(sink);

    // The gate's origin binding reads the Core-published record and event.
    let state = StateReader::open_existing(&s.db).unwrap();
    let outcome = evaluate(&s, &ticket, &state, 1, &[]);
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
    let ticket = review_through_core(&s, &mut lossy, &Pass);
    assert!(ticket.persisted.is_err());
    drop(lossy);
    let state = StateReader::open_existing(&s.db).unwrap();
    let outcome = evaluate(&s, &ticket, &state, 1, &[]);
    assert_ne!(outcome.outcome, IntegrationDecisionOutcome::Accept);
    assert!(
        outcome
            .failed()
            .contains(&AcceptanceCheckId::VerdictAndFindings),
        "the unpublished review is an unadmitted origin: {:#?}",
        outcome.checks
    );
}

#[test]
fn dispositions_must_be_persisted_for_the_exact_review_and_conflicts_block() {
    use receipts_review_integration::{
        FindingDisposition, FindingDispositionKind as Kind, disposition_proposal,
        record_dispositions,
    };
    let mut s = scenario("durable-dispositions");
    std::fs::create_dir(s.dir.0.join("gate-scratch")).unwrap();
    let clock = Clock::default();
    let mut sink = Faulty::open(&s.db, &clock);
    let runtime = Synthetic {
        nonblocking: true,
        ..Synthetic::default()
    };
    let ticket = review_through_core(&s, &mut sink, &runtime);
    assert!(ticket.persisted.is_ok());
    let ds = [FindingDisposition {
        finding_id: "n1".into(),
        kind: Kind::Defer {
            issue_id: "ISSUE-1".into(),
            revisit_point: "wave3".into(),
        },
        decided_by: "a2".into(),
    }];
    let check = |s: &Scenario, ds: &[FindingDisposition]| {
        let state = StateReader::open_existing(&s.db).unwrap();
        evaluate(s, &ticket, &state, 1, ds)
    };
    assert_eq!(check(&s, &ds).outcome, IntegrationDecisionOutcome::Blocked);
    // A genuinely published disposition for another review cannot cover this one.
    let original = s.request.clone();
    let foreign = receipts_review_integration::wire::encode_review_request(&original)
        .replace("request-1", "request-foreign");
    s.request = receipts_review_integration::wire::decode_review_request(&foreign).unwrap();
    let other = review_through_core(&s, &mut sink, &runtime);
    let foreign_proposal =
        disposition_proposal(PROJECT, &s.request, other.review().unwrap().core(), &ds, 1).unwrap();
    assert_eq!(
        record_dispositions(&mut sink, &foreign_proposal),
        Ok(Recorded::Published)
    );
    let foreign_content = sink
        .inner
        .content(foreign_proposal.binding())
        .unwrap()
        .unwrap();
    s.request = original;
    let state = StateReader::open_existing(&s.db).unwrap();
    let foreign = evaluate_with_content(&s, &ticket, &state, 1, &ds, Some(&foreign_content));
    assert_eq!(foreign.outcome, IntegrationDecisionOutcome::Blocked);
    assert!(foreign.checks[3].detail.contains("not persisted"));

    let p =
        disposition_proposal(PROJECT, &s.request, ticket.review().unwrap().core(), &ds, 1).unwrap();
    sink.fault = Fault::PublishLost;
    assert!(record_dispositions(&mut sink, &p).is_err());
    let sealed_content = sink.inner.content(p.binding()).unwrap().unwrap();
    let state = StateReader::open_existing(&s.db).unwrap();
    let sealed = evaluate_with_content(&s, &ticket, &state, 1, &ds, Some(&sealed_content));
    assert_eq!(
        sealed.outcome,
        IntegrationDecisionOutcome::Blocked,
        "a seal is not publication"
    );
    assert!(sealed.checks[3].detail.contains("not persisted"));
    drop(sink);
    let mut sink = Faulty::open(&s.db, &clock);
    assert_eq!(record_dispositions(&mut sink, &p), Ok(Recorded::Published));
    assert_eq!(record_dispositions(&mut sink, &p), Ok(Recorded::Published));
    let content = sink.inner.content(p.binding()).unwrap().unwrap();
    let state = StateReader::open_existing(&s.db).unwrap();
    let accepted = evaluate_with_content(&s, &ticket, &state, 1, &ds, Some(&content));
    assert_eq!(
        accepted.outcome,
        IntegrationDecisionOutcome::Accept,
        "{:#?}",
        accepted.checks
    );
    assert_eq!(accepted.record(&mut sink), Ok(Recorded::Published));
    struct Unavailable;
    impl receipts_review_integration::ReviewContentResolver for Unavailable {
        fn resolve(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(None)
        }
    }
    struct Altered;
    impl receipts_review_integration::ReviewContentResolver for Altered {
        fn resolve(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(Some(b"altered".to_vec()))
        }
    }
    struct Unreadable;
    impl receipts_review_integration::ReviewContentResolver for Unreadable {
        fn resolve(&self, _: &str, _: &str) -> Result<Option<Vec<u8>>, String> {
            Err("storage unavailable".into())
        }
    }
    for resolver in [
        &Unavailable as &dyn receipts_review_integration::ReviewContentResolver,
        &Altered,
        &Unreadable,
    ] {
        assert_eq!(
            evaluate_with_content(&s, &ticket, &state, 1, &ds, Some(resolver)).outcome,
            IntegrationDecisionOutcome::Blocked
        );
    }
    for altered in [
        FindingDisposition {
            decided_by: "someone-else".into(),
            ..ds[0].clone()
        },
        FindingDisposition {
            kind: Kind::Defer {
                issue_id: "ISSUE-OTHER".into(),
                revisit_point: "wave3".into(),
            },
            ..ds[0].clone()
        },
        FindingDisposition {
            finding_id: "other-finding".into(),
            ..ds[0].clone()
        },
    ] {
        assert_eq!(
            evaluate_with_content(&s, &ticket, &state, 1, &[altered], Some(&content)).outcome,
            IntegrationDecisionOutcome::Blocked
        );
    }
    let conflicting = [FindingDisposition {
        kind: Kind::Reject {
            reason: "different decision".into(),
        },
        ..ds[0].clone()
    }];
    let p2 = disposition_proposal(
        PROJECT,
        &s.request,
        ticket.review().unwrap().core(),
        &conflicting,
        1,
    )
    .unwrap();
    assert_eq!(record_dispositions(&mut sink, &p2), Ok(Recorded::Published));
    assert_eq!(
        evaluate_with_content(&s, &ticket, &state, 1, &ds, Some(&content)).outcome,
        IntegrationDecisionOutcome::Blocked,
        "matching evidence does not hide a conflicting decision"
    );
}

#[test]
fn persisted_review_binds_all_request_semantics_but_not_requested_at() {
    let mut s = scenario("semantic-request");
    std::fs::create_dir(s.dir.0.join("gate-scratch")).unwrap();
    let clock = Clock::default();
    let mut sink = open_sink(&s.db, &clock);
    let ticket = review_through_core(&s, &mut sink, &Pass);
    assert!(ticket.passed());
    drop(sink);
    let state = StateReader::open_existing(&s.db).unwrap();
    let original = s.request.clone();
    let json = receipts_review_integration::wire::encode_review_request(&original);
    for changed in [
        json.replace(
            r#""check_command":["grep","-q","ok","src/status.txt"]"#,
            r#""check_command":["false"]"#,
        ),
        json.replace("status says ok", "unreviewed criterion"),
        json.replace(
            r#""allowed_write_paths":["src/**"]"#,
            r#""allowed_write_paths":["docs/**"]"#,
        ),
        json.replace(
            r#""review_scope":"FULL""#,
            r#""review_scope":"FULL","review_policy":{"distinct_provider":"REQUIRED"}"#,
        ),
        json.replace("make status ok", "unreviewed objective"),
    ] {
        assert_ne!(changed, json);
        s.request = receipts_review_integration::wire::decode_review_request(&changed).unwrap();
        let outcome = evaluate(&s, &ticket, &state, 1, &[]);
        assert_eq!(
            outcome.outcome,
            IntegrationDecisionOutcome::Blocked,
            "{:#?}",
            outcome.checks
        );
        assert!(
            outcome
                .failed()
                .contains(&AcceptanceCheckId::CriteriaReproduced)
        );
    }
    s.request = ReviewRequest::new(original.core().clone(), Some(now()));
    assert_eq!(
        evaluate(&s, &ticket, &state, 1, &[]).outcome,
        IntegrationDecisionOutcome::Accept
    );
}

#[test]
fn supersession_order_is_verified_against_core_written_task_provenance() {
    use receipts_orchestration::orchestration::CoreStateWriter;
    use receipts_review_integration::DependencySupersession;
    use receipts_review_integration::state::{DecisionRecordV1, ProvenanceLinkKind as Link};
    for later in [false, true] {
        let mut s = scenario(if later {
            "later-superseder"
        } else {
            "older-superseder"
        });
        // Rebuild the task above a real dependency sequence before reviewing it.
        let start = git(&s.root, &["rev-parse", "HEAD"]);
        std::fs::write(s.root.join("src/dep.txt"), "old\n").unwrap();
        git(&s.root, &["add", "-A"]);
        git(&s.root, &["commit", "-qm", "old"]);
        let old = git(&s.root, &["rev-parse", "HEAD"]);
        std::fs::write(s.root.join("src/dep.txt"), "new\n").unwrap();
        git(&s.root, &["commit", "-qam", "dependency"]);
        let dep = git(&s.root, &["rev-parse", "HEAD"]);
        std::fs::write(s.root.join("src/dep.txt"), "old\n").unwrap();
        git(&s.root, &["commit", "-qam", "replacement"]);
        let replacement = git(&s.root, &["rev-parse", "HEAD"]);
        git(&s.task_wt, &["merge", "--no-edit", "main"]);
        let implementation = git(&s.task_wt, &["rev-parse", "HEAD"]);
        let json = receipts_review_integration::wire::encode_review_request(&s.request)
            .replace(&s.implementation, &implementation);
        s.request = receipts_review_integration::wire::decode_review_request(&json).unwrap();
        s.implementation = implementation.clone();
        s.checks = vec![ReviewCapsuleCheck::new(
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
        std::fs::create_dir(s.dir.0.join("gate-scratch")).unwrap();
        let clock = Clock::default();
        let mut sink = open_sink(&s.db, &clock);
        let ticket = review_through_core(&s, &mut sink, &Pass);
        assert!(ticket.passed(), "{:?}", ticket.disposition);
        drop(sink);
        let mut writer = CoreStateWriter::open(&s.db, identity()).unwrap();
        let superseder = if later {
            (&dep, &replacement)
        } else {
            (&start, &old)
        };
        for (task, from, to) in [("dep-1", &old, &dep), ("sup-1", superseder.0, superseder.1)] {
            for (kind, value) in [(Link::StartSha, from), (Link::ImplementationSha, to)] {
                writer
                    .append_provenance_link(PROJECT, task, kind, value)
                    .unwrap();
            }
            let id = format!("{task}-accepted");
            let json = format!(
                r#"{{"decision_id":"{id}","request_id":"{task}-req","outcome":"ACCEPT","checks_evaluated":[{{"check":"c","result":"PASS"}}],"provenance_chain":[{{"task_id":"{task}","start_sha":"{from}","implementation_sha":"{to}","review_sha":"{to}"}}],"decided_at":"2026-11-08T00:00:00Z"}}"#
            );
            writer
                .append_decision_record(
                    &DecisionRecordV1::parse(
                        PROJECT,
                        DecisionRecordKind::IntegrationDecision,
                        &id,
                        &json,
                    )
                    .unwrap(),
                )
                .unwrap();
            writer
                .append_provenance_link(PROJECT, task, Link::AcceptanceRef, &id)
                .unwrap();
        }
        drop(writer);
        let state = StateReader::open_existing(&s.db).unwrap();
        let ireq = integration_request(&s);
        let review = ticket.review().unwrap().core().clone();
        let outcome = AcceptanceGate::evaluate(&AcceptanceInput {
            project_id: PROJECT,
            integration_request: &ireq,
            review_request: &s.request,
            profile_floor: AssuranceProfile::Light,
            capsule: ticket.capsule.as_ref().unwrap(),
            handoffs: &[handoff(&s)],
            reviews: &[review],
            completed_repair_attempts: 0,
            repository_root: &s.root,
            task_worktree: &s.task_wt,
            merge_target_worktree: &s.root,
            forbidden_write_paths: &[],
            dependencies: &["dep-1".into()],
            state: Some(&state),
            supersessions: &[DependencySupersession {
                dependency_task_id: "dep-1".into(),
                path: "src/dep.txt".into(),
                superseding_task_id: "sup-1".into(),
            }],
            finding_dispositions: &[],
            disposition_content: None,
            checks: &runner(),
            component_suite: &argv(&["true"]),
            scratch: &s.dir.0.join("gate-scratch"),
            clock: &now,
            generation: 1,
        });
        assert_eq!(
            outcome.outcome,
            if later {
                IntegrationDecisionOutcome::Accept
            } else {
                IntegrationDecisionOutcome::Blocked
            },
            "{:#?}",
            outcome.checks
        );
    }
}
