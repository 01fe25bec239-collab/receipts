//! Controller controls over real temporary repositories, real Workspace
//! review worktrees and the real argv runner. The reviewer itself is a
//! scripted synthetic runtime: these prove controller behaviour, not
//! provider or native protected-launch execution.

use crate::controller::*;
use crate::ports::{ImplementerIdentity, ReviewerCandidate, ReviewerRoutingRefusal, SealedReview};
use crate::test_repo::*;
use crate::{
    A4ReviewFindingCategory, A4ReviewReproductionCheckResult, A4ReviewVerdict, AssuranceProfile,
    DistinctProviderPolicy, ReviewCapsuleCheckResult, ReviewRequestPolicy, SafetyInterruptionState,
    SafetyInterruptionTerminalOutcome,
};
use receipts_runtime_adapters::FailureClass;
use receipts_state::{DecisionRecordKind, EventType};
use std::time::Duration;

fn implementer() -> ImplementerIdentity {
    ImplementerIdentity {
        provider_id: "provider-a".into(),
        runtime_id: "runtime-a".into(),
        model_id: None,
        session_ref: Some("implementer-session".into()),
    }
}

struct Case {
    repo: Repo,
    implementation: String,
    request: crate::ReviewRequest,
    checks: Vec<crate::ReviewCapsuleCheck>,
}

fn case(profile: AssuranceProfile, policy: Option<ReviewRequestPolicy>, status: &str) -> Case {
    let mut repo = Repo::new();
    let implementation = repo.commit(&[("src/status.txt", status)], "impl");
    let request = request(&repo.base, &implementation, profile, policy);
    let checks = vec![worker_check(
        &["true"],
        &implementation,
        ReviewCapsuleCheckResult::Pass,
    )];
    Case {
        repo,
        implementation,
        request,
        checks,
    }
}

fn run(
    c: &Case,
    routing: &FixedRouting,
    runtimes: &[&dyn crate::ReviewerRuntime],
    timeout: Duration,
    sink: &mut MemorySink,
) -> ReviewTicket {
    run_generation(c, routing, runtimes, timeout, sink, 1)
}

fn run_generation(
    c: &Case,
    routing: &FixedRouting,
    runtimes: &[&dyn crate::ReviewerRuntime],
    timeout: Duration,
    sink: &mut MemorySink,
    generation: u32,
) -> ReviewTicket {
    let checks = runner(timeout);
    let controller = ReviewController {
        routing,
        runtimes,
        checks: &checks,
        clock: &clock,
    };
    let root = c.repo.root();
    let worktree = c.repo.scratch("review-wt");
    controller.on_attempt_finished(
        AttemptFinished {
            project_id: "project",
            request: &c.request,
            repository_root: &root,
            implementer: &implementer(),
            worker_checks: &c.checks,
            review_worktree: &worktree,
            generation,
        },
        sink,
    )
}

fn pass_reviewer(id: &str) -> ScriptedReviewer {
    ScriptedReviewer::new(id, |cap| Ok(output(&verdict_json(cap, "PASS", &[]))))
}

fn events(t: &ReviewTicket) -> Vec<EventType> {
    t.proposal
        .as_ref()
        .unwrap()
        .events
        .iter()
        .map(|e| e.event_type())
        .collect()
}

fn reviewed(t: &ReviewTicket) -> &crate::A4Review {
    match &t.disposition {
        ReviewDisposition::Reviewed(r) => r,
        other => panic!("expected a review, got {other:?}"),
    }
}

#[test]
fn a3_completion_triggers_review_without_user_action() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut sink = MemorySink::default();
    let t = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    assert!(t.passed(), "{:?}", t.disposition);
    assert_eq!(t.dispatches, 1);
    let calls = reviewer.calls.borrow();
    assert_eq!(calls.len(), 1);
    let (branch, path, head) = &calls[0];
    assert!(branch.starts_with("receipts/review/"), "{branch}");
    assert_eq!(head, &c.implementation);
    assert_eq!(path, &c.repo.scratch("review-wt"));
    // Owned cleanup: worktree and review branch are gone.
    assert!(!path.exists());
    assert!(!git(&c.repo.root(), &["branch", "--list", "receipts/review/*"]).contains("review"));
    let r = reviewed(&t);
    assert_eq!(r.core().verdict(), A4ReviewVerdict::Pass);
    assert!(r.core().independence_attested());
    assert_eq!(r.core().reviewed_sha(), c.implementation);
    let reviewer_id = r.core().reviewer().unwrap();
    assert_eq!(reviewer_id.provider_id(), Some("provider-b"));
    assert_eq!(reviewer_id.session_ref(), Some("reviewer-session"));
    // Controller-run reproduction: criterion grep + worker `true`, both PASS.
    let repro = r.core().reproduction().unwrap();
    assert_eq!(repro.performed(), Some(true));
    assert_eq!(repro.checks().unwrap().len(), 2);
    assert!(repro.checks().unwrap().iter().all(|c| c.result()
        == Some(A4ReviewReproductionCheckResult::Pass)
        && c.code_sha() == c_impl(&t)));
    assert_eq!(r.core().unauthorized_file_changes(), Some(&[][..]));
    assert_eq!(
        events(&t),
        [EventType::ReviewDispatched, EventType::ReviewPassed]
    );
    let p = &sink.proposals[0];
    assert_eq!(p.decision_records[0].kind(), DecisionRecordKind::A4Review);
    for e in &p.events {
        assert!(p.content.iter().any(|(r, _)| r == &e.payload().reference));
    }
}

fn c_impl(t: &ReviewTicket) -> &str {
    t.capsule.as_ref().unwrap().implementation_sha().as_str()
}

#[test]
fn same_session_reviewer_refused() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let mut routing = FixedRouting::candidate("provider-b", "runtime-b");
    if let Ok(cand) = &mut routing.answer {
        cand.resume_session = Some("implementer-session".into());
    }
    let reviewer = pass_reviewer("runtime-b");
    let t = run(
        &c,
        &routing,
        &[&reviewer],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::SameSession)
    ));
    assert_eq!(t.dispatches, 0);
    assert!(reviewer.calls.borrow().is_empty());
    assert_eq!(events(&t), [EventType::HumanRequired]);

    // Any resume (not only the implementer's) is refused: reviews are fresh.
    if let Ok(cand) = &mut routing.answer {
        cand.resume_session = Some("old-review".into());
    }
    let t = run(
        &c,
        &routing,
        &[&reviewer],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::ResumeRefused)
    ));

    // A runtime that reports running in the implementer's session is void.
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let sneaky = ScriptedReviewer::new("runtime-b", |cap| {
        let mut o = output(&verdict_json(cap, "PASS", &[]));
        o.session_ref = Some("implementer-session".into());
        Ok(o)
    });
    let t = run(
        &c,
        &routing,
        &[&sneaky],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(matches!(t.disposition, ReviewDisposition::InvalidOutput(_)));
    assert!(!t.passed());
    // The excluded-session list from the request is also enforced.
    assert!(
        routing.asked.borrow()[0]
            .excluded_session_refs
            .contains(&"implementer-session".to_string())
    );
}

