//! D4 metadata authority. Only trusted composition may supply provisioning,
//! handoff and physical readback evidence; State never observes or signals a
//! process and never resets files. No production composition is installed here.
use crate::{
    AttemptId, CanonicalTimestampV1, DecisionRecordKind, SqliteStateRepository, StateError,
    TrustedClockV1,
};
use crate::{graph_repository as graph, trusted_time};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRecoveryRefusalV1 {
    InvalidShape,
    MissingEvidence,
    ForeignEvidence,
    ConflictingReplay,
    StaleAuthority,
    NotExpired,
    HandoffUnavailable,
    EligibleHandoff,
    ActiveRecovery,
    AlreadyResolved,
    InvalidPhase,
    UnverifiedReset,
}
fn refuse(reason: WorkspaceRecoveryRefusalV1) -> StateError {
    StateError::WorkspaceRecoveryRefused { reason }
}
use WorkspaceRecoveryRefusalV1 as R;
fn sql(e: rusqlite::Error) -> StateError {
    graph::sqlite_error(crate::GraphPhase::Write, e)
}
fn encode<T: Serialize>(v: &T) -> Result<String, StateError> {
    serde_json::to_string(v).map_err(|_| refuse(R::InvalidShape))
}
fn decode<T: serde::de::DeserializeOwned>(v: &str) -> Result<T, StateError> {
    serde_json::from_str(v).map_err(|_| graph::corrupt())
}
fn ids(values: &[&str]) -> Result<(), StateError> {
    if values.iter().all(|s| graph::id(s)) {
        Ok(())
    } else {
        Err(refuse(R::InvalidShape))
    }
}
fn hex(s: &str, n: usize) -> Result<(), StateError> {
    if graph::hex(s, n) {
        Ok(())
    } else {
        Err(refuse(R::InvalidShape))
    }
}
fn digest(s: &str) -> String {
    crate::review_claim::sha256_hex(s.as_bytes())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRecoverySubjectV1 {
    pub project_id: String,
    pub graph_id: String,
    pub attempt_id: String,
}
impl WorkspaceRecoverySubjectV1 {
    fn validate(&self) -> Result<AttemptId, StateError> {
        ids(&[&self.project_id, &self.graph_id])?;
        AttemptId::parse(&self.attempt_id)
    }
}
/// An explicit trusted composition association, not a caller's desired SHA.
/// Its provisioning identity must come from the actual accepted workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceAcceptedTargetV1 {
    pub subject: WorkspaceRecoverySubjectV1,
    pub anchor_id: String,
    pub workspace_id: String,
    pub accepted_commit: String,
    pub accepted_tree: String,
    pub acceptance_claim_id: String,
    pub decision_id: String,
    pub provisioning_identity: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRecoveryAdmissionV1 {
    pub subject: WorkspaceRecoverySubjectV1,
    pub workspace_id: String,
    pub role_id: String,
    pub binding_id: String,
    pub lease_deadline: String,
    pub expected_execution_deadline: String,
    pub context_epoch: String,
    pub provisioning_identity: String,
    pub supervisor_identity: String,
    pub accepted_anchor_id: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkspaceHandoffDispositionV1 {
    Pending,
    Eligible,
    ClosedWithoutHandoff,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceHandoffRecordV1 {
    pub subject: WorkspaceRecoverySubjectV1,
    pub expected_revision: i64,
    pub disposition: WorkspaceHandoffDispositionV1,
    pub evidence_ref: String,
    pub evidence_sha256: String,
    pub provisioning_identity: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRecoveryClaimRequestV1 {
    pub subject: WorkspaceRecoverySubjectV1,
    pub recovery_id: String,
    pub admission_sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRecoveryAuthorityV1 {
    pub subject: WorkspaceRecoverySubjectV1,
    pub recovery_id: String,
    pub expected_revision: i64,
    pub provisioning_identity: String,
    pub supervisor_identity: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRecoveryEvidenceV1 {
    pub recovery_id: String,
    pub checkpoint_id: String,
    pub checkpoint_sha256: String,
    pub output_refs: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkspaceRecoveryEvidenceStageV1 {
    Captured,
    Quiescent,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkspaceRecoveryActionKindV1 {
    Signal,
    Reset,
}
/// Physical observations are supplied by the admitted supervisor, never by a worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceRecoveryResetObservationV1 {
    pub evidence: WorkspaceRecoveryEvidenceV1,
    pub observed_commit: String,
    pub index_tree: String,
    pub tracked_tree: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceRecoveryRecordV1 {
    pub subject: WorkspaceRecoverySubjectV1,
    pub recovery_id: String,
    pub revision: i64,
    pub phase: String,
    pub active: bool,
}
/// Non-cloneable current-action capability. Its mutable borrow retains the
/// exclusive writer through the action; dropping it leaves PREPARED custody.
/// It does not expose any process or filesystem operation.
pub struct WorkspaceRecoveryActionGuardV1<'a> {
    repo: &'a mut SqliteStateRepository,
    authority: WorkspaceRecoveryAuthorityV1,
    kind: WorkspaceRecoveryActionKindV1,
}
impl WorkspaceRecoveryActionGuardV1<'_> {
    pub fn revision(&self) -> i64 {
        self.authority.expected_revision
    }
    pub fn finish_signal(
        self,
        clock: &dyn TrustedClockV1,
        evidence: &WorkspaceRecoveryEvidenceV1,
    ) -> Result<WorkspaceRecoveryRecordV1, StateError> {
        if self.kind != WorkspaceRecoveryActionKindV1::Signal {
            return Err(refuse(R::InvalidPhase));
        }
        self.repo.acknowledge_workspace_recovery_v1(
            clock,
            &self.authority,
            WorkspaceRecoveryEvidenceStageV1::Captured,
            evidence,
        )
    }
    pub fn verify_reset(
        self,
        clock: &dyn TrustedClockV1,
        observation: &WorkspaceRecoveryResetObservationV1,
    ) -> Result<WorkspaceRecoveryRecordV1, StateError> {
        if self.kind != WorkspaceRecoveryActionKindV1::Reset {
            return Err(refuse(R::InvalidPhase));
        }
        self.repo
            .resolve_workspace_recovery_v1(clock, &self.authority, observation)
    }
}

fn admission(
    conn: &Connection,
    subject: &WorkspaceRecoverySubjectV1,
) -> Result<WorkspaceRecoveryAdmissionV1, StateError> {
    subject.validate()?;
    let json:Option<String>=conn.query_row("SELECT record_json FROM workspace_recovery_admission WHERE project_id=?1 AND attempt_id=?2",params![subject.project_id,subject.attempt_id],|r|r.get(0)).optional().map_err(sql)?;
    let value: WorkspaceRecoveryAdmissionV1 =
        decode(&json.ok_or_else(|| refuse(R::MissingEvidence))?)?;
    if value.subject != *subject {
        return Err(refuse(R::ForeignEvidence));
    }
    Ok(value)
}
fn anchor(
    conn: &Connection,
    a: &WorkspaceRecoveryAdmissionV1,
) -> Result<WorkspaceAcceptedTargetV1, StateError> {
    let json:Option<String>=conn.query_row("SELECT record_json FROM workspace_recovery_acceptance WHERE project_id=?1 AND anchor_id=?2",params![a.subject.project_id,a.accepted_anchor_id],|r|r.get(0)).optional().map_err(sql)?;
    let target: WorkspaceAcceptedTargetV1 =
        decode(&json.ok_or_else(|| refuse(R::MissingEvidence))?)?;
    if target.subject.project_id != a.subject.project_id
        || target.subject.graph_id != a.subject.graph_id
        || target.workspace_id != a.workspace_id
        || target.subject.attempt_id == a.subject.attempt_id
    {
        return Err(refuse(R::ForeignEvidence));
    }
    validate_acceptance(conn, &target)?;
    Ok(target)
}
fn validate_acceptance(conn: &Connection, t: &WorkspaceAcceptedTargetV1) -> Result<(), StateError> {
    let id = t.subject.validate()?;
    ids(&[
        &t.anchor_id,
        &t.workspace_id,
        &t.acceptance_claim_id,
        &t.decision_id,
    ])?;
    hex(&t.accepted_commit, 40)?;
    hex(&t.accepted_tree, 40)?;
    hex(&t.provisioning_identity, 64)?;
    let attempt =
        crate::attempt::read_attempt_in_tx(conn, &id)?.ok_or_else(|| refuse(R::MissingEvidence))?;
    if attempt.project_id() != t.subject.project_id
        || attempt.graph_id() != t.subject.graph_id
        || attempt.workspace_id() != t.workspace_id
        || !attempt.is_finished()
        || attempt.end_sha() != Some(&t.accepted_commit)
        || attempt.failure_class().is_some()
    {
        return Err(refuse(R::ForeignEvidence));
    }
    let finalization:Option<(String,String)>=conn.query_row("SELECT record_json,finish_json FROM attempt_finalization WHERE project_id=?1 AND attempt_id=?2",params![t.subject.project_id,t.subject.attempt_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
    let (json, finish) = finalization.ok_or_else(|| refuse(R::MissingEvidence))?;
    let finalization: crate::FinalizationRecordV1 = decode(&json)?;
    let verified = crate::wave3_setup::validate_finalization_evidence(conn, &finalization, &id)?;
    if finalization.final_sha != t.accepted_commit
        || finalization.workspace_id != t.workspace_id
        || verified.finish_canonical().as_ref() != Some(&decode::<serde_json::Value>(&finish)?)
    {
        return Err(refuse(R::ForeignEvidence));
    }
    let claim =
        crate::review_claim::read_claim(conn, &t.subject.project_id, &t.acceptance_claim_id)?
            .ok_or_else(|| refuse(R::MissingEvidence))?;
    let seal = crate::review_claim::read_seal(conn, &t.subject.project_id, &t.acceptance_claim_id)?
        .ok_or_else(|| refuse(R::MissingEvidence))?;
    let decision = crate::decision_records::find_record(
        conn,
        &t.subject.project_id,
        DecisionRecordKind::IntegrationDecision,
        &t.decision_id,
    )?
    .ok_or_else(|| refuse(R::MissingEvidence))?;
    let publication:Option<String>=conn.query_row("SELECT package_sha256 FROM atomic_publication WHERE project_id=?1 AND publication_id=?2",params![t.subject.project_id,seal.publication_id],|r|r.get(0)).optional().map_err(sql)?;
    if claim.state != crate::ReviewClaimStateV1::Completed
        || claim.task_id != attempt.node_id()
        || claim.subject_sha.as_deref() != Some(&t.accepted_commit)
        || claim.outcome_kind.as_deref() != Some("ACCEPTANCE_EVALUATED")
        || claim.outcome_ref.as_deref() != Some(&t.decision_id)
        || claim.outcome_sha256.as_deref() != Some(decision.record_sha256())
        || seal.task_id != claim.task_id
        || seal.subject_sha != claim.subject_sha
        || seal.request_sha256 != claim.request_sha256
        || publication.as_deref() != Some(&seal.package_sha256)
        || seal.settlement.outcome_ref() != Some(&t.decision_id)
        || seal.settlement.outcome_sha256() != Some(decision.record_sha256())
    {
        return Err(refuse(R::ForeignEvidence));
    }
    let value: serde_json::Value = decode(decision.record_json())?;
    let chain = value["provenance_chain"]
        .as_array()
        .ok_or_else(|| refuse(R::MissingEvidence))?;
    let matching: Vec<_> = chain
        .iter()
        .filter(|p| p["task_id"].as_str() == Some(attempt.node_id()))
        .collect();
    if value["outcome"].as_str() != Some("ACCEPT") || matching.len() != 1 {
        return Err(refuse(R::ForeignEvidence));
    }
    let p = matching[0];
    if p["start_sha"].as_str() != Some(attempt.start_sha())
        || ["implementation_sha", "review_sha", "acceptance_sha"]
            .iter()
            .any(|key| p[key].as_str() != Some(&t.accepted_commit))
    {
        return Err(refuse(R::ForeignEvidence));
    }
    Ok(())
}
fn current(
    conn: &Connection,
    a: &WorkspaceRecoveryAdmissionV1,
    expired: bool,
    now: &CanonicalTimestampV1,
) -> Result<(), StateError> {
    let target = anchor(conn, a)?;
    let id = a.subject.validate()?;
    let attempt =
        crate::attempt::read_attempt_in_tx(conn, &id)?.ok_or_else(|| refuse(R::MissingEvidence))?;
    if attempt.is_finished() {
        return Err(refuse(R::StaleAuthority));
    }
    let successor:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM attempt_started other LEFT JOIN attempt_finished f ON f.attempt_id=other.attempt_id WHERE other.project_id=?1 AND other.graph_id=?2 AND other.node_id=?3 AND other.attempt_id<>?4 AND other.attempt_id<>?5 AND NOT EXISTS(SELECT 1 FROM workspace_recovery_resolution z WHERE z.project_id=other.project_id AND z.attempt_id=other.attempt_id) AND (f.attempt_id IS NULL OR other.started_at>=?6))",params![a.subject.project_id,a.subject.graph_id,attempt.node_id(),a.subject.attempt_id,target.subject.attempt_id,attempt.started_at().as_str()],|r|r.get(0)).map_err(sql)?;
    if successor {
        return Err(refuse(R::StaleAuthority));
    }
    let role = crate::logical_role::read_logical_role(conn, &a.role_id)?
        .ok_or_else(|| refuse(R::MissingEvidence))?;
    let binding = crate::executor_binding::read_executor_binding(conn, &a.binding_id)?
        .ok_or_else(|| refuse(R::MissingEvidence))?;
    let epoch = crate::context_epoch::query_latest_context_epoch(conn, &a.subject.project_id)?
        .ok_or_else(|| refuse(R::MissingEvidence))?;
    if attempt.project_id() != a.subject.project_id
        || attempt.graph_id() != a.subject.graph_id
        || attempt.workspace_id() != a.workspace_id
        || attempt.node_id() != target_task(conn, &target)?
        || attempt.start_sha() != target.accepted_commit
        || role.project_id != a.subject.project_id
        || role.status != crate::LogicalRoleStatus::Active
        || role.active_binding_id.as_deref() != Some(&a.binding_id)
        || role.current_context_epoch.as_str() != a.context_epoch
        || epoch.epoch.as_str() != a.context_epoch
        || binding.role_id != a.role_id
        || binding.lease_expires_at != a.lease_deadline
        || (binding.released_at.is_some()
            && binding.release_reason != Some(crate::ReleaseReason::LeaseExpired))
        || binding.released_at.is_some() != binding.release_reason.is_some()
    {
        return Err(refuse(R::StaleAuthority));
    }
    let invalidated:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM context_epoch_invalidated_role WHERE project_id=?1 AND epoch=?2 AND role_id=?3)",params![a.subject.project_id,a.context_epoch,a.role_id],|r|r.get(0)).map_err(sql)?;
    if invalidated || attempt.runtime_ref() != binding.runtime_id {
        return Err(refuse(R::StaleAuthority));
    }
    if expired
        && (now < &CanonicalTimestampV1::parse(&a.lease_deadline)?
            || now < &CanonicalTimestampV1::parse(&a.expected_execution_deadline)?)
    {
        return Err(refuse(R::NotExpired));
    }
    Ok(())
}
fn target_task(conn: &Connection, t: &WorkspaceAcceptedTargetV1) -> Result<String, StateError> {
    Ok(
        crate::attempt::read_attempt_in_tx(conn, &t.subject.validate()?)?
            .ok_or_else(|| refuse(R::MissingEvidence))?
            .node_id()
            .into(),
    )
}
fn handoff(
    conn: &Connection,
    s: &WorkspaceRecoverySubjectV1,
) -> Result<Option<(i64, String, String)>, StateError> {
    conn.query_row("SELECT revision,disposition,record_json FROM workspace_recovery_handoff WHERE project_id=?1 AND attempt_id=?2 ORDER BY revision DESC LIMIT 1",params![s.project_id,s.attempt_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(sql)
}
fn closed(conn: &Connection, s: &WorkspaceRecoverySubjectV1) -> Result<(), StateError> {
    match handoff(conn, s)?.map(|(_, v, _)| v).as_deref() {
        Some("CLOSED_WITHOUT_HANDOFF") => Ok(()),
        Some("ELIGIBLE") => Err(refuse(R::EligibleHandoff)),
        _ => Err(refuse(R::HandoffUnavailable)),
    }
}
fn record(
    conn: &Connection,
    s: &WorkspaceRecoverySubjectV1,
    recovery: &str,
) -> Result<WorkspaceRecoveryRecordV1, StateError> {
    let row:Option<(String,i64,String,bool)>=conn.query_row("SELECT attempt_id,revision,phase,active FROM workspace_recovery_control WHERE project_id=?1 AND recovery_id=?2",params![s.project_id,recovery],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sql)?;
    let (attempt, revision, phase, active) = row.ok_or_else(|| refuse(R::MissingEvidence))?;
    let a = admission(conn, s)?;
    if attempt != a.subject.attempt_id {
        return Err(refuse(R::ForeignEvidence));
    }
    Ok(WorkspaceRecoveryRecordV1 {
        subject: s.clone(),
        recovery_id: recovery.into(),
        revision,
        phase,
        active,
    })
}
fn authorized(
    conn: &Connection,
    a: &WorkspaceRecoveryAuthorityV1,
    now: &CanonicalTimestampV1,
) -> Result<(WorkspaceRecoveryRecordV1, WorkspaceRecoveryAdmissionV1), StateError> {
    let admitted = admission(conn, &a.subject)?;
    if a.provisioning_identity != admitted.provisioning_identity
        || a.supervisor_identity != admitted.supervisor_identity
    {
        return Err(refuse(R::ForeignEvidence));
    }
    let r = record(conn, &a.subject, &a.recovery_id)?;
    if !r.active {
        return Err(refuse(R::AlreadyResolved));
    }
    if r.revision != a.expected_revision {
        return Err(refuse(R::StaleAuthority));
    }
    current(conn, &admitted, true, now)?;
    closed(conn, &a.subject)?;
    Ok((r, admitted))
}
fn evidence(
    conn: &Connection,
    a: &WorkspaceRecoveryAdmissionV1,
    e: &WorkspaceRecoveryEvidenceV1,
    now: &CanonicalTimestampV1,
) -> Result<serde_json::Value, StateError> {
    ids(&[&e.checkpoint_id])?;
    hex(&e.checkpoint_sha256, 64)?;
    if e.output_refs.is_empty()
        || e.output_refs.len() > 1000
        || e.output_refs
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != e.output_refs.len()
    {
        return Err(refuse(R::MissingEvidence));
    }
    let claimed:Option<String>=conn.query_row("SELECT claimed_at FROM workspace_recovery_control WHERE project_id=?1 AND attempt_id=?2 AND recovery_id=?3 AND active=1",params![a.subject.project_id,a.subject.attempt_id,e.recovery_id],|r|r.get(0)).optional().map_err(sql)?;
    let claimed = CanonicalTimestampV1::parse(&claimed.ok_or_else(|| refuse(R::ForeignEvidence))?)?;
    let cp:Option<String>=conn.query_row("SELECT checkpoint_json FROM workspace_checkpoints WHERE project_id=?1 AND checkpoint_id=?2",params![a.subject.project_id,e.checkpoint_id],|r|r.get(0)).optional().map_err(sql)?;
    let cp = cp.ok_or_else(|| refuse(R::MissingEvidence))?;
    crate::CheckpointRecord::parse_json(&a.subject.project_id, &cp)?;
    let value: serde_json::Value = decode(&cp)?;
    if digest(&cp) != e.checkpoint_sha256
        || value["workspace_id"].as_str() != Some(&a.workspace_id)
        || value["attempt_id"].as_str() != Some(&a.subject.attempt_id)
        || value["kind"].as_str() != Some("RECOVERY_CAPTURE")
    {
        return Err(refuse(R::ForeignEvidence));
    }
    let captured = CanonicalTimestampV1::parse(
        value["captured_at"]
            .as_str()
            .ok_or_else(|| refuse(R::MissingEvidence))?,
    )?;
    if captured < claimed || &captured > now {
        return Err(refuse(R::StaleAuthority));
    }
    let diff = &value["dirty_diff_ref"];
    if diff["ref_type"].as_str() != Some("ARTIFACT_ID")
        || !e
            .output_refs
            .iter()
            .any(|r| Some(r.as_str()) == diff["target"].as_str())
    {
        return Err(refuse(R::MissingEvidence));
    }
    for reference in &e.output_refs {
        let row:Option<(String,String,String)>=conn.query_row("SELECT sha256,storage_uri,byte_length FROM output_refs WHERE project_id=?1 AND output_ref=?2",params![a.subject.project_id,reference],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(sql)?;
        let (hash, uri, length) = row.ok_or_else(|| refuse(R::MissingEvidence))?;
        crate::OutputRef::new(&a.subject.project_id, reference, &hash, &uri, &length)?;
        if Some(reference.as_str()) == diff["target"].as_str()
            && diff["digest"].as_str() != Some(hash.as_str())
        {
            return Err(refuse(R::ForeignEvidence));
        }
    }
    Ok(value)
}
fn advance(
    conn: &Connection,
    r: &WorkspaceRecoveryRecordV1,
    phase: &str,
    payload: &str,
    active: bool,
) -> Result<WorkspaceRecoveryRecordV1, StateError> {
    let next = r
        .revision
        .checked_add(1)
        .ok_or_else(|| refuse(R::InvalidShape))?;
    conn.execute(
        "INSERT INTO workspace_recovery_evidence VALUES(?1,?2,?3,?4)",
        params![r.subject.project_id, r.recovery_id, next, payload],
    )
    .map_err(sql)?;
    crate::write_fault::boundary()?;
    conn.execute("UPDATE workspace_recovery_control SET revision=?3,phase=?4,active=?5 WHERE project_id=?1 AND recovery_id=?2",params![r.subject.project_id,r.recovery_id,next,phase,active]).map_err(sql)?;
    crate::write_fault::boundary()?;
    Ok(WorkspaceRecoveryRecordV1 {
        revision: next,
        phase: phase.into(),
        active,
        ..r.clone()
    })
}

impl SqliteStateRepository {
    pub(crate) fn bind_workspace_acceptance_v1(
        &mut self,
        t: &WorkspaceAcceptedTargetV1,
    ) -> Result<crate::SetupWriteOutcomeV1, StateError> {
        let json = encode(t)?;
        graph::write_transaction(self.connection(), |conn| {
            validate_acceptance(conn, t)?;
            let old:Option<String>=conn.query_row("SELECT record_json FROM workspace_recovery_acceptance WHERE project_id=?1 AND anchor_id=?2",params![t.subject.project_id,t.anchor_id],|r|r.get(0)).optional().map_err(sql)?;
            if let Some(old) = old {
                return if old == json {
                    Ok(crate::SetupWriteOutcomeV1::Replayed)
                } else {
                    Err(refuse(R::ConflictingReplay))
                };
            }
            guard_project(conn, &t.subject.project_id)?;
            conn.execute(
                "INSERT INTO workspace_recovery_acceptance VALUES(?1,?2,?3,?4)",
                params![t.subject.project_id, t.anchor_id, t.workspace_id, json],
            )
            .map_err(sql)?;
            Ok(crate::SetupWriteOutcomeV1::Applied)
        })
    }
    pub(crate) fn admit_workspace_recovery_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        a: &WorkspaceRecoveryAdmissionV1,
    ) -> Result<String, StateError> {
        let json = encode(a)?;
        a.subject.validate()?;
        ids(&[
            &a.workspace_id,
            &a.role_id,
            &a.binding_id,
            &a.accepted_anchor_id,
        ])?;
        hex(&a.provisioning_identity, 64)?;
        hex(&a.supervisor_identity, 64)?;
        CanonicalTimestampV1::parse(&a.lease_deadline)?;
        CanonicalTimestampV1::parse(&a.expected_execution_deadline)?;
        crate::StateEpochValueV1::try_from(a.context_epoch.as_str())?;
        let (sample, now) = trusted_time::validate_sample(clock.sample()?)?;
        graph::write_transaction(self.connection(), |conn| {
            trusted_time::accept_sample(conn, &a.subject.project_id, &sample, &now)?;
            let old:Option<String>=conn.query_row("SELECT record_json FROM workspace_recovery_admission WHERE project_id=?1 AND attempt_id=?2",params![a.subject.project_id,a.subject.attempt_id],|r|r.get(0)).optional().map_err(sql)?;
            if let Some(old) = old {
                return if old == json {
                    Ok(digest(&json))
                } else {
                    Err(refuse(R::ConflictingReplay))
                };
            }
            current(conn, a, false, &now)?;
            if now >= CanonicalTimestampV1::parse(&a.lease_deadline)?
                || now >= CanonicalTimestampV1::parse(&a.expected_execution_deadline)?
            {
                return Err(refuse(R::NotExpired));
            }
            guard_workspace(conn, &a.subject.project_id, &a.workspace_id)?;
            let task = crate::attempt::read_attempt_in_tx(conn, &a.subject.validate()?)?
                .ok_or_else(|| refuse(R::MissingEvidence))?;
            conn.execute(
                "INSERT INTO workspace_recovery_admission VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    a.subject.project_id,
                    a.subject.attempt_id,
                    a.workspace_id,
                    a.role_id,
                    a.binding_id,
                    task.node_id(),
                    json
                ],
            )
            .map_err(sql)?;
            Ok(digest(&json))
        })
    }
    pub(crate) fn record_workspace_handoff_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        h: &WorkspaceHandoffRecordV1,
    ) -> Result<i64, StateError> {
        h.subject.validate()?;
        ids(&[&h.evidence_ref])?;
        hex(&h.evidence_sha256, 64)?;
        let json = encode(h)?;
        let (sample, now) = trusted_time::validate_sample(clock.sample()?)?;
        graph::write_transaction(self.connection(), |conn| {
            trusted_time::accept_sample(conn, &h.subject.project_id, &sample, &now)?;
            let a = admission(conn, &h.subject)?;
            if a.provisioning_identity != h.provisioning_identity {
                return Err(refuse(R::ForeignEvidence));
            }
            let previous = handoff(conn, &h.subject)?;
            if let Some((rev, _, ref old)) = previous
                && old == &json
            {
                return Ok(rev);
            }
            if previous.as_ref().map_or(0, |v| v.0) != h.expected_revision {
                return Err(refuse(R::StaleAuthority));
            }
            guard_attempt(conn, &h.subject.attempt_id)?;
            if previous.as_ref().is_some_and(|p| p.1 != "PENDING") {
                return Err(refuse(R::InvalidPhase));
            }
            if h.disposition == WorkspaceHandoffDispositionV1::Eligible {
                let finalized:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM attempt_finalization WHERE project_id=?1 AND attempt_id=?2)",params![h.subject.project_id,h.subject.attempt_id],|r|r.get(0)).map_err(sql)?;
                if !finalized {
                    return Err(refuse(R::MissingEvidence));
                }
            }
            let next = h
                .expected_revision
                .checked_add(1)
                .ok_or_else(|| refuse(R::InvalidShape))?;
            let disposition = match h.disposition {
                WorkspaceHandoffDispositionV1::Pending => "PENDING",
                WorkspaceHandoffDispositionV1::Eligible => "ELIGIBLE",
                WorkspaceHandoffDispositionV1::ClosedWithoutHandoff => "CLOSED_WITHOUT_HANDOFF",
            };
            conn.execute(
                "INSERT INTO workspace_recovery_handoff VALUES(?1,?2,?3,?4,?5)",
                params![
                    h.subject.project_id,
                    h.subject.attempt_id,
                    next,
                    disposition,
                    json
                ],
            )
            .map_err(sql)?;
            Ok(next)
        })
    }
    pub(crate) fn claim_workspace_recovery_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        q: &WorkspaceRecoveryClaimRequestV1,
    ) -> Result<WorkspaceRecoveryRecordV1, StateError> {
        q.subject.validate()?;
        ids(&[&q.recovery_id])?;
        hex(&q.admission_sha256, 64)?;
        let json = encode(q)?;
        let (sample, now) = trusted_time::validate_sample(clock.sample()?)?;
        graph::write_transaction(self.connection(), |conn| {
            trusted_time::accept_sample(conn, &q.subject.project_id, &sample, &now)?;
            let a = admission(conn, &q.subject)?;
            if digest(&encode(&a)?) != q.admission_sha256 {
                return Err(refuse(R::ForeignEvidence));
            }
            let old:Option<(String,String)>=conn.query_row("SELECT recovery_id,record_json FROM workspace_recovery_control WHERE project_id=?1 AND attempt_id=?2",params![q.subject.project_id,q.subject.attempt_id],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql)?;
            if let Some((id, old)) = old {
                if id != q.recovery_id {
                    return Err(refuse(R::AlreadyResolved));
                }
                if old != json {
                    return Err(refuse(R::ConflictingReplay));
                }
                return record(conn, &q.subject, &id);
            }
            closed(conn, &q.subject)?;
            current(conn, &a, true, &now)?;
            guard_workspace(conn, &q.subject.project_id, &a.workspace_id)?;
            guard_binding(conn, &a.binding_id)?;
            conn.execute(
                "INSERT INTO workspace_recovery_control VALUES(?1,?2,?3,?4,?5,0,'CLAIMED',1,?6,?7)",
                params![
                    q.subject.project_id,
                    q.subject.attempt_id,
                    q.recovery_id,
                    a.workspace_id,
                    a.binding_id,
                    json,
                    sample.canonical_utc_timestamp
                ],
            )
            .map_err(sql)?;
            record(conn, &q.subject, &q.recovery_id)
        })
    }
    pub(crate) fn acknowledge_workspace_recovery_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        auth: &WorkspaceRecoveryAuthorityV1,
        stage: WorkspaceRecoveryEvidenceStageV1,
        e: &WorkspaceRecoveryEvidenceV1,
    ) -> Result<WorkspaceRecoveryRecordV1, StateError> {
        let (sample, now) = trusted_time::validate_sample(clock.sample()?)?;
        graph::write_transaction(self.connection(), |conn| {
            trusted_time::accept_sample(conn, &auth.subject.project_id, &sample, &now)?;
            let (r, a) = authorized(conn, auth, &now)?;
            evidence(conn, &a, e, &now)?;
            let phase = match stage {
                WorkspaceRecoveryEvidenceStageV1::Captured
                    if matches!(r.phase.as_str(), "CLAIMED" | "SIGNAL_PREPARED") =>
                {
                    "CAPTURED"
                }
                WorkspaceRecoveryEvidenceStageV1::Quiescent
                    if matches!(
                        r.phase.as_str(),
                        "CAPTURED" | "SIGNAL_PREPARED" | "RESET_PREPARED"
                    ) =>
                {
                    "QUIESCENT"
                }
                _ => return Err(refuse(R::InvalidPhase)),
            };
            advance(conn, &r, phase, &encode(&(auth, stage, e))?, true)
        })
    }
    pub(crate) fn begin_workspace_recovery_action_v1<'a>(
        &'a mut self,
        clock: &dyn TrustedClockV1,
        auth: &WorkspaceRecoveryAuthorityV1,
        kind: WorkspaceRecoveryActionKindV1,
    ) -> Result<WorkspaceRecoveryActionGuardV1<'a>, StateError> {
        let (sample, now) = trusted_time::validate_sample(clock.sample()?)?;
        let r = graph::write_transaction(self.connection(), |conn| {
            trusted_time::accept_sample(conn, &auth.subject.project_id, &sample, &now)?;
            let (r, _) = authorized(conn, auth, &now)?;
            let phase = match kind {
                WorkspaceRecoveryActionKindV1::Signal if r.phase == "CAPTURED" => "SIGNAL_PREPARED",
                WorkspaceRecoveryActionKindV1::Reset if r.phase == "QUIESCENT" => "RESET_PREPARED",
                _ => return Err(refuse(R::InvalidPhase)),
            };
            advance(conn, &r, phase, &encode(&(auth, kind))?, true)
        })?;
        let mut authority = auth.clone();
        authority.expected_revision = r.revision;
        Ok(WorkspaceRecoveryActionGuardV1 {
            repo: self,
            authority,
            kind,
        })
    }
    pub(crate) fn resolve_workspace_recovery_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        auth: &WorkspaceRecoveryAuthorityV1,
        observation: &WorkspaceRecoveryResetObservationV1,
    ) -> Result<WorkspaceRecoveryRecordV1, StateError> {
        let (sample, now) = trusted_time::validate_sample(clock.sample()?)?;
        let json = encode(&(auth, observation))?;
        graph::write_transaction(self.connection(), |conn| {
            trusted_time::accept_sample(conn, &auth.subject.project_id, &sample, &now)?;
            let old:Option<String>=conn.query_row("SELECT record_json FROM workspace_recovery_resolution WHERE project_id=?1 AND attempt_id=?2 AND recovery_id=?3",params![auth.subject.project_id,auth.subject.attempt_id,auth.recovery_id],|r|r.get(0)).optional().map_err(sql)?;
            if let Some(old) = old {
                if old != json {
                    return Err(refuse(R::ConflictingReplay));
                }
                return record(conn, &auth.subject, &auth.recovery_id);
            }
            let (r, a) = authorized(conn, auth, &now)?;
            if r.phase != "RESET_PREPARED" {
                return Err(refuse(R::InvalidPhase));
            }
            let target = anchor(conn, &a)?;
            let cp = evidence(conn, &a, &observation.evidence, &now)?;
            if observation.observed_commit != target.accepted_commit
                || observation.index_tree != target.accepted_tree
                || observation.tracked_tree != target.accepted_tree
                || cp["head_sha"].as_str() != Some(&target.accepted_commit)
                || !cp["modified_files"]
                    .as_array()
                    .is_some_and(|p| p.is_empty())
            {
                return Err(refuse(R::UnverifiedReset));
            }
            conn.execute(
                "INSERT INTO workspace_recovery_resolution VALUES(?1,?2,?3,?4)",
                params![
                    auth.subject.project_id,
                    auth.subject.attempt_id,
                    auth.recovery_id,
                    json
                ],
            )
            .map_err(sql)?;
            crate::write_fault::boundary()?;
            advance(conn, &r, "RESOLVED", &json, false)
        })
    }
    pub fn find_workspace_recovery_v1(
        &self,
        subject: &WorkspaceRecoverySubjectV1,
        recovery_id: &str,
    ) -> Result<WorkspaceRecoveryRecordV1, StateError> {
        crate::graph_history::read_transaction(self.connection(), |conn| {
            record(conn, subject, recovery_id)
        })
    }
}

