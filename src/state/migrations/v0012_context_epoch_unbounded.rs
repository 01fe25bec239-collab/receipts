use rusqlite::{Connection, OptionalExtension};

use crate::error::StateError;

use super::Migration;

pub(crate) const MIGRATION: Migration = Migration {
    version: 12,
    name: "context_epoch_unbounded",
    sql: REBUILD_SQL,
};

const REBUILD_SQL: &str = r#"
CREATE TABLE logical_role_v12 (
    role_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL,
    role_type TEXT NOT NULL CHECK (role_type IN ('RUNTIME_A1', 'RUNTIME_A2')),
    status TEXT NOT NULL CHECK (status IN ('ACTIVE', 'SUSPENDED', 'RETIRED')),
    current_context_epoch TEXT NOT NULL CHECK (typeof(current_context_epoch) = 'text' AND length(current_context_epoch) >= 1 AND current_context_epoch NOT GLOB '*[^0-9]*' AND (current_context_epoch = '0' OR substr(current_context_epoch, 1, 1) BETWEEN '1' AND '9')),
    name TEXT,
    workstream_id TEXT,
    integration_branch TEXT,
    context_manifest_id TEXT,
    active_binding_id TEXT,
    created_at TEXT
);
CREATE TABLE event_v12 (
    event_id TEXT NOT NULL PRIMARY KEY,
    project_id TEXT NOT NULL,
    goal_id TEXT,
    event_type TEXT NOT NULL CHECK (event_type IN ('GOAL_CREATED','GOAL_DECOMPOSED','GOAL_EVALUATED','GOAL_COMPLETED','GOAL_BLOCKED','ROLE_CREATED','EXECUTOR_SELECTED','EXECUTOR_BOUND','EXECUTOR_RELEASED','EXECUTOR_REPLACED','ROUTING_REQUESTED','ROUTING_DECIDED','ROUTING_FAILED_NO_CANDIDATE','USER_ROUTING_INPUT','TASK_CREATED','TASK_READY','TASK_DISPATCHED','TASK_STARTED','TASK_COMPLETED','TASK_FAILED','TASK_CANCELLED','SUBTASK_REQUESTED','SUBTASK_DISPOSITIONED','WORKSPACE_CREATED','CHECKPOINT_WRITTEN','WORKSPACE_RECOVERED','WORKSPACE_REMOVED','REVIEW_DISPATCHED','REVIEW_PASSED','REVIEW_REJECTED','FINDING_RAISED','FINDING_DISPOSITIONED','REPAIR_ISSUED','REPAIR_LIMIT_REACHED','RATE_LIMIT_OBSERVED','PROVIDER_DEGRADED','PROVIDER_RECOVERED','AUTH_REQUIRED','SAFETY_CHECK_PENDING','POLICY_BLOCKED','MODEL_DISCOVERED','MODEL_LIFECYCLE_CHANGED','REGISTRY_REFRESHED','CONTEXT_REHYDRATED','CONTEXT_EPOCH_ADVANCED','CONTEXT_COMPACTED','ACCEPTANCE_EVALUATED','INTEGRATION_ACCEPTED','INTEGRATION_REJECTED','INTEGRATION_BLOCKED','ESCALATED_TO_A2','ESCALATED_TO_A1','HUMAN_REQUIRED')),
    actor_kind TEXT NOT NULL CHECK (actor_kind IN ('SYSTEM', 'ROLE', 'HOST', 'USER', 'PROVIDER')),
    actor_id TEXT,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('TASK', 'ROLE', 'WORKSPACE', 'REVIEW', 'PROVIDER', 'GOAL')),
    subject_id TEXT NOT NULL,
    occurred_at TEXT NOT NULL,
    payload_reference TEXT NOT NULL,
    payload_digest TEXT NOT NULL,
    correlation_id TEXT NOT NULL,
    epoch TEXT NOT NULL CHECK (typeof(epoch) = 'text' AND length(epoch) >= 1 AND epoch NOT GLOB '*[^0-9]*' AND (epoch = '0' OR substr(epoch, 1, 1) BETWEEN '1' AND '9'))
);
CREATE TABLE context_manifest_v12 (
    manifest_id TEXT NOT NULL PRIMARY KEY,
    role_id TEXT NOT NULL UNIQUE REFERENCES logical_role (role_id),
    project_id TEXT NOT NULL,
    epoch TEXT NOT NULL CHECK (typeof(epoch) = 'text' AND length(epoch) >= 1 AND epoch NOT GLOB '*[^0-9]*' AND (epoch = '0' OR substr(epoch, 1, 1) BETWEEN '1' AND '9')),
    created_at TEXT NOT NULL,
    last_rehydrated_at TEXT
);
CREATE TABLE context_epoch_v12 (
    project_id TEXT NOT NULL,
    epoch TEXT NOT NULL CHECK (typeof(epoch) = 'text' AND length(epoch) >= 1 AND epoch NOT GLOB '*[^0-9]*' AND (epoch = '0' OR substr(epoch, 1, 1) BETWEEN '1' AND '9')),
    advanced_at TEXT NOT NULL,
    trigger TEXT NOT NULL CHECK (trigger IN ('A1_INIT','A2_INIT','MODEL_REPLACEMENT','PROVIDER_REPLACEMENT','HOST_SWITCH','CONTEXT_COMPACTION','ARCHITECTURE_CHANGE','CONTRACT_CHANGE','NEW_WAVE','TASK_THRESHOLD','SERIOUS_A4_REJECTION','SECURITY_ESCALATION','BEFORE_A2_INTEGRATION','BEFORE_A1_INTEGRATION','BEFORE_GOAL_COMPLETE')),
    changed_sources_present INTEGER DEFAULT 0 CHECK (changed_sources_present IN (0, 1)),
    PRIMARY KEY (project_id, epoch)
);
CREATE TABLE context_epoch_invalidated_role_v12 (
    project_id TEXT NOT NULL,
    epoch TEXT NOT NULL CHECK (typeof(epoch) = 'text' AND length(epoch) >= 1 AND epoch NOT GLOB '*[^0-9]*' AND (epoch = '0' OR substr(epoch, 1, 1) BETWEEN '1' AND '9')),
    role_id TEXT NOT NULL,
    PRIMARY KEY (project_id, epoch, role_id),
    FOREIGN KEY (project_id, epoch) REFERENCES context_epoch (project_id, epoch),
    FOREIGN KEY (role_id, project_id) REFERENCES logical_role (role_id, project_id)
);
CREATE TABLE context_epoch_changed_source_v12 (
    project_id TEXT NOT NULL,
    epoch TEXT NOT NULL CHECK (typeof(epoch) = 'text' AND length(epoch) >= 1 AND epoch NOT GLOB '*[^0-9]*' AND (epoch = '0' OR substr(epoch, 1, 1) BETWEEN '1' AND '9')),
    source_ordinal INTEGER NOT NULL CHECK (source_ordinal >= 0),
    ref_type TEXT NOT NULL CHECK (ref_type IN ('REPO_PATH', 'STATE_QUERY', 'ARTIFACT_ID', 'URL')),
    target TEXT NOT NULL CHECK (length(CAST(target AS BLOB)) > 0),
    digest TEXT,
    section TEXT,
    PRIMARY KEY (project_id, epoch, source_ordinal),
    FOREIGN KEY (project_id, epoch) REFERENCES context_epoch (project_id, epoch)
);
CREATE TABLE context_rehydration_attempt_v12 (
    project_id TEXT NOT NULL,
    rehydration_attempt_id TEXT NOT NULL,
    durable_role_id TEXT NOT NULL,
    context_manifest_id TEXT NOT NULL,
    context_epoch_id TEXT NOT NULL CHECK (typeof(context_epoch_id) = 'text' AND length(context_epoch_id) >= 1 AND context_epoch_id NOT GLOB '*[^0-9]*' AND (context_epoch_id = '0' OR substr(context_epoch_id, 1, 1) BETWEEN '1' AND '9')),
    trigger_kind TEXT NOT NULL CHECK (trigger_kind IN ('A1_INIT','A2_INIT','MODEL_REPLACEMENT','PROVIDER_REPLACEMENT','HOST_SWITCH','CONTEXT_COMPACTION','ARCHITECTURE_CHANGE','CONTRACT_CHANGE','NEW_WAVE','TASK_THRESHOLD','SERIOUS_A4_REJECTION','SECURITY_ESCALATION','BEFORE_A2_INTEGRATION','BEFORE_A1_INTEGRATION','BEFORE_GOAL_COMPLETE')),
    trigger_reference TEXT,
    task_id TEXT,
    correlation_reference TEXT,
    requested_by_actor_kind TEXT NOT NULL CHECK (requested_by_actor_kind IN ('SYSTEM', 'ROLE', 'HOST', 'USER', 'PROVIDER')),
    requested_by_actor_id TEXT,
    executor_binding_id TEXT,
    session_reference TEXT,
    started_at TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('SUCCEEDED', 'FAILED')),
    failure_code TEXT,
    PRIMARY KEY (project_id, rehydration_attempt_id),
    FOREIGN KEY (durable_role_id, project_id) REFERENCES logical_role (role_id, project_id),
    FOREIGN KEY (context_manifest_id) REFERENCES context_manifest (manifest_id),
    FOREIGN KEY (project_id, context_epoch_id) REFERENCES context_epoch (project_id, epoch),
    FOREIGN KEY (executor_binding_id) REFERENCES executor_binding (binding_id),
    CHECK ((status = 'SUCCEEDED' AND failure_code IS NULL) OR (status = 'FAILED' AND failure_code IS NOT NULL))
);

