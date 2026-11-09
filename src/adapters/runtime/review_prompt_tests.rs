//! Portable Review transport controls. Supported (protected, Linux) Review
//! execution through both adapters is NOT RUN here: this host cannot build a
//! ProtectedLauncher, and tests never forge one.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use receipts_runtime_bindings::*;
use receipts_workspace_execution::execution::ProcessTimeoutPolicy;
use receipts_workspace_execution::{
    CommitSha, ReviewWorkspace, ReviewWorkspaceRequest, WorkspaceCheckpointCheckSource,
    WorkspaceCheckpointExecutedCheckCore, WorkspaceCheckpointRef, WorkspaceCheckpointRefType,
    WorkspaceHandle, WorkspaceProvisionRequest, provision_review_workspace,
};

use crate::review_prompt::{admit_review, binding_matches, review_capsule_json};
use crate::{ClaudeAdapter, CodexAdapter, FailureClass, RuntimeAdapter, RuntimeExecutionPolicy};

struct Repo {
    root: PathBuf,
    repo: PathBuf,
    sha: String,
}
impl Repo {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ra-review-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let repo = root.join("repo");
        fs::create_dir(&repo).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("/usr/bin/git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap();
            assert!(out.status.success());
            String::from_utf8(out.stdout).unwrap()
        };
        git(&["init", "-q"]);
        git(&[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "-qm",
            "base",
        ]);
        let sha = git(&["rev-parse", "HEAD"]).trim().to_owned();
        Self { root, repo, sha }
    }
    fn review(&self, task: &str, name: &str) -> ReviewWorkspace {
        provision_review_workspace(&ReviewWorkspaceRequest {
            repository_root: self.repo.clone(),
            workspace_id: format!("ws-{name}"),
            task_id: task.into(),
            reviewed_sha: CommitSha::parse(&self.sha).unwrap(),
            worktree_path: self.root.join(name),
        })
        .unwrap()
    }
    fn task_workspace(&self) -> WorkspaceHandle {
        WorkspaceProvisionRequest::new(
            &self.repo,
            "ws-task",
            Some("task"),
            "runtime/task",
            self.root.join("task"),
            &self.sha,
        )
        .unwrap()
        .provision()
        .unwrap()
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn reference(target: &str, digest: Option<&str>) -> WorkspaceCheckpointRef {
    WorkspaceCheckpointRef::new(
        WorkspaceCheckpointRefType::RepoPath,
        target,
        digest.map(str::to_owned),
        None,
    )
    .unwrap()
}

fn minimal(sha: &str, task: &str) -> ReviewCapsuleNonTemporalCore {
    ReviewCapsuleNonTemporalCore::new(
        "review-1".into(),
        task.into(),
        "attempt-1".into(),
        sha.into(),
        sha.into(),
        "objective".into(),
        vec![
            ReviewCapsuleCriterion::new(
                "c1".into(),
                "d".into(),
                ReviewCapsuleCriterionKind::Deterministic,
                None,
                None,
            )
            .unwrap(),
        ],
        None,
        None,
        None,
        reference("diff.patch", None),
        vec!["src/**".into()],
        None,
        None,
        ReviewCapsuleReviewScope::Full,
        ReviewCapsuleSeverityPolicy::new(vec!["HIGH".into()], None).unwrap(),
        true,
        None,
        0,
    )
    .unwrap()
}

fn full(sha: &str, epoch: &str) -> ReviewCapsuleNonTemporalCore {
    let check = ReviewCapsuleCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::BrokerExecution,
            vec!["cargo".into(), "test".into()],
            0,
            CommitSha::parse(sha).unwrap(),
            Some(false),
            Some(reference("out.log", Some("sha256:aa"))),
        )
        .unwrap(),
        Some(ReviewCapsuleCheckResult::Pass),
    );
    ReviewCapsuleNonTemporalCore::new_with_context_epoch(
        "review-1".into(),
        "task".into(),
        "attempt-1".into(),
        sha.into(),
        sha.into(),
        "objective".into(),
        vec![
            ReviewCapsuleCriterion::new(
                "c1".into(),
                "d".into(),
                ReviewCapsuleCriterionKind::Semantic,
                Some(vec![]),
                Some(String::new()),
            )
            .unwrap(),
        ],
        Some(vec![]),
        Some(vec![reference("ARCH.md", None)]),
        Some(vec![]),
        reference("diff.patch", Some("sha256:bb")),
        vec!["src/**".into()],
        Some(vec![check]),
        Some(vec!["no secrets".into()]),
        ReviewCapsuleReviewScope::Security,
        ReviewCapsuleSeverityPolicy::new(vec!["HIGH".into()], Some(vec![])).unwrap(),
        false,
        Some(reference("schemas/A4Review.schema.json", None)),
        ReviewRequestNonNegativeInteger::from_decimal(epoch).unwrap(),
    )
    .unwrap()
}

