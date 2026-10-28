//! Checked mutation evidence, distinct from immutable genesis provenance.
use crate::error::StateError;
use crate::graph_node_result::{RawObject, date_time, invalid, present};
use crate::graph_repository::{hex, id, positive};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use std::fmt;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Actor {
    role: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    role_id: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    op: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    node_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    edge_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    detail: Option<RawObject>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MutationWire {
    mutation_id: String,
    graph_id: String,
    parent_version: Box<RawValue>,
    resulting_version: Box<RawValue>,
    actor: Actor,
    reason: String,
    created_at: String,
    operations: Vec<Operation>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    resulting_digest: Option<String>,
}
/// Schema-shaped evidence only; actor strings are not authenticated authority.
pub struct StateGraphMutationV1 {
    wire: MutationWire,
}
impl fmt::Debug for StateGraphMutationV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StateGraphMutationV1")
    }
}
impl StateGraphMutationV1 {
    pub fn parse_json(text: &str) -> Result<Self, StateError> {
        let wire: MutationWire = serde_json::from_str(text).map_err(|_| invalid())?;
        if !id(&wire.mutation_id)
            || !id(&wire.graph_id)
            || !positive(wire.parent_version.get())
            || !positive(wire.resulting_version.get())
            || wire.resulting_version.get() == "1"
            || wire.actor.role_id.as_deref().is_some_and(|s| !id(s))
            || wire.reason.is_empty()
            || !date_time(&wire.created_at)
            || wire.operations.is_empty()
            || wire
                .resulting_digest
                .as_deref()
                .is_some_and(|s| !hex(s, 64))
            || wire.operations.iter().any(|op| {
                !matches!(
                    op.op.as_str(),
                    "ADD_NODE"
                        | "ADD_EDGE"
                        | "SET_NODE_STATE"
                        | "ATTACH_RESULT"
                        | "CANCEL_NODE"
                        | "EXPAND_REPAIR"
                ) || [&op.node_id, &op.edge_id]
                    .iter()
                    .any(|s| s.as_deref().is_some_and(|s| !id(s)))
            })
        {
            return Err(invalid());
        }
        Ok(Self { wire })
    }
    pub fn to_json(&self) -> Result<String, StateError> {
        serde_json::to_string(&self.wire).map_err(|_| invalid())
    }
    pub fn mutation_id(&self) -> &str {
        &self.wire.mutation_id
    }
    pub fn graph_id(&self) -> &str {
        &self.wire.graph_id
    }
    pub fn parent_version(&self) -> &str {
        self.wire.parent_version.get()
    }
    pub fn resulting_version(&self) -> &str {
        self.wire.resulting_version.get()
    }
    pub fn resulting_digest(&self) -> Option<&str> {
        self.wire.resulting_digest.as_deref()
    }
    pub fn actor_role(&self) -> &str {
        &self.wire.actor.role
    }
    pub fn reason(&self) -> &str {
        &self.wire.reason
    }
}

use crate::error::{GraphFailureCode as Code, GraphPhase as Phase};
use crate::graph_repository::{
    self as graph, PersistedCurrentGraphV1, ReadOnlyGraphReader, StateGraphSnapshotV1,
};
use crate::repository::SqliteStateRepository;
use crate::trusted_time::{self, TrustedClockV1};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn schema_matches(conn: &Connection) -> Result<bool, rusqlite::Error> {
    for statement in crate::migrations::v0015_graph_history::SQL.split(";\n") {
        let statement = statement.trim();
        let mut words = statement.split_whitespace();
        let (Some("CREATE"), Some(kind), Some(name)) = (words.next(), words.next(), words.next())
        else {
            continue;
        };
        let stored: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type=?1 AND name=?2",
                params![kind.to_ascii_lowercase(), name],
                |r| r.get(0),
            )
            .optional()?;
        if stored.as_deref() != Some(statement) {
            return Ok(false);
        }
    }
    let count:i64=conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name GLOB 'graph*_immutable_*'",[],|r|r.get(0))?;
    Ok(count == 32)
}

pub(crate) fn read_transaction<T>(
    conn: &Connection,
    work: impl FnOnce(&Connection) -> Result<T, StateError>,
) -> Result<T, StateError> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
    match graph::verify_current(&tx, Phase::Read).and_then(|()| work(&tx)) {
        Ok(value) => {
            tx.commit()
                .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
            Ok(value)
        }
        Err(e) => {
            tx.rollback()
                .map_err(|_| graph::failure(Phase::Read, Code::RollbackFailure, None))?;
            Err(e)
        }
    }
}
fn version_projection(
    conn: &Connection,
    project: &str,
    graph_id: &str,
    version: &str,
) -> Result<Option<PersistedCurrentGraphV1>, StateError> {
    if !id(project) || !id(graph_id) || !positive(version) {
        return Err(invalid());
    }
    validate_partition(conn, project, graph_id)?;
    version_projection_validated(conn, project, graph_id, version)
}

