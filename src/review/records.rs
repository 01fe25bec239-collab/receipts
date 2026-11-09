//! Builds the immutable record proposals Review hands to the trusted core.

use crate::ports::ReviewRecordProposal;
use receipts_state::{
    ActorKind, DecisionRecordKind, DecisionRecordV1, EventActor, EventPayloadReference,
    EventProposalOriginV1, EventProposalV1, EventSubject, EventType, StateEpochValueV1,
    SubjectKind,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Plain-data form of one package: everything needed to rebuild the typed
/// State values through the same validators, so a sink can journal and later
/// replay a package byte-for-byte (see `ReviewRecordProposal::to_journal`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub(crate) struct PackageSpec {
    pub(crate) claim_key: String,
    pub(crate) project_id: String,
    pub(crate) task_id: String,
    pub(crate) context_epoch: String,
    pub(crate) decisions: Vec<DecisionSpec>,
    pub(crate) events: Vec<EventSpec>,
    pub(crate) content: Vec<ContentSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DecisionSpec {
    pub(crate) kind: String,
    pub(crate) id: String,
    pub(crate) json: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EventSpec {
    pub(crate) key: String,
    pub(crate) event_type: String,
    pub(crate) subject_kind: String,
    pub(crate) subject_id: String,
    pub(crate) payload_ref: String,
    pub(crate) payload_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ContentSpec {
    pub(crate) reference: String,
    pub(crate) hex: String,
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err("journal content is not hexadecimal".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|e| e.to_string()))
        .collect()
}

/// Event types and decision kinds this package family can produce.
const EVENT_TYPES: [EventType; 11] = [
    EventType::ReviewDispatched,
    EventType::ReviewPassed,
    EventType::ReviewRejected,
    EventType::FindingRaised,
    EventType::FindingDispositioned,
    EventType::PolicyBlocked,
    EventType::HumanRequired,
    EventType::RepairIssued,
    EventType::RepairLimitReached,
    EventType::EscalatedToA2,
    EventType::AcceptanceEvaluated,
];
const SUBJECT_KINDS: [SubjectKind; 2] = [SubjectKind::Task, SubjectKind::Review];
const DECISION_KINDS: [DecisionRecordKind; 2] = [
    DecisionRecordKind::A4Review,
    DecisionRecordKind::IntegrationDecision,
];

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Deterministic identifier: `<prefix>-<32 hex>` from the given parts, so a
/// replay produces the same identity (State refuses the duplicate).
pub(crate) fn derived_id(prefix: &str, parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update((p.len() as u64).to_be_bytes());
        h.update(p.as_bytes());
    }
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    format!("{prefix}-{}", &hex[..32])
}

/// Accumulates one proposal; every event cites registered content.
pub(crate) struct ProposalBuilder {
    proposal: ReviewRecordProposal,
    task_id: String,
    epoch: StateEpochValueV1,
    spec: PackageSpec,
}

impl ProposalBuilder {
    /// `claim_key` is the durable identity of this package: every event key
    /// derives from it, so a replayed publication names the same events.
    pub(crate) fn new(
        project_id: &str,
        task_id: &str,
        context_epoch: &str,
        claim_key: &str,
    ) -> Result<Self, String> {
        let epoch = StateEpochValueV1::try_from(context_epoch)
            .map_err(|e| format!("context epoch is not a State epoch: {e}"))?;
        Ok(Self {
            proposal: ReviewRecordProposal {
                project_id: project_id.to_owned(),
                decision_records: Vec::new(),
                events: Vec::new(),
                event_keys: Vec::new(),
                content: Vec::new(),
                spec: PackageSpec::default(),
            },
            task_id: task_id.to_owned(),
            epoch,
            spec: PackageSpec {
                claim_key: claim_key.to_owned(),
                project_id: project_id.to_owned(),
                task_id: task_id.to_owned(),
                context_epoch: context_epoch.to_owned(),
                ..PackageSpec::default()
            },
        })
    }

    /// Rebuilds a package from its journal, through the same validators.
    pub(crate) fn replay(spec: &PackageSpec) -> Result<ReviewRecordProposal, String> {
        let mut b = Self::new(
            &spec.project_id,
            &spec.task_id,
            &spec.context_epoch,
            &spec.claim_key,
        )?;
        for d in &spec.decisions {
            let kind = DECISION_KINDS
                .into_iter()
                .find(|k| format!("{k:?}") == d.kind)
                .ok_or_else(|| format!("journal names unknown decision kind {}", d.kind))?;
            b.decision(kind, &d.id, &d.json)?;
        }
        for c in &spec.content {
            b.content(&c.reference, &hex_decode(&c.hex)?);
        }
        for e in &spec.events {
            // Each event must still name exactly the bytes journaled for it.
            let bound = spec.content.iter().any(|c| {
                c.reference == e.payload_ref
                    && hex_decode(&c.hex).is_ok_and(|bytes| {
                        e.payload_digest == format!("sha256:{}", sha256_hex(&bytes))
                    })
            });
            if !bound {
                return Err(format!(
                    "journal event {} does not match its payload content",
                    e.payload_ref
                ));
            }
            let event_type = EVENT_TYPES
                .into_iter()
                .find(|t| t.as_str() == e.event_type)
                .ok_or_else(|| format!("journal names unknown event type {}", e.event_type))?;
            let subject = SUBJECT_KINDS
                .into_iter()
                .find(|k| k.as_str() == e.subject_kind)
                .ok_or_else(|| format!("journal names unknown subject {}", e.subject_kind))?;
            b.event(
                event_type,
                (subject, &e.subject_id),
                EventPayloadReference {
                    reference: e.payload_ref.clone(),
                    digest: e.payload_digest.clone(),
                },
            )?;
        }
        let rebuilt = b.finish();
        if rebuilt.spec != *spec {
            return Err("journal does not rebuild to the sealed package".into());
        }
        Ok(rebuilt)
    }

    /// Registers exact bytes under `reference` and returns the payload.
    pub(crate) fn content(&mut self, reference: &str, bytes: &[u8]) -> EventPayloadReference {
        if !self.proposal.content.iter().any(|(r, _)| r == reference) {
            self.proposal
                .content
                .push((reference.to_owned(), bytes.to_vec()));
            self.spec.content.push(ContentSpec {
                reference: reference.to_owned(),
                hex: hex_encode(bytes),
            });
        }
        EventPayloadReference {
            reference: reference.to_owned(),
            digest: format!("sha256:{}", sha256_hex(bytes)),
        }
    }

    pub(crate) fn decision(
        &mut self,
        kind: DecisionRecordKind,
        id: &str,
        json: &str,
    ) -> Result<(), String> {
        let record = DecisionRecordV1::parse(&self.proposal.project_id, kind, id, json)
            .map_err(|e| format!("State refused the {kind:?} record shape: {e}"))?;
        self.proposal.decision_records.push(record);
        self.spec.decisions.push(DecisionSpec {
            kind: format!("{kind:?}"),
            id: id.to_owned(),
            json: json.to_owned(),
        });
        Ok(())
    }

    pub(crate) fn event(
        &mut self,
        event_type: EventType,
        subject: (SubjectKind, &str),
        payload: EventPayloadReference,
    ) -> Result<(), String> {
        let key = derived_id(
            "revent",
            &[
                &self.spec.claim_key,
                &self.proposal.events.len().to_string(),
                event_type.as_str(),
            ],
        );
        let (subject_kind, subject_id) = (subject.0, subject.1.to_owned());
        let spec = EventSpec {
            key: key.clone(),
            event_type: event_type.as_str().to_owned(),
            subject_kind: subject_kind.as_str().to_owned(),
            subject_id,
            payload_ref: payload.reference.clone(),
            payload_digest: payload.digest.clone(),
        };
        let proposal = EventProposalV1::new(
            EventProposalOriginV1::Review,
            self.proposal.project_id.clone(),
            None,
            event_type,
            EventActor {
                kind: ActorKind::System,
                id: None,
            },
            EventSubject {
                kind: subject.0,
                id: subject.1.to_owned(),
            },
            payload,
            self.task_id.clone(),
            self.epoch.clone(),
            None,
        )
        .map_err(|e| format!("State refused the {event_type:?} proposal: {e}"))?;
        self.proposal.events.push(proposal);
        self.proposal.event_keys.push(key);
        self.spec.events.push(spec);
        Ok(())
    }

    pub(crate) fn finish(mut self) -> ReviewRecordProposal {
        self.proposal.spec = self.spec;
        self.proposal
    }
}
