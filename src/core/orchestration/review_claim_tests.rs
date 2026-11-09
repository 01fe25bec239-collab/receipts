//! Review claim → seal → publish through the real Core→State path: real
//! SQLite stores, the real `CoreStateWriter`, reopen as the restart. No file
//! journal and no State self-minting.

use std::sync::{Arc, Mutex};

use receipts_state::{
    PublicationFailureCode as Code, ReviewClaimSettlementV1, ReviewClaimStateV1 as S, StateError,
    StateWriteRefusalV1,
};

use super::integration_fixtures::{
    AUTHORITY, Codec, Content, Spec, admitted, event_ids, image, open, policy, publication_code,
    state, unit,
};
use super::{
    CorePolicyRefusal, CoreRecoveredReviewV1, CoreRecoveryCodecV1, CoreReviewClaimV1,
    CoreReviewUnitV1,
};
use crate::test_support::{CLOCK, Dir, identity};

fn acquired(claim: CoreReviewClaimV1) {
    assert_eq!(claim, CoreReviewClaimV1::Acquired);
}

#[test]
fn claim_ids_are_deterministic_and_exactly_one_caller_acquires_the_claim() {
    assert_eq!(unit(1).claim_id(), unit(1).claim_id());
    assert_ne!(unit(1).claim_id(), unit(2).claim_id());
    assert_ne!(
        unit(1).claim_id(),
        CoreReviewUnitV1::new(
            "p",
            "t-2",
            Some(super::integration_fixtures::SHA),
            &"b".repeat(64),
            1
        )
        .unwrap()
        .claim_id()
    );
    assert!(CoreReviewUnitV1::new("p", "t-1", None, &"b".repeat(64), 0).is_err());
    assert!(CoreReviewUnitV1::new("p", "t-1", Some("short"), &"b".repeat(64), 1).is_err());

    let dir = Dir::new();
    let writer = Arc::new(Mutex::new(open(&dir, &admitted())));
    let acquisitions: usize = (0..8)
        .map(|_| {
            let writer = Arc::clone(&writer);
            std::thread::spawn(move || {
                writer
                    .lock()
                    .unwrap()
                    .claim_review(&CLOCK, &unit(1))
                    .unwrap()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|claim| *claim == CoreReviewClaimV1::Acquired)
        .count();
    assert_eq!(acquisitions, 1, "exactly one caller may execute");
    let mut writer = writer.lock().unwrap();
    // Every later callback sees the unfinished claim, never a second grant.
    match writer.claim_review(&CLOCK, &unit(1)).unwrap() {
        CoreReviewClaimV1::InFlight(record) => {
            assert_eq!(record.state, S::Claimed);
            assert_eq!(record.claim_id, unit(1).claim_id());
        }
        other => panic!("expected InFlight, got {other:?}"),
    }
    // A project the composition did not admit is refused before State.
    let foreign = CoreReviewUnitV1::new("q", "t-1", None, &"b".repeat(64), 1).unwrap();
    assert_eq!(
        policy(writer.claim_review(&CLOCK, &foreign)),
        CorePolicyRefusal::ProjectNotAdmitted
    );
}

#[test]
fn executed_outcome_follows_claim_seal_publish_and_replays_exactly() {
    let (dir, spec, u) = (Dir::new(), Spec::standard(), unit(1));
    let mut writer = open(&dir, &admitted());
    let content = Content::standard();
    assert_eq!(image(&writer, &u, &spec).claim, None);
    // Nothing is sealed or publishable before the claim executes.
    assert_eq!(
        policy(writer.publish_sealed_review(&CLOCK, &content, &Codec, &u)),
        CorePolicyRefusal::NotSealed
    );
    acquired(writer.claim_review(&CLOCK, &u).unwrap());

    let seal = writer
        .seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())
        .unwrap();
    assert_eq!(seal.recovery_data, spec.bytes());
    assert_eq!(seal.recovery_kind, "REVIEW_SEALED_V1");
    assert_eq!(seal.publication_id, u.publication_id());
    // Sealing publishes nothing and the claim is still unfinished.
    let sealed_image = image(&writer, &u, &spec);
    assert_eq!(sealed_image.review, None);
    assert_eq!(sealed_image.events, [false, false]);
    assert_eq!(sealed_image.claim.as_ref().unwrap().state, S::Claimed);
    // An identical reseal is idempotent and writes nothing.
    assert_eq!(
        writer
            .seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())
            .unwrap(),
        seal
    );
    assert_eq!(image(&writer, &u, &spec), sealed_image);

    let receipt = writer
        .publish_sealed_review(&CLOCK, &content, &Codec, &u)
        .unwrap();
    assert!(!receipt.replayed);
    let claim = receipt.claim.clone().unwrap();
    assert_eq!(claim.state, S::Completed);
    assert_eq!(claim.outcome_kind.as_deref(), Some("REVIEWED"));
    assert_eq!(claim.outcome_ref.as_deref(), Some("rev-1"));
    let ids: Vec<_> = receipt.events.iter().map(|e| e.event_id.clone()).collect();
    assert_eq!(ids, event_ids(&u, &spec));
    let done = image(&writer, &u, &spec);
    assert!(done.review.is_some());
    assert_eq!(done.events, [true, true]);

    // Later callbacks see the finished, sealed outcome and never re-execute.
    match writer.claim_review(&CLOCK, &u).unwrap() {
        CoreReviewClaimV1::Sealed(sealed) => {
            assert_eq!(sealed.claim.state, S::Completed);
            assert_eq!(sealed.seal, seal);
        }
        other => panic!("expected Sealed, got {other:?}"),
    }
    // The exact retry replays the recorded receipt and changes nothing.
    let replay = writer
        .publish_sealed_review(&CLOCK, &content, &Codec, &u)
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.events, receipt.events);
    assert_eq!(replay.recorded_at, receipt.recorded_at);
    assert_eq!(image(&writer, &u, &spec), done);
    // The live ticket is gone with the finished claim.
    assert_eq!(
        policy(writer.seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())),
        CorePolicyRefusal::NoLiveClaim
    );
}