// Only callers that validated this partition in the same transaction may use this leaf.
fn version_projection_validated(
    conn: &Connection,
    project: &str,
    graph_id: &str,
    version: &str,
) -> Result<Option<PersistedCurrentGraphV1>, StateError> {
    let current: Option<String> = conn
        .query_row(
            "SELECT current_version FROM graphs WHERE project_id=?1 AND graph_id=?2",
            params![project, graph_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
    let Some(current) = current else {
        return Ok(None);
    };
    let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM graph_versions WHERE project_id=?1 AND graph_id=?2 AND graph_version=?3)",params![project,graph_id,version],|r|r.get(0)).map_err(|e|graph::sqlite_error(Phase::Read,e))?;
    if !exists {
        return Ok(None);
    }
    graph::read_version_projection(conn, project, graph_id, &current, version).map(Some)
}
fn mutations(
    conn: &Connection,
    project: &str,
    graph_id: &str,
) -> Result<Vec<StateGraphMutationV1>, StateError> {
    if !id(project) || !id(graph_id) {
        return Err(invalid());
    }
    validate_partition(conn, project, graph_id)?;
    let mut stmt=conn.prepare("SELECT mutation_id,parent_version,resulting_version,mutation_json FROM graph_mutations WHERE project_id=?1 AND graph_id=?2 ORDER BY mutation_id COLLATE BINARY").map_err(|e|graph::sqlite_error(Phase::Read,e))?;
    let mut rows = stmt
        .query(params![project, graph_id])
        .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
    let mut result = Vec::new();
    while let Some(row) = rows
        .next()
        .map_err(|e| graph::sqlite_error(Phase::Read, e))?
    {
        let raw: String = row
            .get(3)
            .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
        let mutation = StateGraphMutationV1::parse_json(&raw).map_err(|_| graph::corrupt())?;
        let (mid, parent, version): (String, String, String) = (
            row.get(0)
                .map_err(|e| graph::sqlite_error(Phase::Read, e))?,
            row.get(1)
                .map_err(|e| graph::sqlite_error(Phase::Read, e))?,
            row.get(2)
                .map_err(|e| graph::sqlite_error(Phase::Read, e))?,
        );
        let snapshot = version_projection_validated(conn, project, graph_id, &version)?
            .ok_or_else(graph::corrupt)?;
        if mutation.graph_id() != graph_id
            || mutation.mutation_id() != mid
            || mutation.parent_version() != parent
            || mutation.resulting_version() != version
            || snapshot.parent_version() != Some(Some(parent.as_str()))
            || mutation
                .resulting_digest()
                .is_some_and(|d| d != snapshot.resulting_digest())
            || version_projection_validated(conn, project, graph_id, &parent)?.is_none()
        {
            return Err(graph::corrupt());
        }
        result.push(mutation);
    }
    Ok(result)
}
impl SqliteStateRepository {
    /// Stores the supplied physical successor and moves the mutable head by parent CAS.
    pub fn append_graph_mutation_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        mutation: StateGraphMutationV1,
        snapshot: StateGraphSnapshotV1,
    ) -> Result<(), StateError> {
        let content = &snapshot.content;
        if mutation.graph_id() != content.graph_id()
            || mutation.parent_version() != snapshot.parent_version()
            || mutation.resulting_version() != snapshot.graph_version()
            || mutation
                .resulting_digest()
                .is_some_and(|d| d != content.resulting_digest())
        {
            return Err(graph::failure(Phase::Input, Code::IdentityMismatch, None));
        }
        let raw = mutation.to_json()?;
        let sample = clock
            .sample()
            .map_err(|e| graph::legacy_error(Phase::Clock, e))?;
        let (sample, time) = trusted_time::validate_sample(sample)
            .map_err(|e| graph::legacy_error(Phase::Clock, e))?;
        graph::write_transaction(self.connection(), |conn| {
            let genesis = version_projection(conn, content.project_id(), content.graph_id(), "1")?
                .ok_or_else(|| graph::failure(Phase::Write, Code::IdentityMismatch, None))?;
            if genesis.genesis_provenance() != content.genesis_provenance() {
                return Err(graph::failure(Phase::Write, Code::IdentityMismatch, None));
            }
            let existing:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM graph_mutations WHERE project_id=?1 AND graph_id=?2 AND mutation_id=?3)",params![content.project_id(),content.graph_id(),mutation.mutation_id()],|r|r.get(0)).map_err(|e|graph::sqlite_error(Phase::Write,e))?;
            if existing {
                return Err(graph::failure(Phase::Write, Code::AlreadyExists, None));
            }
            for node in content.nodes() {
                if let Some(reference) = node.result_ref() {
                    let result = crate::graph_node_result::read_result(
                        conn,
                        content.project_id(),
                        content.graph_id(),
                        reference,
                    )?
                    .ok_or_else(|| graph::failure(Phase::Write, Code::IdentityMismatch, None))?;
                    if result.result().node_id() != node.node_id() {
                        return Err(graph::failure(Phase::Write, Code::IdentityMismatch, None));
                    }
                }
            }
            let moved=conn.execute("UPDATE graphs SET current_version=?1 WHERE project_id=?2 AND graph_id=?3 AND current_version=?4",params![snapshot.graph_version(),content.project_id(),content.graph_id(),snapshot.parent_version()]).map_err(|e|graph::sqlite_error(Phase::Write,e))?;
            if moved != 1 {
                return Err(graph::failure(Phase::Write, Code::ParentConflict, None));
            }
            graph::accept_graph_sample(conn, content.project_id(), &sample, &time)?;
            graph::insert_snapshot(conn, content, snapshot.graph_version(), &sample)?;
            conn.execute("INSERT INTO graph_mutations(project_id,graph_id,mutation_id,parent_version,resulting_version,mutation_json) VALUES (?1,?2,?3,?4,?5,?6)",params![content.project_id(),content.graph_id(),mutation.mutation_id(),mutation.parent_version(),mutation.resulting_version(),raw]).map_err(|e|graph::sqlite_error(Phase::Write,e))?;
            graph::seal_snapshot(conn, content, snapshot.graph_version())?;
            Ok(())
        })
    }
    pub fn load_graph_version_v1(
        &self,
        project: &str,
        graph_id: &str,
        version: &str,
    ) -> Result<Option<PersistedCurrentGraphV1>, StateError> {
        read_transaction(self.connection(), |c| {
            version_projection(c, project, graph_id, version)
        })
    }
    pub fn load_current_graph_v1(
        &self,
        project: &str,
        graph_id: &str,
    ) -> Result<Option<PersistedCurrentGraphV1>, StateError> {
        if !id(project) || !id(graph_id) {
            return Err(invalid());
        }
        read_transaction(self.connection(), |c| {
            graph::read_projection(c, project, graph_id)
        })
    }
    /// Deterministic binary mutation-id order; no latest/semantic ordering inferred.
    pub fn list_graph_mutations_v1(
        &self,
        project: &str,
        graph_id: &str,
    ) -> Result<Vec<StateGraphMutationV1>, StateError> {
        read_transaction(self.connection(), |c| mutations(c, project, graph_id))
    }
}
impl ReadOnlyGraphReader {
    pub fn load_graph_version_v1(
        &self,
        project: &str,
        graph_id: &str,
        version: &str,
    ) -> Result<Option<PersistedCurrentGraphV1>, StateError> {
        read_transaction(&self.conn, |c| {
            version_projection(c, project, graph_id, version)
        })
    }
    pub fn list_graph_mutations_v1(
        &self,
        project: &str,
        graph_id: &str,
    ) -> Result<Vec<StateGraphMutationV1>, StateError> {
        read_transaction(&self.conn, |c| mutations(c, project, graph_id))
    }
}

