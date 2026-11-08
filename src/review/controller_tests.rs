//! Controller controls over real temporary repositories, real Workspace
//! review worktrees and the real argv runner. The reviewer itself is a
//! scripted synthetic runtime: these prove controller behaviour, not
//! provider or native protected-launch execution.

use crate::controller::*;
use crate::ports::{ImplementerIdentity, ReviewerCandidate, ReviewerRoutingRefusal};
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
        ("reject without blocking", |cap| {
            output(&verdict_json(cap, "REJECT", &[]))
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
    };
}
