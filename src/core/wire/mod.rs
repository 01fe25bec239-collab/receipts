//! Strict JSON codecs for Orchestration's twelve owned schemas. Graph:
//! `ExecutionGraph`, `GraphNode`, `GraphMutation`, `GraphSnapshot` and
//! `GraphNodeResult` (`GraphEdge` reuses [`GraphEdge::from_json`]). The six
//! capsule/decision codecs live in `contracts` (see its notes).
//!
//! Each `*WireV1` value is schema-shaped data: decoding checks the frozen
//! schema's shape exactly (unknown, duplicate and wrongly typed or null fields
//! refuse) and keeps the admitted JSON tree verbatim, so absence, explicit
//! null, strings, array order and exact number lexemes survive re-encoding.
//! `date-time` is a schema annotation here, so any string is admitted.
//!
//! Converting to an operational value is a separate, fallible step: a
//! schema-valid value outside the operational domain (for example
//! `graph_version: 1.0`, a non-RFC3339 timestamp or an unknown snapshot state)
//! is reported as [`GraphWireError::Unsupported`] and the wire value stays
//! unchanged. Graph semantics (containment, duplicates, endpoints, cycles) are
//! checked only by the operational constructors. Decoding grants no graph,
//! dispatch, write or acceptance authority.

use std::collections::BTreeMap;

use crate::goal::GoalEvaluationContextEpoch;
use crate::node_result::{GraphNodeCheck, GraphNodeResult, GraphNodeResultError};
use crate::orchestration::{
    OrchestrationDateTimeV1, OrchestrationJsonNumberV1, OrchestrationJsonObjectV1,
    OrchestrationJsonValueV1 as V,
};
use crate::{
    CapabilityName, ExecutionGraph, ExecutionGraphEnvelope, ExecutionGraphEnvelopeError,
    ExecutionGraphEnvelopeFields, GraphEdge, GraphError, GraphMutation, GraphMutationActor,
    GraphMutationOperation, GraphMutationOperationKind, GraphNode, GraphNodeAttemptNumber,
    GraphNodeCheckResult, GraphNodeKind, GraphNodeResultOutcome, GraphNodeState, GraphRecordError,
    GraphSnapshot, GraphSnapshotNodeState, GraphVersionV1, MAX_IDENTIFIER_LENGTH,
};

#[cfg(test)]
mod contracts_tests;
#[cfg(test)]
mod tests;

