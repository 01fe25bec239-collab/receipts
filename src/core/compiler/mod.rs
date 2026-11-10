//! GraphCompiler v1: verbatim spec + decomposition proposal → genesis graph
//! (OR-W2-4).
//!
//! [`compile_graph_v1`] is a pure, deterministic function of its inputs. It
//! makes no model call: the decomposition proposal is data the composition
//! obtained (for example from a frontier RUNTIME-A1 call) and hands in. The
//! only read is the spec itself, resolved through the composition's
//! admitted-content resolver and kept byte-for-byte.
//!
//! Output: a version-1 [`ExecutionGraphEnvelope`] and the matching State
//! genesis with `GRAPH_COMPILER` provenance (the admitted compiler id, the
//! project's admitted source and `INITIAL_COMPILATION`). `compiled_from`
//! names the spec reference and the SHA-256 of its exact bytes, so a reload
//! can be checked against the verbatim spec.
//!
//! Canonical shape: node ids are the proposal's (validated); precedence
//! edges are numbered `e1..eN` in topological order of their target, then
//! their source, ties broken by node id. Proposal order never matters.
//! Initial states: `HUMAN_GATE` nodes start `HUMAN_REQUIRED`; a node whose
//! required capability is `LOCKED_REQUIRES_PRO` starts `LOCKED_REQUIRES_PRO`
//! with a fixed visible reason; every other node starts `PLANNED`. A
//! capability with no supplied admission outcome, or an unknown/expired one,
//! refuses: entitlement is never inferred (no live catalog here).
//!
//! Coherence (GOAL_ORCHESTRATOR decomposition check):
//! - too granular: workstreams with identical owned scopes are one
//!   ownership unit and are merged into the lexicographically smallest id;
//! - no code ownership: a workstream with no owned paths is refused, as is
//!   a node naming an unproposed workstream;
//! - no stable interface: workstreams whose owned scopes overlap (or whose
//!   overlap the matcher cannot decide) must be re-cut; refused.
//!
//! Acceptance criteria are required. A `DERIVED` criterion, or a
//! deterministic one with no check command (unmeasurable), compiles only
//! after the user explicitly confirmed it; otherwise the typed refusal lists
//! every criterion needing confirmation.

use std::collections::{BTreeMap, BTreeSet};

use receipts_state::{
    GraphCompilerRefV1, GraphSourceEvidenceRefV1, StateGraphGenesisV1,
    entitlement::ProductEntitlementState,
};
use sha2::{Digest, Sha256};

