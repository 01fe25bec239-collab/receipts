//! The repair and disposition paths publish through the same real Core
//! sink as the review path: one claim per decision, the sealed package
//! published atomically and idempotently, live and after a restart. No
//! Review path reaches State any other way.

mod common;

use common::*;
use receipts_review_integration::state::{DecisionRecordKind, EventType, ReviewClaimStateV1 as S};
use receipts_review_integration::{
    A3HandoffCheck, A3HandoffCheckResult, A3HandoffNonTemporalCore, FindingDisposition,
    FindingDispositionKind, Recorded, RepairController, RepairDecision, RepairInput,
    ReviewClaimBinding, ReviewDisposition, ReviewRecordSink, ReviewTicket, disposition_proposal,
    record_dispositions, record_proposal,
};
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
};

fn handoff(f: &Fixture) -> A3HandoffNonTemporalCore {
    let check = A3HandoffCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            vec!["true".into()],
            0,
            CommitSha::parse(&f.implementation).unwrap(),
            None,
            None,
        )
        .unwrap(),
        Some(A3HandoffCheckResult::Pass),
    );
    A3HandoffNonTemporalCore::new(
        "task".into(),
        "attempt".into(),
        f.request.core().baseline_sha().as_str().into(),
        f.implementation.clone(),
        "b".into(),
        vec![],
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

fn claim_of(
    sink: &receipts_e2e::CoreReviewSink,
    b: &ReviewClaimBinding,
) -> receipts_review_integration::state::ReviewClaimRecordV1 {
    image(sink, b, "x").claim.expect("claim row")
}

#[test]
fn repair_decisions_publish_atomically_idempotently_and_after_restart() {
    let f = fixture("repair");
    let clock = Clock::default();
    let reviewer = Synthetic::rejecting();
    let mut sink = Faulty::open(&f.db, &clock);
    let rejected = run_attempt(&f, &mut sink, &reviewer, 1);
    assert!(!rejected.passed() && rejected.persisted.is_ok());
    let review = rejected.review().unwrap().core().clone();
    let handoffs = [handoff(&f)];
    let input = |generation| RepairInput {
        project_id: PROJECT,
        request: &f.request,
        handoffs: &handoffs,
        reviews: std::slice::from_ref(&review),
        completed_repair_attempts: 0,
        interruption: None,
        generation,
    };
    let outcome = RepairController::decide(&input(1)).unwrap();
    assert!(matches!(outcome.decision, RepairDecision::Issue(_)));
    let b = outcome.proposal.as_ref().unwrap().binding().clone();
    assert_ne!(b.key(), binding(&f, 1).key(), "a repair is its own claim");
    let before = image(&sink.inner, &b, "x").event_ids;

    // A fault inside State's publication leaves the repair sealed, never
    // partial; the same call after a restart completes it.
    sink.fault = Fault::PublishClock;
    assert!(outcome.record(&mut sink).is_err());
    let sealed = image(&sink.inner, &b, "x");
    assert_eq!(sealed.claim.as_ref().unwrap().state, S::Claimed);
    assert!(sealed.seal.is_some() && sealed.event_ids == before);
    drop(sink);

    let mut sink = Faulty::open(&f.db, &clock);
    assert_eq!(outcome.record(&mut sink), Ok(Recorded::Published));
    let claim = claim_of(&sink.inner, &b);
    assert_eq!(claim.state, S::Completed);
    assert_eq!(claim.outcome_kind.as_deref(), Some("REPAIR_ISSUED"));
    let published = image(&sink.inner, &b, "x");
    assert_eq!(published.event_ids.len(), before.len() + 1);
    // Exact replay changes nothing.
    assert_eq!(outcome.record(&mut sink), Ok(Recorded::Published));
    assert_eq!(image(&sink.inner, &b, "x"), published);
    // A new generation is a separate claim, but State refuses to publish the
    // identical event a second time: generations exist to rerun work that
    // never published, not to duplicate a published fact. The refusal writes
    // nothing and leaves that claim sealed and unfinished.
    let g2 = RepairController::decide(&input(2)).unwrap();
    let b2 = g2.proposal.as_ref().unwrap().binding().clone();
    assert_ne!(b2.key(), b.key());
    let refusal = g2.record(&mut sink).unwrap_err();
    assert!(refusal.contains("ALREADY_EXISTS"), "{refusal}");
    let stuck = image(&sink.inner, &b2, "x");
    assert_eq!(stuck.claim.as_ref().unwrap().state, S::Claimed);
    assert!(stuck.seal.is_some());
    assert_eq!(stuck.event_ids, published.event_ids, "nothing was written");
    drop(sink);
    let issued = stored_event_types(&f.db)
        .into_iter()
        .filter(|t| *t == EventType::RepairIssued)
        .count();
    assert_eq!(issued, 1, "published once");

    // A safety interruption escalates (no repair dispatch) and records too.
    let g = fixture("repair-escalate");
    let mut sink = Faulty::open(&g.db, &clock);
    let blocked = Synthetic {
        policy_blocked: true,
        ..Synthetic::default()
    };
    let t = run_attempt(&g, &mut sink, &blocked, 1);
    let ReviewDisposition::PolicyBlocked(interruption) = &t.disposition else {
        panic!("{:?}", t.disposition)
    };
    let escalate = RepairController::decide(&RepairInput {
        project_id: PROJECT,
        request: &g.request,
        handoffs: &[],
        reviews: &[],
        completed_repair_attempts: 0,
        interruption: Some(interruption),
        generation: 1,
    })
    .unwrap();
    assert!(matches!(
        escalate.decision,
        RepairDecision::HumanRequired { .. }
    ));
    assert_eq!(escalate.record(&mut sink), Ok(Recorded::Published));
    let eb = escalate.proposal.as_ref().unwrap().binding().clone();
    assert_eq!(
        claim_of(&sink.inner, &eb).outcome_kind.as_deref(),
        Some("REPAIR_ESCALATED")
    );
    // No repair needed: nothing to claim, nothing in State.
    let pass_f = fixture("repair-none");
    let mut pass_sink = Faulty::open(&pass_f.db, &clock);
    let passed = run_attempt(&pass_f, &mut pass_sink, &Synthetic::default(), 1);
    let none = RepairController::decide(&RepairInput {
        project_id: PROJECT,
        request: &pass_f.request,
        handoffs: &[handoff(&pass_f)],
        reviews: &[passed.review().unwrap().core().clone()],
        completed_repair_attempts: 0,
        interruption: None,
        generation: 1,
    })
    .unwrap();
    assert!(matches!(none.decision, RepairDecision::NotRequired));
    assert_eq!(none.record(&mut pass_sink), Ok(Recorded::NothingToRecord));
}

fn passed_with_findings(f: &Fixture, sink: &mut dyn ReviewRecordSink) -> ReviewTicket {
    let reviewer = Synthetic {
        nonblocking: true,
        ..Synthetic::default()
    };
    let t = run_attempt(f, sink, &reviewer, 1);
    assert_eq!(t.pending_dispositions(), ["n1"], "{:?}", t.disposition);
    assert!(!t.passed(), "pending until dispositioned");
    t
}

fn defer() -> FindingDisposition {
    FindingDisposition {
        finding_id: "n1".into(),
        kind: FindingDispositionKind::Defer {
            issue_id: "ISSUE-1".into(),
            revisit_point: "wave3".into(),
        },
        decided_by: "a2".into(),
    }
}

#[test]
fn finding_dispositions_publish_through_the_same_sink_and_conflicts_are_refused() {
    let f = fixture("dispositions");
    let clock = Clock::default();
    let mut sink = Faulty::open(&f.db, &clock);
    let t = passed_with_findings(&f, &mut sink);
    let review = t.review().unwrap().core().clone();
    let ds = [defer()];
    let p = disposition_proposal(PROJECT, &f.request, &review, &ds, 1).unwrap();
    let b = p.binding().clone();

    sink.fault = Fault::PublishLost;
    assert!(record_dispositions(&mut sink, &p).is_err());
    assert!(image(&sink.inner, &b, "x").seal.is_some());
    drop(sink);
    let mut sink = Faulty::open(&f.db, &clock);
    assert_eq!(record_dispositions(&mut sink, &p), Ok(Recorded::Published));
    let claim = claim_of(&sink.inner, &b);
    assert_eq!(
        (claim.state, claim.outcome_kind.as_deref()),
        (S::Completed, Some("DISPOSITIONS"))
    );
    let done = image(&sink.inner, &b, "x");
    assert_eq!(record_dispositions(&mut sink, &p), Ok(Recorded::Published));
    assert_eq!(image(&sink.inner, &b, "x"), done);

    // A different decision for the same review is a different scope, so it
    // never overwrites; a label of another operation is refused unclaimed.
    let reject = FindingDisposition {
        finding_id: "n1".into(),
        kind: FindingDispositionKind::Reject {
            reason: "not applicable".into(),
        },
        decided_by: "a2".into(),
    };
    let other = disposition_proposal(PROJECT, &f.request, &review, &[reject], 1).unwrap();
    assert_ne!(other.claim_key(), p.claim_key());
    assert!(record_proposal(&mut sink.inner, &p, "REPAIR_ISSUED").is_err());
    assert_eq!(
        record_dispositions(&mut sink, &other),
        Ok(Recorded::Published)
    );
    drop(sink);
    let types = stored_event_types(&f.db);
    assert_eq!(
        types
            .iter()
            .filter(|t| **t == EventType::FindingDispositioned)
            .count(),
        2
    );
    let reader = receipts_review_integration::state::StateReader::open_existing(&f.db).unwrap();
    assert!(
        reader
            .find_decision_record(PROJECT, DecisionRecordKind::A4Review, review.review_id())
            .unwrap()
            .is_some(),
        "the review record is untouched"
    );
}
