//! Durable routing records through the trusted core (MR-W2-1b/2/3).
//!
//! Routing never holds a State writer. It hands the trusted composition ONE
//! immutable [`Publication`] per outcome — State decision records, the State
//! event proposals that cite them (with the exact content their digest covers)
//! and, for availability, the D5 demotion fact — through
//! [`RoutingStatePort::publish`], and reads back through State's read-only
//! repository. The composition root implements the port over the core's
//! `CoreStateWriter` (HI-W4-1); it resolves and re-verifies each event's
//! content, mints event ids and trusted time, and applies admission policy.
//!
//! # Atomic consumer contract
//!
//! `publish` is all-or-nothing: on `Err` nothing of the publication is
//! visible to any reader (live or after a restart); on `Ok` every record,
//! event and fact is. It is also idempotent for an exact retry — the same
//! records, proposals and fact return the ORIGINAL receipts with
//! `replayed = true` and write nothing — and it refuses a conflicting retry
//! (a stored record id, event proposal or fact slot reused with different
//! content) without writing anything. Routing therefore advances its own
//! memory only after `Ok`, rebuilds from State on restart, and a lost
//! acknowledgement is recovered by retrying the identical call.
//!
//! The shared State/Core transaction that implements this over the real
//! `CoreStateWriter` is supplied by State; until it is admitted, only the
//! in-tree test composition ([`crate::test_state`]) implements the contract.
//!
//! Every event payload is the exact content its digest names: a decision
//! record (`state-query:decision_record/<KIND>/<id>`, `sha256:` + the record's
//! State-computed SHA-256, content = the record's JSON bytes) or, for
//! ROUTING_REQUESTED, the request's canonical wire JSON
//! (`routing-request:<request_id>`).

use receipts_state::{
    ActorKind, AvailabilityFactReceiptV1, AvailabilityFactV1, DecisionRecordKind, DecisionRecordV1,
    EventActor, EventPayloadReference, EventPersistReceiptV1, EventProposalOriginV1,
    EventProposalV1, EventSubject, EventType, SqliteStateRepository, StateEpochValueV1, StateError,
    SubjectKind,
};

use crate::wire::{
    RoutingWireError, decode_routing_decision, encode_routing_decision, encode_routing_request,
};
use crate::{RoutingDecisionNonTemporalCore, RoutingDecisionOutcome, RoutingRequest};

/// One event to persist: the proposal and the exact bytes its payload digest
/// covers.
#[derive(Debug, Clone)]
pub struct PublishedEvent {
    pub proposal: EventProposalV1,
    pub content: Vec<u8>,
}

/// Everything one routing outcome makes durable, as an indivisible unit.
#[derive(Debug, Clone, Default)]
pub struct Publication {
    pub records: Vec<DecisionRecordV1>,
    pub events: Vec<PublishedEvent>,
    /// The D5 availability fact, when the outcome demotes a candidate.
    pub fact: Option<AvailabilityFactV1>,
}

/// What a successful publication produced (or, on an exact retry, produced
/// the first time).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicationReceipt {
    /// One receipt per [`Publication::events`] entry, in order.
    pub events: Vec<EventPersistReceiptV1>,
    pub fact: Option<AvailabilityFactReceiptV1>,
    /// This call was an exact retry of an earlier success.
    pub replayed: bool,
}

/// The trusted composition's narrow write callback plus read-only State.
pub trait RoutingStatePort {
    type Error: std::fmt::Debug;
    /// Read-only State; its mutators are not reachable outside State.
    fn reader(&self) -> &SqliteStateRepository;
    /// Persists `publication` under the atomic contract in the module docs.
    fn publish(&mut self, publication: &Publication) -> Result<PublicationReceipt, Self::Error>;
}

/// Where routing events belong.
#[derive(Debug, Clone)]
pub struct EventScope {
    pub project_id: String,
    pub goal_id: Option<String>,
    pub epoch: StateEpochValueV1,
}

