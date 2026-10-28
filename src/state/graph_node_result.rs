//! Checked wire evidence. References and actor data do not confer authority.
use crate::error::{GraphFailureCode as Code, GraphPhase as Phase, StateError};
use crate::graph_repository::{hex, id};
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::value::RawValue;
use std::fmt;

pub(crate) fn invalid() -> StateError {
    StateError::GraphPersistence {
        phase: Phase::Input,
        code: Code::InvalidShape,
        field: None,
    }
}
pub(crate) fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Serialize)]
pub(crate) struct RawObject(pub(crate) Box<RawValue>);
impl<'de> Deserialize<'de> for RawObject {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Object;
        impl<'de> de::Visitor<'de> for Object {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
                while map.next_entry::<String, de::IgnoredAny>()?.is_some() {}
                Ok(())
            }
        }
        let raw = Box::<RawValue>::deserialize(d)?;
        (&*raw)
            .deserialize_map(Object)
            .map_err(|_| de::Error::custom("invalid object"))?;
        Ok(Self(raw))
    }
}
#[derive(Serialize)]
struct Integer(Box<RawValue>);
impl<'de> Deserialize<'de> for Integer {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(d)?;
        if !crate::json_integer::is_integer(&raw) {
            return Err(de::Error::custom("invalid integer"));
        }
        Ok(Self(raw))
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Check {
    command: Vec<String>,
    exit_code: Integer,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    result: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    output_ref: Option<String>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    node_id: String,
    graph_id: String,
    outcome: String,
    produced_at: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    code_sha: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    attempt_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    evidence_refs: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    checks: Option<Vec<Check>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    review_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    routing_decision_id: Option<String>,
}

/// Immutable, schema-shaped result; project/version/ref live in its storage wrapper.
pub struct StateGraphNodeResultV1 {
    wire: Wire,
}
impl fmt::Debug for StateGraphNodeResultV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StateGraphNodeResultV1")
    }
}
impl StateGraphNodeResultV1 {
    pub fn parse_json(text: &str) -> Result<Self, StateError> {
        let wire: Wire = serde_json::from_str(text).map_err(|_| invalid())?;
        if !id(&wire.node_id)
            || !id(&wire.graph_id)
            || !matches!(
                wire.outcome.as_str(),
                "PASS"
                    | "FAIL"
                    | "REJECTED"
                    | "BLOCKED"
                    | "CANCELLED"
                    | "SKIPPED"
                    | "HUMAN_REQUIRED"
            )
            || !date_time(&wire.produced_at)
            || wire.code_sha.as_deref().is_some_and(|s| !hex(s, 40))
            || [&wire.attempt_id, &wire.review_id, &wire.routing_decision_id]
                .iter()
                .any(|s| s.as_deref().is_some_and(|s| !id(s)))
            || wire.checks.as_ref().is_some_and(|checks| {
                checks.iter().any(|c| {
                    c.command.is_empty()
                        || c.result.as_deref().is_some_and(|s| {
                            !matches!(s, "PASS" | "FAIL" | "ERROR" | "SKIPPED" | "UNKNOWN")
                        })
                })
            })
        {
            return Err(invalid());
        }
        Ok(Self { wire })
    }
    pub fn to_json(&self) -> Result<String, StateError> {
        serde_json::to_string(&self.wire).map_err(|_| invalid())
    }
    pub fn graph_id(&self) -> &str {
        &self.wire.graph_id
    }
    pub fn node_id(&self) -> &str {
        &self.wire.node_id
    }
}

// RFC3339 evidence spelling is retained; trusted-clock canonicalization is separate.
pub(crate) fn date_time(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 20
        || !b.is_ascii()
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't')
        || b[13] != b':'
        || b[16] != b':'
    {
        return false;
    }
    let n = |a: usize, z: usize| s[a..z].parse::<u32>().ok();
    for (a, z) in [(0, 4), (5, 7), (8, 10), (11, 13), (14, 16), (17, 19)] {
        if !b[a..z].iter().all(u8::is_ascii_digit) {
            return false;
        }
    }
    let (Some(y), Some(m), Some(d), Some(h), Some(mi), Some(sec)) =
        (n(0, 4), n(5, 7), n(8, 10), n(11, 13), n(14, 16), n(17, 19))
    else {
        return false;
    };
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400)) => 29,
        2 => 28,
        _ => return false,
    };
    if d == 0 || d > days || h > 23 || mi > 59 || sec > 60 {
        return false;
    }
    let mut i = 19;
    if b[i] == b'.' {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return false;
        }
    }
    let Some(zone) = b.get(i..) else {
        return false;
    };
    if matches!(zone, [b'Z'] | [b'z']) {
        return sec < 60 || (h == 23 && mi == 59);
    }
    if zone.len() != 6
        || !matches!(zone[0], b'+' | b'-')
        || zone[3] != b':'
        || !zone[1..3].iter().chain(&zone[4..6]).all(u8::is_ascii_digit)
    {
        return false;
    }
    let zh = u32::from(zone[1] - b'0') * 10 + u32::from(zone[2] - b'0');
    let zm = u32::from(zone[4] - b'0') * 10 + u32::from(zone[5] - b'0');
    if zh > 23 || zm > 59 {
        return false;
    }
    let offset = (zh * 60 + zm) % 1440;
    let local = h * 60 + mi;
    sec < 60
        || if zone[0] == b'+' {
            (local + 1440 - offset) % 1440 == 1439
        } else {
            (local + offset) % 1440 == 1439
        }
}

use crate::graph_repository::{self as graph, ReadOnlyGraphReader, positive};
use crate::repository::SqliteStateRepository;
use rusqlite::{Connection, OptionalExtension, params};

