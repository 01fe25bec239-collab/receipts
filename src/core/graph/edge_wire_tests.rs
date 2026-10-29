//! Strict `GraphEdge` JSON: roundtrip, refusals, schema agreement, and real
//! `ExecutionGraph` insertion of decoded edges.

use serde_json::{Value, json};

use crate::edge::{ControlKind, EdgeClass, GraphEdge, PrecedenceKind};
use crate::error::GraphError;
use crate::execution_graph::ExecutionGraph;
use crate::node::{GraphNode, GraphNodeKind};
use crate::node_state::GraphNodeState;

const PRECEDENCE: [(PrecedenceKind, &str); 3] = [
    (PrecedenceKind::RequiresAccepted, "REQUIRES_ACCEPTED"),
    (PrecedenceKind::RequiresIntegrated, "REQUIRES_INTEGRATED"),
    (PrecedenceKind::RequiresInterface, "REQUIRES_INTERFACE"),
];
const CONTROL: [(ControlKind, &str); 6] = [
    (ControlKind::OnPass, "ON_PASS"),
    (ControlKind::OnReject, "ON_REJECT"),
    (ControlKind::OnFailure, "ON_FAILURE"),
    (ControlKind::OnBlocked, "ON_BLOCKED"),
    (ControlKind::Escalate, "ESCALATE"),
    (ControlKind::ExpandsInto, "EXPANDS_INTO"),
];

fn base() -> Value {
    json!({"edge_id":"e","graph_id":"g","from_node":"a","to_node":"b",
           "edge_class":"PRECEDENCE","precedence_kind":"REQUIRES_ACCEPTED"})
}

fn decode(value: &Value) -> Result<GraphEdge, serde_json::Error> {
    GraphEdge::from_json(&value.to_string())
}

fn roundtrip(edge: &GraphEdge) -> String {
    let json = edge.to_json().unwrap();
    assert_eq!(&GraphEdge::from_json(&json).unwrap(), edge, "{json}");
    json
}