/// A codec whose decoding disagrees with what was sealed.
struct Lying(fn(&mut CoreRecoveredReviewV1));
impl CoreRecoveryCodecV1 for Lying {
    fn kind(&self) -> &str {
        "REVIEW_SEALED_V1"
    }
    fn decode(&self, project: &str, bytes: &[u8]) -> Result<CoreRecoveredReviewV1, String> {
        let mut recovered = Codec.decode(project, bytes)?;
        (self.0)(&mut recovered);
        Ok(recovered)
    }
}

struct OtherKind;
impl CoreRecoveryCodecV1 for OtherKind {
    fn kind(&self) -> &str {
        "OTHER_FORMAT_V1"
    }
    fn decode(&self, project: &str, bytes: &[u8]) -> Result<CoreRecoveredReviewV1, String> {
        Codec.decode(project, bytes)
    }
}

#[test]
fn invalid_or_conflicting_recovery_data_never_seals_and_a_conflicting_reseal_refuses() {
    let (dir, spec, u) = (Dir::new(), Spec::standard(), unit(1));
    let mut writer = open(&dir, &admitted());
    let content = Content::standard();
    acquired(writer.claim_review(&CLOCK, &u).unwrap());
    let claimed = image(&writer, &u, &spec);

    let mut foreign = Spec::standard();
    foreign.project = "q".into();
    let mut bad_digest = Spec::standard();
    bad_digest.events[1].2 = super::integration_fixtures::digest(b"other bytes");
    let mut unresolved = Spec::standard();
    unresolved.events[0].1 = "artifact:never-created".into();
    let mut unsupported = Spec::standard();
    unsupported.events[0].2 = "md5:abc".into();
    let mut empty = Spec::standard();
    (empty.verdict, empty.events) = (String::new(), Vec::new());
    // Recovery data that does not decode, a foreign project, a format of
    // another kind and every content refusal seal nothing.
    assert_eq!(
        policy(writer.seal_review(&CLOCK, &content, &Codec, &u, b"not a review".to_vec())),
        CorePolicyRefusal::RecoveryDataInvalid
    );
    assert_eq!(
        policy(writer.seal_review(&CLOCK, &content, &Codec, &u, foreign.bytes())),
        CorePolicyRefusal::RecoveryDataInvalid
    );
    assert_eq!(
        policy(writer.seal_review(&CLOCK, &content, &Codec, &u, bad_digest.bytes())),
        CorePolicyRefusal::ContentDigestMismatch
    );
    assert_eq!(
        policy(writer.seal_review(&CLOCK, &content, &Codec, &u, unresolved.bytes())),
        CorePolicyRefusal::ContentUnavailable
    );
    assert_eq!(
        policy(writer.seal_review(&CLOCK, &content, &Codec, &u, unsupported.bytes())),
        CorePolicyRefusal::UnsupportedDigest
    );
    assert_eq!(
        publication_code(writer.seal_review(&CLOCK, &content, &Codec, &u, empty.bytes())),
        Code::InvalidShape
    );
    assert_eq!(image(&writer, &u, &spec), claimed, "nothing was sealed");

    // The valid seal; any other bytes, package or disposition then conflict.
    writer
        .seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())
        .unwrap();
    let sealed = image(&writer, &u, &spec);
    let mut other = Spec::standard();
    other.verdict = "REJECT".into();
    let mut refused = Spec::standard();
    (refused.refused, refused.kind) = (true, "POLICY_BLOCKED".into());
    for conflicting in [other, refused] {
        assert_eq!(
            publication_code(writer.seal_review(&CLOCK, &content, &Codec, &u, conflicting.bytes())),
            Code::ConflictingRetry
        );
    }
    assert_eq!(image(&writer, &u, &spec), sealed, "the first seal stands");
}

