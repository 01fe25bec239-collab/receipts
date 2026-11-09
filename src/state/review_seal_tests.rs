//! Durable seal / restart publication controls (ST-W2 remaining item 2):
//! claim once -> seal the exact observed result and package -> publish
//! atomically, with restart readback that finishes only the sealed
//! publication. Crash/fault before the seal, after it and at every
//! publication boundary; exact and conflicting reseal and retry; concurrent
//! claim/seal; foreign, stale and forged inputs; mismatched packages;
//! immutable REJECT and POLICY_BLOCKED; no partial rows, receipts or
//! watermarks. Synthetic same-user controls only.

use std::sync::{Arc, Barrier};

use crate::atomic_publication_tests::{
    Clock, Dir, G0, T1, T2, assert_atomic_at_every_boundary, claim, event, fingerprint, open_store,
    proposal, publication_code, publish, record, reject_review, rows, ticket,
};
use crate::error::{PublicationFailureCode as PCode, RecordFailureCode as RCode, StateError};
use crate::review_claim::sha256_hex;
use crate::{
    DecisionPublicationV1, DecisionRecordKind, EventType, PublicationEventV1, ReviewClaimAbandonV1,
    ReviewClaimOutcomeV1, ReviewClaimRequestV1, ReviewClaimSettlementV1,
    ReviewClaimStateV1 as State, ReviewClaimTicketV1, ReviewRecoveryDataV1, ReviewSealRecordV1,
    ReviewSealRequestV1, SqliteStateRepository, StateWriteOperationV1 as Op,
    StateWriteRefusalV1 as R, TrustedCoreStore,
};

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const KIND: &str = "REVIEW_SEALED_V1";

fn claim_request(claim_id: &str) -> ReviewClaimRequestV1 {
    ReviewClaimRequestV1::new("p", claim_id, "t-1", Some(SHA), &"b".repeat(64)).unwrap()
}

fn recovery(bytes: &[u8]) -> ReviewRecoveryDataV1 {
    ReviewRecoveryDataV1::new(KIND, bytes.to_vec(), &sha256_hex(bytes)).unwrap()
}

fn admission_for(store: &TrustedCoreStore, op: Op) -> crate::StateWriteAdmission {
    store.admit(crate::StateWriteSubjectV1 {
        operation: op,
        project_id: "p".into(),
        graph_id: None,
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    })
}

/// The Core's rebuild of a verdict package from recovery data: stable event
/// ids, the same publication id, the same records. A REJECT here; the
/// `settlement` is what the claim takes.
fn verdict(id: &str, settlement: ReviewClaimSettlementV1) -> Parts {
    Parts {
        id: id.to_owned(),
        settlement,
    }
}

struct Parts {
    id: String,
    settlement: ReviewClaimSettlementV1,
}

impl Parts {
    fn base(&self) -> DecisionPublicationV1 {
        DecisionPublicationV1::new(
            "p",
            &self.id,
            vec![reject_review()],
            vec![
                PublicationEventV1::stable(
                    proposal(EventType::ReviewRejected, "sha256:cc"),
                    &self.id,
                    "review-rejected",
                )
                .unwrap(),
            ],
            vec![],
        )
        .unwrap()
    }
    /// The live claimant's package.
    fn with_ticket(&self, t: &ReviewClaimTicketV1) -> DecisionPublicationV1 {
        self.base()
            .with_claim_settlement(t, self.settlement.clone())
            .unwrap()
    }
    /// The restart package, rebuilt from the sealed record alone.
    fn package_from_seal(&self, seal: &ReviewSealRecordV1) -> DecisionPublicationV1 {
        self.base().with_sealed_claim(seal).unwrap()
    }
}

fn completed_reject() -> ReviewClaimSettlementV1 {
    ReviewClaimSettlementV1::completed("REJECT", Some("rev-1"), None).unwrap()
}

fn seal_with(
    store: &mut TrustedCoreStore,
    clock: &Clock,
    t: &ReviewClaimTicketV1,
    package: &DecisionPublicationV1,
    bytes: &[u8],
) -> Result<ReviewSealRecordV1, StateError> {
    let request = ReviewSealRequestV1::new(&claim_request(t.claim_id()), package, recovery(bytes))?;
    let admission = admission_for(store, Op::ReviewClaim);
    store.seal_review_claim_v1(admission, clock, t, &request)
}