/// External physical identity; the result wire keys remain unchanged.
pub struct PersistedNodeResultV1 {
    project_id: String,
    graph_version: String,
    result_ref: String,
    result: StateGraphNodeResultV1,
}
impl fmt::Debug for PersistedNodeResultV1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PersistedNodeResultV1")
    }
}
impl PersistedNodeResultV1 {
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn graph_version(&self) -> &str {
        &self.graph_version
    }
    pub fn result_ref(&self) -> &str {
        &self.result_ref
    }
    pub fn result(&self) -> &StateGraphNodeResultV1 {
        &self.result
    }
}
pub(crate) fn read_result(
    conn: &Connection,
    project: &str,
    graph_id: &str,
    reference: &str,
) -> Result<Option<PersistedNodeResultV1>, StateError> {
    if !id(project) || !id(graph_id) || reference.is_empty() {
        return Err(invalid());
    }
    crate::graph_history::validate_partition(conn, project, graph_id)?;
    let row:Option<(String,String,String)>=conn.query_row("SELECT graph_version,node_id,result_json FROM graph_node_results WHERE project_id=?1 AND graph_id=?2 AND result_ref=?3",params![project,graph_id,reference],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(|e|graph::sqlite_error(Phase::Read,e))?;
    let Some((version, node, raw)) = row else {
        return Ok(None);
    };
    let result = StateGraphNodeResultV1::parse_json(&raw).map_err(|_| graph::corrupt())?;
    if !positive(&version)
        || result.graph_id() != graph_id
        || result.node_id() != node
        || !node_exists(conn, project, graph_id, &version, &node)?
    {
        return Err(graph::corrupt());
    }
    Ok(Some(PersistedNodeResultV1 {
        project_id: project.into(),
        graph_version: version,
        result_ref: reference.into(),
        result,
    }))
}
fn node_exists(
    conn: &Connection,
    project: &str,
    graph_id: &str,
    version: &str,
    node: &str,
) -> Result<bool, StateError> {
    conn.query_row("SELECT EXISTS(SELECT 1 FROM graph_nodes WHERE project_id=?1 AND graph_id=?2 AND graph_version=?3 AND node_id=?4)",params![project,graph_id,version,node],|r|r.get(0)).map_err(|e|graph::sqlite_error(Phase::Read,e))
}
fn list_results(
    conn: &Connection,
    project: &str,
    graph_id: &str,
    version: &str,
    node: &str,
) -> Result<Vec<PersistedNodeResultV1>, StateError> {
    if !id(project) || !id(graph_id) || !positive(version) || !id(node) {
        return Err(invalid());
    }
    crate::graph_history::validate_partition(conn, project, graph_id)?;
    let mut stmt=conn.prepare("SELECT result_ref FROM graph_node_results WHERE project_id=?1 AND graph_id=?2 AND graph_version=?3 AND node_id=?4 ORDER BY result_ref COLLATE BINARY").map_err(|e|graph::sqlite_error(Phase::Read,e))?;
    let refs = stmt
        .query_map(params![project, graph_id, version, node], |r| {
            r.get::<_, String>(0)
        })
        .map_err(|e| graph::sqlite_error(Phase::Read, e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
    refs.into_iter()
        .map(|reference| {
            read_result(conn, project, graph_id, &reference)?.ok_or_else(graph::corrupt)
        })
        .collect()
}
impl SqliteStateRepository {
    pub fn append_node_result_v1(
        &mut self,
        project: &str,
        version: &str,
        reference: &str,
        result: StateGraphNodeResultV1,
    ) -> Result<(), StateError> {
        if !id(project) || !positive(version) || reference.is_empty() {
            return Err(invalid());
        }
        let raw = result.to_json()?;
        graph::write_transaction(self.connection(), |conn| {
            if read_result(conn, project, result.graph_id(), reference)?.is_some() {
                return Err(graph::failure(Phase::Write, Code::AlreadyExists, None));
            }
            if !node_exists(conn, project, result.graph_id(), version, result.node_id())? {
                return Err(graph::failure(Phase::Write, Code::IdentityMismatch, None));
            }
            conn.execute("INSERT INTO graph_node_results(project_id,graph_id,result_ref,graph_version,node_id,result_json) VALUES (?1,?2,?3,?4,?5,?6)",params![project,result.graph_id(),reference,version,result.node_id(),raw]).map_err(|e|graph::sqlite_error(Phase::Write,e))?;
            Ok(())
        })
    }
    pub fn find_node_result_v1(
        &self,
        project: &str,
        graph_id: &str,
        reference: &str,
    ) -> Result<Option<PersistedNodeResultV1>, StateError> {
        crate::graph_history::read_transaction(self.connection(), |c| {
            read_result(c, project, graph_id, reference)
        })
    }
    pub fn list_node_results_v1(
        &self,
        project: &str,
        graph_id: &str,
        version: &str,
        node: &str,
    ) -> Result<Vec<PersistedNodeResultV1>, StateError> {
        crate::graph_history::read_transaction(self.connection(), |c| {
            list_results(c, project, graph_id, version, node)
        })
    }
}
impl ReadOnlyGraphReader {
    pub fn find_node_result_v1(
        &self,
        project: &str,
        graph_id: &str,
        reference: &str,
    ) -> Result<Option<PersistedNodeResultV1>, StateError> {
        crate::graph_history::read_transaction(&self.conn, |c| {
            read_result(c, project, graph_id, reference)
        })
    }
    pub fn list_node_results_v1(
        &self,
        project: &str,
        graph_id: &str,
        version: &str,
        node: &str,
    ) -> Result<Vec<PersistedNodeResultV1>, StateError> {
        crate::graph_history::read_transaction(&self.conn, |c| {
            list_results(c, project, graph_id, version, node)
        })
    }
}
