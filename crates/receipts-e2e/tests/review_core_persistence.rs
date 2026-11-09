//! Review persists only through the real trusted core and real SQLite State.
//!
//! `CoreReviewSink` (the library-integration adapter over `CoreStateWriter`)
//! is the only sink here: the claim, the exact sealed recovery bytes and the
//! published records live in a temporary State store, a restart is a fresh
//! `CoreStateWriter` over the same file, and the controller's reviewer is a
//! synthetic runtime (not provider proof). Faults are injected at the sink
//! (a transport loss before the core is reached) and at State's own clock
//! (a failure inside the core's State write).

mod common;

use common::*;
use receipts_e2e::{CoreReviewSink, ReviewSealedCodec, SealedContent};
use receipts_orchestration::orchestration::{
    CoreContentResolverV1, CorePolicyRefusal, CoreReviewClaimV1, CoreReviewUnitV1,
    CoreStateWriteError, CoreStateWriter,
};
use receipts_review_integration::runtime::FailureClass;
use receipts_review_integration::state::{
    DecisionRecordKind, EventType, ReviewClaimStateV1 as S, StateReader,
};
use receipts_review_integration::{
    ReviewCapsuleNonTemporalCore, ReviewClaim, ReviewClaimBinding, ReviewDisposition,
    ReviewRecordProposal, ReviewRecordSink, ReviewRefusal, ReviewTicket, ReviewerRoutingRefusal,
    ReviewerRunOutput, ReviewerRuntime, SealedReview,
};
use receipts_workspace_execution::WorkspaceHandle;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

fn reviewed_id(t: &ReviewTicket) -> String {
    t.review().unwrap().core().review_id().to_owned()
}

fn reviewed_events() -> Vec<EventType> {
    expected([EventType::ReviewDispatched, EventType::ReviewPassed])
}

#[test]
fn review_persists_through_claim_seal_publish_and_replay_executes_nothing() {
    let f = fixture("ok");
    let clock = Clock::default();
    let mut sink = open_sink(&f.db, &clock);
    let reviewer = Synthetic::default();
    let ticket = run_attempt(&f, &mut sink, &reviewer, 1);
    assert!(
        ticket.passed(),
        "{:?} {:?}",
        ticket.disposition,
        ticket.persisted
    );
    let (review_id, b) = (reviewed_id(&ticket), binding(&f, 1));

    // The claim, the exact sealed bytes and the settlement are State's.
    let snap = image(&sink, &b, &review_id);
    let claim = snap.claim.clone().unwrap();
    assert_eq!(claim.state, S::Completed);
    assert_eq!(claim.outcome_kind.as_deref(), Some("PASS"));
    assert_eq!(claim.outcome_ref.as_deref(), Some(review_id.as_str()));
    assert_eq!(claim.task_id, b.scope().task_id());
    assert_eq!(claim.subject_sha.as_deref(), b.scope().subject_sha());
    assert_eq!(claim.request_sha256, b.scope().request_sha256());
    let seal = snap.seal.clone().unwrap();
    assert_eq!(seal.recovery_kind, "REVIEW_SEALED_V1");
    let sealed = SealedReview::from_bytes(&seal.recovery_data).unwrap();
    assert_eq!(sealed.binding().unwrap(), b);
    assert_eq!(sealed.outcome, "REVIEWED");
    assert_eq!(
        sealed.proposal().unwrap().to_journal(),
        ticket.proposal.as_ref().unwrap().to_journal()
    );

    // Replays and later callbacks recover the recorded result and run nothing.
    for _ in 0..2 {
        let replay = run_attempt(&f, &mut sink, &reviewer, 1);
        assert!(matches!(replay.disposition, ReviewDisposition::Replayed(_)));
        assert!(replay.persisted.is_ok() && replay.passed());
        assert_eq!(replay.dispatches, 0);
    }
    assert_eq!(reviewer.calls.get(), 1, "the reviewer ran exactly once");
    assert_eq!(image(&sink, &b, &review_id).event_ids, snap.event_ids);
    drop(sink);

    let reader = StateReader::open_existing(&f.db).unwrap();
    let record = reader
        .find_decision_record(PROJECT, DecisionRecordKind::A4Review, &review_id)
        .unwrap()
        .unwrap();
    assert_eq!(record.subject_sha(), Some(f.implementation.as_str()));
    assert!(record.record_json().contains("\"verdict\":\"PASS\""));
    assert_eq!(stored_event_types(&f.db), reviewed_events());
    assert!(
        reader
            .list_events(&query())
            .unwrap()
            .iter()
            .all(|e| e.occurred_at == STAMP),
        "trusted time, not anything Review supplied"
    );
}

