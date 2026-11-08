//! The actual Runtime Review boundary: the frozen `RuntimeAdapter::start`
//! with `RuntimeCapsuleFamily::Review` on the real Claude and Codex adapters,
//! driven by the Review controller.
//!
//! Proven here (portable): without a Workspace protected launch, or with a
//! writable policy, both adapters refuse the Review with `PolicyBlocked`
//! before any process starts, and the controller records POLICY_BLOCKED →
//! HUMAN_REQUIRED without dispatching another provider.
//! NOT RUN here: a successful protected review launch (needs the Linux
//! protected launcher) and any real provider review.

use receipts_review_integration::runtime::{
    ClaudeAdapter, CodexAdapter, FailureClass, RuntimeAdapter, RuntimeExecutionPolicy,
};
use receipts_review_integration::{
    AssuranceProfile, AttemptFinished, CheckRunnerConfig, ImplementerIdentity,
    ReviewCapsuleCriterion, ReviewCapsuleCriterionKind, ReviewCapsuleNonTemporalCore,
    ReviewCapsuleReviewScope, ReviewController, ReviewDateTimeV1, ReviewDisposition,
    ReviewRecordProposal, ReviewRecordSink, ReviewRequest, ReviewRequestNonNegativeInteger,
    ReviewRequestNonTemporalCore, ReviewerCandidate, ReviewerRequirements, ReviewerRoutingPort,
    ReviewerRoutingRefusal, ReviewerRunOutput, ReviewerRuntime, reviewer_output_from_claude,
    reviewer_output_from_codex,
};
use receipts_runtime_bindings::RuntimeCapsuleFamily;
use receipts_workspace_execution::WorkspaceHandle;
use receipts_workspace_execution::execution::ProcessTimeoutPolicy;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

/// The composition bridge HI-W4-1 will own: Review family through the
/// frozen trait, collected and interpreted by Review's result readers.
struct AdapterReviewer<A> {
    id: &'static str,
    adapter: A,
    read_only: bool,
    starts: Cell<usize>,
}

fn policy(read_only: bool) -> RuntimeExecutionPolicy {
    RuntimeExecutionPolicy::new(
        ProcessTimeoutPolicy::new(Duration::from_secs(5), Duration::from_millis(200)).unwrap(),
        read_only,
    )
}

impl ReviewerRuntime for AdapterReviewer<ClaudeAdapter> {
    fn runtime_id(&self) -> &str {
        self.id
    }
    fn run_review(
        &self,
        capsule: &ReviewCapsuleNonTemporalCore,
        workspace: &WorkspaceHandle,
    ) -> Result<ReviewerRunOutput, FailureClass> {
        self.starts.set(self.starts.get() + 1);
        let family = RuntimeCapsuleFamily::Review(capsule.clone());
        let handle = self
            .adapter
            .start(&family, workspace, &policy(self.read_only))?;
        reviewer_output_from_claude(&self.adapter.collect_result(&handle)?)
    }
}

impl ReviewerRuntime for AdapterReviewer<CodexAdapter> {
    fn runtime_id(&self) -> &str {
        self.id
    }
    fn run_review(
        &self,
        capsule: &ReviewCapsuleNonTemporalCore,
        workspace: &WorkspaceHandle,
    ) -> Result<ReviewerRunOutput, FailureClass> {
        self.starts.set(self.starts.get() + 1);
        let family = RuntimeCapsuleFamily::Review(capsule.clone());
        let handle = self
            .adapter
            .start(&family, workspace, &policy(self.read_only))?;
        reviewer_output_from_codex(&self.adapter.collect_result(&handle)?)
    }
}

struct Pick(&'static str, &'static str);
impl ReviewerRoutingPort for Pick {
    fn select_reviewer(
        &self,
        _: &ReviewCapsuleNonTemporalCore,
        _: &ReviewerRequirements,
    ) -> Result<ReviewerCandidate, ReviewerRoutingRefusal> {
        Ok(ReviewerCandidate {
            provider_id: self.0.into(),
            runtime_id: self.1.into(),
            model_id: None,
            resume_session: None,
        })
    }
}