/// Why a wire value was refused or could not become an operational value.
/// Field names come from the frozen schemas; input values are not echoed by
/// the shape and JSON variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphWireError {
    /// Not one strict JSON value (syntax, duplicate key, depth, trailing input).
    Json,
    /// The JSON value does not have the schema's shape at the named field.
    Shape(&'static str),
    /// Schema-valid, but outside the operational domain of the named field.
    Unsupported(&'static str),
    Graph(GraphError),
    Record(GraphRecordError),
    Envelope(ExecutionGraphEnvelopeError),
    Result(GraphNodeResultError),
}

impl std::fmt::Display for GraphWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json => f.write_str("input is not one strict JSON value"),
            Self::Shape(field) => write!(f, "schema shape refused at {field}"),
            Self::Unsupported(field) => {
                write!(f, "schema-valid {field} is outside the operational domain")
            }
            Self::Graph(error) => error.fmt(f),
            Self::Record(error) => error.fmt(f),
            Self::Envelope(error) => error.fmt(f),
            Self::Result(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for GraphWireError {}

impl From<GraphError> for GraphWireError {
    fn from(error: GraphError) -> Self {
        Self::Graph(error)
    }
}
impl From<GraphRecordError> for GraphWireError {
    fn from(error: GraphRecordError) -> Self {
        Self::Record(error)
    }
}
impl From<GraphNodeResultError> for GraphWireError {
    fn from(error: GraphNodeResultError) -> Self {
        Self::Result(error)
    }
}

type Map = BTreeMap<String, V>;
type WireResult<T> = Result<T, GraphWireError>;

macro_rules! wire_type {
    ($(#[$doc:meta])* $name:ident, $validate:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name(OrchestrationJsonObjectV1);

        impl $name {
            /// Decodes strict JSON text and checks the schema shape.
            pub fn from_json(text: &str) -> WireResult<Self> {
                Self::from_value(V::parse_json(text).map_err(|_| GraphWireError::Json)?)
            }
            /// Checks an already parsed value against the schema shape.
            pub fn from_value(value: V) -> WireResult<Self> {
                let V::Object(object) = value else {
                    return Err(GraphWireError::Shape("root"));
                };
                $validate(object.as_map())?;
                Ok(Self(object))
            }
            /// The admitted value, verbatim.
            pub fn as_object(&self) -> &OrchestrationJsonObjectV1 {
                &self.0
            }
            /// Compact JSON with exact number lexemes.
            pub fn to_json(&self) -> String {
                V::Object(self.0.clone()).to_json()
            }
        }
    };
}

wire_type!(
    /// Schema-shaped `GraphNode` JSON.
    GraphNodeWireV1,
    validate_node
);
wire_type!(
    /// Schema-shaped `ExecutionGraph` JSON.
    ExecutionGraphWireV1,
    validate_graph
);
wire_type!(
    /// Schema-shaped `GraphMutation` JSON.
    GraphMutationWireV1,
    validate_mutation
);
wire_type!(
    /// Schema-shaped `GraphSnapshot` JSON.
    GraphSnapshotWireV1,
    validate_snapshot
);
wire_type!(
    /// Schema-shaped `GraphNodeResult` JSON.
    GraphNodeResultWireV1,
    validate_result
);

mod contracts;
pub use contracts::{
    DispatchAdmissionDecisionWireV1, FeatureAdmissionDecisionWireV1, FeatureCapabilitySetWireV1,
    GoalEvaluationWireV1, RepairCapsuleWireV1, TaskCapsuleWireV1,
};

// --- shape primitives -----------------------------------------------------

fn object<'a>(value: &'a V, field: &'static str) -> WireResult<&'a Map> {
    match value {
        V::Object(object) => Ok(object.as_map()),
        _ => Err(GraphWireError::Shape(field)),
    }
}

fn keys(
    map: &Map,
    allowed: &[&str],
    required: &[&'static str],
    at: &'static str,
) -> WireResult<()> {
    if let Some(missing) = required.iter().find(|key| !map.contains_key(**key)) {
        return Err(GraphWireError::Shape(missing));
    }
    if map.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(GraphWireError::Shape(at));
    }
    Ok(())
}

fn string<'a>(map: &'a Map, field: &'static str) -> WireResult<Option<&'a str>> {
    match map.get(field) {
        None => Ok(None),
        Some(V::String(s)) => Ok(Some(s)),
        Some(_) => Err(GraphWireError::Shape(field)),
    }
}

fn nonempty<'a>(map: &'a Map, field: &'static str) -> WireResult<Option<&'a str>> {
    let value = string(map, field)?;
    if value == Some("") {
        return Err(GraphWireError::Shape(field));
    }
    Ok(value)
}

fn identifier<'a>(map: &'a Map, field: &'static str) -> WireResult<Option<&'a str>> {
    let value = string(map, field)?;
    if value.is_some_and(|v| v.is_empty() || v.chars().count() > MAX_IDENTIFIER_LENGTH) {
        return Err(GraphWireError::Shape(field));
    }
    Ok(value)
}

fn hex<'a>(map: &'a Map, field: &'static str, length: usize) -> WireResult<Option<&'a str>> {
    let value = string(map, field)?;
    if value.is_some_and(|v| {
        v.len() != length || !v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }) {
        return Err(GraphWireError::Shape(field));
    }
    Ok(value)
}

fn integer<'a>(
    map: &'a Map,
    field: &'static str,
    min: u8,
) -> WireResult<Option<&'a OrchestrationJsonNumberV1>> {
    match map.get(field) {
        None => Ok(None),
        Some(V::Number(n)) if integer_at_least(n.as_str(), min) => Ok(Some(n)),
        Some(_) => Err(GraphWireError::Shape(field)),
    }
}

fn array<'a>(map: &'a Map, field: &'static str) -> WireResult<Option<&'a [V]>> {
    match map.get(field) {
        None => Ok(None),
        Some(V::Array(items)) => Ok(Some(items)),
        Some(_) => Err(GraphWireError::Shape(field)),
    }
}

fn strings<'a>(map: &'a Map, field: &'static str) -> WireResult<Option<Vec<&'a str>>> {
    array(map, field)?
        .map(|items| {
            items
                .iter()
                .map(|item| match item {
                    V::String(s) => Ok(s.as_str()),
                    _ => Err(GraphWireError::Shape(field)),
                })
                .collect()
        })
        .transpose()
}

fn one_of(map: &Map, field: &'static str, allowed: &[&str]) -> WireResult<()> {
    match string(map, field)? {
        Some(value) if !allowed.contains(&value) => Err(GraphWireError::Shape(field)),
        _ => Ok(()),
    }
}

/// Exact JSON-Schema `integer` test with a small lower bound: the lexeme's
/// mathematical value is integral and at least `min` (0, 1 or 2). Uses only
/// the decimal text; exponents of any length are handled without floats.
pub(crate) fn integer_at_least(lexeme: &str, min: u8) -> bool {
    let (negative, rest) = match lexeme.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, lexeme),
    };
    let (mantissa, exponent) = rest.split_once(['e', 'E']).unwrap_or((rest, "0"));
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{whole}{fraction}");
    let significant = digits.trim_start_matches('0');
    if significant.is_empty() {
        return min == 0;
    }
    let trimmed = significant.trim_end_matches('0');
    let trailing = (significant.len() - trimmed.len()) as i128;
    let exponent_negative = exponent.starts_with('-');
    let magnitude = exponent
        .trim_start_matches(['+', '-'])
        .trim_start_matches('0');
    // Anything longer than 30 digits dwarfs the (input-length) shift.
    let power = if magnitude.len() > 30 {
        if exponent_negative {
            return false;
        }
        i128::MAX / 2
    } else {
        let value: i128 = magnitude.parse().unwrap_or(0);
        if exponent_negative { -value } else { value }
    } - fraction.len() as i128
        + trailing;
    if power < 0 || negative {
        return false;
    }
    min < 2 || trimmed != "1" || power > 0
}