#[test]
fn restart_publishes_only_the_sealed_package_and_never_executes_again() {
    let (dir, spec, u) = (Dir::new(), Spec::standard(), unit(1));
    let id = admitted();
    let mut executions = 0;
    let mut writer = open(&dir, &id);
    let content = Content::standard();
    if writer.claim_review(&CLOCK, &u).unwrap() == CoreReviewClaimV1::Acquired {
        executions += 1;
    }
    writer
        .seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())
        .unwrap();
    drop(writer); // crash after the seal, before the publication

    let mut writer = open(&dir, &id);
    let sealed = writer
        .sealed_review(&u)
        .unwrap()
        .expect("sealed after restart");
    assert_eq!(sealed.claim.state, S::Claimed);
    assert_eq!(sealed.seal.recovery_data, spec.bytes());
    let before = image(&writer, &u, &spec);
    assert_eq!(before.review, None);
    match writer.claim_review(&CLOCK, &u).unwrap() {
        CoreReviewClaimV1::Sealed(again) => assert_eq!(*again, sealed),
        other => panic!("expected Sealed, got {other:?}"),
    }
    // The live ticket is gone: this process can neither reseal nor settle.
    assert_eq!(
        policy(writer.seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())),
        CorePolicyRefusal::NoLiveClaim
    );
    // A rebuild must reproduce exactly what was sealed.
    let tamper: [fn(&mut CoreRecoveredReviewV1); 3] = [
        |r| r.input.records[0] = super::integration_fixtures::review_record("p", "REJECT"),
        |r| r.input.events.truncate(1),
        |r| r.settlement = ReviewClaimSettlementV1::refused("POLICY_BLOCKED", None, None).unwrap(),
    ];
    for t in tamper {
        assert_eq!(
            policy(writer.publish_sealed_review(&CLOCK, &content, &Lying(t), &u)),
            CorePolicyRefusal::RecoveryDataInvalid
        );
        assert_eq!(image(&writer, &u, &spec), before, "no partial publication");
    }
    // A codec of another kind, or content that no longer resolves, publishes nothing.
    assert_eq!(
        policy(writer.publish_sealed_review(&CLOCK, &content, &OtherKind, &u)),
        CorePolicyRefusal::RecoveryDataInvalid
    );
    let missing = Content(vec![(
        "p",
        super::integration_fixtures::REQUEST,
        super::integration_fixtures::REQUEST_BYTES,
    )]);
    assert_eq!(
        policy(writer.publish_sealed_review(&CLOCK, &missing, &Codec, &u)),
        CorePolicyRefusal::ContentUnavailable
    );
    assert_eq!(image(&writer, &u, &spec), before);

    let receipt = writer
        .publish_sealed_review(&CLOCK, &content, &Codec, &u)
        .unwrap();
    assert!(!receipt.replayed);
    assert_eq!(receipt.claim.unwrap().state, S::Completed);
    assert_eq!(image(&writer, &u, &spec).events, [true, true]);
    assert_eq!(executions, 1, "the reviewer ran once, before the restart");
}

