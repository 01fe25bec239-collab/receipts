//! Durable review-claim controls: one execution per claim across repeated,
//! concurrent and restarted callbacks; recorded dispositions and retained
//! terminal refusals; unfinished claims never re-executed and never expiring;
//! explicit authority-named abandonment; failure injection at every write
//! boundary; admission and ticket binding.

use std::sync::{Arc, Mutex};

use crate::atomic_publication_tests::{
    Clock, Dir, G0, T1, T2, assert_atomic_at_every_boundary, claim, fingerprint, project_subject,
    publication_code, refusal, rows, ticket,
};
use crate::error::{PublicationFailureCode as PCode, StateError};
use crate::{
    ReviewClaimAbandonV1, ReviewClaimOutcomeV1, ReviewClaimRecordV1, ReviewClaimRequestV1,
    ReviewClaimSettlementV1, ReviewClaimStateV1 as State, SqliteStateRepository,
    StateWriteOperationV1 as Op, StateWriteRefusalV1 as R, TrustedCoreStore,
};

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn existing(outcome: ReviewClaimOutcomeV1) -> ReviewClaimRecordV1 {
    match outcome {
        ReviewClaimOutcomeV1::Existing(record) => record,
        other => panic!("expected an existing claim, got {other:?}"),
    }
}

fn settle(
    store: &mut TrustedCoreStore,
    clock: &Clock,
    t: &crate::ReviewClaimTicketV1,
    settlement: &ReviewClaimSettlementV1,
) -> Result<ReviewClaimRecordV1, StateError> {
    let admission = store.admit(project_subject(Op::ReviewClaim, "p"));
    store.settle_review_claim_v1(admission, clock, t, settlement)
}

fn open(tag: &str) -> (Dir, TrustedCoreStore) {
    let dir = Dir::new(tag);
    let store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    (dir, store)
}

#[test]
fn only_the_first_claim_may_execute_and_the_unfinished_claim_survives_restart() {
    let (dir, mut store) = open("claim-once");
    let first = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    assert_eq!(first.claim_id(), "claim-1");
    let again = existing(claim(&mut store, &T1, "claim-1").unwrap());
    assert_eq!(again.state, State::Claimed);
    assert_eq!(again.claimed_at, G0.0);
    assert_eq!(again.finished_at, None);

    // Crash and restart: the claim is still there, still unfinished, and no
    // ticket exists for it.
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let after_restart = existing(claim(&mut store, &T2, "claim-1").unwrap());
    assert_eq!(after_restart, again);
    assert_eq!(rows(&store, "review_claim"), 1);
}

#[test]
fn an_unfinished_claim_never_expires_or_reopens_on_its_own() {
    let (_dir, mut store) = open("claim-no-ttl");
    let _ticket = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    // Ten years later by the trusted clock: still unfinished, still no execution.
    let far = Clock("2036-11-08T10:00:00.000000000Z");
    let record = existing(claim(&mut store, &far, "claim-1").unwrap());
    assert_eq!(record.state, State::Claimed);
    assert_eq!(record.claimed_at, G0.0);
    // A distinct claim id is a distinct unit of work.
    assert!(matches!(
        claim(&mut store, &far, "claim-2").unwrap(),
        ReviewClaimOutcomeV1::Claimed(_)
    ));
}