// --- schema shapes --------------------------------------------------------

const NODE_FIELDS: [&str; 15] = [
    "node_id",
    "graph_id",
    "kind",
    "state",
    "title",
    "parent_node_id",
    "attempt_number",
    "required_capabilities",
    "task_capsule_ref",
    "workstream_id",
    "code_sha",
    "workspace_id",
    "result_ref",
    "locked_reason",
    "created_in_version",
];

fn validate_node(map: &Map) -> WireResult<()> {
    keys(
        map,
        &NODE_FIELDS,
        &["node_id", "graph_id", "kind", "state"],
        "GraphNode",
    )?;
    for field in [
        "node_id",
        "graph_id",
        "parent_node_id",
        "workstream_id",
        "workspace_id",
    ] {
        identifier(map, field)?;
    }
    nonempty(map, "kind")?;
    if string(map, "state")?.is_some_and(|s| GraphNodeState::parse(s).is_err()) {
        return Err(GraphWireError::Shape("state"));
    }
    for field in ["title", "task_capsule_ref", "result_ref"] {
        string(map, field)?;
    }
    integer(map, "attempt_number", 1)?;
    integer(map, "created_in_version", 1)?;
    if let Some(capabilities) = strings(map, "required_capabilities")?
        && capabilities
            .iter()
            .any(|c| CapabilityName::new(*c).is_err())
    {
        return Err(GraphWireError::Shape("required_capabilities"));
    }
    hex(map, "code_sha", 40)?;
    match map.get("locked_reason") {
        None | Some(V::Null | V::String(_)) => Ok(()),
        Some(_) => Err(GraphWireError::Shape("locked_reason")),
    }
}

fn validate_edge(value: &V) -> WireResult<GraphEdge> {
    GraphEdge::from_json(&value.to_json()).map_err(|_| GraphWireError::Shape("edges"))
}

const GRAPH_FIELDS: [&str; 12] = [
    "graph_id",
    "graph_version",
    "parent_version",
    "project_id",
    "goal_id",
    "nodes",
    "edges",
    "policy_id",
    "resulting_digest",
    "created_at",
    "compiled_from",
    "context_epoch",
];

fn validate_graph(map: &Map) -> WireResult<()> {
    keys(
        map,
        &GRAPH_FIELDS,
        &[
            "graph_id",
            "graph_version",
            "project_id",
            "goal_id",
            "nodes",
            "edges",
            "created_at",
            "policy_id",
        ],
        "ExecutionGraph",
    )?;
    for field in ["graph_id", "project_id", "goal_id", "policy_id"] {
        identifier(map, field)?;
    }
    integer(map, "graph_version", 1)?;
    if !matches!(map.get("parent_version"), None | Some(V::Null)) {
        integer(map, "parent_version", 1)?;
    }
    let nodes = array(map, "nodes")?.unwrap_or_default();
    if nodes.is_empty() {
        return Err(GraphWireError::Shape("nodes"));
    }
    for node in nodes {
        validate_node(object(node, "nodes")?)?;
    }
    for edge in array(map, "edges")?.unwrap_or_default() {
        validate_edge(edge)?;
    }
    hex(map, "resulting_digest", 64)?;
    string(map, "created_at")?;
    strings(map, "compiled_from")?;
    integer(map, "context_epoch", 0)?;
    Ok(())
}

const OPERATIONS: [&str; 6] = [
    "ADD_NODE",
    "ADD_EDGE",
    "SET_NODE_STATE",
    "ATTACH_RESULT",
    "CANCEL_NODE",
    "EXPAND_REPAIR",
];

