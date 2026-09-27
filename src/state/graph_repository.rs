//! State-owned graph genesis and lossless current-version projection.
use std::collections::HashSet;
use std::fmt;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::canonical_timestamp::CanonicalTimestampV1;
use crate::epoch_value::StateEpochValueV1;
use crate::error::{
    GraphFailureCode as Code, GraphField as Field, GraphPhase as Phase, StateError,
};
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::trusted_time::{self, TrustedClockV1, TrustedTimeSampleV1};

fn failure(phase: Phase, code: Code, field: Option<Field>) -> StateError {
    StateError::GraphPersistence { phase, code, field }
}
fn invalid(field: Field) -> StateError {
    failure(Phase::Input, Code::InvalidShape, Some(field))
}
fn corrupt() -> StateError {
    failure(Phase::Read, Code::CorruptStore, None)
}

pub(crate) fn sqlite_error(phase: Phase, error: rusqlite::Error) -> StateError {
    let code = match error {
        rusqlite::Error::SqliteFailure(ref cause, _) => match cause.code {
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => {
                Code::StoreBusy
            }
            rusqlite::ErrorCode::CannotOpen
            | rusqlite::ErrorCode::PermissionDenied
            | rusqlite::ErrorCode::ReadOnly => Code::StoreUnavailable,
            rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase => {
                Code::CorruptStore
            }
            _ => Code::StorageFailure,
        },
        rusqlite::Error::InvalidColumnType(..)
        | rusqlite::Error::FromSqlConversionFailure(..)
        | rusqlite::Error::Utf8Error(..) => Code::CorruptStore,
        _ => Code::StorageFailure,
    };
    failure(phase, code, None)
}

fn schema_error(phase: Phase, error: rusqlite::Error) -> StateError {
    if matches!(&error, rusqlite::Error::SqliteFailure(cause, _)
        if cause.code == rusqlite::ErrorCode::Unknown)
    {
        failure(phase, Code::CorruptStore, None)
    } else {
        sqlite_error(phase, error)
    }
}

pub(crate) fn legacy_error(phase: Phase, error: StateError) -> StateError {
    let code =
        match error {
            StateError::GraphPersistence { code, .. } => code,
            StateError::TrustedClockRegression { .. }
            | StateError::TrustedTimeSampleStale { .. } => Code::ClockRegression,
            StateError::TrustedClockContinuityUnbound { .. } => Code::ClockContinuity,
            StateError::TrustedClockSampleInvalid { .. }
            | StateError::CanonicalTimestampInvalid { .. } => Code::ClockInvalid,
            StateError::TrustedTimeWatermarkDecodeFailed { .. } => Code::CorruptStore,
            StateError::SchemaVersionMismatch { .. }
            | StateError::ExplicitMigrationRefused { .. } => Code::UnsupportedSchema,
            StateError::MigrationLedgerCorrupt { .. }
            | StateError::SchemaVersionReadFailed { .. } => Code::CorruptStore,
            StateError::TransactionRollbackFailed { .. } => Code::RollbackFailure,
            _ => Code::StorageFailure,
        };
    failure(phase, code, None)
}

fn id(value: &str) -> bool {
    let n = value.chars().count();
    (1..=200).contains(&n)
}
fn positive(value: &str) -> bool {
    let b = value.as_bytes();
    b.first().is_some_and(|x| (b'1'..=b'9').contains(x)) && b[1..].iter().all(u8::is_ascii_digit)
}
fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn capability(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|part| {
            let b = part.as_bytes();
            b.first().is_some_and(|x| x.is_ascii_lowercase())
                && b[1..]
                    .iter()
                    .all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || *x == b'_')
        })
}
fn provenance(value: &str, prefix: &str) -> bool {
    let Some(ulid) = value.strip_prefix(prefix) else {
        return false;
    };
    let b = ulid.as_bytes();
    b.len() == 26
        && (b'0'..=b'7').contains(&b[0])
        && b.iter()
            .all(|x| b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(x))
}

fn accept_graph_sample(
    conn: &Connection,
    project_id: &str,
    sample: &TrustedTimeSampleV1,
    timestamp: &CanonicalTimestampV1,
) -> Result<(), StateError> {
    let current: Option<(String, String, String)> = conn.query_row(
        "SELECT clock_source_id,clock_contract_version,last_accepted_trusted_time FROM trusted_time_watermark WHERE project_id=?1",
        [project_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))
        .optional().map_err(|e| sqlite_error(Phase::Clock,e))?;
    match current {
        None => {
            conn.execute("INSERT INTO trusted_time_watermark(project_id,clock_source_id,clock_contract_version,last_accepted_trusted_time) VALUES (?1,?2,?3,?4)",
                params![project_id,sample.clock_source_id,sample.clock_contract_version,sample.canonical_utc_timestamp])
                .map_err(|e| sqlite_error(Phase::Clock,e))?;
        }
        Some((source, contract, previous)) => {
            if source.is_empty() || contract.is_empty() {
                return Err(failure(Phase::Clock, Code::CorruptStore, None));
            }
            let previous_time = CanonicalTimestampV1::parse(&previous)
                .map_err(|_| failure(Phase::Clock, Code::CorruptStore, None))?;
            if source != sample.clock_source_id || contract != sample.clock_contract_version {
                return Err(failure(Phase::Clock, Code::ClockContinuity, None));
            }
            if timestamp < &previous_time {
                return Err(failure(Phase::Clock, Code::ClockRegression, None));
            }
            if timestamp > &previous_time {
                let changed=conn.execute("UPDATE trusted_time_watermark SET last_accepted_trusted_time=?1 WHERE project_id=?2 AND last_accepted_trusted_time=?3",
                    params![sample.canonical_utc_timestamp,project_id,previous])
                    .map_err(|e| sqlite_error(Phase::Clock,e))?;
                if changed != 1 {
                    return Err(failure(Phase::Clock, Code::CorruptStore, None));
                }
            }
        }
    }
    Ok(())
}

