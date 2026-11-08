//! Task and Repair capsule builders (OR-W2-3, N4).
//!
//! Builders derive every graph-owned field from the graph and trusted
//! context instead of trusting the requester: task id, workstream, attempt
//! number, dependencies, required capabilities, branch and the current
//! context epoch (read from State). They refuse rather than build when:
//!
//! - the node is unknown, of the wrong kind, or not ready (unmet or
//!   unavailable dependency evidence, per `TaskDag`);
//! - the request was prepared against a context epoch other than State's
//!   latest for the project, or State has no epoch for it;
//! - a write path is a root wildcard, is not proven to fall inside the node's
//!   own adopted workstream ownership, overlaps another workstream's
//!   ownership or a forbidden path, or the matcher cannot decide;
//! - any text field looks credential-shaped (a heuristic, see
//!   [`credential_scan`](super::credential_scan); it never proves a string
//!   secret-free).
//!
//! Every other workstream's owned paths are added to `forbidden_write_paths`
//! as explicit traps. A pattern is built only when it is proven to lie
//! inside the workstream's own ownership
//! ([`containment`](super::containment), over the Workspace path grammar)
//! and [`WriteScopePort`] proves it disjoint from every other adopted
//! ownership and forbidden path. Overlap with the owner is not enough
//! (`src/*/safe/**` overlaps `src/core/**` yet reaches unowned paths); a
//! scope whose containment cannot be shown is refused.
//!
//! Repairs (N4) keep the original task's objective, criteria, workstream,
//! write scope (a subset of the original patterns) and quality floor (never
//! lower). They are built only for a planned `REPAIR` node whose
//! `parent_node_id` chain leads back to the original task through repairs,
//! that a `REJECTED` `REVIEW` hands off to through `ON_REJECT` after judging
//! the repair's parent, within the attempt bound. `current_sha` is the
//! judged node's bound code SHA, cross-checked with the attempt the ledger
//! admitted for it.

use receipts_state::SqliteStateRepository;

use super::containment::{Containment, contained_in};
use super::credential_scan::looks_like_credential;
use super::*;
use crate::dag::{InterfaceEvidenceVerifier, TaskDag};
use crate::graph::CapabilityName;
use crate::scheduler::{WorkstreamOwnershipV1, WriteScopeOverlapV1, WriteScopePort};
use crate::{
    ControlKind, ExecutionGraphEnvelope, GraphNode, GraphNodeKind, GraphNodeState,
    GraphResultLedgerV1,
};

#[derive(Debug, Clone, PartialEq)]
pub enum CapsuleBuildError {
    UnknownNode,
    /// The node kind is not built into this capsule type.
    WrongNodeKind,
    NotReady,
    NodeWithoutWorkstream,
    /// The node's workstream has no adopted ownership.
    OwnershipMissing,
    /// State holds no context epoch for the project.
    EpochUnavailable,
    /// The request was prepared against another context epoch.
    StaleEpoch,
    /// The current epoch exceeds this capsule's integer storage.
    EpochOutOfRange,
    State(String),
    PermissiveWildcard {
        pattern: String,
    },
    /// The pattern is not proven to lie inside the node's own ownership.
    OutsideWorkstream {
        pattern: String,
    },
    /// The pattern overlaps another workstream's ownership or a forbidden path.
    WriteScopeCollision {
        pattern: String,
        witness: String,
    },
    WriteScopeUndetermined {
        pattern: String,
    },
    CredentialShapedValue {
        field: &'static str,
    },
    /// Use the repair builder for REPAIR task types.
    RepairNeedsRepairBuilder,
    // --- repair-only -------------------------------------------------------
    /// The repair's parent chain does not lead to the original task.
    ParentMismatch,
    /// No REJECTED REVIEW of the repair's parent hands off to this repair.
    NoRejectedReview,
    AttemptMissing,
    AttemptBoundExceeded,
    /// The judged work has no bound code SHA, or the ledger disagrees.
    CurrentShaUnavailable,
    ObjectiveChanged,
    CriteriaChanged,
    /// A repair path is not one of the original task's patterns.
    RepairScopeWidened {
        pattern: String,
    },
    /// A finding listed as blocking is not marked blocking.
    NonBlockingFindingListedAsBlocking {
        finding_id: String,
    },
    Capsule(CapsuleError),
}

impl From<CapsuleError> for CapsuleBuildError {
    fn from(error: CapsuleError) -> Self {
        Self::Capsule(error)
    }
}