fn validate_mutation(map: &Map) -> WireResult<()> {
    keys(
        map,
        &[
            "mutation_id",
            "graph_id",
            "parent_version",
            "resulting_version",
            "actor",
            "reason",
            "operations",
            "resulting_digest",
            "created_at",
        ],
        &[
            "mutation_id",
            "graph_id",
            "parent_version",
            "resulting_version",
            "actor",
            "reason",
            "created_at",
            "operations",
        ],
        "GraphMutation",
    )?;
    identifier(map, "mutation_id")?;
    identifier(map, "graph_id")?;
    integer(map, "parent_version", 1)?;
    integer(map, "resulting_version", 2)?;
    let actor = object(&map["actor"], "actor")?;
    keys(actor, &["role", "role_id"], &["role"], "actor")?;
    string(actor, "role")?;
    identifier(actor, "role_id")?;
    nonempty(map, "reason")?;
    let operations = array(map, "operations")?.unwrap_or_default();
    if operations.is_empty() {
        return Err(GraphWireError::Shape("operations"));
    }
    for operation in operations {
        let operation = object(operation, "operations")?;
        keys(
            operation,
            &["op", "node_id", "edge_id", "detail"],
            &["op"],
            "operations",
        )?;
        one_of(operation, "op", &OPERATIONS)?;
        identifier(operation, "node_id")?;
        identifier(operation, "edge_id")?;
        if let Some(detail) = operation.get("detail") {
            object(detail, "detail")?;
        }
    }
    hex(map, "resulting_digest", 64)?;
    string(map, "created_at")?;
    Ok(())
}

fn validate_snapshot(map: &Map) -> WireResult<()> {
    keys(
        map,
        &[
            "graph_id",
            "graph_version",
            "captured_at",
            "resulting_digest",
            "node_states",
            "summary",
        ],
        &["graph_id", "graph_version", "captured_at", "node_states"],
        "GraphSnapshot",
    )?;
    identifier(map, "graph_id")?;
    integer(map, "graph_version", 1)?;
    string(map, "captured_at")?;
    hex(map, "resulting_digest", 64)?;
    for entry in array(map, "node_states")?.unwrap_or_default() {
        let entry = object(entry, "node_states")?;
        keys(
            entry,
            &["node_id", "state", "kind", "locked", "code_sha"],
            &["node_id", "state"],
            "node_states",
        )?;
        identifier(entry, "node_id")?;
        string(entry, "state")?;
        nonempty(entry, "kind")?;
        if !matches!(entry.get("locked"), None | Some(V::Boolean(_))) {
            return Err(GraphWireError::Shape("locked"));
        }
        hex(entry, "code_sha", 40)?;
    }
    if let Some(summary) = map.get("summary") {
        object(summary, "summary")?;
    }
    Ok(())
}

const OUTCOMES: [&str; 7] = [
    "PASS",
    "FAIL",
    "REJECTED",
    "BLOCKED",
    "CANCELLED",
    "SKIPPED",
    "HUMAN_REQUIRED",
];
const CHECK_RESULTS: [&str; 5] = ["PASS", "FAIL", "ERROR", "SKIPPED", "UNKNOWN"];

fn validate_result(map: &Map) -> WireResult<()> {
    keys(
        map,
        &[
            "node_id",
            "graph_id",
            "outcome",
            "code_sha",
            "attempt_id",
            "evidence_refs",
            "checks",
            "review_id",
            "routing_decision_id",
            "produced_at",
        ],
        &["node_id", "graph_id", "outcome", "produced_at"],
        "GraphNodeResult",
    )?;
    for field in [
        "node_id",
        "graph_id",
        "attempt_id",
        "review_id",
        "routing_decision_id",
    ] {
        identifier(map, field)?;
    }
    one_of(map, "outcome", &OUTCOMES)?;
    hex(map, "code_sha", 40)?;
    strings(map, "evidence_refs")?;
    for check in array(map, "checks")?.unwrap_or_default() {
        let check = object(check, "checks")?;
        keys(
            check,
            &["command", "exit_code", "result", "output_ref"],
            &["command", "exit_code"],
            "checks",
        )?;
        if strings(check, "command")?.unwrap_or_default().is_empty() {
            return Err(GraphWireError::Shape("command"));
        }
        if !matches!(check.get("exit_code"), Some(V::Number(n)) if integer_at_least_any(n)) {
            return Err(GraphWireError::Shape("exit_code"));
        }
        one_of(check, "result", &CHECK_RESULTS)?;
        string(check, "output_ref")?;
    }
    string(map, "produced_at")?;
    Ok(())
}

/// Any integer, including negative ones.
fn integer_at_least_any(n: &OrchestrationJsonNumberV1) -> bool {
    let text = n.as_str();
    integer_at_least(text.strip_prefix('-').unwrap_or(text), 0)
}

// --- operational conversions ---------------------------------------------

fn s(value: &str) -> V {
    V::String(value.to_owned())
}

fn num(lexeme: &str) -> V {
    V::Number(
        OrchestrationJsonNumberV1::try_new(lexeme).expect("canonical decimal is a JSON number"),
    )
}

fn text<'a>(map: &'a Map, field: &str) -> &'a str {
    match map.get(field) {
        Some(V::String(s)) => s,
        _ => unreachable!("validated required string"),
    }
}

fn opt_text<'a>(map: &'a Map, field: &str) -> Option<&'a str> {
    match map.get(field) {
        Some(V::String(s)) => Some(s),
        _ => None,
    }
}