#[derive(Clone, PartialEq, Eq)]
pub struct GraphCompilerRefV1(String);
impl GraphCompilerRefV1 {
    pub fn parse(value: impl Into<String>) -> Result<Self, StateError> {
        let value = value.into();
        if !provenance(&value, "compiler:") {
            return Err(failure(
                Phase::Input,
                Code::InvalidProvenance,
                Some(Field::CompilerId),
            ));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for GraphCompilerRefV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GraphCompilerRefV1")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct GraphSourceEvidenceRefV1(String);
impl GraphSourceEvidenceRefV1 {
    pub fn parse(value: impl Into<String>) -> Result<Self, StateError> {
        let value = value.into();
        if !provenance(&value, "evidence:") {
            return Err(failure(
                Phase::Input,
                Code::InvalidProvenance,
                Some(Field::SourceRef),
            ));
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl fmt::Debug for GraphSourceEvidenceRefV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GraphSourceEvidenceRefV1")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GraphGenesisReasonV1 {
    InitialCompilation,
}
impl GraphGenesisReasonV1 {
    pub fn parse(value: &str) -> Result<Self, StateError> {
        if value == "INITIAL_COMPILATION" {
            Ok(Self::InitialCompilation)
        } else {
            Err(failure(
                Phase::Input,
                Code::InvalidProvenance,
                Some(Field::CreationReason),
            ))
        }
    }
    pub fn as_str(self) -> &'static str {
        "INITIAL_COMPILATION"
    }
}
impl fmt::Debug for GraphGenesisReasonV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("GraphGenesisReasonV1")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StateGraphGenesisProvenanceV1 {
    compiler_id: GraphCompilerRefV1,
    source_ref: GraphSourceEvidenceRefV1,
    creation_reason: GraphGenesisReasonV1,
}
impl StateGraphGenesisProvenanceV1 {
    pub fn new(
        compiler_id: GraphCompilerRefV1,
        source_ref: GraphSourceEvidenceRefV1,
        creation_reason: GraphGenesisReasonV1,
    ) -> Self {
        Self {
            compiler_id,
            source_ref,
            creation_reason,
        }
    }
    pub fn compiler_id(&self) -> &GraphCompilerRefV1 {
        &self.compiler_id
    }
    pub fn source_ref(&self) -> &GraphSourceEvidenceRefV1 {
        &self.source_ref
    }
    pub fn creation_reason(&self) -> GraphGenesisReasonV1 {
        self.creation_reason
    }
}
impl fmt::Debug for StateGraphGenesisProvenanceV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StateGraphGenesisProvenanceV1")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StateGraphNodeV1 {
    node_id: String,
    graph_id: String,
    kind: String,
    state: String,
    title: Option<String>,
    parent_node_id: Option<String>,
    attempt_number: Option<String>,
    required_capabilities: Option<Vec<String>>,
    task_capsule_ref: Option<String>,
    workstream_id: Option<String>,
    code_sha: Option<String>,
    workspace_id: Option<String>,
    result_ref: Option<String>,
    locked_reason: Option<Option<String>>,
    created_in_version: Option<String>,
}
impl StateGraphNodeV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        node_id: String,
        graph_id: String,
        kind: String,
        state: String,
        title: Option<String>,
        parent_node_id: Option<String>,
        attempt_number: Option<String>,
        required_capabilities: Option<Vec<String>>,
        task_capsule_ref: Option<String>,
        workstream_id: Option<String>,
        code_sha: Option<String>,
        workspace_id: Option<String>,
        result_ref: Option<String>,
        locked_reason: Option<Option<String>>,
        created_in_version: Option<String>,
    ) -> Result<Self, StateError> {
        let value = Self {
            node_id,
            graph_id,
            kind,
            state,
            title,
            parent_node_id,
            attempt_number,
            required_capabilities,
            task_capsule_ref,
            workstream_id,
            code_sha,
            workspace_id,
            result_ref,
            locked_reason,
            created_in_version,
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), StateError> {
        if !id(&self.node_id) {
            return Err(invalid(Field::NodeId));
        }
        if !id(&self.graph_id) {
            return Err(invalid(Field::GraphId));
        }
        if self.kind.is_empty() {
            return Err(invalid(Field::Kind));
        }
        if !matches!(
            self.state.as_str(),
            "PLANNED"
                | "READY"
                | "ADMITTED"
                | "DISPATCHED"
                | "RUNNING"
                | "AWAITING_REVIEW"
                | "PASSED"
                | "REJECTED"
                | "REPAIRING"
                | "ACCEPTED"
                | "INTEGRATED"
                | "BLOCKED"
                | "LOCKED_REQUIRES_PRO"
                | "CANCELLED"
                | "HUMAN_REQUIRED"
        ) {
            return Err(invalid(Field::State));
        }
        if self.parent_node_id.as_deref().is_some_and(|x| !id(x)) {
            return Err(invalid(Field::ParentNodeId));
        }
        if self.workstream_id.as_deref().is_some_and(|x| !id(x)) {
            return Err(invalid(Field::WorkstreamId));
        }
        if self.workspace_id.as_deref().is_some_and(|x| !id(x)) {
            return Err(invalid(Field::WorkspaceId));
        }
        if self.attempt_number.as_deref().is_some_and(|x| !positive(x)) {
            return Err(invalid(Field::AttemptNumber));
        }
        if self
            .created_in_version
            .as_deref()
            .is_some_and(|x| !positive(x))
        {
            return Err(invalid(Field::CreatedInVersion));
        }
        if self.code_sha.as_deref().is_some_and(|x| !hex(x, 40)) {
            return Err(invalid(Field::CodeSha));
        }
        if self
            .required_capabilities
            .as_ref()
            .is_some_and(|xs| xs.iter().any(|x| !capability(x)))
        {
            return Err(invalid(Field::RequiredCapabilities));
        }
        Ok(())
    }
    pub fn node_id(&self) -> &str {
        &self.node_id
    }
    pub fn graph_id(&self) -> &str {
        &self.graph_id
    }
    pub fn kind(&self) -> &str {
        &self.kind
    }
    pub fn state(&self) -> &str {
        &self.state
    }
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }
    pub fn parent_node_id(&self) -> Option<&str> {
        self.parent_node_id.as_deref()
    }
    pub fn attempt_number(&self) -> Option<&str> {
        self.attempt_number.as_deref()
    }
    pub fn required_capabilities(&self) -> Option<&[String]> {
        self.required_capabilities.as_deref()
    }
    pub fn task_capsule_ref(&self) -> Option<&str> {
        self.task_capsule_ref.as_deref()
    }
    pub fn workstream_id(&self) -> Option<&str> {
        self.workstream_id.as_deref()
    }
    pub fn code_sha(&self) -> Option<&str> {
        self.code_sha.as_deref()
    }
    pub fn workspace_id(&self) -> Option<&str> {
        self.workspace_id.as_deref()
    }
    pub fn result_ref(&self) -> Option<&str> {
        self.result_ref.as_deref()
    }
    pub fn locked_reason(&self) -> Option<Option<&str>> {
        self.locked_reason.as_ref().map(|x| x.as_deref())
    }
    pub fn created_in_version(&self) -> Option<&str> {
        self.created_in_version.as_deref()
    }
}
impl fmt::Debug for StateGraphNodeV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StateGraphNodeV1")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum StateGraphEdgeRelationV1 {
    Precedence(String),
    Control(String),
}
impl StateGraphEdgeRelationV1 {
    fn validate(&self) -> Result<(), StateError> {
        match self {
            Self::Precedence(x)
                if matches!(
                    x.as_str(),
                    "REQUIRES_ACCEPTED" | "REQUIRES_INTEGRATED" | "REQUIRES_INTERFACE"
                ) =>
            {
                Ok(())
            }
            Self::Control(x)
                if matches!(
                    x.as_str(),
                    "ON_PASS"
                        | "ON_REJECT"
                        | "ON_FAILURE"
                        | "ON_BLOCKED"
                        | "ESCALATE"
                        | "EXPANDS_INTO"
                ) =>
            {
                Ok(())
            }
            Self::Precedence(_) => Err(invalid(Field::PrecedenceKind)),
            Self::Control(_) => Err(invalid(Field::ControlKind)),
        }
    }
    pub fn edge_class(&self) -> &'static str {
        match self {
            Self::Precedence(_) => "PRECEDENCE",
            Self::Control(_) => "CONTROL",
        }
    }
    pub fn precedence_kind(&self) -> Option<&str> {
        match self {
            Self::Precedence(x) => Some(x),
            _ => None,
        }
    }
    pub fn control_kind(&self) -> Option<&str> {
        match self {
            Self::Control(x) => Some(x),
            _ => None,
        }
    }
}
impl fmt::Debug for StateGraphEdgeRelationV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StateGraphEdgeRelationV1")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StateGraphEdgeV1 {
    edge_id: String,
    graph_id: String,
    from_node: String,
    to_node: String,
    relation: StateGraphEdgeRelationV1,
    note: Option<String>,
}
impl StateGraphEdgeV1 {
    pub fn new(
        edge_id: String,
        graph_id: String,
        from_node: String,
        to_node: String,
        relation: StateGraphEdgeRelationV1,
        note: Option<String>,
    ) -> Result<Self, StateError> {
        let value = Self {
            edge_id,
            graph_id,
            from_node,
            to_node,
            relation,
            note,
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), StateError> {
        if !id(&self.edge_id) {
            return Err(invalid(Field::EdgeId));
        }
        if !id(&self.graph_id) {
            return Err(invalid(Field::GraphId));
        }
        if !id(&self.from_node) {
            return Err(invalid(Field::FromNode));
        }
        if !id(&self.to_node) {
            return Err(invalid(Field::ToNode));
        }
        self.relation.validate()
    }
    pub fn edge_id(&self) -> &str {
        &self.edge_id
    }
    pub fn graph_id(&self) -> &str {
        &self.graph_id
    }
    pub fn from_node(&self) -> &str {
        &self.from_node
    }
    pub fn to_node(&self) -> &str {
        &self.to_node
    }
    pub fn relation(&self) -> &StateGraphEdgeRelationV1 {
        &self.relation
    }
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}
impl fmt::Debug for StateGraphEdgeV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StateGraphEdgeV1")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StateGraphGenesisV1 {
    project_id: String,
    graph_id: String,
    goal_id: String,
    policy_id: String,
    parent_version: Option<Option<String>>,
    compiled_from: Option<Vec<String>>,
    context_epoch: Option<StateEpochValueV1>,
    resulting_digest: String,
    nodes: Vec<StateGraphNodeV1>,
    edges: Vec<StateGraphEdgeV1>,
    genesis_provenance: StateGraphGenesisProvenanceV1,
}
impl StateGraphGenesisV1 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: String,
        graph_id: String,
        goal_id: String,
        policy_id: String,
        parent_version: Option<Option<String>>,
        compiled_from: Option<Vec<String>>,
        context_epoch: Option<StateEpochValueV1>,
        resulting_digest: String,
        nodes: Vec<StateGraphNodeV1>,
        edges: Vec<StateGraphEdgeV1>,
        genesis_provenance: StateGraphGenesisProvenanceV1,
    ) -> Result<Self, StateError> {
        let value = Self {
            project_id,
            graph_id,
            goal_id,
            policy_id,
            parent_version,
            compiled_from,
            context_epoch,
            resulting_digest,
            nodes,
            edges,
            genesis_provenance,
        };
        value.validate()?;
        Ok(value)
    }
    fn validate(&self) -> Result<(), StateError> {
        for (s, field) in [
            (&self.project_id, Field::ProjectId),
            (&self.graph_id, Field::GraphId),
            (&self.goal_id, Field::GoalId),
            (&self.policy_id, Field::PolicyId),
        ] {
            if !id(s) {
                return Err(invalid(field));
            }
        }
        if self.parent_version.as_ref().is_some_and(Option::is_some) {
            return Err(invalid(Field::ParentVersion));
        }
        if !hex(&self.resulting_digest, 64) {
            return Err(invalid(Field::ResultingDigest));
        }
        if self.nodes.is_empty() {
            return Err(invalid(Field::Nodes));
        }
        let mut nodes = HashSet::new();
        for node in &self.nodes {
            node.validate()?;
            if node.graph_id != self.graph_id {
                return Err(failure(
                    Phase::Input,
                    Code::IdentityMismatch,
                    Some(Field::GraphId),
                ));
            }
            if !nodes.insert(node.node_id.as_str()) {
                return Err(invalid(Field::NodeId));
            }
        }
        let mut edges = HashSet::new();
        for edge in &self.edges {
            edge.validate()?;
            if edge.graph_id != self.graph_id {
                return Err(failure(
                    Phase::Input,
                    Code::IdentityMismatch,
                    Some(Field::GraphId),
                ));
            }
            if !edges.insert(edge.edge_id.as_str()) {
                return Err(invalid(Field::EdgeId));
            }
            if !nodes.contains(edge.from_node.as_str()) {
                return Err(invalid(Field::FromNode));
            }
            if !nodes.contains(edge.to_node.as_str()) {
                return Err(invalid(Field::ToNode));
            }
        }
        Ok(())
    }
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn graph_id(&self) -> &str {
        &self.graph_id
    }
    pub fn goal_id(&self) -> &str {
        &self.goal_id
    }
    pub fn policy_id(&self) -> &str {
        &self.policy_id
    }
    pub fn parent_version(&self) -> Option<Option<&str>> {
        self.parent_version.as_ref().map(|x| x.as_deref())
    }
    pub fn compiled_from(&self) -> Option<&[String]> {
        self.compiled_from.as_deref()
    }
    pub fn context_epoch(&self) -> Option<&StateEpochValueV1> {
        self.context_epoch.as_ref()
    }
    pub fn resulting_digest(&self) -> &str {
        &self.resulting_digest
    }
    pub fn nodes(&self) -> &[StateGraphNodeV1] {
        &self.nodes
    }
    pub fn edges(&self) -> &[StateGraphEdgeV1] {
        &self.edges
    }
    pub fn genesis_provenance(&self) -> &StateGraphGenesisProvenanceV1 {
        &self.genesis_provenance
    }
}
impl fmt::Debug for StateGraphGenesisV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StateGraphGenesisV1")
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct PersistedCurrentGraphV1 {
    project_id: String,
    graph_id: String,
    current_version: String,
    graph_version: String,
    parent_version: Option<Option<String>>,
    goal_id: String,
    policy_id: String,
    created_at: CanonicalTimestampV1,
    compiled_from: Option<Vec<String>>,
    context_epoch: Option<StateEpochValueV1>,
    resulting_digest: String,
    nodes: Vec<StateGraphNodeV1>,
    edges: Vec<StateGraphEdgeV1>,
    genesis_provenance: StateGraphGenesisProvenanceV1,
    clock_source_id: String,
    clock_contract_version: String,
}
impl PersistedCurrentGraphV1 {
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn graph_id(&self) -> &str {
        &self.graph_id
    }
    pub fn current_version(&self) -> &str {
        &self.current_version
    }
    pub fn graph_version(&self) -> &str {
        &self.graph_version
    }
    pub fn parent_version(&self) -> Option<Option<&str>> {
        self.parent_version.as_ref().map(|x| x.as_deref())
    }
    pub fn goal_id(&self) -> &str {
        &self.goal_id
    }
    pub fn policy_id(&self) -> &str {
        &self.policy_id
    }
    pub fn created_at(&self) -> &CanonicalTimestampV1 {
        &self.created_at
    }
    pub fn compiled_from(&self) -> Option<&[String]> {
        self.compiled_from.as_deref()
    }
    pub fn context_epoch(&self) -> Option<&StateEpochValueV1> {
        self.context_epoch.as_ref()
    }
    pub fn resulting_digest(&self) -> &str {
        &self.resulting_digest
    }
    pub fn nodes(&self) -> &[StateGraphNodeV1] {
        &self.nodes
    }
    pub fn edges(&self) -> &[StateGraphEdgeV1] {
        &self.edges
    }
    pub fn genesis_provenance(&self) -> &StateGraphGenesisProvenanceV1 {
        &self.genesis_provenance
    }
    pub fn clock_source_id(&self) -> &str {
        &self.clock_source_id
    }
    pub fn clock_contract_version(&self) -> &str {
        &self.clock_contract_version
    }
}
impl fmt::Debug for PersistedCurrentGraphV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PersistedCurrentGraphV1")
    }
}