#[test]
fn distinct_provider_enforced_when_required() {
    // HIGH_ASSURANCE requires a distinct provider: same provider refused.
    let c = case(AssuranceProfile::HighAssurance, None, "ok\n");
    let routing = FixedRouting::candidate("provider-a", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let t = run(
        &c,
        &routing,
        &[&reviewer],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::DistinctProviderRequired)
    ));
    assert_eq!(t.dispatches, 0);
    let asked = routing.asked.borrow()[0].clone();
    assert_eq!(asked.distinct_provider, DistinctProviderPolicy::Required);
    assert_eq!(asked.excluded_providers, ["provider-a"]);

    // A request may raise STANDARD's PREFERRED to REQUIRED…
    let raise = ReviewRequestPolicy::new(None, Some(DistinctProviderPolicy::Required), None, None);
    let c = case(AssuranceProfile::Standard, Some(raise), "ok\n");
    assert_eq!(
        effective_distinct_provider(&c.request),
        DistinctProviderPolicy::Required
    );
    let t = run(
        &c,
        &routing,
        &[&reviewer],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::DistinctProviderRequired)
    ));

    // …but never lower it: OFF on a STANDARD request stays PREFERRED, where
    // a same-provider fresh session is permitted.
    let lower = ReviewRequestPolicy::new(None, Some(DistinctProviderPolicy::Off), None, None);
    let c = case(AssuranceProfile::Standard, Some(lower), "ok\n");
    assert_eq!(
        effective_distinct_provider(&c.request),
        DistinctProviderPolicy::Preferred
    );
    let t = run(
        &c,
        &routing,
        &[&reviewer],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(t.passed(), "{:?}", t.disposition);
    let lower = ReviewRequestPolicy::new(None, Some(DistinctProviderPolicy::Off), None, None);
    let c = case(AssuranceProfile::HighAssurance, Some(lower), "ok\n");
    assert_eq!(
        effective_distinct_provider(&c.request),
        DistinctProviderPolicy::Required
    );
}

#[test]
fn reviewer_worktree_is_detached_read_only() {
    // Workspace decision D-9: a separate task-bound review worktree on a
    // fresh receipts/review/<hex> branch at the exact SHA (not a detached
    // HEAD). Any reviewer write or commit voids the review.
    for tamper in [Tamper::WriteFile, Tamper::Commit] {
        let c = case(AssuranceProfile::Standard, None, "ok\n");
        let routing = FixedRouting::candidate("provider-b", "runtime-b");
        let mut reviewer = pass_reviewer("runtime-b");
        reviewer.tamper = tamper;
        let main_before = git(&c.repo.root(), &["rev-parse", "HEAD"]);
        let t = run(
            &c,
            &routing,
            &[&reviewer],
            time_limit(),
            &mut MemorySink::default(),
        );
        assert!(
            matches!(&t.disposition, ReviewDisposition::InvalidOutput(m) if m.contains("review worktree changed")),
            "{:?}",
            t.disposition
        );
        assert!(!t.passed());
        assert_eq!(git(&c.repo.root(), &["rev-parse", "HEAD"]), main_before);
        // Dirty/moved state is refused by owned cleanup and retained.
        assert!(matches!(t.cleanup, Some(Err(_))));
        assert_eq!(
            events(&t),
            [EventType::ReviewDispatched, EventType::HumanRequired]
        );
    }
}

