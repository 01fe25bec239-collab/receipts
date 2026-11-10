//! Shared synthetic inputs for Wave2 scheduler, capsule, compiler and store tests.

use crate::orchestration::{FeatureAdmissionOutcome, OrchestrationDateTimeV1};
use crate::scheduler::{
    AdmissionBindingV1, AvailabilityEvidenceV1, AvailabilityObservationV1,
    DispatchAdmissionProviderPolicyStatus, DispatchAdmissionRequestV1, EntitlementEvidenceV1,
    EvidenceFreshnessV1, ProviderAuthEvidenceV1, ProviderConnectionV1, ProviderPolicyEvidenceV1,
    QualityFloorEvidenceV1, QualityFloorResultV1, SafetyEvidenceV1, SafetyStateV1,
    WriteScopeOverlapV1, WriteScopePort,
};
use crate::{
    ExecutionGraph, ExecutionGraphEnvelope, ExecutionGraphEnvelopeFields, GraphEdge, GraphNode,
    GraphNodeKind, GraphNodeState, GraphVersionV1, PrecedenceKind,
};

pub(crate) const GRAPH: &str = "g";
pub(crate) const EPOCH: &str = "7";

pub(crate) fn time(value: &str) -> OrchestrationDateTimeV1 {
    OrchestrationDateTimeV1::try_new(value).unwrap()
}

/// `(id, kind, state, workstream)` nodes and `(id, from, to)` REQUIRES_ACCEPTED edges.
pub(crate) fn envelope(
    nodes: &[(&str, &str, GraphNodeState, Option<&str>)],
    edges: &[(&str, &str, &str)],
) -> ExecutionGraphEnvelope {
    let mut graph = ExecutionGraph::new(GRAPH).unwrap();
    for (id, kind, state, workstream) in nodes {
        let mut node = GraphNode::new(
            *id,
            GRAPH,
            GraphNodeKind::new(kind.to_string()).unwrap(),
            *state,
        )
        .unwrap();
        if let Some(ws) = workstream {
            node = node.with_workstream_id(*ws).unwrap();
        }
        graph.add_node(node).unwrap();
    }
    for (id, from, to) in edges {
        graph
            .add_edge(
                GraphEdge::precedence(*id, GRAPH, *from, *to, PrecedenceKind::RequiresAccepted)
                    .unwrap(),
            )
            .unwrap();
    }
    ExecutionGraphEnvelope::bind(
        &graph,
        ExecutionGraphEnvelopeFields::new(
            "p",
            "goal",
            GraphVersionV1::try_new("1").unwrap(),
            time("2026-11-08T00:00:00Z"),
            "policy",
        ),
    )
    .unwrap()
}

pub(crate) fn binding(node: &str) -> AdmissionBindingV1 {
    AdmissionBindingV1 {
        node_id: node.into(),
        graph_id: Some(GRAPH.into()),
        context_epoch: EPOCH.into(),
    }
}

/// All six D11 axes PASS for `node` on `provider`.
pub(crate) fn passing(node: &str, provider: &str) -> DispatchAdmissionRequestV1 {
    let b = || binding(node);
    let fresh = EvidenceFreshnessV1::Fresh;
    DispatchAdmissionRequestV1 {
        decision_id: format!("da-{node}"),
        capability_id: Some("graph.core".into()),
        binding: b(),
        selected_provider_id: Some(provider.into()),
        entitlement: Some(EntitlementEvidenceV1 {
            binding: b(),
            freshness: fresh,
            feature_admission_decision_id: "fa".into(),
            outcome: FeatureAdmissionOutcome::Allow,
        }),
        provider_auth: Some(ProviderAuthEvidenceV1 {
            binding: b(),
            freshness: fresh,
            provider_id: provider.into(),
            connection: ProviderConnectionV1::Authenticated,
            technical_status: "CONNECTED".into(),
        }),
        provider_policy: Some(ProviderPolicyEvidenceV1 {
            binding: b(),
            freshness: fresh,
            provider_policy_eligibility_id: "ppe".into(),
            provider_id: provider.into(),
            status: DispatchAdmissionProviderPolicyStatus::VerifiedAllowed,
            execution_context: "PROGRAMMATIC_WORKER".into(),
        }),
        provider_availability: Some(AvailabilityEvidenceV1 {
            binding: b(),
            freshness: fresh,
            availability_state_ref: "as".into(),
            observation: AvailabilityObservationV1::Available,
            runtime_id: Some("runtime".into()),
        }),
        safety: Some(SafetyEvidenceV1 {
            binding: b(),
            freshness: fresh,
            state: SafetyStateV1::Clear,
            safety_interruption_id: None,
        }),
        quality_floor: Some(QualityFloorEvidenceV1 {
            binding: b(),
            freshness: fresh,
            routing_decision_id: "rd".into(),
            result: QualityFloorResultV1::Satisfied,
        }),
    }
}

/// Synthetic matcher: `x/**` covers `x` and below, anything else is a literal
/// path. A pattern containing `?` is undetermined, as is any set larger than
/// `limit` (a stand-in for the real engine's resource limit).
pub(crate) struct PrefixPort {
    pub(crate) limit: usize,
}