pub(crate) fn verify_ledger(
    conn: &Connection,
    head: usize,
    phase: Phase,
) -> Result<(), StateError> {
    let chain = migrations::registered();
    let mut stmt = conn
        .prepare("SELECT version, migration_name FROM state_schema_version ORDER BY version")
        .map_err(|e| schema_error(phase, e))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| schema_error(phase, e))?;
    let rows = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| schema_error(phase, e))?;
    if rows.len() != head
        || rows
            .iter()
            .zip(&chain[..head])
            .any(|((v, n), m)| *v != i64::from(m.version) || n != m.name)
    {
        return Err(failure(phase, Code::CorruptStore, None));
    }
    Ok(())
}

pub(crate) fn verify_schema(
    conn: &Connection,
    phase: Phase,
    graph: bool,
) -> Result<(), StateError> {
    let integrity: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(|e| schema_error(phase, e))?;
    if integrity != "ok" {
        return Err(failure(phase, Code::CorruptStore, None));
    }
    let violation: Option<i64> = conn
        .query_row("SELECT 1 FROM pragma_foreign_key_check LIMIT 1", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|e| schema_error(phase, e))?;
    if violation.is_some() {
        return Err(failure(phase, Code::CorruptStore, None));
    }
    for table in [
        "state_schema_version",
        "trusted_time_watermark",
        "logical_role",
        "logical_role_ownership_path",
        "executor_binding",
        "event",
        "context_manifest",
        "context_manifest_source",
        "context_manifest_source_required_for",
        "context_epoch",
        "context_epoch_invalidated_role",
        "context_epoch_changed_source",
        "context_rehydration_attempt",
        "context_rehydration_repository_snapshot",
        "context_rehydration_source_evidence",
    ] {
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .map_err(|e| schema_error(phase, e))?;
        if exists != 1 {
            return Err(failure(phase, Code::CorruptStore, None));
        }
    }
    for (table, column) in [
        ("logical_role", "current_context_epoch"),
        ("event", "epoch"),
        ("context_manifest", "epoch"),
        ("context_epoch", "epoch"),
        ("context_epoch_invalidated_role", "epoch"),
        ("context_epoch_changed_source", "epoch"),
        ("context_rehydration_attempt", "context_epoch_id"),
    ] {
        let declared: Option<String> = conn
            .query_row(
                "SELECT type FROM pragma_table_info(?1) WHERE name=?2",
                params![table, column],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| schema_error(phase, e))?;
        if declared.as_deref() != Some("TEXT") {
            return Err(failure(phase, Code::CorruptStore, None));
        }
        let definition: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .map_err(|e| schema_error(phase, e))?;
        for fragment in [
            format!("typeof({column}) = 'text'"),
            format!("length({column}) >= 1"),
            format!("{column} NOT GLOB '*[^0-9]*'"),
            format!("{column} = '0' OR substr({column}, 1, 1) BETWEEN '1' AND '9'"),
        ] {
            if !definition.contains(&fragment) {
                return Err(failure(phase, Code::CorruptStore, None));
            }
        }
    }
    let index: Option<String> = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type='index' AND name='idx_context_epoch_project_numeric_epoch'",
        [], |r| r.get(0)).optional().map_err(|e| sqlite_error(phase,e))?;
    if !index
        .as_deref()
        .is_some_and(|s| s.contains("(project_id, length(epoch) DESC, epoch COLLATE BINARY DESC)"))
    {
        return Err(failure(phase, Code::CorruptStore, None));
    }
    if graph {
        for statement in migrations::v0013_graph_repository::SQL
            .split(';')
            .filter(|s| s.trim_start().starts_with("CREATE TABLE"))
        {
            let name = statement
                .split_whitespace()
                .nth(2)
                .ok_or_else(|| failure(phase, Code::CorruptStore, None))?;
            let stored: Option<String> = conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                    [name],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| schema_error(phase, e))?;
            if stored.as_deref() != Some(statement.trim()) {
                return Err(failure(phase, Code::CorruptStore, None));
            }
        }
    }
    Ok(())
}

