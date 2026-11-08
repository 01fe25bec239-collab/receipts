//! Strict JSON codecs for the remaining six Orchestration schemas:
//! `TaskCapsule`, `RepairCapsule`, `DispatchAdmissionDecision`,
//! `FeatureAdmissionDecision`, `FeatureCapabilitySet` and `GoalEvaluation`.
//!
//! Same contract as the graph codecs: decoding checks the frozen schema
//! shape exactly and keeps the JSON verbatim (absence vs null, exact number
//! lexemes); conversion to the operational type is a separate fallible step
//! that reports schema-valid values outside the operational domain as
//! [`GraphWireError::Unsupported`]. Encoding from an operational value is
//! canonical (absent optionals stay absent).
//!
//! Operational-domain notes (each is an `Unsupported` refusal, never a
//! silent change):
//! - integers stored as `i64` (TaskCapsule counts, budgets, exit codes,
//!   context epoch) need a canonical lexeme within `i64`; lexical integer
//!   types (RepairCapsule, catalog version, GoalEvaluation epoch) need a
//!   canonical lexeme of any magnitude. `1.0`, `1e0` and `-0` are refused.
//! - `TaskCapsule.verification_plan` is a *plan*: each entry projects to
//!   `{source: WORKER_EXECUTION, command, exit_code: <expected>, code_sha:
//!   <start_sha>}`. Decoding accepts exactly that projection; an entry with
//!   another source or SHA, or with executed-check fields, is refused.
//! - `TaskCapsule.required_capabilities` needs at least one item on the
//!   wire, so encoding a capsule with none refuses.
//! - `DispatchAdmissionDecision.selected_provider/runtime`: absent and null
//!   both read as none; none encodes as null.
//! - `RepairCapsule` conversion needs the parent's quality floor (the
//!   operational constructor checks the floor never weakens).
//! - `GoalEvaluation`: only the deterministic projection is operational;
//!   `semantic_layer`, `gaps` and `convergence` are refused on conversion.

use receipts_state::entitlement::{ActivationStateKind, ProductEntitlementState};

use super::*;
use crate::capsules::{
    AssuranceProfileSelector, CostPolicy, CostPriority, Criterion, CriterionKind, DistinctProvider,
    QualityFloor, Ref, RefType, RemotePublishPolicy, RepairAttemptNumberV1, RepairCapsule,
    RepairCapsuleFailedCheckV1, RepairCapsuleFindingV1, RepairCheckResultV1, RepairCheckSourceV1,
    RepairContextEpochV1, RepairExitCodeV1, RepairFindingCategoryV1, RepairFindingConfidenceV1,
    RepairFindingLineV1, RepairFindingSeverityV1, RepairFindingSourceV1, ReviewPolicy, TaskCapsule,
    TaskType, VerificationPlanEntryV1,
};
use crate::goal::{
    DeterministicCondition, DeterministicConditionResult, DeterministicGoalEvaluationCore,
    DeterministicLayer, GoalEvaluationContextEpoch, GoalEvaluationState,
};
use crate::orchestration::{
    FeatureAdmissionDecision, FeatureAdmissionOutcome, FeatureAdmissionUpgradeInfo,
    FeatureCapability, FeatureCapabilitySet, FeatureCapabilityStatus,
};
use crate::scheduler::{
    DispatchAdmissionAxisResult, DispatchAdmissionAxisResults, DispatchAdmissionDecision,
    DispatchAdmissionDecisionCore, DispatchAdmissionDecisionNonTemporalCore,
    DispatchAdmissionDenialReason, DispatchAdmissionEntitlementAxisResult,
    DispatchAdmissionFailingAxis, DispatchAdmissionOutcome,
    DispatchAdmissionProviderAuthAxisResult, DispatchAdmissionProviderAvailabilityAxisResult,
    DispatchAdmissionProviderPolicyAxisResult, DispatchAdmissionProviderPolicyStatus,
    DispatchAdmissionQualityFloorAxisResult, DispatchAdmissionSafetyAxisResult,
};

wire_type!(
    /// Schema-shaped `TaskCapsule` JSON.
    TaskCapsuleWireV1,
    validate_task_capsule
);
wire_type!(
    /// Schema-shaped `RepairCapsule` JSON.
    RepairCapsuleWireV1,
    validate_repair_capsule
);
wire_type!(
    /// Schema-shaped `DispatchAdmissionDecision` JSON.
    DispatchAdmissionDecisionWireV1,
    validate_dispatch_decision
);
wire_type!(
    /// Schema-shaped `FeatureAdmissionDecision` JSON.
    FeatureAdmissionDecisionWireV1,
    validate_feature_decision
);
wire_type!(
    /// Schema-shaped `FeatureCapabilitySet` JSON.
    FeatureCapabilitySetWireV1,
    validate_capability_set
);
wire_type!(
    /// Schema-shaped `GoalEvaluation` JSON.
    GoalEvaluationWireV1,
    validate_goal_evaluation
);

// --- vocabularies ------------------------------------------------------------

const REF_TYPES: [&str; 4] = ["REPO_PATH", "STATE_QUERY", "ARTIFACT_ID", "URL"];
const CRITERION_KINDS: [&str; 2] = ["DETERMINISTIC", "SEMANTIC"];
const CHECK_SOURCES: [&str; 4] = [
    "WORKER_EXECUTION",
    "BROKER_EXECUTION",
    "REVIEW_EXECUTION",
    "GIT_PROVENANCE",
];
const FLOORS: [&str; 3] = ["FRONTIER", "BALANCED", "ECONOMY"];
const SEVERITIES: [&str; 5] = ["INFO", "LOW", "MEDIUM", "HIGH", "CRITICAL"];
const CONFIDENCES: [&str; 3] = ["HIGH", "MEDIUM", "LOW"];
const FINDING_SOURCES: [&str; 5] = [
    "LLM_REVIEW",
    "STATIC_ANALYSIS",
    "DEPENDENCY_SCAN",
    "TEST",
    "CONFIG_CHECK",
];
const AXIS_RESULTS: [&str; 3] = ["PASS", "FAIL", "NOT_APPLICABLE"];

fn names<T: Copy, const N: usize>(
    all: [T; N],
    name: impl Fn(T) -> &'static str,
) -> Vec<&'static str> {
    all.into_iter().map(name).collect()
}

/// The validated variant whose name is `value`.
fn pick<T: Copy, const N: usize>(all: [T; N], name: impl Fn(T) -> &'static str, value: &str) -> T {
    all.into_iter()
        .find(|v| name(*v) == value)
        .expect("validated vocabulary value")
}

fn opt_pick<T: Copy, const N: usize>(
    all: [T; N],
    name: impl Fn(T) -> &'static str,
    map: &Map,
    field: &str,
) -> Option<T> {
    opt_text(map, field).map(|v| pick(all, name, v))
}

// --- shape helpers -------------------------------------------------------------

fn boolean(map: &Map, field: &'static str) -> WireResult<Option<bool>> {
    match map.get(field) {
        None => Ok(None),
        Some(V::Boolean(b)) => Ok(Some(*b)),
        Some(_) => Err(GraphWireError::Shape(field)),
    }
}

fn any_integer(map: &Map, field: &'static str) -> WireResult<()> {
    match map.get(field) {
        None => Ok(()),
        Some(V::Number(n)) if integer_at_least_any(n) => Ok(()),
        Some(_) => Err(GraphWireError::Shape(field)),
    }
}