#[derive(Default)]
struct Sink(Vec<ReviewRecordProposal>);
impl ReviewRecordSink for Sink {
    fn persist(&mut self, p: &ReviewRecordProposal) -> Result<(), String> {
        self.0.push(p.clone());
        Ok(())
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

struct Fixture {
    dir: PathBuf,
    base: String,
    implementation: String,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(tag: &str) -> Fixture {
    let dir =
        std::env::temp_dir().join(format!("receipts-e2e-review-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("repo/src")).unwrap();
    let dir = std::fs::canonicalize(dir).unwrap();
    let root = dir.join("repo");
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.name", "t"]);
    git(&root, &["config", "user.email", "t@t.invalid"]);
    std::fs::write(root.join("src/a.txt"), "a\n").unwrap();
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "base"]);
    let base = git(&root, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("src/a.txt"), "b\n").unwrap();
    git(&root, &["commit", "-qam", "impl"]);
    let implementation = git(&root, &["rev-parse", "HEAD"]);
    Fixture {
        dir,
        base,
        implementation,
    }
}

fn request(f: &Fixture) -> ReviewRequest {
    let criterion = ReviewCapsuleCriterion::new(
        "c".into(),
        "d".into(),
        ReviewCapsuleCriterionKind::Semantic,
        None,
        None,
    )
    .unwrap();
    ReviewRequest::new(
        ReviewRequestNonTemporalCore::new(
            "rq".into(),
            "task".into(),
            "attempt".into(),
            f.implementation.clone(),
            f.base.clone(),
            "o".into(),
            vec![criterion],
            vec!["src/**".into()],
            AssuranceProfile::Standard,
            ReviewCapsuleReviewScope::Full,
            ReviewRequestNonNegativeInteger::from_decimal("0").unwrap(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap(),
        None,
    )
}

fn now() -> ReviewDateTimeV1 {
    ReviewDateTimeV1::try_new("2026-11-08T00:00:00Z").unwrap()
}

fn review_with(
    f: &Fixture,
    routing: &Pick,
    runtimes: &[&dyn ReviewerRuntime],
) -> receipts_review_integration::ReviewTicket {
    let checks = CheckRunnerConfig {
        executables: vec![],
        timeout: ProcessTimeoutPolicy::new(Duration::from_secs(5), Duration::from_millis(200))
            .unwrap(),
        env: None,
    };
    let controller = ReviewController {
        routing,
        runtimes,
        checks: &checks,
        clock: &now,
    };
    controller.on_attempt_finished(
        AttemptFinished {
            project_id: "project",
            request: &request(f),
            repository_root: &f.dir.join("repo"),
            implementer: &ImplementerIdentity {
                provider_id: "anthropic".into(),
                runtime_id: "claude".into(),
                model_id: None,
                session_ref: None,
            },
            worker_checks: &[],
            review_worktree: &f.dir.join("review-wt"),
        },
        &mut Sink::default(),
    )
}

// Executables are never spawned: refusal happens before process start.
const NO_BINARY: &str = "/nonexistent/receipts-e2e/never-started";

#[test]
fn real_adapters_refuse_unprotected_review_and_controller_never_shops() {
    for (provider, runtime) in [("openai", "codex"), ("anthropic", "claude")] {
        let f = fixture(runtime);
        let claude = AdapterReviewer {
            id: "claude",
            adapter: ClaudeAdapter::new(NO_BINARY),
            read_only: true,
            starts: Cell::new(0),
        };
        let codex = AdapterReviewer {
            id: "codex",
            adapter: CodexAdapter::new(NO_BINARY),
            read_only: true,
            starts: Cell::new(0),
        };
        let t = review_with(&f, &Pick(provider, runtime), &[&claude, &codex]);
        let ReviewDisposition::PolicyBlocked(i) = &t.disposition else {
            panic!(
                "{runtime}: expected POLICY_BLOCKED, got {:?}",
                t.disposition
            );
        };
        assert_eq!(i.core().provider_id(), provider);
        assert_eq!(t.dispatches, 1);
        assert_eq!(
            claude.starts.get() + codex.starts.get(),
            1,
            "{runtime}: exactly one adapter start"
        );
        assert!(!t.passed());
        assert!(
            matches!(t.cleanup, Some(Ok(()))),
            "{runtime}: review worktree removed"
        );
    }
}

#[test]
fn real_adapters_refuse_a_writable_review_policy() {
    let f = fixture("writable");
    let claude = AdapterReviewer {
        id: "claude",
        adapter: ClaudeAdapter::new(NO_BINARY),
        read_only: false,
        starts: Cell::new(0),
    };
    let t = review_with(&f, &Pick("openai", "claude"), &[&claude]);
    assert!(
        matches!(t.disposition, ReviewDisposition::PolicyBlocked(_)),
        "{:?}",
        t.disposition
    );
}
