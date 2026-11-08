//! Durable routing records through the trusted core (MR-W2-1b/2/3).
//!
//! Routing never holds a State writer. It hands the trusted composition
//! validated, immutable values through [`RoutingStatePort`] — State decision
//! records, State event proposals (with the exact content their digest
//! covers) and D5 availability facts — and reads back through State's
//! read-only repository. The composition root implements the port over the
//! core's `CoreStateWriter` (HI-W4-1); it mints event ids and trusted time,
//! re-verifies the content digest and applies admission policy.
//!
//! Every event payload is one persisted State decision record: reference
//! `state-query:decision_record/<KIND>/<id>`, digest `sha256:` + the record's
//! State-computed SHA-256, content = the record's exact JSON bytes. Writes
//! are not one transaction: the record is appended before its events, so a
//! refused event leaves the (immutable, idempotently refused on retry) record.

use receipts_state::{
    ActorKind, AvailabilityFactReceiptV1, AvailabilityFactV1, DecisionRecordKind, DecisionRecordV1,
    EventActor, EventPayloadReference, EventPersistReceiptV1, EventProposalOriginV1,
    EventProposalV1, EventSubject, EventType, SqliteStateRepository, StateEpochValueV1, StateError,
    SubjectKind,
};

use crate::wire::{RoutingWireError, decode_routing_decision, encode_routing_decision};
use crate::{RoutingDecisionNonTemporalCore, RoutingDecisionOutcome};

/// The trusted composition's narrow write callbacks plus read-only State.
pub trait RoutingStatePort {
    type Error: std::fmt::Debug;
    /// Read-only State; its mutators are not reachable outside State.
    fn reader(&self) -> &SqliteStateRepository;
    fn append_decision_record(&mut self, record: &DecisionRecordV1) -> Result<(), Self::Error>;
    /// The core verifies `content` against the proposal digest, mints the
    /// event id and uses trusted time.
    fn persist_event_proposal(
        &mut self,
        proposal: &EventProposalV1,
        content: &[u8],
    ) -> Result<EventPersistReceiptV1, Self::Error>;
    fn append_availability_fact(
        &mut self,
        fact: &AvailabilityFactV1,
    ) -> Result<AvailabilityFactReceiptV1, Self::Error>;
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

/// One immutable Routing proposal whose payload is `record`.
pub fn routing_proposal(
    scope: &EventScope,
    event_type: EventType,
    subject: EventSubject,
    record: &DecisionRecordV1,
    correlation_id: &str,
) -> Result<EventProposalV1, StateError> {
    EventProposalV1::new(
        EventProposalOriginV1::Routing,
        scope.project_id.clone(),
        scope.goal_id.clone(),
        event_type,
        EventActor {
            kind: ActorKind::System,
            id: None,
        },
        subject,
        EventPayloadReference {
            reference: record_reference(record),
            digest: format!("sha256:{}", record.record_sha256()),
        },
        correlation_id.to_owned(),
        scope.epoch.clone(),
        None,
    )
}

/// Appends `record`, then persists each proposal with the record bytes.
pub fn persist_record_with_events<P: RoutingStatePort>(
    port: &mut P,
    record: &DecisionRecordV1,
    proposals: &[EventProposalV1],
) -> Result<Vec<EventPersistReceiptV1>, RoutingPersistError<P::Error>> {
    port.append_decision_record(record)
        .map_err(RoutingPersistError::Port)?;
    proposals
        .iter()
        .map(|p| {
            port.persist_event_proposal(p, record.record_json().as_bytes())
                .map_err(RoutingPersistError::Port)
        })
        .collect()
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

/// Persists one immutable decision and its event proposals.
pub fn persist_decision<P: RoutingStatePort>(
    port: &mut P,
    scope: &EventScope,
    decision: &RoutingDecisionNonTemporalCore,
) -> Result<Vec<EventPersistReceiptV1>, RoutingPersistError<P::Error>> {
    let task = decision
        .task_id()
        .ok_or(RoutingPersistError::TaskIdRequired)?;
    let record = DecisionRecordV1::parse(
        &scope.project_id,
        DecisionRecordKind::RoutingDecision,
        decision.decision_id(),
        &encode_routing_decision(decision),
    )?;
    let proposals = decision_event_types(decision.outcome())
        .iter()
        .map(|t| {
            routing_proposal(
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
    persist_record_with_events(port, &record, &proposals)
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
