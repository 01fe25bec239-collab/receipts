//! Review records persist only through the actual trusted-core writer.
//!
//! `ReviewRecordSink` is bound to Orchestration's real `CoreStateWriter`
//! (exclusive TrustedCoreStore, admitted project, content resolver that
//! verifies each payload digest). The Review controller runs against a real
//! repository with a synthetic reviewer; its A4Review decision record and
//! REVIEW_* events land in State, read back through the read-only
//! `StateReader`, and a replay is refused atomically.

mod common;

use common::{FileJournal, event_id_for};
use receipts_orchestration::orchestration::{
    CoreAdmissionIdentity, CoreContentResolverV1, CoreStateWriter,
};
use receipts_review_integration::runtime::FailureClass;
use receipts_review_integration::state::{
    DecisionRecordKind, EventQuery, EventType, GraphCompilerRefV1, GraphSourceEvidenceRefV1,
    StateError, StateReader, TrustedClockV1, TrustedTimeSampleV1,
};
use receipts_review_integration::{
    AssuranceProfile, AttemptFinished, CheckRunnerConfig, ImplementerIdentity,
    ReviewCapsuleCriterion, ReviewCapsuleCriterionKind, ReviewCapsuleNonTemporalCore,
    ReviewCapsuleReviewScope, ReviewClaim, ReviewController, ReviewDateTimeV1,
    ReviewRecordProposal, ReviewRecordSink, ReviewRequest, ReviewRequestNonNegativeInteger,
    ReviewRequestNonTemporalCore, ReviewRequestReviewerFloor, ReviewerCandidate,
    ReviewerQualityEvidence, ReviewerRequirements, ReviewerRoutingPort, ReviewerRoutingRefusal,
    ReviewerRunOutput, ReviewerRuntime, SealedReview,
};
use receipts_workspace_execution::WorkspaceHandle;
use receipts_workspace_execution::execution::ProcessTimeoutPolicy;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

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

struct Resolver<'a>(&'a ReviewRecordProposal);
impl CoreContentResolverV1 for Resolver<'_> {
    fn resolve(&self, project_id: &str, reference: &str) -> Option<Vec<u8>> {
        (project_id == self.0.project_id)
            .then(|| {
                self.0
                    .content
                    .iter()
                    .find(|(r, _)| r == reference)
                    .map(|(_, b)| b.clone())
            })
            .flatten()
    }
}

/// The trusted composition's sink: claims and sealed results live in a
/// durable journal; records and events go through the real core writer,
/// idempotently (an identical stored record or event counts as done, a
/// conflicting one is an error, nothing is ever replaced).
struct CoreSink {
    writer: CoreStateWriter,
    journal: FileJournal,
    /// Fault injection: fail the publish write with this ordinal
    /// (0 = the decision record, then each event in order).
    fail_at: Option<u32>,
    /// Fault injection: fail sealing.
    fail_seal: bool,
}

impl CoreSink {
    fn open(f: &Fixture) -> Self {
        Self {
            writer: open_writer(&f.db),
            journal: FileJournal::new(&f.dir.0.join("journal")),
            fail_at: None,
            fail_seal: false,
        }
    }
}