fn nullable_string(map: &Map, field: &'static str) -> WireResult<()> {
    match map.get(field) {
        None | Some(V::Null | V::String(_)) => Ok(()),
        Some(_) => Err(GraphWireError::Shape(field)),
    }
}

fn string_list(map: &Map, field: &'static str, min: usize) -> WireResult<()> {
    if let Some(items) = strings(map, field)?
        && items.len() < min
    {
        return Err(GraphWireError::Shape(field));
    }
    Ok(())
}

fn identifier_list(map: &Map, field: &'static str) -> WireResult<()> {
    if strings(map, field)?
        .unwrap_or_default()
        .iter()
        .any(|v| v.is_empty() || v.chars().count() > MAX_IDENTIFIER_LENGTH)
    {
        return Err(GraphWireError::Shape(field));
    }
    Ok(())
}

fn objects(
    map: &Map,
    field: &'static str,
    min: usize,
    each: impl Fn(&Map) -> WireResult<()>,
) -> WireResult<()> {
    if let Some(items) = array(map, field)? {
        if items.len() < min {
            return Err(GraphWireError::Shape(field));
        }
        for item in items {
            each(object(item, field)?)?;
        }
    }
    Ok(())
}

fn nested(map: &Map, field: &'static str, each: impl Fn(&Map) -> WireResult<()>) -> WireResult<()> {
    match map.get(field) {
        None => Ok(()),
        Some(value) => each(object(value, field)?),
    }
}

fn capability(map: &Map, field: &'static str) -> WireResult<()> {
    if string(map, field)?.is_some_and(|c| CapabilityName::new(c).is_err()) {
        return Err(GraphWireError::Shape(field));
    }
    Ok(())
}

fn validate_ref(map: &Map) -> WireResult<()> {
    keys(
        map,
        &["ref_type", "target", "digest", "section"],
        &["ref_type", "target"],
        "ref",
    )?;
    one_of(map, "ref_type", &REF_TYPES)?;
    nonempty(map, "target")?;
    string(map, "digest")?;
    string(map, "section")?;
    Ok(())
}

fn validate_criterion(map: &Map) -> WireResult<()> {
    keys(
        map,
        &["id", "description", "kind", "check_command", "rationale"],
        &["id", "description", "kind"],
        "criterion",
    )?;
    identifier(map, "id")?;
    nonempty(map, "description")?;
    one_of(map, "kind", &CRITERION_KINDS)?;
    strings(map, "check_command")?;
    string(map, "rationale")?;
    Ok(())
}

fn validate_check(map: &Map) -> WireResult<()> {
    keys(
        map,
        &[
            "source",
            "command",
            "exit_code",
            "code_sha",
            "timed_out",
            "started_at",
            "finished_at",
            "output_ref",
            "result",
        ],
        &["source", "command", "exit_code", "code_sha"],
        "check",
    )?;
    one_of(map, "source", &CHECK_SOURCES)?;
    string_list(map, "command", 1)?;
    any_integer(map, "exit_code")?;
    hex(map, "code_sha", 40)?;
    boolean(map, "timed_out")?;
    string(map, "started_at")?;
    string(map, "finished_at")?;
    nested(map, "output_ref", validate_ref)?;
    one_of(map, "result", &CHECK_RESULTS)?;
    Ok(())
}

fn validate_finding(map: &Map) -> WireResult<()> {
    keys(
        map,
        &[
            "finding_id",
            "severity",
            "category",
            "description",
            "blocking",
            "path",
            "line",
            "evidence_ref",
            "confidence",
            "source",
        ],
        &[
            "finding_id",
            "severity",
            "category",
            "description",
            "blocking",
        ],
        "finding",
    )?;
    identifier(map, "finding_id")?;
    one_of(map, "severity", &SEVERITIES)?;
    one_of(
        map,
        "category",
        &names(RepairFindingCategoryV1::ALL, |c| c.as_str()),
    )?;
    nonempty(map, "description")?;
    boolean(map, "blocking")?;
    string(map, "path")?;
    integer(map, "line", 1)?;
    nested(map, "evidence_ref", validate_ref)?;
    one_of(map, "confidence", &CONFIDENCES)?;
    one_of(map, "source", &FINDING_SOURCES)?;
    Ok(())
}

// --- schema shapes -------------------------------------------------------------

fn validate_task_capsule(map: &Map) -> WireResult<()> {
    keys(
        map,
        &[
            "task_id",
            "workstream_id",
            "parent_task_id",
            "attempt_number",
            "task_type",
            "objective",
            "acceptance_criteria",
            "non_goals",
            "baseline_sha",
            "start_sha",
            "allowed_write_paths",
            "forbidden_write_paths",
            "relevant_context_refs",
            "architecture_refs",
            "contract_refs",
            "dependencies",
            "quality_floor",
            "required_capabilities",
            "preferred_capabilities",
            "verification_plan",
            "review_policy",
            "assurance_profile",
            "cost_policy",
            "time_budget_seconds",
            "turn_budget",
            "branch",
            "worktree",
            "remote_publish_policy",
            "stop_conditions",
            "handoff_schema",
            "context_epoch",
        ],
        &[
            "task_id",
            "workstream_id",
            "attempt_number",
            "task_type",
            "objective",
            "acceptance_criteria",
            "baseline_sha",
            "start_sha",
            "allowed_write_paths",
            "forbidden_write_paths",
            "quality_floor",
            "required_capabilities",
            "verification_plan",
            "assurance_profile",
            "branch",
            "worktree",
            "stop_conditions",
            "context_epoch",
        ],
        "TaskCapsule",
    )?;
    for field in ["task_id", "workstream_id", "parent_task_id"] {
        identifier(map, field)?;
    }
    integer(map, "attempt_number", 1)?;
    one_of(map, "task_type", &names(TaskType::ALL, |t| t.as_str()))?;
    nonempty(map, "objective")?;
    objects(map, "acceptance_criteria", 1, validate_criterion)?;
    strings(map, "non_goals")?;
    hex(map, "baseline_sha", 40)?;
    hex(map, "start_sha", 40)?;
    string_list(map, "allowed_write_paths", 1)?;
    strings(map, "forbidden_write_paths")?;
    for field in [
        "relevant_context_refs",
        "architecture_refs",
        "contract_refs",
    ] {
        objects(map, field, 0, validate_ref)?;
    }
    identifier_list(map, "dependencies")?;
    one_of(map, "quality_floor", &FLOORS)?;
    string_list(map, "required_capabilities", 1)?;
    strings(map, "preferred_capabilities")?;
    objects(map, "verification_plan", 1, validate_check)?;
    nested(map, "review_policy", |policy| {
        keys(
            policy,
            &["required", "distinct_provider", "reviewer_floor"],
            &[],
            "review_policy",
        )?;
        boolean(policy, "required")?;
        one_of(
            policy,
            "distinct_provider",
            &["OFF", "PREFERRED", "REQUIRED"],
        )?;
        one_of(policy, "reviewer_floor", &FLOORS)
    })?;
    one_of(
        map,
        "assurance_profile",
        &names(AssuranceProfileSelector::ALL, |a| a.as_str()),
    )?;
    nested(map, "cost_policy", |policy| {
        keys(policy, &["max_cost", "priority"], &[], "cost_policy")?;
        if !matches!(policy.get("max_cost"), None | Some(V::Null | V::Number(_))) {
            return Err(GraphWireError::Shape("max_cost"));
        }
        one_of(
            policy,
            "priority",
            &names(CostPriority::ALL, |p| p.as_str()),
        )
    })?;
    integer(map, "time_budget_seconds", 1)?;
    integer(map, "turn_budget", 1)?;
    if string(map, "branch")?.is_some_and(|b| !b.starts_with("runtime-a3/")) {
        return Err(GraphWireError::Shape("branch"));
    }
    nonempty(map, "worktree")?;
    one_of(
        map,
        "remote_publish_policy",
        &names(RemotePublishPolicy::ALL, |p| p.as_str()),
    )?;
    string_list(map, "stop_conditions", 1)?;
    nested(map, "handoff_schema", validate_ref)?;
    integer(map, "context_epoch", 0)?;
    Ok(())
}

