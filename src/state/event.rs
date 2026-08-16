//! Append-only EventEnvelope persistence under STRICT_W1_EVENT_BOUNDARY
//! (migration 0004 slice).
//!
//! [`EventEnvelope`] is the frozen `Event` contract of the event model: the
//! durable audit evidence from which orchestration decisions are
//! reconstructible after the fact. This slice implements the W1 persistence
//! foundation only: State persists already-constructed structural event
//! evidence; it never decides when events should exist, never produces them,
//! and never interprets their contents.
//!
//! STRICT_W1_EVENT_BOUNDARY (BUILD-A1/A0 repair decision) is honored by
//! construction:
//!
//! * the payload is represented **only** as an opaque structural
//!   [`EventPayloadReference`] (reference + digest). There is no public
//!   field, parameter, or API accepting arbitrary raw event body text, and
//!   the storage schema has no raw payload column. Large/output-rich
//!   material belongs behind a reference + digest, owned elsewhere;
//! * there is no caller-provided secret list, `EventRedaction`, or any
//!   redaction-set completeness parameter: security here is a strict
//!   data-shape boundary, not secret classification;
//! * there is no regex/pattern/entropy secret detection of any kind;
//! * structural identity fields — including `event_id` — are never
//!   redacted, rewritten, masked, substituted, truncated, hashed, or
//!   regenerated. Validation either accepts a supplied value byte-for-byte
//!   unchanged or rejects the event before persistence. This structurally
//!   prevents the A4-006-F2 failure (redaction mutating a validated
//!   `event_id`) and the A4-006-F1 failure (raw payload text persisting
//!   behind an incomplete caller secret list).
//!
//! Events are immutable durable evidence: this module offers append and
//! read-by-`event_id` only. There is no update, delete, replace, upsert,
//! merge, rewrite, truncate, or clear capability, and a duplicate
//! `event_id` fails explicitly with the original row untouched. `event_id`
//! is a canonical ULID (exactly 26 Crockford Base32 characters) validated
//! structurally, without a new dependency. `occurred_at` is an opaque
//! contract timestamp string stored as provided: State never reads the wall
//! clock, parses, or compares it.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::StateError;
use crate::repository::{SqliteStateRepository, UnitOfWork};

/// The exact length, in characters, of a canonical ULID.
pub const EVENT_ID_ULID_LENGTH: usize = 26;