fn opt_strings(map: &Map, field: &str) -> Option<Vec<String>> {
    match map.get(field) {
        Some(V::Array(items)) => Some(
            items
                .iter()
                .map(|item| match item {
                    V::String(s) => s.clone(),
                    _ => unreachable!("validated string array"),
                })
                .collect(),
        ),
        _ => None,
    }
}

fn lexeme<'a>(map: &'a Map, field: &str) -> Option<&'a str> {
    match map.get(field) {
        Some(V::Number(n)) => Some(n.as_str()),
        _ => None,
    }
}

fn version(lexeme: &str, field: &'static str) -> WireResult<GraphVersionV1> {
    GraphVersionV1::try_new(lexeme).map_err(|_| GraphWireError::Unsupported(field))
}

fn date_time(map: &Map, field: &'static str) -> WireResult<OrchestrationDateTimeV1> {
    OrchestrationDateTimeV1::try_new(text(map, field))
        .map_err(|_| GraphWireError::Unsupported(field))
}

fn object_value(map: Map) -> V {
    V::Object(OrchestrationJsonObjectV1::new(map))
}

fn put(map: &mut Map, field: &str, value: Option<V>) {
    if let Some(value) = value {
        map.insert(field.to_owned(), value);
    }
}

impl GraphNodeWireV1 {
    /// Converts to an operational node; non-canonical integers are unsupported.
    pub fn to_graph_node(&self) -> WireResult<GraphNode> {
        node_from_map(self.0.as_map())
    }

    /// Encodes an operational node; absent fields stay absent.
    pub fn from_graph_node(node: &GraphNode) -> Self {
        Self(OrchestrationJsonObjectV1::new(node_to_map(node)))
    }
}

pub(crate) fn node_from_map(map: &Map) -> WireResult<GraphNode> {
    let state = GraphNodeState::parse(text(map, "state")).expect("validated state");
    let mut node = GraphNode::new(
        text(map, "node_id"),
        text(map, "graph_id"),
        GraphNodeKind::new(text(map, "kind").to_owned())?,
        state,
    )?;
    if let Some(title) = opt_text(map, "title") {
        node = node.with_title(title);
    }
    if let Some(parent) = opt_text(map, "parent_node_id") {
        node = node.with_parent_node_id(parent)?;
    }
    if let Some(attempt) = lexeme(map, "attempt_number") {
        node = node.with_attempt_number(
            GraphNodeAttemptNumber::try_new(attempt)
                .map_err(|_| GraphWireError::Unsupported("attempt_number"))?,
        );
    }
    if let Some(capabilities) = opt_strings(map, "required_capabilities") {
        node = node.with_required_capabilities(
            capabilities
                .into_iter()
                .map(CapabilityName::new)
                .collect::<Result<_, _>>()?,
        );
    }
    if let Some(value) = opt_text(map, "task_capsule_ref") {
        node = node.with_task_capsule_ref(value);
    }
    if let Some(value) = opt_text(map, "workstream_id") {
        node = node.with_workstream_id(value)?;
    }
    if let Some(value) = opt_text(map, "code_sha") {
        node = node.with_code_sha(value)?;
    }
    if let Some(value) = opt_text(map, "workspace_id") {
        node = node.with_workspace_id(value)?;
    }
    if let Some(value) = opt_text(map, "result_ref") {
        node = node.with_result_ref(value);
    }
    match map.get("locked_reason") {
        Some(V::Null) => node = node.with_null_locked_reason(),
        Some(V::String(reason)) => node = node.with_locked_reason(reason.clone()),
        _ => {}
    }
    if let Some(created) = lexeme(map, "created_in_version") {
        node = node.with_created_in_version(version(created, "created_in_version")?);
    }
    Ok(node)
}

fn node_to_map(node: &GraphNode) -> Map {
    let mut map = Map::new();
    map.insert("node_id".into(), s(node.node_id()));
    map.insert("graph_id".into(), s(node.graph_id()));
    map.insert("kind".into(), s(node.kind().as_str()));
    map.insert("state".into(), s(node.state().as_str()));
    put(&mut map, "title", node.title().map(s));
    put(&mut map, "parent_node_id", node.parent_node_id().map(s));
    put(
        &mut map,
        "attempt_number",
        node.attempt_number().map(|a| num(a.as_str())),
    );
    put(
        &mut map,
        "required_capabilities",
        node.required_capabilities()
            .map(|caps| V::Array(caps.iter().map(|c| s(c.as_str())).collect())),
    );
    put(&mut map, "task_capsule_ref", node.task_capsule_ref().map(s));
    put(&mut map, "workstream_id", node.workstream_id().map(s));
    put(&mut map, "code_sha", node.code_sha().map(s));
    put(&mut map, "workspace_id", node.workspace_id().map(s));
    put(&mut map, "result_ref", node.result_ref().map(s));
    put(
        &mut map,
        "locked_reason",
        node.locked_reason().map(|reason| reason.map_or(V::Null, s)),
    );
    put(
        &mut map,
        "created_in_version",
        node.created_in_version().map(|v| num(v.as_str())),
    );
    map
}

