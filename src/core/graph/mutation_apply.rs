//! Apply-v1: the explicit executable interpretation of a frozen
//! `GraphMutation` (A1 Wave1 completion disposition). Decoding a mutation
//! never executes it; only [`apply_graph_mutation_v1`] does, with these exact
//! operation detail shapes:
//!
//! | op | target | detail |
//! |---|---|---|
//! | `ADD_NODE` | `node_id` | `{node: GraphNode}` |
//! | `ADD_EDGE` | `edge_id` | `{edge: GraphEdge}` |
//! | `SET_NODE_STATE` | `node_id` | `{from: state, to: state, cause: {kind, ..}}` |
//! | `ATTACH_RESULT` | `node_id` | `{result_ref: string, result: GraphNodeResult}` |
//! | `CANCEL_NODE` | `node_id` | absent or `{}` |
//! | `EXPAND_REPAIR` | rejected REVIEW `node_id` | `{nodes: [GraphNode], edges: [GraphEdge]}` |
//!
//! Other schema-valid detail is valid wire data but unsupported here. The
//! whole ordered batch is validated and applied to a private candidate; the
//! v1 child digest is computed once and compared with a supplied digest; on
//! any refusal the inputs are unchanged and nothing is returned.
//!
//! Actor authority comes only from the separately supplied
//! [`GraphMutationScopeV1`] (checked policy over admitted facts, not an
//! authenticated production issuer); `mutation.actor` must merely agree with
//! it. Nothing here persists to State, deletes structure, rewrites a result,
//! or accepts work implicitly.
//!
//! Execution identity is bound across calls: `DISPATCH` admits an attempt id
//! into the returned [`GraphResultLedgerV1`]; `START`, `SUBMIT` and
//! `ATTACH_RESULT` must match it. Resumption, admission and dispatch recheck
//! the current prerequisites, and `EXPAND_REPAIR` requires a complete fresh
//! repair/review round hung off a retained rejected `REVIEW`.

use std::collections::{BTreeMap, BTreeSet};

use crate::dag::{DependencyStatus, InterfaceEvidenceVerifier, TaskDag};
use crate::node_result::GraphNodeResult;
use crate::node_state_machine::{NodeTransitionError, TransitionCauseKind, check_node_transition};
use crate::orchestration::{OrchestrationJsonObjectV1, OrchestrationJsonValueV1 as V};
use crate::wire::{
    GraphWireError, graph_edge_from_value, graph_node_from_value, graph_node_result_from_value,
};
use crate::{
    CapabilityName, ControlKind, EdgeClass, ExecutionGraph, ExecutionGraphEnvelope, GraphEdge,
    GraphError, GraphMutation, GraphMutationOperation, GraphMutationOperationKind,
    GraphMutationPreflightError, GraphNode, GraphNodeAttemptNumber, GraphNodeKind,
    GraphNodeResultOutcome, GraphNodeState, GraphVersionV1, MAX_IDENTIFIER_LENGTH,
    content_digest::GraphContentDigestError, validate_graph_mutation_preflight,
};

/// The admitted caller actor. Built by trusted core from its own facts,
/// never from `mutation.actor`. `RUNTIME_A3`, `RUNTIME_A4` and the genesis
/// `GRAPH_COMPILER` have no successor-mutation scope and cannot be expressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopedActorV1 {
    User {
        role_id: Option<String>,
    },
    RuntimeA1 {
        role_id: Option<String>,
    },
    RuntimeA2 {
        role_id: Option<String>,
        workstream_id: String,
    },
    GoalEvaluator {
        role_id: Option<String>,
    },
}

impl ScopedActorV1 {
    fn role(&self) -> &'static str {
        match self {
            Self::User { .. } => "USER",
            Self::RuntimeA1 { .. } => "RUNTIME_A1",
            Self::RuntimeA2 { .. } => "RUNTIME_A2",
            Self::GoalEvaluator { .. } => "GOAL_EVALUATOR",
        }
    }
    fn role_id(&self) -> Option<&str> {
        match self {
            Self::User { role_id }
            | Self::RuntimeA1 { role_id }
            | Self::RuntimeA2 { role_id, .. }
            | Self::GoalEvaluator { role_id } => role_id.as_deref(),
        }
    }
    fn is_global(&self) -> bool {
        matches!(self, Self::User { .. } | Self::RuntimeA1 { .. })
    }
}

/// Core caller context the mutation must match exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphMutationScopeV1 {
    project_id: String,
    graph_id: String,
    graph_version: GraphVersionV1,
    actor: ScopedActorV1,
}

impl GraphMutationScopeV1 {
    pub fn new(
        project_id: impl Into<String>,
        graph_id: impl Into<String>,
        graph_version: GraphVersionV1,
        actor: ScopedActorV1,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            graph_id: graph_id.into(),
            graph_version,
            actor,
        }
    }
}