/// The closed frozen `EventType` set: all 53 values, in contract order.
///
/// There is no `UNKNOWN`, `OTHER`, `CUSTOM`, fallback, or provider
/// extension variant: any value outside the frozen set fails closed at the
/// decode boundary and is rejected by the storage backstop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    /// `GOAL_CREATED`.
    GoalCreated,
    /// `GOAL_DECOMPOSED`.
    GoalDecomposed,
    /// `GOAL_EVALUATED`.
    GoalEvaluated,
    /// `GOAL_COMPLETED`.
    GoalCompleted,
    /// `GOAL_BLOCKED`.
    GoalBlocked,
    /// `ROLE_CREATED`.
    RoleCreated,
    /// `EXECUTOR_SELECTED`.
    ExecutorSelected,
    /// `EXECUTOR_BOUND`.
    ExecutorBound,
    /// `EXECUTOR_RELEASED`.
    ExecutorReleased,
    /// `EXECUTOR_REPLACED`.
    ExecutorReplaced,
    /// `ROUTING_REQUESTED`.
    RoutingRequested,
    /// `ROUTING_DECIDED`.
    RoutingDecided,
    /// `ROUTING_FAILED_NO_CANDIDATE`.
    RoutingFailedNoCandidate,
    /// `USER_ROUTING_INPUT`.
    UserRoutingInput,
    /// `TASK_CREATED`.
    TaskCreated,
    /// `TASK_READY`.
    TaskReady,
    /// `TASK_DISPATCHED`.
    TaskDispatched,
    /// `TASK_STARTED`.
    TaskStarted,
    /// `TASK_COMPLETED`.
    TaskCompleted,
    /// `TASK_FAILED`.
    TaskFailed,
    /// `TASK_CANCELLED`.
    TaskCancelled,
    /// `SUBTASK_REQUESTED`.
    SubtaskRequested,
    /// `SUBTASK_DISPOSITIONED`.
    SubtaskDispositioned,
    /// `WORKSPACE_CREATED`.
    WorkspaceCreated,
    /// `CHECKPOINT_WRITTEN`.
    CheckpointWritten,
    /// `WORKSPACE_RECOVERED`.
    WorkspaceRecovered,
    /// `WORKSPACE_REMOVED`.
    WorkspaceRemoved,
    /// `REVIEW_DISPATCHED`.
    ReviewDispatched,
    /// `REVIEW_PASSED`.
    ReviewPassed,
    /// `REVIEW_REJECTED`.
    ReviewRejected,
    /// `FINDING_RAISED`.
    FindingRaised,
    /// `FINDING_DISPOSITIONED`.
    FindingDispositioned,
    /// `REPAIR_ISSUED`.
    RepairIssued,
    /// `REPAIR_LIMIT_REACHED`.
    RepairLimitReached,
    /// `RATE_LIMIT_OBSERVED`.
    RateLimitObserved,
    /// `PROVIDER_DEGRADED`.
    ProviderDegraded,
    /// `PROVIDER_RECOVERED`.
    ProviderRecovered,
    /// `AUTH_REQUIRED`.
    AuthRequired,
    /// `SAFETY_CHECK_PENDING`.
    SafetyCheckPending,
    /// `POLICY_BLOCKED`.
    PolicyBlocked,
    /// `MODEL_DISCOVERED`.
    ModelDiscovered,
    /// `MODEL_LIFECYCLE_CHANGED`.
    ModelLifecycleChanged,
    /// `REGISTRY_REFRESHED`.
    RegistryRefreshed,
    /// `CONTEXT_REHYDRATED`.
    ContextRehydrated,
    /// `CONTEXT_EPOCH_ADVANCED`.
    ContextEpochAdvanced,
    /// `CONTEXT_COMPACTED`.
    ContextCompacted,
    /// `ACCEPTANCE_EVALUATED`.
    AcceptanceEvaluated,
    /// `INTEGRATION_ACCEPTED`.
    IntegrationAccepted,
    /// `INTEGRATION_REJECTED`.
    IntegrationRejected,
    /// `INTEGRATION_BLOCKED`.
    IntegrationBlocked,
    /// `ESCALATED_TO_A2`.
    EscalatedToA2,
    /// `ESCALATED_TO_A1`.
    EscalatedToA1,
    /// `HUMAN_REQUIRED`.
    HumanRequired,
}

impl EventType {
    /// Every frozen event type, in contract order.
    pub(crate) const ALL: [EventType; 53] = [
        EventType::GoalCreated,
        EventType::GoalDecomposed,
        EventType::GoalEvaluated,
        EventType::GoalCompleted,
        EventType::GoalBlocked,
        EventType::RoleCreated,
        EventType::ExecutorSelected,
        EventType::ExecutorBound,
        EventType::ExecutorReleased,
        EventType::ExecutorReplaced,
        EventType::RoutingRequested,
        EventType::RoutingDecided,
        EventType::RoutingFailedNoCandidate,
        EventType::UserRoutingInput,
        EventType::TaskCreated,
        EventType::TaskReady,
        EventType::TaskDispatched,
        EventType::TaskStarted,
        EventType::TaskCompleted,
        EventType::TaskFailed,
        EventType::TaskCancelled,
        EventType::SubtaskRequested,
        EventType::SubtaskDispositioned,
        EventType::WorkspaceCreated,
        EventType::CheckpointWritten,
        EventType::WorkspaceRecovered,
        EventType::WorkspaceRemoved,
        EventType::ReviewDispatched,
        EventType::ReviewPassed,
        EventType::ReviewRejected,
        EventType::FindingRaised,
        EventType::FindingDispositioned,
        EventType::RepairIssued,
        EventType::RepairLimitReached,
        EventType::RateLimitObserved,
        EventType::ProviderDegraded,
        EventType::ProviderRecovered,
        EventType::AuthRequired,
        EventType::SafetyCheckPending,
        EventType::PolicyBlocked,
        EventType::ModelDiscovered,
        EventType::ModelLifecycleChanged,
        EventType::RegistryRefreshed,
        EventType::ContextRehydrated,
        EventType::ContextEpochAdvanced,
        EventType::ContextCompacted,
        EventType::AcceptanceEvaluated,
        EventType::IntegrationAccepted,
        EventType::IntegrationRejected,
        EventType::IntegrationBlocked,
        EventType::EscalatedToA2,
        EventType::EscalatedToA1,
        EventType::HumanRequired,
    ];

