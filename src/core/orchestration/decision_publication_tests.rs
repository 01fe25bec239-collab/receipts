//! Typed decision/event/availability publication through the real
//! Core→State path: Core resolves and hashes every payload, derives stable
//! identities and publishes one atomic package; any refusal leaves nothing.

use receipts_state::{
    AvailabilityLifecycleStatusV1, DecisionRecordKind, GraphFailureCode, PublicationEventV1,
    PublicationFailureCode as Code, StateError,
};

use super::integration_fixtures::{
    Content, REQUEST, RESULT, Spec, admitted, digest, open, policy, proposal, publication_code,
    review_record, state,
};
use super::{CoreEventV1, CorePolicyRefusal, CorePublicationInputV1, publication_id_v1};
use crate::test_support::{CLOCK, Dir};

/// What the package may touch, as the reader sees it.
#[derive(Debug, PartialEq)]
struct Image {
    watermark: String,
    review: Option<String>,
    events: Vec<bool>,
    lifecycle: String,
}

fn event_ids(key: &str, spec: &Spec) -> Vec<String> {
    let publication = publication_id_v1("p", key);
    spec.input()
        .events
        .iter()
        .map(|e| {
            PublicationEventV1::stable(e.proposal().clone(), &publication, e.event_key())
                .unwrap()
                .event_id()
                .to_owned()
        })
        .collect()
}

fn image(writer: &super::CoreStateWriter, key: &str, spec: &Spec) -> Image {
    let reader = writer.reader();
    Image {
        watermark: format!("{:?}", reader.find_trusted_time_watermark("p").unwrap()),
        review: reader
            .find_decision_record("p", DecisionRecordKind::A4Review, "rev-1")
            .unwrap()
            .map(|r| r.record_sha256().to_owned()),
        events: event_ids(key, spec)
            .iter()
            .map(|id| reader.find_event(id).unwrap().is_some())
            .collect(),
        lifecycle: format!(
            "{:?}",
            reader
                .availability_lifecycle("p", "anthropic", "claude", "")
                .unwrap()
        ),
    }
}

fn spec_with_fact() -> Spec {
    let mut spec = Spec::standard();
    spec.fact = true;
    spec
}

#[test]
fn one_package_publishes_records_events_and_availability_atomically_and_replays_exactly() {
    let (dir, spec) = (Dir::new(), spec_with_fact());
    let id = admitted();
    let mut writer = open(&dir, &id);
    let content = Content::standard();
    let before = image(&writer, "k", &spec);
    assert_eq!(before.review, None);

    let receipt = writer
        .publish_decision_package(&CLOCK, &content, "k", &spec.input())
        .unwrap();
    assert!(!receipt.replayed);
    assert_eq!(receipt.publication_id, publication_id_v1("p", "k"));
    assert_eq!(receipt.claim, None);
    // Event ids are the stable ones derived from the publication and key.
    let ids: Vec<_> = receipt.events.iter().map(|e| e.event_id.clone()).collect();
    assert_eq!(ids, event_ids("k", &spec));
    assert_eq!(receipt.availability.len(), 1);
    let done = image(&writer, "k", &spec);
    assert!(done.review.is_some());
    assert_eq!(done.events, [true, true]);
    assert_eq!(
        writer
            .reader()
            .availability_lifecycle("p", "anthropic", "claude", "")
            .unwrap()
            .status,
        AvailabilityLifecycleStatusV1::Demoted
    );

    // The exact retry, also after a reopen, writes nothing and returns the
    // recorded receipt.
    let replay = writer
        .publish_decision_package(&CLOCK, &content, "k", &spec.input())
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(
        (&replay.events, &replay.availability, &replay.recorded_at),
        (&receipt.events, &receipt.availability, &receipt.recorded_at)
    );
    drop(writer);
    let mut writer = open(&dir, &id);
    let again = writer
        .publish_decision_package(&CLOCK, &content, "k", &spec.input())
        .unwrap();
    assert!(again.replayed);
    assert_eq!(again.events, receipt.events);
    assert_eq!(image(&writer, "k", &spec), done);
}

