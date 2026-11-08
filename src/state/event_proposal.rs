//! Immutable lower-trust event proposals and their core-only persistence.
//!
//! Any component may build an [`EventProposalV1`]; building one writes
//! nothing and grants nothing. Every one of the 53 frozen event types keeps
//! the trusted core as its only authoritative publisher
//! ([`crate::authoritative_event_publisher`]). The proposal's
//! [`EventProposalOriginV1`] is recorded causal provenance, not authority.
//! Persistence goes only through [`crate::TrustedCoreStore::persist_event_proposal`],
//! which appends the envelope and its receipt in one transaction.

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use crate::canonical_timestamp::CanonicalTimestampV1;
use crate::epoch_value::StateEpochValueV1;
use crate::error::StateError;
use crate::event::{
    EventActor, EventEnvelope, EventPayloadReference, EventSubject, EventType, insert_event,
    validate_for_append,
};
use crate::graph_repository as graph;
use crate::repository::SqliteStateRepository;
use crate::trusted_time::{self, TrustedClockV1};

/// Which component proposed the event (descriptive provenance only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventProposalOriginV1 {
    Orchestration,
    State,
    Routing,
    Runtime,
    Workspace,
    Review,
    Host,
}

impl EventProposalOriginV1 {
    pub const ALL: [Self; 7] = [
        Self::Orchestration,
        Self::State,
        Self::Routing,
        Self::Runtime,
        Self::Workspace,
        Self::Review,
        Self::Host,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Orchestration => "ORCHESTRATION",
            Self::State => "STATE",
            Self::Routing => "ROUTING",
            Self::Runtime => "RUNTIME",
            Self::Workspace => "WORKSPACE",
            Self::Review => "REVIEW",
            Self::Host => "HOST",
        }
    }
}

// Stand-ins used only to run the frozen envelope validation on a proposal
// before the core assigns the real identity and trusted time.
const PROBE_EVENT_ID: &str = "00000000000000000000000000";
const PROBE_OCCURRED_AT: &str = "1970-01-01T00:00:00.000000000Z";

/// An immutable, validated event proposal. Private fields; no setters.
#[derive(Debug, Clone, PartialEq)]
pub struct EventProposalV1 {
    origin: EventProposalOriginV1,
    project_id: String,
    goal_id: Option<String>,
    event_type: EventType,
    actor: EventActor,
    subject: EventSubject,
    payload: EventPayloadReference,
    correlation_id: String,
    epoch: StateEpochValueV1,
    claimed_observed_at: Option<String>,
}

impl EventProposalV1 {
    /// Validates the frozen envelope shape. `claimed_observed_at`, when
    /// present, must be a canonical timestamp; it stays a claim and never
    /// becomes the event's `occurred_at`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        origin: EventProposalOriginV1,
        project_id: String,
        goal_id: Option<String>,
        event_type: EventType,
        actor: EventActor,
        subject: EventSubject,
        payload: EventPayloadReference,
        correlation_id: String,
        epoch: StateEpochValueV1,
        claimed_observed_at: Option<String>,
    ) -> Result<Self, StateError> {
        let proposal = Self {
            origin,
            project_id,
            goal_id,
            event_type,
            actor,
            subject,
            payload,
            correlation_id,
            epoch,
            claimed_observed_at,
        };
        validate_for_append(&proposal.envelope(PROBE_EVENT_ID, PROBE_OCCURRED_AT))?;
        if let Some(claim) = &proposal.claimed_observed_at {
            CanonicalTimestampV1::parse(claim).map_err(|_| StateError::EventValidation {
                detail: "claimed_observed_at must be a canonical timestamp".into(),
            })?;
        }
        Ok(proposal)
    }

    pub fn origin(&self) -> EventProposalOriginV1 {
        self.origin
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn event_type(&self) -> EventType {
        self.event_type
    }
    pub fn claimed_observed_at(&self) -> Option<&str> {
        self.claimed_observed_at.as_deref()
    }
    /// The referenced content and its supplied digest (a claim until the
    /// trusted core verifies the actual bytes).
    pub fn payload(&self) -> &EventPayloadReference {
        &self.payload
    }

    /// Lowercase hex SHA-256 over a fixed-order JSON array of every field.
    pub fn sha256(&self) -> String {
        let fields = serde_json::json!([
            "receipts.event-proposal.v1",
            self.origin.as_str(),
            self.project_id,
            self.goal_id,
            self.event_type.as_str(),
            self.actor.kind.as_str(),
            self.actor.id,
            self.subject.kind.as_str(),
            self.subject.id,
            self.payload.reference,
            self.payload.digest,
            self.correlation_id,
            self.epoch.as_str(),
            self.claimed_observed_at,
        ]);
        let digest = Sha256::digest(fields.to_string().as_bytes());
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    fn envelope(&self, event_id: &str, occurred_at: &str) -> EventEnvelope {
        EventEnvelope {
            event_id: event_id.to_owned(),
            project_id: self.project_id.clone(),
            goal_id: self.goal_id.clone(),
            event_type: self.event_type,
            actor: self.actor.clone(),
            subject: self.subject.clone(),
            occurred_at: occurred_at.to_owned(),
            payload: self.payload.clone(),
            correlation_id: self.correlation_id.clone(),
            epoch: self.epoch.clone(),
        }
    }
}

/// Durable proof that one proposal became one event under this store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventPersistReceiptV1 {
    pub event_id: String,
    pub proposal_sha256: String,
    pub origin: EventProposalOriginV1,
    pub store_id: String,
    /// Trusted time from the clock contract, not the proposal's claim.
    pub occurred_at: String,
}

