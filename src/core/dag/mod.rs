//! `TaskDag` — the reduced read-only compatibility view over the precedence
//! subgraph (TASK_DAG_AND_SCHEDULER): ready tasks, task state and dependency
//! status. It owns no identity, mutation, scheduling or persistence.
//!
//! Dependency semantics:
//!
//! * `REQUIRES_ACCEPTED`: predecessor is `ACCEPTED` or `INTEGRATED`;
//! * `REQUIRES_INTEGRATED`: predecessor is `INTEGRATED`;
//! * `REQUIRES_INTERFACE`: an accepted interface artifact bound to this exact
//!   graph version, edge, predecessor, consumer, code SHA and content hash,
//!   admitted by [`InterfaceEvidenceVerifier`] from a trusted read-only
//!   [`InterfaceArtifactSource`]. Predecessor state, a reference string or a
//!   caller boolean never satisfies it; with no source it is *unavailable*.
//!
//! Control edges never gate readiness. Human-gate nodes and nodes in
//! `HUMAN_REQUIRED`, `LOCKED_REQUIRES_PRO` or any state other than
//! `PLANNED`/`READY` are never ready.

use sha2::{Digest, Sha256};

use crate::{
    ExecutionGraph, ExecutionGraphEnvelope, GraphEdge, GraphNodeKind, GraphNodeState,
    GraphVersionV1, PrecedenceKind,
};

#[cfg(test)]
mod tests;

/// The trusted source could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfaceSourceUnavailable;

/// Acceptance provenance as recorded by the trusted source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceProvenance {
    Accepted { acceptance_id: String },
    NotAccepted,
}

/// What the trusted source records for one interface edge. Its claims are
/// checked, not believed: every binding is compared with the graph and the
/// content hash is recomputed from the source's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceArtifactRecord {
    pub graph_id: String,
    pub graph_version: String,
    pub edge_id: String,
    pub source_node_id: String,
    pub consumer_node_id: String,
    pub artifact_id: String,
    pub code_sha: String,
    pub content_sha256: String,
    pub provenance: InterfaceProvenance,
}

/// Read-only port to the trusted interface-artifact store. No production
/// implementation exists in Wave1; Host/State composition supplies it later.
pub trait InterfaceArtifactSource {
    fn interface_record(
        &self,
        graph_id: &str,
        edge_id: &str,
    ) -> Result<Option<InterfaceArtifactRecord>, InterfaceSourceUnavailable>;
    fn artifact_content(
        &self,
        artifact_id: &str,
    ) -> Result<Option<Vec<u8>>, InterfaceSourceUnavailable>;
}

/// Why interface evidence was not admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceEvidenceRefusal {
    /// No trusted source was supplied: unavailable, never true.
    NoSource,
    SourceUnavailable,
    NotInterfaceEdge,
    MissingRecord,
    WrongGraph,
    StaleVersion,
    WrongEdge,
    WrongSource,
    WrongConsumer,
    NotAccepted,
    PredecessorWithoutCode,
    CodeMismatch,
    MissingContent,
    ContentMismatch,
}

impl InterfaceEvidenceRefusal {
    /// Whether the evidence is merely unavailable rather than contradicted.
    pub fn is_unavailable(self) -> bool {
        matches!(
            self,
            Self::NoSource | Self::SourceUnavailable | Self::MissingRecord | Self::MissingContent
        )
    }
}

/// Admitted evidence; only [`InterfaceEvidenceVerifier::verify`] creates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedInterfaceEvidence {
    edge_id: String,
    artifact_id: String,
    code_sha: String,
    content_sha256: String,
    acceptance_id: String,
}

impl AdmittedInterfaceEvidence {
    pub fn edge_id(&self) -> &str {
        &self.edge_id
    }
    pub fn artifact_id(&self) -> &str {
        &self.artifact_id
    }
    pub fn code_sha(&self) -> &str {
        &self.code_sha
    }
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
    pub fn acceptance_id(&self) -> &str {
        &self.acceptance_id
    }
}

/// Admits `REQUIRES_INTERFACE` evidence from a trusted read-only source.
#[derive(Clone, Copy)]
pub struct InterfaceEvidenceVerifier<'s> {
    source: &'s dyn InterfaceArtifactSource,
}

impl<'s> InterfaceEvidenceVerifier<'s> {
    pub fn new(source: &'s dyn InterfaceArtifactSource) -> Self {
        Self { source }
    }