impl SqliteStateRepository {
    /// Atomically creates the first durable graph version and accepts its clock sample.
    pub fn create_initial_graph_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: StateGraphGenesisV1,
    ) -> Result<(), StateError> {
        request.validate()?;
        let sample = clock.sample().map_err(|e| legacy_error(Phase::Clock, e))?;
        let (sample, timestamp) =
            trusted_time::validate_sample(sample).map_err(|e| legacy_error(Phase::Clock, e))?;
        let conn = self.connection();
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| sqlite_error(Phase::Write, e))?;
        let outcome = (|| {
            verify_ledger(conn, 13, Phase::Write)?;
            verify_schema(conn, Phase::Write, true)?;
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM graphs WHERE project_id=?1 AND graph_id=?2",
                    params![request.project_id, request.graph_id],
                    |r| r.get(0),
                )
                .map_err(|e| sqlite_error(Phase::Write, e))?;
            if exists != 0 {
                return Err(failure(Phase::Write, Code::AlreadyExists, None));
            }
            accept_graph_sample(conn, &request.project_id, &sample, &timestamp)?;
            conn.execute(
                "INSERT INTO graphs(project_id,graph_id,current_version) VALUES (?1,?2,'1')",
                params![request.project_id, request.graph_id],
            )
            .map_err(|e| sqlite_error(Phase::Write, e))?;
            conn.execute("INSERT INTO graph_versions(project_id,graph_id,graph_version,parent_version_present,parent_version,goal_id,policy_id,created_at,compiled_from_present,context_epoch,resulting_digest,clock_source_id,clock_contract_version,compiler_id,source_ref,creation_reason) VALUES (?1,?2,'1',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                params![request.project_id, request.graph_id, i64::from(request.parent_version.is_some()), request.parent_version.as_ref().and_then(|x| x.as_deref()),
                    request.goal_id, request.policy_id, sample.canonical_utc_timestamp, i64::from(request.compiled_from.is_some()),
                    request.context_epoch.as_ref().map(StateEpochValueV1::as_str), request.resulting_digest, sample.clock_source_id,
                    sample.clock_contract_version, request.genesis_provenance.compiler_id.as_str(), request.genesis_provenance.source_ref.as_str(),
                    request.genesis_provenance.creation_reason.as_str()]).map_err(|e| sqlite_error(Phase::Write, e))?;
            if let Some(sources) = &request.compiled_from {
                for (index, source) in sources.iter().enumerate() {
                    let ordinal = i64::try_from(index).map_err(|_| {
                        failure(Phase::Input, Code::InvalidShape, Some(Field::CompiledFrom))
                    })?;
                    conn.execute("INSERT INTO graph_compiled_sources(project_id,graph_id,graph_version,ordinal,source) VALUES (?1,?2,'1',?3,?4)",
                        params![request.project_id, request.graph_id, ordinal, source]).map_err(|e| sqlite_error(Phase::Write, e))?;
                }
            }
            for node in &request.nodes {
                conn.execute("INSERT INTO graph_nodes(project_id,graph_id,graph_version,node_id,kind,state,title,parent_node_id,attempt_number,required_capabilities_present,task_capsule_ref,workstream_id,code_sha,workspace_id,result_ref,locked_reason_present,locked_reason,created_in_version) VALUES (?1,?2,'1',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)",
                    params![request.project_id, request.graph_id, node.node_id, node.kind, node.state, node.title, node.parent_node_id,
                        node.attempt_number, i64::from(node.required_capabilities.is_some()), node.task_capsule_ref, node.workstream_id,
                        node.code_sha, node.workspace_id, node.result_ref, i64::from(node.locked_reason.is_some()),
                        node.locked_reason.as_ref().and_then(|x| x.as_deref()), node.created_in_version]).map_err(|e| sqlite_error(Phase::Write, e))?;
                if let Some(capabilities) = &node.required_capabilities {
                    for (index, capability) in capabilities.iter().enumerate() {
                        let ordinal = i64::try_from(index).map_err(|_| {
                            failure(
                                Phase::Input,
                                Code::InvalidShape,
                                Some(Field::RequiredCapabilities),
                            )
                        })?;
                        conn.execute("INSERT INTO graph_node_capabilities(project_id,graph_id,graph_version,node_id,ordinal,capability) VALUES (?1,?2,'1',?3,?4,?5)",
                            params![request.project_id, request.graph_id, node.node_id, ordinal, capability]).map_err(|e| sqlite_error(Phase::Write, e))?;
                    }
                }
            }
            for edge in &request.edges {
                conn.execute("INSERT INTO graph_edges(project_id,graph_id,graph_version,edge_id,from_node,to_node,edge_class,precedence_kind,control_kind,note) VALUES (?1,?2,'1',?3,?4,?5,?6,?7,?8,?9)",
                    params![request.project_id, request.graph_id, edge.edge_id, edge.from_node, edge.to_node,
                        edge.relation.edge_class(), edge.relation.precedence_kind(), edge.relation.control_kind(), edge.note])
                    .map_err(|e| sqlite_error(Phase::Write, e))?;
            }
            Ok(())
        })();
        match outcome {
            Ok(()) => match conn.execute_batch("COMMIT") {
                Ok(()) => Ok(()),
                Err(e) => {
                    let mapped = sqlite_error(Phase::Write, e);
                    conn.execute_batch("ROLLBACK")
                        .map_err(|_| failure(Phase::Write, Code::RollbackFailure, None))?;
                    Err(mapped)
                }
            },
            Err(e) => {
                conn.execute_batch("ROLLBACK")
                    .map_err(|_| failure(Phase::Write, Code::RollbackFailure, None))?;
                Err(e)
            }
        }
    }
}