#[test]
fn an_unsealed_claim_is_uncertain_after_restart_and_only_authorized_abandonment_ends_it() {
    let (dir, spec, u) = (Dir::new(), Spec::standard(), unit(1));
    let id = admitted();
    let mut writer = open(&dir, &id);
    let content = Content::standard();
    acquired(writer.claim_review(&CLOCK, &u).unwrap());
    drop(writer); // crash after the claim, before any seal

    let mut writer = open(&dir, &id);
    let uncertain = image(&writer, &u, &spec);
    assert_eq!(uncertain.claim.as_ref().unwrap().state, S::Claimed);
    assert!(matches!(
        writer.claim_review(&CLOCK, &u).unwrap(),
        CoreReviewClaimV1::InFlight(_)
    ));
    assert_eq!(
        policy(writer.seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())),
        CorePolicyRefusal::NoLiveClaim
    );
    assert_eq!(
        policy(writer.publish_sealed_review(&CLOCK, &content, &Codec, &u)),
        CorePolicyRefusal::NotSealed
    );
    assert_eq!(
        policy(writer.settle_review_refusal(
            &CLOCK,
            &u,
            &ReviewClaimSettlementV1::refused("POLICY_BLOCKED", None, None).unwrap()
        )),
        CorePolicyRefusal::NoLiveClaim
    );

    // A name is not an authority: only a grant from this writer's admitted
    // identity, for this claim, abandons it.
    let foreign_identity = identity("policy").with_abandonment_authority("p", AUTHORITY);
    let no_authority = identity("policy");
    assert!(no_authority.abandonment_grant("p", &u.claim_id()).is_none());
    assert!(id.abandonment_grant("q", &u.claim_id()).is_none());
    let refused = [
        // The same admitted name, but minted by another identity object.
        foreign_identity
            .abandonment_grant("p", &u.claim_id())
            .unwrap(),
        // A grant for another claim.
        id.abandonment_grant("p", &unit(2).claim_id()).unwrap(),
    ];
    for grant in refused {
        assert_eq!(
            policy(writer.abandon_review_claim(&CLOCK, &u, grant)),
            CorePolicyRefusal::AbandonmentNotAuthorized
        );
        assert_eq!(image(&writer, &u, &spec), uncertain, "still unfinished");
    }
    // A writer whose identity admits no authority refuses even its own clone's grant.
    drop(writer);
    let bare = identity("policy");
    let mut writer = open(&dir, &bare);
    let grant = id.abandonment_grant("p", &u.claim_id()).unwrap();
    assert_eq!(
        policy(writer.abandon_review_claim(&CLOCK, &u, grant)),
        CorePolicyRefusal::AbandonmentNotAuthorized
    );
    assert_eq!(image(&writer, &u, &spec), uncertain);
    drop(writer);

    let mut writer = open(&dir, &id);
    let grant = id.abandonment_grant("p", &u.claim_id()).unwrap();
    let abandoned = writer.abandon_review_claim(&CLOCK, &u, grant).unwrap();
    assert_eq!(abandoned.state, S::Abandoned);
    assert_eq!(abandoned.authority_ref.as_deref(), Some(AUTHORITY));
    assert!(matches!(
        writer.claim_review(&CLOCK, &u).unwrap(),
        CoreReviewClaimV1::Abandoned(_)
    ));
    // Abandonment re-ran nothing; the work needs a new generation.
    acquired(writer.claim_review(&CLOCK, &unit(2)).unwrap());
}