fn existing(outcome: ReviewClaimOutcomeV1) -> crate::ReviewClaimRecordV1 {
    match outcome {
        ReviewClaimOutcomeV1::Existing(record) => record,
        other => panic!("expected an existing claim, got {other:?}"),
    }
}

/// claim-1 claimed and sealed with a REJECT package; returns the seal.
fn sealed_store(
    tag: &str,
) -> (
    Dir,
    TrustedCoreStore,
    ReviewSealRecordV1,
    Parts,
    ReviewClaimTicketV1,
) {
    let (dir, mut store) = open_store(tag);
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let parts = verdict("pub-1", completed_reject());
    let seal = seal_with(&mut store, &T1, &t, &parts.with_ticket(&t), b"journal-v1").unwrap();
    (dir, store, seal, parts, t)
}

#[test]
fn claim_seal_crash_restart_readback_then_publish_exactly_the_sealed_package() {
    let (dir, store, seal, parts, _t) = sealed_store("seal-restart");
    // The seal binds project, claim, task/subject/request, package identity,
    // settlement and the exact recovery data.
    assert_eq!(seal.project_id, "p");
    assert_eq!(seal.claim_id, "claim-1");
    assert_eq!(seal.task_id, "t-1");
    assert_eq!(seal.subject_sha.as_deref(), Some(SHA));
    assert_eq!(seal.request_sha256, "b".repeat(64));
    assert_eq!(seal.publication_id, "pub-1");
    assert_eq!(seal.recovery_kind, KIND);
    assert_eq!(seal.recovery_data, b"journal-v1");
    assert_eq!(seal.recovery_sha256, sha256_hex(b"journal-v1"));
    assert_eq!(seal.settlement, completed_reject());
    assert_eq!(seal.sealed_at, T1.0);
    assert_eq!(seal.package_sha256.len(), 64);

    // Crash after the seal, before anything was published.
    assert_eq!(rows(&store, "decision_records"), 0);
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    // The claim is still unfinished and no ticket exists: it never executes...
    let record = existing(claim(&mut store, &T2, "claim-1").unwrap());
    assert_eq!(record.state, State::Claimed);
    // ...but the sealed result is readable, byte for byte.
    let read = store
        .reader()
        .find_review_seal("p", "claim-1")
        .unwrap()
        .unwrap();
    assert_eq!(read, seal);

    // Finish only that publication, from the seal alone.
    let rebuilt = parts.package_from_seal(&read);
    assert_eq!(rebuilt.sha256(), read.package_sha256, "same package");
    let receipt = publish(&mut store, &T2, &rebuilt).unwrap();
    assert!(!receipt.replayed);
    let settled = receipt.claim.unwrap();
    assert_eq!(settled.state, State::Completed);
    assert_eq!(settled.outcome_kind.as_deref(), Some("REJECT"));
    assert_eq!(settled.outcome_ref.as_deref(), Some("rev-1"));
    assert_eq!(rows(&store, "decision_records"), 1);
    assert_eq!(rows(&store, "event"), 1);

    // Stable event identity: a fresh rebuild names the same event, so a
    // retried publication is an exact replay with the same receipts.
    let again = publish(&mut store, &T2, &parts.package_from_seal(&read)).unwrap();
    assert!(again.replayed);
    assert_eq!(again.events, receipt.events);
    assert_eq!(again.claim, Some(settled.clone()));
    // The seal is still readable and unchanged after completion.
    assert_eq!(
        store.reader().find_review_seal("p", "claim-1").unwrap(),
        Some(seal)
    );
    // Terminal after a restart too.
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert_eq!(
        existing(claim(&mut store, &T2, "claim-1").unwrap()),
        settled
    );
}

#[test]
fn a_crash_before_the_seal_leaves_the_claim_explicitly_uncertain() {
    let (dir, mut store) = open_store("seal-uncertain");
    let _t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    // The claimant observed a result but crashed before sealing it.
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let record = existing(claim(&mut store, &T2, "claim-1").unwrap());
    assert_eq!(record.state, State::Claimed, "unfinished and uncertain");
    assert_eq!(
        store.reader().find_review_seal("p", "claim-1").unwrap(),
        None
    );
    // Nothing can seal or finish it any more: a seal needs the live ticket,
    // and a restart package needs a stored seal.
    let before = fingerprint(&store);
    let forged = ReviewSealRecordV1 {
        claim_id: "claim-1".into(),
        ..sealed_store("seal-uncertain-donor").2
    };
    let parts = verdict("pub-1", completed_reject());
    assert_eq!(
        publication_code(publish(&mut store, &T2, &parts.package_from_seal(&forged)).unwrap_err()),
        PCode::SealNotFound
    );
    assert_eq!(fingerprint(&store), before, "a forged seal writes nothing");
    assert_eq!(
        existing(claim(&mut store, &T2, "claim-1").unwrap()).state,
        State::Claimed
    );
}