    /// The durable storage representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            EventType::GoalCreated => "GOAL_CREATED",
            EventType::GoalDecomposed => "GOAL_DECOMPOSED",
            EventType::GoalEvaluated => "GOAL_EVALUATED",
            EventType::GoalCompleted => "GOAL_COMPLETED",
            EventType::GoalBlocked => "GOAL_BLOCKED",
            EventType::RoleCreated => "ROLE_CREATED",
            EventType::ExecutorSelected => "EXECUTOR_SELECTED",
            EventType::ExecutorBound => "EXECUTOR_BOUND",
            EventType::ExecutorReleased => "EXECUTOR_RELEASED",
            EventType::ExecutorReplaced => "EXECUTOR_REPLACED",
            EventType::RoutingRequested => "ROUTING_REQUESTED",
            EventType::RoutingDecided => "ROUTING_DECIDED",
            EventType::RoutingFailedNoCandidate => "ROUTING_FAILED_NO_CANDIDATE",
            EventType::UserRoutingInput => "USER_ROUTING_INPUT",
            EventType::TaskCreated => "TASK_CREATED",
            EventType::TaskReady => "TASK_READY",
            EventType::TaskDispatched => "TASK_DISPATCHED",
            EventType::TaskStarted => "TASK_STARTED",
            EventType::TaskCompleted => "TASK_COMPLETED",
            EventType::TaskFailed => "TASK_FAILED",
            EventType::TaskCancelled => "TASK_CANCELLED",
            EventType::SubtaskRequested => "SUBTASK_REQUESTED",
            EventType::SubtaskDispositioned => "SUBTASK_DISPOSITIONED",
            EventType::WorkspaceCreated => "WORKSPACE_CREATED",
            EventType::CheckpointWritten => "CHECKPOINT_WRITTEN",
            EventType::WorkspaceRecovered => "WORKSPACE_RECOVERED",
            EventType::WorkspaceRemoved => "WORKSPACE_REMOVED",
            EventType::ReviewDispatched => "REVIEW_DISPATCHED",
            EventType::ReviewPassed => "REVIEW_PASSED",
            EventType::ReviewRejected => "REVIEW_REJECTED",
            EventType::FindingRaised => "FINDING_RAISED",
            EventType::FindingDispositioned => "FINDING_DISPOSITIONED",
            EventType::RepairIssued => "REPAIR_ISSUED",
            EventType::RepairLimitReached => "REPAIR_LIMIT_REACHED",
            EventType::RateLimitObserved => "RATE_LIMIT_OBSERVED",
            EventType::ProviderDegraded => "PROVIDER_DEGRADED",
            EventType::ProviderRecovered => "PROVIDER_RECOVERED",
            EventType::AuthRequired => "AUTH_REQUIRED",
            EventType::SafetyCheckPending => "SAFETY_CHECK_PENDING",
            EventType::PolicyBlocked => "POLICY_BLOCKED",
            EventType::ModelDiscovered => "MODEL_DISCOVERED",
            EventType::ModelLifecycleChanged => "MODEL_LIFECYCLE_CHANGED",
            EventType::RegistryRefreshed => "REGISTRY_REFRESHED",
            EventType::ContextRehydrated => "CONTEXT_REHYDRATED",
            EventType::ContextEpochAdvanced => "CONTEXT_EPOCH_ADVANCED",
            EventType::ContextCompacted => "CONTEXT_COMPACTED",
            EventType::AcceptanceEvaluated => "ACCEPTANCE_EVALUATED",
            EventType::IntegrationAccepted => "INTEGRATION_ACCEPTED",
            EventType::IntegrationRejected => "INTEGRATION_REJECTED",
            EventType::IntegrationBlocked => "INTEGRATION_BLOCKED",
            EventType::EscalatedToA2 => "ESCALATED_TO_A2",
            EventType::EscalatedToA1 => "ESCALATED_TO_A1",
            EventType::HumanRequired => "HUMAN_REQUIRED",
        }
    }

    /// Decodes the durable representation, failing closed on anything other
    /// than the 53 frozen event types.
    pub(crate) fn from_storage(value: &str) -> Result<Self, StateError> {
        for candidate in EventType::ALL {
            if candidate.as_str() == value {
                return Ok(candidate);
            }
        }
        Err(StateError::EventDecodeFailed {
            detail: format!(
                "unknown event_type {value:?}: only the 53 frozen event types are representable"
            ),
        })
    }
}

