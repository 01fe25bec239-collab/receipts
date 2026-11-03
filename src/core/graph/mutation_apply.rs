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
//!
//! An aggregate verdict and its acceptance rest on independent review
//! evidence that trusted core admits through a [`ReviewEvidenceVerifier`]
//! (see [`apply_graph_mutation_with_evidence_v1`]); the worker's own result and
//! an opaque `review_id` prove nothing. Every attempt is code or non-code only
//! by trusted core's say-so: a known node SHA or
//! [`GraphResultLedgerV1::admit_attempt_code_sha`] makes it code, and
//! [`GraphResultLedgerV1::admit_attempt_non_code`] makes it non-code. An
//! omitted SHA classifies nothing: until admitted, results, verdicts and
//! acceptance for the attempt are unavailable.

use std::collections::{BTreeMap, BTreeSet};

use receipts_workspace_execution::CommitSha;

use crate::dag::{DependencyStatus, InterfaceEvidenceVerifier, TaskDag};
use crate::node_result::GraphNodeResult;
use crate::node_state_machine::{NodeTransitionError, TransitionCauseKind, check_node_transition};
use crate::orchestration::{OrchestrationJsonObjectV1, OrchestrationJsonValueV1 as V};
use crate::review_evidence::{
    ReviewEvidenceRefusal, ReviewEvidenceVerifier, ReviewOutcome, ReviewedWork,
};
use crate::wire::{
    GraphWireError, graph_edge_from_value, graph_node_from_value, graph_node_result_from_value,
};
use crate::{
    CapabilityName, ControlKind, EdgeClass, ExecutionGraph, ExecutionGraphEnvelope, GraphEdge,
    GraphError, GraphMutation, GraphMutationOperation, GraphMutationOperationKind,
    GraphMutationPreflightError, GraphNode, GraphNodeAttemptNumber, GraphNodeKind,
    GraphNodeResultOutcome, GraphNodeState, GraphVersionV1, MAX_IDENTIFIER_LENGTH, PrecedenceKind,
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

/// Whether an attempt produced code. Unclassified is the default: an absent
/// SHA is not evidence of non-code. Only trusted core leaves it, once.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AttemptDomain {
    Unclassified,
    Code(String),
    NonCode,
}

impl AttemptDomain {
    /// The SHA a review of this attempt must name (none for non-code).
    fn sha(&self) -> Option<&str> {
        match self {
            Self::Code(sha) => Some(sha),
            _ => None,
        }
    }
}

/// What `DISPATCH` admitted for a node: the attempt and its code domain.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AttemptBinding {
    attempt_id: String,
    domain: AttemptDomain,
}

/// Independent review evidence retained once admitted for a node attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RetainedReview {
    review_id: String,
    outcome: ReviewOutcome,
    code_sha: Option<String>,
}

/// The retained execution record of a graph: attached results by immutable
/// `result_ref`, the attempt identity (and bound code SHA) admitted at
/// `DISPATCH` for each `(project, graph, node)`, and the independent review
/// evidence admitted for each attempt. All of it is append-only through
/// apply-v1 and travels in the returned outcome, so `START`, `SUBMIT`,
/// `ATTACH_RESULT`, verdicts and acceptance in later calls or batches verify
/// against what was actually dispatched and reviewed. A ledger built with
/// [`GraphResultLedgerV1::new`] holds no history: executing, judging or
/// accepting a node through it refuses as missing evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GraphResultLedgerV1 {
    results: BTreeMap<String, GraphNodeResult>,
    /// Successful own-result verdicts, bound to the admitted attempt and result.
    own_verdicts: BTreeMap<(AttemptKey, String), (GraphNodeState, String)>,
    /// Operational creation facts; graph seeds alone do not prove freshness.
    created: BTreeMap<AttemptKey, GraphNode>,
    /// Complete rounds admitted while both new nodes were operationally fresh.
    repair_rounds: BTreeMap<AttemptKey, (String, String, String)>,
    /// The node's current admitted attempt (set by `DISPATCH`).
    current: BTreeMap<AttemptKey, AttemptBinding>,
    /// Every attempt id ever admitted, with its owner. Never reused.
    admitted: BTreeMap<String, AttemptKey>,
    /// Admitted review evidence by reviewed `(node, attempt)`.
    reviews: BTreeMap<(AttemptKey, String), RetainedReview>,
    /// Every review identity ever consumed. A review judges one attempt once.
    review_ids: BTreeMap<String, (AttemptKey, String)>,
}