/// Existing-file, read-only graph projection. The caller authorizes path and project.
pub struct ReadOnlyGraphReader {
    conn: Connection,
}
impl fmt::Debug for ReadOnlyGraphReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReadOnlyGraphReader")
    }
}
impl ReadOnlyGraphReader {
    pub fn open_existing(path: impl AsRef<Path>) -> Result<Self, StateError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| sqlite_error(Phase::Read, e))?;
        conn.busy_timeout(Duration::from_millis(5000))
            .map_err(|e| sqlite_error(Phase::Read, e))?;
        conn.execute_batch("PRAGMA foreign_keys=ON")
            .map_err(|e| sqlite_error(Phase::Read, e))?;
        let reader = Self { conn };
        reader.verify()?;
        Ok(reader)
    }
    fn verify(&self) -> Result<(), StateError> {
        let exists: i64 = self.conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='state_schema_version'", [], |r| r.get(0))
            .map_err(|e| sqlite_error(Phase::Read, e))?;
        if exists != 1 {
            return Err(failure(Phase::Read, Code::UnsupportedSchema, None));
        }
        let head: Option<i64> = self
            .conn
            .query_row("SELECT MAX(version) FROM state_schema_version", [], |r| {
                r.get(0)
            })
            .map_err(|e| sqlite_error(Phase::Read, e))?;
        if head.is_some_and(|version| version < 0) {
            return Err(corrupt());
        }
        if head != Some(13) {
            return Err(failure(Phase::Read, Code::UnsupportedSchema, None));
        }
        verify_ledger(&self.conn, 13, Phase::Read)?;
        verify_schema(&self.conn, Phase::Read, true)
    }
    pub fn read_current_v1(
        &self,
        project_id: &str,
        graph_id: &str,
    ) -> Result<Option<PersistedCurrentGraphV1>, StateError> {
        if !id(project_id) {
            return Err(invalid(Field::ProjectId));
        }
        if !id(graph_id) {
            return Err(invalid(Field::GraphId));
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| sqlite_error(Phase::Read, e))?;
        let outcome = (|| {
            // Verify before the pointer query while the same snapshot remains active.
            verify_ledger(&tx, 13, Phase::Read)?;
            verify_schema(&tx, Phase::Read, true)?;
            read_projection(&tx, project_id, graph_id)
        })();
        match outcome {
            Ok(value) => {
                tx.commit().map_err(|e| sqlite_error(Phase::Read, e))?;
                Ok(value)
            }
            Err(e) => {
                tx.rollback()
                    .map_err(|_| failure(Phase::Read, Code::RollbackFailure, None))?;
                Err(e)
            }
        }
    }
}