/// The closed frozen actor-kind set: exactly `SYSTEM`, `ROLE`, `HOST`,
/// `USER`, `PROVIDER`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    /// `SYSTEM`.
    System,
    /// `ROLE`.
    Role,
    /// `HOST`.
    Host,
    /// `USER`.
    User,
    /// `PROVIDER`.
    Provider,
}

impl ActorKind {
    /// Every frozen actor kind, in contract order.
    #[cfg(test)]
    pub(crate) const ALL: [ActorKind; 5] = [
        ActorKind::System,
        ActorKind::Role,
        ActorKind::Host,
        ActorKind::User,
        ActorKind::Provider,
    ];

    /// The durable storage representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            ActorKind::System => "SYSTEM",
            ActorKind::Role => "ROLE",
            ActorKind::Host => "HOST",
            ActorKind::User => "USER",
            ActorKind::Provider => "PROVIDER",
        }
    }

    /// Decodes the durable representation, failing closed on anything other
    /// than the five frozen actor kinds.
    pub(crate) fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "SYSTEM" => Ok(ActorKind::System),
            "ROLE" => Ok(ActorKind::Role),
            "HOST" => Ok(ActorKind::Host),
            "USER" => Ok(ActorKind::User),
            "PROVIDER" => Ok(ActorKind::Provider),
            other => Err(StateError::EventDecodeFailed {
                detail: format!(
                    "unknown actor_kind {other:?}: only the five frozen actor kinds are representable"
                ),
            }),
        }
    }
}

/// The closed frozen subject-kind set: exactly `TASK`, `ROLE`, `WORKSPACE`,
/// `REVIEW`, `PROVIDER`, `GOAL`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectKind {
    /// `TASK`.
    Task,
    /// `ROLE`.
    Role,
    /// `WORKSPACE`.
    Workspace,
    /// `REVIEW`.
    Review,
    /// `PROVIDER`.
    Provider,
    /// `GOAL`.
    Goal,
}

impl SubjectKind {
    /// Every frozen subject kind, in contract order.
    #[cfg(test)]
    pub(crate) const ALL: [SubjectKind; 6] = [
        SubjectKind::Task,
        SubjectKind::Role,
        SubjectKind::Workspace,
        SubjectKind::Review,
        SubjectKind::Provider,
        SubjectKind::Goal,
    ];

