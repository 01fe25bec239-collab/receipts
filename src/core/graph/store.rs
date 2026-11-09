//! Durable graph persistence through the trusted-core State writer (OR-W2-5).
//!
//! Every write goes through [`CoreStateWriter`], which admits the project,
//! compiler, policy and source before State re-checks the physical tuple.
//! Writes:
//!
//! - [`persist_compiled_graph_v1`]: the admitted genesis (version 1).
//! - [`persist_mutation_v1`]: one apply-v1 outcome as the next version,
//!   published through State's single atomic graph operation: the newly
//!   attached results, the successor snapshot, the mutation history, the seal,
//!   the trusted-time watermark and the head CAS are written in ONE
//!   transaction, so a failure leaves none of them (no orphan result rows).
//!   Before anything is written the stored head must be exactly the parent
//!   the outcome was derived from (version and v1 child digest), so a
//!   concurrent or stale writer gets [`GraphStoreErrorV1::Conflict`]
//!   (retryable after reload) and changes nothing. A mutation id already in
//!   the history is an exact retry only when the stored mutation is
//!   identical and State's recorded attachment membership matches: then the
//!   publication is `Replayed` and writes nothing. Different content under
//!   that id is [`GraphStoreErrorV1::MutationExists`], a different attachment
//!   set (or a legacy mutation with no recorded membership) is
//!   [`GraphStoreErrorV1::ConflictingRetry`].
//!
//! Reads project State's lossless rows back into an
//! [`ExecutionGraphEnvelope`] and recompute the v1 child digest, refusing
//! any mismatch. `created_at` on a reloaded version is State's trusted
//! clock time, not the caller's. The digest profile is v1 throughout; no
//! historical version is rewritten.
//!
//! Not persisted here: the in-memory [`GraphResultLedgerV1`] execution
//! identities and admitted review facts (attempt binding, code
//! classification). Rehydrating them is the Wave3 resume path.

use receipts_state::{
    GraphGenesisReasonV1, GraphPublicationOutcomeV1, PersistedCurrentGraphV1,
    PublicationFailureCode, SqliteStateRepository, StateEpochValueV1, StateError,
    StateGraphEdgeRelationV1, StateGraphEdgeV1, StateGraphGenesisProvenanceV1, StateGraphGenesisV1,
    StateGraphMutationV1, StateGraphNodeResultV1, StateGraphNodeV1, StateGraphSnapshotV1,
    TrustedClockV1,
};

use crate::goal::GoalEvaluationContextEpoch;
use crate::orchestration::{CoreStateWriteError, CoreStateWriter, OrchestrationDateTimeV1};
use crate::persisted_snapshot_projection::{edge as project_edge, node as project_node};
use crate::wire::{GraphMutationWireV1, GraphNodeResultWireV1};
use crate::{
    ExecutionGraph, ExecutionGraphEnvelope, ExecutionGraphEnvelopeFields, GraphEdge, GraphMutation,
    GraphMutationOutcomeV1, GraphNode, GraphNodeResult, GraphSnapshotProjectionErrorV1,
    GraphVersionV1, graph_content_digest_v1,
};

#[derive(Debug)]
pub enum GraphStoreErrorV1 {
    /// The core issuer or State refused the write.
    Write(CoreStateWriteError),
    /// A State read failed.
    Read(StateError),
    /// The stored head is not the parent this successor was derived from.
    /// Nothing was written; reload and re-derive.
    Conflict {
        stored_version: String,
    },
    UnknownGraph,
    /// The mutation id is already in the graph's history with different
    /// content.
    MutationExists,
    /// The mutation is already stored but this is not an exact retry of its
    /// publication (different attachment membership, or a legacy publication
    /// whose membership was never recorded). Nothing was written.
    ConflictingRetry,
    /// The outcome is not the exact successor of the supplied parent.
    NotASuccessor,
    /// A newly attached result is missing from the outcome's ledger.
    MissingResult {
        result_ref: String,
    },
    /// A stored result under this reference has different content.
    ResultConflict {
        result_ref: String,
    },
    Projection(GraphSnapshotProjectionErrorV1),
    /// The stored digest does not match the content that was persisted.
    DigestMismatch,
    /// A stored or supplied value could not be converted between layers.
    Encoding(&'static str),
}

impl std::fmt::Display for GraphStoreErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "graph store refused: {self:?}")
    }
}

impl std::error::Error for GraphStoreErrorV1 {}

impl From<CoreStateWriteError> for GraphStoreErrorV1 {
    fn from(error: CoreStateWriteError) -> Self {
        Self::Write(error)
    }
}

type StoreResult<T> = Result<T, GraphStoreErrorV1>;

