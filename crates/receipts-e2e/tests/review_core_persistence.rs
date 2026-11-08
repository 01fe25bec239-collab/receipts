//! Review records persist only through the actual trusted-core writer.
//!
//! `ReviewRecordSink` is bound to Orchestration's real `CoreStateWriter`
//! (exclusive TrustedCoreStore, admitted project, content resolver that
//! verifies each payload digest). The Review controller runs against a real
//! repository with a synthetic reviewer; its A4Review decision record and
//! REVIEW_* events land in State, read back through the read-only
//! `StateReader`, and a replay is refused atomically.

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
    ReviewCapsuleReviewScope, ReviewController, ReviewDateTimeV1, ReviewRecordProposal,
    ReviewRecordSink, ReviewRequest, ReviewRequestNonNegativeInteger, ReviewRequestNonTemporalCore,
    ReviewerCandidate, ReviewerRequirements, ReviewerRoutingPort, ReviewerRoutingRefusal,
    ReviewerRunOutput, ReviewerRuntime,
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

/// The trusted composition's sink: every record goes through the core.
struct CoreSink {
    writer: CoreStateWriter,
    next: u32,
}
impl ReviewRecordSink for CoreSink {
    fn persist(&mut self, p: &ReviewRecordProposal) -> Result<(), String> {
        for record in &p.decision_records {
            self.writer
                .append_decision_record(record)
                .map_err(|e| e.to_string())?;
        }
        for event in &p.events {
            self.next += 1;
            let id = format!("01ARZ3NDEKTSV4RRFFQ69G{:04}", self.next);
            self.writer
                .persist_event_proposal(&Clock, &Resolver(p), event, &id)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

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
        })
    }
}

/// Synthetic reviewer (not provider proof): returns a PASS for the capsule.
struct Synthetic;
impl ReviewerRuntime for Synthetic {
    fn runtime_id(&self) -> &str {
        "synthetic"
    }
    fn run_review(
        &self,
        c: &ReviewCapsuleNonTemporalCore,
        _: &WorkspaceHandle,
    ) -> Result<ReviewerRunOutput, FailureClass> {
        Ok(ReviewerRunOutput {
            text: Some(format!(
                r#"{{"review_id":"{}","task_id":"{}","reviewed_sha":"{}","independence_attested":true,"verdict":"PASS","blocking_findings":[],"nonblocking_findings":[],"dimensions":[{{"dimension":"CORRECTNESS","assessment":"SATISFIED"}}]}}"#,
                c.review_id(),
                c.task_id(),
                c.implementation_sha().as_str()
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

#[test]
fn review_records_persist_through_the_core_writer_and_replay_is_refused() {
    let dir = std::env::temp_dir().join(format!("receipts-e2e-core-{}", std::process::id()));
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
    let identity = CoreAdmissionIdentity::new(
        GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        "policy".into(),
    )
    .unwrap()
    .with_project(
        "project",
        GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
    );
    let mut sink = CoreSink {
        writer: CoreStateWriter::open(&db, identity).unwrap(),
        next: 0,
    };

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
    let checks = CheckRunnerConfig {
        executables: vec![],
        timeout: ProcessTimeoutPolicy::new(Duration::from_secs(5), Duration::from_millis(200))
            .unwrap(),
        env: None,
    };
    let controller = ReviewController {
        routing: &Pick,
        runtimes: &[&Synthetic],
        checks: &checks,
        clock: &now,
    };
    let implementer = ImplementerIdentity {
        provider_id: "provider-a".into(),
        runtime_id: "x".into(),
        model_id: None,
        session_ref: None,
    };
    let attempt = AttemptFinished {
        project_id: "project",
        request: &request,
        repository_root: &root,
        implementer: &implementer,
        worker_checks: &[],
        review_worktree: &dir.0.join("review-wt"),
    };
    let ticket = controller.on_attempt_finished(attempt, &mut sink);
    assert!(
        ticket.passed(),
        "{:?} {:?}",
        ticket.disposition,
        ticket.persisted
    );
    let review_id = ticket.capsule.as_ref().unwrap().review_id().to_owned();

    // Replaying the same finished attempt re-derives the same identities:
    // the core refuses the duplicate record and nothing new is written.
    let replay = controller.on_attempt_finished(attempt, &mut sink);
    assert!(replay.persisted.is_err(), "replay must be refused");
    assert!(!replay.passed());
    drop(sink);

    let reader = StateReader::open_existing(&db).unwrap();
    let record = reader
        .find_decision_record("project", DecisionRecordKind::A4Review, &review_id)
        .unwrap()
        .unwrap();
    assert_eq!(record.subject_sha(), Some(implementation.as_str()));
    assert!(record.record_json().contains("\"verdict\":\"PASS\""));
    let events = reader
        .list_events(&EventQuery {
            project_id: "project".into(),
            subject: None,
            event_types: None,
            limit: 100,
        })
        .unwrap();
    let types: Vec<EventType> = events.iter().map(|e| e.event_type).collect();
    assert_eq!(
        types,
        [EventType::ReviewDispatched, EventType::ReviewPassed]
    );
    assert!(
        events
            .iter()
            .all(|e| e.occurred_at == "2026-11-08T10:00:00.000000000Z"),
        "trusted time, not the claim"
    );
}