#[test]
fn terminal_policy_refusal_is_retained_and_completion_cannot_be_settled_plainly() {
    let (dir, spec, u) = (Dir::new(), Spec::standard(), unit(1));
    let mut writer = open(&dir, &admitted());
    let content = Content::standard();
    acquired(writer.claim_review(&CLOCK, &u).unwrap());
    // A completed review is only ever a published seal.
    let completed = ReviewClaimSettlementV1::completed("REVIEWED", Some("rev-1"), None).unwrap();
    assert_eq!(
        policy(writer.settle_review_refusal(&CLOCK, &u, &completed)),
        CorePolicyRefusal::CompletionRequiresPublication
    );
    assert_eq!(image(&writer, &u, &spec).claim.unwrap().state, S::Claimed);

    let blocked = ReviewClaimSettlementV1::refused("POLICY_BLOCKED", None, None).unwrap();
    let record = writer.settle_review_refusal(&CLOCK, &u, &blocked).unwrap();
    assert_eq!(record.state, S::Refused);
    assert_eq!(record.outcome_kind.as_deref(), Some("POLICY_BLOCKED"));
    // Later callbacks see the retained refusal; nothing executes or seals.
    assert!(matches!(
        writer.claim_review(&CLOCK, &u).unwrap(),
        CoreReviewClaimV1::Terminal(r) if r.state == S::Refused
    ));
    assert_eq!(
        policy(writer.seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())),
        CorePolicyRefusal::NoLiveClaim
    );
    assert_eq!(
        policy(writer.settle_review_refusal(&CLOCK, &u, &blocked)),
        CorePolicyRefusal::NoLiveClaim
    );

    // A sealed refusal publishes its records and finishes the claim as REFUSED.
    let u2 = unit(2);
    let mut sealed_refusal = Spec::standard();
    (sealed_refusal.refused, sealed_refusal.kind) = (true, "POLICY_BLOCKED".into());
    acquired(writer.claim_review(&CLOCK, &u2).unwrap());
    writer
        .seal_review(&CLOCK, &content, &Codec, &u2, sealed_refusal.bytes())
        .unwrap();
    let receipt = writer
        .publish_sealed_review(&CLOCK, &content, &Codec, &u2)
        .unwrap();
    assert_eq!(receipt.claim.unwrap().state, S::Refused);
    assert!(matches!(
        writer.claim_review(&CLOCK, &u2).unwrap(),
        CoreReviewClaimV1::Sealed(s) if s.claim.state == S::Refused
    ));
    assert_eq!(image(&writer, &u2, &sealed_refusal).events, [true, true]);
}

#[test]
fn plain_settlement_cannot_discard_a_sealed_claim() {
    let (dir, spec, u) = (Dir::new(), Spec::standard(), unit(1));
    let mut writer = open(&dir, &admitted());
    let content = Content::standard();
    acquired(writer.claim_review(&CLOCK, &u).unwrap());
    writer
        .seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())
        .unwrap();
    let sealed = image(&writer, &u, &spec);
    let blocked = ReviewClaimSettlementV1::refused("POLICY_BLOCKED", None, None).unwrap();
    assert_eq!(
        publication_code(writer.settle_review_refusal(&CLOCK, &u, &blocked)),
        Code::ClaimSealed
    );
    assert_eq!(image(&writer, &u, &spec), sealed);
    // The sealed package still publishes, with its records and events.
    writer
        .publish_sealed_review(&CLOCK, &content, &Codec, &u)
        .unwrap();
    assert_eq!(image(&writer, &u, &spec).events, [true, true]);
}