#[test]
fn identical_reseal_is_idempotent_and_different_content_refuses() {
    let (_dir, mut store) = open_store("seal-reseal");
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let parts = verdict("pub-1", completed_reject());
    let first = seal_with(&mut store, &T1, &t, &parts.with_ticket(&t), b"journal-v1").unwrap();
    let sealed = fingerprint(&store);

    // Identical, even at a later time: the stored seal, nothing written.
    let again = seal_with(&mut store, &T2, &t, &parts.with_ticket(&t), b"journal-v1").unwrap();
    assert_eq!(again, first);
    assert_eq!(fingerprint(&store), sealed);

    // Different recovery bytes, a different package, a different disposition.
    let other_settlement = verdict(
        "pub-1",
        ReviewClaimSettlementV1::completed("PASS", Some("rev-1"), None).unwrap(),
    );
    let other_package = Parts {
        id: "pub-2".into(),
        settlement: completed_reject(),
    };
    for (package, bytes) in [
        (parts.with_ticket(&t), &b"journal-v2"[..]),
        (other_package.with_ticket(&t), b"journal-v1"),
        (other_settlement.with_ticket(&t), b"journal-v1"),
    ] {
        assert_eq!(
            publication_code(seal_with(&mut store, &T2, &t, &package, bytes).unwrap_err()),
            PCode::ConflictingRetry
        );
        assert_eq!(fingerprint(&store), sealed);
    }
}

#[test]
fn only_the_live_claimant_can_seal_and_inputs_are_bound() {
    let (dir, mut store) = open_store("seal-claimant");
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let other = ticket(claim(&mut store, &G0, "claim-2").unwrap());
    let parts = verdict("pub-1", completed_reject());
    let package = parts.with_ticket(&t);
    let before = fingerprint(&store);

    // A package that settles another claim than the request names.
    assert_eq!(
        publication_code(
            ReviewSealRequestV1::new(&claim_request("claim-2"), &package, recovery(b"x"))
                .unwrap_err()
        ),
        PCode::InvalidShape
    );
    // A package with no claim settlement, and one already bound to a seal.
    let bare = parts.base();
    assert_eq!(
        publication_code(
            ReviewSealRequestV1::new(&claim_request("claim-1"), &bare, recovery(b"x")).unwrap_err()
        ),
        PCode::InvalidShape
    );
    // Another claim's ticket cannot seal this claim's request.
    let request =
        ReviewSealRequestV1::new(&claim_request("claim-1"), &package, recovery(b"x")).unwrap();
    let admission = admission_for(&store, Op::ReviewClaim);
    assert_eq!(
        publication_code(
            store
                .seal_review_claim_v1(admission, &T1, &other, &request)
                .unwrap_err()
        ),
        PCode::InvalidShape
    );
    // Admission mismatches write nothing.
    let admission = admission_for(&store, Op::DecisionPublication);
    assert!(matches!(
        store
            .seal_review_claim_v1(admission, &T1, &t, &request)
            .unwrap_err(),
        StateError::WriteAuthorityRefused {
            reason: R::WrongOperation
        }
    ));
    let foreign_project = store.admit(crate::StateWriteSubjectV1 {
        operation: Op::ReviewClaim,
        project_id: "q".into(),
        graph_id: None,
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    });
    assert!(matches!(
        store
            .seal_review_claim_v1(foreign_project, &T1, &t, &request)
            .unwrap_err(),
        StateError::WriteAuthorityRefused {
            reason: R::ProjectMismatch
        }
    ));
    assert_eq!(fingerprint(&store), before);

    // A ticket, admission or request from before a reopen never transfers.
    let stale_admission = admission_for(&store, Op::ReviewClaim);
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert!(matches!(
        store
            .seal_review_claim_v1(stale_admission, &T1, &t, &request)
            .unwrap_err(),
        StateError::WriteAuthorityRefused {
            reason: R::WrongStore
        }
    ));
    let admission = admission_for(&store, Op::ReviewClaim);
    assert!(matches!(
        store
            .seal_review_claim_v1(admission, &T1, &t, &request)
            .unwrap_err(),
        StateError::WriteAuthorityRefused {
            reason: R::WrongStore
        }
    ));
    assert_eq!(rows(&store, "review_claim_seal"), 0);

    // A ticket minted by ANOTHER store for the same claim id is foreign, even
    // though the request itself is consistent with this store.
    let t3 = ticket(claim(&mut store, &T1, "claim-3").unwrap());
    let (_other_dir, mut other_store) = open_store("seal-foreign");
    let foreign = ticket(claim(&mut other_store, &G0, "claim-3").unwrap());
    let parts3 = verdict("pub-3", completed_reject());
    let request3 = ReviewSealRequestV1::new(
        &claim_request("claim-3"),
        &parts3.with_ticket(&t3),
        recovery(b"x"),
    )
    .unwrap();
    let admission = admission_for(&store, Op::ReviewClaim);
    assert!(matches!(
        store
            .seal_review_claim_v1(admission, &T1, &foreign, &request3)
            .unwrap_err(),
        StateError::WriteAuthorityRefused {
            reason: R::WrongStore
        }
    ));
    assert_eq!(rows(&store, "review_claim_seal"), 0);
    // The genuine claimant still can.
    let admission = admission_for(&store, Op::ReviewClaim);
    store
        .seal_review_claim_v1(admission, &T1, &t3, &request3)
        .unwrap();
}