fn ordered_list(
    conn: &Connection,
    sql: &str,
    project: &str,
    graph: &str,
    version: &str,
    node: Option<&str>,
) -> Result<Vec<String>, StateError> {
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| sqlite_error(Phase::Read, e))?;
    let mut rows = if let Some(node) = node {
        stmt.query(params![project, graph, version, node])
    } else {
        stmt.query(params![project, graph, version])
    }
    .map_err(|e| sqlite_error(Phase::Read, e))?;
    let mut list = Vec::new();
    while let Some(row) = rows.next().map_err(|e| sqlite_error(Phase::Read, e))? {
        let ordinal: i64 = row.get(0).map_err(|e| sqlite_error(Phase::Read, e))?;
        let value: String = row.get(1).map_err(|e| sqlite_error(Phase::Read, e))?;
        if usize::try_from(ordinal).ok() != Some(list.len()) {
            return Err(corrupt());
        }
        list.push(value);
    }
    Ok(list)
}

fn read_projection(
    conn: &Connection,
    project_id: &str,
    graph_id: &str,
) -> Result<Option<PersistedCurrentGraphV1>, StateError> {
    let current: Option<String> = conn
        .query_row(
            "SELECT current_version FROM graphs WHERE project_id=?1 AND graph_id=?2",
            params![project_id, graph_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| sqlite_error(Phase::Read, e))?;
    let Some(current_version) = current else {
        return Ok(None);
    };
    if !positive(&current_version) {
        return Err(corrupt());
    }
    let mut member_stmt = conn
        .prepare("SELECT graph_version FROM graph_versions WHERE project_id=?1 AND graph_id=?2")
        .map_err(|e| sqlite_error(Phase::Read, e))?;
    let members = member_stmt
        .query_map(params![project_id, graph_id], |r| r.get::<_, String>(0))
        .map_err(|e| sqlite_error(Phase::Read, e))?;
    let mut current_projection = None;
    for member in members {
        let member = member.map_err(|e| sqlite_error(Phase::Read, e))?;
        if !positive(&member) {
            return Err(corrupt());
        }
        let projection =
            read_version_projection(conn, project_id, graph_id, &current_version, &member)?;
        if member == current_version {
            current_projection = Some(projection);
        }
    }
    current_projection.ok_or_else(corrupt).map(Some)
}

fn read_version_projection(
    conn: &Connection,
    project_id: &str,
    graph_id: &str,
    current_version: &str,
    member_version: &str,
) -> Result<PersistedCurrentGraphV1, StateError> {
    type Version = (
        String,
        i64,
        Option<String>,
        String,
        String,
        String,
        i64,
        Option<String>,
        String,
        String,
        String,
        String,
        String,
        String,
    );
    let version: Option<Version> = conn.query_row(
        "SELECT graph_version,parent_version_present,parent_version,goal_id,policy_id,created_at,compiled_from_present,context_epoch,resulting_digest,clock_source_id,clock_contract_version,compiler_id,source_ref,creation_reason FROM graph_versions WHERE project_id=?1 AND graph_id=?2 AND graph_version=?3",
        params![project_id, graph_id, member_version], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?,r.get(12)?,r.get(13)?)))
        .optional().map_err(|e| sqlite_error(Phase::Read,e))?;
    let Some((
        graph_version,
        parent_flag,
        parent,
        goal_id,
        policy_id,
        time,
        compiled_flag,
        epoch,
        digest,
        clock_source_id,
        clock_contract_version,
        compiler,
        source,
        reason,
    )) = version
    else {
        return Err(corrupt());
    };
    if graph_version != member_version
        || !positive(&graph_version)
        || !id(&goal_id)
        || !id(&policy_id)
        || !hex(&digest, 64)
        || !matches!(parent_flag, 0 | 1)
        || !matches!(compiled_flag, 0 | 1)
        || (parent_flag == 0 && parent.is_some())
        || parent.as_deref().is_some_and(|x| !positive(x))
        || (graph_version == "1" && parent.is_some())
        || clock_source_id.is_empty()
        || clock_contract_version.is_empty()
    {
        return Err(corrupt());
    }
    let created_at = CanonicalTimestampV1::parse(&time).map_err(|_| corrupt())?;
    let context_epoch = epoch
        .map(|s| StateEpochValueV1::try_from(s).map_err(|_| corrupt()))
        .transpose()?;
    let compiler_id = GraphCompilerRefV1::parse(compiler).map_err(|_| corrupt())?;
    let source_ref = GraphSourceEvidenceRefV1::parse(source).map_err(|_| corrupt())?;
    let creation_reason = GraphGenesisReasonV1::parse(&reason).map_err(|_| corrupt())?;
    let parent_version = if parent_flag == 0 { None } else { Some(parent) };
    let sources = ordered_list(
        conn,
        "SELECT ordinal,source FROM graph_compiled_sources WHERE project_id=?1 AND graph_id=?2 AND graph_version=?3 ORDER BY ordinal",
        project_id,
        graph_id,
        member_version,
        None,
    )?;
    if compiled_flag == 0 && !sources.is_empty() {
        return Err(corrupt());
    }
    let compiled_from = if compiled_flag == 0 {
        None
    } else {
        Some(sources)
    };

    let mut stmt = conn.prepare("SELECT node_id,graph_id,kind,state,title,parent_node_id,attempt_number,required_capabilities_present,task_capsule_ref,workstream_id,code_sha,workspace_id,result_ref,locked_reason_present,locked_reason,created_in_version FROM graph_nodes WHERE project_id=?1 AND graph_id=?2 AND graph_version=?3 ORDER BY node_id COLLATE BINARY")
        .map_err(|e| sqlite_error(Phase::Read,e))?;
    let mut rows = stmt
        .query(params![project_id, graph_id, member_version])
        .map_err(|e| sqlite_error(Phase::Read, e))?;
    let mut nodes = Vec::new();
    let mut node_ids = HashSet::new();
    while let Some(row) = rows.next().map_err(|e| sqlite_error(Phase::Read, e))? {
        let node_id: String = row.get(0).map_err(|e| sqlite_error(Phase::Read, e))?;
        let child_graph: String = row.get(1).map_err(|e| sqlite_error(Phase::Read, e))?;
        let cap_flag: i64 = row.get(7).map_err(|e| sqlite_error(Phase::Read, e))?;
        let lock_flag: i64 = row.get(13).map_err(|e| sqlite_error(Phase::Read, e))?;
        let lock_value: Option<String> = row.get(14).map_err(|e| sqlite_error(Phase::Read, e))?;
        if child_graph != graph_id
            || !matches!(cap_flag, 0 | 1)
            || !matches!(lock_flag, 0 | 1)
            || (lock_flag == 0 && lock_value.is_some())
            || !node_ids.insert(node_id.clone())
        {
            return Err(corrupt());
        }
        let caps = ordered_list(
            conn,
            "SELECT ordinal,capability FROM graph_node_capabilities WHERE project_id=?1 AND graph_id=?2 AND graph_version=?3 AND node_id=?4 ORDER BY ordinal",
            project_id,
            graph_id,
            member_version,
            Some(&node_id),
        )?;
        if cap_flag == 0 && !caps.is_empty() {
            return Err(corrupt());
        }
        let node = StateGraphNodeV1::new(
            node_id,
            child_graph,
            row.get(2).map_err(|e| sqlite_error(Phase::Read, e))?,
            row.get(3).map_err(|e| sqlite_error(Phase::Read, e))?,
            row.get(4).map_err(|e| sqlite_error(Phase::Read, e))?,
            row.get(5).map_err(|e| sqlite_error(Phase::Read, e))?,
            row.get(6).map_err(|e| sqlite_error(Phase::Read, e))?,
            if cap_flag == 0 { None } else { Some(caps) },
            row.get(8).map_err(|e| sqlite_error(Phase::Read, e))?,
            row.get(9).map_err(|e| sqlite_error(Phase::Read, e))?,
            row.get(10).map_err(|e| sqlite_error(Phase::Read, e))?,
            row.get(11).map_err(|e| sqlite_error(Phase::Read, e))?,
            row.get(12).map_err(|e| sqlite_error(Phase::Read, e))?,
            if lock_flag == 0 {
                None
            } else {
                Some(lock_value)
            },
            row.get(15).map_err(|e| sqlite_error(Phase::Read, e))?,
        )
        .map_err(|_| corrupt())?;
        nodes.push(node);
    }
    if nodes.is_empty() {
        return Err(corrupt());
    }
    drop(rows);
    drop(stmt);
    nodes.sort_by(|a, b| a.node_id().as_bytes().cmp(b.node_id().as_bytes()));

    let mut stmt = conn.prepare("SELECT edge_id,graph_id,from_node,to_node,edge_class,precedence_kind,control_kind,note FROM graph_edges WHERE project_id=?1 AND graph_id=?2 AND graph_version=?3 ORDER BY edge_id COLLATE BINARY")
        .map_err(|e| sqlite_error(Phase::Read,e))?;
    let mut rows = stmt
        .query(params![project_id, graph_id, member_version])
        .map_err(|e| sqlite_error(Phase::Read, e))?;
    let mut edges = Vec::new();
    let mut edge_ids = HashSet::new();
    while let Some(row) = rows.next().map_err(|e| sqlite_error(Phase::Read, e))? {
        let edge_id: String = row.get(0).map_err(|e| sqlite_error(Phase::Read, e))?;
        let child_graph: String = row.get(1).map_err(|e| sqlite_error(Phase::Read, e))?;
        let from_node: String = row.get(2).map_err(|e| sqlite_error(Phase::Read, e))?;
        let to_node: String = row.get(3).map_err(|e| sqlite_error(Phase::Read, e))?;
        let class: String = row.get(4).map_err(|e| sqlite_error(Phase::Read, e))?;
        let precedence: Option<String> = row.get(5).map_err(|e| sqlite_error(Phase::Read, e))?;
        let control: Option<String> = row.get(6).map_err(|e| sqlite_error(Phase::Read, e))?;
        if child_graph != graph_id
            || !edge_ids.insert(edge_id.clone())
            || !node_ids.contains(&from_node)
            || !node_ids.contains(&to_node)
        {
            return Err(corrupt());
        }
        let relation = match (class.as_str(), precedence, control) {
            ("PRECEDENCE", Some(x), None) => StateGraphEdgeRelationV1::Precedence(x),
            ("CONTROL", None, Some(x)) => StateGraphEdgeRelationV1::Control(x),
            _ => return Err(corrupt()),
        };
        let edge = StateGraphEdgeV1::new(
            edge_id,
            child_graph,
            from_node,
            to_node,
            relation,
            row.get(7).map_err(|e| sqlite_error(Phase::Read, e))?,
        )
        .map_err(|_| corrupt())?;
        edges.push(edge);
    }
    edges.sort_by(|a, b| a.edge_id().as_bytes().cmp(b.edge_id().as_bytes()));
    Ok(PersistedCurrentGraphV1 {
        project_id: project_id.to_string(),
        graph_id: graph_id.to_string(),
        current_version: current_version.to_string(),
        graph_version,
        parent_version,
        goal_id,
        policy_id,
        created_at,
        compiled_from,
        context_epoch,
        resulting_digest: digest,
        nodes,
        edges,
        genesis_provenance: StateGraphGenesisProvenanceV1::new(
            compiler_id,
            source_ref,
            creation_reason,
        ),
        clock_source_id,
        clock_contract_version,
    })
}