/// Persists an admitted genesis and returns the reloaded version 1.
pub fn persist_compiled_graph_v1(
    writer: &mut CoreStateWriter,
    clock: &dyn TrustedClockV1,
    genesis: StateGraphGenesisV1,
) -> StoreResult<ExecutionGraphEnvelope> {
    let (project, graph) = (
        genesis.project_id().to_owned(),
        genesis.graph_id().to_owned(),
    );
    let digest = genesis.resulting_digest().to_owned();
    writer.create_initial_graph(clock, genesis)?;
    let stored = load_current_graph_v1(writer.reader(), &project, &graph)?
        .ok_or(GraphStoreErrorV1::UnknownGraph)?;
    if stored.resulting_digest() != Some(digest.as_str()) {
        return Err(GraphStoreErrorV1::DigestMismatch);
    }
    Ok(stored)
}

/// Persists `outcome` (apply-v1 of `mutation` on `parent`) as the next
/// version and returns the reloaded successor.
pub fn persist_mutation_v1(
    writer: &mut CoreStateWriter,
    clock: &dyn TrustedClockV1,
    parent: &ExecutionGraphEnvelope,
    mutation: &GraphMutation,
    outcome: &GraphMutationOutcomeV1,
) -> StoreResult<ExecutionGraphEnvelope> {
    persist_mutation_outcome_v1(writer, clock, parent, mutation, outcome).map(|(graph, _)| graph)
}

/// As [`persist_mutation_v1`], also reporting whether State wrote the
/// publication (`Applied`) or recognized an exact retry (`Replayed`).
pub fn persist_mutation_outcome_v1(
    writer: &mut CoreStateWriter,
    clock: &dyn TrustedClockV1,
    parent: &ExecutionGraphEnvelope,
    mutation: &GraphMutation,
    outcome: &GraphMutationOutcomeV1,
) -> StoreResult<(ExecutionGraphEnvelope, GraphPublicationOutcomeV1)> {
    use GraphStoreErrorV1 as E;
    let next = &outcome.envelope;
    let (project, graph_id) = (parent.project_id(), parent.graph_id());
    if next.project_id() != project
        || next.graph_id() != graph_id
        || next.parent_version() != Some(Some(parent.graph_version()))
        || mutation.graph_id() != graph_id
        || mutation.parent_version() != parent.graph_version()
        || mutation.resulting_version() != next.graph_version()
    {
        return Err(E::NotASuccessor);
    }
    let reader = writer.reader();
    let stored = reader
        .load_current_graph_v1(project, graph_id)
        .map_err(E::Read)?
        .ok_or(E::UnknownGraph)?;
    let state_mutation =
        StateGraphMutationV1::parse_json(&GraphMutationWireV1::from_mutation(mutation).to_json())
            .map_err(|_| E::Encoding("mutation"))?;
    let stored_same_id = reader
        .list_graph_mutations_v1(project, graph_id)
        .map_err(E::Read)?
        .into_iter()
        .find(|m| m.mutation_id() == mutation.mutation_id());
    match &stored_same_id {
        // An exact retry skips the head check: the head has moved on.
        Some(existing) if existing.to_json().ok() == state_mutation.to_json().ok() => {}
        Some(_) => return Err(E::MutationExists),
        None => {
            let parent_digest = graph_content_digest_v1(parent.graph())
                .map_err(|_| E::Encoding("parent digest"))?;
            if stored.current_version() != parent.graph_version().as_str()
                || stored.resulting_digest() != parent_digest
            {
                return Err(E::Conflict {
                    stored_version: stored.current_version().to_owned(),
                });
            }
        }
    }
    let provenance = stored.genesis_provenance().clone();
    let content = state_content_v1(next, provenance, false).map_err(|_| E::Encoding("snapshot"))?;
    let snapshot = StateGraphSnapshotV1::new(
        next.graph_version().as_str().to_owned(),
        parent.graph_version().as_str().to_owned(),
        content,
    )
    .map_err(|_| E::Encoding("snapshot"))?;

    // Results newly attached by this outcome, bound to the parent version.
    let mut attachments = Vec::new();
    for node in next.graph().nodes() {
        let Some(reference) = node.result_ref() else {
            continue;
        };
        if parent
            .graph()
            .node(node.node_id())
            .is_some_and(|old| old.result_ref() == Some(reference))
        {
            continue;
        }
        let result = outcome
            .results
            .get(reference)
            .ok_or_else(|| E::MissingResult {
                result_ref: reference.to_owned(),
            })?;
        let json = GraphNodeResultWireV1::from_graph_node_result(result).to_json();
        let state_result =
            StateGraphNodeResultV1::parse_json(&json).map_err(|_| E::Encoding("result"))?;
        // A row already stored under this reference must be this very result
        // (State adopts an identical one and rolls everything back otherwise;
        // this names the conflict).
        if let Some(existing) = writer
            .reader()
            .find_node_result_v1(project, graph_id, reference)
            .map_err(E::Read)?
        {
            let same = existing.result().to_json().ok() == state_result.to_json().ok()
                && existing.graph_version() == parent.graph_version().as_str();
            if !same {
                return Err(E::ResultConflict {
                    result_ref: reference.to_owned(),
                });
            }
        }
        attachments.push((
            parent.graph_version().as_str().to_owned(),
            reference.to_owned(),
            state_result,
        ));
    }
    let published = writer
        .publish_graph_mutation(clock, attachments, state_mutation, snapshot)
        .map_err(|error| match error {
            CoreStateWriteError::State(StateError::Publication {
                code: PublicationFailureCode::ConflictingRetry,
                ..
            }) => E::ConflictingRetry,
            other => E::Write(other),
        })?;
    let reloaded = load_graph_version_v1(writer.reader(), project, graph_id, next.graph_version())?
        .ok_or(E::UnknownGraph)?;
    if reloaded.resulting_digest() != next.resulting_digest() || reloaded.graph() != next.graph() {
        return Err(E::DigestMismatch);
    }
    Ok((reloaded, published))
}