#[test]
fn a_claim_with_a_different_request_cannot_be_sealed_under_that_claim_id() {
    let (_dir, mut store) = open_store("seal-binding");
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let parts = verdict("pub-1", completed_reject());
    let package = parts.with_ticket(&t);
    // The request claims a different task/request digest than was claimed.
    let wrong =
        ReviewClaimRequestV1::new("p", "claim-1", "t-OTHER", Some(SHA), &"b".repeat(64)).unwrap();
    let request = ReviewSealRequestV1::new(&wrong, &package, recovery(b"x")).unwrap();
    let admission = admission_for(&store, Op::ReviewClaim);
    assert_eq!(
        publication_code(
            store
                .seal_review_claim_v1(admission, &T1, &t, &request)
                .unwrap_err()
        ),
        PCode::ConflictingRetry
    );
    assert_eq!(rows(&store, "review_claim_seal"), 0);
}

#[test]
fn recovery_data_is_bounded_typed_and_digest_verified() {
    let ok = b"abc";
    assert!(ReviewRecoveryDataV1::new(KIND, ok.to_vec(), &sha256_hex(ok)).is_ok());
    for (kind, bytes, digest) in [
        (KIND, ok.to_vec(), "0".repeat(64)),
        (KIND, ok.to_vec(), sha256_hex(b"other")),
        (KIND, Vec::new(), sha256_hex(b"")),
        ("lower", ok.to_vec(), sha256_hex(ok)),
        ("", ok.to_vec(), sha256_hex(ok)),
    ] {
        assert_eq!(
            publication_code(ReviewRecoveryDataV1::new(kind, bytes, &digest).unwrap_err()),
            PCode::InvalidShape
        );
    }
    let big = vec![7u8; crate::review_claim::MAX_RECOVERY_BYTES + 1];
    assert_eq!(
        publication_code(
            ReviewRecoveryDataV1::new(KIND, big.clone(), &sha256_hex(&big)).unwrap_err()
        ),
        PCode::InvalidShape
    );
    let max = vec![7u8; crate::review_claim::MAX_RECOVERY_BYTES];
    assert!(ReviewRecoveryDataV1::new(KIND, max.clone(), &sha256_hex(&max)).is_ok());
}

