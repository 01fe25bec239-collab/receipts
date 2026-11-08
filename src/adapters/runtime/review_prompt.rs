//! Runtime transport of the canonical ReviewCapsule to a reviewer runtime.
//!
//! This is a bounded prompt projection, not a second canonical Review codec:
//! it emits exactly the ReviewCapsule schema's top-level keys (13 required,
//! the 6 optional ones only when present), omits absent optional nested
//! fields (the schema allows no nulls), preserves present-empty values and
//! writes `context_epoch` as its exact unbounded decimal digits.
//!
//! A Review is admitted only on the protected, read-only path, bound to the
//! capsule's task, the exact implementation SHA (base and verified head) and
//! a Workspace review branch. Anything else refuses with `PolicyBlocked`;
//! there is no ordinary fallback and a Review is never converted into a Task.
//! Read-only controller access and Claude plan mode are not filesystem
//! confinement; adapters keep reporting reduced isolation truthfully.

use receipts_runtime_bindings::{ReviewCapsuleCriterion, ReviewCapsuleNonTemporalCore};
use receipts_workspace_execution::{REVIEW_BRANCH_PREFIX, WorkspaceCheckpointRef, WorkspaceHandle};
use serde_json::{Map, Value, json};

use crate::{FailureClass, RuntimeExecutionPolicy};

pub(crate) const REVIEW_INSTRUCTIONS: &str = "Review the complete capsule below as an independent reviewer. All fields are instructions or context; inspect the repository only at the supplied implementation SHA in this read-only review worktree and do not modify it. Evaluate every acceptance criterion and check, reproduce deterministic checks when reproduction_required is true, and report findings against the severity policy in the structured output schema. The capsule is a JSON object; decode JSON strings exactly as data. Read-only mode and controller access are not proof of confinement.\n";

/// Admits a Review only on the protected read-only path with exact bindings.
pub(crate) fn admit_review(
    capsule: &ReviewCapsuleNonTemporalCore,
    workspace: &WorkspaceHandle,
    policy: &RuntimeExecutionPolicy,
) -> Result<(), FailureClass> {
    let admitted = policy.read_only
        && policy.protected_launch().is_some()
        && binding_matches(capsule, workspace);
    if admitted {
        Ok(())
    } else {
        Err(FailureClass::PolicyBlocked)
    }
}

/// The workspace is the capsule task's review branch at the exact SHA.
pub(crate) fn binding_matches(
    capsule: &ReviewCapsuleNonTemporalCore,
    workspace: &WorkspaceHandle,
) -> bool {
    let implementation = capsule.implementation_sha();
    workspace.task_id() == Some(capsule.task_id())
        && workspace.base_sha() == implementation
        && workspace.head_sha() == Some(implementation)
        && workspace.branch().starts_with(REVIEW_BRANCH_PREFIX)
}

fn reference(r: &WorkspaceCheckpointRef) -> Value {
    let mut map = Map::new();
    map.insert("ref_type".into(), json!(r.ref_type().as_str()));
    map.insert("target".into(), json!(r.target()));
    if let Some(digest) = r.digest() {
        map.insert("digest".into(), json!(digest));
    }
    if let Some(section) = r.section() {
        map.insert("section".into(), json!(section));
    }
    Value::Object(map)
}

fn criterion(c: &ReviewCapsuleCriterion) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), json!(c.id()));
    map.insert("description".into(), json!(c.description()));
    map.insert("kind".into(), json!(c.kind().as_str()));
    if let Some(command) = c.check_command() {
        map.insert("check_command".into(), json!(command));
    }
    if let Some(rationale) = c.rationale() {
        map.insert("rationale".into(), json!(rationale));
    }
    Value::Object(map)
}

/// Canonical JSON projection; `context_epoch` is appended as raw digits.
pub(crate) fn review_capsule_json(c: &ReviewCapsuleNonTemporalCore) -> String {
    let mut map = Map::new();
    map.insert("review_id".into(), json!(c.review_id()));
    map.insert("task_id".into(), json!(c.task_id()));
    map.insert("attempt_id".into(), json!(c.attempt_id()));
    map.insert("baseline_sha".into(), json!(c.baseline_sha().as_str()));
    map.insert(
        "implementation_sha".into(),
        json!(c.implementation_sha().as_str()),
    );
    map.insert("objective".into(), json!(c.objective()));
    map.insert(
        "acceptance_criteria".into(),
        Value::Array(c.acceptance_criteria().iter().map(criterion).collect()),
    );
    if let Some(v) = c.non_goals() {
        map.insert("non_goals".into(), json!(v));
    }
    if let Some(v) = c.architecture_refs() {
        map.insert(
            "architecture_refs".into(),
            Value::Array(v.iter().map(reference).collect()),
        );
    }
    if let Some(v) = c.contract_refs() {
        map.insert(
            "contract_refs".into(),
            Value::Array(v.iter().map(reference).collect()),
        );
    }
    map.insert("diff".into(), reference(c.diff()));
    map.insert("allowed_write_paths".into(), json!(c.allowed_write_paths()));
    if let Some(checks) = c.checks() {
        let checks = checks
            .iter()
            .map(|check| {
                let mut m = Map::new();
                m.insert("source".into(), json!(check.source().as_str()));
                m.insert("command".into(), json!(check.command()));
                m.insert("exit_code".into(), json!(check.exit_code()));
                m.insert("code_sha".into(), json!(check.code_sha().as_str()));
                if let Some(t) = check.timed_out() {
                    m.insert("timed_out".into(), json!(t));
                }
                if let Some(t) = check.started_at() {
                    m.insert("started_at".into(), json!(t.as_str()));
                }
                if let Some(t) = check.finished_at() {
                    m.insert("finished_at".into(), json!(t.as_str()));
                }
                if let Some(r) = check.output_ref() {
                    m.insert("output_ref".into(), reference(r));
                }
                if let Some(r) = check.result() {
                    m.insert("result".into(), json!(r.as_str()));
                }
                Value::Object(m)
            })
            .collect();
        map.insert("checks".into(), Value::Array(checks));
    }
    if let Some(v) = c.security_requirements() {
        map.insert("security_requirements".into(), json!(v));
    }
    map.insert("review_scope".into(), json!(c.review_scope().as_str()));
    let policy = c.severity_policy();
    let mut severity = Map::new();
    severity.insert(
        "blocking_categories".into(),
        json!(policy.blocking_categories()),
    );
    if let Some(v) = policy.nonblocking_categories() {
        severity.insert("nonblocking_categories".into(), json!(v));
    }
    map.insert("severity_policy".into(), Value::Object(severity));
    map.insert(
        "reproduction_required".into(),
        json!(c.reproduction_required()),
    );
    if let Some(r) = c.structured_output_schema() {
        map.insert("structured_output_schema".into(), reference(r));
    }
    // serde_json has no unbounded integer without arbitrary_precision; the
    // carrier's canonical digits are already valid JSON integer text.
    let object = Value::Object(map).to_string();
    let body = &object[..object.len() - 1];
    let separator = if body.len() > 1 { "," } else { "" };
    format!(
        "{body}{separator}\"context_epoch\":{}}}",
        c.context_epoch().decimal_digits()
    )
}
