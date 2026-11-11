//! FREE policy preserves graph/history and permits only an explicitly selected
//! eligible host-native runtime. Retry authorization never resets other nodes.
use crate::{ExecutionGraphEnvelope, GraphNodeResult, GraphNodeResultOutcome, GraphNodeState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreePolicyV1 {
    provider: String,
    runtime: String,
}
impl FreePolicyV1 {
    pub fn provider(&self) -> &str {
        &self.provider
    }
    pub fn runtime(&self) -> &str {
        &self.runtime
    }
    pub fn new(provider: String, runtime: String) -> Option<Self> {
        if provider.is_empty() || runtime.is_empty() {
            return None;
        }
        Some(Self { provider, runtime })
    }
}

pub trait GraphExecutionPolicy {
    fn permits(&self, provider: &str, runtime: &str, eligible_host_native: bool) -> bool;
    fn permits_retry(
        &self,
        graph: &ExecutionGraphEnvelope,
        node: &str,
        result_ref: &str,
        result: &GraphNodeResult,
    ) -> bool;
}
impl GraphExecutionPolicy for FreePolicyV1 {
    /// Eligibility/native evidence comes from trusted Host/Runtime observation.
    /// Unknown, stale or unavailable observations must pass false.
    fn permits(&self, provider: &str, runtime: &str, eligible_host_native: bool) -> bool {
        eligible_host_native && self.provider == provider && self.runtime == runtime
    }
    fn permits_retry(
        &self,
        graph: &ExecutionGraphEnvelope,
        node: &str,
        result_ref: &str,
        result: &GraphNodeResult,
    ) -> bool {
        graph
            .graph()
            .node(node)
            .is_some_and(|n| n.result_ref() == Some(result_ref))
            && graph.graph().node_state(node) == Some(GraphNodeState::Rejected)
            && result.graph_id() == graph.graph_id()
            && result.node_id() == node
            && matches!(
                result.outcome(),
                GraphNodeResultOutcome::Fail | GraphNodeResultOutcome::Rejected
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn free_runtime_is_pinned_and_unknown_eligibility_refuses() {
        let p = FreePolicyV1::new("host".into(), "native".into()).unwrap();
        assert!(p.permits("host", "native", true));
        assert!(!p.permits("other", "native", true));
        assert!(!p.permits("host", "other", true));
        assert!(!p.permits("host", "native", false));
        assert!(FreePolicyV1::new("".into(), "native".into()).is_none());
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use crate::test_support::{envelope, time};
    #[test]
    fn retry_never_selects_accepted_history_or_other_nodes() {
        let policy = FreePolicyV1::new("host".into(), "native".into()).unwrap();
        let graph = envelope(
            &[
                ("accepted", "TASK", GraphNodeState::Accepted, Some("w")),
                ("rejected", "TASK", GraphNodeState::Rejected, Some("w")),
            ],
            &[],
        );
        let result = GraphNodeResult::try_new(
            "rejected",
            "g",
            GraphNodeResultOutcome::Fail,
            time("2026-11-08T00:00:00Z"),
        )
        .unwrap();
        assert!(!policy.permits_retry(&graph, "accepted", "r", &result));
        assert!(!policy.permits_retry(&graph, "missing", "r", &result));
        // A rejected state alone cannot substitute for an attached failure.
        assert!(!policy.permits_retry(&graph, "rejected", "r", &result));
    }
}
