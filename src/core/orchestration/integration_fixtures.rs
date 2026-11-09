//! Shared fixtures for the Core→State integration controls: real stores, the
//! real `CoreStateWriter`, a synthetic trusted clock and a synthetic recovery
//! codec. Nothing here writes to State except through the Core.

use receipts_state::{
    ActorKind, AvailabilityFactKindV1, AvailabilityFactSourceV1, AvailabilityFactV1,
    DecisionRecordKind, DecisionRecordV1, EventActor, EventPayloadReference, EventProposalOriginV1,
    EventProposalV1, EventSubject, EventType, PublicationEventV1, ReviewClaimRecordV1,
    ReviewClaimSettlementV1, ReviewSealRecordV1, StateEpochValueV1, StateError, SubjectKind,
    TrustedClockV1, TrustedTimeSampleV1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    CoreAdmissionIdentity, CoreContentResolverV1, CoreEventV1, CorePolicyRefusal,
    CorePublicationInputV1, CoreRecoveredReviewV1, CoreRecoveryCodecV1, CoreReviewUnitV1,
    CoreStateWriteError, CoreStateWriter,
};
use crate::test_support::{Dir, identity};

pub(super) const AUTHORITY: &str = "operator:alice";
pub(super) const REQUEST: &str = "artifact:r1";
pub(super) const RESULT: &str = "artifact:r2";
pub(super) const REQUEST_BYTES: &[u8] = b"review request one";
pub(super) const RESULT_BYTES: &[u8] = b"review result two";
pub(super) const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// The admitted identity of project `p`, with an admitted abandonment
/// authority.
pub(super) fn admitted() -> CoreAdmissionIdentity {
    identity("policy").with_abandonment_authority("p", AUTHORITY)
}

pub(super) fn open(dir: &Dir, identity: &CoreAdmissionIdentity) -> CoreStateWriter {
    CoreStateWriter::open(dir.db(), identity.clone()).unwrap()
}

pub(super) fn digest(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

/// A trusted clock at one fixed instant.
pub(super) struct At(pub(super) &'static str);
impl TrustedClockV1 for At {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: self.0.into(),
            clock_source_id: "synthetic-clock".into(),
            clock_contract_version: "1".into(),
        })
    }
}

/// Synthetic admitted content: exact bytes per `(project, reference)`.
pub(super) struct Content(pub(super) Vec<(&'static str, &'static str, &'static [u8])>);
impl Content {
    pub(super) fn standard() -> Self {
        Self(vec![
            ("p", REQUEST, REQUEST_BYTES),
            ("p", RESULT, RESULT_BYTES),
        ])
    }
}
impl CoreContentResolverV1 for Content {
    fn resolve(&self, project_id: &str, reference: &str) -> Option<Vec<u8>> {
        self.0
            .iter()
            .find(|(p, r, _)| *p == project_id && *r == reference)
            .map(|(_, _, bytes)| bytes.to_vec())
    }
}

pub(super) fn review_json(verdict: &str) -> String {
    format!(
        r#"{{"review_id":"rev-1","task_id":"t-1","reviewed_sha":"{SHA}","verdict":"{verdict}","independence_attested":true,"blocking_findings":[{}],"nonblocking_findings":[],"dimensions":[{{"dimension":"CORRECTNESS","assessment":"SATISFIED"}}]}}"#,
        if verdict == "REJECT" {
            r#"{"finding_id":"f-1","severity":"HIGH","category":"CORRECTNESS","description":"d","blocking":true}"#
        } else {
            ""
        }
    )
}

pub(super) fn review_record(project: &str, verdict: &str) -> DecisionRecordV1 {
    DecisionRecordV1::parse(
        project,
        DecisionRecordKind::A4Review,
        "rev-1",
        &review_json(verdict),
    )
    .unwrap()
}

pub(super) fn proposal(project: &str, reference: &str, payload_digest: &str) -> EventProposalV1 {
    EventProposalV1::new(
        EventProposalOriginV1::Review,
        project.into(),
        None,
        EventType::ReviewDispatched,
        EventActor {
            kind: ActorKind::System,
            id: None,
        },
        EventSubject {
            kind: SubjectKind::Task,
            id: "t-1".into(),
        },
        EventPayloadReference {
            reference: reference.into(),
            digest: payload_digest.into(),
        },
        "corr".into(),
        StateEpochValueV1::try_from(0i64).unwrap(),
        None,
    )
    .unwrap()
}

pub(super) fn fact(project: &str) -> AvailabilityFactV1 {
    AvailabilityFactV1::new(
        project.into(),
        "anthropic".into(),
        "claude".into(),
        String::new(),
        AvailabilityFactKindV1::Demotion,
        "PROVIDER_DOWN".into(),
        AvailabilityFactSourceV1::RuntimeFailureClass,
        "attempt:1".into(),
        None,
    )
    .unwrap()
}

/// A review outcome for the synthetic codec: the Review-shaped package
/// (record, keyed events, optional availability fact) and its disposition.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Spec {
    pub(super) project: String,
    /// `PASS`/`REJECT`, or empty for no record.
    pub(super) verdict: String,
    /// `(event key, reference, payload digest)`.
    pub(super) events: Vec<(String, String, String)>,
    pub(super) fact: bool,
    pub(super) refused: bool,
    pub(super) kind: String,
}

impl Spec {
    pub(super) fn standard() -> Self {
        Self {
            project: "p".into(),
            verdict: "PASS".into(),
            events: vec![
                ("k1".into(), REQUEST.into(), digest(REQUEST_BYTES)),
                ("k2".into(), RESULT.into(), digest(RESULT_BYTES)),
            ],
            fact: false,
            refused: false,
            kind: "REVIEWED".into(),
        }
    }

    pub(super) fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }

    pub(super) fn input(&self) -> CorePublicationInputV1 {
        CorePublicationInputV1 {
            project_id: self.project.clone(),
            records: if self.verdict.is_empty() {
                Vec::new()
            } else {
                vec![review_record(&self.project, &self.verdict)]
            },
            events: self
                .events
                .iter()
                .map(|(key, reference, d)| {
                    CoreEventV1::new(proposal(&self.project, reference, d), key.clone())
                })
                .collect(),
            availability: if self.fact {
                vec![fact(&self.project)]
            } else {
                Vec::new()
            },
        }
    }
}

