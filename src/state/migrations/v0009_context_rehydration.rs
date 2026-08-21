//! Migration 0009: immutable context-rehydration attempts and source evidence.

use super::Migration;

const VERSION: u32 = 9;
const NAME: &str = "context_rehydration";

pub(crate) const MIGRATION: Migration = Migration {
    version: VERSION,
    name: NAME,
    sql: "CREATE TABLE context_rehydration_attempt (
    project_id TEXT NOT NULL,
    rehydration_attempt_id TEXT NOT NULL,
    durable_role_id TEXT NOT NULL,
    context_manifest_id TEXT NOT NULL,
    context_epoch_id INTEGER NOT NULL CHECK (context_epoch_id >= 0),
    trigger_kind TEXT NOT NULL CHECK (trigger_kind IN (
        'A1_INIT', 'A2_INIT', 'MODEL_REPLACEMENT', 'PROVIDER_REPLACEMENT',
        'HOST_SWITCH', 'CONTEXT_COMPACTION', 'ARCHITECTURE_CHANGE',
        'CONTRACT_CHANGE', 'NEW_WAVE', 'TASK_THRESHOLD',
        'SERIOUS_A4_REJECTION', 'SECURITY_ESCALATION',
        'BEFORE_A2_INTEGRATION', 'BEFORE_A1_INTEGRATION',
        'BEFORE_GOAL_COMPLETE'
    )),
    trigger_reference TEXT,
    task_id TEXT,
    correlation_reference TEXT,
    requested_by_actor_kind TEXT NOT NULL CHECK (requested_by_actor_kind IN (
        'SYSTEM', 'ROLE', 'HOST', 'USER', 'PROVIDER'
    )),
    requested_by_actor_id TEXT,
    executor_binding_id TEXT,
    session_reference TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('SUCCEEDED', 'FAILED')),
    failure_code TEXT,
    PRIMARY KEY (project_id, rehydration_attempt_id),
    FOREIGN KEY (durable_role_id, project_id)
        REFERENCES logical_role (role_id, project_id),
    FOREIGN KEY (context_manifest_id)
        REFERENCES context_manifest (manifest_id),
    FOREIGN KEY (project_id, context_epoch_id)
        REFERENCES context_epoch (project_id, epoch),
    FOREIGN KEY (executor_binding_id)
        REFERENCES executor_binding (binding_id),
    CHECK ((status = 'SUCCEEDED' AND failure_code IS NULL)
        OR (status = 'FAILED' AND failure_code IS NOT NULL))
);
CREATE TABLE context_rehydration_repository_snapshot (
    project_id TEXT NOT NULL,
    rehydration_attempt_id TEXT NOT NULL,
    snapshot_ordinal INTEGER NOT NULL CHECK (snapshot_ordinal >= 0),
    repository_id TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    logical_relative_path TEXT NOT NULL,
    PRIMARY KEY (project_id, rehydration_attempt_id, snapshot_ordinal),
    FOREIGN KEY (project_id, rehydration_attempt_id)
        REFERENCES context_rehydration_attempt (project_id, rehydration_attempt_id)
);
CREATE TABLE context_rehydration_source_evidence (
    project_id TEXT NOT NULL,
    rehydration_attempt_id TEXT NOT NULL,
    source_ordinal INTEGER NOT NULL CHECK (source_ordinal >= 0),
    source_id TEXT NOT NULL,
    ref_type TEXT NOT NULL CHECK (ref_type IN ('REPO_PATH', 'STATE_QUERY', 'ARTIFACT_ID')),
    source_class TEXT NOT NULL CHECK (source_class IN ('MANDATORY', 'CONSUMED', 'REFERENCE')),
    canonical_source_identity TEXT NOT NULL,
    materializer_id TEXT,
    provenance TEXT,
    expected_digest TEXT NOT NULL,
    observed_digest TEXT,
    comparison TEXT NOT NULL CHECK (comparison IN ('MATCHED', 'CHANGED', 'NOT_CHECKED', 'FAILED')),
    touch_evidence TEXT,
    disposition TEXT NOT NULL CHECK (disposition IN ('REREAD', 'UNCHANGED', 'DEFERRED', 'FAILED')),
    materialized_at TEXT,
    failure_code TEXT,
    failure_detail TEXT,
    PRIMARY KEY (project_id, rehydration_attempt_id, source_id),
    UNIQUE (project_id, rehydration_attempt_id, source_ordinal),
    FOREIGN KEY (project_id, rehydration_attempt_id)
        REFERENCES context_rehydration_attempt (project_id, rehydration_attempt_id)
);
INSERT INTO state_schema_version (version, migration_name)
VALUES (9, 'context_rehydration');",
};