    /// The durable storage representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            SubjectKind::Task => "TASK",
            SubjectKind::Role => "ROLE",
            SubjectKind::Workspace => "WORKSPACE",
            SubjectKind::Review => "REVIEW",
            SubjectKind::Provider => "PROVIDER",
            SubjectKind::Goal => "GOAL",
        }
    }

    /// Decodes the durable representation, failing closed on anything other
    /// than the six frozen subject kinds.
    pub(crate) fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "TASK" => Ok(SubjectKind::Task),
            "ROLE" => Ok(SubjectKind::Role),
            "WORKSPACE" => Ok(SubjectKind::Workspace),
            "REVIEW" => Ok(SubjectKind::Review),
            "PROVIDER" => Ok(SubjectKind::Provider),
            "GOAL" => Ok(SubjectKind::Goal),
            other => Err(StateError::EventDecodeFailed {
                detail: format!(
                    "unknown subject_kind {other:?}: only the six frozen subject kinds are representable"
                ),
            }),
        }
    }
}

/// Who caused an event (frozen contract: a kind plus an optional id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventActor {
    /// The closed frozen actor kind.
    pub kind: ActorKind,
    /// Optional opaque structural actor identifier; when present it must be
    /// non-empty. Never invented, defaulted, or rewritten by State.
    pub id: Option<String>,
}

/// What an event is about (frozen contract: a kind plus a required id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSubject {
    /// The closed frozen subject kind.
    pub kind: SubjectKind,
    /// Opaque non-empty structural subject identifier.
    pub id: String,
}

/// The STRICT_W1_EVENT_BOUNDARY payload representation: an opaque reference
/// to separately stored/owned material plus an opaque integrity digest.
///
/// This is the only payload representation in this crate. There is no raw
/// free-form payload body anywhere in the typed API or the storage schema.
/// Both fields are structural metadata only: State never interprets,
/// dereferences, or recomputes them, and no digest algorithm is invented
/// here. The referenced material never becomes executable, authoritative,
/// or a command through persistence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventPayloadReference {
    /// Opaque non-empty reference to separately stored/owned material.
    pub reference: String,
    /// Opaque non-empty integrity digest of that material, as supplied.
    pub digest: String,
}

/// An immutable EventEnvelope row (frozen contract under
/// STRICT_W1_EVENT_BOUNDARY).
///
/// Required fields are non-nullable; `goal_id` and `actor.id` are optional
/// exactly as the contract lists them. Every field is set only at
/// construction and persists byte-for-byte unchanged: the repository offers
/// no mutation path for a persisted event, and no field of this struct is
/// ever redacted, rewritten, or normalized between validation, persistence,
/// and readback.
#[derive(Debug, Clone, PartialEq)]
pub struct EventEnvelope {
    /// Frozen ULID event identity: exactly 26 Crockford Base32 characters.
    /// Validated structurally, then persisted byte-for-byte unchanged.
    pub event_id: String,
    /// Owning project. Non-empty opaque structural identifier.
    pub project_id: String,
    /// Optional goal lineage. When present: non-empty opaque structural
    /// identifier.
    pub goal_id: Option<String>,
    /// The frozen closed event type.
    pub event_type: EventType,
    /// Who caused the event.
    pub actor: EventActor,
    /// What the event is about.
    pub subject: EventSubject,
    /// Opaque contract timestamp, stored as provided. State never reads the
    /// wall clock, parses, compares, or evaluates it.
    pub occurred_at: String,
    /// The strict structural payload representation: reference + digest
    /// only; never a raw free-form payload body.
    pub payload: EventPayloadReference,
    /// Threads one task lineage into a readable story. Non-empty opaque
    /// structural identifier.
    pub correlation_id: String,
    /// Non-negative event epoch.
    pub epoch: i64,
}