/// Decodes one `GraphNode` from an already parsed value (used by apply-v1
/// operation detail).
pub(crate) fn graph_node_from_value(value: &V) -> WireResult<GraphNode> {
    let map = object(value, "node")?;
    validate_node(map)?;
    node_from_map(map)
}

/// Decodes one `GraphEdge` from an already parsed value.
pub(crate) fn graph_edge_from_value(value: &V) -> WireResult<GraphEdge> {
    object(value, "edge")?;
    validate_edge(value)
}

/// Decodes one `GraphNodeResult` from an already parsed value.
pub(crate) fn graph_node_result_from_value(value: &V) -> WireResult<GraphNodeResult> {
    GraphNodeResultWireV1::from_value(value.clone())?.to_graph_node_result()
}

pub(crate) fn edge_to_value(edge: &GraphEdge) -> V {
    V::parse_json(&edge.to_json().expect("edge serialization cannot fail"))
        .expect("edge JSON is valid")
}

impl ExecutionGraphWireV1 {
    /// Builds the validated envelope. Graph containment, duplicate ids,
    /// endpoints and precedence cycles refuse through the existing graph API.
    /// A supplied `resulting_digest` is recorded, not verified (as `bind`).
    pub fn to_envelope(&self) -> WireResult<ExecutionGraphEnvelope> {
        let map = self.0.as_map();
        let nodes = match &map["nodes"] {
            V::Array(items) => items
                .iter()
                .map(|n| node_from_map(object(n, "nodes")?))
                .collect::<WireResult<Vec<_>>>()?,
            _ => unreachable!("validated nodes"),
        };
        let edges = match &map["edges"] {
            V::Array(items) => items
                .iter()
                .map(validate_edge)
                .collect::<WireResult<Vec<_>>>()?,
            _ => unreachable!("validated edges"),
        };
        let graph = ExecutionGraph::from_parts(text(map, "graph_id"), nodes, edges)?;
        let mut fields = ExecutionGraphEnvelopeFields::new(
            text(map, "project_id"),
            text(map, "goal_id"),
            version(
                lexeme(map, "graph_version").expect("validated"),
                "graph_version",
            )?,
            date_time(map, "created_at")?,
            text(map, "policy_id"),
        );
        match map.get("parent_version") {
            Some(V::Null) => fields = fields.with_null_parent_version(),
            Some(V::Number(n)) => {
                fields = fields.with_parent_version(version(n.as_str(), "parent_version")?)
            }
            _ => {}
        }
        if let Some(digest) = opt_text(map, "resulting_digest") {
            fields = fields.with_resulting_digest(digest);
        }
        if let Some(refs) = opt_strings(map, "compiled_from") {
            fields = fields.with_compiled_from(refs);
        }
        if let Some(epoch) = lexeme(map, "context_epoch") {
            if !epoch.bytes().all(|b| b.is_ascii_digit()) {
                return Err(GraphWireError::Unsupported("context_epoch"));
            }
            fields = fields.with_context_epoch(
                GoalEvaluationContextEpoch::from_decimal(epoch)
                    .map_err(|_| GraphWireError::Unsupported("context_epoch"))?,
            );
        }
        ExecutionGraphEnvelope::bind(&graph, fields).map_err(GraphWireError::Envelope)
    }

    /// Encodes an envelope with nodes and edges in ascending id order.
    pub fn from_envelope(envelope: &ExecutionGraphEnvelope) -> Self {
        let mut map = Map::new();
        map.insert("graph_id".into(), s(envelope.graph_id()));
        map.insert(
            "graph_version".into(),
            num(envelope.graph_version().as_str()),
        );
        put(
            &mut map,
            "parent_version",
            envelope
                .parent_version()
                .map(|p| p.map_or(V::Null, |v| num(v.as_str()))),
        );
        map.insert("project_id".into(), s(envelope.project_id()));
        map.insert("goal_id".into(), s(envelope.goal_id()));
        map.insert(
            "nodes".into(),
            V::Array(
                envelope
                    .graph()
                    .nodes()
                    .map(|n| object_value(node_to_map(n)))
                    .collect(),
            ),
        );
        map.insert(
            "edges".into(),
            V::Array(envelope.graph().edges().map(edge_to_value).collect()),
        );
        map.insert("policy_id".into(), s(envelope.policy_id()));
        put(
            &mut map,
            "resulting_digest",
            envelope.resulting_digest().map(s),
        );
        map.insert("created_at".into(), s(envelope.created_at().as_str()));
        put(
            &mut map,
            "compiled_from",
            envelope
                .compiled_from()
                .map(|refs| V::Array(refs.iter().map(|r| s(r)).collect())),
        );
        put(
            &mut map,
            "context_epoch",
            envelope.context_epoch().map(|e| num(e.decimal_digits())),
        );
        Self(OrchestrationJsonObjectV1::new(map))
    }
}