impl ReviewRecordSink for CoreSink {
    fn claim(&mut self, key: &str) -> Result<ReviewClaim, String> {
        self.journal.claim(key)
    }
    fn seal(&mut self, key: &str, sealed: &SealedReview) -> Result<(), String> {
        if self.fail_seal {
            return Err("injected seal failure".into());
        }
        self.journal.seal(key, sealed)
    }
    fn publish(&mut self, key: &str, p: &ReviewRecordProposal) -> Result<(), String> {
        let claim_ms = self.journal.claim_time_ms(key);
        let mut ordinal = 0u32;
        let mut boundary = |fail_at: Option<u32>| -> Result<(), String> {
            let hit = fail_at == Some(ordinal);
            ordinal += 1;
            if hit {
                Err("injected failure".into())
            } else {
                Ok(())
            }
        };
        for record in &p.decision_records {
            boundary(self.fail_at)?;
            let stored = self
                .writer
                .reader()
                .find_decision_record(record.project_id(), record.kind(), record.record_id())
                .map_err(|e| e.to_string())?;
            match stored {
                Some(existing) if existing.record_json() == record.record_json() => {}
                Some(_) => return Err("a different record is already stored".into()),
                None => self
                    .writer
                    .append_decision_record(record)
                    .map_err(|e| e.to_string())?,
            }
        }
        let existing: Vec<String> = self
            .writer
            .reader()
            .list_events(&EventQuery {
                project_id: p.project_id.clone(),
                subject: None,
                event_types: None,
                limit: 1000,
            })
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|e| e.event_id)
            .collect();
        for (ordinal, (event, event_key)) in p.events.iter().zip(&p.event_keys).enumerate() {
            boundary(self.fail_at)?;
            let id = event_id_for(claim_ms, ordinal as u16, event_key);
            if existing.contains(&id) {
                continue;
            }
            self.writer
                .persist_event_proposal(&Clock, &Resolver(p), event, &id)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

const ALL_DIMENSIONS: &str = r#"{"dimension":"SCOPE_COMPLIANCE","assessment":"SATISFIED"},{"dimension":"CONTRACT_COMPLIANCE","assessment":"SATISFIED"},{"dimension":"ARCHITECTURE_COMPLIANCE","assessment":"SATISFIED"},{"dimension":"CORRECTNESS","assessment":"SATISFIED"},{"dimension":"ERROR_HANDLING","assessment":"SATISFIED"},{"dimension":"SECURITY_TRUST_BOUNDARIES","assessment":"SATISFIED"},{"dimension":"TEST_ADEQUACY","assessment":"SATISFIED"},{"dimension":"NEGATIVE_TESTS","assessment":"SATISFIED"},{"dimension":"REGRESSION_RISK","assessment":"SATISFIED"},{"dimension":"WRITE_SCOPE","assessment":"SATISFIED"},{"dimension":"UNDISCLOSED_CHANGES","assessment":"SATISFIED"},{"dimension":"EVIDENCE_ACCURACY","assessment":"SATISFIED"}"#;

struct Pick;
impl ReviewerRoutingPort for Pick {
    fn select_reviewer(
        &self,
        _: &ReviewCapsuleNonTemporalCore,
        _: &ReviewerRequirements,
    ) -> Result<ReviewerCandidate, ReviewerRoutingRefusal> {
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

/// Synthetic reviewer (not provider proof): returns a PASS for the capsule.
#[derive(Default)]
struct Synthetic {
    calls: std::cell::Cell<usize>,
    reject: bool,
}
impl Synthetic {
    fn rejecting() -> Self {
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
        let (verdict, blocking) = if self.reject {
            (
                "REJECT",
                r#"{"finding_id":"f1","severity":"HIGH","category":"CORRECTNESS","description":"wrong","blocking":true}"#,
            )
        } else {
            ("PASS", "")
        };
        Ok(ReviewerRunOutput {
            text: Some(format!(
                r#"{{"review_id":"{}","task_id":"{}","reviewed_sha":"{}","independence_attested":true,"verdict":"{verdict}","blocking_findings":[{blocking}],"nonblocking_findings":[],"dimensions":[{}]}}"#,
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

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn now() -> ReviewDateTimeV1 {
    ReviewDateTimeV1::try_new("2026-11-08T00:00:00Z").unwrap()
}

struct Fixture {
    dir: Dir,
    root: PathBuf,
    db: PathBuf,
    implementation: String,
    request: ReviewRequest,
}

fn fixture(tag: &str) -> Fixture {
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
    let criterion = ReviewCapsuleCriterion::new(
        "c".into(),
        "d".into(),
        ReviewCapsuleCriterionKind::Semantic,
        None,
        None,
    )
    .unwrap();
    let request = ReviewRequest::new(
        ReviewRequestNonTemporalCore::new(
            "rq".into(),
            "task".into(),
            "attempt".into(),
            implementation.clone(),
            base,
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
    );
    Fixture {
        dir,
        root,
        db,
        implementation,
        request,
    }
}

fn open_writer(db: &Path) -> CoreStateWriter {
    let identity = CoreAdmissionIdentity::new(
        GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        "policy".into(),
    )
    .unwrap()
    .with_project(
        "project",
        GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
    );
    CoreStateWriter::open(db, identity).unwrap()
}

fn run_attempt(
    f: &Fixture,
    sink: &mut dyn ReviewRecordSink,
    runtime: &dyn ReviewerRuntime,
) -> receipts_review_integration::ReviewTicket {
    run_attempt_in(f, sink, runtime, &f.dir.0.join("review-wt"))
}

fn run_attempt_in(
    f: &Fixture,
    sink: &mut dyn ReviewRecordSink,
    runtime: &dyn ReviewerRuntime,
    wt: &Path,
) -> receipts_review_integration::ReviewTicket {
    let checks = CheckRunnerConfig {
        executables: vec![],
        timeout: ProcessTimeoutPolicy::new(Duration::from_secs(5), Duration::from_millis(200))
            .unwrap(),
        env: None,
    };
    let controller = ReviewController {
        routing: &Pick,
        runtimes: &[runtime],
        checks: &checks,
        clock: &now,
    };
    let implementer = ImplementerIdentity {
        provider_id: "provider-a".into(),
        runtime_id: "x".into(),
        model_id: None,
        session_ref: None,
    };
    controller.on_attempt_finished(
        AttemptFinished {
            project_id: "project",
            request: &f.request,
            repository_root: &f.root,
            implementer: &implementer,
            worker_checks: &[],
            review_worktree: wt,
        },
        sink,
    )
}

fn stored_event_types(db: &Path) -> Vec<EventType> {
    StateReader::open_existing(db)
        .unwrap()
        .list_events(&EventQuery {
            project_id: "project".into(),
            subject: None,
            event_types: None,
            limit: 100,
        })
        .unwrap()
        .iter()
        .map(|e| e.event_type)
        .collect()
}

#[test]
fn review_records_persist_through_the_core_writer_and_replay_is_refused() {
    let f = fixture("ok");
    let mut sink = CoreSink::open(&f);
    let reviewer = Synthetic::default();
    let ticket = run_attempt(&f, &mut sink, &reviewer);
    assert!(
        ticket.passed(),
        "{:?} {:?}",
        ticket.disposition,
        ticket.persisted
    );
    let review_id = ticket.capsule.as_ref().unwrap().review_id().to_owned();
    // Replaying the same finished attempt must not re-execute anything.
    let replay = run_attempt(&f, &mut sink, &reviewer);
    assert_eq!(
        reviewer.calls.get(),
        1,
        "replay dispatched the reviewer again"
    );
    assert_eq!(replay.dispatches, 0);
    drop(sink);

    let reader = StateReader::open_existing(&f.db).unwrap();
    let record = reader
        .find_decision_record("project", DecisionRecordKind::A4Review, &review_id)
        .unwrap()
        .unwrap();
    assert_eq!(record.subject_sha(), Some(f.implementation.as_str()));
    assert!(record.record_json().contains("\"verdict\":\"PASS\""));
    assert_eq!(
        stored_event_types(&f.db),
        [EventType::ReviewDispatched, EventType::ReviewPassed]
    );
    let events = reader
        .list_events(&EventQuery {
            project_id: "project".into(),
            subject: None,
            event_types: None,
            limit: 100,
        })
        .unwrap();
    assert!(
        events
            .iter()
            .all(|e| e.occurred_at == "2026-11-08T10:00:00.000000000Z"),
        "trusted time, not the claim"
    );
}

/// F2: a failure at ANY publication boundary leaves State consistent after
/// restart, without executing the review again and without replacement.
#[test]
fn failure_at_every_publish_boundary_recovers_a_consistent_package() {
    // ordinal 0 = decision record, 1 = REVIEW_DISPATCHED, 2 = REVIEW_PASSED.
    for fail_at in 0..3u32 {
        let f = fixture(&format!("fault{fail_at}"));
        let reviewer = Synthetic::default();
        let first_stored;
        {
            let mut sink = CoreSink::open(&f);
            sink.fail_at = Some(fail_at);
            let ticket = run_attempt(&f, &mut sink, &reviewer);
            assert!(ticket.persisted.is_err(), "boundary {fail_at}");
            assert!(!ticket.passed());
            first_stored = stored_event_types(&f.db);
            assert!(first_stored.len() <= 2);
        }
        // Restart: reopen the real writer and journal, retry the same attempt.
        let mut sink = CoreSink::open(&f);
        let retry = run_attempt(&f, &mut sink, &reviewer);
        assert_eq!(
            reviewer.calls.get(),
            1,
            "boundary {fail_at}: executed again"
        );
        assert_eq!(retry.dispatches, 0);
        assert!(retry.persisted.is_ok(), "{:?}", retry.persisted);
        assert!(
            retry.passed(),
            "boundary {fail_at}: {:?}",
            retry.disposition
        );
        // A third call changes nothing.
        let again = run_attempt(&f, &mut sink, &reviewer);
        assert!(again.persisted.is_ok() && again.passed());
        assert_eq!(reviewer.calls.get(), 1);
        drop(sink);
        assert_eq!(
            stored_event_types(&f.db),
            [EventType::ReviewDispatched, EventType::ReviewPassed],
            "boundary {fail_at}: decision, dispatch and verdict exactly once"
        );
        let reader = StateReader::open_existing(&f.db).unwrap();
        let review_id = retry.review().unwrap().core().review_id().to_owned();
        assert!(
            reader
                .find_decision_record("project", DecisionRecordKind::A4Review, &review_id)
                .unwrap()
                .is_some()
        );
    }
}

/// A fault before the package is sealed leaves a claim and no partial
/// State; the retry refuses instead of executing again.
#[test]
fn seal_failure_blocks_re_execution_and_leaves_no_partial_state() {
    let f = fixture("seal");
    let reviewer = Synthetic::default();
    {
        let mut sink = CoreSink::open(&f);
        sink.fail_seal = true;
        let ticket = run_attempt(&f, &mut sink, &reviewer);
        assert!(ticket.persisted.is_err() && !ticket.passed());
    }
    assert!(stored_event_types(&f.db).is_empty());
    let mut sink = CoreSink::open(&f);
    let retry = run_attempt(&f, &mut sink, &reviewer);
    assert!(matches!(
        retry.disposition,
        receipts_review_integration::ReviewDisposition::Refused(
            receipts_review_integration::ReviewRefusal::ClaimInFlight
        )
    ));
    assert_eq!(reviewer.calls.get(), 1);
    assert!(stored_event_types(&f.db).is_empty());
}

/// A REJECT is preserved through a fault and restart: the stored record is
/// never replaced by the retry, and its rejected SHA stays in history.
#[test]
fn rejected_review_survives_a_fault_unreplaced() {
    let f = fixture("reject");
    let reviewer = Synthetic::rejecting();
    {
        let mut sink = CoreSink::open(&f);
        sink.fail_at = Some(1);
        let t = run_attempt(&f, &mut sink, &reviewer);
        assert!(t.persisted.is_err());
    }
    let mut sink = CoreSink::open(&f);
    let retry = run_attempt(&f, &mut sink, &reviewer);
    assert!(retry.persisted.is_ok() && !retry.passed());
    assert_eq!(reviewer.calls.get(), 1);
    drop(sink);
    let reader = StateReader::open_existing(&f.db).unwrap();
    let id = retry.review().unwrap().core().review_id().to_owned();
    let stored = reader
        .find_decision_record("project", DecisionRecordKind::A4Review, &id)
        .unwrap()
        .unwrap();
    assert!(stored.record_json().contains("\"verdict\":\"REJECT\""));
    assert_eq!(stored.subject_sha(), Some(f.implementation.as_str()));
    assert_eq!(
        stored_event_types(&f.db),
        [
            EventType::ReviewDispatched,
            EventType::ReviewRejected,
            EventType::FindingRaised
        ]
    );
}

/// Concurrent callers for one attempt: exactly one executes; the others
/// refuse or recover. The reviewer's run count is shared across threads.
#[test]
fn concurrent_callers_execute_the_review_exactly_once() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    struct Counting(Arc<AtomicUsize>);
    impl ReviewerRuntime for Counting {
        fn runtime_id(&self) -> &str {
            "synthetic"
        }
        fn run_review(
            &self,
            c: &ReviewCapsuleNonTemporalCore,
            w: &WorkspaceHandle,
        ) -> Result<ReviewerRunOutput, FailureClass> {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(150));
            Synthetic::default().run_review(c, w)
        }
    }
    let f = Arc::new(fixture("concurrent"));
    let runs = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(6));
    let journal_dir = f.dir.0.join("journal");
    let outcomes: Vec<String> = (0..6)
        .map(|n| {
            let (f, runs, barrier, dir) = (
                Arc::clone(&f),
                Arc::clone(&runs),
                Arc::clone(&barrier),
                journal_dir.clone(),
            );
            std::thread::spawn(move || {
                let mut sink = JournalOnlySink::new(&dir);
                barrier.wait();
                let wt = f.dir.0.join(format!("review-wt-{n}"));
                let t = run_attempt_in(&f, &mut sink, &Counting(runs), &wt);
                match &t.disposition {
                    receipts_review_integration::ReviewDisposition::Reviewed(_) => "reviewed",
                    receipts_review_integration::ReviewDisposition::Replayed(_) => "replayed",
                    receipts_review_integration::ReviewDisposition::Refused(
                        receipts_review_integration::ReviewRefusal::ClaimInFlight,
                    ) => "in-flight",
                    other => panic!("{other:?}"),
                }
                .to_owned()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect();
    assert_eq!(runs.load(Ordering::SeqCst), 1, "{outcomes:?}");
    assert_eq!(outcomes.iter().filter(|o| *o == "reviewed").count(), 1);
    assert_eq!(FileJournal::new(&journal_dir).claim_files(), 1);
    // After the winner finished, any caller recovers its recorded result.
    let mut sink = JournalOnlySink::new(&journal_dir);
    let wt = f.dir.0.join("review-wt-late");
    let late = run_attempt_in(&f, &mut sink, &Counting(Arc::clone(&runs)), &wt);
    assert!(matches!(
        late.disposition,
        receipts_review_integration::ReviewDisposition::Replayed(_)
    ));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

/// Mutation control: a deliberately non-atomic claim (check, then create) is
/// caught by the same harness, so the concurrency control above can fail.
#[test]
fn a_non_atomic_claim_is_detected_by_the_concurrency_harness() {
    use std::sync::{Arc, Barrier};
    let dir = std::env::temp_dir().join(format!("receipts-e2e-racy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let barrier = Arc::new(Barrier::new(4));
    let acquired: usize = (0..4)
        .map(|_| {
            let (dir, barrier) = (dir.clone(), Arc::clone(&barrier));
            std::thread::spawn(move || {
                let path = dir.join("k.claim");
                barrier.wait();
                let free = !path.exists();
                std::thread::sleep(Duration::from_millis(50)); // widen the race
                if free {
                    std::fs::write(&path, b"x").unwrap();
                }
                usize::from(free)
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .sum();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(acquired > 1, "the racy claim was not detected");
}

/// Journal-only sink (claim, seal, in-memory publish) for the thread test.
struct JournalOnlySink {
    journal: FileJournal,
}
impl JournalOnlySink {
    fn new(dir: &Path) -> Self {
        Self {
            journal: FileJournal::new(dir),
        }
    }
}
impl ReviewRecordSink for JournalOnlySink {
    fn claim(&mut self, key: &str) -> Result<ReviewClaim, String> {
        self.journal.claim(key)
    }
    fn seal(&mut self, key: &str, sealed: &SealedReview) -> Result<(), String> {
        self.journal.seal(key, sealed)
    }
    fn publish(&mut self, _: &str, _: &ReviewRecordProposal) -> Result<(), String> {
        Ok(())
    }
}
