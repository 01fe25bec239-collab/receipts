//! Strict JSON encoding of [`GraphEdge`] for the frozen `GraphEdge` schema.
//!
//! Decoding builds an edge only through [`GraphEdge::precedence`],
//! [`GraphEdge::control`] and [`GraphEdge::with_note`], so identifier limits
//! and class exclusivity are the existing constructor rules. Decoding is
//! purely structural: it never looks at a graph, so endpoint existence,
//! `graph_id` containment, duplicate ids and precedence cycles are decided
//! later by [`ExecutionGraph::add_edge`](crate::ExecutionGraph::add_edge), and
//! a decoded edge carries no dispatch, actor or authentication authority.
//!
//! The root must be a JSON object; unknown, duplicate and `null` fields,
//! positional arrays, mixed or missing class kinds, and unknown enum values
//! refuse. Strings are data: nothing is trimmed or normalized.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::edge::{ControlKind, EdgeClass, GraphEdge, PrecedenceKind};

const PRECEDENCE_KINDS: [PrecedenceKind; 3] = [
    PrecedenceKind::RequiresAccepted,
    PrecedenceKind::RequiresIntegrated,
    PrecedenceKind::RequiresInterface,
];
const CONTROL_KINDS: [ControlKind; 6] = [
    ControlKind::OnPass,
    ControlKind::OnReject,
    ControlKind::OnFailure,
    ControlKind::OnBlocked,
    ControlKind::Escalate,
    ControlKind::ExpandsInto,
];

impl GraphEdge {
    /// Decodes one edge from strict JSON; trailing input other than JSON
    /// whitespace refuses. Errors never echo field values.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// Encodes the edge, omitting an absent `note` and the other class's kind.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

#[derive(Serialize)]
struct EdgeOut<'a> {
    edge_id: &'a str,
    graph_id: &'a str,
    from_node: &'a str,
    to_node: &'a str,
    edge_class: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    precedence_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    control_kind: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<&'a str>,
}

impl Serialize for GraphEdge {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        EdgeOut {
            edge_id: self.edge_id(),
            graph_id: self.graph_id(),
            from_node: self.from_node(),
            to_node: self.to_node(),
            edge_class: self.class().as_str(),
            precedence_kind: self.precedence_kind().map(PrecedenceKind::as_str),
            control_kind: self.control_kind().map(ControlKind::as_str),
            note: self.note(),
        }
        .serialize(serializer)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EdgeIn {
    edge_id: String,
    graph_id: String,
    from_node: String,
    to_node: String,
    edge_class: String,
    #[serde(default, deserialize_with = "present")]
    precedence_kind: Option<String>,
    #[serde(default, deserialize_with = "present")]
    control_kind: Option<String>,
    #[serde(default, deserialize_with = "present")]
    note: Option<String>,
}

// Missing uses the field default; present null fails because T is not nullable.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

// Serde struct derives also admit sequences; JSON Schema requires objects.
fn object<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Object<T>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Object<T> {
        type Value = T;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a JSON object")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(self, map: A) -> Result<T, A::Error> {
            T::deserialize(serde::de::value::MapAccessDeserializer::new(map))
        }
    }
    deserializer.deserialize_map(Object(std::marker::PhantomData))
}

impl<'de> Deserialize<'de> for GraphEdge {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let EdgeIn {
            edge_id,
            graph_id,
            from_node,
            to_node,
            edge_class,
            precedence_kind,
            control_kind,
            note,
        } = object(deserializer)?;
        let edge = if edge_class == EdgeClass::Precedence.as_str() {
            let (Some(kind), None) = (precedence_kind, control_kind) else {
                return Err(D::Error::custom(
                    "PRECEDENCE requires precedence_kind and forbids control_kind",
                ));
            };
            let kind = PRECEDENCE_KINDS
                .into_iter()
                .find(|candidate| candidate.as_str() == kind)
                .ok_or_else(|| D::Error::custom("invalid precedence_kind"))?;
            GraphEdge::precedence(edge_id, graph_id, from_node, to_node, kind)
        } else if edge_class == EdgeClass::Control.as_str() {
            let (None, Some(kind)) = (precedence_kind, control_kind) else {
                return Err(D::Error::custom(
                    "CONTROL requires control_kind and forbids precedence_kind",
                ));
            };
            let kind = CONTROL_KINDS
                .into_iter()
                .find(|candidate| candidate.as_str() == kind)
                .ok_or_else(|| D::Error::custom("invalid control_kind"))?;
            GraphEdge::control(edge_id, graph_id, from_node, to_node, kind)
        } else {
            return Err(D::Error::custom("invalid edge_class"));
        }
        .map_err(D::Error::custom)?;
        Ok(match note {
            Some(note) => edge.with_note(note),
            None => edge,
        })
    }
}