#[test]
fn unparseable_a4_output_never_pass() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    type Mutate = fn(&crate::ReviewCapsuleNonTemporalCore) -> crate::ReviewerRunOutput;
    let cases: Vec<(&str, Mutate)> = vec![
        ("garbage", |_| output("looks good to me")),
        ("missing", |_| crate::ReviewerRunOutput {
            text: None,
            ..output("")
        }),
        ("truncated", |cap| crate::ReviewerRunOutput {
            truncated: true,
            ..output(&verdict_json(cap, "PASS", &[]))
        }),
        ("two documents", |cap| {
            let j = verdict_json(cap, "PASS", &[]);
            output(&format!("```json\n{j}\n```\n```json\n{j}\n```"))
        }),
        ("cut json", |cap| {
            let j = verdict_json(cap, "PASS", &[]);
            output(&j[..j.len() - 2])
        }),
        ("other sha", |cap| {
            output(
                &verdict_json(cap, "PASS", &[])
                    .replace(cap.implementation_sha().as_str(), &"0".repeat(40)),
            )
        }),
        ("other review", |cap| {
            output(&verdict_json(cap, "PASS", &[]).replace(cap.review_id(), "review-x"))
        }),
        ("pass with blocking", |cap| {
            output(&verdict_json(cap, "PASS", &["CORRECTNESS"]))
        }),
        ("one audit dimension of twelve", |cap| {
            output(&verdict_json(cap, "PASS", &[]).replace(
                &all_dimensions_json(),
                r#"{"dimension":"CORRECTNESS","assessment":"SATISFIED"}"#,
            ))
        }),
        ("independence denied", |cap| {
            output(&verdict_json(cap, "PASS", &[]).replace(
                r#""independence_attested":true"#,
                r#""independence_attested":false"#,
            ))
        }),
        ("unknown member", |cap| {
            output(&verdict_json(cap, "PASS", &[]).replacen('{', r#"{"accepted":true,"#, 1))
        }),
        ("not protected", |cap| crate::ReviewerRunOutput {
            protected: false,
            ..output(&verdict_json(cap, "PASS", &[]))
        }),
    ];
    for (name, make) in cases {
        let reviewer = ScriptedReviewer::new("runtime-b", move |cap| Ok(make(cap)));
        let t = run(
            &c,
            &routing,
            &[&reviewer],
            time_limit(),
            &mut MemorySink::default(),
        );
        assert!(
            matches!(t.disposition, ReviewDisposition::InvalidOutput(_)),
            "{name}: {:?}",
            t.disposition
        );
        assert!(!t.passed(), "{name}");
        assert!(!events(&t).contains(&EventType::ReviewPassed), "{name}");
    }
    // A fenced single document is accepted.
    let fenced = ScriptedReviewer::new("runtime-b", |cap| {
        Ok(output(&format!(
            "Review:\n```json\n{}\n```\n",
            verdict_json(cap, "PASS", &[])
        )))
    });
    assert!(
        run(
            &c,
            &routing,
            &[&fenced],
            time_limit(),
            &mut MemorySink::default()
        )
        .passed()
    );
    // Other runtime failures are never PASS either.
    let failing = ScriptedReviewer::new("runtime-b", |_| Err(FailureClass::Unknown));
    let t = run(
        &c,
        &routing,
        &[&failing],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(matches!(t.disposition, ReviewDisposition::InvalidOutput(_)));
}

#[test]
fn policy_blocked_reviewer_no_provider_shopping() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let blocked = ScriptedReviewer::new("runtime-b", |_| Err(FailureClass::PolicyBlocked));
    let other = pass_reviewer("runtime-c");
    let t = run(
        &c,
        &routing,
        &[&blocked, &other],
        time_limit(),
        &mut MemorySink::default(),
    );
    let ReviewDisposition::PolicyBlocked(i) = &t.disposition else {
        panic!("expected POLICY_BLOCKED, got {:?}", t.disposition);
    };
    assert_eq!(i.core().state(), SafetyInterruptionState::PolicyBlocked);
    assert_eq!(
        i.core().terminal_outcome(),
        Some(SafetyInterruptionTerminalOutcome::HumanRequired)
    );
    assert_eq!(i.core().retry_attempted(), Some(false));
    // Never claimed as an explicit provider signal: it may be Receipts' own refusal.
    assert_eq!(
        i.core().detection_confidence(),
        Some(crate::SafetyInterruptionDetectionConfidence::Unknown)
    );
    assert_eq!(i.core().provider_id(), "provider-b");
    assert_eq!(t.dispatches, 1);
    assert_eq!(routing.asked.borrow().len(), 1, "routing asked once only");
    assert!(
        other.calls.borrow().is_empty(),
        "no second provider dispatched"
    );
    assert!(!t.passed());
    assert_eq!(
        events(&t),
        [
            EventType::ReviewDispatched,
            EventType::PolicyBlocked,
            EventType::HumanRequired
        ]
    );

    // Routing-level policy refusal: nothing dispatched, still HUMAN_REQUIRED.
    let routing = FixedRouting {
        answer: Err(ReviewerRoutingRefusal::PolicyBlocked),
        asked: Default::default(),
    };
    let t = run(
        &c,
        &routing,
        &[&other],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(matches!(t.disposition, ReviewDisposition::PolicyBlocked(_)));
    assert_eq!(t.dispatches, 0);
    assert!(other.calls.borrow().is_empty());
}

#[test]
fn reproduction_mismatch_turns_pass_into_reject() {
    // Deterministic criterion fails on the real tree (status is not ok).
    let c = case(AssuranceProfile::Standard, None, "bad\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let t = run(
        &c,
        &routing,
        &[&pass_reviewer("runtime-b")],
        time_limit(),
        &mut MemorySink::default(),
    );
    let r = reviewed(&t);
    assert_eq!(r.core().verdict(), A4ReviewVerdict::Reject);
    assert!(
        r.core()
            .blocking_findings()
            .iter()
            .any(|f| f.category() == A4ReviewFindingCategory::UnreproducibleEvidence)
    );
    assert!(!t.passed());
    assert_eq!(
        events(&t),
        [
            EventType::ReviewDispatched,
            EventType::ReviewRejected,
            EventType::FindingRaised
        ]
    );

    // A worker PASS claim for a failing command does not reproduce.
    let mut c = case(AssuranceProfile::Standard, None, "ok\n");
    c.checks = vec![worker_check(
        &["false"],
        &c.implementation,
        ReviewCapsuleCheckResult::Pass,
    )];
    let t = run(
        &c,
        &routing,
        &[&pass_reviewer("runtime-b")],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert_eq!(reviewed(&t).core().verdict(), A4ReviewVerdict::Reject);

    // A timed-out reproduction is an ERROR, never a pass.
    let mut c = case(AssuranceProfile::Standard, None, "ok\n");
    c.checks = vec![worker_check(
        &["sleep", "5"],
        &c.implementation,
        ReviewCapsuleCheckResult::Pass,
    )];
    let t = run(
        &c,
        &routing,
        &[&pass_reviewer("runtime-b")],
        Duration::from_millis(300),
        &mut MemorySink::default(),
    );
    let r = reviewed(&t);
    assert_eq!(r.core().verdict(), A4ReviewVerdict::Reject);
    let repro = r.core().reproduction().unwrap().checks().unwrap();
    assert!(repro.iter().any(|c| c.timed_out() == Some(true)
        && c.result() == Some(A4ReviewReproductionCheckResult::Error)));

    // An unadmitted executable cannot run and cannot pass.
    let mut c = case(AssuranceProfile::Standard, None, "ok\n");
    c.checks = vec![worker_check(
        &["sh", "-c", "true"],
        &c.implementation,
        ReviewCapsuleCheckResult::Pass,
    )];
    let t = run(
        &c,
        &routing,
        &[&pass_reviewer("runtime-b")],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert_eq!(reviewed(&t).core().verdict(), A4ReviewVerdict::Reject);
}

#[test]
fn write_scope_violation_and_sink_failure_are_not_pass() {
    let mut repo = Repo::new();
    let implementation = repo.commit(
        &[("src/status.txt", "ok\n"), ("docs/out.md", "x\n")],
        "impl",
    );
    let c = Case {
        request: request(
            &repo.base,
            &implementation,
            AssuranceProfile::Standard,
            None,
        ),
        checks: vec![],
        implementation,
        repo,
    };
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let t = run(
        &c,
        &routing,
        &[&pass_reviewer("runtime-b")],
        time_limit(),
        &mut MemorySink::default(),
    );
    let r = reviewed(&t);
    assert_eq!(r.core().verdict(), A4ReviewVerdict::Reject);
    assert_eq!(
        r.core().unauthorized_file_changes(),
        Some(&["docs/out.md".to_string()][..])
    );

    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let mut sink = MemorySink {
        refuse: true,
        ..Default::default()
    };
    let t = run(
        &c,
        &routing,
        &[&pass_reviewer("runtime-b")],
        time_limit(),
        &mut sink,
    );
    assert!(t.persisted.is_err());
    assert!(!t.passed(), "an unpersisted PASS is not a PASS");
}

#[test]
fn unknown_runtime_and_no_reviewer_refuse_without_dispatch() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-z");
    let t = run(
        &c,
        &routing,
        &[&pass_reviewer("runtime-b")],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::UnknownRuntime(_))
    ));
    let routing = FixedRouting {
        answer: Err(ReviewerRoutingRefusal::NoEligibleReviewer),
        asked: Default::default(),
    };
    let t = run(
        &c,
        &routing,
        &[&pass_reviewer("runtime-b")],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::NoEligibleReviewer)
    ));
    assert_eq!(t.dispatches, 0);
    let _ = ReviewerCandidate {
        provider_id: String::new(),
        runtime_id: String::new(),
        model_id: None,
        resume_session: None,
        quality: None,
    };
}

