//! Shared fixtures for the Review→Core→State controls: real temporary SQLite
//! stores, the real `CoreStateWriter` behind `CoreReviewSink`, a synthetic
//! trusted clock (State stamps everything with it) and a scripted reviewer
//! (a synthetic runtime, not provider proof).

#![allow(dead_code)]

use receipts_e2e::CoreReviewSink;
use receipts_orchestration::orchestration::{CoreAdmissionIdentity, CoreStateWriter};
use receipts_review_integration::runtime::FailureClass;
use receipts_review_integration::state::{
    DecisionRecordKind, EventQuery, EventType, GraphCompilerRefV1, GraphSourceEvidenceRefV1,
    ReviewClaimRecordV1, ReviewSealRecordV1, StateError, StateReader, TrustedClockV1,
    TrustedTimeSampleV1,
};
use receipts_review_integration::{
    AssuranceProfile, AttemptFinished, CheckRunnerConfig, ImplementerIdentity,
    ReviewCapsuleCriterion, ReviewCapsuleCriterionKind, ReviewCapsuleNonTemporalCore,
    ReviewCapsuleReviewScope, ReviewClaim, ReviewClaimBinding, ReviewController, ReviewDateTimeV1,
    ReviewRecordProposal, ReviewRecordSink, ReviewRequest, ReviewRequestNonNegativeInteger,
    ReviewRequestNonTemporalCore, ReviewRequestReviewerFloor, ReviewTicket, ReviewerCandidate,
    ReviewerQualityEvidence, ReviewerRequirements, ReviewerRoutingPort, ReviewerRoutingRefusal,
    ReviewerRunOutput, ReviewerRuntime, SealedReview,
};
use receipts_workspace_execution::WorkspaceHandle;
use receipts_workspace_execution::execution::ProcessTimeoutPolicy;
use std::cell::Cell;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const PROJECT: &str = "project";
pub const AUTHORITY: &str = "operator:alice";
pub const STAMP: &str = "2026-11-08T10:00:00.000000000Z";

/// The trusted clock. Raising `fail` makes every sample fail, which is the
/// fault injected into State's own write path (claim, seal or publication).
#[derive(Clone, Default)]
pub struct Clock {
    pub fail: Arc<AtomicBool>,
}

impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(StateError::InternalQueryFailed {
                detail: "injected clock fault".into(),
            });
        }
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: STAMP.into(),
            clock_source_id: "synthetic-clock".into(),
            clock_contract_version: "1".into(),
        })
    }
}

pub fn identity() -> CoreAdmissionIdentity {
    CoreAdmissionIdentity::new(
        GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        "policy".into(),
    )
    .unwrap()
    .with_project(
        PROJECT,
        GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
    )
    .with_abandonment_authority(PROJECT, AUTHORITY)
}

pub fn open_sink(db: &Path, clock: &Clock) -> CoreReviewSink {
    CoreReviewSink::new(
        CoreStateWriter::open(db, identity()).unwrap(),
        clock.clone(),
    )
}