impl SqliteStateRepository {
    /// Durably appends one immutable EventEnvelope to the event log.
    ///
    /// Append is the only write operation offered for events. The required
    /// sequence is honored exactly: the entire structural envelope is
    /// validated first (fail-closed, before any storage access), then the
    /// immutable row is inserted inside one transaction, and success is
    /// returned only after that transaction commits. A failed append —
    /// invalid structure, duplicate identity, or a storage/transaction
    /// failure — leaves no partial envelope behind.
    ///
    /// A duplicate `event_id` fails explicitly with
    /// [`StateError::EventAlreadyExists`]: the original row remains
    /// byte-for-byte unchanged, never overwritten, replaced, upserted, or
    /// merged, and the primary-key constraint remains the durable backstop
    /// (including against a concurrent writer on the same database file).
    ///
    /// There is deliberately no parameter for raw payload text, a secret
    /// list, or an `EventRedaction` set: the only payload representation is
    /// the structural [`EventPayloadReference`] carried by the envelope, and
    /// no structural field — including `event_id` — is ever transformed
    /// between validation and persistence.
    pub fn append_event(&mut self, event: EventEnvelope) -> Result<(), StateError> {
        validate_for_append(&event)?;
        self.run_transaction(|uow| uow.insert_event(&event))
    }

    /// Reads one EventEnvelope by durable event identity.
    ///
    /// `Ok(None)` is the deterministic absence result for an event that does
    /// not exist. The persisted envelope is returned exactly as stored:
    /// `event_id` and every other structural field read back byte-for-byte
    /// unchanged. Any decoding failure (a stored value outside the frozen
    /// contract, such as an unknown `event_type`, `actor_kind`,
    /// `subject_kind`, or a negative `epoch`) fails closed instead of
    /// returning partially decoded data, a fabricated default, or an
    /// `UNKNOWN` mapping.
    pub fn find_event(&self, event_id: &str) -> Result<Option<EventEnvelope>, StateError> {
        let conn = self.connection();
        let tx = conn
            .unchecked_transaction()
            .map_err(internal_query_failure)?;
        let found = read_event(&tx, event_id)?;
        // Read-only snapshot: commit merely ends it. On the error path above
        // the transaction rolls back on drop, with nothing written to undo.
        tx.commit().map_err(internal_query_failure)?;
        Ok(found)
    }
}

impl UnitOfWork<'_> {
    /// Inserts one validated EventEnvelope inside the open transaction.
    /// Crate-private; invoked by [`SqliteStateRepository::append_event`].
    pub(crate) fn insert_event(&self, event: &EventEnvelope) -> Result<(), StateError> {
        insert_event(self.tx(), event)
    }
}

const EVENT_EXISTS_SQL: &str = "SELECT 1 FROM event WHERE event_id = ?1";

const INSERT_EVENT_SQL: &str = "INSERT INTO event (
    event_id,
    project_id,
    goal_id,
    event_type,
    actor_kind,
    actor_id,
    subject_kind,
    subject_id,
    occurred_at,
    payload_reference,
    payload_digest,
    correlation_id,
    epoch
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)";

const SELECT_EVENT_SQL: &str = "SELECT
    event_id,
    project_id,
    goal_id,
    event_type,
    actor_kind,
    actor_id,
    subject_kind,
    subject_id,
    occurred_at,
    payload_reference,
    payload_digest,
    correlation_id,
    epoch
FROM event
WHERE event_id = ?1";