    /// Checks every binding of the source record against `graph` at
    /// `version`, then recomputes the content SHA-256 from the source bytes.
    pub fn verify(
        &self,
        graph: &ExecutionGraph,
        version: &GraphVersionV1,
        edge: &GraphEdge,
    ) -> Result<AdmittedInterfaceEvidence, InterfaceEvidenceRefusal> {
        use InterfaceEvidenceRefusal as R;
        if edge.precedence_kind() != Some(PrecedenceKind::RequiresInterface) {
            return Err(R::NotInterfaceEdge);
        }
        let record = self
            .source
            .interface_record(graph.graph_id(), edge.edge_id())
            .map_err(|_| R::SourceUnavailable)?
            .ok_or(R::MissingRecord)?;
        let checks = [
            (record.graph_id == graph.graph_id(), R::WrongGraph),
            (record.graph_version == version.as_str(), R::StaleVersion),
            (record.edge_id == edge.edge_id(), R::WrongEdge),
            (record.source_node_id == edge.from_node(), R::WrongSource),
            (record.consumer_node_id == edge.to_node(), R::WrongConsumer),
        ];
        if let Some((_, refusal)) = checks.into_iter().find(|(ok, _)| !ok) {
            return Err(refusal);
        }
        let InterfaceProvenance::Accepted { acceptance_id } = &record.provenance else {
            return Err(R::NotAccepted);
        };
        if acceptance_id.is_empty() {
            return Err(R::NotAccepted);
        }
        let predecessor_sha = graph
            .node(edge.from_node())
            .and_then(|node| node.code_sha())
            .ok_or(R::PredecessorWithoutCode)?;
        if record.code_sha != predecessor_sha {
            return Err(R::CodeMismatch);
        }
        let content = self
            .source
            .artifact_content(&record.artifact_id)
            .map_err(|_| R::SourceUnavailable)?
            .ok_or(R::MissingContent)?;
        let actual: String = Sha256::digest(&content)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != record.content_sha256 {
            return Err(R::ContentMismatch);
        }
        Ok(AdmittedInterfaceEvidence {
            edge_id: record.edge_id,
            artifact_id: record.artifact_id,
            code_sha: record.code_sha,
            content_sha256: actual,
            acceptance_id: acceptance_id.clone(),
        })
    }
}

/// One precedence dependency's status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyStatus {
    Satisfied,
    Unsatisfied,
    InterfaceAdmitted(AdmittedInterfaceEvidence),
    InterfaceRefused(InterfaceEvidenceRefusal),
}

impl DependencyStatus {
    pub fn is_satisfied(&self) -> bool {
        matches!(self, Self::Satisfied | Self::InterfaceAdmitted(_))
    }
}

/// A dependency of a node: the incoming precedence edge and its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyCheck {
    pub edge_id: String,
    pub predecessor: String,
    pub kind: PrecedenceKind,
    pub status: DependencyStatus,
}

/// Ready candidates, plus candidates held only by unavailable evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadyNodes {
    pub ready: Vec<String>,
    pub awaiting_evidence: Vec<String>,
}

/// Read-only readiness view over one graph version.
pub struct TaskDag<'a> {
    graph: &'a ExecutionGraph,
    version: &'a GraphVersionV1,
    interface: Option<InterfaceEvidenceVerifier<'a>>,
}

impl<'a> TaskDag<'a> {
    pub fn new(
        envelope: &'a ExecutionGraphEnvelope,
        interface: Option<InterfaceEvidenceVerifier<'a>>,
    ) -> Self {
        Self::over(envelope.graph(), envelope.graph_version(), interface)
    }

    pub(crate) fn over(
        graph: &'a ExecutionGraph,
        version: &'a GraphVersionV1,
        interface: Option<InterfaceEvidenceVerifier<'a>>,
    ) -> Self {
        Self {
            graph,
            version,
            interface,
        }
    }

    /// Query 2: the exact stored task state.
    pub fn task_state(&self, node_id: &str) -> Option<GraphNodeState> {
        self.graph.node_state(node_id)
    }

    /// Query 3: every incoming precedence dependency, ascending edge id.
    pub fn dependency_status(&self, node_id: &str) -> Option<Vec<DependencyCheck>> {
        self.graph.node(node_id)?;
        Some(
            self.graph
                .precedence_edges()
                .filter(|edge| edge.to_node() == node_id)
                .map(|edge| {
                    let kind = edge.precedence_kind().expect("precedence edge");
                    let state = self.graph.node_state(edge.from_node());
                    let status = match kind {
                        PrecedenceKind::RequiresAccepted => satisfied(matches!(
                            state,
                            Some(GraphNodeState::Accepted | GraphNodeState::Integrated)
                        )),
                        PrecedenceKind::RequiresIntegrated => {
                            satisfied(state == Some(GraphNodeState::Integrated))
                        }
                        PrecedenceKind::RequiresInterface => match &self.interface {
                            None => DependencyStatus::InterfaceRefused(
                                InterfaceEvidenceRefusal::NoSource,
                            ),
                            Some(verifier) => match verifier.verify(self.graph, self.version, edge)
                            {
                                Ok(evidence) => DependencyStatus::InterfaceAdmitted(evidence),
                                Err(refusal) => DependencyStatus::InterfaceRefused(refusal),
                            },
                        },
                    };
                    DependencyCheck {
                        edge_id: edge.edge_id().to_owned(),
                        predecessor: edge.from_node().to_owned(),
                        kind,
                        status,
                    }
                })
                .collect(),
        )
    }

    /// Query 1: dispatchable candidates in ascending node id order.
    pub fn ready_nodes(&self) -> ReadyNodes {
        let mut out = ReadyNodes::default();
        for node in self.graph.nodes() {
            if !matches!(
                node.state(),
                GraphNodeState::Planned | GraphNodeState::Ready
            ) || node.kind() == &GraphNodeKind::HUMAN_GATE
            {
                continue;
            }
            let checks = self.dependency_status(node.node_id()).expect("node exists");
            if checks.iter().all(|c| c.status.is_satisfied()) {
                out.ready.push(node.node_id().to_owned());
            } else if checks.iter().all(|c| {
                c.status.is_satisfied()
                    || matches!(c.status, DependencyStatus::InterfaceRefused(r) if r.is_unavailable())
            }) {
                out.awaiting_evidence.push(node.node_id().to_owned());
            }
        }
        out
    }
}

fn satisfied(ok: bool) -> DependencyStatus {
    if ok {
        DependencyStatus::Satisfied
    } else {
        DependencyStatus::Unsatisfied
    }
}