impl std::fmt::Display for CapsuleBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "capsule not built: {self:?}")
    }
}

impl std::error::Error for CapsuleBuildError {}

/// Trusted context for building capsules on one graph version.
pub struct CapsuleContextV1<'a> {
    pub envelope: &'a ExecutionGraphEnvelope,
    pub interface: Option<InterfaceEvidenceVerifier<'a>>,
    /// Read-only State: the current context epoch comes from here.
    pub state: &'a SqliteStateRepository,
    /// Adopted ownership of every workstream in the goal.
    pub ownership: &'a [WorkstreamOwnershipV1],
    /// The worktree Workspace assigned to this attempt.
    pub worktree: &'a str,
}

/// Requester-supplied task content. Graph-owned fields are not accepted.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskCapsuleRequestV1 {
    pub node_id: String,
    pub task_type: TaskType,
    pub objective: String,
    pub acceptance_criteria: Vec<Criterion>,
    pub non_goals: Option<Vec<String>>,
    /// Workstream branch baseline and the exact commit the worktree starts at.
    pub baseline_sha: String,
    pub start_sha: String,
    pub allowed_write_paths: Vec<String>,
    pub forbidden_write_paths: Vec<String>,
    pub relevant_context_refs: Option<Vec<Ref>>,
    pub architecture_refs: Option<Vec<Ref>>,
    pub contract_refs: Option<Vec<Ref>>,
    pub quality_floor: QualityFloor,
    pub preferred_capabilities: Option<Vec<CapabilityName>>,
    pub verification_plan: Vec<VerificationPlanEntryV1>,
    pub review_policy: Option<ReviewPolicy>,
    pub assurance_profile: AssuranceProfileSelector,
    pub cost_policy: Option<CostPolicy>,
    pub time_budget_seconds: Option<i64>,
    pub turn_budget: Option<i64>,
    pub remote_publish_policy: Option<RemotePublishPolicy>,
    pub stop_conditions: Vec<String>,
    pub handoff_schema: Option<Ref>,
    /// The context epoch the request was prepared against.
    pub context_epoch: String,
}

/// Builds the TaskCapsule for a ready `TASK`/`IMPLEMENTATION` node.
pub fn build_task_capsule_v1(
    context: &CapsuleContextV1<'_>,
    port: &impl WriteScopePort,
    request: TaskCapsuleRequestV1,
) -> Result<TaskCapsule, CapsuleBuildError> {
    use CapsuleBuildError as E;
    if request.task_type == TaskType::Repair {
        return Err(E::RepairNeedsRepairBuilder);
    }
    let node = ready_node(context, &request.node_id)?;
    if !matches!(node.kind().as_str(), "TASK" | "IMPLEMENTATION") {
        return Err(E::WrongNodeKind);
    }
    let workstream = node.workstream_id().ok_or(E::NodeWithoutWorkstream)?;
    let epoch = current_epoch(context, &request.context_epoch)?;
    let epoch: i64 = epoch.parse().map_err(|_| E::EpochOutOfRange)?;
    let forbidden = check_write_scope(
        context,
        port,
        workstream,
        &request.allowed_write_paths,
        &request.forbidden_write_paths,
    )?;
    let mut text: Vec<(&'static str, &str)> = vec![
        ("objective", &request.objective),
        ("worktree", context.worktree),
    ];
    criteria_text(&request.acceptance_criteria, &mut text);
    for (field, values) in [
        (
            "non_goals",
            request.non_goals.as_deref().unwrap_or_default(),
        ),
        ("stop_conditions", &request.stop_conditions[..]),
        ("allowed_write_paths", &request.allowed_write_paths[..]),
        ("forbidden_write_paths", &request.forbidden_write_paths[..]),
    ] {
        text.extend(values.iter().map(|v| (field, v.as_str())));
    }
    for entry in &request.verification_plan {
        text.extend(
            entry
                .command()
                .iter()
                .map(|v| ("verification_plan", v.as_str())),
        );
    }
    for (field, refs) in [
        (
            "relevant_context_refs",
            request.relevant_context_refs.as_deref(),
        ),
        ("architecture_refs", request.architecture_refs.as_deref()),
        ("contract_refs", request.contract_refs.as_deref()),
        ("handoff_schema", request.handoff_schema.as_slice().into()),
    ] {
        refs_text(field, refs.unwrap_or_default(), &mut text);
    }
    scan(&text)?;

    let task_id = node.node_id().to_owned();
    let attempt = match node.attempt_number() {
        None => 1,
        Some(n) => n.as_str().parse().map_err(|_| E::AttemptBoundExceeded)?,
    };
    let mut dependencies: Vec<String> = context
        .envelope
        .graph()
        .precedence_edges()
        .filter(|e| e.to_node() == task_id)
        .map(|e| e.from_node().to_owned())
        .collect();
    dependencies.sort();
    dependencies.dedup();
    Ok(TaskCapsule::try_new(
        task_id.clone(),
        workstream.to_owned(),
        None,
        attempt,
        request.task_type,
        request.objective,
        request.acceptance_criteria,
        request.non_goals,
        request.baseline_sha,
        request.start_sha,
        request.allowed_write_paths,
        forbidden,
        request.relevant_context_refs,
        request.architecture_refs,
        request.contract_refs,
        Some(dependencies),
        request.quality_floor,
        node.required_capabilities().unwrap_or_default().to_vec(),
        request.preferred_capabilities,
        request.verification_plan,
        request.review_policy,
        request.assurance_profile,
        request.cost_policy,
        request.time_budget_seconds,
        request.turn_budget,
        format!("runtime-a3/{task_id}"),
        context.worktree.to_owned(),
        request.remote_publish_policy,
        request.stop_conditions,
        request.handoff_schema,
        epoch,
    )?)
}