/// The current version, or `None` when the graph is not stored.
pub fn load_current_graph_v1(
    reader: &SqliteStateRepository,
    project: &str,
    graph_id: &str,
) -> StoreResult<Option<ExecutionGraphEnvelope>> {
    reader
        .load_current_graph_v1(project, graph_id)
        .map_err(GraphStoreErrorV1::Read)?
        .map(|p| envelope_from_persisted(&p, project, graph_id))
        .transpose()
}

/// One stored version (current or historical).
pub fn load_graph_version_v1(
    reader: &SqliteStateRepository,
    project: &str,
    graph_id: &str,
    version: &GraphVersionV1,
) -> StoreResult<Option<ExecutionGraphEnvelope>> {
    reader
        .load_graph_version_v1(project, graph_id, version.as_str())
        .map_err(GraphStoreErrorV1::Read)?
        .map(|p| envelope_from_persisted(&p, project, graph_id))
        .transpose()
}

/// The stored mutation history in State's deterministic id order.
pub fn load_graph_history_v1(
    reader: &SqliteStateRepository,
    project: &str,
    graph_id: &str,
) -> StoreResult<Vec<GraphMutation>> {
    reader
        .list_graph_mutations_v1(project, graph_id)
        .map_err(GraphStoreErrorV1::Read)?
        .iter()
        .map(|m| {
            let json = m.to_json().map_err(GraphStoreErrorV1::Read)?;
            GraphMutationWireV1::from_json(&json)
                .and_then(|w| w.to_mutation())
                .map_err(|_| GraphStoreErrorV1::Encoding("stored mutation"))
        })
        .collect()
}

/// Stored results attached to `node` at `version`, by reference.
pub fn load_node_results_v1(
    reader: &SqliteStateRepository,
    project: &str,
    graph_id: &str,
    version: &GraphVersionV1,
    node: &str,
) -> StoreResult<Vec<(String, GraphNodeResult)>> {
    reader
        .list_node_results_v1(project, graph_id, version.as_str(), node)
        .map_err(GraphStoreErrorV1::Read)?
        .iter()
        .map(|r| {
            let json = r.result().to_json().map_err(GraphStoreErrorV1::Read)?;
            let result = GraphNodeResultWireV1::from_json(&json)
                .and_then(|w| w.to_graph_node_result())
                .map_err(|_| GraphStoreErrorV1::Encoding("stored result"))?;
            Ok((r.result_ref().to_owned(), result))
        })
        .collect()
}

/// State content for `envelope`. A genesis keeps the envelope's absent or
/// null parent; a successor's parent is set by `StateGraphSnapshotV1`.
pub(crate) fn state_content_v1(
    envelope: &ExecutionGraphEnvelope,
    provenance: StateGraphGenesisProvenanceV1,
    genesis: bool,
) -> Result<StateGraphGenesisV1, StateError> {
    let digest = match envelope.resulting_digest() {
        Some(d) => d.to_owned(),
        None => {
            graph_content_digest_v1(envelope.graph()).map_err(|_| StateError::GraphPersistence {
                phase: receipts_state::GraphPhase::Input,
                code: receipts_state::GraphFailureCode::InvalidShape,
                field: None,
            })?
        }
    };
    let parent = if genesis {
        envelope
            .parent_version()
            .map(|p| p.map(|v| v.as_str().to_owned()))
    } else {
        None
    };
    StateGraphGenesisV1::new(
        envelope.project_id().to_owned(),
        envelope.graph_id().to_owned(),
        envelope.goal_id().to_owned(),
        envelope.policy_id().to_owned(),
        parent,
        envelope.compiled_from().map(<[String]>::to_vec),
        envelope
            .context_epoch()
            .map(|e| StateEpochValueV1::try_from(e.decimal_digits()))
            .transpose()?,
        digest,
        envelope
            .graph()
            .nodes()
            .map(state_node)
            .collect::<Result<_, _>>()?,
        envelope
            .graph()
            .edges()
            .map(state_edge)
            .collect::<Result<_, _>>()?,
        provenance,
    )
}