impl GraphMutationWireV1 {
    /// Converts to the auditable record. Decoding and conversion never
    /// execute the mutation; detail objects are kept verbatim.
    pub fn to_mutation(&self) -> WireResult<GraphMutation> {
        let map = self.0.as_map();
        let actor = object(&map["actor"], "actor")?;
        let actor = GraphMutationActor::try_new(
            text(actor, "role"),
            opt_text(actor, "role_id").map(str::to_owned),
        )?;
        let operations = match &map["operations"] {
            V::Array(items) => items
                .iter()
                .map(|op| {
                    let op = object(op, "operations")?;
                    let kind = GraphMutationOperationKind::ALL
                        .into_iter()
                        .find(|k| k.as_str() == text(op, "op"))
                        .expect("validated op");
                    let detail = match op.get("detail") {
                        Some(V::Object(detail)) => Some(detail.clone()),
                        _ => None,
                    };
                    Ok(GraphMutationOperation::try_new(
                        kind,
                        opt_text(op, "node_id").map(str::to_owned),
                        opt_text(op, "edge_id").map(str::to_owned),
                        detail,
                    )?)
                })
                .collect::<WireResult<Vec<_>>>()?,
            _ => unreachable!("validated operations"),
        };
        Ok(GraphMutation::try_new(
            text(map, "mutation_id"),
            text(map, "graph_id"),
            version(
                lexeme(map, "parent_version").expect("validated"),
                "parent_version",
            )?,
            version(
                lexeme(map, "resulting_version").expect("validated"),
                "resulting_version",
            )?,
            actor,
            text(map, "reason"),
            date_time(map, "created_at")?,
            operations,
            opt_text(map, "resulting_digest").map(str::to_owned),
        )?)
    }

    pub fn from_mutation(mutation: &GraphMutation) -> Self {
        let mut actor = Map::new();
        actor.insert("role".into(), s(mutation.actor().role()));
        put(&mut actor, "role_id", mutation.actor().role_id().map(s));
        let operations = mutation
            .operations()
            .iter()
            .map(|op| {
                let mut map = Map::new();
                map.insert("op".into(), s(op.op().as_str()));
                put(&mut map, "node_id", op.node_id().map(s));
                put(&mut map, "edge_id", op.edge_id().map(s));
                put(&mut map, "detail", op.detail().cloned().map(V::Object));
                object_value(map)
            })
            .collect();
        let mut map = Map::new();
        map.insert("mutation_id".into(), s(mutation.mutation_id()));
        map.insert("graph_id".into(), s(mutation.graph_id()));
        map.insert(
            "parent_version".into(),
            num(mutation.parent_version().as_str()),
        );
        map.insert(
            "resulting_version".into(),
            num(mutation.resulting_version().as_str()),
        );
        map.insert("actor".into(), object_value(actor));
        map.insert("reason".into(), s(mutation.reason()));
        map.insert("operations".into(), V::Array(operations));
        put(
            &mut map,
            "resulting_digest",
            mutation.resulting_digest().map(s),
        );
        map.insert("created_at".into(), s(mutation.created_at().as_str()));
        Self(OrchestrationJsonObjectV1::new(map))
    }
}

impl GraphSnapshotWireV1 {
    /// Unknown node-state strings are schema-valid but unsupported here.
    pub fn to_snapshot(&self) -> WireResult<GraphSnapshot> {
        let map = self.0.as_map();
        let node_states = match &map["node_states"] {
            V::Array(items) => items
                .iter()
                .map(|entry| {
                    let entry = object(entry, "node_states")?;
                    let state = GraphNodeState::parse(text(entry, "state"))
                        .map_err(|_| GraphWireError::Unsupported("state"))?;
                    let kind = opt_text(entry, "kind")
                        .map(|k| GraphNodeKind::new(k.to_owned()))
                        .transpose()?;
                    let locked = match entry.get("locked") {
                        Some(V::Boolean(b)) => Some(*b),
                        _ => None,
                    };
                    Ok(GraphSnapshotNodeState::try_new(
                        text(entry, "node_id"),
                        state,
                        kind,
                        locked,
                        opt_text(entry, "code_sha").map(str::to_owned),
                    )?)
                })
                .collect::<WireResult<Vec<_>>>()?,
            _ => unreachable!("validated node_states"),
        };
        let summary = match map.get("summary") {
            Some(V::Object(summary)) => Some(summary.clone()),
            _ => None,
        };
        Ok(GraphSnapshot::try_new(
            text(map, "graph_id"),
            version(
                lexeme(map, "graph_version").expect("validated"),
                "graph_version",
            )?,
            date_time(map, "captured_at")?,
            node_states,
            opt_text(map, "resulting_digest").map(str::to_owned),
            summary,
        )?)
    }

