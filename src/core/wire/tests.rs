//! Wave1 graph codecs: fixtures, independent schema agreement, exact
//! numbers/null/absence, unsupported operational domains.

use serde_json::{Value, json};

use super::*;
use crate::orchestration::OrchestrationJsonValueV1;

fn read(path: &str) -> String {
    std::fs::read_to_string(format!("{}/../../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

const SCHEMAS: [&str; 6] = [
    "ExecutionGraph",
    "GraphNode",
    "GraphEdge",
    "GraphMutation",
    "GraphSnapshot",
    "GraphNodeResult",
];

fn validator(name: &str) -> jsonschema::Validator {
    let mut registry = jsonschema::Registry::new();
    for schema in SCHEMAS {
        let value: Value =
            serde_json::from_str(&read(&format!("schemas/{schema}.schema.json"))).unwrap();
        registry = registry
            .add(
                format!("https://receipts.invalid/orchestrator/schemas/{schema}.schema.json"),
                value,
            )
            .unwrap();
    }
    let registry = Box::leak(Box::new(registry.prepare().unwrap()));
    let root: Value = serde_json::from_str(&read(&format!("schemas/{name}.schema.json"))).unwrap();
    jsonschema::options()
        .with_registry(registry)
        .build(&root)
        .unwrap()
}

const FIXTURES: [&str; 7] = [
    "01_linear",
    "02_parallel_fanout_fanin",
    "03_repair_expansion",
    "04_human_gate",
    "05_goal_evaluator_append",
    "06_free_graph",
    "07_pro_graph_with_locked_node",
];

#[test]
fn graphs_fixtures_01_to_07_roundtrip() {
    let schema = validator("ExecutionGraph");
    for name in FIXTURES {
        let text = read(&format!("fixtures/graphs/{name}.json"));
        let original: Value = serde_json::from_str(&text).unwrap();
        assert!(schema.is_valid(&original), "{name}");
        let wire = ExecutionGraphWireV1::from_json(&text).unwrap();
        // Verbatim wire roundtrip.
        let again: Value = serde_json::from_str(&wire.to_json()).unwrap();
        assert_eq!(again, original, "{name}");
        // Operational roundtrip: the envelope re-encodes to the same JSON.
        let envelope = wire.to_envelope().unwrap();
        let encoded = ExecutionGraphWireV1::from_envelope(&envelope);
        // Encoding orders children by id; content is otherwise identical.
        let sorted = |mut v: Value| {
            for (list, key) in [("nodes", "node_id"), ("edges", "edge_id")] {
                v[list]
                    .as_array_mut()
                    .unwrap()
                    .sort_by(|a, b| a[key].as_str().cmp(&b[key].as_str()));
            }
            v
        };
        let encoded_value: Value = serde_json::from_str(&encoded.to_json()).unwrap();
        assert_eq!(encoded_value, sorted(original.clone()), "{name}");
        assert_eq!(encoded.to_envelope().unwrap(), envelope);
        assert!(!envelope.graph().has_precedence_cycle(), "{name}");
    }
}

#[test]
fn graphs_negative_fixtures_01_to_04_rejected() {
    let edge_schema = validator("GraphEdge");
    let graph_schema = validator("ExecutionGraph");
    let base: Value = serde_json::from_str(&read("fixtures/graphs/01_linear.json")).unwrap();
    for name in [
        "01_precedence_with_control_kind",
        "02_control_with_precedence_kind",
        "03_precedence_missing_kind",
        "04_control_missing_kind",
    ] {
        let text = read(&format!("fixtures/graphs-negative/{name}.json"));
        let edge: Value = serde_json::from_str(&text).unwrap();
        assert!(!edge_schema.is_valid(&edge), "{name}");
        assert!(GraphEdge::from_json(&text).is_err(), "{name}");
        // Embedded in an otherwise valid graph the whole graph refuses (S45).
        let mut graph = base.clone();
        graph["edges"].as_array_mut().unwrap().push(edge);
        assert!(!graph_schema.is_valid(&graph), "{name}");
        assert_eq!(
            ExecutionGraphWireV1::from_json(&graph.to_string()),
            Err(GraphWireError::Shape("edges")),
            "{name}"
        );
    }
}

#[test]
fn semantic_graph_refusals_happen_at_conversion_not_decoding() {
    let base: Value = serde_json::from_str(&read("fixtures/graphs/01_linear.json")).unwrap();
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut v = base.clone();
        f(&mut v);
        ExecutionGraphWireV1::from_json(&v.to_string()).unwrap()
    };
    for wire in [
        // Precedence cycle n3 -> n1.
        edit(&|v| {
            v["edges"].as_array_mut().unwrap().push(json!({"edge_id":"e3","graph_id":"G-LIN","from_node":"n3","to_node":"n1","edge_class":"PRECEDENCE","precedence_kind":"REQUIRES_ACCEPTED"}))
        }),
        // Foreign child graph, dangling endpoint, duplicate node id.
        edit(&|v| v["nodes"][0]["graph_id"] = json!("OTHER")),
        edit(&|v| v["edges"][0]["to_node"] = json!("ghost")),
        edit(&|v| v["nodes"][1]["node_id"] = json!("n1")),
    ] {
        assert!(matches!(wire.to_envelope(), Err(GraphWireError::Graph(_))));
    }
    // A control-only loop is fine.
    let control = edit(&|v| {
        v["edges"].as_array_mut().unwrap().push(json!({"edge_id":"c","graph_id":"G-LIN","from_node":"n3","to_node":"n1","edge_class":"CONTROL","control_kind":"ON_REJECT"}))
    });
    assert!(control.to_envelope().is_ok());
}

#[test]
fn exact_numbers_and_unsupported_operational_domains() {
    let base: Value = serde_json::from_str(&read("fixtures/graphs/01_linear.json")).unwrap();
    let text = base.to_string();
    let with = |from: &str, to: &str| ExecutionGraphWireV1::from_json(&text.replacen(from, to, 1));
    // Schema integers in non-canonical spellings: admitted and kept verbatim,
    // unsupported operationally.
    for spelling in ["1.0", "1e0", "10E-1", "0.1e1", "1.000"] {
        let wire = with(
            "\"graph_version\":1",
            &format!("\"graph_version\":{spelling}"),
        )
        .unwrap();
        assert!(
            wire.to_json()
                .contains(&format!("\"graph_version\":{spelling}")),
            "{spelling}"
        );
        assert_eq!(
            wire.to_envelope(),
            Err(GraphWireError::Unsupported("graph_version"))
        );
    }
    // Huge exact versions convert without a machine ceiling.
    let huge = format!("1{}", "0".repeat(400));
    let env = with("\"graph_version\":1", &format!("\"graph_version\":{huge}"))
        .unwrap()
        .to_envelope()
        .unwrap();
    assert_eq!(env.graph_version().as_str(), huge);
    // Non-integers and out-of-range values refuse decoding.
    for bad in ["0", "-1", "1.5", "1e-1", "0e5", "\"1\"", "null", "true"] {
        assert_eq!(
            with("\"graph_version\":1", &format!("\"graph_version\":{bad}")),
            Err(GraphWireError::Shape("graph_version")),
            "{bad}"
        );
    }
    // context_epoch: 0 and huge allowed, -0 is a schema 0 but unsupported.
    for (spelling, ok) in [
        ("0", true),
        ("123456789012345678901234567890", true),
        ("-0", false),
        ("2e3", false),
    ] {
        let wire = with(
            "\"context_epoch\":1",
            &format!("\"context_epoch\":{spelling}"),
        )
        .unwrap();
        assert_eq!(wire.to_envelope().is_ok(), ok, "{spelling}");
    }
    assert!(with("\"context_epoch\":1", "\"context_epoch\":-1").is_err());
    // date-time is annotation-only for data, but operationally checked.
    let wire = with("2026-08-15T00:00:00Z", "yesterday").unwrap();
    assert_eq!(
        wire.to_envelope(),
        Err(GraphWireError::Unsupported("created_at"))
    );
    // parent_version: absent / null / value are distinct and preserved.
    let null = with(
        "\"graph_version\":1,",
        "\"graph_version\":1,\"parent_version\":null,",
    )
    .unwrap();
    assert_eq!(null.to_envelope().unwrap().parent_version(), Some(None));
    assert!(
        ExecutionGraphWireV1::from_envelope(&null.to_envelope().unwrap())
            .to_json()
            .contains("\"parent_version\":null")
    );
    assert_eq!(
        wire_err(
            "\"graph_version\":1,",
            "\"graph_version\":1,\"parent_version\":0,",
            &text
        ),
        GraphWireError::Shape("parent_version")
    );
}

fn wire_err(from: &str, to: &str, text: &str) -> GraphWireError {
    ExecutionGraphWireV1::from_json(&text.replacen(from, to, 1)).unwrap_err()
}

#[test]
fn integer_lexeme_test_is_exact() {
    for (lexeme, min, ok) in [
        ("1", 1, true),
        ("2", 2, true),
        ("1", 2, false),
        ("1.0", 2, false),
        ("10e-1", 2, false),
        ("2.0e0", 2, true),
        ("0.2e1", 2, true),
        ("15e-1", 1, false),
        ("-0.0", 0, true),
        ("-0.0", 1, false),
        ("-3", 0, false),
        ("1e999999999999999999999999999999999", 2, true),
        ("1e-999999999999999999999999999999999", 0, false),
        ("100e-2", 1, true),
        ("100e-3", 0, false),
    ] {
        assert_eq!(integer_at_least(lexeme, min), ok, "{lexeme} >= {min}");
    }
}

fn agree(name: &str, cases: &[Value], decode: &dyn Fn(&str) -> bool) {
    let schema = validator(name);
    let (mut accepted, mut refused) = (0, 0);
    for case in cases {
        let valid = schema.is_valid(case);
        assert_eq!(decode(&case.to_string()), valid, "{name}: {case}");
        if valid { accepted += 1 } else { refused += 1 }
    }
    assert!(accepted >= 3 && refused >= 8, "{name} {accepted}/{refused}");
}

fn edits(base: Value, list: Vec<(&str, Value)>) -> Vec<Value> {
    let mut out = vec![base.clone()];
    for (field, value) in list {
        let mut v = base.clone();
        if value == json!("__remove__") {
            v.as_object_mut().unwrap().remove(field);
        } else {
            v[field] = value;
        }
        out.push(v);
    }
    out
}

#[test]
fn graph_node_valid_fixture_roundtrips_and_negative_rejected() {
    let full = json!({"node_id":"n","graph_id":"g","kind":"X_NEW","state":"LOCKED_REQUIRES_PRO",
        "title":"","parent_node_id":"p","attempt_number":7,
        "required_capabilities":["a.b","a.b"],"task_capsule_ref":"","workstream_id":"w",
        "code_sha":"0123456789abcdef0123456789abcdef01234567","workspace_id":"ws","result_ref":"r",
        "locked_reason":null,"created_in_version":2});
    let cases = edits(
        full.clone(),
        vec![
            ("locked_reason", json!("why")),
            ("title", json!("__remove__")),
            ("kind", json!("")),
            ("state", json!("planned")),
            ("node_id", json!("")),
            ("node_id", json!("x".repeat(201))),
            ("attempt_number", json!(0)),
            ("attempt_number", json!(1.5)),
            ("required_capabilities", json!(["nodot"])),
            (
                "code_sha",
                json!("ABCDEF0123456789abcdef0123456789abcdef01"),
            ),
            ("title", json!(null)),
            ("created_in_version", json!("2")),
            ("extra", json!(1)),
            ("state", json!("__remove__")),
            ("workstream_id", json!(null)),
        ],
    );
    agree("GraphNode", &cases, &|t| {
        GraphNodeWireV1::from_json(t).is_ok()
    });
    let wire = GraphNodeWireV1::from_json(&full.to_string()).unwrap();
    let node = wire.to_graph_node().unwrap();
    assert_eq!(node.locked_reason(), Some(None));
    assert_eq!(node.required_capabilities().unwrap().len(), 2);
    assert_eq!(GraphNodeWireV1::from_graph_node(&node), wire);
    // Operational absence stays absent.
    let minimal = GraphNode::new("n", "g", GraphNodeKind::TASK, GraphNodeState::Planned).unwrap();
    assert_eq!(
        GraphNodeWireV1::from_graph_node(&minimal).to_json(),
        r#"{"graph_id":"g","kind":"TASK","node_id":"n","state":"PLANNED"}"#
    );
}

#[test]
fn graph_mutation_valid_fixture_roundtrips_and_negative_rejected() {
    let full = json!({"mutation_id":"m","graph_id":"g","parent_version":1,"resulting_version":2,
        "actor":{"role":"","role_id":"r"},"reason":"why","created_at":"not-a-time",
        "resulting_digest":"a".repeat(64),
        "operations":[{"op":"ADD_NODE","node_id":"n","detail":{"x":[1.5,{"k":1}],"y":null}},{"op":"CANCEL_NODE"}]});
    let cases = edits(
        full.clone(),
        vec![
            ("resulting_version", json!(3)),
            ("created_at", json!("2026-10-29T00:00:00Z")),
            ("resulting_version", json!(1)),
            ("parent_version", json!(0)),
            ("reason", json!("")),
            ("operations", json!([])),
            ("operations", json!([{"op":"DELETE_NODE"}])),
            ("operations", json!([{"op":"ADD_NODE","detail":[]}])),
            ("operations", json!([{"op":"ADD_NODE","extra":1}])),
            ("actor", json!({"role_id":"r"})),
            ("actor", json!({"role":"USER","x":1})),
            ("resulting_digest", json!("A".repeat(64))),
            ("created_at", json!(null)),
        ],
    );
    agree("GraphMutation", &cases, &|t| {
        GraphMutationWireV1::from_json(t).is_ok()
    });
    // Exact lexemes and magic-looking keys survive; 1e400 is outside the
    // f64 oracle's domain, so it is checked here rather than in `agree`.
    let raw = full.to_string().replacen(
        "[1.5,{\"k\":1}]",
        "[1.50,{\"$serde_json::private::RawValue\":1e400}]",
        1,
    );
    let wire = GraphMutationWireV1::from_json(&raw).unwrap();
    assert!(
        wire.to_json()
            .contains("[1.50,{\"$serde_json::private::RawValue\":1e400}]"),
        "{}",
        wire.to_json()
    );
    // Valid wire, but the timestamp is operationally unsupported.
    assert_eq!(
        wire.to_mutation(),
        Err(GraphWireError::Unsupported("created_at"))
    );
    let ok =
        GraphMutationWireV1::from_json(&raw.replace("not-a-time", "2026-10-29T00:00:00Z")).unwrap();
    let mutation = ok.to_mutation().unwrap();
    assert_eq!(mutation.actor().role(), "");
    assert_eq!(GraphMutationWireV1::from_mutation(&mutation), ok);
    // Duplicate keys refuse anywhere, including inside open detail.
    let dup = raw.replacen("\"y\":null", "\"y\":null,\"y\":null", 1);
    assert_eq!(
        GraphMutationWireV1::from_json(&dup),
        Err(GraphWireError::Json)
    );
}

#[test]
fn graph_snapshot_valid_fixture_roundtrips_and_negative_rejected() {
    let full = json!({"graph_id":"g","graph_version":3,"captured_at":"2026-10-29T00:00:00Z",
        "resulting_digest":"b".repeat(64),
        "node_states":[{"node_id":"n","state":"RUNNING","kind":"TASK","locked":false,"code_sha":"c".repeat(40)},{"node_id":"m","state":"READY"}],
        "summary":{"k":[1,2.0]}});
    let cases = edits(
        full.clone(),
        vec![
            ("node_states", json!([])),
            ("node_states", json!([{"node_id":"n","state":"ANYTHING"}])),
            ("node_states", json!([{"node_id":"n"}])),
            (
                "node_states",
                json!([{"node_id":"n","state":"X","locked":"no"}]),
            ),
            (
                "node_states",
                json!([{"node_id":"n","state":"X","kind":""}]),
            ),
            ("summary", json!([])),
            ("graph_version", json!(0)),
            ("captured_at", json!("__remove__")),
            ("extra", json!(1)),
            ("node_states", json!(null)),
            ("resulting_digest", json!("b")),
        ],
    );
    agree("GraphSnapshot", &cases, &|t| {
        GraphSnapshotWireV1::from_json(t).is_ok()
    });
    let wire = GraphSnapshotWireV1::from_json(&full.to_string()).unwrap();
    let snapshot = wire.to_snapshot().unwrap();
    assert_eq!(snapshot.node_states()[0].locked(), Some(false));
    assert_eq!(GraphSnapshotWireV1::from_snapshot(&snapshot), wire);
    let unknown =
        GraphSnapshotWireV1::from_json(&full.to_string().replace("RUNNING", "ANYTHING")).unwrap();
    assert_eq!(
        unknown.to_snapshot(),
        Err(GraphWireError::Unsupported("state"))
    );
}

#[test]
fn graph_node_result_valid_fixture_roundtrips_and_negative_rejected() {
    let full = json!({"node_id":"n","graph_id":"g","outcome":"HUMAN_REQUIRED","produced_at":"2026-10-29T00:00:00+05:30",
        "code_sha":"d".repeat(40),"attempt_id":"a","evidence_refs":["",""],"review_id":"r","routing_decision_id":"rd",
        "checks":[{"command":["cargo","test"],"exit_code":-1,"result":"UNKNOWN","output_ref":"o"},{"command":["x"],"exit_code":0}]});
    let cases = edits(
        full.clone(),
        vec![
            ("outcome", json!("PASS")),
            ("checks", json!([])),
            ("outcome", json!("PASSED")),
            ("checks", json!([{"command":[],"exit_code":0}])),
            ("checks", json!([{"command":["x"],"exit_code":1.5}])),
            ("checks", json!([{"command":["x"]}])),
            (
                "checks",
                json!([{"command":["x"],"exit_code":0,"result":"OK"}]),
            ),
            ("evidence_refs", json!([1])),
            ("attempt_id", json!("")),
            ("produced_at", json!("__remove__")),
            ("extra", json!(1)),
            ("code_sha", json!(null)),
            ("checks", json!([{"command":"x","exit_code":0}])),
        ],
    );
    agree("GraphNodeResult", &cases, &|t| {
        GraphNodeResultWireV1::from_json(t).is_ok()
    });
    let wire = GraphNodeResultWireV1::from_json(&full.to_string()).unwrap();
    let result = wire.to_graph_node_result().unwrap();
    assert_eq!(result.checks().unwrap()[1].exit_code(), "0");
    assert_eq!(GraphNodeResultWireV1::from_graph_node_result(&result), wire);
    for spelling in ["1.0", "-0", "2e1"] {
        let text =
            full.to_string()
                .replacen("\"exit_code\":0", &format!("\"exit_code\":{spelling}"), 1);
        let wire = GraphNodeResultWireV1::from_json(&text).unwrap();
        assert_eq!(
            wire.to_graph_node_result(),
            Err(GraphWireError::Unsupported("exit_code")),
            "{spelling}"
        );
    }
    let huge = full.to_string().replacen(
        "\"exit_code\":0",
        &format!("\"exit_code\":-{}", "9".repeat(300)),
        1,
    );
    let wire = GraphNodeResultWireV1::from_json(&huge).unwrap();
    assert_eq!(
        wire.to_graph_node_result().unwrap().checks().unwrap()[1].exit_code(),
        format!("-{}", "9".repeat(300))
    );
}

#[test]
fn json_value_parse_is_strict_and_exact() {
    for bad in [
        "",
        " ",
        "{} x",
        "{\"a\":1,\"a\":1}",
        "[1,]",
        "01",
        "\"\\ud800\"",
        "\u{feff}{}",
        "NaN",
    ] {
        assert!(
            OrchestrationJsonValueV1::parse_json(bad).is_err(),
            "{bad:?}"
        );
    }
    for (text, back) in [
        (
            " {\"b\":[-0.0E+5, 1e400],\"a\":\"\\u00e9\"} ",
            "{\"a\":\"é\",\"b\":[-0.0E+5,1e400]}",
        ),
        (
            "{\"$serde_json::private::Number\":\"1\"}",
            "{\"$serde_json::private::Number\":\"1\"}",
        ),
        (
            "{\"$serde_json::private::RawValue\":true}",
            "{\"$serde_json::private::RawValue\":true}",
        ),
    ] {
        let value = OrchestrationJsonValueV1::parse_json(text).unwrap();
        assert!(matches!(value, OrchestrationJsonValueV1::Object(_)));
        assert_eq!(value.to_json(), back);
    }
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    assert!(OrchestrationJsonValueV1::parse_json(&deep).is_err());
    let ok = format!("{}{}", "[".repeat(100), "]".repeat(100));
    assert!(OrchestrationJsonValueV1::parse_json(&ok).is_ok());
}

#[test]
fn shape_errors_name_fields_without_echoing_values() {
    let secret = json!({"node_id":"n","graph_id":"g","kind":"TASK","state":"SECRET_VALUE"});
    let error = GraphNodeWireV1::from_json(&secret.to_string())
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("state") && !error.contains("SECRET"),
        "{error}"
    );
    let unknown =
        json!({"node_id":"n","graph_id":"g","kind":"TASK","state":"PLANNED","SECRET_KEY":1});
    let error = GraphNodeWireV1::from_json(&unknown.to_string())
        .unwrap_err()
        .to_string();
    assert!(!error.contains("SECRET"), "{error}");
}
