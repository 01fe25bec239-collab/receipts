//! Migration 0004: append-only EventEnvelope persistence.
//!
//! Creates only the structure required by the frozen `Event` envelope under
//! the BUILD-A1/A0 `STRICT_W1_EVENT_BOUNDARY` decision: one row per event,
//! with the payload represented strictly as an opaque `payload_reference` +
//! `payload_digest` pair. There is deliberately no raw free-form payload
//! column: no `payload`, `payload_text`, `raw_payload`, `event_body`, or
//! `raw_json` column exists, so arbitrary event body text has no durable
//! representation to reach. No context, epoch, entitlement, provider, model,
//! routing-decision, task, attempt, review, finding, workspace,
//! host-session, recovery, lease-scheduler, or failover structures belong
//! here.
//!
//! The table-level CHECK constraints mirror the closed `event_type`,
//! `actor_kind`, and `subject_kind` enumerations of the frozen event model
//! (all 53 event types, 5 actor kinds, 6 subject kinds) and the non-negative
//! `epoch`, so the storage itself remains the durable backstop even against
//! writes that bypass the typed repository layer. Identifier non-emptiness,
//! canonical ULID shape for `event_id`, and payload-reference non-emptiness
//! are enforced at the typed boundary, exactly as identifier constraints are
//! for migrations 0002 and 0003. `goal_id`, `actor_id`, `occurred_at`,
//! `payload_reference`, and `payload_digest` are stored as provided: State
//! does not dereference, recompute, compare, or interpret them.
//!
//! `event_id` is the primary key: duplicate durable event identity is
//! refused by the storage backstop, and events are immutable — no UPDATE,
//! DELETE, UPSERT, or REPLACE statement for this table exists anywhere in
//! this crate.

use super::Migration;

const VERSION: u32 = 4;
const NAME: &str = "event";

/// The registered fourth migration.
pub(crate) const MIGRATION: Migration = Migration {
    version: VERSION,
    name: NAME,
    sql: "CREATE TABLE event (
    event_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL,
    goal_id TEXT,
    event_type TEXT NOT NULL CHECK (event_type IN (
        'GOAL_CREATED',
        'GOAL_DECOMPOSED',
        'GOAL_EVALUATED',
        'GOAL_COMPLETED',
        'GOAL_BLOCKED',
        'ROLE_CREATED',
        'EXECUTOR_SELECTED',
        'EXECUTOR_BOUND',
        'EXECUTOR_RELEASED',
        'EXECUTOR_REPLACED',
        'ROUTING_REQUESTED',
        'ROUTING_DECIDED',
        'ROUTING_FAILED_NO_CANDIDATE',
        'USER_ROUTING_INPUT',
        'TASK_CREATED',
        'TASK_READY',
        'TASK_DISPATCHED',
        'TASK_STARTED',
        'TASK_COMPLETED',
        'TASK_FAILED',
        'TASK_CANCELLED',
        'SUBTASK_REQUESTED',
        'SUBTASK_DISPOSITIONED',
        'WORKSPACE_CREATED',
        'CHECKPOINT_WRITTEN',
        'WORKSPACE_RECOVERED',
        'WORKSPACE_REMOVED',
        'REVIEW_DISPATCHED',
        'REVIEW_PASSED',
        'REVIEW_REJECTED',
        'FINDING_RAISED',
        'FINDING_DISPOSITIONED',
        'REPAIR_ISSUED',
        'REPAIR_LIMIT_REACHED',
        'RATE_LIMIT_OBSERVED',
        'PROVIDER_DEGRADED',
        'PROVIDER_RECOVERED',
        'AUTH_REQUIRED',
        'SAFETY_CHECK_PENDING',
        'POLICY_BLOCKED',
        'MODEL_DISCOVERED',
        'MODEL_LIFECYCLE_CHANGED',
        'REGISTRY_REFRESHED',
        'CONTEXT_REHYDRATED',
        'CONTEXT_EPOCH_ADVANCED',
        'CONTEXT_COMPACTED',
        'ACCEPTANCE_EVALUATED',
        'INTEGRATION_ACCEPTED',
        'INTEGRATION_REJECTED',
        'INTEGRATION_BLOCKED',
        'ESCALATED_TO_A2',
        'ESCALATED_TO_A1',
        'HUMAN_REQUIRED'
    )),
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('SYSTEM', 'ROLE', 'HOST', 'USER', 'PROVIDER')),
    actor_id TEXT,
    subject_kind TEXT NOT NULL CHECK (
        subject_kind IN ('TASK', 'ROLE', 'WORKSPACE', 'REVIEW', 'PROVIDER', 'GOAL')
    ),
    subject_id TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    payload_reference TEXT NOT NULL,
    payload_digest TEXT NOT NULL,
    correlation_id TEXT NOT NULL,
    epoch INTEGER NOT NULL CHECK (epoch >= 0)
);
INSERT INTO state_schema_version (version, migration_name)
VALUES (4, 'event');",
};