#[test]
fn abandoning_a_sealed_claim_stops_its_publication() {
    let (dir, spec, u) = (Dir::new(), Spec::standard(), unit(1));
    let id = admitted();
    let mut writer = open(&dir, &id);
    let content = Content::standard();
    acquired(writer.claim_review(&CLOCK, &u).unwrap());
    writer
        .seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())
        .unwrap();
    let grant = id.abandonment_grant("p", &u.claim_id()).unwrap();
    assert_eq!(
        writer
            .abandon_review_claim(&CLOCK, &u, grant)
            .unwrap()
            .state,
        S::Abandoned
    );
    assert_eq!(
        publication_code(writer.publish_sealed_review(&CLOCK, &content, &Codec, &u)),
        Code::ClaimNotUnfinished
    );
    let after = image(&writer, &u, &spec);
    assert_eq!((after.review, after.events), (None, vec![false, false]));
    assert!(after.seal.is_some(), "the seal stays as history");
    assert!(matches!(
        writer.claim_review(&CLOCK, &u).unwrap(),
        CoreReviewClaimV1::Abandoned(_)
    ));
}

#[test]
fn foreign_and_stale_tickets_are_refused_by_state() {
    let (dir_a, dir_b, spec, u) = (Dir::new(), Dir::new(), Spec::standard(), unit(1));
    let id = admitted();
    let content = Content::standard();
    let mut a = open(&dir_a, &id);
    acquired(a.claim_review(&CLOCK, &u).unwrap());
    let mut b = open(&dir_b, &id);
    acquired(b.claim_review(&CLOCK, &u).unwrap());
    let key = ("p".to_owned(), u.claim_id());
    // B holds A's ticket (another store) instead of its own.
    let foreign = a.review_tickets.remove(&key).unwrap();
    b.review_tickets.insert(key.clone(), foreign);
    let claimed = image(&b, &u, &spec);
    assert!(matches!(
        state(b.seal_review(&CLOCK, &content, &Codec, &u, spec.bytes())),
        StateError::WriteAuthorityRefused {
            reason: StateWriteRefusalV1::WrongStore
        }
    ));
    assert_eq!(image(&b, &u, &spec), claimed);
    drop((a, b));

    // A ticket from before a restart is stale for the reopened store.
    let (dir_c, u3) = (Dir::new(), unit(3));
    let mut before = open(&dir_c, &id);
    acquired(before.claim_review(&CLOCK, &u3).unwrap());
    let old = before
        .review_tickets
        .remove(&("p".to_owned(), u3.claim_id()))
        .unwrap();
    drop(before);
    let mut after = open(&dir_c, &id);
    after
        .review_tickets
        .insert(("p".to_owned(), u3.claim_id()), old);
    assert!(matches!(
        state(after.seal_review(&CLOCK, &content, &Codec, &u3, spec.bytes())),
        StateError::WriteAuthorityRefused {
            reason: StateWriteRefusalV1::WrongStore
        }
    ));
    assert!(
        after
            .reader()
            .find_review_seal("p", &u3.claim_id())
            .unwrap()
            .is_none()
    );
    let blocked = ReviewClaimSettlementV1::refused("POLICY_BLOCKED", None, None).unwrap();
    assert!(matches!(
        state(after.settle_review_refusal(&CLOCK, &u3, &blocked)),
        StateError::WriteAuthorityRefused {
            reason: StateWriteRefusalV1::WrongStore
        }
    ));
}