/// A failure at each point of the claim → seal → publish sequence, injected
/// at the sink and at State's clock, recovers a consistent package after a
/// restart without executing the review again.
#[test]
fn publication_faults_recover_the_sealed_package_after_restart_without_re_execution() {
    for fault in [Fault::PublishLost, Fault::PublishClock] {
        let f = fixture(&format!("pub-{fault:?}"));
        let clock = Clock::default();
        let reviewer = Synthetic::default();
        let (b, review_id);
        {
            let mut sink = Faulty::open(&f.db, &clock);
            sink.fault = fault;
            let first = run_attempt(&f, &mut sink, &reviewer, 1);
            assert!(first.persisted.is_err() && !first.passed(), "{fault:?}");
            b = binding(&f, 1);
            review_id = reviewed_id(&first);
            // The seal is durable; nothing else is: no record, no event, and
            // the claim is still unfinished (atomic: no partial publication).
            let snap = image(&sink.inner, &b, &review_id);
            assert!(snap.seal.is_some());
            assert_eq!(snap.claim.as_ref().unwrap().state, S::Claimed);
            assert!(snap.event_ids.is_empty() && snap.decision_sha.is_none());
        } // the process "restarts": the writer and its live ticket are gone

        let mut sink = open_sink(&f.db, &clock);
        let retry = run_attempt(&f, &mut sink, &reviewer, 1);
        assert_eq!(reviewer.calls.get(), 1, "{fault:?}: executed again");
        assert_eq!(retry.dispatches, 0);
        assert!(retry.persisted.is_ok(), "{fault:?}: {:?}", retry.persisted);
        assert!(retry.passed(), "{fault:?}: {:?}", retry.disposition);
        let done = image(&sink, &b, &review_id);
        assert_eq!(done.claim.as_ref().unwrap().state, S::Completed);
        assert_eq!(done.event_ids.len(), 2);
        // Another callback changes nothing.
        let again = run_attempt(&f, &mut sink, &reviewer, 1);
        assert!(again.persisted.is_ok() && again.passed());
        assert_eq!(image(&sink, &b, &review_id), done);
        assert_eq!(reviewer.calls.get(), 1);
        drop(sink);
        assert_eq!(stored_event_types(&f.db), reviewed_events(), "{fault:?}");
    }
}