#[test]
fn a_sealed_claim_is_finished_only_by_its_sealed_package() {
    let (_dir, mut store, seal, parts, _t) = sealed_store("seal-gate");
    let sealed = fingerprint(&store);

    // A different package for the same claim: different records.
    let pass = DecisionPublicationV1::new(
        "p",
        "pub-1",
        vec![record(
            DecisionRecordKind::A4Review,
            "rev-2",
            &format!(
                r#"{{"review_id":"rev-2","task_id":"t-1","reviewed_sha":"{SHA}","verdict":"PASS","independence_attested":true,"blocking_findings":[],"nonblocking_findings":[],"dimensions":[{{"dimension":"CORRECTNESS","assessment":"SATISFIED"}}]}}"#
            ),
        )],
        vec![event(
            EventType::ReviewPassed,
            "sha256:dd",
            "01ARZ3NDEKTSV4RRFFQ69G5FAW",
        )],
        vec![],
    )
    .unwrap()
    .with_sealed_claim(&seal)
    .unwrap();
    assert_eq!(
        publication_code(publish(&mut store, &T2, &pass).unwrap_err()),
        PCode::SealMismatch
    );
    // Same records, a different publication identity.
    let renamed = Parts {
        id: "pub-OTHER".into(),
        settlement: completed_reject(),
    };
    assert_eq!(
        publication_code(publish(&mut store, &T2, &renamed.package_from_seal(&seal)).unwrap_err()),
        PCode::SealMismatch
    );
    // A forged disposition on an otherwise identical package changes its digest.
    let forged = ReviewSealRecordV1 {
        settlement: ReviewClaimSettlementV1::completed("PASS", Some("rev-1"), None).unwrap(),
        ..seal.clone()
    };
    assert_eq!(
        publication_code(publish(&mut store, &T2, &parts.package_from_seal(&forged)).unwrap_err()),
        PCode::SealMismatch
    );
    assert_eq!(fingerprint(&store), sealed, "nothing partial leaked");

    // The sealed package itself completes it.
    publish(&mut store, &T2, &parts.package_from_seal(&seal)).unwrap();
}

#[test]
fn plain_settlement_of_a_sealed_claim_is_refused_for_the_live_ticket_too() {
    let (_dir, mut store) = open_store("seal-plain-settle");
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let parts = verdict("pub-1", completed_reject());
    seal_with(&mut store, &T1, &t, &parts.with_ticket(&t), b"journal-v1").unwrap();
    let sealed = fingerprint(&store);
    let admission = admission_for(&store, Op::ReviewClaim);
    let error = store
        .settle_review_claim_v1(
            admission,
            &T2,
            &t,
            &ReviewClaimSettlementV1::completed("PASS", None, None).unwrap(),
        )
        .unwrap_err();
    assert_eq!(publication_code(error), PCode::ClaimSealed);
    // And so is a ticket package that is not the sealed one.
    let other = Parts {
        id: "pub-2".into(),
        settlement: completed_reject(),
    };
    assert_eq!(
        publication_code(publish(&mut store, &T2, &other.with_ticket(&t)).unwrap_err()),
        PCode::SealMismatch
    );
    assert_eq!(fingerprint(&store), sealed);
    // The claimant's own sealed package works through its ticket as well.
    publish(&mut store, &T2, &parts.with_ticket(&t)).unwrap();
}

#[test]
fn terminal_claims_stay_terminal_for_seals_and_publications() {
    // Completed and sealed: an identical reseal and an identical settlement
    // are accepted without writing; anything different refuses.
    let (_dir, mut store, seal, parts, t) = sealed_store("seal-terminal");
    let receipt = publish(&mut store, &T2, &parts.package_from_seal(&seal)).unwrap();
    assert_eq!(receipt.claim.as_ref().unwrap().state, State::Completed);
    let done = fingerprint(&store);
    let again = seal_with(&mut store, &T2, &t, &parts.with_ticket(&t), b"journal-v1").unwrap();
    assert_eq!(again, seal);
    assert_eq!(
        publication_code(
            seal_with(&mut store, &T2, &t, &parts.with_ticket(&t), b"journal-v2").unwrap_err()
        ),
        PCode::ConflictingRetry
    );
    let admission = admission_for(&store, Op::ReviewClaim);
    assert_eq!(
        store
            .settle_review_claim_v1(admission, &T2, &t, &completed_reject())
            .unwrap(),
        receipt.claim.unwrap()
    );
    let admission = admission_for(&store, Op::ReviewClaim);
    assert_eq!(
        publication_code(
            store
                .settle_review_claim_v1(
                    admission,
                    &T2,
                    &t,
                    &ReviewClaimSettlementV1::completed("PASS", None, None).unwrap()
                )
                .unwrap_err()
        ),
        PCode::ClaimNotUnfinished
    );
    assert_eq!(fingerprint(&store), done);

    // Abandoned before a seal: no seal, no completion.
    let (_dir, mut store) = open_store("seal-abandoned");
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let admission = admission_for(&store, Op::ReviewClaim);
    store
        .abandon_review_claim_v1(
            admission,
            &T1,
            &ReviewClaimAbandonV1::new("p", "claim-1", "human:operator-1").unwrap(),
        )
        .unwrap();
    let parts = verdict("pub-1", completed_reject());
    assert_eq!(
        publication_code(
            seal_with(&mut store, &T2, &t, &parts.with_ticket(&t), b"late").unwrap_err()
        ),
        PCode::ClaimNotUnfinished
    );
    assert_eq!(rows(&store, "review_claim_seal"), 0);
}