/// Inserts the immutable event row using bound parameters.
///
/// Duplicate durable identity is refused twice: an explicit existence check
/// gives the common path a deterministic, driver-independent error, and the
/// primary-key constraint remains the durable backstop. The supplied
/// structural values — including `event_id` — are bound exactly as
/// validated: nothing is rewritten on the way to storage.
fn insert_event(conn: &Connection, event: &EventEnvelope) -> Result<(), StateError> {
    let exists: Option<i64> = conn
        .query_row(EVENT_EXISTS_SQL, [&event.event_id], |row| row.get(0))
        .optional()
        .map_err(write_failure)?;
    if exists.is_some() {
        return Err(StateError::EventAlreadyExists {
            event_id: event.event_id.clone(),
        });
    }

    conn.execute(
        INSERT_EVENT_SQL,
        params![
            event.event_id,
            event.project_id,
            event.goal_id,
            event.event_type.as_str(),
            event.actor.kind.as_str(),
            event.actor.id,
            event.subject.kind.as_str(),
            event.subject.id,
            event.occurred_at,
            event.payload.reference,
            event.payload.digest,
            event.correlation_id,
            event.epoch,
        ],
    )
    .map_err(|error| {
        if is_event_id_constraint_violation(&error) {
            StateError::EventAlreadyExists {
                event_id: event.event_id.clone(),
            }
        } else {
            write_failure(error)
        }
    })?;
    Ok(())
}

/// Reads one event row on the caller's snapshot and applies contract
/// decoding, failing closed on any violation.
fn read_event(conn: &Connection, event_id: &str) -> Result<Option<EventEnvelope>, StateError> {
    conn.query_row(SELECT_EVENT_SQL, [event_id], extract_event_row)
        .optional()
        .map_err(internal_query_failure)?
        .map(EventRow::into_event_envelope)
        .transpose()
}

/// Raw column image of one `event` row, before any contract interpretation
/// is applied.
struct EventRow {
    event_id: String,
    project_id: String,
    goal_id: Option<String>,
    event_type: String,
    actor_kind: String,
    actor_id: Option<String>,
    subject_kind: String,
    subject_id: String,
    occurred_at: String,
    payload_reference: String,
    payload_digest: String,
    correlation_id: String,
    epoch: i64,
}

impl EventRow {
    /// Applies contract decoding (closed enumerations, non-negative epoch)
    /// and fails closed on any violation. Persisted structural strings are
    /// surfaced exactly as stored; they are never repaired or rewritten.
    fn into_event_envelope(self) -> Result<EventEnvelope, StateError> {
        if self.epoch < 0 {
            return Err(StateError::EventDecodeFailed {
                detail: format!("persisted epoch {} is negative", self.epoch),
            });
        }
        Ok(EventEnvelope {
            event_id: self.event_id,
            project_id: self.project_id,
            goal_id: self.goal_id,
            event_type: EventType::from_storage(&self.event_type)?,
            actor: EventActor {
                kind: ActorKind::from_storage(&self.actor_kind)?,
                id: self.actor_id,
            },
            subject: EventSubject {
                kind: SubjectKind::from_storage(&self.subject_kind)?,
                id: self.subject_id,
            },
            occurred_at: self.occurred_at,
            payload: EventPayloadReference {
                reference: self.payload_reference,
                digest: self.payload_digest,
            },
            correlation_id: self.correlation_id,
            epoch: self.epoch,
        })
    }
}

fn extract_event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EventRow> {
    Ok(EventRow {
        event_id: row.get("event_id")?,
        project_id: row.get("project_id")?,
        goal_id: row.get("goal_id")?,
        event_type: row.get("event_type")?,
        actor_kind: row.get("actor_kind")?,
        actor_id: row.get("actor_id")?,
        subject_kind: row.get("subject_kind")?,
        subject_id: row.get("subject_id")?,
        occurred_at: row.get("occurred_at")?,
        payload_reference: row.get("payload_reference")?,
        payload_digest: row.get("payload_digest")?,
        correlation_id: row.get("correlation_id")?,
        epoch: row.get("epoch")?,
    })
}