/// An unsealed claim is uncertain: nothing executes it again, across a
/// restart. Only an authorized abandonment ends it and only a new generation
/// supplied by the composition runs the work again.
#[test]
fn an_unsealed_claim_is_uncertain_until_abandoned_and_a_new_generation_runs_anew() {
    for fault in [Fault::SealLost, Fault::SealClock] {
        let f = fixture(&format!("seal-{fault:?}"));
        let clock = Clock::default();
        let reviewer = Synthetic::default();
        let b1 = binding(&f, 1);
        {
            let mut sink = Faulty::open(&f.db, &clock);
            sink.fault = fault;
            let first = run_attempt(&f, &mut sink, &reviewer, 1);
            assert!(first.persisted.is_err() && !first.passed(), "{fault:?}");
            assert_eq!(reviewer.calls.get(), 1);
            let snap = image(&sink.inner, &b1, "x");
            assert_eq!(snap.claim.as_ref().unwrap().state, S::Claimed);
            assert!(
                snap.seal.is_none() && snap.event_ids.is_empty(),
                "{fault:?}"
            );
            // Same process, same generation: in flight, executes nothing.
            let retry = run_attempt(&f, &mut sink, &reviewer, 1);
            assert!(matches!(
                retry.disposition,
                ReviewDisposition::Refused(ReviewRefusal::ClaimInFlight)
            ));
        }
        // After a restart it is still uncertain, never silently resumed.
        let mut sink = open_sink(&f.db, &clock);
        let retry = run_attempt(&f, &mut sink, &reviewer, 1);
        assert!(matches!(
            retry.disposition,
            ReviewDisposition::Refused(ReviewRefusal::ClaimInFlight)
        ));
        assert_eq!(reviewer.calls.get(), 1, "{fault:?}: no second execution");

        // A grant from an identity that does not own the writer is refused,
        // as is a grant for another claim; the claim stays unfinished.
        let foreign = identity()
            .abandonment_grant(PROJECT, &CoreReviewSink::claim_id(&b1).unwrap())
            .unwrap();
        assert!(
            sink.abandon(&b1, foreign)
                .unwrap_err()
                .contains("AbandonmentNotAuthorized")
        );
        let other = identity()
            .abandonment_grant(PROJECT, &CoreReviewSink::claim_id(&binding(&f, 2)).unwrap())
            .unwrap();
        assert!(sink.abandon(&b1, other).is_err());
        assert_eq!(
            image(&sink, &b1, "x").claim.unwrap().state,
            S::Claimed,
            "still unfinished"
        );
        drop(sink);

        // The admitted identity (kept by the trusted composition) abandons it.
        let admitted = identity();
        let mut sink = CoreReviewSink::new(
            CoreStateWriter::open(&f.db, admitted.clone()).unwrap(),
            clock.clone(),
        );
        let grant = admitted
            .abandonment_grant(PROJECT, &CoreReviewSink::claim_id(&b1).unwrap())
            .unwrap();
        let record = sink.abandon(&b1, grant).unwrap();
        assert_eq!(record.state, S::Abandoned);
        assert_eq!(record.authority_ref.as_deref(), Some(AUTHORITY));
        let blocked = run_attempt(&f, &mut sink, &reviewer, 1);
        assert!(matches!(
            blocked.disposition,
            ReviewDisposition::Refused(ReviewRefusal::ClaimAbandoned)
        ));
        assert_eq!(reviewer.calls.get(), 1, "abandonment ran nothing");

        // The composition's new generation is explicit new work.
        let fresh = run_attempt(&f, &mut sink, &reviewer, 2);
        assert!(
            fresh.passed(),
            "{:?} {:?}",
            fresh.disposition,
            fresh.persisted
        );
        assert_eq!(reviewer.calls.get(), 2);
        let b2 = binding(&f, 2);
        assert_ne!(b1.key(), b2.key());
        assert_eq!(
            image(&sink, &b1, "x").claim.unwrap().state,
            S::Abandoned,
            "generation 1 stays abandoned"
        );
        assert_eq!(
            image(&sink, &b2, &reviewed_id(&fresh)).claim.unwrap().state,
            S::Completed
        );
        drop(sink);
        assert_eq!(stored_event_types(&f.db), reviewed_events());
    }
}

#[test]
fn a_claim_that_cannot_be_taken_executes_nothing_and_leaves_no_state() {
    let f = fixture("noclaim");
    let clock = Clock::default();
    let reviewer = Synthetic::default();
    let mut sink = Faulty::open(&f.db, &clock);
    sink.fault = Fault::ClaimClock;
    let t = run_attempt(&f, &mut sink, &reviewer, 1);
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::Records(_))
    ));
    assert_eq!(reviewer.calls.get(), 0);
    let b = binding(&f, 1);
    assert_eq!(image(&sink.inner, &b, "x").claim, None);
    // Once State works, the same call acquires and runs exactly once.
    let t = run_attempt(&f, &mut sink, &reviewer, 1);
    assert!(t.passed());
    assert_eq!(reviewer.calls.get(), 1);

    // A scope the core cannot name is refused before any claim.
    let g = fixture("scope");
    let mut sink = open_sink(&g.db, &clock);
    let t = run_attempt(&g, &mut sink, &reviewer, 0);
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::Scope(_))
    ));
    assert_eq!(reviewer.calls.get(), 1, "no execution for generation 0");
    assert!(stored_event_types(&g.db).is_empty());
}