/// Genesis provenance for a compiled graph.
pub(crate) fn compiled_provenance(
    compiler: receipts_state::GraphCompilerRefV1,
    source: receipts_state::GraphSourceEvidenceRefV1,
) -> StateGraphGenesisProvenanceV1 {
    StateGraphGenesisProvenanceV1::new(compiler, source, GraphGenesisReasonV1::InitialCompilation)
}

fn state_node(node: &GraphNode) -> Result<StateGraphNodeV1, StateError> {
    StateGraphNodeV1::new(
        node.node_id().to_owned(),
        node.graph_id().to_owned(),
        node.kind().as_str().to_owned(),
        node.state().as_str().to_owned(),
        node.title().map(str::to_owned),
        node.parent_node_id().map(str::to_owned),
        node.attempt_number().map(|a| a.as_str().to_owned()),
        node.required_capabilities()
            .map(|caps| caps.iter().map(|c| c.as_str().to_owned()).collect()),
        node.task_capsule_ref().map(str::to_owned),
        node.workstream_id().map(str::to_owned),
        node.code_sha().map(str::to_owned),
        node.workspace_id().map(str::to_owned),
        node.result_ref().map(str::to_owned),
        node.locked_reason().map(|r| r.map(str::to_owned)),
        node.created_in_version().map(|v| v.as_str().to_owned()),
    )
}

fn state_edge(edge: &GraphEdge) -> Result<StateGraphEdgeV1, StateError> {
    let relation = match (edge.precedence_kind(), edge.control_kind()) {
        (Some(kind), _) => StateGraphEdgeRelationV1::Precedence(kind.as_str().to_owned()),
        (None, Some(kind)) => StateGraphEdgeRelationV1::Control(kind.as_str().to_owned()),
        (None, None) => unreachable!("every edge has exactly one kind"),
    };
    StateGraphEdgeV1::new(
        edge.edge_id().to_owned(),
        edge.graph_id().to_owned(),
        edge.from_node().to_owned(),
        edge.to_node().to_owned(),
        relation,
        edge.note().map(str::to_owned),
    )
}

fn envelope_from_persisted(
    persisted: &PersistedCurrentGraphV1,
    project: &str,
    graph_id: &str,
) -> StoreResult<ExecutionGraphEnvelope> {
    use GraphSnapshotProjectionErrorV1 as P;
    let projection = GraphStoreErrorV1::Projection;
    if persisted.project_id() != project || persisted.graph_id() != graph_id {
        return Err(projection(P::IdentityMismatch));
    }
    let nodes = persisted
        .nodes()
        .iter()
        .map(project_node)
        .collect::<Result<Vec<_>, _>>()
        .map_err(projection)?;
    let edges = persisted
        .edges()
        .iter()
        .map(project_edge)
        .collect::<Result<Vec<_>, _>>()
        .map_err(projection)?;
    let graph = ExecutionGraph::from_parts(graph_id, nodes, edges)
        .map_err(|_| projection(P::InvalidTopology))?;
    let digest = graph_content_digest_v1(&graph).map_err(|_| projection(P::DigestFailure))?;
    if digest != persisted.resulting_digest() {
        return Err(projection(P::DigestMismatch));
    }
    let version = GraphVersionV1::try_new(persisted.graph_version())
        .map_err(|_| projection(P::InvalidProjection))?;
    let created_at = OrchestrationDateTimeV1::try_new(persisted.created_at().as_str())
        .map_err(|_| projection(P::InvalidCapturedAt))?;
    let mut fields = ExecutionGraphEnvelopeFields::new(
        project,
        persisted.goal_id(),
        version,
        created_at,
        persisted.policy_id(),
    )
    .with_resulting_digest(digest);
    match persisted.parent_version() {
        None => {}
        Some(None) => fields = fields.with_null_parent_version(),
        Some(Some(parent)) => {
            fields = fields.with_parent_version(
                GraphVersionV1::try_new(parent).map_err(|_| projection(P::InvalidProjection))?,
            )
        }
    }
    if let Some(sources) = persisted.compiled_from() {
        fields = fields.with_compiled_from(sources.to_vec());
    }
    if let Some(epoch) = persisted.context_epoch() {
        fields = fields.with_context_epoch(
            GoalEvaluationContextEpoch::from_decimal(epoch.as_str())
                .map_err(|_| projection(P::InvalidProjection))?,
        );
    }
    ExecutionGraphEnvelope::bind(&graph, fields).map_err(|_| projection(P::InvalidProjection))
}
