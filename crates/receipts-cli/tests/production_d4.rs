//! Owned local production-entry controls. The reviewer is synthetic; publication,
//! State authority, process custody, reset and readback use production code.
#![cfg(any(
    all(target_os = "macos", target_arch = "aarch64"),
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
use receipts_cli::workspace_recovery::*;
use receipts_orchestration::orchestration::{CoreAdmissionIdentity, CoreStateWriter};
use receipts_review_integration::*;
use receipts_state::*;
use receipts_workspace_execution::execution::ProcessTimeoutPolicy;
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant, SystemTime},
};
const PROJECT: &str = "project";
const ACCEPTED: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const RUNNING: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const COMPILER: &str = "compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV";
const SOURCE: &str = "evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW";
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("/usr/bin/git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
#[track_caller]
fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "owned fixture deadline");
        std::thread::sleep(Duration::from_millis(10));
    }
}
struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        if std::thread::panicking() || self.0.join("retain").exists() {
            eprintln!("retained owned fixture: {}", self.0.display());
        } else {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
struct Fixture {
    child: Option<Child>,
    client: SupervisorClient,
    config: SupervisorConfig,
    _dir: Dir,
    base: String,
    deadline: SystemTime,
}
impl Fixture {
    fn seeded() -> (Dir, SupervisorConfig, String, SystemTime) {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = fs::canonicalize("/tmp").unwrap().join(format!(
            "receipts-production-d4-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let dir = Dir(root.clone());
        let repo = root.join("repo");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        fs::write(repo.join("tracked.txt"), b"accepted-base\n").unwrap();
        git(&repo, &["add", "tracked.txt"]);
        git(&repo, &["commit", "-qm", "base"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.join("state"))
            .unwrap();
        let config = SupervisorConfig {
            state: root.join("state/state.db"),
            repository: repo.clone(),
            worktree: root.join("worktree"),
            project: PROJECT.into(),
            graph: "graph".into(),
            task: "task".into(),
            workspace: "workspace".into(),
            branch: "task/d4".into(),
            base: base.clone(),
            compiler: COMPILER.into(),
            policy: "policy".into(),
            source: SOURCE.into(),
            role: "role".into(),
            allowed_write_paths: vec!["tracked.txt".into()],
            forbidden_write_paths: vec![],
        };
        let identity = CoreAdmissionIdentity::new(
            GraphCompilerRefV1::parse(COMPILER).unwrap(),
            "policy".into(),
        )
        .unwrap()
        .with_project(PROJECT, GraphSourceEvidenceRefV1::parse(SOURCE).unwrap());
        let mut writer = CoreStateWriter::open(&config.state, identity).unwrap();
        let node = StateGraphNodeV1::new(
            "task".into(),
            "graph".into(),
            "TASK".into(),
            "PLANNED".into(),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("1".into()),
        )
        .unwrap();
        let graph = StateGraphGenesisV1::new(
            PROJECT.into(),
            "graph".into(),
            "goal".into(),
            "policy".into(),
            Some(None),
            None,
            None,
            "a".repeat(64),
            vec![node],
            vec![],
            StateGraphGenesisProvenanceV1::new(
                GraphCompilerRefV1::parse(COMPILER).unwrap(),
                GraphSourceEvidenceRefV1::parse(SOURCE).unwrap(),
                GraphGenesisReasonV1::InitialCompilation,
            ),
        )
        .unwrap();
        writer
            .create_initial_graph(&SupervisorClock, graph)
            .unwrap();
        let stamp = SupervisorClock::at(SystemTime::now()).unwrap();
        writer
            .append_context_epoch(ContextEpoch {
                project_id: PROJECT.into(),
                epoch: StateEpochValueV1::try_from("0").unwrap(),
                advanced_at: stamp.clone(),
                trigger: ContextEpochTrigger::A1Init,
            })
            .unwrap();
        writer
            .create_logical_role(LogicalRole {
                role_id: "role".into(),
                project_id: PROJECT.into(),
                role_type: LogicalRoleType::RuntimeA2,
                status: LogicalRoleStatus::Active,
                current_context_epoch: StateEpochValueV1::try_from("0").unwrap(),
                name: None,
                workstream_id: None,
                ownership_paths: vec![],
                integration_branch: None,
                context_manifest_id: None,
                active_binding_id: None,
                created_at: Some(stamp.clone()),
            })
            .unwrap();
        // Leave room for real State/Git setup on hosted runners before testing expiry.
        let deadline = SystemTime::now() + Duration::from_secs(60);
        writer
            .create_executor_binding(ExecutorBinding {
                binding_id: "binding".into(),
                role_id: "role".into(),
                provider_id: "fixture-provider".into(),
                model_id: "fixture-model".into(),
                runtime_id: "fixture-runtime".into(),
                session_ref: None,
                routing_decision_id: None,
                bound_at: stamp.clone(),
                lease_expires_at: SupervisorClock::at(deadline).unwrap(),
                released_at: None,
                release_reason: None,
                rehydration_completed: None,
            })
            .unwrap();
        writer
            .set_logical_role_active_binding(
                "role",
                None,
                Some("binding"),
                &RoleChangeEvidenceV1 {
                    changed_at: CanonicalTimestampV1::parse(&stamp).unwrap(),
                    reason: Some("fixture binding".into()),
                },
            )
            .unwrap();
        drop(writer);
        (dir, config, base, deadline)
    }
    fn new() -> Self {
        let (dir, config, base, deadline) = Self::seeded();
        let root = dir.0.clone();
        let (child, client) = spawn_supervisor(
            Path::new(env!("CARGO_BIN_EXE_receipts")),
            &root.join("supervisor"),
            &config,
        )
        .unwrap();
        Self {
            child: Some(child),
            client,
            config,
            _dir: dir,
            base,
            deadline,
        }
    }
    fn control(&self) -> &Path {
        &self._dir.0
    }
    fn launch(&self, id: &str, number: &str, mode: &str) -> Value {
        fs::write(self.control().join("worker-mode"), mode).unwrap();
        self.client
            .launch(SupervisorLaunch {
                attempt: id.into(),
                number: number.into(),
                executable: std::env::current_exe().unwrap(),
                args: vec![
                    "worker".into(),
                    "--exact".into(),
                    "--ignored".into(),
                    "--nocapture".into(),
                ],
                expected_window: Duration::from_secs(2),
                timeout: Duration::from_secs(90),
            })
            .unwrap()
    }
    fn settled(&self) -> Value {
        let mut status = Value::Null;
        until(|| {
            status = self.client.inspect().unwrap();
            assert!(status["assurance_failure"].is_null(), "{status}");
            status["attempt"]["collected"] == true
        });
        status
    }
    fn acceptance(&mut self) -> String {
        self.launch(ACCEPTED, "1", "accept");
        self.settled();
        self.client.finalize().unwrap();
        let implementation = git(&self.config.worktree, &["rev-parse", "HEAD"]);
        publish_real_acceptance(self, &implementation);
        let s = self.client.inspect().unwrap();
        assert_eq!(s["anchor"]["accepted_commit"], implementation);
        implementation
    }
    fn expire(&self) {
        if let Ok(wait) = self.deadline.duration_since(SystemTime::now()) {
            std::thread::sleep(wait + Duration::from_millis(20));
        }
    }
    fn stop(&mut self) {
        let _ = self.client.stop();
        if let Some(mut child) = self.child.take() {
            until(|| child.try_wait().unwrap().is_some());
            let status = child.wait().unwrap();
            assert!(
                status.success(),
                "{}",
                fs::read_to_string(self.control().join("supervisor/supervisor.stderr")).unwrap()
            );
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = self.client.stop();
            let deadline = Instant::now() + Duration::from_secs(5);
            while child.try_wait().is_ok_and(|s| s.is_none()) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if child.try_wait().is_ok_and(|s| s.is_none()) {
                let _ = child.kill();
            }
            let _ = child.wait();
            let _ = fs::write(self.control().join("worker-stop"), b"");
            let pid = fs::read_to_string(self.control().join("worker-pid"))
                .ok()
                .and_then(|p| p.parse::<u32>().ok());
            let deadline = Instant::now() + Duration::from_secs(5);
            while pid.is_some_and(alive) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            if pid.is_some_and(alive) {
                let _ = fs::write(
                    self.control().join("retain"),
                    b"OWNED_WORKER_CLEANUP_UNVERIFIED",
                );
            }
        }
    }
}
#[test]
#[ignore]
fn worker() {
    assert!(std::env::vars_os().next().is_none());
    let root = std::env::current_dir().unwrap();
    let control = root.parent().unwrap();
    let mode = fs::read_to_string(control.join("worker-mode")).unwrap();
    if mode == "accept" {
        fs::write("tracked.txt", b"accepted implementation\n").unwrap();
        git(&root, &["add", "tracked.txt"]);
        git(&root, &["commit", "-qm", "accepted implementation"]);
        return;
    }
    if mode == "noop" {
        return;
    }
    if mode == "obstruct" {
        git(&root, &["rm", "-q", "tracked.txt"]);
        git(&root, &["commit", "-qm", "unaccepted deletion"]);
    } else {
        fs::write("tracked.txt", b"unaccepted commit\n").unwrap();
        git(&root, &["add", "tracked.txt"]);
        git(&root, &["commit", "-qm", "unaccepted"]);
    }
    fs::write("tracked.txt", b"dirty after unaccepted commit\n").unwrap();
    fs::write("untracked-note", b"retain this evidence\n").unwrap();
    fs::write(control.join("worker-pid"), std::process::id().to_string()).unwrap();
    for tick in 0..20_000 {
        if control.join("worker-stop").exists() {
            return;
        }
        fs::write(control.join("pulse"), tick.to_string()).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub const ALL_DIMENSIONS: &str = r#"{"dimension":"SCOPE_COMPLIANCE","assessment":"SATISFIED"},{"dimension":"CONTRACT_COMPLIANCE","assessment":"SATISFIED"},{"dimension":"ARCHITECTURE_COMPLIANCE","assessment":"SATISFIED"},{"dimension":"CORRECTNESS","assessment":"SATISFIED"},{"dimension":"ERROR_HANDLING","assessment":"SATISFIED"},{"dimension":"SECURITY_TRUST_BOUNDARIES","assessment":"SATISFIED"},{"dimension":"TEST_ADEQUACY","assessment":"SATISFIED"},{"dimension":"NEGATIVE_TESTS","assessment":"SATISFIED"},{"dimension":"REGRESSION_RISK","assessment":"SATISFIED"},{"dimension":"WRITE_SCOPE","assessment":"SATISFIED"},{"dimension":"UNDISCLOSED_CHANGES","assessment":"SATISFIED"},{"dimension":"EVIDENCE_ACCURACY","assessment":"SATISFIED"}"#;
struct Routing;
impl ReviewerRoutingPort for Routing {
    fn select_reviewer(
        &self,
        _: &ReviewCapsuleNonTemporalCore,
        _: &ReviewerRequirements,
    ) -> Result<ReviewerCandidate, ReviewerRoutingRefusal> {
        Ok(ReviewerCandidate {
            provider_id: "review-provider".into(),
            runtime_id: "synthetic".into(),
            model_id: None,
            resume_session: None,
            quality: Some(ReviewerQualityEvidence {
                tier: ReviewRequestReviewerFloor::Frontier,
                basis_ref: "fixture:frontier".into(),
            }),
        })
    }
}
struct Reviewer;
impl ReviewerRuntime for Reviewer {
    fn runtime_id(&self) -> &str {
        "synthetic"
    }
    fn run_review(
        &self,
        c: &ReviewCapsuleNonTemporalCore,
        _: &receipts_workspace_execution::WorkspaceHandle,
    ) -> Result<ReviewerRunOutput, receipts_review_integration::runtime::FailureClass> {
        Ok(ReviewerRunOutput {
            text: Some(format!(
                r#"{{"review_id":"{}","task_id":"{}","reviewed_sha":"{}","independence_attested":true,"verdict":"PASS","blocking_findings":[],"nonblocking_findings":[],"dimensions":[{}]}}"#,
                c.review_id(),
                c.task_id(),
                c.implementation_sha().as_str(),
                ALL_DIMENSIONS
            )),
            truncated: false,
            session_ref: None,
            protected: true,
            reduced_isolation: true,
        })
    }
}
fn review_now() -> ReviewDateTimeV1 {
    ReviewDateTimeV1::try_new(SupervisorClock::at(SystemTime::now()).unwrap()).unwrap()
}
fn runner() -> CheckRunnerConfig {
    CheckRunnerConfig {
        executables: vec![("true".into(), PathBuf::from("/usr/bin/true"))],
        timeout: ProcessTimeoutPolicy::new(Duration::from_secs(10), Duration::from_millis(100))
            .unwrap(),
        env: None,
    }
}
fn real_acceptance_gate(f: &mut Fixture, sha: &str) -> GateOutcome {
    let request = ReviewRequest::new(
        ReviewRequestNonTemporalCore::new(
            "request".into(),
            "task".into(),
            ACCEPTED.into(),
            sha.into(),
            f.base.clone(),
            "change tracked content".into(),
            vec![
                ReviewCapsuleCriterion::new(
                    "c".into(),
                    "owned fixture check".into(),
                    ReviewCapsuleCriterionKind::Deterministic,
                    Some(vec!["true".into()]),
                    None,
                )
                .unwrap(),
            ],
            vec!["tracked.txt".into()],
            AssuranceProfile::Standard,
            ReviewCapsuleReviewScope::Full,
            ReviewRequestNonNegativeInteger::from_decimal("0").unwrap(),
            Some("workspace".into()),
            Some("task/d4".into()),
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
    let check = WorkspaceCheckpointExecutedCheckCore::new(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        vec!["true".into()],
        0,
        CommitSha::parse(sha).unwrap(),
        Some(false),
        None,
    )
    .unwrap();
    let checks = vec![ReviewCapsuleCheck::new(
        check.clone(),
        Some(ReviewCapsuleCheckResult::Pass),
    )];
    let config = runner();
    let controller = ReviewController {
        routing: &Routing,
        runtimes: &[&Reviewer],
        checks: &config,
        clock: &review_now,
    };
    let ticket = controller.on_attempt_finished(
        AttemptFinished {
            project_id: PROJECT,
            request: &request,
            repository_root: &f.config.repository,
            implementer: &ImplementerIdentity {
                provider_id: "implementer-provider".into(),
                runtime_id: "fixture-runtime".into(),
                model_id: None,
                session_ref: None,
            },
            worker_checks: &checks,
            review_worktree: &f.control().join("review"),
            generation: 1,
        },
        &mut f.client,
    );
    assert!(
        ticket.passed(),
        "{:?} {:?}",
        ticket.disposition,
        ticket.persisted
    );
    let handoff = A3HandoffNonTemporalCore::new(
        "task".into(),
        ACCEPTED.into(),
        f.base.clone(),
        sha.into(),
        "task/d4".into(),
        vec!["tracked.txt".into()],
        vec![A3HandoffCheck::new(check, Some(A3HandoffCheckResult::Pass))],
        true,
        None,
        None,
        None,
        None,
        None,
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
    .unwrap();
    let integration = IntegrationRequestNonTemporalCore::new(
        "integration".into(),
        IntegrationRequestGateLevel::A2Acceptance,
        "task/d4".into(),
        sha.into(),
        "main".into(),
        vec![
            IntegrationRequestTask::new(
                "task".into(),
                sha.into(),
                sha.into(),
                IntegrationRequestA4Verdict::Pass,
                None,
                IntegrationDecisionNullableString::Omitted,
            )
            .unwrap(),
        ],
        None,
        None,
        None,
        None,
        Some(IntegrationRequestAttestations::new(
            Some(false),
            Some(false),
            Some(false),
            Some(false),
            Some(false),
            Some(false),
        )),
    )
    .unwrap();
    fs::create_dir(f.control().join("gate")).unwrap();
    let state = StateReader::open_existing(&f.config.state).unwrap();
    let gate = AcceptanceGate::evaluate(&AcceptanceInput {
        project_id: PROJECT,
        integration_request: &integration,
        review_request: &request,
        profile_floor: AssuranceProfile::Light,
        capsule: ticket.capsule.as_ref().unwrap(),
        handoffs: &[handoff],
        reviews: &[ticket.review().unwrap().core().clone()],
        completed_repair_attempts: 0,
        repository_root: &f.config.repository,
        task_worktree: &f.config.worktree,
        merge_target_worktree: &f.config.repository,
        forbidden_write_paths: &[],
        dependencies: &[],
        state: Some(&state),
        supersessions: &[],
        disposition_content: None,
        finding_dispositions: &[],
        checks: &config,
        component_suite: &["true".into()],
        scratch: &f.control().join("gate"),
        clock: &review_now,
        generation: 1,
    });
    assert_eq!(
        gate.outcome,
        IntegrationDecisionOutcome::Accept,
        "{:#?}",
        gate.checks
    );
    gate
}
fn publish_real_acceptance(f: &mut Fixture, sha: &str) {
    let gate = real_acceptance_gate(f, sha);
    gate.record(&mut f.client).unwrap();
    // Replay publishes the stored seal and rebinds the same immutable anchor.
    gate.record(&mut f.client).unwrap();
}
#[test]
fn production_entry_publishes_acceptance_and_binds_the_original_workspace() {
    let mut f = Fixture::new();
    let accepted = f.acceptance();
    let status = f.client.inspect().unwrap();
    assert_eq!(status["anchor"]["accepted_commit"], accepted);
    assert_eq!(status["anchor"]["workspace_id"], "workspace");
    f.stop();
    let store = TrustedCoreStore::open_exclusive(&f.config.state).unwrap();
    let state = store.reader();
    let attempt = state
        .find_attempt(&AttemptId::parse(ACCEPTED).unwrap())
        .unwrap()
        .unwrap();
    assert!(attempt.is_finished());
    assert!(
        state
            .find_finalization_v1(PROJECT, &AttemptId::parse(ACCEPTED).unwrap())
            .unwrap()
            .is_some()
    );
    drop(store);
}
fn alive(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    unsafe { kill(pid as i32, 0) == 0 }
}
fn worker_pid(f: &Fixture) -> u32 {
    fs::read_to_string(f.control().join("worker-pid"))
        .unwrap()
        .parse()
        .unwrap()
}
fn controller(f: &Fixture, mode: &str) -> Value {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["controller_probe", "--exact", "--ignored", "--nocapture"])
        .env_clear()
        .env("D4_ROOT", f.control())
        .env("D4_MODE", mode)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&fs::read(f.control().join("controller-result")).unwrap()).unwrap()
}
#[test]
#[ignore]
fn controller_probe() {
    let root = PathBuf::from(std::env::var_os("D4_ROOT").unwrap());
    let mode = std::env::var("D4_MODE").unwrap();
    let c = SupervisorClient::connect(&root.join("supervisor")).unwrap();
    let before = c.inspect().unwrap();
    let result = match mode.as_str() {
        "prepare" => c.prepare_recovery("recovery").unwrap(),
        "reset" => c.prepare_reset("recovery").unwrap(),
        _ => c.inspect().unwrap(),
    };
    fs::write(
        root.join("controller-result"),
        serde_json::to_vec(&json!({"incarnation":before["incarnation"],"result":result})).unwrap(),
    )
    .unwrap();
}
#[test]
fn controller_restart_retains_original_custody_and_recovers_the_exact_accepted_tree() {
    let mut f = Fixture::new();
    let accepted = f.acceptance();
    f.launch(RUNNING, "2", "dirty");
    until(|| f.control().join("worker-pid").exists());
    let pid = worker_pid(&f);
    let unaccepted = git(&f.config.worktree, &["rev-parse", "HEAD"]);
    assert_ne!(accepted, unaccepted);
    assert!(
        f.client.prepare_recovery("recovery").is_err(),
        "open handoff refuses"
    );
    f.client.close_handoff().unwrap();
    assert!(
        f.client.prepare_recovery("recovery").is_err(),
        "unexpired lease refuses"
    );
    assert!(
        TrustedCoreStore::open_exclusive(&f.config.state).is_err(),
        "supervisor retains exclusive writer"
    );
    let identity = controller(&f, "inspect")["incarnation"].clone();
    let pulse = fs::read(f.control().join("pulse")).unwrap();
    until(|| fs::read(f.control().join("pulse")).is_ok_and(|p| p != pulse));
    assert_eq!(worker_pid(&f), pid);
    assert!(alive(pid));
    f.expire();
    let prepared = controller(&f, "prepare");
    assert_eq!(prepared["incarnation"], identity);
    assert_eq!(prepared["result"]["phase"], "SIGNAL_PREPARED");
    assert!(alive(pid));
    assert!(f.client.recover("foreign-recovery").is_err());
    assert_eq!(git(&f.config.worktree, &["rev-parse", "HEAD"]), unaccepted);
    // Replacing the pointer with identical bytes must not adopt a new inode.
    let pointer = f.config.worktree.join(".git");
    let retained = f.control().join("original-git-pointer");
    fs::rename(&pointer, &retained).unwrap();
    fs::write(&pointer, fs::read(&retained).unwrap()).unwrap();
    assert!(f.client.recover("recovery").is_err());
    assert!(alive(pid));
    fs::remove_file(&pointer).unwrap();
    fs::rename(&retained, &pointer).unwrap();
    let reset = controller(&f, "reset");
    assert_eq!(reset["incarnation"], identity);
    assert_eq!(reset["result"]["phase"], "RESET_PREPARED");
    assert!(!alive(pid));
    assert_eq!(git(&f.config.worktree, &["rev-parse", "HEAD"]), unaccepted);
    let reconnected = SupervisorClient::connect(&f.control().join("supervisor")).unwrap();
    let resolved = reconnected.recover("recovery").unwrap();
    assert_eq!(resolved["phase"], "RESOLVED");
    assert_eq!(resolved["active"], false);
    assert_eq!(git(&f.config.worktree, &["rev-parse", "HEAD"]), accepted);
    assert_eq!(
        git(&f.config.worktree, &["write-tree"]),
        git(&f.config.worktree, &["rev-parse", "HEAD^{tree}"])
    );
    assert_eq!(
        fs::read(f.config.worktree.join("tracked.txt")).unwrap(),
        b"accepted implementation\n"
    );
    assert_eq!(
        fs::read(f.config.worktree.join("untracked-note")).unwrap(),
        b"retain this evidence\n"
    );
    assert_eq!(
        reconnected.recover("recovery").unwrap()["phase"],
        "RESOLVED"
    );
    f.stop();
    let store = TrustedCoreStore::open_exclusive(&f.config.state).unwrap();
    let record = store
        .reader()
        .find_workspace_recovery_v1(
            &WorkspaceRecoverySubjectV1 {
                project_id: PROJECT.into(),
                graph_id: "graph".into(),
                attempt_id: RUNNING.into(),
            },
            "recovery",
        )
        .unwrap();
    assert_eq!(record.phase, "RESOLVED");
    assert!(!record.active);
    assert!(
        !store
            .reader()
            .find_attempt(&AttemptId::parse(RUNNING).unwrap())
            .unwrap()
            .unwrap()
            .is_finished(),
        "recovery never promotes a successful finish"
    );
    drop(store);
    retain_proof(
        &f,
        "restart-reset",
        json!({"accepted":accepted,"unaccepted":unaccepted,"original_pid":pid,"incarnation":identity,"resolved":resolved}),
    );
}
#[test]
fn ignored_untracked_obstruction_is_retained_and_reset_stays_prepared() {
    let mut f = Fixture::new();
    let accepted = f.acceptance();
    f.launch(RUNNING, "2", "obstruct");
    until(|| f.control().join("worker-pid").exists());
    f.client.close_handoff().unwrap();
    fs::write(
        f.config.repository.join(".git/info/exclude"),
        b"tracked.txt\n",
    )
    .unwrap();
    f.expire();
    let failed = f.client.recover("recovery").unwrap_err();
    assert!(failed.to_string().contains("obstruction"));
    assert_ne!(git(&f.config.worktree, &["rev-parse", "HEAD"]), accepted);
    assert_eq!(
        fs::read(f.config.worktree.join("tracked.txt")).unwrap(),
        b"dirty after unaccepted commit\n"
    );
    fs::rename(
        f.config.worktree.join("tracked.txt"),
        f.config.worktree.join("retained-obstruction"),
    )
    .unwrap();
    assert_eq!(f.client.recover("recovery").unwrap()["phase"], "RESOLVED");
    assert_eq!(
        fs::read(f.config.worktree.join("retained-obstruction")).unwrap(),
        b"dirty after unaccepted commit\n"
    );
    f.stop();
    retain_proof(
        &f,
        "obstruction",
        json!({"accepted":accepted,"refusal":failed.to_string()}),
    );
}
#[test]
fn eligible_handoff_cannot_be_recovered_or_promoted_by_recovery() {
    let mut f = Fixture::new();
    f.acceptance();
    f.launch(RUNNING, "2", "noop");
    f.settled();
    f.client.finalize().unwrap();
    f.expire();
    let refused = f.client.prepare_recovery("recovery").unwrap_err();
    assert!(refused.to_string().contains("EligibleHandoff"), "{refused}");
    assert!(
        f.client.close_handoff().is_err(),
        "eligible closure cannot be rewritten"
    );
    assert_eq!(
        f.client.inspect().unwrap()["anchor"]["subject"]["attempt_id"],
        ACCEPTED
    );
    f.stop();
    retain_proof(&f, "eligible", json!({"refusal":refused.to_string()}));
}
#[test]
fn failed_start_checkpoint_quarantines_the_registered_attempt_before_spawn() {
    let mut f = Fixture::new();
    let outputs = f.control().join("supervisor/outputs");
    let retained = f.control().join("retained-outputs");
    fs::rename(&outputs, &retained).unwrap();
    fs::write(&outputs, b"owned obstruction").unwrap();
    let launch = |id: &str, number: &str| SupervisorLaunch {
        attempt: id.into(),
        number: number.into(),
        executable: std::env::current_exe().unwrap(),
        args: vec!["worker".into(), "--exact".into(), "--ignored".into()],
        expected_window: Duration::from_millis(200),
        timeout: Duration::from_secs(30),
    };
    assert!(f.client.launch(launch(ACCEPTED, "1")).is_err());
    fs::remove_file(&outputs).unwrap();
    fs::rename(retained, outputs).unwrap();
    let retry = f.client.launch(launch(RUNNING, "2"));
    assert!(
        retry.is_err(),
        "a partially registered attempt must quarantine subsequent launch"
    );
    assert!(!f.control().join("worker-pid").exists());
    let stopped = f.client.stop().unwrap();
    assert!(!stopped["assurance_failure"].is_null());
    let mut child = f.child.take().unwrap();
    until(|| child.try_wait().unwrap().is_some());
    assert!(!child.wait().unwrap().success());
    let store = TrustedCoreStore::open_exclusive(&f.config.state).unwrap();
    let state = store.reader();
    assert!(
        state
            .find_attempt(&AttemptId::parse(ACCEPTED).unwrap())
            .unwrap()
            .is_some()
    );
    assert!(
        state
            .find_attempt(&AttemptId::parse(RUNNING).unwrap())
            .unwrap()
            .is_none()
    );
    drop(store);
    retain_proof(&f, "prelaunch-quarantine", json!({"cleanup":stopped}));
}
#[test]
fn persistence_failure_after_launch_still_cleans_the_original_process_and_retains_custody() {
    let mut f = Fixture::new();
    f.acceptance();
    f.launch(RUNNING, "2", "dirty");
    until(|| f.control().join("worker-pid").exists());
    let pid = worker_pid(&f);
    let outputs = f.control().join("supervisor/outputs");
    fs::rename(&outputs, f.control().join("retained-outputs")).unwrap();
    fs::write(&outputs, b"owned obstruction").unwrap();
    let result = f.client.stop().unwrap();
    assert!(!result["assurance_failure"].is_null());
    let mut child = f.child.take().unwrap();
    until(|| child.try_wait().unwrap().is_some());
    assert!(!child.wait().unwrap().success());
    assert!(!alive(pid));
    let admin = git(&f.config.worktree, &["rev-parse", "--absolute-git-dir"]);
    assert!(Path::new(&admin).join("receipts-d4-reservation").exists());
    assert!(
        SupervisorClient::connect(&f.control().join("supervisor"))
            .unwrap()
            .inspect()
            .is_err()
    );
    retain_proof(
        &f,
        "emergency",
        json!({"original_pid":pid,"cleanup":result}),
    );
}
fn retain_proof(f: &Fixture, name: &str, observations: Value) {
    let Some(base) = std::env::var_os("D4_EVIDENCE_DIR") else {
        return;
    };
    let base = PathBuf::from(base);
    fs::create_dir_all(&base).unwrap();
    let destination = base.join(format!(
        "{name}-{}",
        f.control().file_name().unwrap().to_string_lossy()
    ));
    fs::create_dir(&destination).unwrap();
    fs::write(
        destination.join("observations.json"),
        serde_json::to_vec_pretty(&observations).unwrap(),
    )
    .unwrap();
    copy_files(f.control(), &destination.join("fixture"));
}
fn copy_files(source: &Path, target: &Path) {
    fs::create_dir(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if entry.file_name() == "socket" || entry.file_name() == "capability" {
            continue;
        }
        let destination = target.join(entry.file_name());
        if kind.is_dir() {
            copy_files(&entry.path(), &destination);
        } else if kind.is_file() {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}
#[test]
fn unauthenticated_unknown_and_unsupported_entry_requests_do_not_grant_authority() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    let mut f = Fixture::new();
    let mut stream = UnixStream::connect(f.control().join("supervisor/socket")).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    stream.write_all(&[0; 32]).unwrap();
    assert!(stream.read_exact(&mut [0]).is_err());
    let token = fs::read(f.control().join("supervisor/capability")).unwrap();
    let payload =
        serde_json::to_vec(&json!({"op":"Recover","recovery":"r","desired_sha":f.base})).unwrap();
    let mut stream = UnixStream::connect(f.control().join("supervisor/socket")).unwrap();
    stream.write_all(&token).unwrap();
    stream
        .write_all(&(payload.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&payload).unwrap();
    let mut length = [0; 4];
    stream.read_exact(&mut length).unwrap();
    let mut bytes = vec![0; u32::from_be_bytes(length) as usize];
    stream.read_exact(&mut bytes).unwrap();
    let reply: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(reply["error"].as_str().unwrap().contains("unknown field"));
    assert_eq!(f.client.inspect().unwrap()["head"], f.base);
    assert!(f.client.recover("r").is_err());
    f.stop();
    let replay = Command::new(env!("CARGO_BIN_EXE_receipts"))
        .args([
            "__workspace-supervisor",
            f.control().join("supervisor").to_str().unwrap(),
        ])
        .env_clear()
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut replay = replay;
    replay.stdin.take().unwrap().write_all(&token).unwrap();
    assert!(!replay.wait().unwrap().success());
}
#[test]
fn supervisor_loss_refuses_adoption_and_preserves_the_original_reservation() {
    let mut f = Fixture::new();
    f.acceptance();
    f.launch(RUNNING, "2", "dirty");
    until(|| f.control().join("worker-pid").exists());
    let pid = worker_pid(&f);
    let mut child = f.child.take().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(f.client.inspect().is_err());
    fs::write(f.control().join("worker-stop"), b"").unwrap();
    until(|| !alive(pid));
    let admin = git(&f.config.worktree, &["rev-parse", "--absolute-git-dir"]);
    assert!(Path::new(&admin).join("receipts-d4-reservation").exists());
    assert!(
        spawn_supervisor(
            Path::new(env!("CARGO_BIN_EXE_receipts")),
            &f.control().join("replacement"),
            &f.config
        )
        .is_err()
    );
    assert_eq!(
        fs::read(f.config.worktree.join("untracked-note")).unwrap(),
        b"retain this evidence\n"
    );
    retain_proof(
        &f,
        "supervisor-loss",
        json!({"original_pid":pid,"adoption":"REFUSED"}),
    );
}
#[test]
fn reset_readback_after_interruption_resolves_without_repeating_the_physical_action() {
    let mut f = Fixture::new();
    let accepted = f.acceptance();
    f.launch(RUNNING, "2", "dirty");
    until(|| f.control().join("worker-pid").exists());
    f.client.close_handoff().unwrap();
    f.expire();
    assert_eq!(
        f.client.prepare_reset("recovery").unwrap()["phase"],
        "RESET_PREPARED"
    );
    // Controlled interruption fixture: the reset completed but its result was lost.
    git(&f.config.worktree, &["reset", "--hard", &accepted]);
    let lock = f.config.repository.join(".git/refs/heads/task/d4.lock");
    fs::write(&lock, b"owned fixture lock").unwrap();
    let refused = Command::new("/usr/bin/git")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "reset",
            "--hard",
            &accepted,
        ])
        .current_dir(&f.config.worktree)
        .env_clear()
        .output()
        .unwrap();
    assert!(
        !refused.status.success(),
        "a repeated reset must be blocked by the owned ref lock"
    );
    let resolved = SupervisorClient::connect(&f.control().join("supervisor"))
        .unwrap()
        .recover("recovery")
        .unwrap();
    assert_eq!(resolved["phase"], "RESOLVED");
    assert_eq!(fs::read(&lock).unwrap(), b"owned fixture lock");
    fs::remove_file(lock).unwrap();
    f.stop();
    retain_proof(
        &f,
        "readback-reconcile",
        json!({"accepted":accepted,"resolved":resolved,"second_physical_reset":"PREVENTED_BY_OWNED_LOCK"}),
    );
}
#[test]
fn trickled_authenticated_frame_cannot_hold_the_supervisor_past_its_io_budget() {
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    let mut f = Fixture::new();
    let token = fs::read(f.control().join("supervisor/capability")).unwrap();
    let mut stream = UnixStream::connect(f.control().join("supervisor/socket")).unwrap();
    let (sent, ready) = std::sync::mpsc::sync_channel(1);
    let trickler = std::thread::spawn(move || {
        stream.write_all(&token).unwrap();
        stream.write_all(&500u32.to_be_bytes()).unwrap();
        sent.send(()).unwrap();
        for _ in 0..50 {
            if stream.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(70));
        }
    });
    ready.recv().unwrap();
    let started = Instant::now();
    let status = f.client.inspect().unwrap();
    let elapsed = started.elapsed();
    trickler.join().unwrap();
    assert!(
        elapsed < Duration::from_secs(2),
        "a per-read timeout allowed a trickled frame to monopolize the owner for {elapsed:?}"
    );
    assert!(status["attempt"].is_null());
    f.stop();
}
#[test]
fn publication_to_anchor_failure_quarantines_the_launch_path() {
    let mut f = Fixture::new();
    f.launch(ACCEPTED, "1", "accept");
    f.settled();
    f.client.finalize().unwrap();
    let accepted = git(&f.config.worktree, &["rev-parse", "HEAD"]);
    let gate = real_acceptance_gate(&mut f, &accepted);
    fs::write(
        f.config.worktree.join("tracked.txt"),
        b"changed after gate observation\n",
    )
    .unwrap();
    assert!(gate.record(&mut f.client).is_err());
    assert!(f.client.inspect().unwrap()["anchor"].is_null());
    fs::write(
        f.config.worktree.join("tracked.txt"),
        b"accepted implementation\n",
    )
    .unwrap();
    let attempted = f.client.launch(SupervisorLaunch {
        attempt: RUNNING.into(),
        number: "2".into(),
        executable: std::env::current_exe().unwrap(),
        args: vec!["worker".into(), "--exact".into(), "--ignored".into()],
        expected_window: Duration::from_millis(200),
        timeout: Duration::from_secs(30),
    });
    assert!(
        attempted.is_err(),
        "published acceptance without a physical anchor cannot silently become an unadmitted launch"
    );
    let stopped = f.client.stop().unwrap();
    assert!(!stopped["assurance_failure"].is_null());
    let mut child = f.child.take().unwrap();
    until(|| child.try_wait().unwrap().is_some());
    assert!(!child.wait().unwrap().success());
    let store = TrustedCoreStore::open_exclusive(&f.config.state).unwrap();
    assert!(
        store
            .reader()
            .find_attempt(&AttemptId::parse(RUNNING).unwrap())
            .unwrap()
            .is_none()
    );
    drop(store);
    retain_proof(
        &f,
        "anchor-quarantine",
        json!({"accepted":accepted,"stopped":stopped}),
    );
}
#[test]
fn unsupported_socket_path_refuses_before_workspace_provisioning() {
    let mut f = Fixture::new();
    f.stop();
    let mut config = f.config.clone();
    config.branch = "task/long-path".into();
    config.worktree = f.control().join("must-not-be-provisioned");
    let refused = spawn_supervisor(
        Path::new(env!("CARGO_BIN_EXE_receipts")),
        &f.control().join("x".repeat(120)),
        &config,
    )
    .err()
    .expect("the local socket path is unsupported");
    assert!(refused.to_string().contains("path"), "{refused}");
    assert!(!config.worktree.exists());
    let branch = Command::new("/usr/bin/git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/task/long-path",
        ])
        .current_dir(&f.config.repository)
        .output()
        .unwrap();
    assert!(!branch.status.success());
}

#[test]
fn concurrent_supervisor_startup_retains_independent_custody() {
    for round in 0..3 {
        let barrier = std::sync::Barrier::new(8);
        std::thread::scope(|scope| {
            let mut threads = Vec::new();
            for slot in 0..8 {
                let barrier = &barrier;
                threads.push(scope.spawn(move || {
                    barrier.wait();
                    let mut f = Fixture::new();
                    let status = f.client.inspect().unwrap();
                    assert!(!status["incarnation"].as_str().unwrap().is_empty());
                    f.stop();
                    eprintln!("CONCURRENT round={round} slot={slot} ready-and-reaped");
                    status["incarnation"].as_str().unwrap().to_owned()
                }));
            }
            let incarnations: std::collections::HashSet<_> =
                threads.into_iter().map(|t| t.join().unwrap()).collect();
            assert_eq!(
                incarnations.len(),
                8,
                "each startup has independent custody"
            );
        });
    }
}

struct BootstrapFixture {
    child: Child,
    input: Option<std::process::ChildStdin>,
    client: SupervisorClient,
    config: SupervisorConfig,
    dir: Dir,
}
impl BootstrapFixture {
    const CAPABILITY: [u8; 32] = [0xa5; 32];
    fn new() -> Self {
        let (dir, config, _, _) = Fixture::seeded();
        let root = dir.0.join("supervisor");
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        for (name, bytes) in [
            ("config", serde_json::to_vec(&config).unwrap()),
            ("capability", Self::CAPABILITY.to_vec()),
        ] {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(root.join(name))
                .unwrap();
            file.write_all(&bytes).unwrap();
            file.sync_all().unwrap();
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_receipts"))
            .arg("__workspace-supervisor")
            .arg(&root)
            .env_clear()
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let client = SupervisorClient::connect(&root).unwrap();
        Self {
            child,
            input,
            client,
            config,
            dir,
        }
    }
    fn write(&mut self, bytes: &[u8]) {
        use std::io::Write;
        self.input.as_mut().unwrap().write_all(bytes).unwrap();
    }
    fn ready_with_open_writer(&mut self) -> Value {
        // Match the owner's startup budget rather than imposing a faster fixture limit.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(status) = self.client.inspect() {
                assert!(self.input.is_some());
                assert!(self.child.try_wait().unwrap().is_none());
                return status;
            }
            assert!(
                self.child.try_wait().unwrap().is_none(),
                "bootstrap exited before authenticated readiness"
            );
            assert!(
                Instant::now() < deadline,
                "authenticated readiness must not await stdin EOF"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn refused(&mut self) {
        until(|| self.child.try_wait().unwrap().is_some());
        assert!(!self.child.wait().unwrap().success());
        assert!(!self.dir.0.join("supervisor/started").exists());
        assert!(!self.dir.0.join("supervisor/socket").exists());
        assert!(!self.config.worktree.exists());
    }
}
impl Drop for BootstrapFixture {
    fn drop(&mut self) {
        let writer_was_open = self.input.take().is_some();
        let killed = if self.child.try_wait().is_ok_and(|s| s.is_none()) {
            Some(self.child.kill().is_ok())
        } else {
            None
        };
        let reaped = self.child.wait().is_ok();
        eprintln!(
            "BOOTSTRAP_FIXTURE writer_was_open={writer_was_open} kill_requested={} reaped={reaped}",
            killed.is_some()
        );
        if let Some(base) = std::env::var_os("D4_EVIDENCE_DIR") {
            let base = PathBuf::from(base);
            let _ = fs::create_dir_all(&base);
            let _ = fs::write(
                base.join(format!(
                    "bootstrap-cleanup-{}-{}.json",
                    std::process::id(),
                    self.child.id()
                )),
                serde_json::to_vec(
                    &json!({"writer_was_open":writer_was_open,"kill_ok":killed,"reaped":reaped}),
                )
                .unwrap(),
            );
        }
    }
}
#[test]
fn bootstrap_frame_readiness_does_not_wait_for_writer_eof() {
    let mut f = BootstrapFixture::new();
    f.write(&BootstrapFixture::CAPABILITY);
    let status = f.ready_with_open_writer();
    assert!(!status["incarnation"].as_str().unwrap().is_empty());
    assert_eq!(status["head"], f.config.base);
    f.client.stop().unwrap();
    until(|| f.child.try_wait().unwrap().is_some());
    assert!(f.child.wait().unwrap().success());
}
#[test]
fn bootstrap_frame_trailing_bytes_are_never_another_command() {
    let mut f = BootstrapFixture::new();
    let mut bytes = BootstrapFixture::CAPABILITY.to_vec();
    bytes.extend_from_slice(&serde_json::to_vec(&json!({"op":"Stop"})).unwrap());
    f.write(&bytes);
    let before = f.ready_with_open_writer();
    f.write(&serde_json::to_vec(&json!({"op":"Recover","recovery":"untrusted-stdin"})).unwrap());
    assert_eq!(
        f.client.inspect().unwrap()["incarnation"],
        before["incarnation"]
    );
    f.client.stop().unwrap();
    until(|| f.child.try_wait().unwrap().is_some());
    assert!(f.child.wait().unwrap().success());
}
#[test]
fn bootstrap_frame_wrong_capability_and_short_frames_refuse_before_provisioning() {
    let mut wrong = BootstrapFixture::new();
    let mut token = BootstrapFixture::CAPABILITY;
    token[0] ^= 1;
    wrong.write(&token);
    wrong.refused();
    for length in [0, 31] {
        let mut short = BootstrapFixture::new();
        short.write(&BootstrapFixture::CAPABILITY[..length]);
        drop(short.input.take());
        short.refused();
    }
}
#[test]
fn bootstrap_incomplete_startup_keeps_owner_deadline_and_reaps() {
    use std::os::unix::fs::PermissionsExt;
    let (dir, config, _, _) = Fixture::seeded();
    let exe = dir.0.join("no-bootstrap.sh");
    let pidfile = dir.0.join("owned-pid");
    fs::write(
        &exe,
        format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec /bin/sleep 30\n",
            pidfile.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o700)).unwrap();
    let start = Instant::now();
    let refused = spawn_supervisor(&exe, &dir.0.join("supervisor"), &config)
        .err()
        .unwrap();
    assert!(refused.to_string().contains("startup deadline"));
    assert!((Duration::from_secs(10)..Duration::from_secs(12)).contains(&start.elapsed()));
    let pid = fs::read_to_string(pidfile).unwrap().parse().unwrap();
    assert!(!alive(pid));
    assert!(!dir.0.join("supervisor/started").exists());
    assert!(!config.worktree.exists());
    eprintln!(
        "BOOTSTRAP_OWNER deadline_ms={} owned_child_absent=true",
        start.elapsed().as_millis()
    );
}

#[test]
fn automatic_cleanup_persists_the_attempt_snapshot_before_acknowledgement() {
    use sha2::{Digest, Sha256};
    let mut f = Fixture::new();
    f.launch(ACCEPTED, "1", "accept");
    let status = f.settled();
    assert_eq!(status["attempt"]["cleanup_evidence"], "Acknowledged");
    let receipt: Value = serde_json::from_slice(
        &fs::read(f.control().join(format!("supervisor/cleanup-{ACCEPTED}"))).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["attempt"], ACCEPTED);
    assert_eq!(receipt["reason"], "NaturalCompletion");
    assert_eq!(receipt["artifacts"].as_array().unwrap().len(), 2);
    for artifact in receipt["artifacts"].as_array().unwrap() {
        let bytes = fs::read(artifact["path"].as_str().unwrap()).unwrap();
        let digest = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        assert_eq!(artifact["digest"], digest);
        if artifact["stream"] == "stdout" {
            assert!(
                String::from_utf8(bytes)
                    .unwrap()
                    .contains("test result: ok")
            );
        }
    }
    f.client.finalize().unwrap();
    f.stop();
}
