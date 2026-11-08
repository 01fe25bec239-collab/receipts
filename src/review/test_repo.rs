//! Test-only temporary Git repositories and fakes for Review controls.
//! Product code never runs Git directly; these helpers only set up fixtures.

use crate::ports::{
    ReviewRecordProposal, ReviewRecordSink, ReviewerCandidate, ReviewerRequirements,
    ReviewerRoutingPort, ReviewerRoutingRefusal, ReviewerRunOutput, ReviewerRuntime,
};
use crate::reproduction::CheckRunnerConfig;
use crate::{
    AssuranceProfile, ReviewCapsuleCheck, ReviewCapsuleCheckResult, ReviewCapsuleCriterion,
    ReviewCapsuleCriterionKind, ReviewCapsuleNonTemporalCore, ReviewCapsuleReviewScope,
    ReviewDateTimeV1, ReviewRequest, ReviewRequestNonNegativeInteger, ReviewRequestNonTemporalCore,
    ReviewRequestPolicy,
};
use receipts_runtime_adapters::FailureClass;
use receipts_workspace_execution::execution::ProcessTimeoutPolicy;
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
    WorkspaceHandle,
};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A temporary directory removed on drop.
pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    pub(crate) fn new(tag: &str) -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let base =
            std::env::temp_dir().join(format!("receipts-review-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        Self(std::fs::canonicalize(base).unwrap())
    }
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// A repository with `base` (status.txt = "base") and helpers to commit.
pub(crate) struct Repo {
    pub(crate) dir: TempDir,
    pub(crate) base: String,
}

impl Repo {
    pub(crate) fn new() -> Self {
        let dir = TempDir::new("repo");
        let root = dir.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.name", "Review Test"]);
        git(&root, &["config", "user.email", "review@test.invalid"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        let mut repo = Self {
            dir,
            base: String::new(),
        };
        repo.base = repo.commit(&[("src/status.txt", "base\n")], "base");
        repo
    }
    pub(crate) fn root(&self) -> PathBuf {
        self.dir.path().join("repo")
    }
    /// Commits files on the current branch of the main checkout.
    pub(crate) fn commit(&mut self, files: &[(&str, &str)], message: &str) -> String {
        let root = self.root();
        for (path, content) in files {
            let p = root.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", message]);
        git(&root, &["rev-parse", "HEAD"])
    }
    pub(crate) fn scratch(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

pub(crate) fn sha(s: &str) -> CommitSha {
    CommitSha::parse(s).unwrap()
}

fn first_existing(candidates: &[&str]) -> PathBuf {
    candidates
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("none of {candidates:?} exists"))
}

pub(crate) fn runner(timeout: Duration) -> CheckRunnerConfig {
    CheckRunnerConfig {
        executables: vec![
            (
                "true".into(),
                first_existing(&["/usr/bin/true", "/bin/true"]),
            ),
            (
                "false".into(),
                first_existing(&["/usr/bin/false", "/bin/false"]),
            ),
            (
                "grep".into(),
                first_existing(&["/usr/bin/grep", "/bin/grep"]),
            ),
            (
                "sleep".into(),
                first_existing(&["/bin/sleep", "/usr/bin/sleep"]),
            ),
        ],
        timeout: ProcessTimeoutPolicy::new(timeout, Duration::from_millis(500)).unwrap(),
        env: None,
    }
}

pub(crate) fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

pub(crate) fn worker_check(
    command: &[&str],
    at: &str,
    result: ReviewCapsuleCheckResult,
) -> ReviewCapsuleCheck {
    ReviewCapsuleCheck::new(
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::WorkerExecution,
            argv(command),
            0,
            sha(at),
            Some(false),
            None,
        )
        .unwrap(),
        Some(result),
    )
}

/// Criterion `c1`: status.txt contains "ok" (deterministic, reproducible).
pub(crate) fn request(
    base: &str,
    implementation: &str,
    profile: AssuranceProfile,
    policy: Option<ReviewRequestPolicy>,
) -> ReviewRequest {
    let criteria = vec![
        ReviewCapsuleCriterion::new(
            "c1".into(),
            "status says ok".into(),
            ReviewCapsuleCriterionKind::Deterministic,
            Some(argv(&["grep", "-q", "ok", "src/status.txt"])),
            None,
        )
        .unwrap(),
        ReviewCapsuleCriterion::new(
            "c2".into(),
            "reads well".into(),
            ReviewCapsuleCriterionKind::Semantic,
            None,
            None,
        )
        .unwrap(),
    ];
    ReviewRequest::new(
        ReviewRequestNonTemporalCore::new(
            "request-1".into(),
            "task-1".into(),
            "attempt-1".into(),
            implementation.into(),
            base.into(),
            "make status ok".into(),
            criteria,
            vec!["src/**".into()],
            profile,
            ReviewCapsuleReviewScope::Full,
            ReviewRequestNonNegativeInteger::from_decimal("3").unwrap(),
            Some("ws-1".into()),
            Some("receipts/task/1".into()),
            None,
            None,
            None,
            None,
            policy,
            None,
            None,
        )
        .unwrap(),
        None,
    )
}

pub(crate) fn clock() -> ReviewDateTimeV1 {
    ReviewDateTimeV1::try_new("2026-11-08T12:00:00Z").unwrap()
}

/// Routing fake: returns one fixed answer and records every request.
pub(crate) struct FixedRouting {
    pub(crate) answer: Result<ReviewerCandidate, ReviewerRoutingRefusal>,
    pub(crate) asked: RefCell<Vec<ReviewerRequirements>>,
}

impl FixedRouting {
    pub(crate) fn candidate(provider: &str, runtime: &str) -> Self {
        Self {
            answer: Ok(ReviewerCandidate {
                provider_id: provider.into(),
                runtime_id: runtime.into(),
                model_id: Some("model".into()),
                resume_session: None,
            }),
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl ReviewerRoutingPort for FixedRouting {
    fn select_reviewer(
        &self,
        _: &ReviewCapsuleNonTemporalCore,
        requirements: &ReviewerRequirements,
    ) -> Result<ReviewerCandidate, ReviewerRoutingRefusal> {
        self.asked.borrow_mut().push(requirements.clone());
        self.answer.clone()
    }
}

/// What the scripted reviewer does inside its worktree before answering.
pub(crate) enum Tamper {
    None,
    WriteFile,
    Commit,
}

/// Synthetic reviewer runtime: a scripted answer, not provider proof.
type Answer = Box<dyn Fn(&ReviewCapsuleNonTemporalCore) -> Result<ReviewerRunOutput, FailureClass>>;

pub(crate) struct ScriptedReviewer {
    pub(crate) id: String,
    pub(crate) answer: RefCell<Answer>,
    pub(crate) tamper: Tamper,
    pub(crate) calls: RefCell<Vec<(String, PathBuf, String)>>,
}

impl ScriptedReviewer {
    pub(crate) fn new(
        id: &str,
        answer: impl Fn(&ReviewCapsuleNonTemporalCore) -> Result<ReviewerRunOutput, FailureClass>
        + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            answer: RefCell::new(Box::new(answer)),
            tamper: Tamper::None,
            calls: RefCell::new(Vec::new()),
        }
    }
}

impl ReviewerRuntime for ScriptedReviewer {
    fn runtime_id(&self) -> &str {
        &self.id
    }
    fn run_review(
        &self,
        capsule: &ReviewCapsuleNonTemporalCore,
        workspace: &WorkspaceHandle,
    ) -> Result<ReviewerRunOutput, FailureClass> {
        self.calls.borrow_mut().push((
            workspace.branch().to_owned(),
            workspace.worktree_path().to_path_buf(),
            workspace
                .head_sha()
                .map(|s| s.as_str().to_owned())
                .unwrap_or_default(),
        ));
        let wt = workspace.worktree_path();
        match self.tamper {
            Tamper::None => {}
            Tamper::WriteFile => std::fs::write(wt.join("src/status.txt"), "tampered\n").unwrap(),
            Tamper::Commit => {
                std::fs::write(wt.join("src/extra.txt"), "x\n").unwrap();
                git(wt, &["add", "-A"]);
                git(
                    wt,
                    &[
                        "-c",
                        "user.name=r",
                        "-c",
                        "user.email=r@x",
                        "commit",
                        "-q",
                        "-m",
                        "r",
                    ],
                );
            }
        }
        (self.answer.borrow())(capsule)
    }
}

pub(crate) fn output(text: &str) -> ReviewerRunOutput {
    ReviewerRunOutput {
        text: Some(text.into()),
        truncated: false,
        session_ref: Some("reviewer-session".into()),
        protected: true,
        reduced_isolation: true,
    }
}

/// A4Review JSON the scripted reviewer returns for `capsule`.
pub(crate) fn verdict_json(
    capsule: &ReviewCapsuleNonTemporalCore,
    verdict: &str,
    blocking: &[&str],
) -> String {
    let findings: Vec<String> = blocking
        .iter()
        .map(|cat| {
            format!(
                r#"{{"finding_id":"f-{cat}","severity":"HIGH","category":"{cat}","description":"problem","blocking":true}}"#
            )
        })
        .collect();
    format!(
        r#"{{"review_id":"{}","task_id":"{}","reviewed_sha":"{}","independence_attested":true,"verdict":"{verdict}","blocking_findings":[{}],"nonblocking_findings":[],"dimensions":[{{"dimension":"CORRECTNESS","assessment":"SATISFIED"}}]}}"#,
        capsule.review_id(),
        capsule.task_id(),
        capsule.implementation_sha().as_str(),
        findings.join(",")
    )
}

/// Records proposals; optionally refuses.
#[derive(Default)]
pub(crate) struct MemorySink {
    pub(crate) proposals: Vec<ReviewRecordProposal>,
    pub(crate) refuse: bool,
}

impl ReviewRecordSink for MemorySink {
    fn persist(&mut self, proposal: &ReviewRecordProposal) -> Result<(), String> {
        if self.refuse {
            return Err("sink refused".into());
        }
        self.proposals.push(proposal.clone());
        Ok(())
    }
}

pub(crate) fn time_limit() -> Duration {
    Duration::from_secs(20)
}
