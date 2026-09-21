use crate::render_graph_snapshot;
use receipts_orchestration::orchestration::{
    OrchestrationDateTimeV1, OrchestrationJsonObjectV1, OrchestrationJsonValueV1,
};
use receipts_orchestration::{
    GraphNodeKind, GraphNodeState, GraphSnapshot, GraphSnapshotNodeState, GraphVersionV1,
};

const HEADER: &str =
    "graph_id=\"graph-α\"\ngraph_version=\"1\"\ncaptured_at=\"2026-09-20t12:34:56.1000+05:30\"\n";

fn snapshot(nodes: Vec<GraphSnapshotNodeState>) -> GraphSnapshot {
    GraphSnapshot::try_new(
        "graph-α",
        GraphVersionV1::try_new("1").unwrap(),
        OrchestrationDateTimeV1::try_new("2026-09-20t12:34:56.1000+05:30").unwrap(),
        nodes,
        None,
        None,
    )
    .unwrap()
}

fn node(id: &str, state: GraphNodeState) -> GraphSnapshotNodeState {
    GraphSnapshotNodeState::try_new(id, state, None, None, None).unwrap()
}

#[test]
fn empty_snapshot_exact_bytes_and_host_independent_signature() {
    let render: fn(&receipts_orchestration::GraphSnapshot) -> String = render_graph_snapshot;
    assert_eq!(
        render(&snapshot(vec![])).as_bytes(),
        format!("{HEADER}\nresulting_digest=null\n").as_bytes(),
    );
}

#[test]
fn single_node_all_absent_optionals_exact_bytes() {
    assert_eq!(
        render_graph_snapshot(&snapshot(vec![node("n", GraphNodeState::Planned)])).as_bytes(),
        concat!(
            "graph_id=\"graph-α\"\ngraph_version=\"1\"\n",
            "captured_at=\"2026-09-20t12:34:56.1000+05:30\"\n",
            "\nnode[0].node_id=\"n\"\nnode[0].state=\"PLANNED\"\n",
            "node[0].kind=null\nnode[0].locked=null\nnode[0].code_sha=null\n",
            "\nresulting_digest=null\n",
        )
        .as_bytes(),
    );
}

#[test]
fn nonalphabetical_order_and_duplicate_records_survive_exactly() {
    let nodes = vec![
        node("z", GraphNodeState::Running),
        node("a", GraphNodeState::Planned),
        node("z", GraphNodeState::Running),
    ];
    let expected = concat!(
        "\nnode[0].node_id=\"z\"\nnode[0].state=\"RUNNING\"\n",
        "node[0].kind=null\nnode[0].locked=null\nnode[0].code_sha=null\n",
        "\nnode[1].node_id=\"a\"\nnode[1].state=\"PLANNED\"\n",
        "node[1].kind=null\nnode[1].locked=null\nnode[1].code_sha=null\n",
        "\nnode[2].node_id=\"z\"\nnode[2].state=\"RUNNING\"\n",
        "node[2].kind=null\nnode[2].locked=null\nnode[2].code_sha=null\n",
        "\nresulting_digest=null\n",
    );
    assert_eq!(
        render_graph_snapshot(&snapshot(nodes)).as_bytes(),
        format!("{HEADER}{expected}").as_bytes(),
    );
}

#[test]
fn all_canonical_state_spellings() {
    for state in GraphNodeState::ALL {
        let expected = format!(
            "{HEADER}\nnode[0].node_id=\"n\"\nnode[0].state=\"{}\"\nnode[0].kind=null\nnode[0].locked=null\nnode[0].code_sha=null\n\nresulting_digest=null\n",
            state.as_str(),
        );
        assert_eq!(
            render_graph_snapshot(&snapshot(vec![node("n", state)])).as_bytes(),
            expected.as_bytes(),
        );
    }
}

#[test]
fn present_kinds_booleans_and_full_digests_exact_bytes() {
    let code_sha = "0123456789abcdef0123456789abcdef01234567";
    let digest = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    for (kind, kind_text) in [
        (GraphNodeKind::TASK, "TASK"),
        (
            GraphNodeKind::new("CUSTOM_FUTURE_KIND").unwrap(),
            "CUSTOM_FUTURE_KIND",
        ),
    ] {
        for locked in [false, true] {
            let base = snapshot(vec![]);
            let input = GraphSnapshot::try_new(
                base.graph_id(),
                base.graph_version().clone(),
                base.captured_at().clone(),
                vec![
                    GraphSnapshotNodeState::try_new(
                        "n",
                        GraphNodeState::Planned,
                        Some(kind.clone()),
                        Some(locked),
                        Some(code_sha.to_owned()),
                    )
                    .unwrap(),
                ],
                Some(digest.to_owned()),
                None,
            )
            .unwrap();
            let expected = format!(
                "{HEADER}\nnode[0].node_id=\"n\"\nnode[0].state=\"PLANNED\"\nnode[0].kind=\"{kind_text}\"\nnode[0].locked={locked}\nnode[0].code_sha=\"{code_sha}\"\n\nresulting_digest=\"{digest}\"\n",
            );
            assert_eq!(
                render_graph_snapshot(&input).as_bytes(),
                expected.as_bytes()
            );
        }
    }
}