impl SqliteStateRepository {
    pub(crate) fn persist_event_proposal(
        &mut self,
        clock: &dyn TrustedClockV1,
        proposal: &EventProposalV1,
        event_id: &str,
        store_id: &str,
    ) -> Result<EventPersistReceiptV1, StateError> {
        let sample = clock
            .sample()
            .map_err(|e| graph::legacy_error(crate::GraphPhase::Clock, e))?;
        let (sample, timestamp) = trusted_time::validate_sample(sample)
            .map_err(|e| graph::legacy_error(crate::GraphPhase::Clock, e))?;
        let event = proposal.envelope(event_id, &sample.canonical_utc_timestamp);
        validate_for_append(&event)?;
        let receipt = EventPersistReceiptV1 {
            event_id: event_id.to_owned(),
            proposal_sha256: proposal.sha256(),
            origin: proposal.origin,
            store_id: store_id.to_owned(),
            occurred_at: sample.canonical_utc_timestamp.clone(),
        };
        graph::write_transaction(self.connection(), |conn| {
            if receipt_exists(conn, &proposal.project_id, &receipt.proposal_sha256)? {
                return Err(graph::failure(
                    crate::GraphPhase::Write,
                    crate::GraphFailureCode::AlreadyExists,
                    None,
                ));
            }
            graph::accept_graph_sample(conn, &proposal.project_id, &sample, &timestamp)?;
            insert_event(conn, &event)?;
            conn.execute(
                "INSERT INTO event_proposal_receipt(event_id,project_id,proposal_sha256,origin,claimed_observed_at,store_id,occurred_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    receipt.event_id,
                    proposal.project_id,
                    receipt.proposal_sha256,
                    receipt.origin.as_str(),
                    proposal.claimed_observed_at,
                    receipt.store_id,
                    receipt.occurred_at
                ],
            )
            .map_err(|e| graph::sqlite_error(crate::GraphPhase::Write, e))?;
            Ok(())
        })?;
        Ok(receipt)
    }

    /// Reads the receipt that persisted `event_id`, if any.
    pub fn find_event_receipt(
        &self,
        event_id: &str,
    ) -> Result<Option<EventPersistReceiptV1>, StateError> {
        use rusqlite::OptionalExtension;
        let row: Option<(String, String, String, String)> = self
            .connection()
            .query_row(
                "SELECT proposal_sha256,origin,store_id,occurred_at FROM event_proposal_receipt WHERE event_id=?1",
                [event_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(|e| graph::sqlite_error(crate::GraphPhase::Read, e))?;
        row.map(|(proposal_sha256, origin, store_id, occurred_at)| {
            let origin = EventProposalOriginV1::ALL
                .into_iter()
                .find(|o| o.as_str() == origin)
                .ok_or_else(graph::corrupt)?;
            Ok(EventPersistReceiptV1 {
                event_id: event_id.to_owned(),
                proposal_sha256,
                origin,
                store_id,
                occurred_at,
            })
        })
        .transpose()
    }
}

fn receipt_exists(conn: &Connection, project: &str, digest: &str) -> Result<bool, StateError> {
    conn.query_row(
        "SELECT COUNT(*) FROM event_proposal_receipt WHERE project_id=?1 AND proposal_sha256=?2",
        params![project, digest],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n != 0)
    .map_err(|e| graph::sqlite_error(crate::GraphPhase::Write, e))
}