pub(crate) const PORT: PrefixPort = PrefixPort { limit: 64 };

impl WriteScopePort for PrefixPort {
    fn validate(&self, paths: &[String]) -> bool {
        !paths.is_empty() && paths.iter().all(|p| !p.is_empty() && !p.contains('?'))
    }

    fn overlap(&self, left: &[String], right: &[String]) -> WriteScopeOverlapV1 {
        if left.len() * right.len() > self.limit
            || left.iter().chain(right).any(|p| p.contains('?'))
        {
            return WriteScopeOverlapV1::Undetermined;
        }
        let split = |p: &str| match p.strip_suffix("/**") {
            Some(prefix) => (prefix.to_owned(), true),
            None => (p.to_owned(), false),
        };
        let under =
            |path: &str, prefix: &str| path == prefix || path.starts_with(&format!("{prefix}/"));
        for a in left {
            for b in right {
                let ((pa, wa), (pb, wb)) = (split(a), split(b));
                let witness = match (wa, wb) {
                    (false, false) if pa == pb => Some(pa.clone()),
                    (true, false) if under(&pb, &pa) => Some(pb.clone()),
                    (false, true) if under(&pa, &pb) => Some(pa.clone()),
                    (true, true) if under(&pa, &pb) => Some(format!("{pa}/x")),
                    (true, true) if under(&pb, &pa) => Some(format!("{pb}/x")),
                    _ => None,
                };
                if let Some(witness) = witness {
                    return WriteScopeOverlapV1::Overlaps { witness };
                }
            }
        }
        WriteScopeOverlapV1::Disjoint
    }
}

/// The real Workspace path-language engine behind the port, exactly as the
/// receipts-e2e adapter binds it (Core cannot depend on that crate).
pub(crate) struct RealPort;

impl WriteScopePort for RealPort {
    fn validate(&self, paths: &[String]) -> bool {
        !paths.is_empty()
            && paths
                .iter()
                .all(|p| receipts_workspace_execution::WritePathPattern::parse(p).is_ok())
    }

    fn overlap(&self, left: &[String], right: &[String]) -> WriteScopeOverlapV1 {
        use receipts_workspace_execution::{WritePathPattern, WritePathSet, intersects};
        let parse = |paths: &[String]| {
            paths
                .iter()
                .map(|p| WritePathPattern::parse(p).ok())
                .collect::<Option<Vec<_>>>()
                .map(WritePathSet::new)
        };
        let (Some(left), Some(right)) = (parse(left), parse(right)) else {
            return WriteScopeOverlapV1::Undetermined;
        };
        match intersects(&left, &right) {
            Ok(None) => WriteScopeOverlapV1::Disjoint,
            Ok(Some(witness)) => WriteScopeOverlapV1::Overlaps { witness },
            Err(_) => WriteScopeOverlapV1::Undetermined,
        }
    }
}

pub(crate) fn paths(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

// --- State-backed helpers ---------------------------------------------------

use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use receipts_state::{
    ContextEpoch, ContextEpochTrigger, GraphCompilerRefV1, GraphSourceEvidenceRefV1,
    StateEpochValueV1, StateError, TrustedClockV1, TrustedTimeSampleV1,
};

use crate::orchestration::{CoreAdmissionIdentity, CoreStateWriter};

pub(crate) const COMPILER: &str = "compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV";
pub(crate) const SOURCE: &str = "evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW";

/// A synthetic trusted clock at a fixed instant.
pub(crate) struct Clock(pub(crate) &'static str);
pub(crate) const CLOCK: Clock = Clock("2026-11-08T10:00:00.000000000Z");

impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: self.0.into(),
            clock_source_id: "synthetic-clock".into(),
            clock_contract_version: "1".into(),
        })
    }
}

/// An owner-only temporary directory, removed on drop.
pub(crate) struct Dir(PathBuf);
impl Dir {
    pub(crate) fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "receipts-or-w2-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
    pub(crate) fn db(&self) -> PathBuf {
        self.0.join("state.db")
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn identity(policy: &str) -> CoreAdmissionIdentity {
    CoreAdmissionIdentity::new(GraphCompilerRefV1::parse(COMPILER).unwrap(), policy.into())
        .unwrap()
        .with_project("p", GraphSourceEvidenceRefV1::parse(SOURCE).unwrap())
}

/// A core writer for project `p` whose latest context epoch is `epochs.last()`.
pub(crate) fn writer_with_epochs(dir: &Dir, policy: &str, epochs: &[&str]) -> CoreStateWriter {
    let mut writer = CoreStateWriter::open(dir.db(), identity(policy)).unwrap();
    for epoch in epochs {
        writer
            .append_context_epoch(ContextEpoch {
                project_id: "p".into(),
                epoch: StateEpochValueV1::try_from(*epoch).unwrap(),
                advanced_at: "2026-11-08T09:00:00Z".into(),
                trigger: ContextEpochTrigger::A1Init,
            })
            .unwrap();
    }
    writer
}