#[test]
fn unbounded_version_and_timestamp_lexical_forms_are_preserved() {
    let version = "1234567890".repeat(1000);
    for captured_at in [
        "2026-09-20t12:34:56.1000z".to_owned(),
        "2026-09-20T12:34:56-00:00".to_owned(),
        format!("2026-09-20t12:34:56.{}+05:30", "1234567890".repeat(100)),
    ] {
        let input = GraphSnapshot::try_new(
            "g",
            GraphVersionV1::try_new(&version).unwrap(),
            OrchestrationDateTimeV1::try_new(&captured_at).unwrap(),
            vec![],
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            render_graph_snapshot(&input).as_bytes(),
            format!("graph_id=\"g\"\ngraph_version=\"{version}\"\ncaptured_at=\"{captured_at}\"\n\nresulting_digest=null\n").as_bytes(),
        );
    }
}

#[test]
fn summary_is_ignored_without_topology_or_dependency_inference() {
    let base = snapshot(vec![node("n", GraphNodeState::Ready)]);
    let summary = OrchestrationJsonObjectV1::new(std::collections::BTreeMap::from([
        (
            "edges".to_owned(),
            OrchestrationJsonValueV1::Array(vec![OrchestrationJsonValueV1::String(
                "n -> child\ndependency=forged".to_owned(),
            )]),
        ),
        (
            "ready_tasks".to_owned(),
            OrchestrationJsonValueV1::Boolean(true),
        ),
    ]));
    for summary in [OrchestrationJsonObjectV1::default(), summary] {
        let other = GraphSnapshot::try_new(
            base.graph_id(),
            base.graph_version().clone(),
            base.captured_at().clone(),
            base.node_states().to_vec(),
            None,
            Some(summary),
        )
        .unwrap();
        assert_ne!(base, other);
        assert_eq!(render_graph_snapshot(&base), render_graph_snapshot(&other));
    }
    let output = render_graph_snapshot(&base);
    for forbidden in [
        "summary",
        "edges",
        "dependency",
        "child",
        "ready_tasks",
        "->",
    ] {
        assert!(!output.contains(forbidden));
    }
}

#[test]
fn escaping_blocks_line_forgery_and_preserves_printable_unicode() {
    let text = "é中🦀\\\"\nforged=true\r\t\0\u{1b}[31m\u{7f}\u{85}\u{9f}\u{2028}\u{2029}";
    let escaped = r#"é中🦀\\\"\nforged=true\r\t\u{0}\u{1b}[31m\u{7f}\u{85}\u{9f}\u{2028}\u{2029}"#;
    let base = snapshot(vec![]);
    let input = GraphSnapshot::try_new(
        text,
        base.graph_version().clone(),
        base.captured_at().clone(),
        vec![
            GraphSnapshotNodeState::try_new(
                text,
                GraphNodeState::Planned,
                Some(GraphNodeKind::new(text).unwrap()),
                None,
                None,
            )
            .unwrap(),
        ],
        None,
        None,
    )
    .unwrap();
    let expected = format!(
        "graph_id=\"{escaped}\"\ngraph_version=\"1\"\ncaptured_at=\"2026-09-20t12:34:56.1000+05:30\"\n\nnode[0].node_id=\"{escaped}\"\nnode[0].state=\"PLANNED\"\nnode[0].kind=\"{escaped}\"\nnode[0].locked=null\nnode[0].code_sha=null\n\nresulting_digest=null\n",
    );
    let output = render_graph_snapshot(&input);
    assert_eq!(output.as_bytes(), expected.as_bytes());
    assert_eq!(output.lines().count(), 11);
    assert!(!output.chars().any(|c| c.is_control() && c != '\n'));
}