// ---- Controls for F1/F3/F6/F13. They were first run red against the
// unfixed controller (see the repair evidence directory).

fn scripted(
    json: impl Fn(&crate::ReviewCapsuleNonTemporalCore) -> String + 'static,
) -> ScriptedReviewer {
    ScriptedReviewer::new("runtime-b", move |cap| Ok(output(&json(cap))))
}

#[test]
fn replay_of_a_finished_attempt_does_not_dispatch_again() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut sink = MemorySink::default();
    let _first = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    let _second = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    assert_eq!(reviewer.calls.borrow().len(), 1, "reviewer ran twice");
    assert_eq!(routing.asked.borrow().len(), 1, "routing asked twice");
}

#[test]
fn quality_floor_is_derived_and_raise_only() {
    use crate::ReviewRequestReviewerFloor as F;
    for (profile, asked, want) in [
        (AssuranceProfile::Standard, None, F::Frontier),
        (AssuranceProfile::Light, None, F::Balanced),
        (AssuranceProfile::Standard, Some(F::Economy), F::Frontier),
        (AssuranceProfile::Light, Some(F::Frontier), F::Frontier),
        (AssuranceProfile::Light, Some(F::Economy), F::Balanced),
    ] {
        let policy = asked.map(|f| ReviewRequestPolicy::new(None, None, Some(f), None));
        let c = case(profile, policy, "ok\n");
        let routing = FixedRouting::candidate("provider-b", "runtime-b");
        let reviewer = pass_reviewer("runtime-b");
        let _ = run(
            &c,
            &routing,
            &[&reviewer],
            time_limit(),
            &mut MemorySink::default(),
        );
        assert_eq!(
            routing.asked.borrow()[0].reviewer_floor,
            want,
            "{profile:?} {asked:?}"
        );
    }
}

#[test]
fn nonblocking_mandatory_category_finding_cannot_produce_a_passing_ticket() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = scripted(|cap| {
        format!(
            r#"{{"review_id":"{}","task_id":"{}","reviewed_sha":"{}","independence_attested":true,"verdict":"PASS_WITH_NONBLOCKING_FINDINGS","blocking_findings":[],"nonblocking_findings":[{{"finding_id":"n1","severity":"HIGH","category":"SECURITY_BOUNDARY_VIOLATION","description":"boundary","blocking":false}}],"dimensions":[{}]}}"#,
            cap.review_id(),
            cap.task_id(),
            cap.implementation_sha().as_str(),
            all_dimensions_json()
        )
    });
    let t = run(
        &c,
        &routing,
        &[&reviewer],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(
        !t.passed(),
        "mandatory category passed: {:?}",
        t.disposition
    );
    assert!(
        !events(&t).contains(&EventType::ReviewPassed),
        "{:?}",
        events(&t)
    );
}

#[test]
fn every_audit_dimension_must_be_assessed_and_not_applicable_is_accepted() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    // One dimension of twelve: incomplete, never a passing ticket.
    let one = scripted(|cap| {
        verdict_json(cap, "PASS", &[]).replace(
            &all_dimensions_json(),
            r#"{"dimension":"CORRECTNESS","assessment":"SATISFIED"}"#,
        )
    });
    let t = run(
        &c,
        &routing,
        &[&one],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(!t.passed());
    let ReviewDisposition::InvalidOutput(why) = &t.disposition else {
        panic!("{:?}", t.disposition)
    };
    assert!(why.contains("IncompleteDimensions"), "{why}");
    assert!(why.contains("SCOPE_COMPLIANCE"), "{why}");
    // NOT_APPLICABLE counts as assessed.
    let na = scripted(|cap| {
        verdict_json(cap, "PASS", &[]).replace(
            r#"{"dimension":"NEGATIVE_TESTS","assessment":"SATISFIED"}"#,
            r#"{"dimension":"NEGATIVE_TESTS","assessment":"NOT_APPLICABLE"}"#,
        )
    });
    let t = run(
        &c,
        &routing,
        &[&na],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(t.passed(), "{:?}", t.disposition);
}

#[test]
fn empty_reject_is_retained_and_never_becomes_acceptance() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = scripted(|cap| verdict_json(cap, "REJECT", &[]));
    let mut sink = MemorySink::default();
    let t = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    // Schema admits it and policy keeps it a REJECT: recorded, rejected SHA
    // preserved, never passing, whatever its (empty) finding arrays hold.
    assert_eq!(reviewed(&t).core().verdict(), A4ReviewVerdict::Reject);
    assert!(reviewed(&t).core().blocking_findings().is_empty());
    assert!(!t.passed());
    assert_eq!(
        events(&t),
        [EventType::ReviewDispatched, EventType::ReviewRejected]
    );
    assert_eq!(sink.proposals[0].decision_records.len(), 1);
}

#[test]
fn finding_raised_events_identify_each_finding() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = scripted(|cap| {
        let f = |id: &str| {
            format!(
                r#"{{"finding_id":"{id}","severity":"LOW","category":"STYLE","description":"d","blocking":false}}"#
            )
        };
        format!(
            r#"{{"review_id":"{}","task_id":"{}","reviewed_sha":"{}","independence_attested":true,"verdict":"PASS_WITH_NONBLOCKING_FINDINGS","blocking_findings":[],"nonblocking_findings":[{},{}],"dimensions":[{}]}}"#,
            cap.review_id(),
            cap.task_id(),
            cap.implementation_sha().as_str(),
            f("a"),
            f("b"),
            all_dimensions_json()
        )
    });
    let t = run(
        &c,
        &routing,
        &[&reviewer],
        time_limit(),
        &mut MemorySink::default(),
    );
    let p = t.proposal.as_ref().unwrap();
    let raised: Vec<_> = p
        .events
        .iter()
        .filter(|e| e.event_type() == EventType::FindingRaised)
        .map(|e| e.payload().digest.clone())
        .collect();
    assert_eq!(raised.len(), 2);
    assert_ne!(raised[0], raised[1], "both events carry the same payload");
    // PASS_WITH_NONBLOCKING cannot pass before its findings are dispositioned.
    assert!(!t.passed(), "undispositioned nonblocking findings passed");
}