#[test]
fn abandoning_a_sealed_claim_is_terminal_and_the_sealed_package_can_no_longer_publish() {
    let (_dir, mut store, seal, parts, _t) = sealed_store("seal-abandon-sealed");
    let admission = admission_for(&store, Op::ReviewClaim);
    let abandoned = store
        .abandon_review_claim_v1(
            admission,
            &T1,
            &ReviewClaimAbandonV1::new("p", "claim-1", "human:operator-1").unwrap(),
        )
        .unwrap();
    assert_eq!(abandoned.state, State::Abandoned);
    let after = fingerprint(&store);
    assert_eq!(
        publication_code(publish(&mut store, &T2, &parts.package_from_seal(&seal)).unwrap_err()),
        PCode::ClaimNotUnfinished
    );
    assert_eq!(fingerprint(&store), after, "no verdict record or event");
    assert_eq!(rows(&store, "decision_records"), 0);
    // The seal itself is immutable history.
    assert_eq!(
        store.reader().find_review_seal("p", "claim-1").unwrap(),
        Some(seal)
    );
}

#[test]
fn policy_blocked_and_reject_stay_immutable_after_a_sealed_publication() {
    // POLICY_BLOCKED: a sealed refusal completes the claim as REFUSED.
    let (dir, mut store) = open_store("seal-policy");
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let blocked = verdict(
        "pub-blocked",
        ReviewClaimSettlementV1::refused("POLICY_BLOCKED", None, None).unwrap(),
    );
    let seal = seal_with(&mut store, &T1, &t, &blocked.with_ticket(&t), b"policy-v1").unwrap();
    drop(store);
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let receipt = publish(&mut store, &T2, &blocked.package_from_seal(&seal)).unwrap();
    let record = receipt.claim.unwrap();
    assert_eq!(record.state, State::Refused);
    assert_eq!(record.outcome_kind.as_deref(), Some("POLICY_BLOCKED"));
    // A later callback sees the retained refusal and never re-routes.
    assert_eq!(existing(claim(&mut store, &T2, "claim-1").unwrap()), record);
}