#[test]
fn missing_or_wrong_event_content_leaves_no_partial_publication() {
    let (dir, spec) = (Dir::new(), spec_with_fact());
    let mut writer = open(&dir, &admitted());
    let content = Content::standard();
    let before = image(&writer, "k", &spec);
    let with = |edit: &dyn Fn(&mut Spec)| {
        let mut s = spec.clone();
        edit(&mut s);
        s
    };
    // The FIRST event is fine in every case, the second is not: a record and a
    // good event must not survive a bad sibling.
    let cases = [
        (
            with(&|s| s.events[1].1 = "artifact:never-created".into()),
            CorePolicyRefusal::ContentUnavailable,
        ),
        (
            with(&|s| s.events[1].2 = digest(b"other bytes")),
            CorePolicyRefusal::ContentDigestMismatch,
        ),
        (
            with(&|s| s.events[1].2 = "md5:abc".into()),
            CorePolicyRefusal::UnsupportedDigest,
        ),
    ];
    for (bad, expected) in cases {
        assert_eq!(
            policy(writer.publish_decision_package(&CLOCK, &content, "k", &bad.input())),
            expected
        );
        assert_eq!(image(&writer, "k", &spec), before, "{expected:?}");
    }
    // Content that exists only under another project is not resolvable.
    let foreign = Content(vec![
        ("p", REQUEST, super::integration_fixtures::REQUEST_BYTES),
        ("r", RESULT, super::integration_fixtures::RESULT_BYTES),
    ]);
    assert_eq!(
        policy(writer.publish_decision_package(&CLOCK, &foreign, "k", &spec.input())),
        CorePolicyRefusal::ContentUnavailable
    );
    assert_eq!(image(&writer, "k", &spec), before);
    // With the real content the same package publishes.
    writer
        .publish_decision_package(&CLOCK, &content, "k", &spec.input())
        .unwrap();
    assert_eq!(image(&writer, "k", &spec).events, [true, true]);
}

#[test]
fn conflicting_retries_and_reused_identities_refuse_with_nothing_written() {
    let (dir, spec) = (Dir::new(), Spec::standard());
    let mut writer = open(&dir, &admitted());
    let content = Content::standard();
    let mut reject = spec.clone();
    reject.verdict = "REJECT".into();
    writer
        .publish_decision_package(&CLOCK, &content, "k", &reject.input())
        .unwrap();
    let published = image(&writer, "k", &spec);

    // The same publication key with different content.
    assert_eq!(
        publication_code(writer.publish_decision_package(&CLOCK, &content, "k", &spec.input())),
        Code::ConflictingRetry
    );
    // A stored REJECT is never replaced by a later PASS, and its sibling
    // events under the new publication do not leak.
    assert!(matches!(
        state(writer.publish_decision_package(&CLOCK, &content, "k2", &spec.input())),
        StateError::GraphPersistence {
            code: GraphFailureCode::AlreadyExists,
            ..
        }
    ));
    assert_eq!(image(&writer, "k", &spec), published);
    assert_eq!(image(&writer, "k2", &spec).events, [false, false]);
}

#[test]
fn project_shape_and_key_errors_are_refused_before_anything_is_written() {
    let (dir, spec) = (Dir::new(), Spec::standard());
    let mut writer = open(&dir, &admitted());
    let content = Content::standard();
    let before = image(&writer, "k", &spec);
    // A project the composition did not admit.
    let mut other = spec.clone();
    other.project = "q".into();
    assert_eq!(
        policy(writer.publish_decision_package(&CLOCK, &content, "k", &other.input())),
        CorePolicyRefusal::ProjectNotAdmitted
    );
    // An item of another project inside an admitted project's package.
    let mut mixed = spec.input();
    mixed.events.push(CoreEventV1::new(
        proposal(
            "q",
            REQUEST,
            &digest(super::integration_fixtures::REQUEST_BYTES),
        ),
        "k3",
    ));
    assert_eq!(
        policy(writer.publish_decision_package(&CLOCK, &content, "k", &mixed)),
        CorePolicyRefusal::InvalidShape
    );
    let mut record = spec.input();
    record.records = vec![review_record("q", "PASS")];
    assert_eq!(
        publication_code(writer.publish_decision_package(&CLOCK, &content, "k", &record)),
        Code::InvalidShape
    );
    // An empty package, a repeated event key and bad publication keys.
    let empty = CorePublicationInputV1 {
        project_id: "p".into(),
        records: Vec::new(),
        events: Vec::new(),
        availability: vec![],
    };
    assert_eq!(
        publication_code(writer.publish_decision_package(&CLOCK, &content, "k", &empty)),
        Code::InvalidShape
    );
    let mut repeated = spec.input();
    repeated.events[1] = CoreEventV1::new(repeated.events[1].proposal().clone(), "k1");
    assert_eq!(
        publication_code(writer.publish_decision_package(&CLOCK, &content, "k", &repeated)),
        Code::InvalidShape
    );
    for key in ["", "bad\u{7}key", &"x".repeat(201)] {
        assert_eq!(
            policy(writer.publish_decision_package(&CLOCK, &content, key, &spec.input())),
            CorePolicyRefusal::InvalidShape
        );
    }
    assert_eq!(image(&writer, "k", &spec), before);
}