fn validate_repair_capsule(map: &Map) -> WireResult<()> {
    keys(
        map,
        &[
            "task_id",
            "original_task_id",
            "parent_attempt_id",
            "attempt_number",
            "current_sha",
            "original_objective",
            "original_acceptance_criteria",
            "a4_findings",
            "blocking_findings",
            "failed_checks",
            "repair_scope",
            "prior_attempt_summary",
            "relevant_context_refs",
            "allowed_write_paths",
            "forbidden_write_paths",
            "required_quality_floor",
            "branch",
            "worktree",
            "context_epoch",
        ],
        &[
            "task_id",
            "original_task_id",
            "attempt_number",
            "current_sha",
            "original_objective",
            "original_acceptance_criteria",
            "blocking_findings",
            "repair_scope",
            "required_quality_floor",
            "allowed_write_paths",
            "branch",
            "worktree",
            "context_epoch",
        ],
        "RepairCapsule",
    )?;
    for field in ["task_id", "original_task_id", "parent_attempt_id"] {
        identifier(map, field)?;
    }
    integer(map, "attempt_number", 2)?;
    hex(map, "current_sha", 40)?;
    nonempty(map, "original_objective")?;
    objects(map, "original_acceptance_criteria", 1, validate_criterion)?;
    objects(map, "a4_findings", 0, validate_finding)?;
    objects(map, "blocking_findings", 1, validate_finding)?;
    objects(map, "failed_checks", 0, validate_check)?;
    nonempty(map, "repair_scope")?;
    string(map, "prior_attempt_summary")?;
    objects(map, "relevant_context_refs", 0, validate_ref)?;
    string_list(map, "allowed_write_paths", 1)?;
    strings(map, "forbidden_write_paths")?;
    one_of(map, "required_quality_floor", &FLOORS)?;
    if string(map, "branch")?.is_some_and(|b| !b.starts_with("runtime-a3/")) {
        return Err(GraphWireError::Shape("branch"));
    }
    string(map, "worktree")?;
    integer(map, "context_epoch", 0)?;
    Ok(())
}

const AXES: [(&str, &[&str]); 6] = [
    ("entitlement", &["result", "feature_admission_decision_id"]),
    (
        "provider_auth",
        &["result", "provider_id", "technical_status"],
    ),
    (
        "provider_policy",
        &[
            "result",
            "provider_policy_eligibility_id",
            "policy_status",
            "execution_context",
        ],
    ),
    (
        "provider_availability",
        &["result", "availability_state_ref"],
    ),
    ("safety", &["result", "safety_interruption_id"]),
    ("quality_floor", &["result", "routing_decision_id"]),
];

fn validate_dispatch_decision(map: &Map) -> WireResult<()> {
    keys(
        map,
        &[
            "decision_id",
            "node_id",
            "graph_id",
            "capability_id",
            "outcome",
            "failing_axis",
            "denial_reason",
            "axis_results",
            "selected_provider",
            "selected_runtime",
            "decided_at",
        ],
        &[
            "decision_id",
            "node_id",
            "outcome",
            "failing_axis",
            "axis_results",
            "decided_at",
        ],
        "DispatchAdmissionDecision",
    )?;
    for field in ["decision_id", "node_id", "graph_id"] {
        identifier(map, field)?;
    }
    string(map, "capability_id")?;
    one_of(map, "outcome", &["ALLOW", "DENY"])?;
    one_of(
        map,
        "failing_axis",
        &names(DispatchAdmissionFailingAxis::ALL, |a| a.as_str()),
    )?;
    one_of(
        map,
        "denial_reason",
        &names(DispatchAdmissionDenialReason::ALL, |r| r.as_str()),
    )?;
    let axes = object(&map["axis_results"], "axis_results")?;
    let required: Vec<&'static str> = AXES.iter().map(|(name, _)| *name).collect();
    keys(axes, &required, &required, "axis_results")?;
    for (name, fields) in AXES {
        let axis = object(&axes[name], "axis_results")?;
        keys(axis, fields, &["result"], "axis_results")?;
        one_of(axis, "result", &AXIS_RESULTS)?;
        for field in [
            "feature_admission_decision_id",
            "provider_policy_eligibility_id",
            "availability_state_ref",
            "safety_interruption_id",
            "routing_decision_id",
        ] {
            identifier(axis, field)?;
        }
        string(axis, "provider_id")?;
        string(axis, "technical_status")?;
        string(axis, "execution_context")?;
        one_of(
            axis,
            "policy_status",
            &names(DispatchAdmissionProviderPolicyStatus::ALL, |s| s.as_str()),
        )?;
    }
    nullable_string(map, "selected_provider")?;
    nullable_string(map, "selected_runtime")?;
    string(map, "decided_at")?;
    Ok(())
}

fn validate_feature_decision(map: &Map) -> WireResult<()> {
    keys(
        map,
        &[
            "decision_id",
            "capability_id",
            "outcome",
            "entitlement_state",
            "tier_id",
            "reason",
            "upgrade_info",
            "dispatch_permitted",
            "decided_at",
            "activation_state",
        ],
        &[
            "decision_id",
            "capability_id",
            "outcome",
            "entitlement_state",
            "activation_state",
            "decided_at",
        ],
        "FeatureAdmissionDecision",
    )?;
    identifier(map, "decision_id")?;
    capability(map, "capability_id")?;
    one_of(
        map,
        "outcome",
        &names(FeatureAdmissionOutcome::ALL, |o| o.as_str()),
    )?;
    one_of(
        map,
        "entitlement_state",
        &names(ProductEntitlementState::ALL, |s| s.as_str()),
    )?;
    string(map, "tier_id")?;
    string(map, "reason")?;
    nullable_string(map, "upgrade_info")?;
    boolean(map, "dispatch_permitted")?;
    string(map, "decided_at")?;
    one_of(
        map,
        "activation_state",
        &names(ACTIVATION_STATES, |s| s.as_str()),
    )?;
    Ok(())
}

const ACTIVATION_STATES: [ActivationStateKind; 3] = [
    ActivationStateKind::NeverActivated,
    ActivationStateKind::ActivatedKnown,
    ActivationStateKind::LoggedOut,
];

fn validate_capability_set(map: &Map) -> WireResult<()> {
    keys(
        map,
        &["catalog_version", "features"],
        &["catalog_version", "features"],
        "FeatureCapabilitySet",
    )?;
    integer(map, "catalog_version", 1)?;
    objects(map, "features", 1, |feature| {
        keys(
            feature,
            &[
                "capability_id",
                "name",
                "description",
                "tier_required",
                "status",
                "upgrade_hint",
            ],
            &["capability_id", "name", "tier_required", "description"],
            "features",
        )?;
        capability(feature, "capability_id")?;
        string(feature, "name")?;
        nonempty(feature, "description")?;
        nonempty(feature, "tier_required")?;
        one_of(
            feature,
            "status",
            &names(FeatureCapabilityStatus::ALL, |s| s.as_str()),
        )?;
        string(feature, "upgrade_hint")?;
        Ok(())
    })
}