#[test]
fn concurrent_callbacks_through_the_exclusive_store_execute_exactly_once() {
    let (_dir, store) = open("claim-threads");
    let store = Arc::new(Mutex::new(store));
    let winners = Arc::new(Mutex::new(Vec::new()));
    let handles: Vec<_> = (0..8)
        .map(|n| {
            let store = Arc::clone(&store);
            let winners = Arc::clone(&winners);
            std::thread::spawn(move || {
                let mut store = store.lock().unwrap();
                if let ReviewClaimOutcomeV1::Claimed(_) = claim(&mut store, &T1, "claim-1").unwrap()
                {
                    winners.lock().unwrap().push(n);
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    assert_eq!(winners.lock().unwrap().len(), 1);
    assert_eq!(rows(&store.lock().unwrap(), "review_claim"), 1);
}

#[test]
fn separate_connections_racing_for_one_claim_insert_exactly_one_row() {
    // Below the exclusive-store lock: the database itself serializes writers.
    let dir = Dir::new("claim-connections");
    drop(SqliteStateRepository::open(dir.db()).unwrap());
    let barrier = Arc::new(std::sync::Barrier::new(6));
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let path = dir.db();
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                let mut repo = SqliteStateRepository::open(path).unwrap();
                let request =
                    ReviewClaimRequestV1::new("p", "claim-1", "t-1", Some(SHA), &"b".repeat(64))
                        .unwrap();
                barrier.wait();
                repo.claim_review_v1(&T1, &request)
                    .map(|(inserted, _)| inserted)
            })
        })
        .collect();
    let inserted: Vec<bool> = handles
        .into_iter()
        .map(|h| h.join().unwrap().expect("busy writers wait, never fail"))
        .collect();
    assert_eq!(inserted.iter().filter(|x| **x).count(), 1);
}

#[test]
fn completed_and_refused_claims_expose_their_retained_disposition() {
    let (dir, mut store) = open("claim-dispositions");
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let sha = "d".repeat(64);
    let done = ReviewClaimSettlementV1::completed("REJECT", Some("rev-1"), Some(&sha)).unwrap();
    let record = settle(&mut store, &T1, &t, &done).unwrap();
    assert_eq!(record.state, State::Completed);
    assert_eq!(record.outcome_kind.as_deref(), Some("REJECT"));
    assert_eq!(record.outcome_ref.as_deref(), Some("rev-1"));
    assert_eq!(record.outcome_sha256.as_deref(), Some(sha.as_str()));
    assert_eq!(record.finished_at.as_deref(), Some(T1.0));
    // Idempotent for the identical disposition; a different one refuses.
    assert_eq!(settle(&mut store, &T2, &t, &done).unwrap(), record);
    let other = ReviewClaimSettlementV1::completed("PASS", Some("rev-1"), Some(&sha)).unwrap();
    assert_eq!(
        publication_code(settle(&mut store, &T2, &t, &other).unwrap_err()),
        PCode::ClaimNotUnfinished
    );
    // A later callback sees the recorded disposition, across restart too.
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert_eq!(existing(claim(&mut store, &T2, "claim-1").unwrap()), record);

    // A terminal policy refusal is retained and cannot be completed afterwards.
    let t = ticket(claim(&mut store, &T2, "claim-2").unwrap());
    let blocked = ReviewClaimSettlementV1::refused("POLICY_BLOCKED", None, None).unwrap();
    let refused = settle(&mut store, &T2, &t, &blocked).unwrap();
    assert_eq!(refused.state, State::Refused);
    let late = ReviewClaimSettlementV1::completed("PASS", None, None).unwrap();
    assert_eq!(
        publication_code(settle(&mut store, &T2, &t, &late).unwrap_err()),
        PCode::ClaimNotUnfinished
    );
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let seen = existing(claim(&mut store, &T2, "claim-2").unwrap());
    assert_eq!(seen.state, State::Refused);
    assert_eq!(seen.outcome_kind.as_deref(), Some("POLICY_BLOCKED"));
}

#[test]
fn reusing_a_claim_id_for_different_work_is_refused() {
    let (_dir, mut store) = open("claim-conflict");
    let _t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let before = fingerprint(&store);
    for (task, sha, request) in [
        ("t-2", Some(SHA), "b".repeat(64)),
        (
            "t-1",
            Some("cccccccccccccccccccccccccccccccccccccccc"),
            "b".repeat(64),
        ),
        ("t-1", None, "b".repeat(64)),
        ("t-1", Some(SHA), "c".repeat(64)),
    ] {
        let admission = store.admit(project_subject(Op::ReviewClaim, "p"));
        let error = store
            .claim_review_v1(
                admission,
                &T1,
                &ReviewClaimRequestV1::new("p", "claim-1", task, sha, &request).unwrap(),
            )
            .unwrap_err();
        assert_eq!(publication_code(error), PCode::ConflictingRetry);
    }
    assert_eq!(fingerprint(&store), before);
}

#[test]
fn abandonment_is_explicit_authority_named_terminal_and_runs_nothing() {
    let (dir, mut store) = open("claim-abandon");
    let stale = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    drop(store);
    // After the crash nothing can settle the claim; only an explicit act ends it.
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert_eq!(
        existing(claim(&mut store, &T1, "claim-1").unwrap()).state,
        State::Claimed
    );
    let admission = store.admit(project_subject(Op::ReviewClaim, "p"));
    assert_eq!(
        refusal(
            store
                .settle_review_claim_v1(
                    admission,
                    &T1,
                    &stale,
                    &ReviewClaimSettlementV1::completed("PASS", None, None).unwrap()
                )
                .unwrap_err()
        ),
        R::WrongStore
    );
    let abandon = ReviewClaimAbandonV1::new("p", "claim-1", "human:operator-1").unwrap();
    let admission = store.admit(project_subject(Op::ReviewClaim, "p"));
    let record = store
        .abandon_review_claim_v1(admission, &T1, &abandon)
        .unwrap();
    assert_eq!(record.state, State::Abandoned);
    assert_eq!(record.authority_ref.as_deref(), Some("human:operator-1"));
    // Same authority is idempotent; another authority, and any later claim of
    // the same id, never re-execute.
    let admission = store.admit(project_subject(Op::ReviewClaim, "p"));
    assert_eq!(
        store
            .abandon_review_claim_v1(admission, &T2, &abandon)
            .unwrap(),
        record
    );
    let admission = store.admit(project_subject(Op::ReviewClaim, "p"));
    assert_eq!(
        publication_code(
            store
                .abandon_review_claim_v1(
                    admission,
                    &T2,
                    &ReviewClaimAbandonV1::new("p", "claim-1", "human:other").unwrap()
                )
                .unwrap_err()
        ),
        PCode::ClaimNotUnfinished
    );
    assert_eq!(
        existing(claim(&mut store, &T2, "claim-1").unwrap()).state,
        State::Abandoned
    );
    // Unknown claims are refused, not created.
    let admission = store.admit(project_subject(Op::ReviewClaim, "p"));
    assert_eq!(
        publication_code(
            store
                .abandon_review_claim_v1(
                    admission,
                    &T2,
                    &ReviewClaimAbandonV1::new("p", "never-claimed", "human:operator-1").unwrap()
                )
                .unwrap_err()
        ),
        PCode::ClaimNotFound
    );
    // Deliberate re-execution is a new, separately recorded claim id.
    assert!(matches!(
        claim(&mut store, &T2, "claim-1/generation-2").unwrap(),
        ReviewClaimOutcomeV1::Claimed(_)
    ));
}

#[test]
fn claim_settle_and_abandon_are_atomic_at_every_write_boundary() {
    let at_claim = assert_atomic_at_every_boundary(
        || open("claim-fault-claim"),
        |(_, store)| fingerprint(store),
        |(_, store)| claim(store, &T1, "claim-1").map(|_| ()),
    );
    assert!(at_claim >= 2);
    let at_settle = assert_atomic_at_every_boundary(
        || {
            let (dir, mut store) = open("claim-fault-settle");
            let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
            (dir, store, t)
        },
        |(_, store, _)| fingerprint(store),
        |(_, store, t)| {
            settle(
                store,
                &T1,
                t,
                &ReviewClaimSettlementV1::refused("POLICY_BLOCKED", None, None).unwrap(),
            )
            .map(|_| ())
        },
    );
    assert!(at_settle >= 2);
    let at_abandon = assert_atomic_at_every_boundary(
        || {
            let (dir, mut store) = open("claim-fault-abandon");
            claim(&mut store, &G0, "claim-1").unwrap();
            (dir, store)
        },
        |(_, store)| fingerprint(store),
        |(_, store)| {
            let admission = store.admit(project_subject(Op::ReviewClaim, "p"));
            store
                .abandon_review_claim_v1(
                    admission,
                    &T1,
                    &ReviewClaimAbandonV1::new("p", "claim-1", "human:operator-1").unwrap(),
                )
                .map(|_| ())
        },
    );
    assert!(at_abandon >= 2);
}

#[test]
fn a_clock_regression_claims_nothing() {
    let (_dir, mut store) = open("claim-clock");
    let _t = ticket(claim(&mut store, &T1, "claim-1").unwrap());
    let before = fingerprint(&store);
    assert!(claim(&mut store, &G0, "claim-2").is_err());
    assert_eq!(fingerprint(&store), before);
    assert_eq!(rows(&store, "review_claim"), 1);
}

#[test]
fn claim_admission_is_project_operation_and_instance_bound() {
    let (dir, mut store) = open("claim-auth");
    let before = fingerprint(&store);
    let request = ReviewClaimRequestV1::new("p", "claim-1", "t-1", None, &"b".repeat(64)).unwrap();
    for (subject, expected) in [
        (project_subject(Op::ReviewClaim, "q"), R::ProjectMismatch),
        (
            project_subject(Op::DecisionPublication, "p"),
            R::WrongOperation,
        ),
        (project_subject(Op::DecisionRecord, "p"), R::WrongOperation),
    ] {
        let admission = store.admit(subject);
        assert_eq!(
            refusal(store.claim_review_v1(admission, &T1, &request).unwrap_err()),
            expected
        );
    }
    assert_eq!(fingerprint(&store), before);
    let stale = store.admit(project_subject(Op::ReviewClaim, "p"));
    drop(store);
    let mut reopened = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert_eq!(
        refusal(reopened.claim_review_v1(stale, &T1, &request).unwrap_err()),
        R::WrongStore
    );
    // Malformed requests never reach the store.
    assert!(ReviewClaimRequestV1::new("p", "", "t", None, &"b".repeat(64)).is_err());
    assert!(ReviewClaimRequestV1::new("p", "c", "t", Some("ABC"), &"b".repeat(64)).is_err());
    assert!(ReviewClaimRequestV1::new("p", "c", "t", None, "short").is_err());
    assert!(ReviewClaimSettlementV1::completed("lower", None, None).is_err());
    assert!(ReviewClaimSettlementV1::completed("OK", None, Some("zz")).is_err());
    assert!(ReviewClaimAbandonV1::new("p", "c", "").is_err());
}

#[test]
fn claim_rows_enforce_their_own_state_machine_below_the_api() {
    let (_dir, mut store) = open("claim-sql");
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let conn = store.reader().connection();
    for sql in [
        "DELETE FROM review_claim",
        "UPDATE review_claim SET task_id='other'",
        "UPDATE review_claim SET claimed_at='2036-01-01T00:00:00.000000000Z'",
        "UPDATE review_claim SET state='CLAIMED'",
        "UPDATE review_claim SET state='COMPLETED'",
        "INSERT INTO review_claim(project_id,claim_id,task_id,request_sha256,state,outcome_kind,claimed_at,finished_at) VALUES ('p','c2','t','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb','COMPLETED','PASS','x','x')",
        "INSERT OR REPLACE INTO review_claim(project_id,claim_id,task_id,request_sha256,state,claimed_at) VALUES ('p','claim-1','t','bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb','CLAIMED','x')",
    ] {
        assert!(conn.execute_batch(sql).is_err(), "{sql}");
    }
    // The one allowed move works, once.
    settle(
        &mut store,
        &T1,
        &t,
        &ReviewClaimSettlementV1::completed("PASS", None, None).unwrap(),
    )
    .unwrap();
    let conn = store.reader().connection();
    for sql in [
        "UPDATE review_claim SET state='REFUSED'",
        "UPDATE review_claim SET outcome_kind='REJECT'",
        "UPDATE review_claim SET finished_at='2037-01-01T00:00:00.000000000Z'",
    ] {
        assert!(conn.execute_batch(sql).is_err(), "{sql}");
    }
    assert_eq!(rows(&store, "review_claim"), 1);
}