/// Requester-supplied repair content; the original task is authoritative.
#[derive(Debug, Clone, PartialEq)]
pub struct RepairCapsuleRequestV1 {
    pub repair_node_id: String,
    /// What the requester believes the original objective and criteria are;
    /// a repair never changes them (N4).
    pub original_objective: String,
    pub original_acceptance_criteria: Vec<Criterion>,
    pub a4_findings: Option<Vec<RepairCapsuleFindingV1>>,
    pub blocking_findings: Vec<RepairCapsuleFindingV1>,
    pub failed_checks: Option<Vec<RepairCapsuleFailedCheckV1>>,
    pub repair_scope: String,
    pub prior_attempt_summary: Option<String>,
    pub relevant_context_refs: Option<Vec<Ref>>,
    pub allowed_write_paths: Vec<String>,
    pub forbidden_write_paths: Option<Vec<String>>,
    pub required_quality_floor: QualityFloor,
    pub context_epoch: String,
}

/// Builds the RepairCapsule for a planned REPAIR node of `original`'s task.
pub fn build_repair_capsule_v1(
    context: &CapsuleContextV1<'_>,
    port: &impl WriteScopePort,
    results: &GraphResultLedgerV1,
    original: &TaskCapsule,
    max_attempt: u64,
    request: RepairCapsuleRequestV1,
) -> Result<RepairCapsule, CapsuleBuildError> {
    use CapsuleBuildError as E;
    let graph = context.envelope.graph();
    let repair = ready_node(context, &request.repair_node_id)?;
    if repair.kind() != &GraphNodeKind::REPAIR {
        return Err(E::WrongNodeKind);
    }
    if repair.workstream_id() != Some(original.workstream_id()) {
        return Err(E::ParentMismatch);
    }
    // The parent chain must reach the original task through repairs only.
    let judged = repair.parent_node_id().ok_or(E::ParentMismatch)?;
    let mut cursor = judged;
    for _ in 0..=graph.node_count() {
        if cursor == original.task_id() {
            break;
        }
        let node = graph.node(cursor).ok_or(E::ParentMismatch)?;
        if node.kind() != &GraphNodeKind::REPAIR {
            return Err(E::ParentMismatch);
        }
        cursor = node.parent_node_id().ok_or(E::ParentMismatch)?;
    }
    if cursor != original.task_id() {
        return Err(E::ParentMismatch);
    }
    // A REJECTED REVIEW that judged the parent hands off to this repair.
    let handed_off = graph.control_edges().any(|edge| {
        edge.to_node() == repair.node_id()
            && edge.control_kind() == Some(ControlKind::OnReject)
            && graph.node(edge.from_node()).is_some_and(|review| {
                review.kind() == &GraphNodeKind::REVIEW
                    && review.state() == GraphNodeState::Rejected
                    && graph
                        .precedence_edges()
                        .any(|p| p.from_node() == judged && p.to_node() == review.node_id())
            })
    });
    if !handed_off {
        return Err(E::NoRejectedReview);
    }
    let attempt = repair.attempt_number().ok_or(E::AttemptMissing)?;
    let within = attempt
        .as_str()
        .parse::<u64>()
        .is_ok_and(|n| n >= 2 && n <= max_attempt);
    if !within {
        return Err(E::AttemptBoundExceeded);
    }
    let attempt = RepairAttemptNumberV1::try_new(attempt.as_str())?;
    let project = context.envelope.project_id();
    let current_sha = graph
        .node(judged)
        .and_then(GraphNode::code_sha)
        .ok_or(E::CurrentShaUnavailable)?;
    if results
        .attempt_code_sha(project, graph.graph_id(), judged)
        .is_some_and(|sha| sha != current_sha)
    {
        return Err(E::CurrentShaUnavailable);
    }
    if request.original_objective != original.objective() {
        return Err(E::ObjectiveChanged);
    }
    if request.original_acceptance_criteria != original.acceptance_criteria() {
        return Err(E::CriteriaChanged);
    }
    if let Some(pattern) = request
        .allowed_write_paths
        .iter()
        .find(|p| !original.allowed_write_paths().contains(p))
    {
        return Err(E::RepairScopeWidened {
            pattern: pattern.clone(),
        });
    }
    if let Some(finding) = request.blocking_findings.iter().find(|f| !f.blocking()) {
        return Err(E::NonBlockingFindingListedAsBlocking {
            finding_id: finding.finding_id().to_owned(),
        });
    }
    let epoch = current_epoch(context, &request.context_epoch)?;
    let mut forbidden: Vec<String> = original.forbidden_write_paths().to_vec();
    for path in request.forbidden_write_paths.iter().flatten() {
        if !forbidden.contains(path) {
            forbidden.push(path.clone());
        }
    }
    let forbidden = check_write_scope(
        context,
        port,
        original.workstream_id(),
        &request.allowed_write_paths,
        &forbidden,
    )?;

    let mut text: Vec<(&'static str, &str)> = vec![
        ("repair_scope", &request.repair_scope),
        ("worktree", context.worktree),
    ];
    if let Some(summary) = &request.prior_attempt_summary {
        text.push(("prior_attempt_summary", summary));
    }
    for finding in request
        .a4_findings
        .iter()
        .flatten()
        .chain(&request.blocking_findings)
    {
        text.push(("findings", finding.description()));
        text.extend(finding.path().map(|p| ("findings", p)));
        refs_text(
            "findings",
            finding
                .evidence_ref()
                .map(std::slice::from_ref)
                .unwrap_or_default(),
            &mut text,
        );
    }
    for check in request.failed_checks.iter().flatten() {
        text.extend(
            check
                .command()
                .iter()
                .map(|c| ("failed_checks", c.as_str())),
        );
        refs_text(
            "failed_checks",
            check
                .output_ref()
                .map(std::slice::from_ref)
                .unwrap_or_default(),
            &mut text,
        );
    }
    refs_text(
        "relevant_context_refs",
        request.relevant_context_refs.as_deref().unwrap_or_default(),
        &mut text,
    );
    text.extend(
        request
            .allowed_write_paths
            .iter()
            .map(|p| ("allowed_write_paths", p.as_str())),
    );
    scan(&text)?;

    let task_id = repair.node_id().to_owned();
    Ok(RepairCapsule::try_new(
        task_id.clone(),
        original.task_id().to_owned(),
        results
            .attempt(project, graph.graph_id(), judged)
            .map(str::to_owned),
        attempt,
        current_sha.to_owned(),
        original.objective().to_owned(),
        original.acceptance_criteria().to_vec(),
        request.a4_findings,
        request.blocking_findings,
        request.failed_checks,
        request.repair_scope,
        request.prior_attempt_summary,
        request.relevant_context_refs,
        request.allowed_write_paths,
        Some(forbidden),
        request.required_quality_floor,
        format!("runtime-a3/{task_id}"),
        context.worktree.to_owned(),
        RepairContextEpochV1::try_new(epoch)?,
        original.quality_floor(),
    )?)
}

