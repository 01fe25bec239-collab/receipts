//! Deterministic topology display from one verified projection.
use crate::graph_snapshot_renderer::display_text;
use receipts_orchestration::ReadOnlyGraphViewV1;

pub fn render_graph_tree(view: &ReadOnlyGraphViewV1) -> String {
    render(view.snapshot(), view.edges())
}

fn render(
    snapshot: &receipts_orchestration::GraphSnapshot,
    edges: &[receipts_orchestration::GraphEdge],
) -> String {
    let mut output = format!(
        "graph {} version {} digest {}\n",
        display_text(Some(snapshot.graph_id())),
        display_text(Some(snapshot.graph_version().as_str())),
        display_text(snapshot.resulting_digest())
    );
    // Workstream membership is unavailable in the merged projection; do not infer it.
    for node in snapshot.node_states() {
        output.push_str(&format!(
            "├─ [{}] {}\n",
            node.state().as_str(),
            display_text(Some(node.node_id()))
        ));
        for edge in edges.iter().filter(|edge| edge.to_node() == node.node_id()) {
            let state = snapshot
                .node_states()
                .iter()
                .find(|n| n.node_id() == edge.from_node())
                .map(|n| n.state().as_str())
                .unwrap_or("UNKNOWN");
            output.push_str(&format!(
                "│  ← {} [{}] {} {}\n",
                display_text(Some(edge.from_node())),
                state,
                edge.class().as_str(),
                edge.relation().kind_as_str()
            ));
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use receipts_orchestration::orchestration::OrchestrationDateTimeV1;
    use receipts_orchestration::{
        GraphEdge, GraphNodeState, GraphSnapshot, GraphSnapshotNodeState, GraphVersionV1,
        PrecedenceKind,
    };
    #[test]
    fn dependency_state_and_escaped_ids_are_shown() {
        let snapshot = GraphSnapshot::try_new(
            "g",
            GraphVersionV1::try_new("2").unwrap(),
            OrchestrationDateTimeV1::try_new("2026-11-11T00:00:00Z").unwrap(),
            vec![
                GraphSnapshotNodeState::try_new(
                    "a\u{202e}",
                    GraphNodeState::Accepted,
                    None,
                    None,
                    None,
                )
                .unwrap(),
                GraphSnapshotNodeState::try_new("b", GraphNodeState::Blocked, None, None, None)
                    .unwrap(),
            ],
            None,
            None,
        )
        .unwrap();
        let edge =
            GraphEdge::precedence("e", "g", "a\u{202e}", "b", PrecedenceKind::RequiresAccepted)
                .unwrap();
        assert_eq!(
            render(&snapshot, &[edge]),
            concat!(
                "graph \"g\" version \"2\" digest null\n",
                "├─ [ACCEPTED] \"a\\u{202e}\"\n├─ [BLOCKED] \"b\"\n",
                "│  ← \"a\\u{202e}\" [ACCEPTED] PRECEDENCE REQUIRES_ACCEPTED\n"
            )
        );
    }
}