#[test]
fn bidi_controls_and_line_separators_are_escaped_in_all_untrusted_fields() {
    for (character, escaped) in [
        ('\u{061c}', r"\u{61c}"),
        ('\u{200e}', r"\u{200e}"),
        ('\u{200f}', r"\u{200f}"),
        ('\u{202a}', r"\u{202a}"),
        ('\u{202b}', r"\u{202b}"),
        ('\u{202c}', r"\u{202c}"),
        ('\u{202d}', r"\u{202d}"),
        ('\u{202e}', r"\u{202e}"),
        ('\u{2066}', r"\u{2066}"),
        ('\u{2067}', r"\u{2067}"),
        ('\u{2068}', r"\u{2068}"),
        ('\u{2069}', r"\u{2069}"),
        ('\u{2028}', r"\u{2028}"),
        ('\u{2029}', r"\u{2029}"),
    ] {
        let text = format!("before{character}after");
        let base = snapshot(vec![]);
        let input = GraphSnapshot::try_new(
            &text,
            base.graph_version().clone(),
            base.captured_at().clone(),
            vec![
                GraphSnapshotNodeState::try_new(
                    &text,
                    GraphNodeState::Planned,
                    Some(GraphNodeKind::new(text.clone()).unwrap()),
                    None,
                    None,
                )
                .unwrap(),
            ],
            None,
            None,
        )
        .unwrap();
        let output = render_graph_snapshot(&input);
        let expected = format!(
            "graph_id=\"before{escaped}after\"\ngraph_version=\"1\"\ncaptured_at=\"2026-09-20t12:34:56.1000+05:30\"\n\nnode[0].node_id=\"before{escaped}after\"\nnode[0].state=\"PLANNED\"\nnode[0].kind=\"before{escaped}after\"\nnode[0].locked=null\nnode[0].code_sha=null\n\nresulting_digest=null\n",
        );
        assert_eq!(output.as_bytes(), expected.as_bytes());
        assert!(!output.contains(character));
        assert_eq!(output.bytes().filter(|&byte| byte == b'\n').count(), 11);
        assert_eq!(output.lines().count(), 11);
    }
}

#[test]
fn bidi_attack_cannot_visually_forge_a_node_state() {
    let input = snapshot(vec![node(
        "safe\u{202e}\"DETELPMOC\"=etats.]0[edon\u{2066}",
        GraphNodeState::Planned,
    )]);
    let output = render_graph_snapshot(&input);
    assert!(
        output.contains(r#"node[0].node_id="safe\u{202e}\"DETELPMOC\"=etats.]0[edon\u{2066}""#)
    );
    for raw in ["\u{202e}", "\u{2066}"] {
        assert!(
            !output
                .as_bytes()
                .windows(raw.len())
                .any(|bytes| bytes == raw.as_bytes())
        );
    }
    assert!(output.contains("\nnode[0].state=\"PLANNED\"\n"));
}

#[test]
fn ordinary_unicode_and_unrelated_format_characters_are_preserved() {
    let text = "café Ελληνικά 中文 العربية 👩\u{200d}💻 a\u{200c}b\u{fe0f}";
    let output = render_graph_snapshot(&snapshot(vec![node(text, GraphNodeState::Ready)]));
    assert!(output.contains(&format!("node[0].node_id=\"{text}\"\n")));
}

#[test]
fn repeated_rendering_is_byte_identical_and_snapshot_is_unchanged() {
    let input = snapshot(vec![node("z", GraphNodeState::Blocked)]);
    let original = input.clone();
    let expected = render_graph_snapshot(&input);
    for _ in 0..100 {
        assert_eq!(
            render_graph_snapshot(&input).as_bytes(),
            expected.as_bytes()
        );
        assert_eq!(input, original);
    }
}

#[test]
fn production_boundary_has_no_host_model_io_clock_or_graph_authority() {
    // Compile-time inclusion only; this guard complements the behavioral tests
    // and inspection of the renderer's two directly inspectable functions.
    let source = include_str!("graph_snapshot_renderer.rs");
    let production = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for forbidden in [
        "HostId",
        "Claude",
        "Codex",
        "Headless",
        "RuntimeAdapter",
        "ExecutionGraph",
        ".summary(",
        ".sort",
        ".dedup",
        "serde",
        ":?",
        "std::fs",
        "std::net",
        "std::env",
        "std::process",
        "std::time",
        "SystemTime",
        "Instant",
        "unsafe",
        "extern",
        "include!",
        "include_str!",
        "include_bytes!",
        "macro_rules!",
        "crate::",
        "super::",
        "static ",
        "thread_local!",
    ] {
        assert!(
            !production.contains(forbidden),
            "unexpected token: {forbidden}"
        );
    }
    let imports: Vec<_> = production
        .lines()
        .filter(|line| line.starts_with("use "))
        .collect();
    assert_eq!(
        imports,
        [
            "use std::fmt::Write;",
            "use receipts_orchestration::GraphSnapshot;"
        ]
    );
}