// ---- F1/F2: durable claim before execution; replay recovers, never re-runs.

fn calls(r: &ScriptedReviewer) -> usize {
    r.calls.borrow().len()
}

#[test]
fn replay_recovers_the_recorded_disposition_without_executing_anything() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut sink = MemorySink::default();
    let first = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    assert!(first.passed());
    let second = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    assert_eq!(calls(&reviewer), 1, "reviewer ran twice");
    assert_eq!(routing.asked.borrow().len(), 1, "routing asked twice");
    assert_eq!(second.dispatches, 0);
    assert!(second.cleanup.is_none(), "no worktree was provisioned");
    let ReviewDisposition::Replayed(recorded) = &second.disposition else {
        panic!("{:?}", second.disposition)
    };
    assert_eq!(recorded.outcome, "REVIEWED");
    assert_eq!(
        recorded.review.as_ref().unwrap().core().review_id(),
        reviewed(&first).core().review_id()
    );
    assert!(second.passed(), "the recovered PASS is still a PASS");
    assert_eq!(sink.proposals.len(), 1, "published exactly once");
}

#[test]
fn no_claim_means_nothing_executes() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut sink = MemorySink {
        refuse: true,
        ..Default::default()
    };
    let t = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::Records(_))
    ));
    assert!(t.persisted.is_err() && !t.passed());
    assert_eq!(calls(&reviewer), 0);
    assert!(routing.asked.borrow().is_empty(), "routed before claiming");
    assert!(!c.repo.scratch("review-wt").exists());
}

#[test]
fn seal_failure_never_leads_to_a_second_execution() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut sink = MemorySink {
        fail_seal: true,
        ..Default::default()
    };
    let first = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    assert!(first.persisted.is_err() && !first.passed());
    // The retry finds the claim without a sealed result: safe refusal.
    sink.fail_seal = false;
    let retry = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    assert!(matches!(
        retry.disposition,
        ReviewDisposition::Refused(ReviewRefusal::ClaimInFlight)
    ));
    assert!(!retry.passed());
    assert_eq!(calls(&reviewer), 1);
    assert_eq!(routing.asked.borrow().len(), 1);
    assert!(sink.proposals.is_empty(), "nothing partial was published");
}

#[test]
fn publish_failure_is_completed_from_the_sealed_package() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut sink = MemorySink {
        fail_publish: 1,
        ..Default::default()
    };
    let first = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    assert!(first.persisted.is_err() && !first.passed());
    assert!(sink.proposals.is_empty());
    let retry = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    assert!(retry.persisted.is_ok(), "{:?}", retry.persisted);
    assert!(retry.passed());
    assert_eq!(calls(&reviewer), 1);
    assert_eq!(sink.proposals.len(), 1);
    // The published package is exactly the one the first run sealed.
    assert_eq!(
        sink.proposals[0].to_journal(),
        first.proposal.unwrap().to_journal()
    );
}

#[test]
fn policy_blocked_survives_repeated_callbacks_without_shopping() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let blocked = ScriptedReviewer::new("runtime-b", |_| Err(FailureClass::PolicyBlocked));
    let mut sink = MemorySink::default();
    let first = run(&c, &routing, &[&blocked], time_limit(), &mut sink);
    assert!(matches!(
        first.disposition,
        ReviewDisposition::PolicyBlocked(_)
    ));
    for _ in 0..3 {
        let again = run(&c, &routing, &[&blocked], time_limit(), &mut sink);
        let ReviewDisposition::Replayed(r) = &again.disposition else {
            panic!("{:?}", again.disposition)
        };
        assert_eq!(r.outcome, "POLICY_BLOCKED");
        assert!(!again.passed());
    }
    assert_eq!(calls(&blocked), 1);
    assert_eq!(routing.asked.borrow().len(), 1, "no second route/provider");
    assert_eq!(sink.proposals.len(), 1);
}

#[test]
fn sealed_journal_rebuilds_the_identical_package_and_refuses_tampering() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut sink = MemorySink::default();
    let t = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    let package = t.proposal.unwrap();
    let again = crate::ReviewRecordProposal::from_journal(&package.to_journal()).unwrap();
    assert_eq!(again.to_journal(), package.to_journal());
    assert_eq!(again.event_keys, package.event_keys);
    assert_eq!(again.content, package.content);
    assert_eq!(again.events.len(), package.events.len());
    // Sealed bytes round-trip, and any edit is refused.
    let sealed = crate::SealedReview::new(&package, "REVIEWED", true, 1);
    let bytes = sealed.to_bytes();
    assert!(crate::SealedReview::from_bytes(&bytes).is_ok());
    let text = String::from_utf8(bytes).unwrap();
    let edited = text.replacen("task-1", "task-2", 1);
    assert_ne!(edited, text, "the edit changed nothing");
    assert!(crate::SealedReview::from_bytes(edited.as_bytes()).is_err());
    // An event whose payload bytes were swapped is refused on rebuild.
    let journal = String::from_utf8(package.to_journal()).unwrap();
    let first = package.content[0].1.clone();
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let mut altered = first.clone();
    altered[0] ^= 1;
    let swapped = journal.replacen(&hex(&first), &hex(&altered), 1);
    assert_ne!(swapped, journal);
    assert!(crate::ReviewRecordProposal::from_journal(swapped.as_bytes()).is_err());
}

// ---- F3: the quality floor is derived, raise-only and evidenced.

fn candidate_with(tier: Option<crate::ReviewRequestReviewerFloor>, basis: &str) -> FixedRouting {
    let mut routing = FixedRouting::candidate("provider-b", "runtime-b");
    routing.answer = routing.answer.map(|mut c| {
        c.quality = tier.map(|tier| crate::ReviewerQualityEvidence {
            tier,
            basis_ref: basis.into(),
        });
        c
    });
    routing
}