#[derive(Debug)]
pub enum RoutingPersistError<E> {
    /// State refused to build the record or proposal (shape/identity).
    State(StateError),
    /// Persisted decision events are about a task; the decision had none.
    TaskIdRequired,
    /// A stored record did not decode.
    Decode(RoutingWireError),
    /// An event's content does not match its proposal's payload digest, so
    /// nothing was sent to the port.
    ContentMismatch,
    /// The port reported success with a receipt that does not match the
    /// publication (event count or fact); the outcome is not trusted.
    ReceiptShape,
    Port(E),
}

impl<E> From<StateError> for RoutingPersistError<E> {
    fn from(e: StateError) -> Self {
        Self::State(e)
    }
}

/// Reference used for an event whose payload is a decision record.
pub fn record_reference(record: &DecisionRecordV1) -> String {
    format!(
        "state-query:decision_record/{}/{}",
        record.kind().as_str(),
        record.record_id()
    )
}

/// Reference used for ROUTING_REQUESTED, whose payload is the request body.
pub fn request_reference(request_id: &str) -> String {
    format!("routing-request:{request_id}")
}

fn system_actor() -> EventActor {
    EventActor {
        kind: ActorKind::System,
        id: None,
    }
}

/// One immutable Routing event whose payload is `record`, with its content.
pub fn record_event(
    scope: &EventScope,
    event_type: EventType,
    subject: EventSubject,
    record: &DecisionRecordV1,
    correlation_id: &str,
) -> Result<PublishedEvent, StateError> {
    Ok(PublishedEvent {
        proposal: EventProposalV1::new(
            EventProposalOriginV1::Routing,
            scope.project_id.clone(),
            scope.goal_id.clone(),
            event_type,
            system_actor(),
            subject,
            EventPayloadReference {
                reference: record_reference(record),
                digest: format!("sha256:{}", record.record_sha256()),
            },
            correlation_id.to_owned(),
            scope.epoch.clone(),
            None,
        )?,
        content: record.record_json().as_bytes().to_vec(),
    })
}

/// Sends `publication` to the port once and checks the receipt it returns.
pub fn publish<P: RoutingStatePort>(
    port: &mut P,
    publication: &Publication,
) -> Result<PublicationReceipt, RoutingPersistError<P::Error>> {
    if publication
        .events
        .iter()
        .any(|e| e.proposal.payload().digest != format!("sha256:{}", sha256_hex(&e.content)))
    {
        return Err(RoutingPersistError::ContentMismatch);
    }
    let receipt = port
        .publish(publication)
        .map_err(RoutingPersistError::Port)?;
    if receipt.events.len() != publication.events.len()
        || receipt.fact.is_some() != publication.fact.is_some()
    {
        return Err(RoutingPersistError::ReceiptShape);
    }
    Ok(receipt)
}

/// The events a decision proposes: SELECTED → ROUTING_DECIDED and
/// EXECUTOR_SELECTED; NO_ELIGIBLE_CANDIDATE → ROUTING_FAILED_NO_CANDIDATE;
/// USER_INPUT_REQUIRED/BLOCKED → ROUTING_DECIDED.
pub fn decision_event_types(outcome: RoutingDecisionOutcome) -> &'static [EventType] {
    match outcome {
        RoutingDecisionOutcome::Selected => {
            &[EventType::RoutingDecided, EventType::ExecutorSelected]
        }
        RoutingDecisionOutcome::NoEligibleCandidate => &[EventType::RoutingFailedNoCandidate],
        RoutingDecisionOutcome::UserInputRequired | RoutingDecisionOutcome::Blocked => {
            &[EventType::RoutingDecided]
        }
    }
}

