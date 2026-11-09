//! Test-only temporary Git repositories and fakes for Review controls.
//! Product code never runs Git directly; these helpers only set up fixtures.

use crate::ports::{
    ReviewClaim, ReviewRecordProposal, ReviewRecordSink, ReviewerCandidate,
    ReviewerQualityEvidence, ReviewerRequirements, ReviewerRoutingPort, ReviewerRoutingRefusal,
    ReviewerRunOutput, ReviewerRuntime, SealedReview,
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
use std::collections::{BTreeMap, BTreeSet};
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
                quality: Some(ReviewerQualityEvidence {
                    tier: crate::ReviewRequestReviewerFloor::Frontier,
                    basis_ref: "registry:test-frontier".into(),
                }),
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
        r#"{{"review_id":"{}","task_id":"{}","reviewed_sha":"{}","independence_attested":true,"verdict":"{verdict}","blocking_findings":[{}],"nonblocking_findings":[],"dimensions":[{}]}}"#,
        capsule.review_id(),
        capsule.task_id(),
        capsule.implementation_sha().as_str(),
        findings.join(","),
        all_dimensions_json()
    )
}

/// Every audit dimension assessed, as the frozen lifecycle requires.
pub(crate) fn all_dimensions_json() -> String {
    crate::A4ReviewDimension::ALL
        .iter()
        .map(|d| {
            format!(
                r#"{{"dimension":"{}","assessment":"SATISFIED"}}"#,
                d.as_str()
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Test fake of a conforming sink. It is in-memory, so it proves controller
/// behaviour against the sink contract; durability across restart is proven
/// in `receipts-e2e` against a file journal and the real Core writer.
#[derive(Default)]
pub(crate) struct MemorySink {
    /// Published packages, once each (publication is idempotent by claim key).
    pub(crate) proposals: Vec<ReviewRecordProposal>,
    pub(crate) claims: BTreeMap<String, Option<SealedReview>>,
    pub(crate) published: BTreeSet<String>,
    /// `claim` fails: no execution may follow.
    pub(crate) refuse: bool,
    pub(crate) fail_seal: bool,
    /// Remaining `publish` calls that fail before succeeding.
    pub(crate) fail_publish: usize,
}

impl ReviewRecordSink for MemorySink {
    fn claim(&mut self, key: &str) -> Result<ReviewClaim, String> {
        if self.refuse {
            return Err("sink refused".into());
        }
        Ok(match self.claims.get(key) {
            None => {
                self.claims.insert(key.to_owned(), None);
                ReviewClaim::Acquired
            }
            Some(None) => ReviewClaim::InFlight,
            Some(Some(sealed)) => ReviewClaim::Sealed(sealed.clone()),
        })
    }
    fn seal(&mut self, key: &str, sealed: &SealedReview) -> Result<(), String> {
        if self.fail_seal {
            return Err("seal failed".into());
        }
        match self.claims.get_mut(key) {
            Some(slot @ None) => {
                *slot = Some(sealed.clone());
                Ok(())
            }
            Some(Some(existing)) if existing.to_bytes() == sealed.to_bytes() => Ok(()),
            Some(Some(_)) => Err("a different package is already sealed".into()),
            None => Err("no claim".into()),
        }
    }
    fn publish(&mut self, key: &str, proposal: &ReviewRecordProposal) -> Result<(), String> {
        if self.fail_publish > 0 {
            self.fail_publish -= 1;
            return Err("publish failed".into());
        }
        if self.published.insert(key.to_owned()) {
            self.proposals.push(proposal.clone());
        }
        Ok(())
    }
}

/// Persists review packages into a real State store (trusted-core store,
/// trusted time sample) and returns a read-only view.
pub(crate) fn state_with_packages(
    dir: &TempDir,
    packages: &[&ReviewRecordProposal],
    extra: impl FnOnce(&mut receipts_state::TrustedCoreStore),
) -> receipts_state::StateReader {
    use receipts_state::{
        StateError, StateReader, StateWriteOperationV1, StateWriteSubjectV1, TrustedClockV1,
        TrustedCoreStore, TrustedTimeSampleV1,
    };
    use std::os::unix::fs::PermissionsExt;
    struct Clock;
    impl TrustedClockV1 for Clock {
        fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
            Ok(TrustedTimeSampleV1 {
                canonical_utc_timestamp: "2026-11-08T10:00:00.000000000Z".into(),
                clock_source_id: "synthetic-clock".into(),
                clock_contract_version: "1".into(),
            })
        }
    }
    let state_dir = dir.path().join("state");
    std::fs::create_dir(&state_dir).unwrap();
    std::fs::set_permissions(&state_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let db = state_dir.join("state.db");
    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    let mut n = 0u32;
    for package in packages {
        for record in &package.decision_records {
            let admission = store.admit(StateWriteSubjectV1 {
                operation: StateWriteOperationV1::DecisionRecord,
                project_id: package.project_id.clone(),
                graph_id: None,
                source_ref: None,
                compiler_id: None,
                policy_id: None,
            });
            store.append_decision_record(admission, record).unwrap();
        }
        for event in &package.events {
            n += 1;
            let admission = store.admit(StateWriteSubjectV1 {
                operation: StateWriteOperationV1::EventProposal,
                project_id: package.project_id.clone(),
                graph_id: None,
                source_ref: None,
                compiler_id: None,
                policy_id: None,
            });
            store
                .persist_event_proposal(
                    admission,
                    &Clock,
                    event,
                    &format!("01ARZ3NDEKTSV4RRFFQ69G{n:04}"),
                )
                .unwrap();
        }
    }
    extra(&mut store);
    drop(store);
    StateReader::open_existing(&db).unwrap()
}

pub(crate) fn time_limit() -> Duration {
    Duration::from_secs(20)
}
