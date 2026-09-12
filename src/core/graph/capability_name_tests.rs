//! Behavioral coverage for the frozen, open capability-id syntax.

use crate::{CapabilityName, GraphError, GraphNode, GraphNodeKind, GraphNodeState};

#[test]
fn valid_capability_names_preserve_exact_values_and_open_namespaces() {
    for value in [
        "graph.core",
        "graph.validate",
        "orchestration.multi_runtime",
        "review.independent_a4",
        "graph_2.validate_1",
        "a.b",
        "a0.b0",
        "a_b.c_d",
        "future_engine.some_capability",
        "experimental2.next_generation",
        "foo.bar_baz7",
        "a.b.c",
        "a.b.c.d",
        "z9_.a0_",
    ] {
        let capability = CapabilityName::new(value).expect(value);
        assert_eq!(capability.as_str(), value);
        assert_eq!(capability.as_ref(), value);
    }
}

#[test]
fn invalid_capability_names_preserve_original_input_in_syntax_error() {
    for value in [
        "",
        " ",
        "graph",
        ".graph",
        "graph.",
        "graph..core",
        "Graph.core",
        "graph.Core",
        "MODEL-X",
        "graph-core.value",
        "graph/core",
        "graph core",
        "?",
        "!",
        "_",
        "1graph.core",
        "graph.1core",
        "graph._core",
        "☃",
        "gráph.core",
        "a..b",
        "a.b.",
        ".a.b",
        "a.-b",
        "a./b",
        "a. b",
        "a.é",
        "a.B",
        "A.b",
        "_a.b",
        "a.b-c",
        "a.b/c",
        " a.b",
        "a.b ",
        "a.b\n",
        "a.\tb",
        "a.b\r",
        "a.b\0",
        "a.b\u{000b}",
        "a.b\u{000c}",
        "a.b\u{00a0}",
        "a.e\u{0301}",
        "a．b",
    ] {
        let error = CapabilityName::new(value).expect_err(value);
        assert_eq!(
            error,
            GraphError::InvalidCapabilitySyntax {
                value: value.to_owned(),
            }
        );
        assert_eq!(
            error.to_string(),
            format!("invalid capability syntax for {value:?}")
        );
    }
}

#[test]
fn capability_name_ascii_character_boundaries() {
    for byte in 0..=127u8 {
        let character = char::from(byte);
        for value in [format!("{character}a.b"), format!("a.{character}b")] {
            assert_eq!(
                CapabilityName::new(&value).is_ok(),
                byte.is_ascii_lowercase(),
                "{value:?}"
            );
        }
        for value in [format!("a{character}.b"), format!("a.b{character}")] {
            let allowed = byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_';
            assert_eq!(CapabilityName::new(&value).is_ok(), allowed, "{value:?}");
        }
    }
}

#[test]
fn capability_name_has_no_graph_identifier_length_limit() {
    for value in [
        format!("{}.{}", "a".repeat(10_000), "b0_".repeat(10_000)),
        "a.".repeat(10_000) + "b",
    ] {
        let capability = CapabilityName::new(value.clone()).expect("valid without a length cap");
        assert_eq!(capability.as_str(), value);
    }
}

#[test]
fn graph_node_accepts_empty_required_capability_names() {
    let node = GraphNode::new(
        "empty",
        GraphNodeKind::TASK,
        GraphNodeState::Planned,
        vec![],
    )
    .expect("valid node");
    assert!(node.required_capabilities().is_empty());
}

#[test]
fn graph_node_preserves_capability_names_order_and_duplicates() {
    let values = [
        "graph.validate",
        "future_engine.some_capability",
        "graph.validate",
        "orchestration.multi_runtime",
    ];
    let capabilities = values
        .iter()
        .map(|value| CapabilityName::new(*value).expect("valid capability"))
        .collect();
    let node = GraphNode::new(
        "ordered",
        GraphNodeKind::TASK,
        GraphNodeState::Planned,
        capabilities,
    )
    .expect("valid node");
    let actual: Vec<_> = node
        .required_capabilities()
        .iter()
        .map(CapabilityName::as_str)
        .collect();
    assert_eq!(actual, values);
}