fn validate_goal_evaluation(map: &Map) -> WireResult<()> {
    keys(
        map,
        &[
            "evaluation_id",
            "goal_id",
            "project_id",
            "state",
            "deterministic_layer",
            "semantic_layer",
            "gaps",
            "convergence",
            "integrated_sha",
            "context_epoch",
            "evaluated_at",
        ],
        &[
            "evaluation_id",
            "goal_id",
            "state",
            "deterministic_layer",
            "evaluated_at",
        ],
        "GoalEvaluation",
    )?;
    for field in ["evaluation_id", "goal_id", "project_id"] {
        identifier(map, field)?;
    }
    one_of(
        map,
        "state",
        &names(GoalEvaluationState::ALL, |s| s.as_str()),
    )?;
    nested(map, "deterministic_layer", |layer| {
        keys(
            layer,
            &["passed", "conditions"],
            &["passed"],
            "deterministic_layer",
        )?;
        boolean(layer, "passed")?;
        objects(layer, "conditions", 0, |condition| {
            keys(
                condition,
                &["condition", "result"],
                &["condition", "result"],
                "conditions",
            )?;
            string(condition, "condition")?;
            one_of(condition, "result", &AXIS_RESULTS)
        })
    })?;
    nested(map, "semantic_layer", |layer| {
        keys(
            layer,
            &["performed", "routing_decision_id", "criteria_assessed"],
            &[],
            "semantic_layer",
        )?;
        boolean(layer, "performed")?;
        identifier(layer, "routing_decision_id")?;
        objects(layer, "criteria_assessed", 0, |c| {
            keys(
                c,
                &["criterion_id", "satisfied", "evidence_ref", "note"],
                &["criterion_id", "satisfied"],
                "criteria_assessed",
            )?;
            identifier(c, "criterion_id")?;
            boolean(c, "satisfied")?;
            nested(c, "evidence_ref", validate_ref)?;
            string(c, "note").map(|_| ())
        })
    })?;
    objects(map, "gaps", 0, |gap| {
        keys(
            gap,
            &[
                "criterion_id",
                "description",
                "evidence_ref",
                "proposed_task",
            ],
            &["criterion_id", "description"],
            "gaps",
        )?;
        identifier(gap, "criterion_id")?;
        string(gap, "description")?;
        nested(gap, "evidence_ref", validate_ref)?;
        string(gap, "proposed_task").map(|_| ())
    })?;
    nested(map, "convergence", |c| {
        keys(c, &["repeated_gap_count", "escalated"], &[], "convergence")?;
        integer(c, "repeated_gap_count", 0)?;
        boolean(c, "escalated").map(|_| ())
    })?;
    hex(map, "integrated_sha", 40)?;
    integer(map, "context_epoch", 0)?;
    string(map, "evaluated_at")?;
    Ok(())
}

// --- numeric domains -----------------------------------------------------------

/// `-?(0|[1-9][0-9]*)` and not `-0`.
fn canonical_integer(lexeme: &str) -> bool {
    let digits = lexeme.strip_prefix('-').unwrap_or(lexeme);
    let canonical = digits == "0"
        || (digits
            .as_bytes()
            .first()
            .is_some_and(|b| (b'1'..=b'9').contains(b))
            && digits.bytes().all(|b| b.is_ascii_digit()));
    canonical && lexeme != "-0"
}

fn canonical_lexeme<'a>(map: &'a Map, field: &'static str) -> WireResult<Option<&'a str>> {
    match lexeme(map, field) {
        Some(l) if !canonical_integer(l) => Err(GraphWireError::Unsupported(field)),
        other => Ok(other),
    }
}

fn int64(map: &Map, field: &'static str) -> WireResult<Option<i64>> {
    canonical_lexeme(map, field)?
        .map(|l| l.parse().map_err(|_| GraphWireError::Unsupported(field)))
        .transpose()
}

fn unsupported<E>(field: &'static str) -> impl Fn(E) -> GraphWireError {
    move |_| GraphWireError::Unsupported(field)
}

// --- shared operational conversions --------------------------------------------

fn sub<'a>(map: &'a Map, field: &str) -> Option<&'a Map> {
    match map.get(field) {
        Some(V::Object(o)) => Some(o.as_map()),
        _ => None,
    }
}

fn list<'a>(map: &'a Map, field: &str) -> Option<Vec<&'a Map>> {
    match map.get(field) {
        Some(V::Array(items)) => Some(
            items
                .iter()
                .map(|i| match i {
                    V::Object(o) => o.as_map(),
                    _ => unreachable!("validated object array"),
                })
                .collect(),
        ),
        _ => None,
    }
}

fn to_ref(map: &Map) -> WireResult<Ref> {
    Ref::try_new(
        pick(RefType::ALL, |r| r.as_str(), text(map, "ref_type")),
        text(map, "target").to_owned(),
        opt_text(map, "digest").map(str::to_owned),
        opt_text(map, "section").map(str::to_owned),
    )
    .map_err(unsupported("ref"))
}

fn from_ref(r: &Ref) -> V {
    let mut map = Map::new();
    map.insert("ref_type".into(), s(r.ref_type().as_str()));
    map.insert("target".into(), s(r.target()));
    put(&mut map, "digest", r.digest().map(s));
    put(&mut map, "section", r.section().map(s));
    object_value(map)
}

fn refs(map: &Map, field: &'static str) -> WireResult<Option<Vec<Ref>>> {
    list(map, field)
        .map(|items| items.into_iter().map(to_ref).collect())
        .transpose()
}

fn put_refs(map: &mut Map, field: &str, refs: Option<&[Ref]>) {
    put(
        map,
        field,
        refs.map(|r| V::Array(r.iter().map(from_ref).collect())),
    );
}

fn string_array(values: &[String]) -> V {
    V::Array(values.iter().map(|v| s(v)).collect())
}

fn to_criterion(map: &Map) -> WireResult<Criterion> {
    Criterion::try_new(
        text(map, "id").to_owned(),
        text(map, "description").to_owned(),
        pick(CriterionKind::ALL, |k| k.as_str(), text(map, "kind")),
        opt_strings(map, "check_command"),
        opt_text(map, "rationale").map(str::to_owned),
    )
    .map_err(unsupported("criterion"))
}

fn from_criterion(c: &Criterion) -> V {
    let mut map = Map::new();
    map.insert("id".into(), s(c.id()));
    map.insert("description".into(), s(c.description()));
    map.insert("kind".into(), s(c.kind().as_str()));
    put(
        &mut map,
        "check_command",
        c.check_command().map(string_array),
    );
    put(&mut map, "rationale", c.rationale().map(s));
    object_value(map)
}

fn criteria(map: &Map, field: &'static str) -> WireResult<Vec<Criterion>> {
    list(map, field)
        .unwrap_or_default()
        .into_iter()
        .map(to_criterion)
        .collect()
}

fn capabilities(map: &Map, field: &'static str) -> WireResult<Option<Vec<CapabilityName>>> {
    opt_strings(map, field)
        .map(|caps| {
            caps.into_iter()
                .map(|c| CapabilityName::new(c).map_err(unsupported(field)))
                .collect()
        })
        .transpose()
}

fn floor(value: &str) -> QualityFloor {
    pick(QualityFloor::ALL, |f| f.as_str(), value)
}

// --- TaskCapsule ------------------------------------------------------------------