/// `(project_id, graph_id, node_id)`: the identity an attempt is admitted for.
type AttemptKey = (String, String, String);

/// The retained execution record of a graph: attached results by immutable
/// `result_ref`, plus the attempt identity admitted at `DISPATCH` for each
/// `(project, graph, node)`. Both are append-only through apply-v1 and travel
/// in the returned outcome, so `START`, `SUBMIT` and `ATTACH_RESULT` in later
/// calls or batches verify against what was actually dispatched. A ledger
/// built with [`GraphResultLedgerV1::new`] holds no dispatch: executing a node
/// that was never dispatched through it refuses as a missing binding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphResultLedgerV1 {
    results: BTreeMap<String, GraphNodeResult>,
    /// The node's current admitted attempt (set by `DISPATCH`).
    current: BTreeMap<AttemptKey, String>,
    /// Every attempt id ever admitted, with its owner. Never reused.
    admitted: BTreeMap<String, AttemptKey>,
}

impl GraphResultLedgerV1 {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn get(&self, result_ref: &str) -> Option<&GraphNodeResult> {
        self.results.get(result_ref)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &GraphNodeResult)> {
        self.results.iter().map(|(k, v)| (k.as_str(), v))
    }
    pub fn len(&self) -> usize {
        self.results.len()
    }
    pub fn is_empty(&self) -> bool {
        self.results.is_empty()
    }
    /// The attempt id currently admitted for the node, if it was dispatched.
    pub fn attempt(&self, project_id: &str, graph_id: &str, node_id: &str) -> Option<&str> {
        self.current
            .get(&(project_id.into(), graph_id.into(), node_id.into()))
            .map(String::as_str)
    }
}

/// The single successful outcome: the next envelope and result ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphMutationOutcomeV1 {
    pub envelope: ExecutionGraphEnvelope,
    pub results: GraphResultLedgerV1,
}

