//! Read-only, zero-model summary of exactly one supplied snapshot.
use crate::graph_snapshot_renderer::display_text;
use receipts_orchestration::GraphSnapshot;

/// Holds no store, handle, callback or mutable access.
/// ```compile_fail
/// use receipts_host_integration::EconomyRenderer;
/// use receipts_orchestration::GraphSnapshot;
/// fn mutate(view: EconomyRenderer<'_>, snapshot: GraphSnapshot) {
///     *view.snapshot() = snapshot;
/// }
/// ```
pub struct EconomyRenderer<'a>(&'a GraphSnapshot);
impl<'a> EconomyRenderer<'a> {
    pub fn new(snapshot: &'a GraphSnapshot) -> Self {
        Self(snapshot)
    }
    pub fn snapshot(&self) -> &'a GraphSnapshot {
        self.0
    }
    pub fn render(&self) -> String {
        let mut counts = std::collections::BTreeMap::new();
        for node in self.0.node_states() {
            *counts.entry(node.state().as_str()).or_insert(0usize) += 1;
        }
        let mut output = format!(
            "graph {} version {} digest {}\n",
            display_text(Some(self.0.graph_id())),
            display_text(Some(self.0.graph_version().as_str())),
            display_text(self.0.resulting_digest())
        );
        for (state, count) in counts {
            output.push_str(&format!("{state}: {count}\n"));
        }
        output
    }
}