use crate::capsules::{Criterion, CriterionKind};
use crate::goal::GoalEvaluationContextEpoch;
use crate::orchestration::{
    CoreContentResolverV1, FeatureAdmissionOutcome, OrchestrationDateTimeV1,
};
use crate::scheduler::{WorkstreamOwnershipV1, WriteScopeOverlapV1, WriteScopePort};
use crate::store::{compiled_provenance, state_content_v1};
use crate::{
    CapabilityName, ExecutionGraph, ExecutionGraphEnvelope, ExecutionGraphEnvelopeFields,
    GraphEdge, GraphError, GraphNode, GraphNodeKind, GraphNodeState, GraphVersionV1,
    PrecedenceKind, graph_content_digest_v1,
};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedNodeV1 {
    pub node_id: String,
    pub kind: GraphNodeKind,
    pub title: Option<String>,
    pub workstream_id: Option<String>,
    pub required_capabilities: Option<Vec<CapabilityName>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedDependencyV1 {
    pub from: String,
    pub to: String,
    pub kind: PrecedenceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CriterionOriginV1 {
    /// Taken from the user's spec.
    Stated,
    /// Proposed by the system; requires user confirmation.
    Derived,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedCriterionV1 {
    pub criterion: Criterion,
    pub origin: CriterionOriginV1,
}

/// A decomposition proposal: data, never authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecompositionProposalV1 {
    pub workstreams: Vec<WorkstreamOwnershipV1>,
    pub nodes: Vec<ProposedNodeV1>,
    pub dependencies: Vec<ProposedDependencyV1>,
    pub criteria: Vec<ProposedCriterionV1>,
}

/// Trusted compile context supplied by the composition.
pub struct CompileContextV1<'a> {
    pub project_id: &'a str,
    pub graph_id: &'a str,
    pub goal_id: &'a str,
    pub policy_id: &'a str,
    pub created_at: OrchestrationDateTimeV1,
    pub context_epoch: Option<GoalEvaluationContextEpoch>,
    pub compiler_id: GraphCompilerRefV1,
    pub source: GraphSourceEvidenceRefV1,
    /// The spec reference, resolved verbatim through `content`.
    pub spec_ref: &'a str,
    pub content: &'a dyn CoreContentResolverV1,
    /// Admitted feature outcome per required capability.
    pub capability_admission: &'a BTreeMap<String, FeatureAdmissionOutcome>,
    pub entitlement_state: ProductEntitlementState,
    /// Criterion ids the user explicitly confirmed.
    pub confirmed_criteria: &'a BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledGraphV1 {
    pub envelope: ExecutionGraphEnvelope,
    pub genesis: StateGraphGenesisV1,
    /// The spec bytes exactly as resolved.
    pub spec: Vec<u8>,
    pub spec_sha256: String,
    /// Adopted ownership after merging.
    pub workstreams: Vec<WorkstreamOwnershipV1>,
    /// `(merged, into)` pairs, ascending.
    pub merged_workstreams: Vec<(String, String)>,
    pub criteria: Vec<ProposedCriterionV1>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompileErrorV1 {
    SpecUnavailable,
    EmptyProposal,
    NoAcceptanceCriteria,
    DuplicateCriterion(String),
    /// Derived or unmeasurable criteria the user has not confirmed.
    CriteriaRequireConfirmation(Vec<String>),
    DuplicateWorkstream(String),
    WorkstreamWithoutOwnership(String),
    /// Two workstreams' owned scopes overlap: re-cut the boundary.
    OverlappingOwnership {
        left: String,
        right: String,
        witness: String,
    },
    OwnershipUndetermined {
        left: String,
        right: String,
    },
    UnownedWorkstream {
        node_id: String,
        workstream_id: String,
    },
    DuplicateNode(String),
    UnknownDependencyEndpoint(String),
    DuplicateDependency {
        from: String,
        to: String,
    },
    SelfDependency(String),
    PrecedenceCycle,
    CapabilityAdmissionUnresolved(String),
    Graph(GraphError),
    Encoding(&'static str),
}

impl std::fmt::Display for CompileErrorV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "graph not compiled: {self:?}")
    }
}

impl std::error::Error for CompileErrorV1 {}

impl From<GraphError> for CompileErrorV1 {
    fn from(error: GraphError) -> Self {
        Self::Graph(error)
    }
}