#[test]
fn reject_survives_a_later_conflicting_sealed_pass() {
    let (_dir, mut store, seal, parts, _t) = sealed_store("seal-reject-immutable");
    publish(&mut store, &T2, &parts.package_from_seal(&seal)).unwrap();
    let applied = fingerprint(&store);

    let t2 = ticket(claim(&mut store, &T2, "claim-2").unwrap());
    let pass = DecisionPublicationV1::new(
        "p",
        "pub-pass",
        vec![crate::atomic_publication_tests::pass_review()],
        vec![event(
            EventType::ReviewPassed,
            "sha256:dd",
            "01ARZ3NDEKTSV4RRFFQ69G5FAW",
        )],
        vec![],
    )
    .unwrap()
    .with_claim_settlement(
        &t2,
        ReviewClaimSettlementV1::completed("PASS", Some("rev-1"), None).unwrap(),
    )
    .unwrap();
    let seal2 = seal_with(&mut store, &T2, &t2, &pass, b"pass-journal").unwrap();
    let sealed = fingerprint(&store);
    // Publishing refuses: the stored REJECT is never replaced and no PASS
    // event, receipt or watermark leaks.
    let rebuilt = DecisionPublicationV1::new(
        "p",
        "pub-pass",
        vec![crate::atomic_publication_tests::pass_review()],
        vec![event(
            EventType::ReviewPassed,
            "sha256:dd",
            "01ARZ3NDEKTSV4RRFFQ69G5FAW",
        )],
        vec![],
    )
    .unwrap()
    .with_sealed_claim(&seal2)
    .unwrap();
    assert!(matches!(
        publish(&mut store, &T2, &rebuilt).unwrap_err(),
        StateError::DecisionRecord {
            code: RCode::AlreadyExists,
            ..
        }
    ));
    assert_eq!(fingerprint(&store), sealed);
    assert_ne!(applied, sealed, "only the seal row differs");
    assert!(
        store
            .reader()
            .find_decision_record("p", DecisionRecordKind::A4Review, "rev-1")
            .unwrap()
            .unwrap()
            .record_json()
            .contains(r#""verdict":"REJECT""#)
    );
    assert_eq!(
        existing(claim(&mut store, &T2, "claim-2").unwrap()).state,
        State::Claimed
    );
}

#[test]
fn seal_is_atomic_at_every_write_boundary() {
    let boundaries = assert_atomic_at_every_boundary(
        || {
            let (dir, mut store) = open_store("seal-fault");
            let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
            (dir, store, t)
        },
        |(_, store, _)| fingerprint(store),
        |(_, store, t)| {
            let parts = verdict("pub-1", completed_reject());
            seal_with(store, &T1, t, &parts.with_ticket(t), b"journal-v1").map(|_| ())
        },
    );
    // the watermark and the seal row each have their own boundary
    assert!(boundaries >= 2, "{boundaries}");
}

#[test]
fn sealed_restart_publication_is_atomic_at_every_write_boundary() {
    let boundaries = assert_atomic_at_every_boundary(
        || {
            let (dir, store, seal, parts, _t) = sealed_store("seal-publish-fault");
            (dir, store, seal, parts)
        },
        |(_, store, _, _)| fingerprint(store),
        |(_, store, seal, parts)| {
            publish(store, &T2, &parts.package_from_seal(seal)).map(|receipt| {
                assert_eq!(receipt.claim.unwrap().state, State::Completed);
            })
        },
    );
    // watermark, publication row, record, event and the claim settlement
    assert!(boundaries >= 5, "{boundaries}");
}

#[test]
fn racing_connections_seal_a_claim_exactly_once() {
    let (dir, mut store) = open_store("seal-race");
    let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
    let parts = verdict("pub-1", completed_reject());
    let package = parts.with_ticket(&t);
    drop(store);
    let barrier = Arc::new(Barrier::new(6));
    let handles: Vec<_> = (0..6)
        .map(|n| {
            let path = dir.db();
            let barrier = Arc::clone(&barrier);
            let request = ReviewSealRequestV1::new(
                &claim_request("claim-1"),
                &package,
                recovery(format!("journal-{n}").as_bytes()),
            )
            .unwrap();
            std::thread::spawn(move || {
                let mut repo = SqliteStateRepository::open(path).unwrap();
                barrier.wait();
                repo.seal_review_claim_v1(&T1, &request)
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let winners = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(winners, 1, "exactly one seal: {results:?}");
    for loser in results.iter().filter_map(|r| r.as_ref().err()) {
        assert!(matches!(
            loser,
            StateError::Publication {
                code: PCode::ConflictingRetry,
                ..
            }
        ));
    }
    let repo = SqliteStateRepository::open(dir.db()).unwrap();
    assert_eq!(repo.count_table_rows("review_claim_seal").unwrap(), 1);
}

#[test]
fn a_seal_racing_an_abandonment_leaves_one_consistent_end_state() {
    for round in 0..4 {
        let (dir, mut store) = open_store("seal-vs-abandon");
        let t = ticket(claim(&mut store, &G0, "claim-1").unwrap());
        let parts = verdict("pub-1", completed_reject());
        let request = ReviewSealRequestV1::new(
            &claim_request("claim-1"),
            &parts.with_ticket(&t),
            recovery(b"journal"),
        )
        .unwrap();
        drop(store);
        let barrier = Arc::new(Barrier::new(2));
        let sealer = {
            let (path, barrier) = (dir.db(), Arc::clone(&barrier));
            std::thread::spawn(move || {
                let mut repo = SqliteStateRepository::open(path).unwrap();
                barrier.wait();
                repo.seal_review_claim_v1(&T1, &request)
            })
        };
        let abandoner = {
            let (path, barrier) = (dir.db(), Arc::clone(&barrier));
            std::thread::spawn(move || {
                let mut repo = SqliteStateRepository::open(path).unwrap();
                barrier.wait();
                repo.abandon_review_claim_v1(
                    &T1,
                    &ReviewClaimAbandonV1::new("p", "claim-1", "human:operator-1").unwrap(),
                )
            })
        };
        let sealed = sealer.join().unwrap();
        abandoner.join().unwrap().unwrap();
        let repo = SqliteStateRepository::open(dir.db()).unwrap();
        let claim = repo.find_review_claim("p", "claim-1").unwrap().unwrap();
        assert_eq!(claim.state, State::Abandoned, "round {round}");
        let seal = repo.find_review_seal("p", "claim-1").unwrap();
        match sealed {
            Ok(_) => assert!(seal.is_some(), "sealed first, then abandoned"),
            Err(StateError::Publication {
                code: PCode::ClaimNotUnfinished,
                ..
            }) => assert!(seal.is_none(), "abandoned first, so no seal"),
            Err(other) => panic!("{other:?}"),
        }
    }
}

#[test]
fn seal_rows_enforce_their_own_guards_below_the_api() {
    let (_dir, mut store, _seal, _parts, _t) = sealed_store("seal-sql");
    let conn = store.reader().connection();
    for sql in [
        "UPDATE review_claim_seal SET recovery_data=x'00'",
        "UPDATE review_claim_seal SET package_sha256=lower(hex(zeroblob(32)))",
        "DELETE FROM review_claim_seal",
        "INSERT OR REPLACE INTO review_claim_seal SELECT * FROM review_claim_seal",
    ] {
        assert!(conn.execute_batch(sql).is_err(), "{sql}");
    }
    // A seal for a claim that does not exist, is not CLAIMED or names another
    // task/request is refused by the schema itself.
    let insert = |claim: &str, task: &str, publication: &str| {
        format!(
            "INSERT INTO review_claim_seal(project_id,claim_id,task_id,subject_sha,request_sha256,publication_id,package_sha256,resolution,outcome_kind,recovery_kind,recovery_data,recovery_sha256,seal_sha256,sealed_at) VALUES ('p','{claim}','{task}','{SHA}','{b}','{publication}','{b}','COMPLETED','REJECT','K',x'01','{b}','{b}','x')",
            b = "b".repeat(64)
        )
    };
    assert!(
        conn.execute_batch(&insert("missing", "t-1", "pub-9"))
            .is_err()
    );
    let claim2 = {
        let admission = admission_for(&store, Op::ReviewClaim);
        let outcome = store
            .claim_review_v1(
                admission,
                &T2,
                &ReviewClaimRequestV1::new("p", "claim-2", "t-1", Some(SHA), &"b".repeat(64))
                    .unwrap(),
            )
            .unwrap();
        ticket(outcome)
    };
    let conn = store.reader().connection();
    assert!(
        conn.execute_batch(&insert("claim-2", "t-WRONG", "pub-9"))
            .is_err()
    );
    // Matching claim, but the publication identity is already sealed elsewhere.
    assert!(
        conn.execute_batch(&insert("claim-2", "t-1", "pub-1"))
            .is_err()
    );
    // A valid insert works, proving the rejections above were the guards.
    conn.execute_batch(&insert("claim-2", "t-1", "pub-9"))
        .unwrap();
    let _ = claim2;
    assert_eq!(rows(&store, "review_claim_seal"), 2);
}

#[test]
fn a_tampered_seal_fails_closed_on_readback() {
    let (_dir, store, _seal, _parts, _t) = sealed_store("seal-tamper");
    let conn = store.reader().connection();
    conn.execute_batch("DROP TRIGGER review_claim_seal_no_update")
        .unwrap();
    conn.execute_batch("UPDATE review_claim_seal SET recovery_data=x'6a6f75726e616c2d7632'")
        .unwrap();
    assert!(matches!(
        store.reader().find_review_seal("p", "claim-1").unwrap_err(),
        StateError::Publication {
            code: PCode::CorruptStore,
            ..
        }
    ));
}

#[test]
fn stable_event_ids_are_deterministic_and_distinct() {
    let make = |publication: &str, key: &str| {
        PublicationEventV1::stable(
            proposal(EventType::ReviewRejected, "sha256:cc"),
            publication,
            key,
        )
        .unwrap()
    };
    let a = make("pub-1", "k1");
    assert_eq!(a.event_id(), make("pub-1", "k1").event_id());
    assert_ne!(a.event_id(), make("pub-1", "k2").event_id());
    assert_ne!(a.event_id(), make("pub-2", "k1").event_id());
    assert!(
        PublicationEventV1::stable(proposal(EventType::ReviewRejected, "sha256:cc"), "", "k1")
            .is_err()
    );
}