/// Why trusted core could not admit a code SHA for an attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttemptAdmissionError {
    /// The node has no admitted attempt of that id.
    NoSuchAttempt,
    /// Not exactly 40 lowercase hex digits.
    InvalidCodeSha,
    /// A different SHA is already bound to the attempt.
    Conflict,
    /// The attempt is already classified the other way (code versus
    /// non-code); a classification is never downgraded or rebound.
    DomainConflict,
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
    fn binding(&self, project_id: &str, graph_id: &str, node_id: &str) -> Option<&AttemptBinding> {
        self.current
            .get(&(project_id.into(), graph_id.into(), node_id.into()))
    }
    /// The attempt id currently admitted for the node, if it was dispatched.
    pub fn attempt(&self, project_id: &str, graph_id: &str, node_id: &str) -> Option<&str> {
        self.binding(project_id, graph_id, node_id)
            .map(|b| b.attempt_id.as_str())
    }
    /// The code SHA bound to the node's current attempt, if known.
    pub fn attempt_code_sha(
        &self,
        project_id: &str,
        graph_id: &str,
        node_id: &str,
    ) -> Option<&str> {
        self.binding(project_id, graph_id, node_id)?.domain.sha()
    }
    /// Whether trusted core admitted the node's current attempt as non-code.
    pub fn attempt_is_non_code(&self, project_id: &str, graph_id: &str, node_id: &str) -> bool {
        self.binding(project_id, graph_id, node_id)
            .is_some_and(|b| b.domain == AttemptDomain::NonCode)
    }
    /// The review identity and outcome admitted for the node's current attempt.
    pub fn review(
        &self,
        project_id: &str,
        graph_id: &str,
        node_id: &str,
    ) -> Option<(&str, ReviewOutcome)> {
        let key = (project_id.into(), graph_id.into(), node_id.into());
        let attempt = self.current.get(&key)?.attempt_id.clone();
        let review = self.reviews.get(&(key, attempt))?;
        Some((review.review_id.as_str(), review.outcome))
    }

    /// Trusted core admits the code SHA an attempt produced. Required before
    /// a result may claim a SHA for a node whose graph data names none: a
    /// result's own `code_sha` never creates this binding. The attempt must be
    /// the node's current admitted attempt; a different SHA already bound
    /// (from the node's known SHA or an earlier admission) refuses, as does an
    /// attempt already admitted as non-code. Returns a new ledger; `self` is
    /// unchanged.
    pub fn admit_attempt_code_sha(
        &self,
        project_id: &str,
        graph_id: &str,
        node_id: &str,
        attempt_id: &str,
        code_sha: &str,
    ) -> Result<Self, AttemptAdmissionError> {
        let key: AttemptKey = (project_id.into(), graph_id.into(), node_id.into());
        let binding = self
            .current
            .get(&key)
            .filter(|b| b.attempt_id == attempt_id)
            .ok_or(AttemptAdmissionError::NoSuchAttempt)?;
        CommitSha::parse(code_sha).map_err(|_| AttemptAdmissionError::InvalidCodeSha)?;
        match &binding.domain {
            AttemptDomain::NonCode => return Err(AttemptAdmissionError::DomainConflict),
            AttemptDomain::Code(bound) if bound != code_sha => {
                return Err(AttemptAdmissionError::Conflict);
            }
            _ => {}
        }
        let mut next = self.clone();
        next.current.get_mut(&key).expect("checked above").domain =
            AttemptDomain::Code(code_sha.to_owned());
        Ok(next)
    }

    /// Trusted core admits that an attempt produced no code. This is the only
    /// way a SHA-less attempt becomes promotable: an omitted SHA, a node
    /// label, a worker result or a review record cannot. Keyed to the exact
    /// project/graph/node/current attempt; an attempt with a known or admitted
    /// code SHA refuses (never downgraded). Returns a new ledger.
    pub fn admit_attempt_non_code(
        &self,
        project_id: &str,
        graph_id: &str,
        node_id: &str,
        attempt_id: &str,
    ) -> Result<Self, AttemptAdmissionError> {
        let key: AttemptKey = (project_id.into(), graph_id.into(), node_id.into());
        let binding = self
            .current
            .get(&key)
            .filter(|b| b.attempt_id == attempt_id)
            .ok_or(AttemptAdmissionError::NoSuchAttempt)?;
        if matches!(binding.domain, AttemptDomain::Code(_)) {
            return Err(AttemptAdmissionError::DomainConflict);
        }
        let mut next = self.clone();
        next.current.get_mut(&key).expect("checked above").domain = AttemptDomain::NonCode;
        Ok(next)
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
    /// Independent review evidence was unavailable or contradicted; the
    /// typed reason says which.
    Review(ReviewEvidenceRefusal),
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

/// Trusted evidence sources apply-v1 may consult. Supplied by trusted core,
/// never derived from a mutation or a result.
#[derive(Clone, Copy, Default)]
pub struct ApplyEvidenceV1<'a> {
    /// Verifies `REQUIRES_INTERFACE` dependencies.
    pub interface: Option<InterfaceEvidenceVerifier<'a>>,
    /// Admits independent review evidence for aggregate verdicts. With none,
    /// every verdict that needs it refuses as unavailable.
    pub review: Option<ReviewEvidenceVerifier<'a>>,
}