#[test]
fn candidate_quality_evidence_must_meet_the_derived_floor() {
    use crate::ReviewRequestReviewerFloor as F;
    // (profile, request floor, evidenced tier, basis) -> refusal?
    let cases = [
        (AssuranceProfile::Standard, None, None, "b", "missing"),
        (
            AssuranceProfile::Standard,
            None,
            Some(F::Balanced),
            "b",
            "below",
        ),
        (
            AssuranceProfile::Standard,
            None,
            Some(F::Economy),
            "b",
            "below",
        ),
        (
            AssuranceProfile::Standard,
            Some(F::Economy),
            Some(F::Economy),
            "b",
            "below",
        ),
        (
            AssuranceProfile::Standard,
            None,
            Some(F::Frontier),
            " ",
            "missing",
        ),
        (
            AssuranceProfile::Light,
            None,
            Some(F::Economy),
            "b",
            "below",
        ),
        (
            AssuranceProfile::Light,
            Some(F::Frontier),
            Some(F::Balanced),
            "b",
            "below",
        ),
        (
            AssuranceProfile::Standard,
            None,
            Some(F::Frontier),
            "b",
            "ok",
        ),
        (AssuranceProfile::Light, None, Some(F::Balanced), "b", "ok"),
        (
            AssuranceProfile::Light,
            Some(F::Economy),
            Some(F::Balanced),
            "b",
            "ok",
        ),
        (
            AssuranceProfile::Light,
            Some(F::Frontier),
            Some(F::Frontier),
            "b",
            "ok",
        ),
    ];
    for (profile, asked, tier, basis, want) in cases {
        let policy = asked.map(|f| ReviewRequestPolicy::new(None, None, Some(f), None));
        let c = case(profile, policy, "ok\n");
        let routing = candidate_with(tier, basis);
        let reviewer = pass_reviewer("runtime-b");
        let t = run(
            &c,
            &routing,
            &[&reviewer],
            time_limit(),
            &mut MemorySink::default(),
        );
        let label = format!("{profile:?} {asked:?} {tier:?} {basis:?}");
        match (want, &t.disposition) {
            ("missing", ReviewDisposition::Refused(ReviewRefusal::QualityEvidenceMissing))
            | ("below", ReviewDisposition::Refused(ReviewRefusal::QualityBelowFloor { .. })) => {
                assert_eq!(t.dispatches, 0, "{label}");
                assert_eq!(calls(&reviewer), 0, "{label}: dispatched without evidence");
                assert!(!t.passed(), "{label}");
            }
            ("ok", ReviewDisposition::Reviewed(_)) => assert!(t.passed(), "{label}"),
            (_, other) => panic!("{label}: {other:?}"),
        }
    }
}

// ---- F6: policy is applied before any success is produced.

fn nonblocking_json(cap: &crate::ReviewCapsuleNonTemporalCore, categories: &[&str]) -> String {
    let findings: Vec<String> = categories
        .iter()
        .enumerate()
        .map(|(n, cat)| {
            format!(
                r#"{{"finding_id":"n{n}","severity":"LOW","category":"{cat}","description":"d","blocking":false}}"#
            )
        })
        .collect();
    format!(
        r#"{{"review_id":"{}","task_id":"{}","reviewed_sha":"{}","independence_attested":true,"verdict":"PASS_WITH_NONBLOCKING_FINDINGS","blocking_findings":[],"nonblocking_findings":[{}],"dimensions":[{}]}}"#,
        cap.review_id(),
        cap.task_id(),
        cap.implementation_sha().as_str(),
        findings.join(","),
        all_dimensions_json()
    )
}

#[test]
fn mandatory_categories_are_blocking_whatever_the_reviewer_flagged() {
    let mandatory = [
        "ARCHITECTURE_VIOLATION",
        "CONTRACT_VIOLATION",
        "SECURITY_BOUNDARY_VIOLATION",
        "WRITE_SCOPE_VIOLATION",
        "UNDISCLOSED_CHANGE",
        "UNREPRODUCIBLE_EVIDENCE",
        "OVERSTATED_LABEL",
        "TEST_WEAKENED_OR_DELETED",
        "MISSING_REQUIRED_NEGATIVE_TEST",
    ];
    for category in mandatory {
        let c = case(AssuranceProfile::Standard, None, "ok\n");
        let routing = FixedRouting::candidate("provider-b", "runtime-b");
        let reviewer = scripted(move |cap| nonblocking_json(cap, &[category, "STYLE"]));
        let mut sink = MemorySink::default();
        let t = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
        assert!(!t.passed(), "{category} passed");
        let r = reviewed(&t).core();
        assert_eq!(r.verdict(), A4ReviewVerdict::Reject, "{category}");
        assert_eq!(r.blocking_findings().len(), 1, "{category}");
        assert_eq!(r.blocking_findings()[0].category().as_str(), category);
        assert!(r.blocking_findings()[0].blocking());
        // The truly nonblocking finding stays nonblocking.
        assert_eq!(r.nonblocking_findings().len(), 1);
        let kinds = events(&t);
        assert!(kinds.contains(&EventType::ReviewRejected), "{category}");
        assert!(!kinds.contains(&EventType::ReviewPassed), "{category}");
        assert_eq!(
            kinds
                .iter()
                .filter(|k| **k == EventType::FindingRaised)
                .count(),
            2
        );
        // The stored record says REJECT too.
        assert!(
            sink.proposals[0].decision_records[0]
                .record_json()
                .contains("\"verdict\":\"REJECT\"")
        );
    }
}

#[test]
fn nonblocking_ordinary_findings_stay_pending_until_dispositioned() {
    use crate::{FindingDisposition, FindingDispositionKind, disposition_proposal};
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = scripted(|cap| nonblocking_json(cap, &["STYLE", "NAMING"]));
    let mut sink = MemorySink::default();
    let t = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    let r = reviewed(&t).core().clone();
    assert_eq!(r.verdict(), A4ReviewVerdict::PassWithNonblockingFindings);
    assert!(!t.passed(), "undispositioned findings progressed");
    assert_eq!(t.pending_dispositions(), ["n0", "n1"]);
    // One FINDING_RAISED per finding, each with its own payload.
    let p = t.proposal.as_ref().unwrap();
    let raised: Vec<_> = p
        .events
        .iter()
        .filter(|e| e.event_type() == EventType::FindingRaised)
        .collect();
    assert_eq!(raised.len(), 2);
    assert_ne!(raised[0].payload().digest, raised[1].payload().digest);
    assert!(
        p.content.iter().any(|(r, b)| r.ends_with(":n0")
            && String::from_utf8_lossy(b).contains("\"finding_id\":\"n0\""))
    );
    // Explicit dispositions complete it: one DISPOSITIONED event per finding.
    let dispositions = vec![
        FindingDisposition {
            finding_id: "n0".into(),
            kind: FindingDispositionKind::FixNow {
                task_id: "task-2".into(),
            },
            decided_by: "a2".into(),
        },
        FindingDisposition {
            finding_id: "n1".into(),
            kind: FindingDispositionKind::Defer {
                issue_id: "ISSUE-9".into(),
                revisit_point: "wave 3".into(),
            },
            decided_by: "a2".into(),
        },
    ];
    let package = disposition_proposal("project", &c.request, &r, &dispositions, 1).unwrap();
    let kinds: Vec<_> = package.events.iter().map(|e| e.event_type()).collect();
    assert_eq!(
        kinds,
        [
            EventType::FindingDispositioned,
            EventType::FindingDispositioned
        ]
    );
    assert_ne!(
        package.events[0].payload().digest,
        package.events[1].payload().digest
    );
}

