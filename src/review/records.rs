//! Builds the immutable record proposals Review hands to the trusted core.

use crate::ports::ReviewRecordProposal;
use receipts_state::{
    ActorKind, DecisionRecordKind, DecisionRecordV1, EventActor, EventPayloadReference,
    EventProposalOriginV1, EventProposalV1, EventSubject, EventType, StateEpochValueV1,
    SubjectKind,
};
use sha2::{Digest, Sha256};

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
}

impl ProposalBuilder {
    pub(crate) fn new(
        project_id: &str,
        task_id: &str,
        context_epoch: &str,
    ) -> Result<Self, String> {
        let epoch = StateEpochValueV1::try_from(context_epoch)
            .map_err(|e| format!("context epoch is not a State epoch: {e}"))?;
        Ok(Self {
            proposal: ReviewRecordProposal {
                project_id: project_id.to_owned(),
                decision_records: Vec::new(),
                events: Vec::new(),
                content: Vec::new(),
            },
            task_id: task_id.to_owned(),
            epoch,
        })
    }

    /// Registers exact bytes under `reference` and returns the payload.
    pub(crate) fn content(&mut self, reference: &str, bytes: &[u8]) -> EventPayloadReference {
        if !self.proposal.content.iter().any(|(r, _)| r == reference) {
            self.proposal
                .content
                .push((reference.to_owned(), bytes.to_vec()));
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
        Ok(())
    }

    pub(crate) fn event(
        &mut self,
        event_type: EventType,
        subject: (SubjectKind, &str),
        payload: EventPayloadReference,
    ) -> Result<(), String> {
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
        Ok(())
    }

    pub(crate) fn finish(self) -> ReviewRecordProposal {
        self.proposal
    }
}