#[test]
fn every_kind_roundtrips_with_absent_and_present_empty_note() {
    for (kind, name) in PRECEDENCE {
        let edge = GraphEdge::precedence("e", "g", "a", "b", kind).unwrap();
        assert_eq!(
            roundtrip(&edge),
            format!(
                r#"{{"edge_id":"e","graph_id":"g","from_node":"a","to_node":"b","edge_class":"PRECEDENCE","precedence_kind":"{name}"}}"#
            )
        );
        let empty = roundtrip(&edge.clone().with_note(""));
        assert!(empty.ends_with(r#""note":""}"#), "{empty}");
        assert_eq!(GraphEdge::from_json(&empty).unwrap().note(), Some(""));
        roundtrip(&edge.with_note(" \t\n note ✓ "));
    }
    for (kind, name) in CONTROL {
        let edge = GraphEdge::control("e", "g", "a", "b", kind).unwrap();
        assert_eq!(
            roundtrip(&edge),
            format!(
                r#"{{"edge_id":"e","graph_id":"g","from_node":"a","to_node":"b","edge_class":"CONTROL","control_kind":"{name}"}}"#
            )
        );
        assert_eq!(
            GraphEdge::from_json(&roundtrip(&edge)).unwrap().note(),
            None
        );
        let empty = roundtrip(&edge.clone().with_note(""));
        assert_eq!(GraphEdge::from_json(&empty).unwrap().note(), Some(""));
        roundtrip(&edge.with_note("n"));
    }
    // The serde impls also work inside containers, and field order is irrelevant.
    let list = r#"[{"edge_class":"CONTROL","control_kind":"ESCALATE","to_node":"b","from_node":"a","graph_id":"g","edge_id":"e"}]"#;
    let edges: Vec<GraphEdge> = serde_json::from_str(list).unwrap();
    assert_eq!(edges[0].class(), EdgeClass::Control);
    assert_eq!(edges[0].control_kind(), Some(ControlKind::Escalate));
}

#[test]
fn identifiers_keep_exact_bytes_and_enforce_scalar_bounds() {
    let max_ascii = "x".repeat(200);
    let max_two_byte = "é".repeat(200);
    let max_four_byte = "😀".repeat(200);
    for id in [
        "a",
        " ",
        "\t",
        " padded ",
        "a\nb",
        "\u{0}",
        "e\u{301}",
        &max_ascii,
        &max_two_byte,
        &max_four_byte,
    ] {
        let edge = GraphEdge::control(id, id, id, id, ControlKind::OnPass).unwrap();
        let back = GraphEdge::from_json(&roundtrip(&edge)).unwrap();
        assert_eq!(back.edge_id(), id);
        assert_eq!(back.graph_id(), id);
        assert_eq!(back.from_node(), id);
        assert_eq!(back.to_node(), id);
    }
    for field in ["edge_id", "graph_id", "from_node", "to_node"] {
        for (id, ok) in [
            ("".to_owned(), false),
            ("x".repeat(201), false),
            ("😀".repeat(201), false),
            ("😀".repeat(200), true),
        ] {
            let mut v = base();
            v[field] = json!(id);
            assert_eq!(decode(&v).is_ok(), ok, "{field} {}", id.chars().count());
        }
    }
    // Escapes decode to the same scalar; a lone surrogate is not a scalar.
    let escaped = r#"{"edge_id":"é😀","graph_id":"g","from_node":"a","to_node":"b","edge_class":"CONTROL","control_kind":"ON_PASS"}"#;
    assert_eq!(GraphEdge::from_json(escaped).unwrap().edge_id(), "é😀");
    assert!(GraphEdge::from_json(&escaped.replace(r"😀", r"\ud83d")).is_err());
}

#[test]
fn malformed_inputs_refuse() {
    let valid = base().to_string();
    assert!(GraphEdge::from_json(&format!(" \n{valid}\t\n")).is_ok());
    for bad in [
        String::new(),
        "   ".into(),
        format!("{valid} x"),
        format!("{valid}{valid}"),
        format!("{valid},"),
        valid[..valid.len() - 1].to_owned(),
        format!("\u{feff}{valid}"),
        "null".into(),
        "true".into(),
        "7".into(),
        r#""PRECEDENCE""#.into(),
        "{}".into(),
        "[]".into(),
        r#"["e","g","a","b","PRECEDENCE","REQUIRES_ACCEPTED"]"#.into(),
        r#"["e","g","a","b","PRECEDENCE","REQUIRES_ACCEPTED",null]"#.into(),
        r#"[{"edge_id":"e","graph_id":"g","from_node":"a","to_node":"b","edge_class":"CONTROL","control_kind":"ON_PASS"}]"#.into(),
    ] {
        assert!(GraphEdge::from_json(&bad).is_err(), "{bad}");
    }
}

#[test]
fn required_unknown_null_and_mistyped_fields_refuse() {
    for field in ["edge_id", "graph_id", "from_node", "to_node", "edge_class"] {
        let mut v = base();
        v.as_object_mut().unwrap().remove(field);
        assert!(decode(&v).is_err(), "missing {field}");
    }
    let mut v = base();
    v.as_object_mut().unwrap().remove("precedence_kind");
    assert!(decode(&v).is_err(), "missing kind");

    for field in [
        "edge_id",
        "graph_id",
        "from_node",
        "to_node",
        "edge_class",
        "precedence_kind",
        "note",
    ] {
        for wrong in [json!(null), json!(1), json!(true), json!([]), json!({})] {
            let mut v = base();
            v[field] = wrong.clone();
            assert!(decode(&v).is_err(), "{field}={wrong}");
        }
    }
    let mut v = base();
    v["extra"] = json!("x");
    assert!(decode(&v).is_err());
    let mut v = base();
    v["extra"] = json!(null);
    assert!(decode(&v).is_err());
    let mut v = base();
    v["Note"] = json!("x");
    assert!(decode(&v).is_err(), "field names are case-sensitive");
}

#[test]
fn duplicate_fields_refuse_even_when_null_or_equal() {
    let prefix = r#"{"edge_id":"e","graph_id":"g","from_node":"a","to_node":"b","edge_class":"PRECEDENCE","precedence_kind":"REQUIRES_ACCEPTED""#;
    assert!(GraphEdge::from_json(&format!("{prefix}}}")).is_ok());
    for dup in [
        r#","edge_id":"e""#,
        r#","edge_id":"other""#,
        r#","edge_id":null"#,
        r#","graph_id":"g""#,
        r#","from_node":"a""#,
        r#","to_node":"b""#,
        r#","edge_class":"PRECEDENCE""#,
        r#","edge_class":"CONTROL""#,
        r#","precedence_kind":"REQUIRES_ACCEPTED""#,
        r#","precedence_kind":"REQUIRES_INTERFACE""#,
        r#","precedence_kind":null"#,
        r#","note":"a","note":"b""#,
        r#","note":"","note":null"#,
        r#","control_kind":null,"control_kind":null"#,
    ] {
        assert!(
            GraphEdge::from_json(&format!("{prefix}{dup}}}")).is_err(),
            "{dup}"
        );
    }
}

#[test]
fn class_and_kind_must_match_exactly() {
    let mixed = |class: &str, precedence: Option<&str>, control: Option<&str>| {
        let mut v = base();
        v["edge_class"] = json!(class);
        let map = v.as_object_mut().unwrap();
        map.remove("precedence_kind");
        if let Some(kind) = precedence {
            map.insert("precedence_kind".into(), json!(kind));
        }
        if let Some(kind) = control {
            map.insert("control_kind".into(), json!(kind));
        }
        decode(&v)
    };
    assert!(mixed("PRECEDENCE", Some("REQUIRES_ACCEPTED"), None).is_ok());
    assert!(mixed("CONTROL", None, Some("ON_PASS")).is_ok());
    for (class, precedence, control) in [
        ("PRECEDENCE", None, None),
        ("CONTROL", None, None),
        ("PRECEDENCE", None, Some("ON_PASS")),
        ("CONTROL", Some("REQUIRES_ACCEPTED"), None),
        ("PRECEDENCE", Some("REQUIRES_ACCEPTED"), Some("ON_PASS")),
        ("CONTROL", Some("REQUIRES_ACCEPTED"), Some("ON_PASS")),
        // Kind values from the other class's enum.
        ("PRECEDENCE", Some("ON_PASS"), None),
        ("CONTROL", None, Some("REQUIRES_ACCEPTED")),
        // Unknown, case-changed, padded and empty enum text.
        ("precedence", Some("REQUIRES_ACCEPTED"), None),
        ("Control", None, Some("ON_PASS")),
        ("PRECEDENCE ", Some("REQUIRES_ACCEPTED"), None),
        ("", Some("REQUIRES_ACCEPTED"), None),
        ("OTHER", Some("REQUIRES_ACCEPTED"), None),
        ("OTHER", None, Some("ON_PASS")),
        ("PRECEDENCE", Some("requires_accepted"), None),
        ("PRECEDENCE", Some("REQUIRES_ACCEPTED "), None),
        ("PRECEDENCE", Some(""), None),
        ("PRECEDENCE", Some("REQUIRES_OTHER"), None),
        ("CONTROL", None, Some("on_pass")),
        ("CONTROL", None, Some("ON_PASS\n")),
        ("CONTROL", None, Some("")),
        ("CONTROL", None, Some("ON_OTHER")),
    ] {
        assert!(
            mixed(class, precedence, control).is_err(),
            "{class} {precedence:?} {control:?}"
        );
    }
}

#[test]
fn refusals_do_not_echo_values() {
    for (field, value) in [
        ("edge_class", "SECRET_VALUE"),
        ("precedence_kind", "SECRET_VALUE"),
        ("edge_id", ""),
        ("note", "unused"),
    ] {
        let mut v = base();
        v[field] = json!(value);
        if let Err(error) = decode(&v) {
            assert!(!error.to_string().contains("SECRET_VALUE"), "{error}");
        }
    }
    let mut v = base();
    v["edge_id"] = json!("S".repeat(201));
    let error = decode(&v).unwrap_err().to_string();
    assert!(
        error.contains("edge_id") && !error.contains("SSS"),
        "{error}"
    );
    let mut v = base();
    v["edge_class"] = json!("CONTROL");
    v["note"] = json!("SECRET_VALUE");
    assert!(!decode(&v).unwrap_err().to_string().contains("SECRET_VALUE"));
}

// --- Independent schema agreement ----------------------------------------

fn schema() -> jsonschema::Validator {
    let schema: Value =
        serde_json::from_str(include_str!("../../../schemas/GraphEdge.schema.json")).unwrap();
    jsonschema::options().build(&schema).unwrap()
}

#[test]
fn decoder_agrees_with_unchanged_local_schema() {
    let schema = schema();
    let mut cases: Vec<Value> = vec![base()];
    let with = |edit: &dyn Fn(&mut Value)| {
        let mut v = base();
        edit(&mut v);
        v
    };
    // Valid: every kind, note absent/empty/text, boundary ids.
    for (_, name) in PRECEDENCE {
        cases.push(with(&|v| v["precedence_kind"] = json!(name)));
    }
    for (_, name) in CONTROL {
        cases.push(with(&|v| {
            v["edge_class"] = json!("CONTROL");
            v.as_object_mut().unwrap().remove("precedence_kind");
            v["control_kind"] = json!(name);
        }));
    }
    for note in ["", " ", "text ✓", "\n"] {
        cases.push(with(&|v| v["note"] = json!(note)));
    }
    for id in ["x".repeat(200), "😀".repeat(200), " ".into(), "a b".into()] {
        cases.push(with(&|v| v["edge_id"] = json!(id)));
    }
    // Invalid: every shape the decoder must refuse (duplicate keys cannot be
    // expressed as a JSON value; they are covered by the textual test above).
    for field in ["edge_id", "graph_id", "from_node", "to_node", "edge_class"] {
        cases.push(with(&|v| {
            v.as_object_mut().unwrap().remove(field);
        }));
    }
    for field in ["edge_id", "graph_id", "from_node", "to_node"] {
        cases.push(with(&|v| v[field] = json!("")));
        cases.push(with(&|v| v[field] = json!("x".repeat(201))));
        cases.push(with(&|v| v[field] = json!("😀".repeat(201))));
    }
    for field in ["edge_id", "edge_class", "precedence_kind", "note"] {
        for wrong in [json!(null), json!(1), json!([]), json!({})] {
            cases.push(with(&|v| v[field] = wrong.clone()));
        }
    }
    cases.push(with(&|v| v["extra"] = json!(null)));
    cases.push(with(&|v| v["control_kind"] = json!("ON_PASS")));
    cases.push(with(&|v| v["control_kind"] = json!(null)));
    cases.push(with(&|v| {
        v.as_object_mut().unwrap().remove("precedence_kind");
    }));
    cases.push(with(&|v| v["edge_class"] = json!("CONTROL")));
    cases.push(with(&|v| {
        v["edge_class"] = json!("CONTROL");
        v["control_kind"] = json!("ON_PASS");
    }));
    cases.push(with(&|v| v["edge_class"] = json!("precedence")));
    cases.push(with(&|v| v["precedence_kind"] = json!("ON_PASS")));
    cases.push(with(&|v| v["precedence_kind"] = json!("requires_accepted")));
    cases.push(json!([]));
    cases.push(json!([
        "e",
        "g",
        "a",
        "b",
        "PRECEDENCE",
        "REQUIRES_ACCEPTED"
    ]));
    cases.push(json!(null));
    cases.push(json!("PRECEDENCE"));

    let (mut accepted, mut refused) = (0, 0);
    for case in &cases {
        let valid = schema.is_valid(case);
        let decoded = decode(case);
        assert_eq!(decoded.is_ok(), valid, "{case}");
        if let Ok(edge) = decoded {
            accepted += 1;
            // Generated output is itself schema-valid and decodes to the same edge.
            let out: Value = serde_json::from_str(&roundtrip(&edge)).unwrap();
            assert!(schema.is_valid(&out), "{out}");
        } else {
            refused += 1;
        }
    }
    assert!(accepted >= 15 && refused >= 40, "{accepted}/{refused}");
}

// --- Decoded edges through real graph insertion ---------------------------

fn node(id: &str, graph: &str) -> GraphNode {
    GraphNode::new(id, graph, GraphNodeKind::TASK, GraphNodeState::Planned).unwrap()
}

fn graph_ab() -> ExecutionGraph {
    let mut graph = ExecutionGraph::new("g").unwrap();
    for id in ["a", "b", "c"] {
        graph.add_node(node(id, "g")).unwrap();
    }
    graph
}

fn edge_json(id: &str, graph: &str, from: &str, to: &str, class: &str) -> GraphEdge {
    let kind = if class == "CONTROL" {
        r#""control_kind":"ON_REJECT""#
    } else {
        r#""precedence_kind":"REQUIRES_INTEGRATED""#
    };
    GraphEdge::from_json(&format!(
        r#"{{"edge_id":"{id}","graph_id":"{graph}","from_node":"{from}","to_node":"{to}","edge_class":"{class}",{kind}}}"#
    ))
    .unwrap()
}

#[test]
fn decoding_alone_checks_no_graph_policy() {
    // Unknown endpoints, a foreign graph and a self-loop all decode.
    let edge = edge_json("e", "no-such-graph", "ghost", "ghost", "PRECEDENCE");
    assert_eq!(edge.from_node(), "ghost");
}

#[test]
fn decoded_edges_insert_and_keep_control_cycle_exclusion() {
    let mut graph = graph_ab();
    graph
        .add_edge(edge_json("p1", "g", "a", "b", "PRECEDENCE"))
        .unwrap();
    graph
        .add_edge(edge_json("p2", "g", "b", "c", "PRECEDENCE"))
        .unwrap();
    // A control edge that would close a loop is accepted; it is not traversed.
    graph
        .add_edge(edge_json("c1", "g", "c", "a", "CONTROL"))
        .unwrap();
    graph
        .add_edge(edge_json("c2", "g", "a", "a", "CONTROL"))
        .unwrap();
    assert!(!graph.has_precedence_cycle());
    assert_eq!(graph.precedence_edges().count(), 2);
    assert_eq!(graph.control_edges().count(), 2);
    assert_eq!(
        graph.edge("p1").unwrap().precedence_kind(),
        Some(PrecedenceKind::RequiresIntegrated)
    );
    assert_eq!(
        graph.edge("c1").unwrap().control_kind(),
        Some(ControlKind::OnReject)
    );
}

#[test]
fn decoded_edges_keep_existing_semantic_refusals_without_mutation() {
    let mut graph = graph_ab();
    graph
        .add_edge(edge_json("p1", "g", "a", "b", "PRECEDENCE"))
        .unwrap();
    graph
        .add_edge(edge_json("p2", "g", "b", "c", "PRECEDENCE"))
        .unwrap();
    let before = graph.clone();

    let refusals = [
        (
            edge_json("x", "other", "a", "b", "PRECEDENCE"),
            GraphError::ChildGraphIdMismatch {
                child: "edge",
                child_id: "x".into(),
                graph_id: "g".into(),
                child_graph_id: "other".into(),
            },
        ),
        (
            edge_json("x", "g", "a", "ghost", "CONTROL"),
            GraphError::UnknownNodeReference {
                edge_id: "x".into(),
                node_id: "ghost".into(),
            },
        ),
        (
            edge_json("p1", "g", "c", "a", "CONTROL"),
            GraphError::DuplicateEdgeId {
                edge_id: "p1".into(),
            },
        ),
        (
            edge_json("x", "g", "c", "a", "PRECEDENCE"),
            GraphError::PrecedenceCycleRejected {
                candidate_edge_id: "x".into(),
                candidate_from_node: "c".into(),
                candidate_to_node: "a".into(),
                closing_path_nodes: vec!["a".into(), "b".into(), "c".into()],
                closing_path_edge_ids: vec!["p1".into(), "p2".into()],
            },
        ),
        (
            edge_json("x", "g", "a", "a", "PRECEDENCE"),
            GraphError::PrecedenceCycleRejected {
                candidate_edge_id: "x".into(),
                candidate_from_node: "a".into(),
                candidate_to_node: "a".into(),
                closing_path_nodes: vec![],
                closing_path_edge_ids: vec![],
            },
        ),
    ];
    for (edge, expected) in refusals {
        assert_eq!(graph.validate_edge_addition(&edge), Err(expected.clone()));
        assert_eq!(graph.add_edge(edge), Err(expected));
        assert_eq!(graph, before, "refused insertion mutated the graph");
    }
}