// ---- F9 through reviewer-output parsing.

#[test]
fn reviewer_output_with_a_signed_unicode_escape_is_invalid_output() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = scripted(|cap| {
        verdict_json(cap, "PASS", &[]).replacen(
            r#""independence_attested":true"#,
            r#""rationale":"\u+041","independence_attested":true"#,
            1,
        )
    });
    let t = run(
        &c,
        &routing,
        &[&reviewer],
        time_limit(),
        &mut MemorySink::default(),
    );
    assert!(matches!(t.disposition, ReviewDisposition::InvalidOutput(_)));
    assert!(!t.passed());
}

// ---- Typed claim scope, operation binding, generation and claim states.

fn binding_of(
    c: &Case,
    implementer: &ImplementerIdentity,
    generation: u32,
) -> Result<crate::ReviewClaimBinding, String> {
    let root = c.repo.root();
    let worktree = c.repo.scratch("unused");
    review_claim_binding(&AttemptFinished {
        project_id: "project",
        request: &c.request,
        repository_root: &root,
        implementer,
        worker_checks: &c.checks,
        review_worktree: &worktree,
        generation,
    })
}

#[test]
fn claim_scope_is_typed_and_covers_the_semantic_request_and_generation() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let b1 = binding_of(&c, &implementer(), 1).unwrap();
    assert_eq!(b1, binding_of(&c, &implementer(), 1).unwrap());
    let scope = b1.scope();
    assert_eq!(b1.operation(), crate::ReviewOperation::Review);
    assert_eq!(scope.project_id(), "project");
    assert_eq!(scope.task_id(), c.request.core().task_id());
    assert_eq!(scope.subject_sha(), Some(c.implementation.as_str()));
    assert_eq!(scope.generation(), 1);
    assert_eq!(scope.request_sha256().len(), 64);
    // The key is derived from the scope and operation, never parsed back.
    assert_eq!(
        crate::ReviewClaimBinding::from_parts(b1.operation(), scope.clone(), b1.key()).unwrap(),
        b1
    );
    assert!(
        crate::ReviewClaimBinding::from_parts(b1.operation(), scope.clone(), "review-claim-x")
            .is_err()
    );
    // Generation is part of the claim, and only the composition supplies it.
    let b2 = binding_of(&c, &implementer(), 2).unwrap();
    assert_ne!(b1.key(), b2.key());
    assert_eq!(b2.scope().request_sha256(), scope.request_sha256());
    assert!(binding_of(&c, &implementer(), 0).is_err());
    // The same request id with different semantics is a different claim.
    let policy = ReviewRequestPolicy::new(None, Some(DistinctProviderPolicy::Required), None, None);
    let stricter = case(AssuranceProfile::Standard, Some(policy), "ok\n");
    assert_eq!(
        stricter.request.core().request_id(),
        c.request.core().request_id()
    );
    let other = binding_of(&stricter, &implementer(), 1).unwrap();
    assert_ne!(other.scope().request_sha256(), scope.request_sha256());
    assert_ne!(other.key(), b1.key());
    // So is another implementer identity.
    let mut changed = implementer();
    changed.provider_id = "provider-z".into();
    assert_ne!(
        binding_of(&c, &changed, 1)
            .unwrap()
            .scope()
            .request_sha256(),
        scope.request_sha256()
    );
    // `requested_at` says when, not what: a redelivered callback is the same work.
    let late = crate::ReviewRequest::new(c.request.core().clone(), Some(clock()));
    let redelivered = Case {
        repo: Repo::new(),
        implementation: c.implementation.clone(),
        request: late,
        checks: c.checks.clone(),
    };
    assert_eq!(
        binding_of(&redelivered, &implementer(), 1).unwrap().key(),
        b1.key()
    );
}

#[test]
fn an_invalid_scope_refuses_before_any_claim_routing_or_dispatch() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut sink = MemorySink::default();
    let t = run_generation(&c, &routing, &[&reviewer], time_limit(), &mut sink, 0);
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::Scope(_))
    ));
    assert!(sink.claims.is_empty(), "no claim was taken");
    assert!(reviewer.calls.borrow().is_empty());
    assert!(routing.asked.borrow().is_empty());
}

#[test]
fn terminal_and_abandoned_claims_never_authorize_execution() {
    for abandoned in [false, true] {
        let c = case(AssuranceProfile::Standard, None, "ok\n");
        let routing = FixedRouting::candidate("provider-b", "runtime-b");
        let reviewer = pass_reviewer("runtime-b");
        let key = binding_of(&c, &implementer(), 1).unwrap().key().to_owned();
        let mut sink = MemorySink::default();
        if abandoned {
            sink.abandoned.insert(key);
        } else {
            sink.terminal.insert(key);
        }
        let t = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
        assert!(
            matches!(
                (&t.disposition, abandoned),
                (
                    ReviewDisposition::Refused(ReviewRefusal::ClaimAbandoned),
                    true
                ) | (
                    ReviewDisposition::Refused(ReviewRefusal::ClaimTerminal(_)),
                    false
                )
            ),
            "{:?}",
            t.disposition
        );
        assert!(t.persisted.is_err() && !t.passed());
        assert!(reviewer.calls.borrow().is_empty(), "no execution");
        assert!(routing.asked.borrow().is_empty());
    }
}