fn ready_node<'a>(
    context: &CapsuleContextV1<'a>,
    node_id: &str,
) -> Result<&'a GraphNode, CapsuleBuildError> {
    let node = context
        .envelope
        .graph()
        .node(node_id)
        .ok_or(CapsuleBuildError::UnknownNode)?;
    let ready = TaskDag::new(context.envelope, context.interface).ready_nodes();
    if !ready.ready.iter().any(|n| n == node_id) {
        return Err(CapsuleBuildError::NotReady);
    }
    Ok(node)
}

/// State's latest epoch for the project, which the request must match.
fn current_epoch(
    context: &CapsuleContextV1<'_>,
    requested: &str,
) -> Result<String, CapsuleBuildError> {
    let latest = context
        .state
        .find_latest_context_epoch(context.envelope.project_id())
        .map_err(|e| CapsuleBuildError::State(e.to_string()))?
        .ok_or(CapsuleBuildError::EpochUnavailable)?;
    if latest.epoch.as_str() != requested {
        return Err(CapsuleBuildError::StaleEpoch);
    }
    Ok(latest.epoch.as_str().to_owned())
}

/// Checks the write scope (containment in the workstream's ownership, then
/// disjointness from everything else) and returns `forbidden` plus every other
/// workstream's owned paths (deduplicated, input order then ownership order).
fn check_write_scope(
    context: &CapsuleContextV1<'_>,
    port: &impl WriteScopePort,
    workstream: &str,
    allowed: &[String],
    forbidden: &[String],
) -> Result<Vec<String>, CapsuleBuildError> {
    use CapsuleBuildError as E;
    let own = context
        .ownership
        .iter()
        .find(|o| o.workstream_id == workstream)
        .ok_or(E::OwnershipMissing)?;
    let mut traps: Vec<String> = forbidden.to_vec();
    for other in context
        .ownership
        .iter()
        .filter(|o| o.workstream_id != workstream)
    {
        for path in &other.owned_paths {
            if !traps.contains(path) {
                traps.push(path.clone());
            }
        }
    }
    for pattern in allowed {
        let first = pattern.split('/').next().unwrap_or_default();
        if first.is_empty() || first.contains('*') {
            return Err(E::PermissiveWildcard {
                pattern: pattern.clone(),
            });
        }
        let single = std::slice::from_ref(pattern);
        match port.overlap(single, &own.owned_paths) {
            WriteScopeOverlapV1::Overlaps { .. } => {}
            WriteScopeOverlapV1::Disjoint => {
                return Err(E::OutsideWorkstream {
                    pattern: pattern.clone(),
                });
            }
            WriteScopeOverlapV1::Undetermined => {
                return Err(E::WriteScopeUndetermined {
                    pattern: pattern.clone(),
                });
            }
        }
        // Overlap alone cannot show the pattern stays inside the owner:
        // `src/*/safe/**` overlaps `src/core/**` yet also reaches unowned paths.
        match contained_in(pattern, &own.owned_paths) {
            Containment::Contained => {}
            Containment::NotProven => {
                return Err(E::OutsideWorkstream {
                    pattern: pattern.clone(),
                });
            }
            Containment::Undecided => {
                return Err(E::WriteScopeUndetermined {
                    pattern: pattern.clone(),
                });
            }
        }
        if traps.is_empty() {
            continue;
        }
        match port.overlap(single, &traps) {
            WriteScopeOverlapV1::Disjoint => {}
            WriteScopeOverlapV1::Overlaps { witness } => {
                return Err(E::WriteScopeCollision {
                    pattern: pattern.clone(),
                    witness,
                });
            }
            WriteScopeOverlapV1::Undetermined => {
                return Err(E::WriteScopeUndetermined {
                    pattern: pattern.clone(),
                });
            }
        }
    }
    Ok(traps)
}

fn criteria_text<'a>(criteria: &'a [Criterion], out: &mut Vec<(&'static str, &'a str)>) {
    for c in criteria {
        out.push(("acceptance_criteria", c.description()));
        out.extend(c.rationale().map(|r| ("acceptance_criteria", r)));
        out.extend(
            c.check_command()
                .unwrap_or_default()
                .iter()
                .map(|a| ("acceptance_criteria", a.as_str())),
        );
    }
}

fn refs_text<'a>(field: &'static str, refs: &'a [Ref], out: &mut Vec<(&'static str, &'a str)>) {
    for r in refs {
        out.push((field, r.target()));
        out.extend(r.section().map(|s| (field, s)));
    }
}

fn scan(fields: &[(&'static str, &str)]) -> Result<(), CapsuleBuildError> {
    match fields
        .iter()
        .find(|(_, value)| looks_like_credential(value))
    {
        Some((field, _)) => Err(CapsuleBuildError::CredentialShapedValue { field }),
        None => Ok(()),
    }
}