/// Why one operation refused (the whole batch refuses with it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphOperationError {
    MissingTarget,
    UnexpectedTarget,
    TargetMismatch,
    /// Detail is missing, has an unrelated/missing key or a malformed value.
    Detail(&'static str),
    Wire(Box<GraphWireError>),
    UnknownNode,
    StateMismatch {
        expected: GraphNodeState,
        actual: GraphNodeState,
    },
    Transition(NodeTransitionError),
    /// A REVIEW verdict (PASSED/REJECTED) is retained, never revised.
    RetainedVerdict,
    /// Required external evidence is not available: refused, never assumed.
    EvidenceUnavailable(&'static str),
    EvidenceRefused(&'static str),
    ResultSlotOccupied,
    ResultAssociation(&'static str),
    /// `DISPATCH`/`START`/`SUBMIT` attempt identity is reused, missing or
    /// does not match the one admitted for the node.
    Attempt(&'static str),
    NewNode(&'static str),
    Repair(&'static str),
    NotPermitted(&'static str),
    Graph(Box<GraphError>),
}

impl From<GraphWireError> for GraphOperationError {
    fn from(error: GraphWireError) -> Self {
        Self::Wire(Box::new(error))
    }
}
impl From<GraphError> for GraphOperationError {
    fn from(error: GraphError) -> Self {
        Self::Graph(Box::new(error))
    }
}
impl From<NodeTransitionError> for GraphOperationError {
    fn from(error: NodeTransitionError) -> Self {
        Self::Transition(error)
    }
}

/// Why apply-v1 returned no outcome. Inputs are always unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphMutationApplyError {
    Preflight(GraphMutationPreflightError),
    /// The envelope or mutation disagrees with the caller scope field.
    Scope(&'static str),
    ResultingVersionNotSuccessor,
    Operation {
        index: usize,
        error: GraphOperationError,
    },
    Digest(GraphContentDigestError),
    DigestMismatch,
}

impl std::fmt::Display for GraphMutationApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preflight(error) => error.fmt(f),
            Self::Scope(field) => write!(f, "mutation does not match caller scope at {field}"),
            Self::ResultingVersionNotSuccessor => {
                f.write_str("resulting_version is not the exact successor")
            }
            Self::Operation { index, error } => write!(f, "operations[{index}] refused: {error:?}"),
            Self::Digest(error) => error.fmt(f),
            Self::DigestMismatch => f.write_str("resulting_digest does not match computed digest"),
        }
    }
}

impl std::error::Error for GraphMutationApplyError {}

/// Applies `mutation` to `envelope`/`results` atomically under `scope`.
/// `interface` is the optional trusted interface-evidence verifier used when
/// `PLANNED → READY` depends on a `REQUIRES_INTERFACE` edge.
pub fn apply_graph_mutation_v1(
    envelope: &ExecutionGraphEnvelope,
    results: &GraphResultLedgerV1,
    mutation: &GraphMutation,
    scope: &GraphMutationScopeV1,
    interface: Option<InterfaceEvidenceVerifier<'_>>,
) -> Result<GraphMutationOutcomeV1, GraphMutationApplyError> {
    use GraphMutationApplyError as E;
    validate_graph_mutation_preflight(envelope.graph(), envelope.graph_version(), mutation)
        .map_err(E::Preflight)?;
    if scope.project_id != envelope.project_id() {
        return Err(E::Scope("project_id"));
    }
    if scope.graph_id != envelope.graph_id() {
        return Err(E::Scope("graph_id"));
    }
    if &scope.graph_version != envelope.graph_version() {
        return Err(E::Scope("graph_version"));
    }
    if mutation.actor().role() != scope.actor.role() {
        return Err(E::Scope("actor.role"));
    }
    if mutation.actor().role_id() != scope.actor.role_id() {
        return Err(E::Scope("actor.role_id"));
    }
    let successor = envelope.graph_version().successor();
    if mutation.resulting_version() != &successor {
        return Err(E::ResultingVersionNotSuccessor);
    }
    let mut batch = Batch {
        graph: envelope.graph().clone(),
        ledger: results.clone(),
        project_id: envelope.project_id(),
        added: BTreeSet::new(),
        actor: &scope.actor,
        parent: envelope.graph_version(),
        successor,
        interface,
    };
    for (index, operation) in mutation.operations().iter().enumerate() {
        batch
            .apply(operation)
            .map_err(|error| E::Operation { index, error })?;
    }
    let envelope = envelope
        .successor(
            batch.graph,
            mutation.created_at().clone(),
            mutation.resulting_digest(),
        )
        .map_err(|error| error.map_or(E::DigestMismatch, E::Digest))?;
    Ok(GraphMutationOutcomeV1 {
        envelope,
        results: batch.ledger,
    })
}

type OpResult<T = ()> = Result<T, GraphOperationError>;
type Map = BTreeMap<String, V>;

struct Batch<'a> {
    graph: ExecutionGraph,
    ledger: GraphResultLedgerV1,
    project_id: &'a str,
    added: BTreeSet<String>,
    actor: &'a ScopedActorV1,
    parent: &'a GraphVersionV1,
    successor: GraphVersionV1,
    interface: Option<InterfaceEvidenceVerifier<'a>>,
}

fn detail<'d>(op: &'d GraphMutationOperation, fields: &[&'static str]) -> OpResult<&'d Map> {
    let map = op
        .detail()
        .map(OrchestrationJsonObjectV1::as_map)
        .ok_or(GraphOperationError::Detail("detail"))?;
    exact_keys(map, fields)?;
    Ok(map)
}

fn exact_keys(map: &Map, fields: &[&'static str]) -> OpResult {
    if let Some(missing) = fields.iter().find(|f| !map.contains_key(**f)) {
        return Err(GraphOperationError::Detail(missing));
    }
    if map.len() != fields.len() {
        return Err(GraphOperationError::Detail("unrelated key"));
    }
    Ok(())
}

fn text<'m>(map: &'m Map, field: &'static str) -> OpResult<&'m str> {
    match map.get(field) {
        Some(V::String(s)) => Ok(s),
        _ => Err(GraphOperationError::Detail(field)),
    }
}

/// A non-empty identifier of at most 200 scalars.
fn id<'m>(map: &'m Map, field: &'static str) -> OpResult<&'m str> {
    let value = text(map, field)?;
    if value.is_empty() || value.chars().count() > MAX_IDENTIFIER_LENGTH {
        return Err(GraphOperationError::Detail(field));
    }
    Ok(value)
}

fn state(map: &Map, field: &'static str) -> OpResult<GraphNodeState> {
    GraphNodeState::parse(text(map, field)?).map_err(|_| GraphOperationError::Detail(field))
}

fn is_review(node: &GraphNode) -> bool {
    node.kind() == &GraphNodeKind::REVIEW
}

fn is_retained_verdict(node: &GraphNode) -> bool {
    is_review(node)
        && matches!(
            node.state(),
            GraphNodeState::Passed | GraphNodeState::Rejected
        )
}

impl Batch<'_> {
    fn apply(&mut self, op: &GraphMutationOperation) -> OpResult {
        use GraphMutationOperationKind as K;
        let wants_edge = op.op() == K::AddEdge;
        match (op.node_id(), op.edge_id(), wants_edge) {
            (_, Some(_), false) | (Some(_), _, true) => {
                return Err(GraphOperationError::UnexpectedTarget);
            }
            (None, _, false) | (_, None, true) => return Err(GraphOperationError::MissingTarget),
            _ => {}
        }
        if matches!(self.actor, ScopedActorV1::GoalEvaluator { .. })
            && !matches!(op.op(), K::AddNode | K::AddEdge)
        {
            return Err(GraphOperationError::NotPermitted(
                "goal evaluator only appends",
            ));
        }
        match op.op() {
            K::AddNode => {
                let node = graph_node_from_value(&detail(op, &["node"])?["node"])?;
                if Some(node.node_id()) != op.node_id() {
                    return Err(GraphOperationError::TargetMismatch);
                }
                self.add_node(node, false)
            }
            K::AddEdge => {
                let edge = graph_edge_from_value(&detail(op, &["edge"])?["edge"])?;
                if Some(edge.edge_id()) != op.edge_id() {
                    return Err(GraphOperationError::TargetMismatch);
                }
                self.add_edge(edge)
            }
            K::SetNodeState => self.set_state(op),
            K::AttachResult => self.attach_result(op),
            K::CancelNode => self.cancel(op),
            K::ExpandRepair => self.expand_repair(op),
        }
    }

    fn existing(&self, op: &GraphMutationOperation) -> OpResult<GraphNode> {
        let node = self
            .graph
            .node(op.node_id().expect("checked target"))
            .ok_or(GraphOperationError::UnknownNode)?
            .clone();
        self.in_scope(&node)?;
        Ok(node)
    }

    fn in_scope(&self, node: &GraphNode) -> OpResult {
        if let ScopedActorV1::RuntimeA2 { workstream_id, .. } = self.actor
            && node.workstream_id() != Some(workstream_id.as_str())
        {
            return Err(GraphOperationError::NotPermitted(
                "outside admitted workstream",
            ));
        }
        Ok(())
    }

    fn add_node(&mut self, node: GraphNode, repair: bool) -> OpResult {
        let allowed = if repair {
            node.state() == GraphNodeState::Planned
        } else {
            matches!(
                node.state(),
                GraphNodeState::Planned
                    | GraphNodeState::HumanRequired
                    | GraphNodeState::LockedRequiresPro
            )
        };
        if !allowed {
            return Err(GraphOperationError::NewNode("state"));
        }
        if node.result_ref().is_some() {
            return Err(GraphOperationError::NewNode("result_ref"));
        }
        if node
            .created_in_version()
            .is_some_and(|v| v != &self.successor)
        {
            return Err(GraphOperationError::NewNode("created_in_version"));
        }
        self.in_scope(&node)?;
        let node_id = node.node_id().to_owned();
        self.graph.add_node(node)?;
        self.added.insert(node_id);
        Ok(())
    }

    fn add_edge(&mut self, edge: GraphEdge) -> OpResult {
        if matches!(self.actor, ScopedActorV1::GoalEvaluator { .. })
            && !self.added.contains(edge.to_node())
        {
            return Err(GraphOperationError::NotPermitted(
                "goal evaluator edges must target appended nodes",
            ));
        }
        for endpoint in [edge.from_node(), edge.to_node()] {
            if let Some(node) = self.graph.node(endpoint) {
                self.in_scope(node)?;
            }
        }
        self.graph.add_edge(edge)?;
        Ok(())
    }

    fn set_state(&mut self, op: &GraphMutationOperation) -> OpResult {
        let map = detail(op, &["from", "to", "cause"])?;
        let (from, to) = (state(map, "from")?, state(map, "to")?);
        let node = self.existing(op)?;
        if node.state() != from {
            return Err(GraphOperationError::StateMismatch {
                expected: from,
                actual: node.state(),
            });
        }
        if is_retained_verdict(&node) && to != GraphNodeState::Accepted {
            return Err(GraphOperationError::RetainedVerdict);
        }
        let V::Object(cause) = &map["cause"] else {
            return Err(GraphOperationError::Detail("cause"));
        };
        let cause = cause.as_map();
        let kind = cause_kind(text(cause, "kind")?)?;
        check_node_transition(from, to, kind)?;
        self.verify_cause(&node, to, kind, cause)?;
        self.bind_attempt(&node, kind, cause)?;
        self.graph.replace_node(node.with_state(to));
        Ok(())
    }

    /// `DISPATCH` admits a never-used attempt id for this exact
    /// project/graph/node; `START` and `SUBMIT` must name the attempt
    /// currently admitted for it. A missing or different binding refuses.
    fn bind_attempt(
        &mut self,
        node: &GraphNode,
        kind: TransitionCauseKind,
        cause: &Map,
    ) -> OpResult {
        use GraphOperationError as E;
        use TransitionCauseKind as C;
        if !matches!(kind, C::Dispatch | C::Start | C::Submit) {
            return Ok(());
        }
        let attempt = id(cause, "attempt_id")?;
        let key = (
            self.project_id.to_owned(),
            self.graph.graph_id().to_owned(),
            node.node_id().to_owned(),
        );
        if kind == C::Dispatch {
            if self.ledger.admitted.contains_key(attempt) {
                return Err(E::Attempt("attempt id already admitted"));
            }
            self.ledger.admitted.insert(attempt.to_owned(), key.clone());
            self.ledger.current.insert(key, attempt.to_owned());
            return Ok(());
        }
        match self.ledger.current.get(&key) {
            None => Err(E::Attempt("node has no admitted attempt")),
            Some(admitted) if admitted != attempt => Err(E::Attempt("attempt id not admitted")),
            Some(_) => Ok(()),
        }
    }

    /// Every incoming precedence dependency must be satisfied *now*, against
    /// the candidate graph (earlier operations of this batch included) and
    /// the current parent version for interface evidence. Unavailable
    /// interface evidence is reported as such, never assumed.
    fn require_dependencies(&self, node: &GraphNode) -> OpResult {
        use GraphOperationError as E;
        let dag = TaskDag::over(&self.graph, self.parent, self.interface);
        let checks = dag.dependency_status(node.node_id()).expect("node exists");
        for check in &checks {
            match &check.status {
                status if status.is_satisfied() => {}
                DependencyStatus::InterfaceRefused(r) if r.is_unavailable() => {
                    return Err(E::EvidenceUnavailable("interface artifact"));
                }
                _ => return Err(E::EvidenceRefused("dependencies")),
            }
        }
        Ok(())
    }

    fn verify_cause(
        &self,
        node: &GraphNode,
        to: GraphNodeState,
        kind: TransitionCauseKind,
        cause: &Map,
    ) -> OpResult {
        use GraphOperationError as E;
        use TransitionCauseKind as C;
        let fields: &[&'static str] = match kind {
            C::DependenciesSatisfied | C::CapabilityAdmission => &["kind"],
            C::Admission => &["kind", "admission_decision_id"],
            C::Dispatch | C::Start | C::Submit => &["kind", "attempt_id"],
            C::ReviewVerdict => &["kind", "result_node_id"],
            C::RepairEntry | C::RepairSubmitted => &["kind", "repair_node_id"],
            C::Acceptance | C::Integration => &["kind", "decision_id"],
            C::Blocked | C::HumanRequired => &["kind", "reason"],
            C::Resolved => &["kind", "resolution_ref"],
            C::HumanResolved => &["kind", "instruction_ref"],
            C::FeatureLocked => &["kind", "capability_id"],
            C::Cancellation => unreachable!("not decodable as a cause"),
        };
        exact_keys(cause, fields)?;
        for field in &fields[1..] {
            if matches!(*field, "reason") {
                if text(cause, field)?.is_empty() {
                    return Err(E::Detail("reason"));
                }
            } else if *field != "capability_id" {
                id(cause, field)?;
            }
        }
        match kind {
            C::DependenciesSatisfied => {
                if node.kind() == &GraphNodeKind::HUMAN_GATE {
                    return Err(E::EvidenceRefused("human gate"));
                }
                self.require_dependencies(node)
            }
            C::ReviewVerdict => self.verify_verdict(node, to, id(cause, "result_node_id")?),
            C::RepairEntry => {
                let repair = self.repair_child(node, id(cause, "repair_node_id")?)?;
                let mut sources = self.graph.control_edges().filter(|edge| {
                    edge.to_node() == repair.node_id()
                        && edge.control_kind() == Some(ControlKind::OnReject)
                });
                match (sources.next(), sources.next()) {
                    (Some(edge), None) => repair_provenance(&self.graph, edge.from_node(), repair),
                    _ => Err(E::Repair("repair needs exactly one ON_REJECT source")),
                }
            }
            C::RepairSubmitted => {
                let repair = self.repair_child(node, id(cause, "repair_node_id")?)?;
                if !matches!(
                    repair.state(),
                    GraphNodeState::AwaitingReview
                        | GraphNodeState::Passed
                        | GraphNodeState::Accepted
                        | GraphNodeState::Integrated
                ) {
                    return Err(E::EvidenceRefused("repair not submitted"));
                }
                Ok(())
            }
            C::Acceptance | C::Integration if !self.actor.is_global() => Err(E::NotPermitted(
                "acceptance and integration need RUNTIME_A1 or USER",
            )),
            C::Resolved | C::HumanResolved if node.result_ref().is_some() => Err(
                E::EvidenceRefused("resuming would reuse an attached result; expand a repair"),
            ),
            C::HumanResolved if !matches!(self.actor, ScopedActorV1::User { .. }) => {
                Err(E::NotPermitted("only USER resolves a human gate"))
            }
            // Resumption, fresh admission and dispatch recheck the current
            // prerequisites; a resolution or admission reference proves none.
            C::Resolved | C::HumanResolved | C::Admission | C::Dispatch => {
                self.require_dependencies(node)
            }
            C::FeatureLocked => {
                let capability = CapabilityName::new(text(cause, "capability_id")?)
                    .map_err(|_| E::Detail("capability_id"))?;
                if !node
                    .required_capabilities()
                    .is_some_and(|caps| caps.contains(&capability))
                {
                    return Err(E::EvidenceRefused("capability not required by node"));
                }
                Ok(())
            }
            C::CapabilityAdmission => Err(E::EvidenceUnavailable(
                "verified capability admission (no Wave1 source)",
            )),
            _ => Ok(()),
        }
    }

    fn repair_child(&self, subject: &GraphNode, repair_id: &str) -> OpResult<&GraphNode> {
        let repair = self
            .graph
            .node(repair_id)
            .ok_or(GraphOperationError::EvidenceRefused("unknown repair node"))?;
        if repair.parent_node_id() != Some(subject.node_id()) {
            return Err(GraphOperationError::EvidenceRefused(
                "repair parent mismatch",
            ));
        }
        Ok(repair)
    }

    /// Verdict evidence is an attached result: the subject's own (first
    /// round, no repair children yet) or that of its newest REPAIR child,
    /// which must already carry the same verdict. A rejected round's result
    /// is never reused for a later verdict.
    fn verify_verdict(&self, node: &GraphNode, to: GraphNodeState, cited_id: &str) -> OpResult {
        use GraphOperationError as E;
        let expected = if to == GraphNodeState::Passed {
            GraphNodeResultOutcome::Pass
        } else {
            GraphNodeResultOutcome::Rejected
        };
        let newest = self.newest_repair_child(node)?;
        let cited = if cited_id == node.node_id() {
            if newest.is_some() {
                return Err(E::EvidenceRefused(
                    "repaired node needs its repair's verdict",
                ));
            }
            node
        } else {
            match newest {
                Some(child) if child.node_id() == cited_id && child.state() == to => child,
                _ => return Err(E::EvidenceRefused("cited result is not the current round")),
            }
        };
        let result = cited
            .result_ref()
            .and_then(|r| self.ledger.get(r))
            .ok_or(E::EvidenceUnavailable("attached review result"))?;
        if result.outcome() != expected || result.review_id().is_none() {
            return Err(E::EvidenceRefused("review result outcome"));
        }
        if cited.code_sha().is_some() && result.code_sha() != cited.code_sha() {
            return Err(E::EvidenceRefused("review result code_sha"));
        }
        Ok(())
    }

    /// The unique REPAIR child with the greatest `created_in_version`.
    fn newest_repair_child(&self, subject: &GraphNode) -> OpResult<Option<&GraphNode>> {
        let key = |n: &GraphNode| {
            n.created_in_version()
                .map(|v| (v.as_str().len(), v.as_str().to_owned()))
                .unwrap_or_default()
        };
        let mut children: Vec<&GraphNode> = self
            .graph
            .nodes()
            .filter(|n| {
                n.kind() == &GraphNodeKind::REPAIR && n.parent_node_id() == Some(subject.node_id())
            })
            .collect();
        children.sort_by_key(|n| key(n));
        match children.as_slice() {
            [] => Ok(None),
            [.., a, b] if key(a) == key(b) => Err(GraphOperationError::EvidenceRefused(
                "ambiguous repair round",
            )),
            [.., last] => Ok(Some(last)),
        }
    }

    fn attach_result(&mut self, op: &GraphMutationOperation) -> OpResult {
        use GraphOperationError as E;
        let map = detail(op, &["result_ref", "result"])?;
        let result_ref = text(map, "result_ref")?;
        if result_ref.is_empty() {
            return Err(E::Detail("result_ref"));
        }
        let result = graph_node_result_from_value(&map["result"])?;
        let node = self.existing(op)?;
        if node.result_ref().is_some() || self.ledger.get(result_ref).is_some() {
            return Err(E::ResultSlotOccupied);
        }
        if result.graph_id() != self.graph.graph_id() {
            return Err(E::ResultAssociation("graph_id"));
        }
        if result.node_id() != node.node_id() {
            return Err(E::ResultAssociation("node_id"));
        }
        if node.code_sha().is_some() && result.code_sha() != node.code_sha() {
            return Err(E::ResultAssociation("code_sha"));
        }
        // Evidence must come from the attempt dispatched for this exact
        // project/graph/node; a result claiming its own identity proves none.
        let admitted = self
            .ledger
            .attempt(self.project_id, self.graph.graph_id(), node.node_id())
            .ok_or(E::Attempt("node has no admitted attempt"))?;
        if result.attempt_id() != Some(admitted) {
            return Err(E::ResultAssociation("attempt_id"));
        }
        self.ledger.results.insert(result_ref.to_owned(), result);
        self.graph.replace_node(node.with_result_ref(result_ref));
        Ok(())
    }

    fn cancel(&mut self, op: &GraphMutationOperation) -> OpResult {
        if op.detail().is_some_and(|d| !d.as_map().is_empty()) {
            return Err(GraphOperationError::Detail("unrelated key"));
        }
        let node = self.existing(op)?;
        if is_retained_verdict(&node) {
            return Err(GraphOperationError::RetainedVerdict);
        }
        check_node_transition(
            node.state(),
            GraphNodeState::Cancelled,
            TransitionCauseKind::Cancellation,
        )?;
        self.graph
            .replace_node(node.with_state(GraphNodeState::Cancelled));
        Ok(())
    }

    /// Expands a retained `REJECTED` REVIEW into a fresh repair round: one
    /// `REPAIR` (parent = the implementation that review judged), one
    /// distinct fresh `REVIEW` after it, the review's `ON_REJECT` control edge
    /// to the repair and the repair's forward precedence to the new review.
    /// The rejected review, its result and attempts are never touched.
    fn expand_repair(&mut self, op: &GraphMutationOperation) -> OpResult {
        use GraphOperationError as E;
        let map = detail(op, &["nodes", "edges"])?;
        let source = self.existing(op)?;
        if !is_review(&source) || source.state() != GraphNodeState::Rejected {
            return Err(E::Repair("source must be a REJECTED REVIEW"));
        }
        let (V::Array(nodes), V::Array(edges)) = (&map["nodes"], &map["edges"]) else {
            return Err(E::Detail("nodes/edges"));
        };
        let nodes = nodes
            .iter()
            .map(graph_node_from_value)
            .collect::<Result<Vec<_>, _>>()?;
        let edges = edges
            .iter()
            .map(graph_edge_from_value)
            .collect::<Result<Vec<_>, _>>()?;
        if nodes.iter().any(|n| n.created_in_version().is_none()) {
            return Err(E::NewNode("created_in_version"));
        }
        let source_id = source.node_id();
        if self.graph.control_edges().any(|edge| {
            edge.control_kind() == Some(ControlKind::OnReject) && edge.from_node() == source_id
        }) {
            return Err(E::Repair("rejected review is already expanded"));
        }
        let subject = reviewed_subject(&self.graph, source_id)?;
        let repair = only_new(&nodes, &GraphNodeKind::REPAIR)?;
        let review = only_new(&nodes, &GraphNodeKind::REVIEW)?;
        if repair.parent_node_id() != Some(subject.node_id()) {
            return Err(E::Repair("repair parent must be the reviewed subject"));
        }
        if repair.workstream_id() != subject.workstream_id()
            || review.workstream_id() != repair.workstream_id()
        {
            return Err(E::Repair("repair round workstream binding"));
        }
        fn round(n: &GraphNode) -> Option<&str> {
            n.attempt_number().map(GraphNodeAttemptNumber::as_str)
        }
        let later = |new: Option<&str>, old: Option<&str>| match (new, old) {
            (Some(new), Some(old)) => (new.len(), new) > (old.len(), old),
            _ => true,
        };
        if round(repair)
            .zip(round(review))
            .is_some_and(|(a, b)| a != b)
            || !later(round(repair), round(subject))
            || !later(round(review), round(&source))
        {
            return Err(E::Repair("repair round ordinals"));
        }
        let new_ids: BTreeSet<&str> = nodes.iter().map(GraphNode::node_id).collect();
        if edges.iter().any(|edge| {
            edge.class() == EdgeClass::Precedence
                && (edge.from_node() == source_id || edge.to_node() == source_id)
        }) {
            return Err(E::Repair(
                "precedence edges may not touch the rejected source",
            ));
        }
        if edges
            .iter()
            .any(|e| !new_ids.contains(e.from_node()) && !new_ids.contains(e.to_node()))
        {
            return Err(E::Repair("every edge must touch an expanded node"));
        }
        let (repair_id, review_id) = (repair.node_id().to_owned(), review.node_id().to_owned());
        for node in nodes {
            self.add_node(node, true)?;
        }
        for edge in edges {
            self.add_edge(edge)?;
        }
        // The staged result must carry the complete provenance, and the new
        // review must judge exactly this repair.
        let repair = self.graph.node(&repair_id).expect("staged");
        repair_provenance(&self.graph, source_id, repair)?;
        if reviewed_subject(&self.graph, &review_id)?.node_id() != repair_id {
            return Err(E::Repair("new review must judge exactly the repair"));
        }
        Ok(())
    }
}

/// The only node of `kind` in an expansion; none or several refuse.
fn only_new<'n>(nodes: &'n [GraphNode], kind: &GraphNodeKind) -> OpResult<&'n GraphNode> {
    let mut of_kind = nodes.iter().filter(|n| n.kind() == kind);
    match (of_kind.next(), of_kind.next()) {
        (Some(node), None) => Ok(node),
        _ => Err(GraphOperationError::Repair(
            "expansion needs exactly one REPAIR and one REVIEW node",
        )),
    }
}

/// The unique execution subject (`TASK`/`IMPLEMENTATION`/`REPAIR`) that
/// precedes `review_id` by a PRECEDENCE edge: the prior work that review
/// judged. None or several refuse.
fn reviewed_subject<'g>(graph: &'g ExecutionGraph, review_id: &str) -> OpResult<&'g GraphNode> {
    let subjects: BTreeSet<&str> = graph
        .precedence_edges()
        .filter(|edge| edge.to_node() == review_id)
        .filter_map(|edge| graph.node(edge.from_node()))
        .filter(|n| {
            [
                GraphNodeKind::TASK,
                GraphNodeKind::IMPLEMENTATION,
                GraphNodeKind::REPAIR,
            ]
            .contains(n.kind())
        })
        .map(GraphNode::node_id)
        .collect();
    let mut subjects = subjects.into_iter();
    match (subjects.next(), subjects.next()) {
        (Some(subject), None) => Ok(graph.node(subject).expect("listed")),
        (None, _) => Err(GraphOperationError::Repair(
            "rejected review has no reviewed subject",
        )),
        _ => Err(GraphOperationError::Repair(
            "rejected review subject is ambiguous",
        )),
    }
}

