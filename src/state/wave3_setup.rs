//! Trusted, internal durability for the Wave 3 consumer ports.
//!
//! Reservations charge one dispatch and append its attempt start atomically.
//! A committed reservation is NOT proof that an external process launched.
//! Replayed or unresolved reservations must be reconciled, never relaunched.
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::{
    AttemptFinish, AttemptId, AttemptStart, GraphPhase, SqliteStateRepository, StateEpochValueV1,
    StateError, graph_repository as graph,
};

/// Closed refusal reasons; no supplied identifiers or payloads are formatted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupRefusalV1 {
    InvalidShape,
    MissingEvidence,
    ForeignEvidence,
    StaleRevision,
    StaleEpoch,
    BudgetExhausted,
    ConflictingReplay,
    UnresolvedLaunch,
}
fn refuse(reason: SetupRefusalV1) -> StateError {
    StateError::SetupRefused { reason }
}
fn number(text: &str) -> Result<StateEpochValueV1, StateError> {
    StateEpochValueV1::try_from(text).map_err(|_| refuse(SetupRefusalV1::InvalidShape))
}
fn valid_ids(ids: &[&str]) -> Result<(), StateError> {
    if ids.iter().all(|id| graph::id(id)) {
        Ok(())
    } else {
        Err(refuse(SetupRefusalV1::InvalidShape))
    }
}
fn encode<T: Serialize>(value: &T) -> Result<String, StateError> {
    serde_json::to_string(value).map_err(|_| refuse(SetupRefusalV1::InvalidShape))
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T, StateError> {
    serde_json::from_str(value).map_err(|_| graph::corrupt())
}
fn sql(error: rusqlite::Error) -> StateError {
    graph::sqlite_error(GraphPhase::Write, error)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupWriteOutcomeV1 {
    Applied,
    Replayed,
}

/// Immutable goal identity and dispatch-count budget. The spec reference names
/// immutable verbatim bytes; State stores its digest, not the spec payload.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalDefinitionV1 {
    pub project_id: String,
    pub goal_id: String,
    pub graph_id: String,
    pub spec_ref: String,
    pub spec_sha256: String,
    /// Canonical unbounded nonnegative integer. Every reservation charges one.
    pub dispatch_limit: String,
}
impl GoalDefinitionV1 {
    fn validate(&self) -> Result<(), StateError> {
        valid_ids(&[&self.project_id, &self.goal_id, &self.graph_id])?;
        if self.spec_ref.is_empty() || !graph::hex(&self.spec_sha256, 64) {
            return Err(refuse(SetupRefusalV1::InvalidShape));
        }
        number(&self.dispatch_limit)?;
        Ok(())
    }
}

/// Owned read projection. Mutating a returned value changes no durable state.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalRecordV1 {
    pub definition: GoalDefinitionV1,
    pub revision: String,
    pub context_epoch: String,
    pub dispatches_used: String,
    /// Opaque immutable continuation artifact; never worker execution authority.
    pub continuation_ref: Option<String>,
}
impl GoalRecordV1 {
    fn validate(&self) -> Result<(), StateError> {
        self.definition.validate()?;
        number(&self.revision)?;
        number(&self.context_epoch)?;
        if number(&self.dispatches_used)? > number(&self.definition.dispatch_limit)?
            || self.continuation_ref.as_ref().is_some_and(|s| s.is_empty())
        {
            return Err(refuse(SetupRefusalV1::InvalidShape));
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalProgressV1 {
    pub project_id: String,
    pub goal_id: String,
    pub expected_revision: String,
    pub expected_epoch: String,
    pub context_epoch: String,
    pub continuation_ref: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalDispatchV1 {
    pub project_id: String,
    pub goal_id: String,
    pub dispatch_id: String,
    pub expected_revision: String,
    pub context_epoch: String,
    pub graph_version: String,
}

/// Append-only reconciliation of the external launch boundary. No resolution
/// grants permission to reuse the old attempt: retries need a new reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LaunchResolutionV1 {
    Launched,
    NotLaunched,
    Uncertain,
}
impl LaunchResolutionV1 {
    fn as_str(self) -> &'static str {
        match self {
            Self::Launched => "LAUNCHED",
            Self::NotLaunched => "NOT_LAUNCHED",
            Self::Uncertain => "UNCERTAIN",
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
pub struct GoalDispatchRecordV1 {
    pub request: GoalDispatchV1,
    pub attempt_id: AttemptId,
    /// None means registration/launch may have been interrupted.
    pub resolution: Option<LaunchResolutionV1>,
    pub evidence_ref: Option<String>,
}

fn goal_at(
    conn: &Connection,
    project: &str,
    goal: &str,
    revision: Option<&str>,
) -> Result<Option<GoalRecordV1>, StateError> {
    let row: Option<(String, String)> = if let Some(rev) = revision {
        conn.query_row("SELECT revision,record_json FROM goal_history WHERE project_id=?1 AND goal_id=?2 AND revision=?3",params![project,goal,rev],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?
    } else {
        conn.query_row("SELECT revision,record_json FROM goal_history WHERE project_id=?1 AND goal_id=?2 ORDER BY length(revision) DESC,revision DESC LIMIT 1",params![project,goal],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?
    };
    row.map(|(stored_revision, s)| {
        let record: GoalRecordV1 = decode(&s)?;
        record.validate().map_err(|_| graph::corrupt())?;
        if record.definition.project_id != project
            || record.definition.goal_id != goal
            || stored_revision != record.revision
        {
            return Err(graph::corrupt());
        }
        Ok(record)
    })
    .transpose()
}
fn insert_goal(conn: &Connection, record: &GoalRecordV1) -> Result<(), StateError> {
    record.validate()?;
    conn.execute(
        "INSERT INTO goal_history VALUES (?1,?2,?3,?4)",
        params![
            record.definition.project_id,
            record.definition.goal_id,
            record.revision,
            encode(record)?
        ],
    )
    .map_err(sql)?;
    crate::write_fault::boundary()
}
fn current_epoch(conn: &Connection, project: &str, expected: &str) -> Result<(), StateError> {
    number(expected)?;
    let latest = crate::context_epoch::query_latest_context_epoch(conn, project)?
        .ok_or_else(|| refuse(SetupRefusalV1::MissingEvidence))?;
    if latest.epoch.as_str() != expected {
        return Err(refuse(SetupRefusalV1::StaleEpoch));
    }
    Ok(())
}
fn check_graph(
    conn: &Connection,
    definition: &GoalDefinitionV1,
) -> Result<crate::PersistedCurrentGraphV1, StateError> {
    let stored = graph::read_projection(conn, &definition.project_id, &definition.graph_id)?
        .ok_or_else(|| refuse(SetupRefusalV1::MissingEvidence))?;
    if stored.goal_id() != definition.goal_id {
        return Err(refuse(SetupRefusalV1::ForeignEvidence));
    }
    Ok(stored)
}
fn expected_goal(
    conn: &Connection,
    project: &str,
    goal: &str,
    revision: &str,
    epoch: &str,
) -> Result<GoalRecordV1, StateError> {
    number(revision)?;
    number(epoch)?;
    let record = goal_at(conn, project, goal, None)?
        .ok_or_else(|| refuse(SetupRefusalV1::MissingEvidence))?;
    if record.revision != revision {
        return Err(refuse(SetupRefusalV1::StaleRevision));
    }
    if record.context_epoch != epoch {
        return Err(refuse(SetupRefusalV1::StaleEpoch));
    }
    Ok(record)
}

impl SqliteStateRepository {
    pub(crate) fn create_goal_v1(
        &mut self,
        definition: &GoalDefinitionV1,
        context_epoch: &str,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        definition.validate()?;
        number(context_epoch)?;
        let record = GoalRecordV1 {
            definition: definition.clone(),
            revision: "0".into(),
            context_epoch: context_epoch.into(),
            dispatches_used: "0".into(),
            continuation_ref: None,
        };
        graph::write_transaction(self.connection(), |conn| {
            if let Some(existing) =
                goal_at(conn, &definition.project_id, &definition.goal_id, Some("0"))?
            {
                return if existing == record {
                    Ok(SetupWriteOutcomeV1::Replayed)
                } else {
                    Err(refuse(SetupRefusalV1::ConflictingReplay))
                };
            }
            let graph = check_graph(conn, definition)?;
            if graph
                .context_epoch()
                .is_some_and(|epoch| epoch.as_str() != context_epoch)
            {
                return Err(refuse(SetupRefusalV1::StaleEpoch));
            }
            current_epoch(conn, &definition.project_id, context_epoch)?;
            insert_goal(conn, &record)?;
            Ok(SetupWriteOutcomeV1::Applied)
        })
    }
    pub(crate) fn advance_goal_v1(
        &mut self,
        progress: &GoalProgressV1,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        valid_ids(&[&progress.project_id, &progress.goal_id])?;
        let next = number(&progress.expected_revision)?
            .successor()?
            .to_string();
        number(&progress.expected_epoch)?;
        number(&progress.context_epoch)?;
        graph::write_transaction(self.connection(), |conn| {
            let prior = goal_at(
                conn,
                &progress.project_id,
                &progress.goal_id,
                Some(&progress.expected_revision),
            )?
            .ok_or_else(|| refuse(SetupRefusalV1::MissingEvidence))?;
            if prior.context_epoch != progress.expected_epoch {
                return Err(refuse(SetupRefusalV1::StaleEpoch));
            }
            let candidate = GoalRecordV1 {
                revision: next.clone(),
                context_epoch: progress.context_epoch.clone(),
                continuation_ref: progress.continuation_ref.clone(),
                ..prior
            };
            candidate.validate()?;
            if let Some(stored) =
                goal_at(conn, &progress.project_id, &progress.goal_id, Some(&next))?
            {
                return if stored == candidate {
                    Ok(SetupWriteOutcomeV1::Replayed)
                } else {
                    Err(refuse(SetupRefusalV1::ConflictingReplay))
                };
            }
            expected_goal(
                conn,
                &progress.project_id,
                &progress.goal_id,
                &progress.expected_revision,
                &progress.expected_epoch,
            )?;
            current_epoch(conn, &progress.project_id, &progress.context_epoch)?;
            check_graph(conn, &candidate.definition)?;
            insert_goal(conn, &candidate)?;
            Ok(SetupWriteOutcomeV1::Applied)
        })
    }
    /// Only Applied permits the caller to attempt this new external launch.
    /// Replayed never permits relaunch; read/reconcile the reservation instead.
    pub(crate) fn reserve_goal_dispatch_v1(
        &mut self,
        request: &GoalDispatchV1,
        start: &AttemptStart,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        valid_ids(&[&request.project_id, &request.goal_id, &request.dispatch_id])?;
        number(&request.expected_revision)?;
        number(&request.context_epoch)?;
        if !graph::positive(&request.graph_version) || request.project_id != start.project_id() {
            return Err(refuse(SetupRefusalV1::ForeignEvidence));
        }
        let identity = encode(&(request, start.canonical()))?;
        graph::write_transaction(self.connection(), |conn| {
            let stored: Option<String> = conn
                .query_row(
                    "SELECT request_json FROM goal_dispatch WHERE project_id=?1 AND dispatch_id=?2",
                    params![request.project_id, request.dispatch_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(sql)?;
            if let Some(stored) = stored {
                return if stored == identity {
                    Ok(SetupWriteOutcomeV1::Replayed)
                } else {
                    Err(refuse(SetupRefusalV1::ConflictingReplay))
                };
            }
            let mut goal = expected_goal(
                conn,
                &request.project_id,
                &request.goal_id,
                &request.expected_revision,
                &request.context_epoch,
            )?;
            current_epoch(conn, &request.project_id, &request.context_epoch)?;
            let graph = check_graph(conn, &goal.definition)?;
            if graph
                .context_epoch()
                .is_some_and(|epoch| epoch.as_str() != request.context_epoch)
            {
                return Err(refuse(SetupRefusalV1::StaleEpoch));
            }
            let start_json = start.canonical();
            if start.graph_id() != goal.definition.graph_id
                || graph.current_version() != request.graph_version
                || !graph
                    .nodes()
                    .iter()
                    .any(|n| Some(n.node_id()) == start_json[3].as_str())
            {
                return Err(refuse(SetupRefusalV1::ForeignEvidence));
            }
            // A previous unfinished external boundary blocks a replacement for
            // that node, while unrelated nodes may reserve concurrently.
            let unresolved:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM goal_dispatch d JOIN attempt_started a ON a.attempt_id=d.attempt_id LEFT JOIN goal_dispatch_resolution r ON r.project_id=d.project_id AND r.dispatch_id=d.dispatch_id LEFT JOIN attempt_finished f ON f.attempt_id=a.attempt_id WHERE d.project_id=?1 AND d.goal_id=?2 AND a.node_id=?3 AND (r.resolution IS NULL OR r.resolution='UNCERTAIN' OR (r.resolution='LAUNCHED' AND f.attempt_id IS NULL)))",params![request.project_id,request.goal_id,start_json[3].as_str()],|r|r.get(0)).map_err(sql)?;
            if unresolved {
                return Err(refuse(SetupRefusalV1::UnresolvedLaunch));
            }
            let used = number(&goal.dispatches_used)?.successor()?;
            if used > number(&goal.definition.dispatch_limit)? {
                return Err(refuse(SetupRefusalV1::BudgetExhausted));
            }
            goal.dispatches_used = used.to_string();
            goal.revision = number(&goal.revision)?.successor()?.to_string();
            crate::attempt::insert_start_in_tx(conn, start)?;
            crate::write_fault::boundary()?;
            conn.execute(
                "INSERT INTO goal_dispatch VALUES (?1,?2,?3,?4,?5)",
                params![
                    request.project_id,
                    request.dispatch_id,
                    request.goal_id,
                    start.attempt_id().as_str(),
                    identity
                ],
            )
            .map_err(sql)?;
            crate::write_fault::boundary()?;
            insert_goal(conn, &goal)?;
            Ok(SetupWriteOutcomeV1::Applied)
        })
    }
    pub(crate) fn resolve_goal_dispatch_v1(
        &mut self,
        project: &str,
        dispatch: &str,
        resolution: LaunchResolutionV1,
        evidence_ref: &str,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        valid_ids(&[project, dispatch])?;
        if evidence_ref.is_empty() {
            return Err(refuse(SetupRefusalV1::InvalidShape));
        }
        graph::write_transaction(self.connection(), |conn| {
            let record = dispatch_in_tx(conn, project, dispatch)?
                .ok_or_else(|| refuse(SetupRefusalV1::MissingEvidence))?;
            if let Some(stored) = record.resolution {
                return if stored == resolution
                    && record.evidence_ref.as_deref() == Some(evidence_ref)
                {
                    Ok(SetupWriteOutcomeV1::Replayed)
                } else {
                    Err(refuse(SetupRefusalV1::ConflictingReplay))
                };
            }
            conn.execute(
                "INSERT INTO goal_dispatch_resolution VALUES (?1,?2,?3,?4)",
                params![project, dispatch, resolution.as_str(), evidence_ref],
            )
            .map_err(sql)?;
            crate::write_fault::boundary()?;
            Ok(SetupWriteOutcomeV1::Applied)
        })
    }
    pub fn find_goal_v1(
        &self,
        project: &str,
        goal: &str,
    ) -> Result<Option<GoalRecordV1>, StateError> {
        valid_ids(&[project, goal])?;
        crate::graph_history::read_transaction(self.connection(), |conn| {
            goal_at(conn, project, goal, None)
        })
    }
    pub fn find_goal_dispatch_v1(
        &self,
        project: &str,
        dispatch: &str,
    ) -> Result<Option<GoalDispatchRecordV1>, StateError> {
        valid_ids(&[project, dispatch])?;
        crate::graph_history::read_transaction(self.connection(), |conn| {
            dispatch_in_tx(conn, project, dispatch)
        })
    }
}
type DispatchRow = (String, String, Option<String>, Option<String>, String);

fn dispatch_in_tx(
    conn: &Connection,
    project: &str,
    dispatch: &str,
) -> Result<Option<GoalDispatchRecordV1>, StateError> {
    let row:Option<DispatchRow>=conn.query_row("SELECT d.request_json,d.attempt_id,r.resolution,r.evidence_ref,d.goal_id FROM goal_dispatch d LEFT JOIN goal_dispatch_resolution r ON r.project_id=d.project_id AND r.dispatch_id=d.dispatch_id WHERE d.project_id=?1 AND d.dispatch_id=?2",params![project,dispatch],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(sql)?;
    row.map(|(json, id, resolution, evidence_ref, goal_id)| {
        let (request, start): (GoalDispatchV1, serde_json::Value) = decode(&json)?;
        if request.goal_id != goal_id
            || request.project_id != project
            || request.dispatch_id != dispatch
            || start[0].as_str() != Some(&id)
        {
            return Err(graph::corrupt());
        }
        let attempt_id = AttemptId::parse(&id).map_err(|_| graph::corrupt())?;
        valid_ids(&[&request.project_id, &request.goal_id, &request.dispatch_id])
            .map_err(|_| graph::corrupt())?;
        number(&request.expected_revision).map_err(|_| graph::corrupt())?;
        number(&request.context_epoch).map_err(|_| graph::corrupt())?;
        if !graph::positive(&request.graph_version) {
            return Err(graph::corrupt());
        }
        let attempt =
            crate::attempt::read_attempt_in_tx(conn, &attempt_id)?.ok_or_else(graph::corrupt)?;
        let goal = goal_at(
            conn,
            project,
            &request.goal_id,
            Some(&request.expected_revision),
        )?
        .ok_or_else(graph::corrupt)?;
        if attempt.project_id() != project
            || attempt.graph_id() != goal.definition.graph_id
            || attempt.start_canonical() != start
            || goal.context_epoch != request.context_epoch
        {
            return Err(graph::corrupt());
        }
        let resolution = resolution
            .map(|s| match s.as_str() {
                "LAUNCHED" => Ok(LaunchResolutionV1::Launched),
                "NOT_LAUNCHED" => Ok(LaunchResolutionV1::NotLaunched),
                "UNCERTAIN" => Ok(LaunchResolutionV1::Uncertain),
                _ => Err(graph::corrupt()),
            })
            .transpose()?;
        Ok(GoalDispatchRecordV1 {
            request,
            attempt_id,
            resolution,
            evidence_ref,
        })
    })
    .transpose()
}

/// Successful mechanical finalization, not a review verdict or acceptance.
/// Checks must equal the authoritative checkpoint's executed checks, and all
/// checked code SHAs must equal final_sha. References never fetch content.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizationRecordV1 {
    pub project_id: String,
    pub graph_id: String,
    pub attempt_id: String,
    pub workspace_id: String,
    pub checkpoint_id: String,
    pub final_sha: String,
    pub write_scope_verdict: String,
    pub check_results: Vec<serde_json::Value>,
    pub output_refs: Vec<String>,
}
impl FinalizationRecordV1 {
    fn validate(&self) -> Result<AttemptId, StateError> {
        valid_ids(&[
            &self.project_id,
            &self.graph_id,
            &self.workspace_id,
            &self.checkpoint_id,
        ])?;
        if !graph::hex(&self.final_sha, 40)
            || self.write_scope_verdict != "PASS"
            || self.output_refs.iter().any(|r| r.is_empty())
        {
            return Err(refuse(SetupRefusalV1::InvalidShape));
        }
        let unique: std::collections::BTreeSet<_> = self.output_refs.iter().collect();
        if unique.len() != self.output_refs.len() {
            return Err(refuse(SetupRefusalV1::InvalidShape));
        }
        AttemptId::parse(&self.attempt_id).map_err(|_| refuse(SetupRefusalV1::InvalidShape))
    }
}
impl SqliteStateRepository {
    pub(crate) fn finish_with_finalization_v1(
        &mut self,
        record: &FinalizationRecordV1,
        finish: &AttemptFinish,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        let id = record.validate()?;
        if &id != finish.attempt_id() || !finish.successful_at(&record.final_sha) {
            return Err(refuse(SetupRefusalV1::ForeignEvidence));
        }
        let json = encode(record)?;
        let finish_json = encode(&finish.canonical())?;
        graph::write_transaction(self.connection(), |conn| {
            let existing:Option<(String,String)>=conn.query_row("SELECT record_json,finish_json FROM attempt_finalization WHERE project_id=?1 AND attempt_id=?2",params![record.project_id,record.attempt_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
            if let Some((a, b)) = existing {
                return if a == json && b == finish_json {
                    let attempt = validate_finalization_evidence(conn, record, &id)
                        .map_err(|_| graph::corrupt())?;
                    if attempt.finish_canonical().as_ref() != Some(&finish.canonical()) {
                        return Err(graph::corrupt());
                    }
                    Ok(SetupWriteOutcomeV1::Replayed)
                } else {
                    Err(refuse(SetupRefusalV1::ConflictingReplay))
                };
            }
            validate_finalization_evidence(conn, record, &id)?;
            crate::attempt::insert_finish_in_tx(conn, finish)?;
            crate::write_fault::boundary()?;
            conn.execute(
                "INSERT INTO attempt_finalization VALUES (?1,?2,?3,?4)",
                params![record.project_id, record.attempt_id, json, finish_json],
            )
            .map_err(sql)?;
            crate::write_fault::boundary()?;
            Ok(SetupWriteOutcomeV1::Applied)
        })
    }
    pub fn find_finalization_v1(
        &self,
        project: &str,
        attempt: &AttemptId,
    ) -> Result<Option<FinalizationRecordV1>, StateError> {
        valid_ids(&[project])?;
        crate::graph_history::read_transaction(self.connection(), |conn| {
            let json:Option<(String,String)>=conn.query_row("SELECT record_json,finish_json FROM attempt_finalization WHERE project_id=?1 AND attempt_id=?2",params![project,attempt.as_str()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
            json.map(|(s, finish_json)| {
                let record: FinalizationRecordV1 = decode(&s)?;
                record.validate().map_err(|_| graph::corrupt())?;
                if record.project_id != project || record.attempt_id != attempt.as_str() {
                    return Err(graph::corrupt());
                }
                let evidence = validate_finalization_evidence(conn, &record, attempt)
                    .map_err(|_| graph::corrupt())?;
                let finish: serde_json::Value = decode(&finish_json)?;
                if evidence.finish_canonical().as_ref() != Some(&finish) {
                    return Err(graph::corrupt());
                }
                Ok(record)
            })
            .transpose()
        })
    }
}

fn validate_finalization_evidence(
    conn: &Connection,
    record: &FinalizationRecordV1,
    id: &AttemptId,
) -> Result<crate::AttemptRecord, StateError> {
    let attempt = crate::attempt::read_attempt_in_tx(conn, id)?
        .ok_or_else(|| refuse(SetupRefusalV1::MissingEvidence))?;
    if attempt.project_id() != record.project_id
        || attempt.graph_id() != record.graph_id
        || attempt.workspace_id() != record.workspace_id
    {
        return Err(refuse(SetupRefusalV1::ForeignEvidence));
    }
    let checkpoint:Option<String>=conn.query_row("SELECT checkpoint_json FROM workspace_checkpoints WHERE project_id=?1 AND checkpoint_id=?2",params![record.project_id,record.checkpoint_id],|r|r.get(0)).optional().map_err(sql)?;
    let checkpoint = checkpoint.ok_or_else(|| refuse(SetupRefusalV1::MissingEvidence))?;
    let checked = crate::CheckpointRecord::parse_json(&record.project_id, &checkpoint)
        .map_err(|_| graph::corrupt())?;
    let checkpoint: serde_json::Value = decode(&checked.to_json()?)?;
    if checkpoint["workspace_id"].as_str() != Some(&record.workspace_id)
        || checkpoint["attempt_id"].as_str() != Some(&record.attempt_id)
        || checkpoint["head_sha"].as_str() != Some(&record.final_sha)
        || checkpoint
            .get("executed_checks")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]))
            != serde_json::json!(record.check_results)
        || record.check_results.iter().any(|c| {
            c["code_sha"].as_str() != Some(&record.final_sha)
                || c["exit_code"] != serde_json::json!(0)
                || c["timed_out"] == serde_json::json!(true)
                || c["result"].as_str().is_some_and(|r| r != "PASS")
        })
    {
        return Err(refuse(SetupRefusalV1::ForeignEvidence));
    }
    for reference in &record.output_refs {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM output_refs WHERE project_id=?1 AND output_ref=?2)",
                params![record.project_id, reference],
                |r| r.get(0),
            )
            .map_err(sql)?;
        if !exists {
            return Err(refuse(SetupRefusalV1::MissingEvidence));
        }
    }
    for check in &record.check_results {
        if let Some(reference) = check.get("output_ref") {
            let target = reference["target"]
                .as_str()
                .ok_or_else(|| refuse(SetupRefusalV1::MissingEvidence))?;
            if !record.output_refs.iter().any(|r| r == target) {
                return Err(refuse(SetupRefusalV1::ForeignEvidence));
            }
        }
    }
    Ok(attempt)
}