impl TaskCapsuleWireV1 {
    /// Converts to the operational capsule (see the module notes).
    pub fn to_task_capsule(&self) -> WireResult<TaskCapsule> {
        let map = self.0.as_map();
        let start_sha = text(map, "start_sha");
        let plan = list(map, "verification_plan")
            .unwrap_or_default()
            .into_iter()
            .map(|entry| {
                let projected = text(entry, "source") == "WORKER_EXECUTION"
                    && text(entry, "code_sha") == start_sha
                    && entry.len() == 4;
                if !projected {
                    return Err(GraphWireError::Unsupported("verification_plan"));
                }
                VerificationPlanEntryV1::try_new(
                    opt_strings(entry, "command").expect("validated"),
                    int64(entry, "exit_code")?.expect("validated"),
                )
                .map_err(unsupported("verification_plan"))
            })
            .collect::<WireResult<Vec<_>>>()?;
        let review_policy = sub(map, "review_policy")
            .map(|p| {
                ReviewPolicy::try_new(
                    p.get("required").map(|v| matches!(v, V::Boolean(true))),
                    opt_pick(
                        DistinctProvider::ALL,
                        |d| d.as_str(),
                        p,
                        "distinct_provider",
                    ),
                    opt_text(p, "reviewer_floor").map(floor),
                )
                .map_err(unsupported("review_policy"))
            })
            .transpose()?;
        let cost_policy = sub(map, "cost_policy")
            .map(|p| {
                let max_cost = match p.get("max_cost") {
                    None => None,
                    Some(V::Null) => Some(None),
                    Some(V::Number(n)) => Some(Some(
                        n.as_str().parse::<f64>().map_err(unsupported("max_cost"))?,
                    )),
                    Some(_) => unreachable!("validated max_cost"),
                };
                CostPolicy::try_new(
                    max_cost,
                    opt_pick(CostPriority::ALL, |c| c.as_str(), p, "priority"),
                )
                .map_err(unsupported("max_cost"))
            })
            .transpose()?;
        TaskCapsule::try_new(
            text(map, "task_id").to_owned(),
            text(map, "workstream_id").to_owned(),
            opt_text(map, "parent_task_id").map(str::to_owned),
            int64(map, "attempt_number")?.expect("validated"),
            pick(TaskType::ALL, |t| t.as_str(), text(map, "task_type")),
            text(map, "objective").to_owned(),
            criteria(map, "acceptance_criteria")?,
            opt_strings(map, "non_goals"),
            text(map, "baseline_sha").to_owned(),
            start_sha.to_owned(),
            opt_strings(map, "allowed_write_paths").expect("validated"),
            opt_strings(map, "forbidden_write_paths").expect("validated"),
            refs(map, "relevant_context_refs")?,
            refs(map, "architecture_refs")?,
            refs(map, "contract_refs")?,
            opt_strings(map, "dependencies"),
            floor(text(map, "quality_floor")),
            capabilities(map, "required_capabilities")?.expect("validated"),
            capabilities(map, "preferred_capabilities")?,
            plan,
            review_policy,
            pick(
                AssuranceProfileSelector::ALL,
                |a| a.as_str(),
                text(map, "assurance_profile"),
            ),
            cost_policy,
            int64(map, "time_budget_seconds")?,
            int64(map, "turn_budget")?,
            text(map, "branch").to_owned(),
            text(map, "worktree").to_owned(),
            opt_pick(
                RemotePublishPolicy::ALL,
                |r| r.as_str(),
                map,
                "remote_publish_policy",
            ),
            opt_strings(map, "stop_conditions").expect("validated"),
            sub(map, "handoff_schema").map(to_ref).transpose()?,
            int64(map, "context_epoch")?.expect("validated"),
        )
        .map_err(unsupported("TaskCapsule"))
    }

    /// Encodes an operational capsule. Refuses a capsule with no required
    /// capability, which the schema cannot represent.
    pub fn from_task_capsule(c: &TaskCapsule) -> WireResult<Self> {
        if c.required_capabilities().is_empty() {
            return Err(GraphWireError::Unsupported("required_capabilities"));
        }
        let mut map = Map::new();
        map.insert("task_id".into(), s(c.task_id()));
        map.insert("workstream_id".into(), s(c.workstream_id()));
        put(&mut map, "parent_task_id", c.parent_task_id().map(s));
        map.insert(
            "attempt_number".into(),
            num(&c.attempt_number().to_string()),
        );
        map.insert("task_type".into(), s(c.task_type().as_str()));
        map.insert("objective".into(), s(c.objective()));
        map.insert(
            "acceptance_criteria".into(),
            V::Array(c.acceptance_criteria().iter().map(from_criterion).collect()),
        );
        put(&mut map, "non_goals", c.non_goals().map(string_array));
        map.insert("baseline_sha".into(), s(c.baseline_sha()));
        map.insert("start_sha".into(), s(c.start_sha()));
        map.insert(
            "allowed_write_paths".into(),
            string_array(c.allowed_write_paths()),
        );
        map.insert(
            "forbidden_write_paths".into(),
            string_array(c.forbidden_write_paths()),
        );
        put_refs(&mut map, "relevant_context_refs", c.relevant_context_refs());
        put_refs(&mut map, "architecture_refs", c.architecture_refs());
        put_refs(&mut map, "contract_refs", c.contract_refs());
        put(&mut map, "dependencies", c.dependencies().map(string_array));
        map.insert("quality_floor".into(), s(c.quality_floor().as_str()));
        let caps = |caps: &[CapabilityName]| V::Array(caps.iter().map(|c| s(c.as_str())).collect());
        map.insert(
            "required_capabilities".into(),
            caps(c.required_capabilities()),
        );
        put(
            &mut map,
            "preferred_capabilities",
            c.preferred_capabilities().map(caps),
        );
        map.insert(
            "verification_plan".into(),
            V::Array(
                c.verification_plan()
                    .iter()
                    .map(|entry| {
                        let mut e = Map::new();
                        e.insert("source".into(), s("WORKER_EXECUTION"));
                        e.insert("command".into(), string_array(entry.command()));
                        e.insert(
                            "exit_code".into(),
                            num(&entry.expected_exit_code().to_string()),
                        );
                        e.insert("code_sha".into(), s(c.start_sha()));
                        object_value(e)
                    })
                    .collect(),
            ),
        );
        put(
            &mut map,
            "review_policy",
            c.review_policy().map(|p| {
                let mut m = Map::new();
                put(&mut m, "required", p.required().map(V::Boolean));
                put(
                    &mut m,
                    "distinct_provider",
                    p.distinct_provider().map(|d| s(d.as_str())),
                );
                put(
                    &mut m,
                    "reviewer_floor",
                    p.reviewer_floor().map(|f| s(f.as_str())),
                );
                object_value(m)
            }),
        );
        map.insert(
            "assurance_profile".into(),
            s(c.assurance_profile().as_str()),
        );
        put(
            &mut map,
            "cost_policy",
            c.cost_policy().map(|p| {
                let mut m = Map::new();
                put(
                    &mut m,
                    "max_cost",
                    p.max_cost()
                        .map(|v| v.map_or(V::Null, |n| num(&format!("{n:?}")))),
                );
                put(&mut m, "priority", p.priority().map(|x| s(x.as_str())));
                object_value(m)
            }),
        );
        put(
            &mut map,
            "time_budget_seconds",
            c.time_budget_seconds().map(|v| num(&v.to_string())),
        );
        put(
            &mut map,
            "turn_budget",
            c.turn_budget().map(|v| num(&v.to_string())),
        );
        map.insert("branch".into(), s(c.branch()));
        map.insert("worktree".into(), s(c.worktree()));
        put(
            &mut map,
            "remote_publish_policy",
            c.remote_publish_policy().map(|p| s(p.as_str())),
        );
        map.insert("stop_conditions".into(), string_array(c.stop_conditions()));
        put(&mut map, "handoff_schema", c.handoff_schema().map(from_ref));
        map.insert("context_epoch".into(), num(&c.context_epoch().to_string()));
        Ok(Self(OrchestrationJsonObjectV1::new(map)))
    }
}