/// Persists one immutable decision and its events as one publication. An
/// exact retry returns the original receipts; a different decision under the
/// same id is refused by the port.
pub fn persist_decision<P: RoutingStatePort>(
    port: &mut P,
    scope: &EventScope,
    decision: &RoutingDecisionNonTemporalCore,
) -> Result<PublicationReceipt, RoutingPersistError<P::Error>> {
    let task = decision
        .task_id()
        .ok_or(RoutingPersistError::TaskIdRequired)?;
    let record = DecisionRecordV1::parse(
        &scope.project_id,
        DecisionRecordKind::RoutingDecision,
        decision.decision_id(),
        &encode_routing_decision(decision),
    )?;
    let events = decision_event_types(decision.outcome())
        .iter()
        .map(|t| {
            record_event(
                scope,
                *t,
                EventSubject {
                    kind: SubjectKind::Task,
                    id: task.to_owned(),
                },
                &record,
                decision.request_id(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    publish(
        port,
        &Publication {
            records: vec![record],
            events,
            fact: None,
        },
    )
}

/// Persists ROUTING_REQUESTED for `request`: the event's payload is the
/// request's canonical wire JSON under [`request_reference`], digest
/// `sha256:` + its SHA-256. The composition's content resolver must resolve
/// that reference to exactly those bytes (the request Routing was handed),
/// so the core can verify the digest before persisting. No State table is
/// involved and Routing holds no writer. An exact retry returns the original
/// receipt; ROUTING_REQUESTED is published once per request, before its
/// first decision.
pub fn persist_request<P: RoutingStatePort>(
    port: &mut P,
    scope: &EventScope,
    request: &RoutingRequest,
) -> Result<PublicationReceipt, RoutingPersistError<P::Error>> {
    let core = request.core();
    let task = core.task_id().ok_or(RoutingPersistError::TaskIdRequired)?;
    let content = encode_routing_request(request).into_bytes();
    let proposal = EventProposalV1::new(
        EventProposalOriginV1::Routing,
        scope.project_id.clone(),
        scope.goal_id.clone(),
        EventType::RoutingRequested,
        system_actor(),
        EventSubject {
            kind: SubjectKind::Task,
            id: task.to_owned(),
        },
        EventPayloadReference {
            reference: request_reference(core.request_id()),
            digest: format!("sha256:{}", sha256_hex(&content)),
        },
        core.request_id().to_owned(),
        scope.epoch.clone(),
        None,
    )?;
    publish(
        port,
        &Publication {
            records: Vec::new(),
            events: vec![PublishedEvent { proposal, content }],
            fact: None,
        },
    )
}

/// Reads one persisted decision back (P-05 explain): the full immutable
/// record with every alternative, reason, floor, freshness and evidence.
pub fn load_decision(
    reader: &SqliteStateRepository,
    project_id: &str,
    decision_id: &str,
) -> Result<Option<RoutingDecisionNonTemporalCore>, RoutingPersistError<std::convert::Infallible>> {
    reader
        .find_decision_record(project_id, DecisionRecordKind::RoutingDecision, decision_id)?
        .map(|r| decode_routing_decision(r.record_json()).map_err(RoutingPersistError::Decode))
        .transpose()
}

/// Number of persisted `fallback_from` links behind `decision_id`
/// (0 for an original decision). Refuses a cycle or a missing link.
pub fn fallback_chain_length(
    reader: &SqliteStateRepository,
    project_id: &str,
    decision_id: &str,
) -> Result<u32, RoutingPersistError<std::convert::Infallible>> {
    let mut length = 0;
    let mut current = decision_id.to_owned();
    loop {
        let decision = load_decision(reader, project_id, &current)?.ok_or(
            RoutingPersistError::Decode(RoutingWireError::Refused("fallback_from")),
        )?;
        match decision.fallback_from() {
            None => return Ok(length),
            Some(previous) => {
                length += 1;
                if length > 10_000 {
                    // ponytail: a chain this long is a cycle or corruption.
                    return Err(RoutingPersistError::Decode(RoutingWireError::Refused(
                        "fallback_from",
                    )));
                }
                current = previous.to_owned();
            }
        }
    }
}

/// Lowercase hex SHA-256 (FIPS 180-4). State offers no public digest helper
/// for arbitrary bytes and Routing may not add a dependency; checked against
/// the standard vectors and State's own record digests in the tests.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64).wrapping_mul(8).to_be_bytes());
    for block in message.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (slot, v) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *slot = slot.wrapping_add(v);
        }
    }
    h.iter().map(|v| format!("{v:08x}")).collect()
}