/// Compiles `proposal` against the verbatim spec. See the module docs.
pub fn compile_graph_v1(
    context: &CompileContextV1<'_>,
    proposal: &DecompositionProposalV1,
    port: &impl WriteScopePort,
) -> Result<CompiledGraphV1, CompileErrorV1> {
    use CompileErrorV1 as E;
    if proposal.nodes.is_empty() {
        return Err(E::EmptyProposal);
    }
    let spec = context
        .content
        .resolve(context.project_id, context.spec_ref)
        .ok_or(E::SpecUnavailable)?;
    let spec_sha256: String = Sha256::digest(&spec)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    // Criteria: required, unique, confirmed where derived or unmeasurable.
    if proposal.criteria.is_empty() {
        return Err(E::NoAcceptanceCriteria);
    }
    let mut ids = BTreeSet::new();
    for c in &proposal.criteria {
        if !ids.insert(c.criterion.id()) {
            return Err(E::DuplicateCriterion(c.criterion.id().to_owned()));
        }
    }
    let unconfirmed: Vec<String> = proposal
        .criteria
        .iter()
        .filter(|c| {
            let unmeasurable = c.criterion.kind() == CriterionKind::Deterministic
                && c.criterion.check_command().is_none_or(<[String]>::is_empty);
            (c.origin == CriterionOriginV1::Derived || unmeasurable)
                && !context.confirmed_criteria.contains(c.criterion.id())
        })
        .map(|c| c.criterion.id().to_owned())
        .collect();
    if !unconfirmed.is_empty() {
        return Err(E::CriteriaRequireConfirmation(unconfirmed));
    }

    let (workstreams, merged) = coherent_workstreams(&proposal.workstreams, port)?;
    let merged_into: BTreeMap<&str, &str> = merged
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();

    // Nodes.
    let mut nodes: BTreeMap<&str, GraphNode> = BTreeMap::new();
    for proposed in &proposal.nodes {
        let state = initial_state(context, proposed)?;
        let mut node = GraphNode::new(
            proposed.node_id.as_str(),
            context.graph_id,
            proposed.kind.clone(),
            state.0,
        )?;
        if let Some(title) = &proposed.title {
            node = node.with_title(title.as_str());
        }
        if let Some(ws) = &proposed.workstream_id {
            let ws = merged_into.get(ws.as_str()).copied().unwrap_or(ws);
            if !workstreams.iter().any(|w| w.workstream_id == ws) {
                return Err(E::UnownedWorkstream {
                    node_id: proposed.node_id.clone(),
                    workstream_id: ws.to_owned(),
                });
            }
            node = node.with_workstream_id(ws)?;
        }
        if let Some(caps) = &proposed.required_capabilities {
            node = node.with_required_capabilities(caps.clone());
        }
        if let Some(reason) = state.1 {
            node = node.with_locked_reason(reason);
        }
        if nodes.insert(proposed.node_id.as_str(), node).is_some() {
            return Err(E::DuplicateNode(proposed.node_id.clone()));
        }
    }

    // Dependencies: known endpoints, no duplicates or self loops, acyclic.
    let mut pairs = BTreeSet::new();
    let mut successors: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut indegree: BTreeMap<&str, usize> = nodes.keys().map(|k| (*k, 0)).collect();
    for d in &proposal.dependencies {
        for end in [&d.from, &d.to] {
            if !nodes.contains_key(end.as_str()) {
                return Err(E::UnknownDependencyEndpoint(end.clone()));
            }
        }
        if d.from == d.to {
            return Err(E::SelfDependency(d.from.clone()));
        }
        if !pairs.insert((d.from.as_str(), d.to.as_str())) {
            return Err(E::DuplicateDependency {
                from: d.from.clone(),
                to: d.to.clone(),
            });
        }
        successors.entry(&d.from).or_default().push(&d.to);
        *indegree.get_mut(d.to.as_str()).expect("known endpoint") += 1;
    }
    let mut order: BTreeMap<&str, usize> = BTreeMap::new();
    let mut frontier: BTreeSet<&str> = indegree
        .iter()
        .filter(|(_, n)| **n == 0)
        .map(|(k, _)| *k)
        .collect();
    while let Some(next) = frontier.pop_first() {
        order.insert(next, order.len());
        for to in successors.get(next).into_iter().flatten() {
            let n = indegree.get_mut(to).expect("known endpoint");
            *n -= 1;
            if *n == 0 {
                frontier.insert(to);
            }
        }
    }
    if order.len() != nodes.len() {
        return Err(E::PrecedenceCycle);
    }
    let mut deps: Vec<&ProposedDependencyV1> = proposal.dependencies.iter().collect();
    deps.sort_by_key(|d| (order[d.to.as_str()], order[d.from.as_str()]));

    let mut graph = ExecutionGraph::new(context.graph_id)?;
    for node in nodes.into_values() {
        graph.add_node(node)?;
    }
    for (index, d) in deps.iter().enumerate() {
        graph.add_edge(GraphEdge::precedence(
            format!("e{}", index + 1),
            context.graph_id,
            d.from.as_str(),
            d.to.as_str(),
            d.kind,
        )?)?;
    }

    let digest = graph_content_digest_v1(&graph).map_err(|_| E::Encoding("digest"))?;
    let mut fields = ExecutionGraphEnvelopeFields::new(
        context.project_id,
        context.goal_id,
        GraphVersionV1::try_new("1").expect("version one"),
        context.created_at.clone(),
        context.policy_id,
    )
    .with_resulting_digest(digest)
    .with_compiled_from(vec![
        context.spec_ref.to_owned(),
        format!("sha256:{spec_sha256}"),
    ]);
    if let Some(epoch) = &context.context_epoch {
        fields = fields.with_context_epoch(epoch.clone());
    }
    let envelope =
        ExecutionGraphEnvelope::bind(&graph, fields).map_err(|_| E::Encoding("envelope"))?;
    let genesis = state_content_v1(
        &envelope,
        compiled_provenance(context.compiler_id.clone(), context.source.clone()),
        true,
    )
    .map_err(|_| E::Encoding("genesis"))?;
    Ok(CompiledGraphV1 {
        envelope,
        genesis,
        spec,
        spec_sha256,
        workstreams,
        merged_workstreams: merged,
        criteria: proposal.criteria.clone(),
    })
}