#[cfg(test)]
std::thread_local! {
    static PARTITION_VALIDATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn take_partition_validation_count() -> usize {
    PARTITION_VALIDATIONS.with(|count| count.replace(0))
}

/// Validates immutable associations inside the caller's coherent read snapshot.
pub(crate) fn validate_partition(
    conn: &Connection,
    project: &str,
    graph_id: &str,
) -> Result<(), StateError> {
    #[cfg(test)]
    PARTITION_VALIDATIONS.with(|count| count.set(count.get() + 1));
    let map = |e| graph::sqlite_error(Phase::Read, e);
    // Reuse the leaf decoder, which does not call this partition boundary.
    let current: Option<String> = conn
        .query_row(
            "SELECT current_version FROM graphs WHERE project_id=?1 AND graph_id=?2",
            params![project, graph_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(map)?;
    let mut versions = conn
        .prepare("SELECT graph_version FROM graph_versions WHERE project_id=?1 AND graph_id=?2")
        .map_err(map)?;
    let members = versions
        .query_map(params![project, graph_id], |r| r.get::<_, String>(0))
        .map_err(map)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map)?;
    match current {
        Some(current) => {
            if !positive(&current) || !members.contains(&current) {
                return Err(graph::corrupt());
            }
            for version in &members {
                graph::read_version_projection(conn, project, graph_id, &current, version)?;
            }
        }
        None if !members.is_empty() => return Err(graph::corrupt()),
        None => {}
    }
    let broken:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM graph_versions v LEFT JOIN graph_snapshot_seals s USING(project_id,graph_id,graph_version) WHERE v.project_id=?1 AND v.graph_id=?2 AND s.graph_version IS NULL) OR EXISTS(SELECT 1 FROM graph_snapshot_seals s LEFT JOIN graph_versions v USING(project_id,graph_id,graph_version) WHERE s.project_id=?1 AND s.graph_id=?2 AND v.graph_version IS NULL)",params![project,graph_id],|r|r.get(0)).map_err(map)?;
    if broken {
        return Err(graph::corrupt());
    }
    let mut stmt=conn.prepare("SELECT r.graph_version,r.node_id,r.result_json FROM graph_node_results r WHERE project_id=?1 AND graph_id=?2").map_err(map)?;
    let rows = stmt
        .query_map(params![project, graph_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(map)?;
    for row in rows {
        let (version, node, json) = row.map_err(map)?;
        let result = crate::graph_node_result::StateGraphNodeResultV1::parse_json(&json)
            .map_err(|_| graph::corrupt())?;
        let exists:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM graph_nodes WHERE project_id=?1 AND graph_id=?2 AND graph_version=?3 AND node_id=?4)",params![project,graph_id,version,node],|r|r.get(0)).map_err(map)?;
        if !exists
            || !positive(&version)
            || result.graph_id() != graph_id
            || result.node_id() != node
        {
            return Err(graph::corrupt());
        }
    }
    let mut stmt=conn.prepare("SELECT v.graph_version,v.parent_version,v.resulting_digest,s.legacy FROM graph_versions v JOIN graph_snapshot_seals s USING(project_id,graph_id,graph_version) WHERE v.project_id=?1 AND v.graph_id=?2").map_err(map)?;
    let rows = stmt
        .query_map(params![project, graph_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(map)?;
    for row in rows {
        let (version, parent, digest, legacy) = row.map_err(map)?;
        if !matches!(legacy, 0 | 1) {
            return Err(graph::corrupt());
        }
        if legacy == 1 {
            let has_mutation:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM graph_mutations WHERE project_id=?1 AND graph_id=?2 AND resulting_version=?3)",params![project,graph_id,version],|r|r.get(0)).map_err(map)?;
            if has_mutation {
                return Err(graph::corrupt());
            }
        }
        if version != "1" && legacy == 0 {
            let association:Option<(String,String,String)>=conn.query_row("SELECT mutation_id,parent_version,mutation_json FROM graph_mutations WHERE project_id=?1 AND graph_id=?2 AND resulting_version=?3",params![project,graph_id,version],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(map)?;
            let Some((mid, p, json)) = association else {
                return Err(graph::corrupt());
            };
            let mutation = StateGraphMutationV1::parse_json(&json).map_err(|_| graph::corrupt())?;
            if mutation.mutation_id() != mid
                || mutation.graph_id() != graph_id
                || mutation.parent_version() != p
                || parent.as_deref() != Some(p.as_str())
                || mutation.resulting_version() != version
                || mutation.resulting_digest().is_some_and(|d| d != digest)
            {
                return Err(graph::corrupt());
            }
            let missing:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM graph_nodes n LEFT JOIN graph_node_results r ON r.project_id=n.project_id AND r.graph_id=n.graph_id AND r.node_id=n.node_id AND r.result_ref=n.result_ref WHERE n.project_id=?1 AND n.graph_id=?2 AND n.graph_version=?3 AND n.result_ref IS NOT NULL AND r.result_ref IS NULL)",params![project,graph_id,version],|r|r.get(0)).map_err(map)?;
            if missing {
                return Err(graph::corrupt());
            }
        }
    }
    let mut stmt=conn.prepare("SELECT mutation_id,parent_version,resulting_version,mutation_json FROM graph_mutations WHERE project_id=?1 AND graph_id=?2").map_err(map)?;
    let rows = stmt
        .query_map(params![project, graph_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(map)?;
    for row in rows {
        let (mid, parent, version, json) = row.map_err(map)?;
        let m = StateGraphMutationV1::parse_json(&json).map_err(|_| graph::corrupt())?;
        let count:i64=conn.query_row("SELECT COUNT(*) FROM graph_versions WHERE project_id=?1 AND graph_id=?2 AND graph_version IN (?3,?4)",params![project,graph_id,parent,version],|r|r.get(0)).map_err(map)?;
        if parent == version
            || count != 2
            || m.graph_id() != graph_id
            || m.mutation_id() != mid
            || m.parent_version() != parent
            || m.resulting_version() != version
        {
            return Err(graph::corrupt());
        }
    }
    Ok(())
}