/// Applies `mutation` to `envelope`/`results` atomically under `scope`.
/// `interface` is the optional trusted interface-evidence verifier used when
/// a step depends on a `REQUIRES_INTERFACE` edge. No review evidence is
/// available through this entry point: use
/// [`apply_graph_mutation_with_evidence_v1`] for evidence-backed verdicts.
pub fn apply_graph_mutation_v1(
    envelope: &ExecutionGraphEnvelope,
    results: &GraphResultLedgerV1,
    mutation: &GraphMutation,
    scope: &GraphMutationScopeV1,
    interface: Option<InterfaceEvidenceVerifier<'_>>,
) -> Result<GraphMutationOutcomeV1, GraphMutationApplyError> {
    apply_graph_mutation_with_evidence_v1(
        envelope,
        results,
        mutation,
        scope,
        ApplyEvidenceV1 {
            interface,
            review: None,
        },
    )
}

/// [`apply_graph_mutation_v1`] with every trusted evidence source.
pub fn apply_graph_mutation_with_evidence_v1(
    envelope: &ExecutionGraphEnvelope,
    results: &GraphResultLedgerV1,
    mutation: &GraphMutation,
    scope: &GraphMutationScopeV1,
    evidence: ApplyEvidenceV1<'_>,
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
        touched_repairs: BTreeSet::new(),
        actor: &scope.actor,
        parent: envelope.graph_version(),
        successor,
        interface: evidence.interface,
        review: evidence.review,
    };
    for (index, operation) in mutation.operations().iter().enumerate() {
        batch
            .apply(operation)
            .map_err(|error| E::Operation { index, error })?;
    }
    for repair in batch
        .graph
        .nodes()
        .filter(|n| n.kind() == &GraphNodeKind::REPAIR)
        .cloned()
        .collect::<Vec<_>>()
    {
        let sources: Vec<_> = batch
            .graph
            .control_edges()
            .filter(|e| {
                e.to_node() == repair.node_id() && e.control_kind() == Some(ControlKind::OnReject)
            })
            .map(|e| e.from_node().to_owned())
            .collect();
        if !sources.is_empty()
            && (batch.added.contains(repair.node_id())
                || batch.touched_repairs.contains(repair.node_id())
                || batch
                    .ledger
                    .repair_rounds
                    .contains_key(&batch.key_of(&repair)))
        {
            if sources.len() != 1 {
                return Err(E::Operation {
                    index: mutation.operations().len() - 1,
                    error: GraphOperationError::Repair("repair needs exactly one ON_REJECT source"),
                });
            }
            batch
                .require_repair_round(&sources[0], &repair)
                .map_err(|error| E::Operation {
                    index: mutation.operations().len() - 1,
                    error,
                })?;
        }
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

const UNCLASSIFIED: &str = "attempt code or non-code classification";

type OpResult<T = ()> = Result<T, GraphOperationError>;
type Map = BTreeMap<String, V>;

struct Batch<'a> {
    graph: ExecutionGraph,
    ledger: GraphResultLedgerV1,
    project_id: &'a str,
    added: BTreeSet<String>,
    touched_repairs: BTreeSet<String>,
    actor: &'a ScopedActorV1,
    parent: &'a GraphVersionV1,
    successor: GraphVersionV1,
    interface: Option<InterfaceEvidenceVerifier<'a>>,
    review: Option<ReviewEvidenceVerifier<'a>>,
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
        self.graph.add_node(node.clone())?;
        self.ledger.created.insert(self.key_of(&node), node);
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
        for endpoint in [edge.from_node(), edge.to_node()] {
            if self
                .graph
                .node(endpoint)
                .is_some_and(|n| n.kind() == &GraphNodeKind::REPAIR)
            {
                self.touched_repairs.insert(endpoint.to_owned());
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
        self.require_repair_node(&node)?;
        self.verify_cause(&node, to, kind, cause)?;
        self.bind_attempt(&node, kind, cause)?;
        self.graph.replace_node(node.with_state(to));
        Ok(())
    }

    fn key_of(&self, node: &GraphNode) -> AttemptKey {
        (
            self.project_id.to_owned(),
            self.graph.graph_id().to_owned(),
            node.node_id().to_owned(),
        )
    }

    fn binding(&self, node: &GraphNode) -> Option<&AttemptBinding> {
        self.ledger
            .binding(self.project_id, self.graph.graph_id(), node.node_id())
    }

    /// The node's current attempt, which trusted core must have classified as
    /// code (known or admitted SHA) or non-code before its result, verdict or
    /// acceptance can be used. Missing or unclassified is unavailable.
    fn classified_binding(&self, node: &GraphNode) -> OpResult<AttemptBinding> {
        use GraphOperationError as E;
        let binding = self
            .binding(node)
            .ok_or(E::Attempt("node has no admitted attempt"))?;
        if binding.domain == AttemptDomain::Unclassified {
            return Err(E::EvidenceUnavailable(UNCLASSIFIED));
        }
        Ok(binding.clone())
    }

    /// The review evidence admitted for the node's current attempt.
    fn retained_review(&self, node: &GraphNode) -> Option<&RetainedReview> {
        let attempt = self.binding(node)?.attempt_id.clone();
        self.ledger.reviews.get(&(self.key_of(node), attempt))
    }

    /// `DISPATCH` admits a never-used attempt id for this exact
    /// project/graph/node and binds the node's known code SHA to it; `START`
    /// and `SUBMIT` must name the attempt currently admitted for it and the
    /// node's known SHA must still be the one bound. A missing, different or
    /// conflicting binding refuses.
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
        let key = self.key_of(node);
        if kind == C::Dispatch {
            if self.ledger.admitted.contains_key(attempt) {
                return Err(E::Attempt("attempt id already admitted"));
            }
            self.ledger.admitted.insert(attempt.to_owned(), key.clone());
            self.ledger.current.insert(
                key,
                AttemptBinding {
                    attempt_id: attempt.to_owned(),
                    domain: node.code_sha().map_or(AttemptDomain::Unclassified, |sha| {
                        AttemptDomain::Code(sha.to_owned())
                    }),
                },
            );
            return Ok(());
        }
        let binding = self
            .ledger
            .current
            .get(&key)
            .ok_or(E::Attempt("node has no admitted attempt"))?;
        if binding.attempt_id != attempt {
            return Err(E::Attempt("attempt id not admitted"));
        }
        check_code_binding(node, binding)
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
        &mut self,
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
            C::ReviewVerdict if self.verdict_names_review(node) => {
                &["kind", "result_node_id", "review_id"]
            }
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
            C::ReviewVerdict => self.verify_verdict(node, to, cause),
            C::RepairEntry => {
                let repair = self.repair_child(node, id(cause, "repair_node_id")?)?;
                let repair = repair.clone();
                let sources: Vec<_> = self
                    .graph
                    .control_edges()
                    .filter(|edge| {
                        edge.to_node() == repair.node_id()
                            && edge.control_kind() == Some(ControlKind::OnReject)
                    })
                    .map(|e| e.from_node().to_owned())
                    .collect();
                match sources.as_slice() {
                    [source] => self.require_repair_round(source, &repair),
                    _ => Err(E::Repair("repair needs exactly one ON_REJECT source")),
                }
            }
            C::RepairSubmitted => {
                let repair = self
                    .repair_child(node, id(cause, "repair_node_id")?)?
                    .clone();
                self.require_repair_node(&repair)?;
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
            C::Acceptance => self.require_review_for_acceptance(node),
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

    /// Whether the verdict cause must also name the independent review: the
    /// first judgement of a work node's current attempt. Once judged, a
    /// node's next verdict cites its repair, whose own verdict already
    /// consumed the review; other kinds judge by their result.
    fn verdict_names_review(&self, node: &GraphNode) -> bool {
        requires_independent_review(node) && self.retained_review(node).is_none()
    }

    /// A work node's verdict rests on independent review evidence admitted
    /// by trusted core for its exact current attempt, SHA and round; its own
    /// result and a `review_id` it names prove nothing. A node already
    /// judged (rejected) is judged again through its newest repair, whose own
    /// admitted review (and matching state) is the evidence. Other kinds (a
    /// REVIEW node, a deterministic check, ...) keep their own result as the
    /// evidence.
    fn verify_verdict(&mut self, node: &GraphNode, to: GraphNodeState, cause: &Map) -> OpResult {
        use GraphOperationError as E;
        let cited_id = id(cause, "result_node_id")?;
        // No kind is exempt: an omitted SHA never says the attempt is non-code.
        self.classified_binding(node)?;
        if !requires_independent_review(node) {
            let newest = self.newest_repair_child(node)?.cloned();
            self.verify_own_result_verdict(node, to, cited_id, newest.as_ref())?;
            let binding = self.classified_binding(node)?;
            self.ledger.own_verdicts.insert(
                (self.key_of(node), binding.attempt_id),
                (to, cited_id.to_owned()),
            );
            return Ok(());
        }
        if self.retained_review(node).is_none() {
            if cited_id != node.node_id() {
                return Err(E::EvidenceRefused("cited result is not the current round"));
            }
            return self.admit_review(node, to, id(cause, "review_id")?);
        }
        let repaired = || E::EvidenceRefused("repaired node needs its repair's verdict");
        if cited_id == node.node_id() {
            return Err(repaired());
        }
        let child = self
            .newest_repair_child(node)?
            .cloned()
            .ok_or_else(repaired)?;
        if child.node_id() != cited_id || child.state() != to {
            return Err(E::EvidenceRefused("cited result is not the current round"));
        }
        let retained = self
            .retained_review(&child)
            .ok_or(E::Review(ReviewEvidenceRefusal::MissingRecord))?;
        if retained.outcome != review_outcome(to) {
            return Err(E::Review(ReviewEvidenceRefusal::OutcomeMismatch));
        }
        Ok(())
    }

    /// Resolves, verifies and retains the independent review of `node`'s
    /// current attempt (see [`Self::verify_verdict`]).
    fn admit_review(&mut self, node: &GraphNode, to: GraphNodeState, review_id: &str) -> OpResult {
        use GraphOperationError as E;
        let binding = self.classified_binding(node)?;
        let verifier = self
            .review
            .ok_or(E::Review(ReviewEvidenceRefusal::NoSource))?;
        let result = node
            .result_ref()
            .and_then(|r| self.ledger.get(r))
            .ok_or(E::EvidenceUnavailable("attached implementation result"))?;
        if to == GraphNodeState::Passed && result.outcome() != GraphNodeResultOutcome::Pass {
            return Err(E::EvidenceRefused("implementation result outcome"));
        }
        let key = self.key_of(node);
        if self
            .ledger
            .reviews
            .contains_key(&(key.clone(), binding.attempt_id.clone()))
        {
            return Err(E::EvidenceRefused("attempt already has admitted review"));
        }
        if self.ledger.review_ids.contains_key(review_id) {
            return Err(E::EvidenceRefused("review identity already used"));
        }
        self.review_not_circular(node, review_id, to)?;
        let work = ReviewedWork {
            project_id: self.project_id,
            graph_id: self.graph.graph_id(),
            review_id,
            node_id: node.node_id(),
            attempt_id: &binding.attempt_id,
            code_sha: binding.domain.sha(),
            round: node.attempt_number().map(GraphNodeAttemptNumber::as_str),
            expected: review_outcome(to),
        };
        let admitted = verifier.verify(&work).map_err(E::Review)?;
        self.ledger.reviews.insert(
            (key.clone(), binding.attempt_id.clone()),
            RetainedReview {
                review_id: admitted.review_id().to_owned(),
                outcome: admitted.outcome(),
                code_sha: admitted.code_sha().map(str::to_owned),
            },
        );
        self.ledger
            .review_ids
            .insert(admitted.review_id().to_owned(), (key, binding.attempt_id));
        Ok(())
    }

    /// A review that waits for the very acceptance it would gate is circular
    /// and never evidence. A graph-resident review is used only once it
    /// holds the matching verdict and its dependencies were satisfiable; any
    /// other identity is an external review from the trusted source.
    fn review_not_circular(
        &self,
        subject: &GraphNode,
        review_id: &str,
        to: GraphNodeState,
    ) -> OpResult {
        use GraphOperationError as E;
        let Some(review) = self.graph.node(review_id) else {
            return Ok(());
        };
        if !is_review(review) {
            return Err(E::EvidenceRefused(
                "review identity names a non-review node",
            ));
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        let mut pending = vec![subject.node_id()];
        while let Some(from) = pending.pop() {
            for edge in self.graph.precedence_edges().filter(|edge| {
                edge.from_node() == from
                    && edge.precedence_kind() != Some(PrecedenceKind::RequiresInterface)
            }) {
                if edge.to_node() == review_id {
                    return Err(E::EvidenceRefused(
                        "review requires the acceptance it would gate",
                    ));
                }
                if seen.insert(edge.to_node()) {
                    pending.push(edge.to_node());
                }
            }
        }
        let judged = match to {
            GraphNodeState::Passed => matches!(
                review.state(),
                GraphNodeState::Passed | GraphNodeState::Accepted | GraphNodeState::Integrated
            ),
            _ => review.state() == GraphNodeState::Rejected,
        };
        if !judged {
            return Err(E::EvidenceRefused("graph review has no matching verdict"));
        }
        Ok(())
    }

    /// Acceptance needs the admitted `PASS` review of the judged attempt
    /// (the newest repair's after a repair), still bound to the same SHA.
    fn require_review_for_acceptance(&self, node: &GraphNode) -> OpResult {
        use GraphOperationError as E;
        if !requires_independent_review(node) {
            let binding = self.classified_binding(node)?;
            check_code_binding(node, &binding)?;
            let (verdict, cited) = self
                .ledger
                .own_verdicts
                .get(&(self.key_of(node), binding.attempt_id))
                .ok_or(E::EvidenceUnavailable("admitted own-result verdict"))?;
            if *verdict != GraphNodeState::Passed {
                return Err(E::EvidenceRefused("own-result verdict is not PASS"));
            }
            let newest = self.newest_repair_child(node)?;
            return self.verify_own_result_verdict(node, GraphNodeState::Passed, cited, newest);
        }
        // A node accepted on its own review is judged by it; one rejected
        // once is accepted on its newest repair's review.
        let own_pass = self
            .retained_review(node)
            .is_some_and(|review| review.outcome == ReviewOutcome::Pass);
        let judged = match self.newest_repair_child(node)? {
            Some(child) if !own_pass => child,
            _ => node,
        };
        let retained = self
            .retained_review(judged)
            .ok_or(E::Review(ReviewEvidenceRefusal::MissingRecord))?;
        if retained.outcome != ReviewOutcome::Pass {
            return Err(E::Review(ReviewEvidenceRefusal::OutcomeMismatch));
        }
        let bound = self.classified_binding(judged)?;
        if retained.code_sha.as_deref() != bound.domain.sha() {
            return Err(E::Review(ReviewEvidenceRefusal::CodeMismatch));
        }
        Ok(())
    }

    /// Verdict evidence for kinds that are their own review: an attached
    /// result, the subject's own (no repair children yet) or that of its
    /// newest REPAIR child, which must already carry the same verdict. A
    /// rejected round's result is never reused for a later verdict.
    fn verify_own_result_verdict(
        &self,
        node: &GraphNode,
        to: GraphNodeState,
        cited_id: &str,
        newest: Option<&GraphNode>,
    ) -> OpResult {
        use GraphOperationError as E;
        let expected = if to == GraphNodeState::Passed {
            GraphNodeResultOutcome::Pass
        } else {
            GraphNodeResultOutcome::Rejected
        };
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
        let binding = self.classified_binding(cited)?;
        check_code_binding(cited, &binding)?;
        if result.node_id() != cited.node_id()
            || result.graph_id() != self.graph.graph_id()
            || result.attempt_id() != Some(binding.attempt_id.as_str())
            || result.code_sha() != binding.domain.sha()
        {
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
            [.., last] => {
                if !self.ledger.repair_rounds.contains_key(&self.key_of(last)) {
                    return Err(GraphOperationError::Repair("missing admitted repair round"));
                }
                Ok(Some(last))
            }
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
        self.require_repair_node(&node)?;
        if node.result_ref().is_some() || self.ledger.get(result_ref).is_some() {
            return Err(E::ResultSlotOccupied);
        }
        if result.graph_id() != self.graph.graph_id() {
            return Err(E::ResultAssociation("graph_id"));
        }
        if result.node_id() != node.node_id() {
            return Err(E::ResultAssociation("node_id"));
        }
        // Evidence must come from the attempt dispatched for this exact
        // project/graph/node; a result claiming its own identity proves none.
        let binding = self
            .binding(&node)
            .cloned()
            .ok_or(E::Attempt("node has no admitted attempt"))?;
        check_code_binding(&node, &binding)?;
        if result.attempt_id() != Some(binding.attempt_id.as_str()) {
            return Err(E::ResultAssociation("attempt_id"));
        }
        // The result must match the domain trusted core classified: a code
        // attempt's result carries exactly its SHA, a non-code one none. The
        // claim (or its absence) never classifies the attempt.
        match (&binding.domain, result.code_sha()) {
            (AttemptDomain::Unclassified, _) => return Err(E::EvidenceUnavailable(UNCLASSIFIED)),
            (AttemptDomain::Code(bound), Some(claimed)) if bound == claimed => {}
            (AttemptDomain::NonCode, None) => {}
            _ => return Err(E::ResultAssociation("code_sha")),
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

    fn require_repair_node(&mut self, node: &GraphNode) -> OpResult {
        if node.kind() != &GraphNodeKind::REPAIR {
            return Ok(());
        }
        let sources: Vec<_> = self
            .graph
            .control_edges()
            .filter(|e| {
                e.to_node() == node.node_id() && e.control_kind() == Some(ControlKind::OnReject)
            })
            .map(|e| e.from_node().to_owned())
            .collect();
        match sources.as_slice() {
            [source] => self.require_repair_round(source, node),
            _ => Err(GraphOperationError::Repair(
                "repair needs exactly one ON_REJECT source",
            )),
        }
    }

    fn require_repair_round(&mut self, source_id: &str, repair: &GraphNode) -> OpResult {
        use GraphOperationError as E;
        repair_provenance(&self.graph, source_id, repair)?;
        let source = self.graph.node(source_id).expect("provenance checked");
        let subject = reviewed_subject(&self.graph, source_id)?;
        let review = self
            .graph
            .precedence_edges()
            .filter(|e| e.from_node() == repair.node_id())
            .filter_map(|e| self.graph.node(e.to_node()))
            .find(|n| is_review(n))
            .expect("provenance checked");
        if reviewed_subject(&self.graph, review.node_id())?.node_id() != repair.node_id() {
            return Err(E::Repair("new review must judge exactly the repair"));
        }
        if repair.workstream_id() != subject.workstream_id()
            || review.workstream_id() != repair.workstream_id()
        {
            return Err(E::Repair("repair round workstream binding"));
        }
        let round = |n: &GraphNode| n.attempt_number().map(|a| a.as_str().to_owned());
        let later = |new: Option<String>, old: Option<String>| match (new, old) {
            (Some(new), Some(old)) => (new.len(), new) > (old.len(), old),
            _ => true,
        };
        if round(repair)
            .zip(round(review))
            .is_some_and(|(a, b)| a != b)
            || !later(round(repair), round(subject))
            || !later(round(review), round(source))
        {
            return Err(E::Repair("repair round ordinals"));
        }
        let sources: Vec<_> = self
            .graph
            .control_edges()
            .filter(|e| {
                e.control_kind() == Some(ControlKind::OnReject)
                    && (e.to_node() == repair.node_id() || e.from_node() == source_id)
            })
            .collect();
        if sources.len() != 1 {
            return Err(E::Repair("repair needs exactly one ON_REJECT source"));
        }
        for node in [repair, review] {
            let created = self
                .ledger
                .created
                .get(&self.key_of(node))
                .ok_or(E::Repair("missing operational creation evidence"))?;
            if created.state() != GraphNodeState::Planned
                || created.created_in_version().is_none()
                || created.created_in_version() != node.created_in_version()
                || created.kind() != node.kind()
                || created.parent_node_id() != node.parent_node_id()
                || created.workstream_id() != node.workstream_id()
                || created.attempt_number() != node.attempt_number()
            {
                return Err(E::Repair("repair round creation binding"));
            }
        }
        if repair.created_in_version() != review.created_in_version() {
            return Err(E::Repair("repair round creation binding"));
        }
        let key = self.key_of(repair);
        let facts = (
            source_id.to_owned(),
            subject.node_id().to_owned(),
            review.node_id().to_owned(),
        );
        match self.ledger.repair_rounds.get(&key) {
            Some(old) if old != &facts => return Err(E::Repair("repair round provenance changed")),
            Some(_) => return Ok(()),
            None => {}
        }
        if [repair, review].iter().any(|n| {
            n.state() != GraphNodeState::Planned
                || n.result_ref().is_some()
                || self.binding(n).is_some()
        }) || self
            .ledger
            .repair_rounds
            .values()
            .any(|(_, _, fresh)| fresh == review.node_id())
        {
            return Err(E::Repair("repair needs operationally fresh nodes"));
        }
        self.ledger.repair_rounds.insert(key, facts);
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
        let repair = repair.clone();
        self.require_repair_round(source_id, &repair)?;
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

/// Work nodes need independent review; a REVIEW node and the other
/// self-judging kinds are judged by their own attached result. Unknown kinds
/// fail closed (they need review).
fn requires_independent_review(node: &GraphNode) -> bool {
    ![
        GraphNodeKind::REVIEW,
        GraphNodeKind::DETERMINISTIC_CHECK,
        GraphNodeKind::GOAL,
        GraphNodeKind::WORKSTREAM,
        GraphNodeKind::GOAL_EVALUATION,
        GraphNodeKind::ROUTING,
        GraphNodeKind::INTEGRATION,
        GraphNodeKind::HUMAN_GATE,
    ]
    .contains(node.kind())
}

fn review_outcome(to: GraphNodeState) -> ReviewOutcome {
    if to == GraphNodeState::Passed {
        ReviewOutcome::Pass
    } else {
        ReviewOutcome::Rejected
    }
}

/// The node's known code SHA must be the one bound to its admitted attempt.
fn check_code_binding(node: &GraphNode, binding: &AttemptBinding) -> OpResult {
    match node.code_sha() {
        Some(known) if binding.domain.sha() != Some(known) => Err(GraphOperationError::Attempt(
            "code sha differs from the admitted attempt",
        )),
        _ => Ok(()),
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