/// Complete provenance of `repair` against its `ON_REJECT` source: the source
/// is a retained `REJECTED` REVIEW; `repair.parent_node_id` is the
/// implementation that review judged; the control edge exists; and the
/// repair precedes exactly one other, distinct REVIEW that judges it.
fn repair_provenance(graph: &ExecutionGraph, source_id: &str, repair: &GraphNode) -> OpResult {
    use GraphOperationError as E;
    let source = graph
        .node(source_id)
        .filter(|n| is_review(n) && n.state() == GraphNodeState::Rejected)
        .ok_or(E::Repair("source must be a REJECTED REVIEW"))?;
    let subject = reviewed_subject(graph, source.node_id())?;
    if repair.kind() != &GraphNodeKind::REPAIR || repair.parent_node_id() != Some(subject.node_id())
    {
        return Err(E::Repair("repair parent must be the reviewed subject"));
    }
    let on_reject = graph.control_edges().any(|edge| {
        edge.control_kind() == Some(ControlKind::OnReject)
            && edge.from_node() == source.node_id()
            && edge.to_node() == repair.node_id()
    });
    if !on_reject {
        return Err(E::Repair("missing ON_REJECT control edge to the repair"));
    }
    let reviews: BTreeSet<&str> = graph
        .precedence_edges()
        .filter(|edge| edge.from_node() == repair.node_id())
        .filter_map(|edge| graph.node(edge.to_node()))
        .filter(|n| is_review(n))
        .map(GraphNode::node_id)
        .collect();
    let mut reviews = reviews.into_iter();
    match (reviews.next(), reviews.next()) {
        (Some(fresh), None) if fresh != source.node_id() => Ok(()),
        _ => Err(E::Repair("repair needs one distinct fresh REVIEW after it")),
    }
}

fn cause_kind(value: &str) -> OpResult<TransitionCauseKind> {
    use TransitionCauseKind as C;
    Ok(match value {
        "DEPENDENCIES_SATISFIED" => C::DependenciesSatisfied,
        "ADMISSION" => C::Admission,
        "DISPATCH" => C::Dispatch,
        "START" => C::Start,
        "SUBMIT" => C::Submit,
        "REVIEW_VERDICT" => C::ReviewVerdict,
        "REPAIR_ENTRY" => C::RepairEntry,
        "REPAIR_SUBMITTED" => C::RepairSubmitted,
        "ACCEPTANCE" => C::Acceptance,
        "INTEGRATION" => C::Integration,
        "BLOCKED" => C::Blocked,
        "HUMAN_REQUIRED" => C::HumanRequired,
        "RESOLVED" => C::Resolved,
        "HUMAN_RESOLVED" => C::HumanResolved,
        "FEATURE_LOCKED" => C::FeatureLocked,
        "CAPABILITY_ADMISSION" => C::CapabilityAdmission,
        _ => return Err(GraphOperationError::Detail("cause.kind")),
    })
}