INSERT INTO logical_role_v12 SELECT role_id, project_id, role_type, status, CAST(current_context_epoch AS TEXT), name, workstream_id, integration_branch, context_manifest_id, active_binding_id, created_at FROM logical_role;
INSERT INTO event_v12 SELECT event_id, project_id, goal_id, event_type, actor_kind, actor_id, subject_kind, subject_id, occurred_at, payload_reference, payload_digest, correlation_id, CAST(epoch AS TEXT) FROM event;
INSERT INTO context_manifest_v12 SELECT manifest_id, role_id, project_id, CAST(epoch AS TEXT), created_at, last_rehydrated_at FROM context_manifest;
INSERT INTO context_epoch_v12 SELECT project_id, CAST(epoch AS TEXT), advanced_at, trigger, changed_sources_present FROM context_epoch;
INSERT INTO context_epoch_invalidated_role_v12 SELECT project_id, CAST(epoch AS TEXT), role_id FROM context_epoch_invalidated_role;
INSERT INTO context_epoch_changed_source_v12 SELECT project_id, CAST(epoch AS TEXT), source_ordinal, ref_type, target, digest, section FROM context_epoch_changed_source;
INSERT INTO context_rehydration_attempt_v12 SELECT project_id, rehydration_attempt_id, durable_role_id, context_manifest_id, CAST(context_epoch_id AS TEXT), trigger_kind, trigger_reference, task_id, correlation_reference, requested_by_actor_kind, requested_by_actor_id, executor_binding_id, session_reference, started_at, completed_at, status, failure_code FROM context_rehydration_attempt;