/// Decodes [`Spec`] bytes. Ignores the project it is given, so the Core's own
/// project check is what refuses a foreign blob.
pub(super) struct Codec;
impl CoreRecoveryCodecV1 for Codec {
    fn kind(&self) -> &str {
        "REVIEW_SEALED_V1"
    }
    fn decode(&self, _project_id: &str, bytes: &[u8]) -> Result<CoreRecoveredReviewV1, String> {
        let spec: Spec = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        let settlement = if spec.refused {
            ReviewClaimSettlementV1::refused(&spec.kind, None, None)
        } else {
            ReviewClaimSettlementV1::completed(&spec.kind, Some("rev-1"), None)
        }
        .map_err(|e| e.to_string())?;
        Ok(CoreRecoveredReviewV1 {
            input: spec.input(),
            settlement,
        })
    }
}

pub(super) fn unit(generation: u32) -> CoreReviewUnitV1 {
    CoreReviewUnitV1::new("p", "t-1", Some(SHA), &"b".repeat(64), generation).unwrap()
}

/// The event ids a published `spec` must carry for `unit`'s publication.
pub(super) fn event_ids(unit: &CoreReviewUnitV1, spec: &Spec) -> Vec<String> {
    let publication = unit.publication_id();
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

/// Everything a review publication may touch, as the reader sees it.
#[derive(Debug, PartialEq)]
pub(super) struct Image {
    pub(super) watermark: String,
    pub(super) review: Option<String>,
    pub(super) events: Vec<bool>,
    pub(super) lifecycle: String,
    pub(super) claim: Option<ReviewClaimRecordV1>,
    pub(super) seal: Option<ReviewSealRecordV1>,
}

pub(super) fn image(writer: &CoreStateWriter, unit: &CoreReviewUnitV1, spec: &Spec) -> Image {
    let reader = writer.reader();
    Image {
        watermark: format!("{:?}", reader.find_trusted_time_watermark("p").unwrap()),
        review: reader
            .find_decision_record("p", DecisionRecordKind::A4Review, "rev-1")
            .unwrap()
            .map(|r| r.record_sha256().to_owned()),
        events: event_ids(unit, spec)
            .iter()
            .map(|id| reader.find_event(id).unwrap().is_some())
            .collect(),
        lifecycle: format!(
            "{:?}",
            reader
                .availability_lifecycle("p", "anthropic", "claude", "")
                .unwrap()
        ),
        claim: reader.find_review_claim("p", &unit.claim_id()).unwrap(),
        seal: reader.find_review_seal("p", &unit.claim_id()).unwrap(),
    }
}

pub(super) fn policy<T: std::fmt::Debug>(
    result: Result<T, CoreStateWriteError>,
) -> CorePolicyRefusal {
    match result {
        Err(CoreStateWriteError::Policy(refusal)) => refusal,
        other => panic!("expected a core policy refusal, got {other:?}"),
    }
}

pub(super) fn state<T: std::fmt::Debug>(result: Result<T, CoreStateWriteError>) -> StateError {
    match result {
        Err(CoreStateWriteError::State(error)) => error,
        other => panic!("expected a State refusal, got {other:?}"),
    }
}

pub(super) fn publication_code<T: std::fmt::Debug>(
    result: Result<T, CoreStateWriteError>,
) -> receipts_state::PublicationFailureCode {
    match state(result) {
        StateError::Publication { code, .. } => code,
        other => panic!("expected a publication refusal, got {other:?}"),
    }
}