#[test]
fn a_new_generation_runs_the_work_again_and_the_same_generation_never_does() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut sink = MemorySink::default();
    run_generation(&c, &routing, &[&reviewer], time_limit(), &mut sink, 1);
    run_generation(&c, &routing, &[&reviewer], time_limit(), &mut sink, 1);
    assert_eq!(reviewer.calls.borrow().len(), 1);
    let again = run_generation(&c, &routing, &[&reviewer], time_limit(), &mut sink, 2);
    assert_eq!(reviewer.calls.borrow().len(), 2, "generation 2 is new work");
    assert!(again.persisted.is_ok());
    assert_eq!(sink.claims.len(), 2);
}

#[test]
fn a_sealed_package_for_another_scope_is_never_published() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut other = MemorySink::default();
    let t = run_generation(&c, &routing, &[&reviewer], time_limit(), &mut other, 2);
    let foreign = SealedReview::new(t.proposal.as_ref().unwrap(), "REVIEWED", true, 1);
    let key1 = binding_of(&c, &implementer(), 1).unwrap().key().to_owned();
    let mut sink = MemorySink::default();
    sink.claims.insert(key1, Some(foreign));
    let before = reviewer.calls.borrow().len();
    let t = run_generation(&c, &routing, &[&reviewer], time_limit(), &mut sink, 1);
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::SealedScopeMismatch)
    ));
    assert!(sink.proposals.is_empty(), "nothing was published");
    assert_eq!(reviewer.calls.borrow().len(), before, "nothing executed");
}

#[test]
fn a_sealed_outcome_must_belong_to_its_operation_and_its_records() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let mut sink = MemorySink::default();
    let t = run(&c, &routing, &[&reviewer], time_limit(), &mut sink);
    let proposal = t.proposal.unwrap();
    let good = SealedReview::new(&proposal, "REVIEWED", true, 1);
    assert_eq!(good.settlement().unwrap().outcome_kind(), "PASS");
    assert!(good.settlement().unwrap().is_completed());
    // Another operation's label, a label that contradicts the records and a
    // refusal label over a reviewed package are all refused.
    for label in [
        "DISPOSITIONS",
        "REPAIR_ISSUED",
        "INVALID_OUTPUT",
        "POLICY_BLOCKED",
    ] {
        assert!(
            SealedReview::new(&proposal, label, true, 1)
                .settlement()
                .is_err(),
            "{label}"
        );
        assert!(
            SealedReview::from_bytes(&SealedReview::new(&proposal, label, true, 1).to_bytes())
                .is_err()
        );
    }
    // Bytes that do not round-trip are refused: a flipped scope field breaks
    // the digest, and a digest repaired over a wrong key breaks the binding.
    let bytes = good.to_bytes();
    assert!(SealedReview::from_bytes(&bytes).is_ok());
    let text = String::from_utf8(bytes).unwrap();
    let flipped = text.replacen("project", "projecx", 1);
    assert!(SealedReview::from_bytes(flipped.as_bytes()).is_err());
}

/// Re-signs a sealed review after `edit` changed its journal: the journal
/// digest is repaired, so only the package's own validators can refuse it.
fn resigned(sealed: &SealedReview, edit: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut outer: serde_json::Value = serde_json::from_slice(&sealed.to_bytes()).unwrap();
    let mut journal: serde_json::Value =
        serde_json::from_str(outer["journal"].as_str().unwrap()).unwrap();
    edit(&mut journal);
    let text = journal.to_string();
    outer["journal_sha256"] = crate::records::sha256_hex(text.as_bytes()).into();
    outer["journal"] = text.into();
    serde_json::to_vec(&outer).unwrap()
}

#[test]
fn a_resigned_sealed_package_that_contradicts_its_scope_or_content_is_refused() {
    let c = case(AssuranceProfile::Standard, None, "ok\n");
    let routing = FixedRouting::candidate("provider-b", "runtime-b");
    let reviewer = pass_reviewer("runtime-b");
    let t = run(
        &c,
        &routing,
        &[&reviewer],
        time_limit(),
        &mut MemorySink::default(),
    );
    let good = SealedReview::new(t.proposal.as_ref().unwrap(), "REVIEWED", true, 1);
    assert!(
        SealedReview::from_bytes(&resigned(&good, |_| {})).is_ok(),
        "control"
    );

    let other_sha = "1".repeat(40);
    type Edit = Box<dyn FnOnce(&mut serde_json::Value)>;
    let edits: Vec<(&str, Edit)> = vec![
        (
            "key",
            Box::new(|j| j["binding"]["key"] = "review-claim-x".into()),
        ),
        (
            "operation",
            Box::new(|j| j["binding"]["operation"] = "REPAIR".into()),
        ),
        (
            "generation",
            Box::new(|j| j["binding"]["generation"] = 2.into()),
        ),
        (
            "zero generation",
            Box::new(|j| j["binding"]["generation"] = 0.into()),
        ),
        (
            "request digest",
            Box::new(|j| j["binding"]["request_sha256"] = "2".repeat(64).into()),
        ),
        (
            "project",
            Box::new(|j| j["binding"]["project_id"] = "other".into()),
        ),
        (
            "decision json",
            Box::new(|j| {
                let text = j["decisions"][0]["json"]
                    .as_str()
                    .unwrap()
                    .replace("PASS", "REJECT");
                j["decisions"][0]["json"] = text.into();
            }),
        ),
        (
            "event content",
            Box::new(|j| j["content"][0]["hex"] = "00".repeat(8).into()),
        ),
        (
            "dropped record",
            Box::new(|j| j["decisions"] = serde_json::json!([])),
        ),
        (
            "unknown event type",
            Box::new(|j| j["events"][0]["event_type"] = "NOT_AN_EVENT".into()),
        ),
    ];
    for (name, edit) in edits {
        assert!(
            SealedReview::from_bytes(&resigned(&good, edit)).is_err(),
            "{name} was accepted"
        );
    }
    // A consistent binding for a different subject (key recomputed) still
    // contradicts the sealed A4Review record's reviewed SHA.
    let scope = good.binding().unwrap().scope().clone();
    let moved = crate::ReviewClaimScope::new(
        scope.project_id(),
        scope.task_id(),
        Some(&other_sha),
        scope.request_sha256(),
        scope.generation(),
    )
    .unwrap();
    let rebound = crate::ReviewClaimBinding::new(crate::ReviewOperation::Review, moved.clone());
    let bytes = resigned(&good, |j| {
        j["binding"]["subject_sha"] = other_sha.clone().into();
        j["binding"]["key"] = rebound.key().into();
    });
    assert!(SealedReview::from_bytes(&bytes).is_err(), "foreign subject");
}