/// Contract-level validation performed before any storage access on append.
///
/// Exactly the frozen constraints are enforced — no invented orchestration
/// rules and no secret heuristics: `event_id` must be a canonical ULID
/// (exactly 26 Crockford Base32 characters); the opaque structural
/// identifiers `project_id`, `subject.id`, `occurred_at`, `correlation_id`,
/// `payload.reference`, and `payload.digest` must be non-empty; optional
/// `goal_id` and `actor.id` must be non-empty when present; and `epoch`
/// must be non-negative. The enum-typed fields are already restricted to
/// their closed frozen sets by construction.
///
/// Validation never transforms a supplied value: each field is either
/// accepted byte-for-byte unchanged or the append is rejected.
fn validate_for_append(event: &EventEnvelope) -> Result<(), StateError> {
    ensure_canonical_ulid(&event.event_id)?;
    ensure_non_empty("project_id", &event.project_id)?;
    if let Some(goal_id) = &event.goal_id {
        ensure_non_empty("goal_id", goal_id)?;
    }
    if let Some(actor_id) = &event.actor.id {
        ensure_non_empty("actor.id", actor_id)?;
    }
    ensure_non_empty("subject.id", &event.subject.id)?;
    ensure_non_empty("occurred_at", &event.occurred_at)?;
    ensure_non_empty("payload.reference", &event.payload.reference)?;
    ensure_non_empty("payload.digest", &event.payload.digest)?;
    ensure_non_empty("correlation_id", &event.correlation_id)?;
    if event.epoch < 0 {
        return Err(StateError::EventValidation {
            detail: format!("epoch must be >= 0, found {}", event.epoch),
        });
    }
    Ok(())
}

/// Validates that `value` is a canonical ULID: exactly
/// [`EVENT_ID_ULID_LENGTH`] characters, every one from the Crockford Base32
/// alphabet (`0-9`, `A-H`, `J-K`, `M-N`, `P-T`, `V-Z` — `I`, `L`, `O`, and
/// `U` are excluded as non-canonical spellings), and therefore never empty.
///
/// This is structural validation only, implemented without a new
/// dependency: the validated string is used exactly as supplied and is
/// never normalized, re-encoded, or substituted.
fn ensure_canonical_ulid(value: &str) -> Result<(), StateError> {
    let bytes = value.as_bytes();
    if bytes.len() != EVENT_ID_ULID_LENGTH {
        return Err(StateError::EventValidation {
            detail: format!(
                "event_id must be exactly {EVENT_ID_ULID_LENGTH} Crockford Base32 characters, \
                 found {} characters",
                value.chars().count()
            ),
        });
    }
    if let Some(offending) = bytes
        .iter()
        .copied()
        .find(|byte| !is_crockford_base32(*byte))
    {
        return Err(StateError::EventValidation {
            detail: format!(
                "event_id contains {offending:?}, which is not in the Crockford Base32 ULID \
                 alphabet (I, L, O, and U are excluded as non-canonical)"
            ),
        });
    }
    Ok(())
}

/// The Crockford Base32 alphabet used by canonical ULIDs.
const CROCKFORD_BASE32: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn is_crockford_base32(byte: u8) -> bool {
    CROCKFORD_BASE32.contains(&byte)
}

fn ensure_non_empty(field: &str, value: &str) -> Result<(), StateError> {
    if value.is_empty() {
        return Err(StateError::EventValidation {
            detail: format!("{field} must not be empty"),
        });
    }
    Ok(())
}

fn write_failure(error: rusqlite::Error) -> StateError {
    StateError::EventWriteFailed {
        detail: error.to_string(),
    }
}

fn internal_query_failure(error: rusqlite::Error) -> StateError {
    StateError::InternalQueryFailed {
        detail: error.to_string(),
    }
}

/// SQLite extended result codes for identity constraint violations on the
/// `event` primary key: 1555 = `SQLITE_CONSTRAINT_PRIMARYKEY`,
/// 2067 = `SQLITE_CONSTRAINT_UNIQUE`.
fn is_event_id_constraint_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                extended_code: 1555 | 2067,
                ..
            },
            _
        )
    )
}