// --- RepairCapsule ----------------------------------------------------------------

fn to_finding(map: &Map) -> WireResult<RepairCapsuleFindingV1> {
    RepairCapsuleFindingV1::try_new(
        text(map, "finding_id").to_owned(),
        pick(
            RepairFindingSeverityV1::ALL,
            |x| x.as_str(),
            text(map, "severity"),
        ),
        pick(
            RepairFindingCategoryV1::ALL,
            |x| x.as_str(),
            text(map, "category"),
        ),
        text(map, "description").to_owned(),
        matches!(map.get("blocking"), Some(V::Boolean(true))),
        opt_text(map, "path").map(str::to_owned),
        canonical_lexeme(map, "line")?
            .map(|l| RepairFindingLineV1::try_new(l).map_err(unsupported("line")))
            .transpose()?,
        sub(map, "evidence_ref").map(to_ref).transpose()?,
        opt_pick(
            RepairFindingConfidenceV1::ALL,
            |x| x.as_str(),
            map,
            "confidence",
        ),
        opt_pick(RepairFindingSourceV1::ALL, |x| x.as_str(), map, "source"),
    )
    .map_err(unsupported("finding"))
}

fn from_finding(f: &RepairCapsuleFindingV1) -> V {
    let mut map = Map::new();
    map.insert("finding_id".into(), s(f.finding_id()));
    map.insert("severity".into(), s(f.severity().as_str()));
    map.insert("category".into(), s(f.category().as_str()));
    map.insert("description".into(), s(f.description()));
    map.insert("blocking".into(), V::Boolean(f.blocking()));
    put(&mut map, "path", f.path().map(s));
    put(&mut map, "line", f.line().map(|l| num(l.as_str())));
    put(&mut map, "evidence_ref", f.evidence_ref().map(from_ref));
    put(
        &mut map,
        "confidence",
        f.confidence().map(|c| s(c.as_str())),
    );
    put(&mut map, "source", f.source().map(|c| s(c.as_str())));
    object_value(map)
}

fn to_failed_check(map: &Map) -> WireResult<RepairCapsuleFailedCheckV1> {
    let time = |field: &'static str| {
        opt_text(map, field)
            .map(|t| OrchestrationDateTimeV1::try_new(t).map_err(unsupported(field)))
            .transpose()
    };
    RepairCapsuleFailedCheckV1::try_new(
        pick(
            RepairCheckSourceV1::ALL,
            |x| x.as_str(),
            text(map, "source"),
        ),
        opt_strings(map, "command").expect("validated"),
        RepairExitCodeV1::try_new(canonical_lexeme(map, "exit_code")?.expect("validated"))
            .map_err(unsupported("exit_code"))?,
        text(map, "code_sha").to_owned(),
        map.get("timed_out").map(|v| matches!(v, V::Boolean(true))),
        time("started_at")?,
        time("finished_at")?,
        sub(map, "output_ref").map(to_ref).transpose()?,
        opt_pick(RepairCheckResultV1::ALL, |x| x.as_str(), map, "result"),
    )
    .map_err(unsupported("failed_checks"))
}

fn from_failed_check(c: &RepairCapsuleFailedCheckV1) -> V {
    let mut map = Map::new();
    map.insert("source".into(), s(c.source().as_str()));
    map.insert("command".into(), string_array(c.command()));
    map.insert("exit_code".into(), num(c.exit_code().as_str()));
    map.insert("code_sha".into(), s(c.code_sha()));
    put(&mut map, "timed_out", c.timed_out().map(V::Boolean));
    put(
        &mut map,
        "started_at",
        c.started_at().map(|t| s(t.as_str())),
    );
    put(
        &mut map,
        "finished_at",
        c.finished_at().map(|t| s(t.as_str())),
    );
    put(&mut map, "output_ref", c.output_ref().map(from_ref));
    put(&mut map, "result", c.result().map(|r| s(r.as_str())));
    object_value(map)
}

impl RepairCapsuleWireV1 {
    /// Converts with the parent's quality floor (validation-only evidence).
    pub fn to_repair_capsule(
        &self,
        parent_quality_floor: QualityFloor,
    ) -> WireResult<RepairCapsule> {
        let map = self.0.as_map();
        let findings = |field| {
            list(map, field)
                .map(|items| {
                    items
                        .into_iter()
                        .map(to_finding)
                        .collect::<WireResult<Vec<_>>>()
                })
                .transpose()
        };
        RepairCapsule::try_new(
            text(map, "task_id").to_owned(),
            text(map, "original_task_id").to_owned(),
            opt_text(map, "parent_attempt_id").map(str::to_owned),
            RepairAttemptNumberV1::try_new(
                canonical_lexeme(map, "attempt_number")?.expect("validated"),
            )
            .map_err(unsupported("attempt_number"))?,
            text(map, "current_sha").to_owned(),
            text(map, "original_objective").to_owned(),
            criteria(map, "original_acceptance_criteria")?,
            findings("a4_findings")?,
            findings("blocking_findings")?.expect("validated"),
            list(map, "failed_checks")
                .map(|items| items.into_iter().map(to_failed_check).collect())
                .transpose()?,
            text(map, "repair_scope").to_owned(),
            opt_text(map, "prior_attempt_summary").map(str::to_owned),
            refs(map, "relevant_context_refs")?,
            opt_strings(map, "allowed_write_paths").expect("validated"),
            opt_strings(map, "forbidden_write_paths"),
            floor(text(map, "required_quality_floor")),
            text(map, "branch").to_owned(),
            text(map, "worktree").to_owned(),
            RepairContextEpochV1::try_new(
                canonical_lexeme(map, "context_epoch")?.expect("validated"),
            )
            .map_err(unsupported("context_epoch"))?,
            parent_quality_floor,
        )
        .map_err(unsupported("RepairCapsule"))
    }

    pub fn from_repair_capsule(c: &RepairCapsule) -> Self {
        let mut map = Map::new();
        map.insert("task_id".into(), s(c.task_id()));
        map.insert("original_task_id".into(), s(c.original_task_id()));
        put(&mut map, "parent_attempt_id", c.parent_attempt_id().map(s));
        map.insert("attempt_number".into(), num(c.attempt_number().as_str()));
        map.insert("current_sha".into(), s(c.current_sha()));
        map.insert("original_objective".into(), s(c.original_objective()));
        map.insert(
            "original_acceptance_criteria".into(),
            V::Array(
                c.original_acceptance_criteria()
                    .iter()
                    .map(from_criterion)
                    .collect(),
            ),
        );
        put(
            &mut map,
            "a4_findings",
            c.a4_findings()
                .map(|f| V::Array(f.iter().map(from_finding).collect())),
        );
        map.insert(
            "blocking_findings".into(),
            V::Array(c.blocking_findings().iter().map(from_finding).collect()),
        );
        put(
            &mut map,
            "failed_checks",
            c.failed_checks()
                .map(|f| V::Array(f.iter().map(from_failed_check).collect())),
        );
        map.insert("repair_scope".into(), s(c.repair_scope()));
        put(
            &mut map,
            "prior_attempt_summary",
            c.prior_attempt_summary().map(s),
        );
        put_refs(&mut map, "relevant_context_refs", c.relevant_context_refs());
        map.insert(
            "allowed_write_paths".into(),
            string_array(c.allowed_write_paths()),
        );
        put(
            &mut map,
            "forbidden_write_paths",
            c.forbidden_write_paths().map(string_array),
        );
        map.insert(
            "required_quality_floor".into(),
            s(c.required_quality_floor().as_str()),
        );
        map.insert("branch".into(), s(c.branch()));
        map.insert("worktree".into(), s(c.worktree()));
        map.insert("context_epoch".into(), num(c.context_epoch().as_str()));
        Self(OrchestrationJsonObjectV1::new(map))
    }
}