/// A REJECT is preserved through a fault and restart: the stored record is
/// never replaced by the retry, and its rejected SHA stays in history.
#[test]
fn rejected_review_survives_a_fault_unreplaced() {
    let f = fixture("reject");
    let clock = Clock::default();
    let reviewer = Synthetic::rejecting();
    {
        let mut sink = Faulty::open(&f.db, &clock);
        sink.fault = Fault::PublishClock;
        let t = run_attempt(&f, &mut sink, &reviewer, 1);
        assert!(t.persisted.is_err());
    }
    let mut sink = open_sink(&f.db, &clock);
    let retry = run_attempt(&f, &mut sink, &reviewer, 1);
    assert!(retry.persisted.is_ok() && !retry.passed());
    assert_eq!(reviewer.calls.get(), 1);
    let id = reviewed_id(&retry);
    let b = binding(&f, 1);
    let claim = image(&sink, &b, &id).claim.unwrap();
    assert_eq!(claim.state, S::Completed);
    assert_eq!(claim.outcome_kind.as_deref(), Some("REJECT"));
    drop(sink);
    let reader = StateReader::open_existing(&f.db).unwrap();
    let stored = reader
        .find_decision_record(PROJECT, DecisionRecordKind::A4Review, &id)
        .unwrap()
        .unwrap();
    assert!(stored.record_json().contains("\"verdict\":\"REJECT\""));
    assert_eq!(stored.subject_sha(), Some(f.implementation.as_str()));
    assert_eq!(
        stored_event_types(&f.db),
        expected([
            EventType::ReviewDispatched,
            EventType::ReviewRejected,
            EventType::FindingRaised
        ])
    );
}

/// Terminal policy and refusal outcomes are sealed with a refusal
/// settlement, published, and later callbacks recover them without routing.
#[test]
fn policy_blocked_and_refused_outcomes_are_sealed_published_and_retained() {
    // Policy refusal from the reviewer runtime.
    let f = fixture("blocked");
    let clock = Clock::default();
    let mut sink = open_sink(&f.db, &clock);
    let reviewer = Synthetic {
        policy_blocked: true,
        ..Synthetic::default()
    };
    let t = run_attempt(&f, &mut sink, &reviewer, 1);
    assert!(matches!(t.disposition, ReviewDisposition::PolicyBlocked(_)));
    assert!(t.persisted.is_ok() && !t.passed());
    let b = binding(&f, 1);
    let claim = image(&sink, &b, "x").claim.unwrap();
    assert_eq!(claim.state, S::Refused);
    assert_eq!(claim.outcome_kind.as_deref(), Some("POLICY_BLOCKED"));
    let replay = run_attempt(&f, &mut sink, &reviewer, 1);
    assert!(matches!(replay.disposition, ReviewDisposition::Replayed(_)));
    assert_eq!(reviewer.calls.get(), 1, "never re-dispatched or re-routed");
    drop(sink);
    assert_eq!(
        stored_event_types(&f.db),
        expected([
            EventType::ReviewDispatched,
            EventType::PolicyBlocked,
            EventType::HumanRequired
        ])
    );

    // Routing refuses before any provider is selected: no dispatch event.
    let g = fixture("routing-blocked");
    let mut sink = open_sink(&g.db, &clock);
    let routing = Pick(Err(ReviewerRoutingRefusal::PolicyBlocked));
    let wt = g.dir.0.join("review-wt");
    let reviewer = Synthetic::default();
    let t = run_attempt_in(&g, &mut sink, &reviewer, &routing, &wt, 1);
    assert!(matches!(t.disposition, ReviewDisposition::PolicyBlocked(_)));
    assert_eq!(reviewer.calls.get(), 0);
    drop(sink);
    assert_eq!(
        stored_event_types(&g.db),
        expected([EventType::PolicyBlocked, EventType::HumanRequired])
    );

    // No eligible reviewer: a retained REFUSED outcome, nothing dispatched.
    let h = fixture("no-reviewer");
    let mut sink = open_sink(&h.db, &clock);
    let routing = Pick(Err(ReviewerRoutingRefusal::NoEligibleReviewer));
    let wt = h.dir.0.join("review-wt");
    let t = run_attempt_in(&h, &mut sink, &reviewer, &routing, &wt, 1);
    assert!(matches!(
        t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::NoEligibleReviewer)
    ));
    let b = binding(&h, 1);
    let claim = image(&sink, &b, "x").claim.unwrap();
    assert_eq!(
        (claim.state, claim.outcome_kind.as_deref()),
        (S::Refused, Some("REFUSED"))
    );
    // The refusal is retained for this generation; a better route needs a
    // new generation from the composition.
    let again = run_attempt_in(&h, &mut sink, &reviewer, &Pick::default(), &wt, 1);
    assert!(matches!(again.disposition, ReviewDisposition::Replayed(_)));
    assert_eq!(reviewer.calls.get(), 0);
    let fresh = run_attempt_in(&h, &mut sink, &reviewer, &Pick::default(), &wt, 2);
    assert!(fresh.passed());
    assert_eq!(reviewer.calls.get(), 1);
}