    pub fn from_snapshot(snapshot: &GraphSnapshot) -> Self {
        let node_states = snapshot
            .node_states()
            .iter()
            .map(|entry| {
                let mut map = Map::new();
                map.insert("node_id".into(), s(entry.node_id()));
                map.insert("state".into(), s(entry.state().as_str()));
                put(&mut map, "kind", entry.kind().map(|k| s(k.as_str())));
                put(&mut map, "locked", entry.locked().map(V::Boolean));
                put(&mut map, "code_sha", entry.code_sha().map(s));
                object_value(map)
            })
            .collect();
        let mut map = Map::new();
        map.insert("graph_id".into(), s(snapshot.graph_id()));
        map.insert(
            "graph_version".into(),
            num(snapshot.graph_version().as_str()),
        );
        map.insert("captured_at".into(), s(snapshot.captured_at().as_str()));
        put(
            &mut map,
            "resulting_digest",
            snapshot.resulting_digest().map(s),
        );
        map.insert("node_states".into(), V::Array(node_states));
        put(
            &mut map,
            "summary",
            snapshot.summary().cloned().map(V::Object),
        );
        Self(OrchestrationJsonObjectV1::new(map))
    }
}

impl GraphNodeResultWireV1 {
    /// Non-canonical exit codes (`1.0`, `-0`, `1e2`) are unsupported.
    pub fn to_graph_node_result(&self) -> WireResult<GraphNodeResult> {
        let map = self.0.as_map();
        let outcome = GraphNodeResultOutcome::ALL
            .into_iter()
            .find(|o| o.as_str() == text(map, "outcome"))
            .expect("validated outcome");
        let mut result = GraphNodeResult::try_new(
            text(map, "node_id"),
            text(map, "graph_id"),
            outcome,
            date_time(map, "produced_at")?,
        )?;
        if let Some(sha) = opt_text(map, "code_sha") {
            result = result.with_code_sha(sha)?;
        }
        if let Some(id) = opt_text(map, "attempt_id") {
            result = result.with_attempt_id(id)?;
        }
        if let Some(refs) = opt_strings(map, "evidence_refs") {
            result = result.with_evidence_refs(refs);
        }
        if let Some(V::Array(checks)) = map.get("checks") {
            let checks = checks
                .iter()
                .map(|check| {
                    let check = object(check, "checks")?;
                    let exit_code = lexeme(check, "exit_code").expect("validated");
                    let result = opt_text(check, "result").map(|r| {
                        GraphNodeCheckResult::ALL
                            .into_iter()
                            .find(|c| c.as_str() == r)
                            .expect("validated check result")
                    });
                    GraphNodeCheck::try_new(
                        opt_strings(check, "command").expect("validated"),
                        exit_code,
                        result,
                        opt_text(check, "output_ref").map(str::to_owned),
                    )
                    .map_err(|error| match error {
                        GraphNodeResultError::InvalidExitCode => {
                            GraphWireError::Unsupported("exit_code")
                        }
                        other => GraphWireError::Result(other),
                    })
                })
                .collect::<WireResult<Vec<_>>>()?;
            result = result.with_checks(checks);
        }
        if let Some(id) = opt_text(map, "review_id") {
            result = result.with_review_id(id)?;
        }
        if let Some(id) = opt_text(map, "routing_decision_id") {
            result = result.with_routing_decision_id(id)?;
        }
        Ok(result)
    }

    pub fn from_graph_node_result(result: &GraphNodeResult) -> Self {
        let mut map = Map::new();
        map.insert("node_id".into(), s(result.node_id()));
        map.insert("graph_id".into(), s(result.graph_id()));
        map.insert("outcome".into(), s(result.outcome().as_str()));
        put(&mut map, "code_sha", result.code_sha().map(s));
        put(&mut map, "attempt_id", result.attempt_id().map(s));
        put(
            &mut map,
            "evidence_refs",
            result
                .evidence_refs()
                .map(|refs| V::Array(refs.iter().map(|r| s(r)).collect())),
        );
        put(
            &mut map,
            "checks",
            result.checks().map(|checks| {
                V::Array(
                    checks
                        .iter()
                        .map(|check| {
                            let mut map = Map::new();
                            map.insert(
                                "command".into(),
                                V::Array(check.command().iter().map(|c| s(c)).collect()),
                            );
                            map.insert("exit_code".into(), num(check.exit_code()));
                            put(&mut map, "result", check.result().map(|r| s(r.as_str())));
                            put(&mut map, "output_ref", check.output_ref().map(s));
                            object_value(map)
                        })
                        .collect(),
                )
            }),
        );
        put(&mut map, "review_id", result.review_id().map(s));
        put(
            &mut map,
            "routing_decision_id",
            result.routing_decision_id().map(s),
        );
        map.insert("produced_at".into(), s(result.produced_at().as_str()));
        Self(OrchestrationJsonObjectV1::new(map))
    }
}