fn schema() -> serde_json::Value {
    let path = format!(
        "{}/../../schemas/ReviewCapsule.schema.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}

fn keys(json: &str) -> Vec<String> {
    let value: serde_json::Value = serde_json::from_str(json).unwrap();
    let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

#[test]
fn minimal_projection_has_exactly_the_thirteen_required_keys() {
    let s = schema();
    let mut required: Vec<String> = s["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    required.sort();
    assert_eq!(required.len(), 13);
    let json = review_capsule_json(&minimal(&"a".repeat(40), "task"));
    assert_eq!(keys(&json), required);
    assert!(
        !json.contains("null"),
        "absent optionals are omitted, never null"
    );
}

#[test]
fn full_projection_has_all_nineteen_keys_and_preserves_present_empty_values() {
    let s = schema();
    let mut all: Vec<String> = s["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    all.sort();
    assert_eq!(all.len(), 19);
    let epoch = format!("1{}", "0".repeat(60));
    let json = review_capsule_json(&full(&"b".repeat(40), &epoch));
    assert_eq!(keys(&json), all);
    assert!(
        json.ends_with(&format!("\"context_epoch\":{epoch}}}")),
        "exact unbounded digits"
    );
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["non_goals"], serde_json::json!([]));
    assert_eq!(v["contract_refs"], serde_json::json!([]));
    assert_eq!(
        v["acceptance_criteria"][0]["check_command"],
        serde_json::json!([])
    );
    assert_eq!(
        v["acceptance_criteria"][0]["rationale"],
        serde_json::json!("")
    );
    assert_eq!(
        v["severity_policy"]["nonblocking_categories"],
        serde_json::json!([])
    );
    assert_eq!(v["checks"][0]["result"], "PASS");
    assert_eq!(v["checks"][0]["output_ref"]["digest"], "sha256:aa");
    assert!(v["checks"][0].get("started_at").is_none());
    assert_eq!(v["review_scope"], "SECURITY");
    assert_eq!(v["reproduction_required"], false);
    assert!(!json.contains("null"));
}

#[test]
fn binding_requires_the_capsule_task_exact_sha_and_a_review_branch() {
    let repo = Repo::new();
    let review = repo.review("task", "r1");
    let capsule = minimal(&repo.sha, "task");
    assert!(binding_matches(&capsule, review.handle()));
    assert!(!binding_matches(
        &minimal(&repo.sha, "other-task"),
        review.handle()
    ));
    assert!(!binding_matches(
        &minimal(&"c".repeat(40), "task"),
        review.handle()
    ));
    // A task workspace at the same SHA is not a review workspace.
    assert!(!binding_matches(&capsule, &repo.task_workspace()));
}

#[test]
fn unprotected_or_writable_review_is_refused_through_both_adapters_without_launching() {
    let repo = Repo::new();
    let review = repo.review("task", "r1");
    let capsule = RuntimeCapsuleFamily::Review(minimal(&repo.sha, "task"));
    let exe = repo.root.join("worker");
    fs::write(&exe, "#!/bin/sh\n: > launched\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    let timeout = ProcessTimeoutPolicy::new(
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(1),
    )
    .unwrap();
    for read_only in [true, false] {
        let policy = RuntimeExecutionPolicy::new(timeout, read_only);
        if let RuntimeCapsuleFamily::Review(c) = &capsule {
            assert_eq!(
                admit_review(c, review.handle(), &policy),
                Err(FailureClass::PolicyBlocked)
            );
        }
        assert!(matches!(
            ClaudeAdapter::new(&exe).start(&capsule, review.handle(), &policy),
            Err(FailureClass::PolicyBlocked)
        ));
        assert!(matches!(
            CodexAdapter::new(&exe).start(&capsule, review.handle(), &policy),
            Err(FailureClass::PolicyBlocked)
        ));
    }
    assert!(
        !review.handle().worktree_path().join("launched").exists(),
        "no ordinary fallback launched"
    );
}