DROP TABLE context_epoch_changed_source;
DROP TABLE context_epoch_invalidated_role;
DROP TABLE context_rehydration_attempt;
DROP TABLE event;
DROP TABLE context_manifest;
DROP TABLE context_epoch;
DROP TABLE logical_role;

ALTER TABLE logical_role_v12 RENAME TO logical_role;
ALTER TABLE event_v12 RENAME TO event;
ALTER TABLE context_manifest_v12 RENAME TO context_manifest;
ALTER TABLE context_epoch_v12 RENAME TO context_epoch;
ALTER TABLE context_epoch_invalidated_role_v12 RENAME TO context_epoch_invalidated_role;
ALTER TABLE context_epoch_changed_source_v12 RENAME TO context_epoch_changed_source;
ALTER TABLE context_rehydration_attempt_v12 RENAME TO context_rehydration_attempt;

CREATE UNIQUE INDEX idx_logical_role_role_id_project_id ON logical_role (role_id, project_id);
CREATE INDEX idx_context_epoch_project_numeric_epoch ON context_epoch (project_id, length(epoch) DESC, epoch COLLATE BINARY DESC);
INSERT INTO state_schema_version (version, migration_name) VALUES (12, 'context_epoch_unbounded');
"#;

pub(crate) fn apply(conn: &mut Connection) -> Result<(), StateError> {
    require_foreign_keys(conn, 1)?;
    require_no_foreign_key_violations(conn, "before v12 migration")?;
    validate_legacy_epochs(conn)?;
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(v12_failure)?;
    require_foreign_keys(conn, 0)?;

    let migration_result = (|| {
        let tx = conn.transaction().map_err(v12_failure)?;
        tx.execute_batch(REBUILD_SQL).map_err(v12_failure)?;
        require_no_foreign_key_violations(&tx, "inside v12 transaction")?;
        validate_v12_schema(&tx)?;
        tx.commit().map_err(v12_failure)
    })();

    let restore_result = (|| {
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(v12_failure)?;
        require_foreign_keys(conn, 1)?;
        require_no_foreign_key_violations(conn, "after enabling foreign keys")
    })();

    migration_result.and(restore_result)
}