pub(crate) fn configure_graph_writer(conn: &Connection) -> Result<(), StateError> {
    conn.busy_timeout(Duration::from_millis(5000))
        .map_err(|e| sqlite_error(Phase::Migration, e))?;
    let mode: String = conn
        .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
        .map_err(|e| sqlite_error(Phase::Migration, e))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(failure(Phase::Migration, Code::StorageFailure, None));
    }
    conn.execute_batch("PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")
        .map_err(|e| sqlite_error(Phase::Migration, e))?;
    let sync: i64 = conn
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .map_err(|e| sqlite_error(Phase::Migration, e))?;
    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .map_err(|e| sqlite_error(Phase::Migration, e))?;
    if sync != 2 || fk != 1 {
        return Err(failure(Phase::Migration, Code::StorageFailure, None));
    }
    Ok(())
}

pub(crate) fn migrate_graph_store(conn: &Connection) -> Result<(), StateError> {
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| sqlite_error(Phase::Migration, e))?;
    let outcome = (|| {
        verify_ledger(conn, 12, Phase::Migration)?;
        verify_schema(conn, Phase::Migration, false)?;
        conn.execute_batch(migrations::v0013_graph_repository::SQL)
            .map_err(|e| sqlite_error(Phase::Migration, e))?;
        verify_ledger(conn, 13, Phase::Migration)?;
        verify_schema(conn, Phase::Migration, true)
    })();
    match outcome {
        Ok(()) => match conn.execute_batch("COMMIT") {
            Ok(()) => Ok(()),
            Err(e) => {
                let mapped = sqlite_error(Phase::Migration, e);
                conn.execute_batch("ROLLBACK")
                    .map_err(|_| failure(Phase::Migration, Code::RollbackFailure, None))?;
                Err(mapped)
            }
        },
        Err(e) => {
            conn.execute_batch("ROLLBACK")
                .map_err(|_| failure(Phase::Migration, Code::RollbackFailure, None))?;
            Err(e)
        }
    }
}

pub(crate) fn schema_head(conn: &Connection, phase: Phase) -> Result<Option<i64>, StateError> {
    let exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='state_schema_version'",
            [],
            |r| r.get(0),
        )
        .map_err(|e| sqlite_error(phase, e))?;
    if exists == 0 {
        return Ok(None);
    }
    conn.query_row("SELECT MAX(version) FROM state_schema_version", [], |r| {
        r.get(0)
    })
    .map_err(|e| schema_error(phase, e))
}
