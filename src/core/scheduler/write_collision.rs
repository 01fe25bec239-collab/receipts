//! Resolving a write collision between two graph nodes (S10).
//!
//! Exactly one resolution per collision, never "both, carefully"
//! (CONCURRENCY_MODEL): DEFER changes nothing and the later writer waits;
//! SERIALIZE and CANONICAL_WRITER add one `REQUIRES_ACCEPTED` precedence edge
//! from the writer that goes first to the other. The edge is a real
//! `GraphMutation` executed by apply-v1 under the caller's
//! [`GraphMutationScopeV1`], so the adopted scope decides who may add it
//! (a `RUNTIME_A2` only inside its own workstream; A1/USER globally) and the
//! graph refuses cycles. Nothing here persists; the caller persists the
//! returned mutation through the graph store.
//!
//! A collision must be shown first: a proven-disjoint pair has nothing to
//! resolve. CANONICAL_WRITER additionally needs the adopted workstream
//! ownership: the writer's workstream must provably own the overlap witness
//! and the consumer's must provably not. Uncertainty refuses.

use super::write_scope_port::{
    WriteScopeCollisionV1, WriteScopeOverlapV1, WriteScopePort, classify_write_collision,
};
use crate::orchestration::{OrchestrationDateTimeV1, OrchestrationJsonObjectV1};
use crate::{
    ExecutionGraphEnvelope, GraphEdge, GraphMutation, GraphMutationActor, GraphMutationApplyError,
    GraphMutationOperation, GraphMutationOperationKind, GraphMutationOutcomeV1,
    GraphMutationScopeV1, GraphRecordError, GraphResultLedgerV1, PrecedenceKind,
    apply_graph_mutation_v1,
};

/// One side of a collision: a graph node and its declared write scope.
#[derive(Debug, Clone, Copy)]
pub struct CollidingWriterV1<'a> {
    pub node_id: &'a str,
    pub write_paths: &'a [String],
}

/// The adopted owned paths of one workstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkstreamOwnershipV1 {
    pub workstream_id: String,
    pub owned_paths: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum WriteCollisionResolutionV1<'a> {
    Defer,
    /// `first` runs to acceptance before `second`.
    Serialize {
        first: &'a str,
        second: &'a str,
    },
    /// The owner of the overlapping path writes; the other consumes it.
    CanonicalWriter {
        ownership: &'a [WorkstreamOwnershipV1],
    },
}