fn validate_legacy_epochs(conn: &Connection) -> Result<(), StateError> {
    for (table, column) in [
        ("logical_role", "current_context_epoch"),
        ("event", "epoch"),
        ("context_manifest", "epoch"),
        ("context_epoch", "epoch"),
        ("context_epoch_invalidated_role", "epoch"),
        ("context_epoch_changed_source", "epoch"),
        ("context_rehydration_attempt", "context_epoch_id"),
    ] {
        let invalid: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE typeof({column}) != 'integer' OR {column} < 0"),
                [],
                |row| row.get(0),
            )
            .map_err(v12_failure)?;
        if invalid != 0 {
            return Err(StateError::V12PhysicalMigrationFailed {
                detail: format!(
                    "legacy {table}.{column} contains {invalid} non-integer or negative value(s)"
                ),
            });
        }
    }
    Ok(())
}

fn validate_v12_schema(conn: &Connection) -> Result<(), StateError> {
    for (table, column) in [
        ("logical_role", "current_context_epoch"),
        ("event", "epoch"),
        ("context_manifest", "epoch"),
        ("context_epoch", "epoch"),
        ("context_epoch_invalidated_role", "epoch"),
        ("context_epoch_changed_source", "epoch"),
        ("context_rehydration_attempt", "context_epoch_id"),
    ] {
        let mut statement = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(v12_failure)?;
        let declared = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
            })
            .map_err(v12_failure)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(v12_failure)?
            .into_iter()
            .find_map(|(name, ty)| (name == column).then_some(ty));
        if declared.as_deref() != Some("TEXT") {
            return Err(StateError::V12PhysicalMigrationFailed {
                detail: format!("{table}.{column} is not declared TEXT"),
            });
        }
        let table_sql: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .map_err(v12_failure)?;
        for fragment in [
            format!("typeof({column}) = 'text'"),
            format!("length({column}) >= 1"),
            format!("{column} NOT GLOB '*[^0-9]*'"),
            format!("{column} = '0' OR substr({column}, 1, 1) BETWEEN '1' AND '9'"),
        ] {
            if !table_sql.contains(&fragment) {
                return Err(StateError::V12PhysicalMigrationFailed {
                    detail: format!("{table}.{column} is missing canonical check {fragment:?}"),
                });
            }
        }
    }
    let index_sql: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'index' AND name = 'idx_context_epoch_project_numeric_epoch'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(v12_failure)?;
    if !index_sql.as_deref().is_some_and(|sql| {
        sql.contains("(project_id, length(epoch) DESC, epoch COLLATE BINARY DESC)")
    }) {
        return Err(StateError::V12PhysicalMigrationFailed {
            detail: "numeric context epoch index is missing or malformed".to_string(),
        });
    }
    Ok(())
}

fn require_foreign_keys(conn: &Connection, expected: i64) -> Result<(), StateError> {
    let found: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
        .map_err(v12_failure)?;
    if found != expected {
        return Err(StateError::V12PhysicalMigrationFailed {
            detail: format!("foreign_keys expected {expected}, found {found}"),
        });
    }
    Ok(())
}

fn require_no_foreign_key_violations(conn: &Connection, phase: &str) -> Result<(), StateError> {
    let violation: Option<(String, i64, String, i64)> = conn
        .query_row("PRAGMA foreign_key_check", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .optional()
        .map_err(v12_failure)?;
    if let Some(violation) = violation {
        return Err(StateError::V12PhysicalMigrationFailed {
            detail: format!("foreign_key_check failed {phase}: {violation:?}"),
        });
    }
    Ok(())
}

fn v12_failure(error: rusqlite::Error) -> StateError {
    StateError::V12PhysicalMigrationFailed {
        detail: error.to_string(),
    }
}