pub fn git(dir: &Path, args: &[&str]) -> String {
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

pub struct Dir(pub PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn now() -> ReviewDateTimeV1 {
    ReviewDateTimeV1::try_new("2026-11-08T00:00:00Z").unwrap()
}

pub struct Fixture {
    pub dir: Dir,
    pub root: PathBuf,
    pub db: PathBuf,
    pub implementation: String,
    pub request: ReviewRequest,
}

pub fn fixture(tag: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("receipts-e2e-core-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .recursive(true)
        .create(dir.join("state"))
        .unwrap();
    let dir = Dir(std::fs::canonicalize(dir).unwrap());
    let root = dir.0.join("repo");
    std::fs::create_dir_all(root.join("src")).unwrap();
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
    let db = dir.0.join("state/state.db");
    let request = request_with(&base, &implementation, "rq", "o");
    Fixture {
        dir,
        root,
        db,
        implementation,
        request,
    }
}

/// A valid request; `objective` is part of its semantics.
pub fn request_with(base: &str, implementation: &str, id: &str, objective: &str) -> ReviewRequest {
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
            id.into(),
            "task".into(),
            "attempt".into(),
            implementation.into(),
            base.into(),
            objective.into(),
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

pub const ALL_DIMENSIONS: &str = r#"{"dimension":"SCOPE_COMPLIANCE","assessment":"SATISFIED"},{"dimension":"CONTRACT_COMPLIANCE","assessment":"SATISFIED"},{"dimension":"ARCHITECTURE_COMPLIANCE","assessment":"SATISFIED"},{"dimension":"CORRECTNESS","assessment":"SATISFIED"},{"dimension":"ERROR_HANDLING","assessment":"SATISFIED"},{"dimension":"SECURITY_TRUST_BOUNDARIES","assessment":"SATISFIED"},{"dimension":"TEST_ADEQUACY","assessment":"SATISFIED"},{"dimension":"NEGATIVE_TESTS","assessment":"SATISFIED"},{"dimension":"REGRESSION_RISK","assessment":"SATISFIED"},{"dimension":"WRITE_SCOPE","assessment":"SATISFIED"},{"dimension":"UNDISCLOSED_CHANGES","assessment":"SATISFIED"},{"dimension":"EVIDENCE_ACCURACY","assessment":"SATISFIED"}"#;

pub struct Pick(pub Result<(), ReviewerRoutingRefusal>);
impl Default for Pick {
    fn default() -> Self {
        Self(Ok(()))
    }
}
impl ReviewerRoutingPort for Pick {
    fn select_reviewer(
        &self,
        _: &ReviewCapsuleNonTemporalCore,
        _: &ReviewerRequirements,
    ) -> Result<ReviewerCandidate, ReviewerRoutingRefusal> {
        self.0?;
        Ok(ReviewerCandidate {
            provider_id: "provider-b".into(),
            runtime_id: "synthetic".into(),
            model_id: None,
            resume_session: None,
            quality: Some(ReviewerQualityEvidence {
                tier: ReviewRequestReviewerFloor::Frontier,
                basis_ref: "registry:synthetic-frontier".into(),
            }),
        })
    }
}

/// Synthetic reviewer (not provider proof): PASS, or a blocking REJECT.
#[derive(Default)]
pub struct Synthetic {
    pub calls: Cell<usize>,
    pub reject: bool,
    pub policy_blocked: bool,
    /// PASS_WITH_NONBLOCKING_FINDINGS with one nonblocking style finding `n1`.
    pub nonblocking: bool,
}
impl Synthetic {
    pub fn rejecting() -> Self {
        Self {
            reject: true,
            ..Self::default()
        }
    }
}
impl ReviewerRuntime for Synthetic {
    fn runtime_id(&self) -> &str {
        "synthetic"
    }
    fn run_review(
        &self,
        c: &ReviewCapsuleNonTemporalCore,
        _: &WorkspaceHandle,
    ) -> Result<ReviewerRunOutput, FailureClass> {
        self.calls.set(self.calls.get() + 1);
        if self.policy_blocked {
            return Err(FailureClass::PolicyBlocked);
        }
        let (verdict, blocking, nonblocking) = if self.reject {
            (
                "REJECT",
                r#"{"finding_id":"f1","severity":"HIGH","category":"CORRECTNESS","description":"wrong","blocking":true}"#,
                "",
            )
        } else if self.nonblocking {
            (
                "PASS_WITH_NONBLOCKING_FINDINGS",
                "",
                r#"{"finding_id":"n1","severity":"LOW","category":"STYLE","description":"nit","blocking":false}"#,
            )
        } else {
            ("PASS", "", "")
        };
        Ok(ReviewerRunOutput {
            text: Some(format!(
                r#"{{"review_id":"{}","task_id":"{}","reviewed_sha":"{}","independence_attested":true,"verdict":"{verdict}","blocking_findings":[{blocking}],"nonblocking_findings":[{nonblocking}],"dimensions":[{}]}}"#,
                c.review_id(),
                c.task_id(),
                c.implementation_sha().as_str(),
                ALL_DIMENSIONS
            )),
            truncated: false,
            session_ref: Some("s".into()),
            protected: true,
            reduced_isolation: true,
        })
    }
}

pub fn implementer() -> ImplementerIdentity {
    ImplementerIdentity {
        provider_id: "provider-a".into(),
        runtime_id: "x".into(),
        model_id: None,
        session_ref: None,
    }
}

pub fn run_attempt(
    f: &Fixture,
    sink: &mut dyn ReviewRecordSink,
    runtime: &dyn ReviewerRuntime,
    generation: u32,
) -> ReviewTicket {
    run_attempt_in(
        f,
        sink,
        runtime,
        &Pick::default(),
        &f.dir.0.join("review-wt"),
        generation,
    )
}

pub fn run_attempt_in(
    f: &Fixture,
    sink: &mut dyn ReviewRecordSink,
    runtime: &dyn ReviewerRuntime,
    routing: &Pick,
    wt: &Path,
    generation: u32,
) -> ReviewTicket {
    let checks = CheckRunnerConfig {
        executables: vec![],
        timeout: ProcessTimeoutPolicy::new(Duration::from_secs(5), Duration::from_millis(200))
            .unwrap(),
        env: None,
    };
    let controller = ReviewController {
        routing,
        runtimes: &[runtime],
        checks: &checks,
        clock: &now,
    };
    controller.on_attempt_finished(
        AttemptFinished {
            project_id: PROJECT,
            request: &f.request,
            repository_root: &f.root,
            implementer: &implementer(),
            worker_checks: &[],
            review_worktree: wt,
            generation,
        },
        sink,
    )
}

pub fn query() -> EventQuery {
    EventQuery {
        project_id: PROJECT.into(),
        subject: None,
        event_types: None,
        limit: 100,
    }
}

/// Event types in a fixed order. State lists a publication's events by their
/// hash-derived identity, which says nothing about publication order, so
/// controls compare the set.
pub fn sorted(mut types: Vec<EventType>) -> Vec<EventType> {
    types.sort_by_key(|t| t.as_str());
    types
}

pub fn stored_event_types(db: &Path) -> Vec<EventType> {
    sorted(
        StateReader::open_existing(db)
            .unwrap()
            .list_events(&query())
            .unwrap()
            .iter()
            .map(|e| e.event_type)
            .collect(),
    )
}

pub fn expected<const N: usize>(types: [EventType; N]) -> Vec<EventType> {
    sorted(types.to_vec())
}

/// Everything a review claim may touch, as the live reader sees it.
#[derive(Debug, PartialEq)]
pub struct Image {
    pub watermark: String,
    pub event_ids: Vec<String>,
    pub decision_sha: Option<String>,
    pub claim: Option<ReviewClaimRecordV1>,
    pub seal: Option<ReviewSealRecordV1>,
}

pub fn image(sink: &CoreReviewSink, binding: &ReviewClaimBinding, review_id: &str) -> Image {
    let reader = sink.writer().reader();
    let claim_id = CoreReviewSink::claim_id(binding).unwrap();
    Image {
        watermark: format!("{:?}", reader.find_trusted_time_watermark(PROJECT).unwrap()),
        event_ids: reader
            .list_events(&query())
            .unwrap()
            .into_iter()
            .map(|e| e.event_id)
            .collect(),
        decision_sha: reader
            .find_decision_record(PROJECT, DecisionRecordKind::A4Review, review_id)
            .unwrap()
            .map(|r| r.record_sha256().to_owned()),
        claim: reader.find_review_claim(PROJECT, &claim_id).unwrap(),
        seal: reader.find_review_seal(PROJECT, &claim_id).unwrap(),
    }
}

pub fn binding(f: &Fixture, generation: u32) -> ReviewClaimBinding {
    receipts_review_integration::review_claim_binding(&AttemptFinished {
        project_id: PROJECT,
        request: &f.request,
        repository_root: &f.root,
        implementer: &implementer(),
        worker_checks: &[],
        review_worktree: &f.dir.0.join("unused"),
        generation,
    })
    .unwrap()
}

/// One injected fault, consumed by the next matching sink call.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Fault {
    #[default]
    None,
    /// State's clock fails while the core claims.
    ClaimClock,
    /// The seal never reaches the core.
    SealLost,
    /// State's clock fails while the core seals.
    SealClock,
    /// The publication never reaches the core.
    PublishLost,
    /// State's clock fails while the core publishes.
    PublishClock,
}

pub struct Faulty {
    pub inner: CoreReviewSink,
    pub clock: Clock,
    pub fault: Fault,
}

impl Faulty {
    pub fn open(db: &Path, clock: &Clock) -> Self {
        Self {
            inner: open_sink(db, clock),
            clock: clock.clone(),
            fault: Fault::None,
        }
    }
    fn hit(&mut self, clock_fault: Fault, lost: Option<Fault>) -> Result<bool, String> {
        if lost == Some(self.fault) {
            self.fault = Fault::None;
            return Err("injected loss before the core".into());
        }
        let hit = self.fault == clock_fault;
        if hit {
            self.fault = Fault::None;
            self.clock.fail.store(true, Ordering::SeqCst);
        }
        Ok(hit)
    }
}

impl ReviewRecordSink for Faulty {
    fn claim(&mut self, b: &ReviewClaimBinding) -> Result<ReviewClaim, String> {
        let armed = self.hit(Fault::ClaimClock, None)?;
        let result = self.inner.claim(b);
        if armed {
            self.clock.fail.store(false, Ordering::SeqCst);
        }
        result
    }
    fn seal(&mut self, b: &ReviewClaimBinding, s: &SealedReview) -> Result<(), String> {
        let armed = self.hit(Fault::SealClock, Some(Fault::SealLost))?;
        let result = self.inner.seal(b, s);
        if armed {
            self.clock.fail.store(false, Ordering::SeqCst);
        }
        result
    }
    fn publish(&mut self, b: &ReviewClaimBinding, p: &ReviewRecordProposal) -> Result<(), String> {
        let armed = self.hit(Fault::PublishClock, Some(Fault::PublishLost))?;
        let result = self.inner.publish(b, p);
        if armed {
            self.clock.fail.store(false, Ordering::SeqCst);
        }
        result
    }
}