// --- DispatchAdmissionDecision ----------------------------------------------------

fn axis_result(map: &Map) -> DispatchAdmissionAxisResult {
    pick(
        DispatchAdmissionAxisResult::ALL,
        |r| r.as_str(),
        text(map, "result"),
    )
}

fn owned(map: &Map, field: &str) -> Option<String> {
    opt_text(map, field).map(str::to_owned)
}

impl DispatchAdmissionDecisionWireV1 {
    pub fn to_decision(&self) -> WireResult<DispatchAdmissionDecision> {
        let map = self.0.as_map();
        let axes = sub(map, "axis_results").expect("validated");
        let axis = |name: &str| sub(axes, name).expect("validated");
        let (e, a, p, v, sf, q) = (
            axis("entitlement"),
            axis("provider_auth"),
            axis("provider_policy"),
            axis("provider_availability"),
            axis("safety"),
            axis("quality_floor"),
        );
        let results = DispatchAdmissionAxisResults {
            entitlement: DispatchAdmissionEntitlementAxisResult::try_new(
                axis_result(e),
                owned(e, "feature_admission_decision_id"),
            )
            .map_err(unsupported("axis_results"))?,
            provider_auth: DispatchAdmissionProviderAuthAxisResult::new(
                axis_result(a),
                owned(a, "provider_id"),
                owned(a, "technical_status"),
            ),
            provider_policy: DispatchAdmissionProviderPolicyAxisResult::try_new(
                axis_result(p),
                owned(p, "provider_policy_eligibility_id"),
                opt_pick(
                    DispatchAdmissionProviderPolicyStatus::ALL,
                    |x| x.as_str(),
                    p,
                    "policy_status",
                ),
                owned(p, "execution_context"),
            )
            .map_err(unsupported("axis_results"))?,
            provider_availability: DispatchAdmissionProviderAvailabilityAxisResult::try_new(
                axis_result(v),
                owned(v, "availability_state_ref"),
            )
            .map_err(unsupported("axis_results"))?,
            safety: DispatchAdmissionSafetyAxisResult::try_new(
                axis_result(sf),
                owned(sf, "safety_interruption_id"),
            )
            .map_err(unsupported("axis_results"))?,
            quality_floor: DispatchAdmissionQualityFloorAxisResult::try_new(
                axis_result(q),
                owned(q, "routing_decision_id"),
            )
            .map_err(unsupported("axis_results"))?,
        };
        let core = DispatchAdmissionDecisionCore::try_new(
            text(map, "decision_id").to_owned(),
            text(map, "node_id").to_owned(),
            owned(map, "graph_id"),
            owned(map, "capability_id"),
            pick(
                DispatchAdmissionOutcome::ALL,
                |o| o.as_str(),
                text(map, "outcome"),
            ),
            pick(
                DispatchAdmissionFailingAxis::ALL,
                |x| x.as_str(),
                text(map, "failing_axis"),
            ),
            opt_pick(
                DispatchAdmissionDenialReason::ALL,
                |x| x.as_str(),
                map,
                "denial_reason",
            ),
            owned(map, "selected_provider"),
            owned(map, "selected_runtime"),
        )
        .map_err(unsupported("DispatchAdmissionDecision"))?;
        Ok(DispatchAdmissionDecision::new(
            DispatchAdmissionDecisionNonTemporalCore::try_new(core, results)
                .map_err(unsupported("DispatchAdmissionDecision"))?,
            date_time(map, "decided_at")?,
        ))
    }

    pub fn from_decision(d: &DispatchAdmissionDecision) -> Self {
        let core = d.core().core();
        let axes = d.core().axis_results();
        let mut map = Map::new();
        map.insert("decision_id".into(), s(core.decision_id()));
        map.insert("node_id".into(), s(core.node_id()));
        put(&mut map, "graph_id", core.graph_id().map(s));
        put(&mut map, "capability_id", core.capability_id().map(s));
        map.insert("outcome".into(), s(core.outcome().as_str()));
        map.insert("failing_axis".into(), s(core.failing_axis().as_str()));
        put(
            &mut map,
            "denial_reason",
            core.denial_reason().map(|r| s(r.as_str())),
        );
        let axis = |result: DispatchAdmissionAxisResult, extra: Vec<(&str, Option<V>)>| {
            let mut m = Map::new();
            m.insert("result".into(), s(result.as_str()));
            for (k, v) in extra {
                put(&mut m, k, v);
            }
            object_value(m)
        };
        let mut axis_map = Map::new();
        axis_map.insert(
            "entitlement".into(),
            axis(
                axes.entitlement.result(),
                vec![(
                    "feature_admission_decision_id",
                    axes.entitlement.feature_admission_decision_id().map(s),
                )],
            ),
        );
        axis_map.insert(
            "provider_auth".into(),
            axis(
                axes.provider_auth.result(),
                vec![
                    ("provider_id", axes.provider_auth.provider_id().map(s)),
                    (
                        "technical_status",
                        axes.provider_auth.technical_status().map(s),
                    ),
                ],
            ),
        );
        axis_map.insert(
            "provider_policy".into(),
            axis(
                axes.provider_policy.result(),
                vec![
                    (
                        "provider_policy_eligibility_id",
                        axes.provider_policy.provider_policy_eligibility_id().map(s),
                    ),
                    (
                        "policy_status",
                        axes.provider_policy.policy_status().map(|x| s(x.as_str())),
                    ),
                    (
                        "execution_context",
                        axes.provider_policy.execution_context().map(s),
                    ),
                ],
            ),
        );
        axis_map.insert(
            "provider_availability".into(),
            axis(
                axes.provider_availability.result(),
                vec![(
                    "availability_state_ref",
                    axes.provider_availability.availability_state_ref().map(s),
                )],
            ),
        );
        axis_map.insert(
            "safety".into(),
            axis(
                axes.safety.result(),
                vec![(
                    "safety_interruption_id",
                    axes.safety.safety_interruption_id().map(s),
                )],
            ),
        );
        axis_map.insert(
            "quality_floor".into(),
            axis(
                axes.quality_floor.result(),
                vec![(
                    "routing_decision_id",
                    axes.quality_floor.routing_decision_id().map(s),
                )],
            ),
        );
        map.insert("axis_results".into(), object_value(axis_map));
        map.insert(
            "selected_provider".into(),
            core.selected_provider().map_or(V::Null, s),
        );
        map.insert(
            "selected_runtime".into(),
            core.selected_runtime().map_or(V::Null, s),
        );
        map.insert("decided_at".into(), s(d.decided_at().as_str()));
        Self(OrchestrationJsonObjectV1::new(map))
    }
}