// Shared guards run inside the writer's existing transaction. Historical schema
// checkers count triggers on old tables, so new SQL guards protect D4 records;
// all reachable legacy writer paths use these typed preflights.
// Only historical migration fixtures need writers before D4 tables exist.
#[cfg(test)]
fn historical_writer(conn: &Connection) -> Result<bool, StateError> {
    conn.query_row(
        "SELECT MAX(version)<27 FROM state_schema_version",
        [],
        |r| r.get(0),
    )
    .map_err(sql)
}
#[cfg(not(test))]
fn historical_writer(_: &Connection) -> Result<bool, StateError> {
    Ok(false)
}
pub(crate) fn guard_project(conn: &Connection, project: &str) -> Result<(), StateError> {
    if historical_writer(conn)? {
        return Ok(());
    }

    let held:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM workspace_recovery_control WHERE project_id=?1 AND active=1)",[project],|r|r.get(0)).map_err(sql)?;
    if held {
        Err(refuse(R::ActiveRecovery))
    } else {
        Ok(())
    }
}
pub(crate) fn guard_role(conn: &Connection, role: &str) -> Result<(), StateError> {
    if historical_writer(conn)? {
        return Ok(());
    }

    let held:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM workspace_recovery_admission a JOIN workspace_recovery_control c USING(project_id,attempt_id) WHERE a.role_id=?1 AND c.active=1)",[role],|r|r.get(0)).map_err(sql)?;
    if held {
        Err(refuse(R::ActiveRecovery))
    } else {
        Ok(())
    }
}
pub(crate) fn guard_binding(conn: &Connection, binding: &str) -> Result<(), StateError> {
    if historical_writer(conn)? {
        return Ok(());
    }

    let held:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM workspace_recovery_control WHERE binding_id=?1 AND active=1)",[binding],|r|r.get(0)).map_err(sql)?;
    if held {
        Err(refuse(R::ActiveRecovery))
    } else {
        Ok(())
    }
}
pub(crate) fn guard_workspace(
    conn: &Connection,
    project: &str,
    workspace: &str,
) -> Result<(), StateError> {
    if historical_writer(conn)? {
        return Ok(());
    }

    let held:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM workspace_recovery_control WHERE project_id=?1 AND workspace_id=?2 AND active=1)",params![project,workspace],|r|r.get(0)).map_err(sql)?;
    if held {
        Err(refuse(R::ActiveRecovery))
    } else {
        Ok(())
    }
}
pub(crate) fn guard_attempt(conn: &Connection, attempt: &str) -> Result<(), StateError> {
    if historical_writer(conn)? {
        return Ok(());
    }

    let held: Option<bool> = conn
        .query_row(
            "SELECT active FROM workspace_recovery_control WHERE attempt_id=?1",
            [attempt],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    match held {
        Some(true) => Err(refuse(R::ActiveRecovery)),
        Some(false) => Err(refuse(R::AlreadyResolved)),
        None => Ok(()),
    }
}
pub(crate) fn guard_start(
    conn: &Connection,
    project: &str,
    workspace: &str,
    start_sha: &str,
    graph_id: &str,
    node_id: &str,
) -> Result<(), StateError> {
    if historical_writer(conn)? {
        return Ok(());
    }

    let node_held:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM workspace_recovery_admission a JOIN workspace_recovery_control c USING(project_id,attempt_id) JOIN attempt_started s ON s.attempt_id=a.attempt_id WHERE a.project_id=?1 AND s.graph_id=?2 AND a.task_id=?3 AND c.active=1)",params![project,graph_id,node_id],|r|r.get(0)).map_err(sql)?;
    if node_held {
        return Err(refuse(R::ActiveRecovery));
    }
    guard_workspace(conn, project, workspace)?;
    let previous:Option<String>=conn.query_row("SELECT a.record_json FROM workspace_recovery_admission a JOIN workspace_recovery_control c USING(project_id,attempt_id) WHERE a.project_id=?1 AND a.workspace_id=?2 AND c.active=0 ORDER BY c.revision DESC,c.recovery_id LIMIT 1",params![project,workspace],|r|r.get(0)).optional().map_err(sql)?;
    if let Some(json) = previous {
        let a: WorkspaceRecoveryAdmissionV1 = decode(&json)?;
        let t = anchor(conn, &a)?;
        if start_sha != t.accepted_commit || graph_id != a.subject.graph_id {
            return Err(refuse(R::ForeignEvidence));
        }
    }
    Ok(())
}
pub(crate) fn guard_review(
    conn: &Connection,
    project: &str,
    task: &str,
    subject: Option<&str>,
) -> Result<(), StateError> {
    if historical_writer(conn)? {
        return Ok(());
    }

    let mut query=conn.prepare("SELECT a.record_json FROM workspace_recovery_admission a WHERE a.project_id=?1 AND a.task_id=?2").map_err(sql)?;
    let admissions = query
        .query_map(params![project, task], |r| r.get::<_, String>(0))
        .map_err(sql)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(sql)?;
    if admissions.is_empty() {
        return Ok(());
    }
    let mut eligible = 0;
    for json in admissions {
        let a: WorkspaceRecoveryAdmissionV1 = decode(&json)?;
        match guard_attempt(conn, &a.subject.attempt_id) {
            Err(StateError::WorkspaceRecoveryRefused {
                reason: R::AlreadyResolved,
            }) => continue,
            other => other?,
        }
        let disposition = handoff(conn, &a.subject)?;
        if !disposition
            .as_ref()
            .is_some_and(|(_, s, _)| s == "ELIGIBLE")
        {
            return Err(refuse(R::HandoffUnavailable));
        }
        if disposition.is_some_and(|(_, s, _)| s == "ELIGIBLE") {
            let attempt = crate::attempt::read_attempt_in_tx(conn, &a.subject.validate()?)?
                .ok_or_else(|| refuse(R::MissingEvidence))?;
            if attempt.end_sha() == subject {
                eligible += 1;
            }
        }
    }
    if eligible == 1 {
        Ok(())
    } else {
        Err(refuse(R::HandoffUnavailable))
    }
}
pub(crate) fn guard_decision(
    conn: &Connection,
    v: &crate::DecisionRecordV1,
) -> Result<(), StateError> {
    if historical_writer(conn)? {
        return Ok(());
    }

    match v.kind() {
        DecisionRecordKind::ReviewRequest | DecisionRecordKind::A4Review => {
            if let Some(task) = v.task_id() {
                guard_review(conn, v.project_id(), task, v.subject_sha())?;
            }
        }
        DecisionRecordKind::IntegrationDecision => {
            let value: serde_json::Value = decode(v.record_json())?;
            if let Some(chain) = value["provenance_chain"]
                .as_array()
                .filter(|c| !c.is_empty())
            {
                for p in chain {
                    if let Some(task) = p["task_id"].as_str() {
                        guard_review(conn, v.project_id(), task, p["implementation_sha"].as_str())?;
                    }
                }
            } else {
                let participating:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM workspace_recovery_admission WHERE project_id=?1)",[v.project_id()],|r|r.get(0)).map_err(sql)?;
                if participating {
                    return Err(refuse(R::MissingEvidence));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn stale_lease() -> StateError {
    refuse(R::StaleAuthority)
}