type Coherent = (Vec<WorkstreamOwnershipV1>, Vec<(String, String)>);

fn coherent_workstreams(
    proposed: &[WorkstreamOwnershipV1],
    port: &impl WriteScopePort,
) -> Result<Coherent, CompileErrorV1> {
    use CompileErrorV1 as E;
    // Normalised scope → smallest workstream id owning exactly that scope.
    let mut by_scope: BTreeMap<Vec<String>, String> = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut sorted: Vec<&WorkstreamOwnershipV1> = proposed.iter().collect();
    sorted.sort_by(|a, b| a.workstream_id.cmp(&b.workstream_id));
    let mut merged = Vec::new();
    for ws in sorted {
        if !seen.insert(ws.workstream_id.as_str()) {
            return Err(E::DuplicateWorkstream(ws.workstream_id.clone()));
        }
        let mut scope = ws.owned_paths.clone();
        scope.sort();
        scope.dedup();
        if scope.is_empty() {
            return Err(E::WorkstreamWithoutOwnership(ws.workstream_id.clone()));
        }
        match by_scope.get(&scope) {
            Some(into) => merged.push((ws.workstream_id.clone(), into.clone())),
            None => {
                by_scope.insert(scope, ws.workstream_id.clone());
            }
        }
    }
    let mut kept: Vec<WorkstreamOwnershipV1> = by_scope
        .into_iter()
        .map(|(owned_paths, workstream_id)| WorkstreamOwnershipV1 {
            workstream_id,
            owned_paths,
        })
        .collect();
    kept.sort_by(|a, b| a.workstream_id.cmp(&b.workstream_id));
    for (i, left) in kept.iter().enumerate() {
        for right in &kept[i + 1..] {
            match port.overlap(&left.owned_paths, &right.owned_paths) {
                WriteScopeOverlapV1::Disjoint => {}
                WriteScopeOverlapV1::Overlaps { witness } => {
                    return Err(E::OverlappingOwnership {
                        left: left.workstream_id.clone(),
                        right: right.workstream_id.clone(),
                        witness,
                    });
                }
                WriteScopeOverlapV1::Undetermined => {
                    return Err(E::OwnershipUndetermined {
                        left: left.workstream_id.clone(),
                        right: right.workstream_id.clone(),
                    });
                }
            }
        }
    }
    merged.sort();
    Ok((kept, merged))
}

/// The node's initial state and locked reason.
fn initial_state(
    context: &CompileContextV1<'_>,
    node: &ProposedNodeV1,
) -> Result<(GraphNodeState, Option<String>), CompileErrorV1> {
    let mut locked = None;
    for capability in node.required_capabilities.iter().flatten() {
        match context.capability_admission.get(capability.as_str()) {
            Some(FeatureAdmissionOutcome::Allow) => {}
            Some(FeatureAdmissionOutcome::LockedRequiresPro) => {
                locked.get_or_insert_with(|| capability.as_str().to_owned());
            }
            _ => {
                return Err(CompileErrorV1::CapabilityAdmissionUnresolved(
                    capability.as_str().to_owned(),
                ));
            }
        }
    }
    Ok(match locked {
        Some(capability) => (
            GraphNodeState::LockedRequiresPro,
            Some(format!(
                "Requires {capability}; entitlement is {}. Visible, never dispatched.",
                context.entitlement_state.as_str()
            )),
        ),
        None if node.kind == GraphNodeKind::HUMAN_GATE => (GraphNodeState::HumanRequired, None),
        None => (GraphNodeState::Planned, None),
    })
}