// --- FeatureAdmissionDecision -----------------------------------------------------

impl FeatureAdmissionDecisionWireV1 {
    pub fn to_decision(&self) -> WireResult<FeatureAdmissionDecision> {
        let map = self.0.as_map();
        FeatureAdmissionDecision::try_new(
            text(map, "decision_id"),
            CapabilityName::new(text(map, "capability_id")).expect("validated capability"),
            pick(
                FeatureAdmissionOutcome::ALL,
                |o| o.as_str(),
                text(map, "outcome"),
            ),
            pick(
                ProductEntitlementState::ALL,
                |x| x.as_str(),
                text(map, "entitlement_state"),
            ),
            pick(
                ACTIVATION_STATES,
                |x| x.as_str(),
                text(map, "activation_state"),
            ),
            owned(map, "tier_id"),
            owned(map, "reason"),
            match map.get("upgrade_info") {
                None => None,
                Some(V::Null) => Some(FeatureAdmissionUpgradeInfo::Null),
                Some(V::String(v)) => Some(FeatureAdmissionUpgradeInfo::String(v.clone())),
                Some(_) => unreachable!("validated upgrade_info"),
            },
            map.get("dispatch_permitted")
                .map(|v| matches!(v, V::Boolean(true))),
            date_time(map, "decided_at")?,
        )
        .map_err(unsupported("FeatureAdmissionDecision"))
    }

    pub fn from_decision(d: &FeatureAdmissionDecision) -> Self {
        let mut map = Map::new();
        map.insert("decision_id".into(), s(d.decision_id()));
        map.insert("capability_id".into(), s(d.capability_id().as_str()));
        map.insert("outcome".into(), s(d.outcome().as_str()));
        map.insert(
            "entitlement_state".into(),
            s(d.entitlement_state().as_str()),
        );
        put(&mut map, "tier_id", d.tier_id().map(s));
        put(&mut map, "reason", d.reason().map(s));
        put(
            &mut map,
            "upgrade_info",
            d.upgrade_info().map(|u| match u {
                FeatureAdmissionUpgradeInfo::Null => V::Null,
                FeatureAdmissionUpgradeInfo::String(v) => s(v),
            }),
        );
        put(
            &mut map,
            "dispatch_permitted",
            d.dispatch_permitted().map(V::Boolean),
        );
        map.insert("decided_at".into(), s(d.decided_at().as_str()));
        map.insert("activation_state".into(), s(d.activation_state().as_str()));
        Self(OrchestrationJsonObjectV1::new(map))
    }
}

// --- FeatureCapabilitySet ---------------------------------------------------------

impl FeatureCapabilitySetWireV1 {
    pub fn to_capability_set(&self) -> WireResult<FeatureCapabilitySet> {
        let map = self.0.as_map();
        let features = list(map, "features")
            .unwrap_or_default()
            .into_iter()
            .map(|f| {
                FeatureCapability::try_new(
                    CapabilityName::new(text(f, "capability_id")).expect("validated capability"),
                    text(f, "name"),
                    text(f, "tier_required"),
                    text(f, "description"),
                    opt_pick(FeatureCapabilityStatus::ALL, |x| x.as_str(), f, "status"),
                    owned(f, "upgrade_hint"),
                )
                .map_err(unsupported("features"))
            })
            .collect::<WireResult<Vec<_>>>()?;
        FeatureCapabilitySet::try_new(
            canonical_lexeme(map, "catalog_version")?.expect("validated"),
            features,
        )
        .map_err(unsupported("catalog_version"))
    }

    pub fn from_capability_set(set: &FeatureCapabilitySet) -> Self {
        let mut map = Map::new();
        map.insert("catalog_version".into(), num(set.catalog_version()));
        map.insert(
            "features".into(),
            V::Array(
                set.features()
                    .iter()
                    .map(|f| {
                        let mut m = Map::new();
                        m.insert("capability_id".into(), s(f.capability_id().as_str()));
                        m.insert("name".into(), s(f.name()));
                        m.insert("description".into(), s(f.description()));
                        m.insert("tier_required".into(), s(f.tier_required()));
                        put(&mut m, "status", f.status().map(|x| s(x.as_str())));
                        put(&mut m, "upgrade_hint", f.upgrade_hint().map(s));
                        object_value(m)
                    })
                    .collect(),
            ),
        );
        Self(OrchestrationJsonObjectV1::new(map))
    }
}

// --- GoalEvaluation ---------------------------------------------------------------

impl GoalEvaluationWireV1 {
    /// The deterministic projection; semantic layer, gaps and convergence
    /// have no operational representation yet and are refused.
    pub fn to_deterministic_core(&self) -> WireResult<DeterministicGoalEvaluationCore> {
        let map = self.0.as_map();
        for field in ["semantic_layer", "gaps", "convergence"] {
            if map.contains_key(field) {
                return Err(GraphWireError::Unsupported(field));
            }
        }
        let layer = sub(map, "deterministic_layer").expect("validated");
        let conditions = list(layer, "conditions").map(|items| {
            items
                .into_iter()
                .map(|c| DeterministicCondition {
                    condition: text(c, "condition").to_owned(),
                    result: pick(
                        DeterministicConditionResult::ALL,
                        |r| r.as_str(),
                        text(c, "result"),
                    ),
                })
                .collect()
        });
        let layer = DeterministicLayer::try_new(
            matches!(layer.get("passed"), Some(V::Boolean(true))),
            conditions,
        )
        .map_err(unsupported("deterministic_layer"))?;
        let epoch = canonical_lexeme(map, "context_epoch")?
            .map(|e| {
                GoalEvaluationContextEpoch::from_decimal(e).map_err(unsupported("context_epoch"))
            })
            .transpose()?;
        DeterministicGoalEvaluationCore::try_new_with_context_epoch(
            text(map, "evaluation_id").to_owned(),
            text(map, "goal_id").to_owned(),
            owned(map, "project_id"),
            pick(GoalEvaluationState::ALL, |x| x.as_str(), text(map, "state")),
            layer,
            owned(map, "integrated_sha"),
            epoch,
            date_time(map, "evaluated_at")?,
        )
        .map_err(unsupported("GoalEvaluation"))
    }

    pub fn from_deterministic_core(core: &DeterministicGoalEvaluationCore) -> Self {
        let mut map = Map::new();
        map.insert("evaluation_id".into(), s(core.evaluation_id()));
        map.insert("goal_id".into(), s(core.goal_id()));
        put(&mut map, "project_id", core.project_id().map(s));
        map.insert("state".into(), s(core.state().as_str()));
        let mut layer = Map::new();
        layer.insert(
            "passed".into(),
            V::Boolean(core.deterministic_layer().passed()),
        );
        put(
            &mut layer,
            "conditions",
            core.deterministic_layer().conditions().map(|cs| {
                V::Array(
                    cs.iter()
                        .map(|c| {
                            let mut m = Map::new();
                            m.insert("condition".into(), s(&c.condition));
                            m.insert("result".into(), s(c.result.as_str()));
                            object_value(m)
                        })
                        .collect(),
                )
            }),
        );
        map.insert("deterministic_layer".into(), object_value(layer));
        put(&mut map, "integrated_sha", core.integrated_sha().map(s));
        put(
            &mut map,
            "context_epoch",
            core.context_epoch().map(|e| num(e.decimal_digits())),
        );
        map.insert("evaluated_at".into(), s(core.evaluated_at().as_str()));
        Self(OrchestrationJsonObjectV1::new(map))
    }
}
