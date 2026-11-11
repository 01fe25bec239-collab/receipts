//! Goal-owned scheduling and graph continuation over trusted consumer ports.
use crate::policy::GraphExecutionPolicy;
use receipts_state::{SetupRefusalV1, StateError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

use super::{GoalEvaluationState, engine::*, evaluate::*, persistence::*};
use crate::capsules::{Criterion, CriterionKind};
use crate::compiler::CompiledGraphV1;
use crate::dag::{InterfaceEvidenceVerifier, TaskDag};
use crate::policy::FreePolicyV1;
use crate::scheduler::*;
use crate::{
    ApplyEvidenceV1, ExecutionGraphEnvelope, GraphMutation, GraphMutationOutcomeV1,
    GraphMutationScopeV1, GraphNodeState, GraphResultLedgerV1,
    apply_graph_mutation_with_evidence_v1,
};

/// Immutable, goal-owned continuation. Its fields are checked against State and
/// the current graph before any effect. Physical attempt/review identities are
/// rehydrated from admitted State by the snapshot supplier, not synthesized here.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoalContinuationV1 {
    project: String,
    goal: String,
    graph: String,
    spec_sha256: String,
    provider: String,
    runtime: String,
    limits: GoalBudgetLimitsV1,
    criteria: Vec<AdoptedCriterionV1>,
    state: String,
    gaps: Vec<String>,
    evaluated_graph_version: Option<String>,
    integrated_sha: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AdoptedCriterionV1 {
    id: String,
    description: String,
    semantic: bool,
    command: Option<Vec<String>>,
    rationale: Option<String>,
}
impl AdoptedCriterionV1 {
    fn from_criterion(c: &Criterion) -> Self {
        Self {
            id: c.id().into(),
            description: c.description().into(),
            semantic: c.kind() == CriterionKind::Semantic,
            command: c.check_command().map(<[String]>::to_vec),
            rationale: c.rationale().map(str::to_owned),
        }
    }
    fn criterion(&self) -> Result<Criterion, StateError> {
        Criterion::try_new(
            self.id.clone(),
            self.description.clone(),
            if self.semantic {
                CriterionKind::Semantic
            } else {
                CriterionKind::Deterministic
            },
            self.command.clone(),
            self.rationale.clone(),
        )
        .map_err(|_| refused())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalLifecycleEventV1 {
    GoalCreated,
    GoalDecomposed,
    TaskCreated { node: String },
    TaskReady { node: String },
    TaskCompleted { node: String },
    TaskFailed { node: String },
    TaskCancelled { node: String },
    TaskDispatched { node: String, attempt: String },
    TaskStarted { node: String, attempt: String },
    GoalEvaluated,
    GoalCompleted,
    GoalBlocked,
    HumanRequired,
}

pub struct GoalRouteV1 {
    pub dispatch_id: String,
    pub attempt_id: receipts_state::AttemptId,
    pub attempt_number: String,
    pub provider_id: String,
    pub runtime_id: String,
    pub eligible_host_native: bool,
    pub workspace_id: String,
    pub start_sha: String,
    pub started_at: String,
}

pub struct GoalSnapshotV1 {
    pub goal: GoalRecordV1,
    pub state: GoalEvaluationState,
    pub gaps: Vec<String>,
    pub evaluated_graph_version: Option<String>,
    pub integrated_sha: Option<String>,
}

pub struct GoalGraphChangeV1 {
    pub mutation: GraphMutation,
    pub scope: GraphMutationScopeV1,
}

/// Fresh observations from the trusted State/Workspace composition. Cost and
/// elapsed observations must include all reserved/running attempts; unknown
/// cost under a cap is represented by None and refuses new work.
#[derive(Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GoalBudgetLimitsV1 {
    pub wall_seconds: Option<u64>,
    pub cost_units: Option<u64>,
    pub repairs: Option<u64>,
}
#[derive(Clone, Default)]
pub struct GoalBudgetUsageV1 {
    pub elapsed_seconds: u64,
    /// None is unknown, never zero under a cap.
    pub cost_units: Option<u64>,
    pub repairs: u64,
}
pub struct GoalRuntimeSnapshotV1 {
    pub graph: ExecutionGraphEnvelope,
    pub results: GraphResultLedgerV1,
    pub current_epoch: String,
    pub spec: Vec<u8>,
    pub criteria: Vec<Criterion>,
    pub completion: GoalCompletionFactsV1,
    pub concurrency: ConcurrencyLimitsV1,
    pub in_flight: Vec<InFlightWriterV1>,
    pub remaining_budget: DispatchBudgetV1,
    pub budget_limits: GoalBudgetLimitsV1,
    pub budget_usage: GoalBudgetUsageV1,
}

/// Effects only: Core derives readiness, admission, mutation validity and goal
/// completion. Consumers adapt routing/execution/review without a crate edge.
/// All writes are admitted and retry-safe. Artifact writes precede publication
/// of their references. `recover` refuses unsafe orphan/continuity evidence.
pub trait GoalConsumerPortV1: CriterionCheckPortV1 + WriteScopePort {
    fn emit(
        &mut self,
        goal: &GoalRecordV1,
        graph_version: &str,
        event: GoalLifecycleEventV1,
    ) -> Result<(), StateError>;
    fn task_criteria(
        &mut self,
        goal: &GoalRecordV1,
        node: &str,
    ) -> Result<Vec<Criterion>, StateError>;
    fn persist_compilation(&mut self, compiled: &CompiledGraphV1) -> Result<(), StateError>;
    fn recover(&mut self, goal: &GoalRecordV1) -> Result<(), StateError>;
    fn snapshot(&mut self, goal: &GoalRecordV1) -> Result<GoalRuntimeSnapshotV1, StateError>;
    fn read_artifact(&self, reference: &str) -> Result<Vec<u8>, StateError>;
    fn put_artifact(&mut self, content: &[u8]) -> Result<String, StateError>;
    /// Completion/review observations propose mutations; proposals grant no
    /// acceptance. Core applies the existing evidence-backed mutation rules.
    fn completion_changes(
        &mut self,
        goal: &GoalRecordV1,
    ) -> Result<Vec<GoalGraphChangeV1>, StateError>;
    fn publish_change(
        &mut self,
        parent: &ExecutionGraphEnvelope,
        change: &GoalGraphChangeV1,
        outcome: &GraphMutationOutcomeV1,
    ) -> Result<(), StateError>;
    /// A stable request identity derived from the goal, node and attempt makes
    /// repeated ticks idempotent. FREE supplies deterministic gate evidence;
    /// this method must not silently invoke an independent provider reviewer.
    fn request_review(
        &mut self,
        goal: &GoalRecordV1,
        node: &str,
        gate: &GoalAssessmentV1,
    ) -> Result<(), StateError>;
    fn candidate(
        &mut self,
        goal: &GoalRecordV1,
        node: &str,
    ) -> Result<DispatchCandidateV1, StateError>;
    /// Build through the existing task/repair capsule builders and route only
    /// this admitted node. Missing capsule/evidence must refuse before launch.
    fn route(&mut self, goal: &GoalRecordV1, node: &str) -> Result<GoalRouteV1, StateError>;
    fn launch(
        &mut self,
        goal: &GoalRecordV1,
        dispatch: &PreparedGoalDispatchV1,
    ) -> Result<(LaunchResolutionV1, String), StateError>;
    /// Persist the evaluation bound to current graph/version/epoch, plus the
    /// corresponding GOAL_EVALUATED/COMPLETED/BLOCKED/HUMAN_REQUIRED events.
    fn record_evaluation(
        &mut self,
        goal: &GoalRecordV1,
        graph: &ExecutionGraphEnvelope,
        assessment: &GoalAssessmentV1,
    ) -> Result<(), StateError>;
}

/// Concrete Core loop adapter. The facade uses this adapter with the admitted
/// reservation engine. Evidence verifiers are independent trusted read sources.
pub struct GoalDriverV1<'a, P> {
    pub consumers: &'a mut P,
    pub policy: &'a FreePolicyV1,
    pub evidence: ApplyEvidenceV1<'a>,
    pub retry_failed: &'a BTreeSet<String>,
    pub(super) evaluation_binding: Option<(String, Option<String>, String)>,
}
impl<P: GoalConsumerPortV1> GoalDriverV1<'_, P> {
    fn continuation(&self, goal: &GoalRecordV1) -> Result<GoalContinuationV1, StateError> {
        let reference = goal.continuation_ref.as_ref().ok_or_else(refused)?;
        let bytes = self.consumers.read_artifact(reference)?;
        let c: GoalContinuationV1 = serde_json::from_slice(&bytes).map_err(|_| refused())?;
        if c.project != goal.definition.project_id
            || c.goal != goal.definition.goal_id
            || c.graph != goal.definition.graph_id
            || c.spec_sha256 != goal.definition.spec_sha256
            || !self.policy.permits(&c.provider, &c.runtime, true)
        {
            return Err(refused());
        }
        Ok(c)
    }
    fn publish_dispatch(
        &mut self,
        _goal: &GoalRecordV1,
        dispatch: &PreparedGoalDispatchV1,
        snapshot: &GoalRuntimeSnapshotV1,
        operations: Vec<serde_json::Value>,
        phase: &str,
    ) -> Result<(), StateError> {
        let next =
            receipts_state::StateEpochValueV1::try_from(snapshot.graph.graph_version().as_str())?
                .successor()?
                .to_string();
        let parent_number: serde_json::Value =
            serde_json::from_str(snapshot.graph.graph_version().as_str()).map_err(|_| refused())?;
        let next_number: serde_json::Value = serde_json::from_str(&next).map_err(|_| refused())?;
        let json = serde_json::json!({"mutation_id":format!("{}-{phase}",dispatch.request.dispatch_id),"graph_id":snapshot.graph.graph_id(),"parent_version":parent_number,"resulting_version":next_number,"actor":{"role":"RUNTIME_A1"},"reason":phase,"created_at":dispatch.started_at,"operations":operations});
        let mutation = crate::wire::GraphMutationWireV1::from_json(&json.to_string())
            .and_then(|w| w.to_mutation())
            .map_err(|_| refused())?;
        let scope = GraphMutationScopeV1::new(
            snapshot.graph.project_id(),
            snapshot.graph.graph_id(),
            snapshot.graph.graph_version().clone(),
            crate::ScopedActorV1::RuntimeA1 { role_id: None },
        );
        let change = GoalGraphChangeV1 { mutation, scope };
        let outcome = apply_graph_mutation_with_evidence_v1(
            &snapshot.graph,
            &snapshot.results,
            &change.mutation,
            &change.scope,
            self.evidence,
        )
        .map_err(|_| refused())?;
        self.consumers
            .publish_change(&snapshot.graph, &change, &outcome)
    }
    pub(super) fn durable_snapshot(
        &self,
        goal: GoalRecordV1,
    ) -> Result<GoalSnapshotV1, StateError> {
        let c = self.continuation(&goal)?;
        let state = match c.state.as_str() {
            "COMPLETE" if c.gaps.is_empty() => GoalEvaluationState::Complete,
            "INCOMPLETE" => GoalEvaluationState::Incomplete,
            "BLOCKED" => GoalEvaluationState::Blocked,
            "HUMAN_REQUIRED" => GoalEvaluationState::HumanRequired,
            _ => return Err(refused()),
        };
        Ok(GoalSnapshotV1 {
            goal,
            state,
            gaps: c.gaps,
            evaluated_graph_version: c.evaluated_graph_version,
            integrated_sha: c.integrated_sha,
        })
    }
    fn emit_changes(
        &mut self,
        goal: &GoalRecordV1,
        parent: &ExecutionGraphEnvelope,
        outcome: &GraphMutationOutcomeV1,
    ) -> Result<(), StateError> {
        for node in outcome.envelope.graph().nodes() {
            let old = parent.graph().node(node.node_id());
            let event = if old.is_none() {
                Some(GoalLifecycleEventV1::TaskCreated {
                    node: node.node_id().into(),
                })
            } else if old.is_some_and(|n| n.state() == node.state()) {
                None
            } else {
                match node.state() {
                    GraphNodeState::AwaitingReview => Some(GoalLifecycleEventV1::TaskCompleted {
                        node: node.node_id().into(),
                    }),
                    GraphNodeState::Rejected => Some(GoalLifecycleEventV1::TaskFailed {
                        node: node.node_id().into(),
                    }),
                    GraphNodeState::Cancelled => Some(GoalLifecycleEventV1::TaskCancelled {
                        node: node.node_id().into(),
                    }),
                    _ => None,
                }
            };
            if let Some(event) = event {
                self.consumers
                    .emit(goal, outcome.envelope.graph_version().as_str(), event)?;
            }
        }
        Ok(())
    }
    fn validate_snapshot(
        &self,
        goal: &GoalRecordV1,
        s: &GoalRuntimeSnapshotV1,
    ) -> Result<(), StateError> {
        let digest = Sha256::digest(&s.spec)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        if s.graph.project_id() != goal.definition.project_id
            || s.graph.goal_id() != goal.definition.goal_id
            || s.graph.graph_id() != goal.definition.graph_id
            || digest != goal.definition.spec_sha256
            || s.graph
                .context_epoch()
                .is_some_and(|e| e.decimal_digits() != s.current_epoch)
        {
            return Err(refused());
        }
        Ok(())
    }
}
impl<P: GoalConsumerPortV1> GoalLoopPortV1 for GoalDriverV1<'_, P> {
    fn persist_compilation(&mut self, compiled: &CompiledGraphV1) -> Result<(), StateError> {
        self.consumers.persist_compilation(compiled)
    }
    fn reconcile(&mut self, goal: &GoalRecordV1) -> Result<GoalRoundV1, StateError> {
        self.consumers.recover(goal)?;
        let continuation = self.continuation(goal)?;
        let mut snapshot = self.consumers.snapshot(goal)?;
        self.validate_snapshot(goal, &snapshot)?;
        if snapshot.current_epoch != goal.context_epoch {
            return Ok(GoalRoundV1 {
                current_epoch: snapshot.current_epoch.clone(),
                dispatch: None,
                assessment: GoalAssessmentV1::incomplete(vec![
                    "epoch reconciled; next tick re-derives readiness".into(),
                ]),
            });
        }
        if snapshot.budget_limits != continuation.limits {
            return Err(refused());
        }
        let mut repair_count = snapshot.budget_usage.repairs.max(
            snapshot
                .graph
                .graph()
                .nodes()
                .filter(|n| n.kind() == &crate::GraphNodeKind::REPAIR)
                .count() as u64,
        );
        for change in self.consumers.completion_changes(goal)? {
            for op in change
                .mutation
                .operations()
                .iter()
                .filter(|op| op.op() == crate::GraphMutationOperationKind::ExpandRepair)
            {
                let node = op.node_id().ok_or_else(refused)?;
                let result_ref = snapshot
                    .graph
                    .graph()
                    .node(node)
                    .and_then(|n| n.result_ref())
                    .ok_or_else(refused)?;
                let result = snapshot.results.get(result_ref).ok_or_else(refused)?;
                if snapshot
                    .budget_limits
                    .repairs
                    .is_some_and(|limit| repair_count >= limit)
                {
                    return Ok(GoalRoundV1 {
                        current_epoch: snapshot.current_epoch.clone(),
                        dispatch: None,
                        assessment: blocked(
                            "repair budget exhausted; explicit resume disposition required",
                        ),
                    });
                }
                repair_count = repair_count.checked_add(1).ok_or_else(refused)?;
                if !self.retry_failed.contains(node)
                    || !self
                        .policy
                        .permits_retry(&snapshot.graph, node, result_ref, result)
                {
                    return Ok(GoalRoundV1 {
                        current_epoch: snapshot.current_epoch.clone(),
                        dispatch: None,
                        assessment: blocked(
                            "FREE retry requires an explicit request for a failed node",
                        ),
                    });
                }
            }
            let outcome = apply_graph_mutation_with_evidence_v1(
                &snapshot.graph,
                &snapshot.results,
                &change.mutation,
                &change.scope,
                self.evidence,
            )
            .map_err(|_| refused())?;
            self.consumers
                .publish_change(&snapshot.graph, &change, &outcome)?;
            self.emit_changes(goal, &snapshot.graph, &outcome)?;
            snapshot.graph = outcome.envelope;
            snapshot.results = outcome.results;
        }
        // Refresh integration, budget and epoch observations after publications.
        snapshot = self.consumers.snapshot(goal)?;
        self.validate_snapshot(goal, &snapshot)?;
        if snapshot.budget_limits != continuation.limits {
            return Err(refused());
        }
        if snapshot.current_epoch != goal.context_epoch {
            return Ok(GoalRoundV1 {
                current_epoch: snapshot.current_epoch.clone(),
                dispatch: None,
                assessment: GoalAssessmentV1::incomplete(vec![
                    "epoch changed during reconciliation; refresh readiness next tick".into(),
                ]),
            });
        }
        let epoch = snapshot.current_epoch.clone();
        let round = |assessment, dispatch| GoalRoundV1 {
            current_epoch: epoch.clone(),
            assessment,
            dispatch,
        };
        if snapshot
            .budget_limits
            .wall_seconds
            .is_some_and(|limit| snapshot.budget_usage.elapsed_seconds >= limit)
        {
            return Ok(round(
                blocked("wall-clock budget exhausted; explicit resume disposition required"),
                None,
            ));
        }
        for node in snapshot.graph.graph().nodes().filter(|n| {
            n.state() == GraphNodeState::AwaitingReview
                && ![
                    crate::GraphNodeKind::REVIEW,
                    crate::GraphNodeKind::DETERMINISTIC_CHECK,
                    crate::GraphNodeKind::GOAL,
                    crate::GraphNodeKind::WORKSTREAM,
                    crate::GraphNodeKind::GOAL_EVALUATION,
                    crate::GraphNodeKind::ROUTING,
                    crate::GraphNodeKind::INTEGRATION,
                    crate::GraphNodeKind::HUMAN_GATE,
                ]
                .contains(n.kind())
        }) {
            let criteria = self.consumers.task_criteria(goal, node.node_id())?;
            let facts = GoalCompletionFactsV1 {
                required_work_integrated: true,
                no_blocking_tasks_or_findings: true,
                security_review_complete: true,
                dependencies_resolved: true,
                epoch_reconciled: true,
                integrated_sha: snapshot
                    .results
                    .attempt_code_sha(
                        snapshot.graph.project_id(),
                        snapshot.graph.graph_id(),
                        node.node_id(),
                    )
                    .or_else(|| node.code_sha())
                    .map(str::to_owned),
            };
            let gate = evaluate_required_criteria_v1(&criteria, &facts, self.consumers);
            self.consumers.request_review(goal, node.node_id(), &gate)?;
        }
        if let Some(limit) = snapshot.budget_limits.cost_units {
            let Some(used) = snapshot.budget_usage.cost_units else {
                return Ok(round(
                    blocked("cost usage unknown under a capped budget"),
                    None,
                ));
            };
            if used >= limit {
                return Ok(round(
                    blocked("cost budget exhausted; explicit resume disposition required"),
                    None,
                ));
            }
            let remaining = limit - used;
            snapshot.remaining_budget.remaining_cost = Some(
                snapshot
                    .remaining_budget
                    .remaining_cost
                    .map_or(remaining, |prior| prior.min(remaining)),
            );
        }
        let interface: Option<InterfaceEvidenceVerifier<'_>> = self.evidence.interface;
        let ready = TaskDag::new(&snapshot.graph, interface).ready_nodes();
        let mut candidates = Vec::new();
        for node in &ready.ready {
            self.consumers.emit(
                goal,
                snapshot.graph.graph_version().as_str(),
                GoalLifecycleEventV1::TaskReady { node: node.clone() },
            )?;
            candidates.push(self.consumers.candidate(goal, node)?);
        }
        let schedule = schedule_dispatch_v1(
            &SchedulingContextV1 {
                envelope: &snapshot.graph,
                interface,
                current_context_epoch: &snapshot.current_epoch,
                limits: &snapshot.concurrency,
                budget: snapshot.remaining_budget,
                in_flight: &snapshot.in_flight,
            },
            &candidates,
            self.consumers,
        );
        if let Some(node) = schedule.admitted().first() {
            let route = self.consumers.route(goal, node)?;
            let Some(SchedulingDecisionV1::Admitted { derivation }) = schedule.decision(node)
            else {
                return Err(refused());
            };
            let admitted = derivation.decision().core();
            if admitted.selected_provider() != Some(route.provider_id.as_str())
                || admitted.selected_runtime() != Some(route.runtime_id.as_str())
            {
                return Err(refused());
            }
            if !self.policy.permits(
                &route.provider_id,
                &route.runtime_id,
                route.eligible_host_native,
            ) {
                return Err(refused());
            }
            let start = AttemptStart::new(
                route.attempt_id,
                &goal.definition.project_id,
                &goal.definition.graph_id,
                node,
                &route.attempt_number,
                receipts_state::AttemptRole::RuntimeA3,
                &route.runtime_id,
                &route.workspace_id,
                &route.start_sha,
                &route.started_at,
            )?;
            let candidate = candidates
                .iter()
                .find(|c| c.node_id == *node)
                .ok_or_else(refused)?;
            let dispatch = PreparedGoalDispatchV1 {
                node_id: (*node).into(),
                admission_decision_id: candidate.admission.decision_id.clone(),
                started_at: route.started_at.clone(),
                provider_id: route.provider_id,
                runtime_id: route.runtime_id,
                eligible_host_native: route.eligible_host_native,
                start,
                request: GoalDispatchV1 {
                    project_id: goal.definition.project_id.clone(),
                    goal_id: goal.definition.goal_id.clone(),
                    dispatch_id: route.dispatch_id,
                    expected_revision: goal.revision.clone(),
                    context_epoch: snapshot.current_epoch.clone(),
                    graph_version: snapshot.graph.graph_version().as_str().into(),
                },
            };
            return Ok(round(
                GoalAssessmentV1::incomplete(vec!["admitted work pending verification".into()]),
                Some(dispatch),
            ));
        }
        if schedule.decisions.iter().any(|d| {
            matches!(
                d.decision,
                SchedulingDecisionV1::Deferred {
                    reason: DeferralV1::WorkerBudgetExhausted
                        | DeferralV1::CostBudgetExhausted
                        | DeferralV1::CostEstimateUnknown,
                    ..
                }
            )
        }) {
            return Ok(round(
                blocked("worker or cost budget unavailable; explicit resume disposition required"),
                None,
            ));
        }
        let historical_rejection = |node: &crate::GraphNode| {
            node.kind() == &crate::GraphNodeKind::REVIEW
                && node.state() == GraphNodeState::Rejected
                && snapshot.graph.graph().control_edges().any(|e| {
                    e.from_node() == node.node_id()
                        && e.control_kind() == Some(crate::ControlKind::OnReject)
                        && snapshot
                            .graph
                            .graph()
                            .node(e.to_node())
                            .is_some_and(|n| n.kind() == &crate::GraphNodeKind::REPAIR)
                })
        };
        let human: Vec<_> = snapshot
            .graph
            .graph()
            .nodes()
            .filter(|n| {
                matches!(
                    n.state(),
                    GraphNodeState::HumanRequired | GraphNodeState::Rejected
                ) && !historical_rejection(n)
            })
            .map(|n| {
                format!(
                    "{}: human input or explicit failed-node retry required",
                    n.node_id()
                )
            })
            .collect();
        if !human.is_empty() {
            return Ok(round(
                GoalAssessmentV1 {
                    state: GoalEvaluationState::HumanRequired,
                    gaps: human,
                },
                None,
            ));
        }
        let blocked_nodes: Vec<_> = snapshot
            .graph
            .graph()
            .nodes()
            .filter(|n| {
                matches!(
                    n.state(),
                    GraphNodeState::Blocked | GraphNodeState::LockedRequiresPro
                )
            })
            .map(|n| format!("{}: {}", n.node_id(), n.state().as_str()))
            .collect();
        if !blocked_nodes.is_empty() {
            return Ok(round(
                GoalAssessmentV1 {
                    state: GoalEvaluationState::Blocked,
                    gaps: blocked_nodes,
                },
                None,
            ));
        }
        let criteria = continuation
            .criteria
            .iter()
            .map(AdoptedCriterionV1::criterion)
            .collect::<Result<Vec<_>, _>>()?;
        snapshot.completion.no_blocking_tasks_or_findings &=
            snapshot.graph.graph().nodes().all(|n| {
                matches!(
                    n.state(),
                    GraphNodeState::Accepted | GraphNodeState::Integrated | GraphNodeState::Passed
                ) || historical_rejection(n)
            });
        snapshot.completion.required_work_integrated &=
            snapshot.graph.graph().nodes().next().is_some();
        let mut assessment =
            evaluate_required_criteria_v1(&criteria, &snapshot.completion, self.consumers);
        if !ready.awaiting_evidence.is_empty() {
            assessment
                .gaps
                .push("interface evidence unavailable".into());
        }
        if schedule.decisions.iter().any(|d| {
            matches!(
                d.decision,
                SchedulingDecisionV1::Denied { .. } | SchedulingDecisionV1::Refused(_)
            )
        }) {
            assessment =
                blocked("dispatch admission refused; refresh authoritative evidence before resume");
        }
        self.evaluation_binding = Some((
            snapshot.graph.graph_version().as_str().into(),
            snapshot.completion.integrated_sha.clone(),
            snapshot.current_epoch.clone(),
        ));
        Ok(round(assessment, None))
    }
    fn launch(
        &mut self,
        goal: &GoalRecordV1,
        dispatch: &PreparedGoalDispatchV1,
    ) -> Result<(LaunchResolutionV1, String), StateError> {
        let snapshot = self.consumers.snapshot(goal)?;
        self.validate_snapshot(goal, &snapshot)?;
        if snapshot.current_epoch != dispatch.request.context_epoch
            || snapshot.graph.graph_version().as_str() != dispatch.request.graph_version
        {
            return Err(refused());
        }
        let node = snapshot
            .graph
            .graph()
            .node(&dispatch.node_id)
            .ok_or_else(refused)?;
        let mut operations = Vec::new();
        let set = |from: &str, to: &str, cause: serde_json::Value| serde_json::json!({"op":"SET_NODE_STATE","node_id":dispatch.node_id,"detail":{"from":from,"to":to,"cause":cause}});
        if node.state() == GraphNodeState::Planned {
            operations.push(set(
                "PLANNED",
                "READY",
                serde_json::json!({"kind":"DEPENDENCIES_SATISFIED"}),
            ));
        }
        operations.push(set("READY", "ADMITTED", serde_json::json!({"kind":"ADMISSION","admission_decision_id":dispatch.admission_decision_id})));
        operations.push(set("ADMITTED", "DISPATCHED", serde_json::json!({"kind":"DISPATCH","attempt_id":dispatch.start.attempt_id().as_str()})));
        self.publish_dispatch(goal, dispatch, &snapshot, operations, "dispatch")?;
        let dispatched_version = snapshot.graph.graph_version().successor();
        self.consumers.emit(
            goal,
            dispatched_version.as_str(),
            GoalLifecycleEventV1::TaskDispatched {
                node: dispatch.node_id.clone(),
                attempt: dispatch.start.attempt_id().as_str().into(),
            },
        )?;
        let outcome = self.consumers.launch(goal, dispatch)?;
        if outcome.0 == LaunchResolutionV1::Launched {
            let snapshot = self.consumers.snapshot(goal)?;
            self.publish_dispatch(goal, dispatch, &snapshot, vec![set("DISPATCHED", "RUNNING", serde_json::json!({"kind":"START","attempt_id":dispatch.start.attempt_id().as_str()}))], "start")?;
            self.consumers.emit(
                goal,
                snapshot.graph.graph_version().successor().as_str(),
                GoalLifecycleEventV1::TaskStarted {
                    node: dispatch.node_id.clone(),
                    attempt: dispatch.start.attempt_id().as_str().into(),
                },
            )?;
        }
        Ok(outcome)
    }
    fn save_continuation(
        &mut self,
        goal: &GoalRecordV1,
        assessment: &GoalAssessmentV1,
    ) -> Result<String, StateError> {
        let snapshot = self.consumers.snapshot(goal)?;
        self.validate_snapshot(goal, &snapshot)?;
        let binding = (
            snapshot.graph.graph_version().as_str().into(),
            snapshot.completion.integrated_sha.clone(),
            snapshot.current_epoch.clone(),
        );
        if assessment.state() == GoalEvaluationState::Complete
            && self.evaluation_binding.as_ref() != Some(&binding)
        {
            return Err(refused());
        }
        self.consumers
            .record_evaluation(goal, &snapshot.graph, assessment)?;
        self.consumers.emit(
            goal,
            snapshot.graph.graph_version().as_str(),
            GoalLifecycleEventV1::GoalEvaluated,
        )?;
        match assessment.state() {
            GoalEvaluationState::Complete => self.consumers.emit(
                goal,
                snapshot.graph.graph_version().as_str(),
                GoalLifecycleEventV1::GoalCompleted,
            )?,
            GoalEvaluationState::Blocked => self.consumers.emit(
                goal,
                snapshot.graph.graph_version().as_str(),
                GoalLifecycleEventV1::GoalBlocked,
            )?,
            GoalEvaluationState::HumanRequired => self.consumers.emit(
                goal,
                snapshot.graph.graph_version().as_str(),
                GoalLifecycleEventV1::HumanRequired,
            )?,
            GoalEvaluationState::Incomplete => (),
        }
        let mut c = self.continuation(goal)?;
        c.state = assessment.state().as_str().into();
        c.gaps = assessment.gaps().to_vec();
        c.evaluated_graph_version = Some(snapshot.graph.graph_version().as_str().into());
        c.integrated_sha = snapshot.completion.integrated_sha;
        self.consumers
            .put_artifact(&serde_json::to_vec(&c).map_err(|_| refused())?)
    }
}