struct Shared(Arc<Mutex<CoreReviewSink>>);
impl ReviewRecordSink for Shared {
    fn claim(&mut self, b: &ReviewClaimBinding) -> Result<ReviewClaim, String> {
        self.0.lock().unwrap().claim(b)
    }
    fn seal(&mut self, b: &ReviewClaimBinding, s: &SealedReview) -> Result<(), String> {
        self.0.lock().unwrap().seal(b, s)
    }
    fn publish(&mut self, b: &ReviewClaimBinding, p: &ReviewRecordProposal) -> Result<(), String> {
        self.0.lock().unwrap().publish(b, p)
    }
}

/// Mutation control: no exclusion at all (every claim "acquired").
struct AlwaysAcquire(Shared);
impl ReviewRecordSink for AlwaysAcquire {
    fn claim(&mut self, _: &ReviewClaimBinding) -> Result<ReviewClaim, String> {
        Ok(ReviewClaim::Acquired)
    }
    fn seal(&mut self, b: &ReviewClaimBinding, s: &SealedReview) -> Result<(), String> {
        self.0.seal(b, s)
    }
    fn publish(&mut self, b: &ReviewClaimBinding, p: &ReviewRecordProposal) -> Result<(), String> {
        self.0.publish(b, p)
    }
}

struct Counting(Arc<AtomicUsize>);
impl ReviewerRuntime for Counting {
    fn runtime_id(&self) -> &str {
        "synthetic"
    }
    fn run_review(
        &self,
        c: &ReviewCapsuleNonTemporalCore,
        w: &WorkspaceHandle,
    ) -> Result<ReviewerRunOutput, FailureClass> {
        self.0.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        Synthetic::default().run_review(c, w)
    }
}