/// Who adds the edge and with which identities. Authority is the scope.
pub struct CollisionMutationV1<'a> {
    pub envelope: &'a ExecutionGraphEnvelope,
    pub results: &'a GraphResultLedgerV1,
    pub scope: &'a GraphMutationScopeV1,
    pub actor: GraphMutationActor,
    pub mutation_id: &'a str,
    pub edge_id: &'a str,
    pub created_at: OrchestrationDateTimeV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteCollisionResolvedV1 {
    Deferred,
    Ordered {
        writer: String,
        consumer: String,
        mutation: Box<GraphMutation>,
        outcome: Box<GraphMutationOutcomeV1>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteCollisionResolutionErrorV1 {
    /// The scopes are proven disjoint: nothing to resolve.
    NoCollision,
    /// SERIALIZE named nodes other than the two colliding writers.
    NotTheCollidingPair,
    /// CANONICAL_WRITER needs a concrete overlap witness.
    NoWitness,
    UnknownNode,
    /// A colliding node has no workstream, or its workstream has no adopted ownership.
    OwnershipMissing,
    /// Ownership of the witness could not be decided for both workstreams.
    OwnershipUndetermined,
    /// Not exactly one of the two workstreams owns the witness.
    NoCanonicalOwner,
    Record(GraphRecordError),
    /// apply-v1 refused the edge (scope, cycle, identity, ...).
    Apply(GraphMutationApplyError),
}

/// Resolves the collision between `left` and `right` as `resolution` says.
pub fn resolve_write_collision_v1(
    port: &impl WriteScopePort,
    left: CollidingWriterV1<'_>,
    right: CollidingWriterV1<'_>,
    resolution: WriteCollisionResolutionV1<'_>,
    mutation: CollisionMutationV1<'_>,
) -> Result<WriteCollisionResolvedV1, WriteCollisionResolutionErrorV1> {
    use WriteCollisionResolutionErrorV1 as E;
    let collision = classify_write_collision(port, left.write_paths, right.write_paths);
    if collision == WriteScopeCollisionV1::Concurrent {
        return Err(E::NoCollision);
    }
    let (writer, consumer) = match resolution {
        WriteCollisionResolutionV1::Defer => return Ok(WriteCollisionResolvedV1::Deferred),
        WriteCollisionResolutionV1::Serialize { first, second } => {
            let pair = (first == left.node_id && second == right.node_id)
                || (first == right.node_id && second == left.node_id);
            if !pair {
                return Err(E::NotTheCollidingPair);
            }
            (first, second)
        }
        WriteCollisionResolutionV1::CanonicalWriter { ownership } => {
            let WriteScopeCollisionV1::Conflict { witness } = &collision else {
                return Err(E::NoWitness);
            };
            let owns = |node_id: &str| -> Result<bool, WriteCollisionResolutionErrorV1> {
                let node = mutation
                    .envelope
                    .graph()
                    .node(node_id)
                    .ok_or(E::UnknownNode)?;
                let workstream = node.workstream_id().ok_or(E::OwnershipMissing)?;
                let owned = ownership
                    .iter()
                    .find(|o| o.workstream_id == workstream)
                    .ok_or(E::OwnershipMissing)?;
                match port.overlap(std::slice::from_ref(witness), &owned.owned_paths) {
                    WriteScopeOverlapV1::Overlaps { .. } => Ok(true),
                    WriteScopeOverlapV1::Disjoint => Ok(false),
                    WriteScopeOverlapV1::Undetermined => Err(E::OwnershipUndetermined),
                }
            };
            match (owns(left.node_id)?, owns(right.node_id)?) {
                (true, false) => (left.node_id, right.node_id),
                (false, true) => (right.node_id, left.node_id),
                _ => return Err(E::NoCanonicalOwner),
            }
        }
    };
    let (record, outcome) = add_precedence_edge(&mutation, writer, consumer)?;
    Ok(WriteCollisionResolvedV1::Ordered {
        writer: writer.to_owned(),
        consumer: consumer.to_owned(),
        mutation: Box::new(record),
        outcome: Box::new(outcome),
    })
}

fn add_precedence_edge(
    m: &CollisionMutationV1<'_>,
    from: &str,
    to: &str,
) -> Result<(GraphMutation, GraphMutationOutcomeV1), WriteCollisionResolutionErrorV1> {
    use WriteCollisionResolutionErrorV1 as E;
    let graph_id = m.envelope.graph_id();
    let edge = GraphEdge::precedence(
        m.edge_id,
        graph_id,
        from,
        to,
        PrecedenceKind::RequiresAccepted,
    )
    .map_err(|e| E::Record(e.into()))?;
    let mut detail = OrchestrationJsonObjectV1::new(Default::default());
    detail.insert("edge".to_owned(), crate::wire::edge_to_value(&edge));
    let operation = GraphMutationOperation::try_new(
        GraphMutationOperationKind::AddEdge,
        None,
        Some(m.edge_id.to_owned()),
        Some(detail),
    )
    .map_err(E::Record)?;
    let record = GraphMutation::try_new(
        m.mutation_id,
        graph_id,
        m.envelope.graph_version().clone(),
        m.envelope.graph_version().successor(),
        m.actor.clone(),
        format!("serialize write collision: {from} before {to}"),
        m.created_at.clone(),
        vec![operation],
        None,
    )
    .map_err(E::Record)?;
    let outcome =
        apply_graph_mutation_v1(m.envelope, m.results, &record, m.scope, None).map_err(E::Apply)?;
    Ok((record, outcome))
}