/// Creates the first immutable continuation before dispatch. A crash before the
/// CAS leaves only an unreferenced artifact; a retry adopts no launch authority.
pub fn initialize_goal_continuation_v1(
    state: &mut impl GoalPersistencePortV1,
    consumers: &mut impl GoalConsumerPortV1,
    policy: &FreePolicyV1,
    project: &str,
    goal_id: &str,
) -> Result<GoalRecordV1, StateError> {
    let goal = snapshot_goal_v1(state, project, goal_id)?;
    if goal.continuation_ref.is_some() {
        return Ok(goal);
    }
    if goal.dispatches_used != "0" {
        return Err(refused());
    }
    let snapshot = consumers.snapshot(&goal)?;
    let driver = GoalDriverV1 {
        consumers,
        policy,
        evidence: ApplyEvidenceV1::default(),
        retry_failed: &BTreeSet::new(),
        evaluation_binding: None,
    };
    driver.validate_snapshot(&goal, &snapshot)?;
    let c = GoalContinuationV1 {
        project: project.into(),
        goal: goal_id.into(),
        graph: goal.definition.graph_id.clone(),
        spec_sha256: goal.definition.spec_sha256.clone(),
        provider: policy.provider().into(),
        runtime: policy.runtime().into(),
        limits: snapshot.budget_limits,
        state: "INCOMPLETE".into(),
        gaps: vec!["goal initialized; first tick pending".into()],
        evaluated_graph_version: None,
        integrated_sha: None,
        criteria: snapshot
            .criteria
            .iter()
            .map(AdoptedCriterionV1::from_criterion)
            .collect(),
    };
    let reference = driver
        .consumers
        .put_artifact(&serde_json::to_vec(&c).map_err(|_| refused())?)?;
    state.advance_goal(&GoalProgressV1 {
        project_id: project.into(),
        goal_id: goal_id.into(),
        expected_revision: goal.revision,
        expected_epoch: goal.context_epoch.clone(),
        context_epoch: goal.context_epoch,
        continuation_ref: Some(reference),
    })?;
    snapshot_goal_v1(state, project, goal_id)
}
fn refused() -> StateError {
    StateError::SetupRefused {
        reason: SetupRefusalV1::ForeignEvidence,
    }
}
fn blocked(reason: &str) -> GoalAssessmentV1 {
    GoalAssessmentV1 {
        state: GoalEvaluationState::Blocked,
        gaps: vec![reason.into()],
    }
}