fn race(
    f: &Arc<Fixture>,
    shared: &Arc<Mutex<CoreReviewSink>>,
    mutated: bool,
) -> (usize, Vec<String>) {
    let runs = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(6));
    let outcomes: Vec<String> = (0..6)
        .map(|n| {
            let (f, runs, barrier, shared) = (
                Arc::clone(f),
                Arc::clone(&runs),
                Arc::clone(&barrier),
                Arc::clone(shared),
            );
            std::thread::spawn(move || {
                let mut sink: Box<dyn ReviewRecordSink> = if mutated {
                    Box::new(AlwaysAcquire(Shared(shared)))
                } else {
                    Box::new(Shared(shared))
                };
                barrier.wait();
                let wt = f.dir.0.join(format!("review-wt-{n}"));
                let t = run_attempt_in(&f, &mut *sink, &Counting(runs), &Pick::default(), &wt, 1);
                match &t.disposition {
                    ReviewDisposition::Reviewed(_) => "reviewed",
                    ReviewDisposition::Replayed(_) => "replayed",
                    ReviewDisposition::Refused(ReviewRefusal::ClaimInFlight) => "in-flight",
                    _ => "other",
                }
                .to_owned()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    (runs.load(Ordering::SeqCst), outcomes)
}

/// Concurrent callbacks for one claim: exactly one acquires and executes; the
/// others find it in flight or recover its sealed result.
#[test]
fn concurrent_callers_acquire_the_claim_exactly_once() {
    let f = Arc::new(fixture("concurrent"));
    let clock = Clock::default();
    let shared = Arc::new(Mutex::new(open_sink(&f.db, &clock)));
    let (runs, outcomes) = race(&f, &shared, false);
    assert_eq!(runs, 1, "{outcomes:?}");
    assert_eq!(outcomes.iter().filter(|o| *o == "reviewed").count(), 1);
    assert!(
        outcomes.iter().all(|o| o != "other"),
        "every other caller is in flight or replayed: {outcomes:?}"
    );
    // After the winner finished, any caller recovers its recorded result.
    let late = {
        let mut sink = Shared(Arc::clone(&shared));
        run_attempt(&f, &mut sink, &Counting(Arc::new(AtomicUsize::new(0))), 1)
    };
    assert!(matches!(late.disposition, ReviewDisposition::Replayed(_)));
    assert!(late.passed());
    drop(shared);
    assert_eq!(stored_event_types(&f.db), reviewed_events());
}

/// The harness above can fail: with no exclusion the reviewer runs many times.
#[test]
fn a_sink_without_exclusion_is_caught_by_the_concurrency_harness() {
    let f = Arc::new(fixture("racy"));
    let clock = Clock::default();
    let shared = Arc::new(Mutex::new(open_sink(&f.db, &clock)));
    let (runs, outcomes) = race(&f, &shared, true);
    assert!(runs > 1, "the mutant was not detected: {outcomes:?}");
}

/// The package a run of `reviewer` produces for `f`'s request at `generation`,
/// recorded in a throwaway store so the store under test stays untouched.
fn package_of(f: &Fixture, generation: u32, reviewer: &Synthetic) -> (ReviewTicket, SealedReview) {
    let tag = format!("scratch-{generation}-{}", reviewer.reject);
    let state = f.dir.0.join(&tag);
    let mut builder = std::fs::DirBuilder::new();
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder.create(&state).unwrap();
    let mut sink = open_sink(&state.join("state.db"), &Clock::default());
    let t = run_attempt_in(
        f,
        &mut sink,
        reviewer,
        &Pick::default(),
        &f.dir.0.join(format!("wt-{tag}")),
        generation,
    );
    let sealed = SealedReview::new(t.proposal.as_ref().unwrap(), "REVIEWED", true, 1);
    (t, sealed)
}

/// Resolves every reference to the same wrong bytes.
struct Wrong;
impl CoreContentResolverV1 for Wrong {
    fn resolve(&self, _: &str, _: &str) -> Option<Vec<u8>> {
        Some(b"not what was sealed".to_vec())
    }
}

/// Conflicting scope, package, content or disposition: refused with no
/// partial writes, and what was sealed first stands.
#[test]
fn conflicting_scope_package_and_disposition_are_refused_without_partial_writes() {
    let f = fixture("conflicts");
    let clock = Clock::default();
    let mut sink = open_sink(&f.db, &clock);
    let (b1, b2) = (binding(&f, 1), binding(&f, 2));
    let (pass_ticket, pass1) = package_of(&f, 1, &Synthetic::default());
    let (_, pass2) = package_of(&f, 2, &Synthetic::default());
    let (_, reject1) = package_of(&f, 1, &Synthetic::rejecting());
    let id = reviewed_id(&pass_ticket);
    assert_eq!(pass1.binding().unwrap(), b1);
    assert_eq!(
        reject1.binding().unwrap(),
        b1,
        "same scope, different package"
    );

    assert!(matches!(sink.claim(&b1).unwrap(), ReviewClaim::Acquired));
    let claimed = image(&sink, &b1, &id);
    // Scope: a package sealed for generation 2 cannot be sealed under 1, and
    // the core's own codec boundary refuses it even if the sink did not.
    assert!(
        sink.seal(&b1, &pass2)
            .unwrap_err()
            .contains("another claim scope")
    );
    assert!(
        sink.publish(&b1, pass2.proposal().as_ref().unwrap())
            .is_err()
    );
    // A settlement that contradicts the package (label or records) never seals.
    for label in [
        "INVALID_OUTPUT",
        "POLICY_BLOCKED",
        "DISPOSITIONS",
        "REPAIR_ISSUED",
    ] {
        let bad = SealedReview::new(&pass1.proposal().unwrap(), label, true, 1);
        assert!(sink.seal(&b1, &bad).is_err(), "{label}");
    }
    assert_eq!(image(&sink, &b1, &id), claimed, "nothing was sealed");

    // The first valid seal stands; a different package or bytes conflict.
    sink.seal(&b1, &pass1).unwrap();
    let sealed = image(&sink, &b1, &id);
    let mut changed = pass1.proposal().unwrap();
    changed.content[0].1.push(0);
    assert!(
        sink.publish(&b1, &changed).is_err(),
        "changed public content"
    );
    assert_eq!(image(&sink, &b1, &id), sealed);
    sink.seal(&b1, &pass1).unwrap(); // identical re-seal is a no-op
    assert!(sink.seal(&b1, &reject1).is_err());
    assert_eq!(image(&sink, &b1, &id), sealed);
    // Publish refuses a package that is not the sealed one: nothing written.
    assert!(
        sink.publish(&b1, reject1.proposal().as_ref().unwrap())
            .unwrap_err()
            .contains("not the one sealed")
    );
    assert_eq!(image(&sink, &b1, &id), sealed);
    // Publishing a claim that was never sealed refuses too.
    assert!(matches!(sink.claim(&b2).unwrap(), ReviewClaim::Acquired));
    assert!(
        sink.publish(&b2, pass2.proposal().as_ref().unwrap())
            .unwrap_err()
            .contains("nothing is sealed")
    );

    // The sealed package publishes exactly once.
    sink.publish(&b1, pass1.proposal().as_ref().unwrap())
        .unwrap();
    let published = image(&sink, &b1, &id);
    assert_eq!(published.event_ids.len(), 2);
    sink.publish(&b1, pass1.proposal().as_ref().unwrap())
        .unwrap();
    assert_eq!(
        image(&sink, &b1, &id),
        published,
        "exact replay changes nothing"
    );
    assert_eq!(published.claim.unwrap().state, S::Completed);
}

/// The core boundary itself (bypassing the sink): tampered, truncated or
/// foreign recovery bytes never seal.
#[test]
fn the_core_boundary_refuses_tampered_truncated_and_foreign_recovery_bytes() {
    let f = fixture("tamper");
    let clock = Clock::default();
    let (_, pass2) = package_of(&f, 2, &Synthetic::default());
    let (_, pass1) = package_of(&f, 1, &Synthetic::default());
    let b1 = binding(&f, 1);
    let mut writer = CoreStateWriter::open(&f.db, identity()).unwrap();
    let scope = b1.scope();
    let unit = CoreReviewUnitV1::new(
        scope.project_id(),
        scope.task_id(),
        scope.subject_sha(),
        scope.request_sha256(),
        scope.generation(),
    )
    .unwrap();
    assert_eq!(
        writer.claim_review(&clock, &unit).unwrap(),
        CoreReviewClaimV1::Acquired
    );
    let content = SealedContent::of(&pass1.proposal().unwrap());
    let codec = ReviewSealedCodec::for_binding(&b1);
    let good = pass1.to_bytes();
    let mut flipped = good.clone();
    let at = flipped.len() / 2;
    flipped[at] = if flipped[at] == b'a' { b'b' } else { b'a' };
    let cases: [(&str, Vec<u8>); 4] = [
        ("flipped byte", flipped),
        ("truncated", good[..good.len() / 2].to_vec()),
        ("not a seal", b"{}".to_vec()),
        ("foreign scope", pass2.to_bytes()),
    ];
    for (name, bytes) in cases {
        let refusal = writer.seal_review(&clock, &content, &codec, &unit, bytes);
        assert!(
            matches!(
                refusal,
                Err(CoreStateWriteError::Policy(
                    CorePolicyRefusal::RecoveryDataInvalid
                ))
            ),
            "{name}: {refusal:?}"
        );
    }
    assert!(
        writer.sealed_review(&unit).unwrap().is_none(),
        "nothing sealed"
    );
    // A codec for another scope refuses even the valid bytes.
    let wrong = ReviewSealedCodec::for_binding(&binding(&f, 2));
    assert!(matches!(
        writer.seal_review(&clock, &content, &wrong, &unit, good.clone()),
        Err(CoreStateWriteError::Policy(
            CorePolicyRefusal::RecoveryDataInvalid
        ))
    ));
    // Content that no longer matches the digest the event names is refused by
    // the core's own resolution and hashing.
    assert!(matches!(
        writer.seal_review(&clock, &Wrong, &codec, &unit, good.clone()),
        Err(CoreStateWriteError::Policy(
            CorePolicyRefusal::ContentDigestMismatch
        ))
    ));
    assert!(writer.sealed_review(&unit).unwrap().is_none());
    // The valid bytes seal; State then refuses any other bytes as a conflict.
    writer
        .seal_review(&clock, &content, &codec, &unit, good)
        .unwrap();
    assert!(matches!(
        writer.seal_review(&clock, &content, &codec, &unit, b"{\"outcome\":1}".to_vec()),
        Err(CoreStateWriteError::Policy(
            CorePolicyRefusal::RecoveryDataInvalid
        ))
    ));
}

/// A claim another writer settled as a plain terminal refusal (nothing
/// sealed) never authorizes execution through the sink either.
#[test]
fn a_plainly_settled_terminal_claim_never_executes() {
    use receipts_review_integration::state::ReviewClaimSettlementV1;
    let f = fixture("terminal");
    let clock = Clock::default();
    let b = binding(&f, 1);
    {
        let mut writer = CoreStateWriter::open(&f.db, identity()).unwrap();
        let scope = b.scope();
        let unit = CoreReviewUnitV1::new(
            scope.project_id(),
            scope.task_id(),
            scope.subject_sha(),
            scope.request_sha256(),
            scope.generation(),
        )
        .unwrap();
        assert_eq!(
            writer.claim_review(&clock, &unit).unwrap(),
            CoreReviewClaimV1::Acquired
        );
        let blocked = ReviewClaimSettlementV1::refused("POLICY_BLOCKED", None, None).unwrap();
        writer
            .settle_review_refusal(&clock, &unit, &blocked)
            .unwrap();
    }
    let mut sink = open_sink(&f.db, &clock);
    let reviewer = Synthetic::default();
    let t = run_attempt(&f, &mut sink, &reviewer, 1);
    assert!(matches!(
        &t.disposition,
        ReviewDisposition::Refused(ReviewRefusal::ClaimTerminal(kind)) if kind == "POLICY_BLOCKED"
    ));
    assert_eq!(reviewer.calls.get(), 0);
    assert!(stored_event_types(&f.db).is_empty());
}
