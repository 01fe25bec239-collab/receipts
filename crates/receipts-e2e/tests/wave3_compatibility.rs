//! Library composition witness; controlled Codex protocol process, no provider or product CLI.
mod common;
use common::{Clock, PROJECT, STAMP, fixture, git, identity};
use receipts_e2e::WorkspaceWriteScopeMatcher;
use receipts_orchestration::{
    capsules::*,
    compiler::*,
    goal::evaluate::*,
    goal::persistence::*,
    goal::*,
    orchestration::{
        CoreContentResolverV1, CoreStateWriter, FeatureAdmissionOutcome, OrchestrationDateTimeV1,
    },
    policy::FreePolicyV1,
    scheduler::*,
    *,
};
use receipts_review_integration::{
    runtime::{
        CodexAdapter, CodexAttemptEvent, RuntimeAdapter, RuntimeExecutionPolicy,
        registry::RuntimeEventStream,
    },
    state::*,
};
use receipts_runtime_bindings::RuntimeCapsuleFamily;
use receipts_workspace_execution::{
    checkpoint_writer::capture_and_persist, execution::ProcessTimeoutPolicy, *,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::{Duration, Instant},
};
const ATTEMPT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const SPEC: &[u8] = b"Keep this spec verbatim.\r\n";
struct Spec(Vec<u8>);
impl CoreContentResolverV1 for Spec {
    fn resolve(&self, project: &str, reference: &str) -> Option<Vec<u8>> {
        assert_eq!((project, reference), (PROJECT, "spec"));
        Some(self.0.clone())
    }
}
// Release the exclusive State writer between port calls so Core and Workspace use one database.
struct Durable(PathBuf);
impl Durable {
    fn open(&self) -> TrustedCoreStore {
        TrustedCoreStore::open_exclusive(&self.0).unwrap()
    }
}
impl GoalPersistencePortV1 for Durable {
    fn load_goal(&self, project: &str, goal: &str) -> Result<Option<GoalRecordV1>, StateError> {
        self.open().load_goal(project, goal)
    }
    fn load_dispatch(
        &self,
        project: &str,
        dispatch: &str,
    ) -> Result<Option<GoalDispatchRecordV1>, StateError> {
        self.open().load_dispatch(project, dispatch)
    }
    fn create_goal(
        &mut self,
        definition: &GoalDefinitionV1,
        epoch: &str,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        self.open().create_goal(definition, epoch)
    }
    fn advance_goal(
        &mut self,
        progress: &GoalProgressV1,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        self.open().advance_goal(progress)
    }
    fn reserve_dispatch(
        &mut self,
        request: &GoalDispatchV1,
        start: &AttemptStart,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        self.open().reserve_dispatch(request, start)
    }
    fn resolve_dispatch(
        &mut self,
        project: &str,
        dispatch: &str,
        resolution: LaunchResolutionV1,
        evidence_ref: &str,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        self.open()
            .resolve_dispatch(project, dispatch, resolution, evidence_ref)
    }
}
struct Consumers {
    db: PathBuf,
    artifacts: PathBuf,
    workspace: WorkspaceHandle,
    executable: PathBuf,
    criteria: Vec<Criterion>,
    launches: usize,
    records: Vec<Vec<u8>>,
}
impl Consumers {
    fn writer(&self) -> CoreStateWriter {
        CoreStateWriter::open(&self.db, identity()).unwrap()
    }
    fn graph(&self) -> ExecutionGraphEnvelope {
        load_current_graph_v1(self.writer().reader(), PROJECT, "g")
            .unwrap()
            .unwrap()
    }
    fn results(&self) -> GraphResultLedgerV1 {
        let writer = self.writer();
        let mut history = load_graph_history_v1(writer.reader(), PROJECT, "g").unwrap();
        history.sort_by_key(|m| m.parent_version().as_str().parse::<u64>().unwrap());
        let mut results = GraphResultLedgerV1::new();
        for mutation in history {
            let parent =
                load_graph_version_v1(writer.reader(), PROJECT, "g", mutation.parent_version())
                    .unwrap()
                    .unwrap();
            let scope = GraphMutationScopeV1::new(
                PROJECT,
                "g",
                parent.graph_version().clone(),
                ScopedActorV1::RuntimeA1 { role_id: None },
            );
            results = apply_graph_mutation_v1(&parent, &results, &mutation, &scope, None)
                .unwrap()
                .results;
        }
        results
    }
}
impl CriterionCheckPortV1 for Consumers {
    fn check(&mut self, _: &Criterion, _: &str) -> bool {
        panic!("unfinished task cannot establish completion")
    }
}
impl WriteScopePort for Consumers {
    fn validate(&self, p: &[String]) -> bool {
        WorkspaceWriteScopeMatcher.validate(p)
    }
    fn overlap(&self, a: &[String], b: &[String]) -> WriteScopeOverlapV1 {
        WorkspaceWriteScopeMatcher.overlap(a, b)
    }
}
impl GoalConsumerPortV1 for Consumers {
    fn emit(
        &mut self,
        goal: &GoalRecordV1,
        _: &str,
        _: GoalLifecycleEventV1,
    ) -> Result<(), StateError> {
        assert_eq!(goal.definition.project_id, PROJECT);
        Ok(())
    }
    fn task_criteria(&mut self, _: &GoalRecordV1, _: &str) -> Result<Vec<Criterion>, StateError> {
        Ok(self.criteria.clone())
    }
    fn persist_compilation(&mut self, c: &CompiledGraphV1) -> Result<(), StateError> {
        assert_eq!(c.spec, SPEC);
        fs::write(self.artifacts.join("spec"), &c.spec).unwrap();
        persist_compiled_graph_v1(&mut self.writer(), &Clock::default(), c.genesis.clone())
            .unwrap();
        Ok(())
    }
    fn recover(&mut self, _: &GoalRecordV1) -> Result<(), StateError> {
        Ok(())
    }
    fn snapshot(&mut self, _: &GoalRecordV1) -> Result<GoalRuntimeSnapshotV1, StateError> {
        Ok(GoalRuntimeSnapshotV1 {
            graph: self.graph(),
            results: self.results(),
            current_epoch: "0".into(),
            spec: fs::read(self.artifacts.join("spec")).unwrap(),
            criteria: self.criteria.clone(),
            completion: GoalCompletionFactsV1::default(),
            concurrency: ConcurrencyLimitsV1 {
                global: 1,
                per_workstream: BTreeMap::from([("w".into(), 1)]),
                per_provider: BTreeMap::from([("host".into(), 1)]),
            },
            in_flight: vec![],
            remaining_budget: DispatchBudgetV1 {
                remaining_worker_starts: 2,
                remaining_cost: None,
            },
            budget_limits: GoalBudgetLimitsV1::default(),
            budget_usage: GoalBudgetUsageV1::default(),
        })
    }
    fn read_artifact(&self, r: &str) -> Result<Vec<u8>, StateError> {
        Ok(fs::read(self.artifacts.join(r)).unwrap())
    }
    fn put_artifact(&mut self, bytes: &[u8]) -> Result<String, StateError> {
        let r = format!(
            "artifact-{}",
            fs::read_dir(&self.artifacts).unwrap().count()
        );
        fs::write(self.artifacts.join(&r), bytes).unwrap();
        Ok(r)
    }
    fn completion_changes(
        &mut self,
        _: &GoalRecordV1,
    ) -> Result<Vec<GoalGraphChangeV1>, StateError> {
        Ok(vec![])
    }
    fn publish_change(
        &mut self,
        parent: &ExecutionGraphEnvelope,
        change: &GoalGraphChangeV1,
        result: &GraphMutationOutcomeV1,
    ) -> Result<(), StateError> {
        persist_mutation_v1(
            &mut self.writer(),
            &Clock::default(),
            parent,
            &change.mutation,
            result,
        )
        .unwrap();
        Ok(())
    }
    fn request_review(
        &mut self,
        _: &GoalRecordV1,
        _: &str,
        _: &GoalAssessmentV1,
    ) -> Result<(), StateError> {
        panic!("no review verdict in this fixture")
    }
    fn candidate(
        &mut self,
        _: &GoalRecordV1,
        node: &str,
    ) -> Result<DispatchCandidateV1, StateError> {
        Ok(DispatchCandidateV1 {
            node_id: node.into(),
            write_paths: vec!["src/**".into()],
            cost_estimate: None,
            ready_sequence: 0,
            user_priority: 0,
            admission: passing(node),
        })
    }
    fn route(&mut self, _: &GoalRecordV1, node: &str) -> Result<GoalRouteV1, StateError> {
        assert_eq!(node, "n");
        Ok(GoalRouteV1 {
            dispatch_id: "dispatch".into(),
            attempt_id: receipts_review_integration::state::AttemptId::parse(ATTEMPT).unwrap(),
            attempt_number: "1".into(),
            provider_id: "host".into(),
            runtime_id: "codex".into(),
            eligible_host_native: true,
            workspace_id: self.workspace.workspace_id().into(),
            start_sha: self.workspace.base_sha().as_str().into(),
            started_at: STAMP.into(),
        })
    }
    fn launch(
        &mut self,
        goal: &GoalRecordV1,
        dispatch: &PreparedGoalDispatchV1,
    ) -> Result<(LaunchResolutionV1, String), StateError> {
        assert_eq!(goal.definition.graph_id, "g");
        assert_eq!(dispatch.node_id, "n");
        assert_eq!(dispatch.start.attempt_id().as_str(), ATTEMPT);
        self.launches += 1;
        let task = TaskCapsule::try_new(
            "n".into(),
            "w".into(),
            None,
            1,
            TaskType::Implementation,
            String::from_utf8(SPEC.to_vec()).unwrap(),
            self.criteria.clone(),
            None,
            self.workspace.base_sha().as_str().into(),
            self.workspace.base_sha().as_str().into(),
            vec!["src/**".into()],
            vec![],
            None,
            None,
            None,
            None,
            QualityFloor::Frontier,
            vec![],
            None,
            vec![VerificationPlanEntryV1::try_new(vec!["test".into()], 0).unwrap()],
            None,
            AssuranceProfileSelector::Standard,
            None,
            None,
            None,
            self.workspace.branch().into(),
            self.workspace.worktree_path().to_str().unwrap().into(),
            None,
            vec!["stop".into()],
            None,
            0,
        )
        .unwrap();
        let adapter = CodexAdapter::new(&self.executable);
        let policy = RuntimeExecutionPolicy::new(
            ProcessTimeoutPolicy::new(Duration::from_secs(10), Duration::from_millis(100)).unwrap(),
            false,
        );
        let handle = adapter
            .start(&RuntimeCapsuleFamily::Task(task), &self.workspace, &policy)
            .unwrap();
        let mut stream = adapter.stream_events(&handle);
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(Instant::now() < deadline, "stream deadline");
            match Pin::new(&mut stream).poll_event(&mut cx) {
                Poll::Ready(Some(CodexAttemptEvent::Record(bytes))) => self.records.push(bytes),
                Poll::Ready(Some(CodexAttemptEvent::Unavailable(e))) => {
                    panic!("stream unavailable: {e:?}")
                }
                Poll::Ready(None) => break,
                Poll::Pending => std::thread::park_timeout(Duration::from_millis(100)),
            }
        }
        drop(stream);
        let result = adapter.collect_result(&handle).unwrap();
        assert!(
            result.failure().is_none(),
            "failure={:?}, protocol={:?}, process={:?}",
            result.failure(),
            result.protocol_termination(),
            result.outcome.process().terminal_cause()
        );
        assert_eq!(self.records.len(), 3);
        assert!(String::from_utf8_lossy(&self.records[0]).contains("turn.started"));
        assert!(String::from_utf8_lossy(&self.records[2]).contains("turn.completed"));
        let mut store = TrustedCoreStore::open_exclusive(&self.db).unwrap();
        let receipt = capture_and_persist(
            &mut TrustedAttemptEvidenceSinkV1::new(&mut store),
            PROJECT,
            WorkspaceCheckpointEvidenceCaptureRequest {
                directory: self.workspace.worktree_path(),
                checkpoint_id: "checkpoint".into(),
                workspace_id: self.workspace.workspace_id().into(),
                task_id: Some("n".into()),
                attempt_id: Some(ATTEMPT.into()),
                kind: WorkspaceCheckpointKind::Progress,
                base_sha: Some(self.workspace.base_sha().as_str()),
                dirty_diff_ref: None,
                executed_checks: vec![],
            },
            WorkspaceDateTimeV1::try_new(STAMP).unwrap(),
            &[],
        )
        .unwrap();
        assert_eq!(receipt.attempt_id().as_str(), ATTEMPT);
        assert_eq!(receipt.project_id(), PROJECT);
        Ok((LaunchResolutionV1::Launched, receipt.checkpoint_id().into()))
    }
    fn record_evaluation(
        &mut self,
        _: &GoalRecordV1,
        _: &ExecutionGraphEnvelope,
        _: &GoalAssessmentV1,
    ) -> Result<(), StateError> {
        Ok(())
    }
}
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn passing(node: &str) -> DispatchAdmissionRequestV1 {
    let provider = "host";
    let b = || AdmissionBindingV1 {
        node_id: node.into(),
        graph_id: Some("g".into()),
        context_epoch: "0".into(),
    };
    let fresh = EvidenceFreshnessV1::Fresh;
    DispatchAdmissionRequestV1 {
        decision_id: format!("da-{node}"),
        capability_id: Some("graph.core".into()),
        binding: b(),
        selected_provider_id: Some(provider.into()),
        entitlement: Some(EntitlementEvidenceV1 {
            binding: b(),
            freshness: fresh,
            feature_admission_decision_id: "fa".into(),
            outcome: FeatureAdmissionOutcome::Allow,
        }),
        provider_auth: Some(ProviderAuthEvidenceV1 {
            binding: b(),
            freshness: fresh,
            provider_id: provider.into(),
            connection: ProviderConnectionV1::Authenticated,
            technical_status: "CONNECTED".into(),
        }),
        provider_policy: Some(ProviderPolicyEvidenceV1 {
            binding: b(),
            freshness: fresh,
            provider_policy_eligibility_id: "ppe".into(),
            provider_id: provider.into(),
            status: DispatchAdmissionProviderPolicyStatus::VerifiedAllowed,
            execution_context: "PROGRAMMATIC_WORKER".into(),
        }),
        provider_availability: Some(AvailabilityEvidenceV1 {
            binding: b(),
            freshness: fresh,
            availability_state_ref: "as".into(),
            observation: AvailabilityObservationV1::Available,
            runtime_id: Some("codex".into()),
        }),
        safety: Some(SafetyEvidenceV1 {
            binding: b(),
            freshness: fresh,
            state: SafetyStateV1::Clear,
            safety_interruption_id: None,
        }),
        quality_floor: Some(QualityFloorEvidenceV1 {
            binding: b(),
            freshness: fresh,
            routing_decision_id: "rd".into(),
            result: QualityFloorResultV1::Satisfied,
        }),
    }
}

#[test]
fn core_workspace_state_runtime_and_host_share_one_durable_fixture() {
    let f = fixture("wave3-composition");
    fs::write(f.root.join("spec"), SPEC).unwrap();
    git(&f.root, &["add", "spec"]);
    git(&f.root, &["commit", "-qm", "verbatim spec"]);
    let sha = git(&f.root, &["rev-parse", "HEAD"]);
    let workspace = WorkspaceProvisionRequest::new(
        &f.root,
        "workspace",
        Some("n"),
        "runtime-a3/n",
        f.dir.0.join("work"),
        &sha,
    )
    .unwrap()
    .provision()
    .unwrap();
    let executable = f.dir.0.join("worker");
    fs::write(&executable,"#!/bin/sh\nprintf 'run\\n' >> \"${0%/*}/launches\"\n/bin/cat > \"${0%/*}/prompt\"\nprintf '%s\\n' '{\"type\":\"turn.started\"}' '{\"type\":\"item.completed\",\"item\":{\"id\":\"message\",\"type\":\"agent_message\",\"text\":\"fixture done\"}}' '{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}'\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let artifacts = f.dir.0.join("artifacts");
    fs::create_dir(&artifacts).unwrap();
    let criterion = Criterion::try_new(
        "c".into(),
        "required check".into(),
        CriterionKind::Deterministic,
        Some(vec!["test".into()]),
        None,
    )
    .unwrap();
    let mut consumers = Consumers {
        db: f.db.clone(),
        artifacts: artifacts.clone(),
        workspace,
        executable: executable.clone(),
        criteria: vec![criterion.clone()],
        launches: 0,
        records: vec![],
    };
    let mut durable = Durable(f.db.clone());
    {
        let mut store = durable.open();
        seed_context(&mut store);
    }
    let spec = {
        let mut store = durable.open();
        let mut materializer = GitBlobMaterializer::new(&f.root, PROJECT, "repo", STAMP).unwrap();
        let admission = admit(&mut store, StateWriteOperationV1::ContextRehydration);
        let outcome = store
            .rehydrate_context(
                admission,
                ContextRehydrationRequest {
                    rehydration_attempt_id: ATTEMPT.into(),
                    project_id: PROJECT.into(),
                    durable_role_id: "role-1".into(),
                    context_epoch_id: StateEpochValueV1::try_from("0").unwrap(),
                    context_manifest_id: "manifest-1".into(),
                    executor_binding_id: None,
                    session_reference: None,
                    task_id: Some("n".into()),
                    requested_by_actor: EventActor {
                        kind: ActorKind::System,
                        id: Some("orchestrator".into()),
                    },
                    correlation_reference: Some("correlation".into()),
                    trigger_kind: ContextEpochTrigger::HostSwitch,
                    trigger_reference: Some("switch".into()),
                    started_at: STAMP.into(),
                    completed_at: STAMP.into(),
                    source_bindings: vec![ContextSourceBindingV1 {
                        source_ordinal: 0,
                        source_id: "spec".into(),
                        source_ref: BoundContextSourceRefV1::RepositorySnapshot(
                            RepositorySnapshotRefV1 {
                                project_id: PROJECT.into(),
                                repository_id: "repo".into(),
                                commit_sha: sha.clone(),
                                logical_relative_path: "spec".into(),
                            },
                        ),
                    }],
                    touch_evidence: vec![],
                    demands: vec![],
                },
                &mut materializer,
                &mut NoArtifacts,
                &mut EventSupplier,
            )
            .unwrap();
        assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Succeeded);
        assert_eq!(outcome.sources.len(), 1);
        assert_eq!(outcome.sources[0].bytes, SPEC);
        Spec(outcome.sources[0].bytes.clone())
    };
    let admissions = BTreeMap::new();
    let confirmed = BTreeSet::new();
    let context = CompileContextV1 {
        project_id: PROJECT,
        graph_id: "g",
        goal_id: "goal",
        policy_id: "policy",
        created_at: OrchestrationDateTimeV1::try_new(STAMP).unwrap(),
        context_epoch: Some(GoalEvaluationContextEpoch::from_decimal("0").unwrap()),
        compiler_id: GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        source: GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
        spec_ref: "spec",
        content: &spec,
        capability_admission: &admissions,
        entitlement_state: entitlement::ProductEntitlementState::Free,
        confirmed_criteria: &confirmed,
    };
    let proposal = DecompositionProposalV1 {
        workstreams: vec![WorkstreamOwnershipV1 {
            workstream_id: "w".into(),
            owned_paths: vec!["src/**".into()],
        }],
        nodes: vec![ProposedNodeV1 {
            node_id: "n".into(),
            kind: GraphNodeKind::TASK,
            title: None,
            workstream_id: Some("w".into()),
            required_capabilities: None,
        }],
        dependencies: vec![],
        criteria: vec![ProposedCriterionV1 {
            criterion,
            origin: CriterionOriginV1::Stated,
        }],
    };
    let policy = FreePolicyV1::new("host".into(), "codex".into()).unwrap();
    {
        let mut core = CoreFacade {
            state: &mut durable,
            consumers: &mut consumers,
            policy: &policy,
            evidence: ApplyEvidenceV1::default(),
        };
        assert!(matches!(
            core.start_goal(&context, &proposal, &WorkspaceWriteScopeMatcher, "2".into())
                .unwrap(),
            StartGoalOutcomeV1::Accepted(_)
        ));
        assert_eq!(
            core.tick(PROJECT, "goal").unwrap().state(),
            GoalEvaluationState::Incomplete
        );
    }
    assert_eq!(consumers.launches, 1);
    let before_goal = durable.load_goal(PROJECT, "goal").unwrap().unwrap();
    assert_eq!(before_goal.dispatches_used, "1");
    let before_dispatch = durable.load_dispatch(PROJECT, "dispatch").unwrap().unwrap();
    assert_eq!(before_dispatch.attempt_id.as_str(), ATTEMPT);
    let checkpoint = durable
        .open()
        .reader()
        .find_checkpoint(PROJECT, "checkpoint")
        .unwrap()
        .unwrap();
    let checkpoint_json = checkpoint.to_json().unwrap();
    assert_eq!(checkpoint.workspace_id(), "workspace");
    let capture = WorkspaceCheckpointWire::from_json(&checkpoint_json)
        .unwrap()
        .to_temporal_capture_core()
        .unwrap();
    assert_eq!(capture.core().head_sha().as_str(), sha);
    assert_eq!(capture.core().attempt_id(), Some(ATTEMPT));
    assert_eq!(capture.core().task_id(), Some("n"));
    // Recreate the consumer and State port without launch counters, streams or graph caches.
    let Consumers {
        workspace,
        criteria,
        ..
    } = consumers;
    drop(durable);
    let mut durable = Durable(f.db.clone());
    let mut consumers = Consumers {
        db: f.db.clone(),
        artifacts,
        workspace,
        executable,
        criteria,
        launches: 0,
        records: vec![],
    };
    {
        let mut core = CoreFacade {
            state: &mut durable,
            consumers: &mut consumers,
            policy: &policy,
            evidence: ApplyEvidenceV1::default(),
        };
        assert_ne!(
            core.resume_goal(PROJECT, "goal").unwrap().state(),
            GoalEvaluationState::Complete
        );
        assert!(core.resume_goal("foreign", "goal").is_err());
    }
    assert_eq!(fs::read(f.dir.0.join("launches")).unwrap(), b"run\n");
    let prompt = fs::read_to_string(f.dir.0.join("prompt")).unwrap();
    assert!(prompt.contains("Keep this spec verbatim."));
    assert!(prompt.contains(&sha));
    assert_eq!(consumers.launches, 0);
    assert!(consumers.records.is_empty());
    assert_eq!(
        durable
            .load_goal(PROJECT, "goal")
            .unwrap()
            .unwrap()
            .dispatches_used,
        "1"
    );
    assert!(durable.load_dispatch(PROJECT, "dispatch").unwrap().unwrap() == before_dispatch);
    assert_eq!(
        durable
            .open()
            .reader()
            .find_checkpoint(PROJECT, "checkpoint")
            .unwrap()
            .unwrap()
            .to_json()
            .unwrap(),
        checkpoint_json
    );
    let store = durable.open();
    assert_eq!(
        store
            .reader()
            .find_context_rehydration_attempt(PROJECT, ATTEMPT)
            .unwrap()
            .unwrap()
            .status,
        ContextRehydrationStatus::Succeeded
    );
    let attempt = store
        .reader()
        .find_attempt(&receipts_review_integration::state::AttemptId::parse(ATTEMPT).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            attempt.project_id(),
            attempt.graph_id(),
            attempt.node_id(),
            attempt.workspace_id(),
            attempt.runtime_ref(),
            attempt.start_sha()
        ),
        (PROJECT, "g", "n", "workspace", "codex", sha.as_str())
    );
    assert_eq!(
        before_dispatch.resolution,
        Some(LaunchResolutionV1::Launched)
    );
    assert_eq!(before_dispatch.evidence_ref.as_deref(), Some("checkpoint"));
    let persisted = store
        .reader()
        .load_current_graph_v1(PROJECT, "g")
        .unwrap()
        .unwrap();
    let view = project_persisted_graph_view_v1(&persisted, PROJECT, "g").unwrap();
    assert!(project_persisted_graph_view_v1(&persisted, "foreign", "g").is_err());
    let before = fs::read(&f.db).unwrap();
    let rendered = receipts_host_integration::render_graph_tree(&view);
    assert!(rendered.contains("RUNNING"));
    assert!(rendered.contains("\"n\""));
    assert!(rendered.contains(view.snapshot().resulting_digest().unwrap()));
    assert_eq!(
        rendered,
        receipts_host_integration::render_graph_tree(&view)
    );
    assert_eq!(
        receipts_host_integration::render_graph_snapshot(view.snapshot()),
        receipts_host_integration::render_graph_snapshot(view.snapshot())
    );
    assert_eq!(
        fs::read(&f.db).unwrap(),
        before,
        "rendering must not change durable bytes"
    );
    assert_eq!(
        store
            .reader()
            .find_checkpoint(PROJECT, "checkpoint")
            .unwrap()
            .unwrap()
            .to_json()
            .unwrap(),
        checkpoint_json
    );
    assert_eq!(git(&f.root, &["rev-parse", "HEAD"]), sha);
}
struct EventSupplier;
struct NoArtifacts;
impl ArtifactMaterializer for NoArtifacts {
    fn materialize(
        &mut self,
        _: &ArtifactRefV1,
        _: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        panic!("unexpected artifact")
    }
}
impl ContextRehydratedEventSupplier for EventSupplier {
    fn event_for(&mut self, attempt: &ContextRehydrationAttempt) -> EventEnvelope {
        let payload = context_rehydration_event_payload(attempt).expect("payload");
        EventEnvelope {
            event_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            project_id: attempt.project_id.clone(),
            goal_id: None,
            event_type: EventType::ContextRehydrated,
            actor: attempt.requested_by_actor.clone(),
            subject: EventSubject {
                kind: SubjectKind::Role,
                id: attempt.durable_role_id.clone(),
            },
            occurred_at: attempt.completed_at.clone(),
            payload,
            correlation_id: attempt
                .correlation_reference
                .clone()
                .expect("test correlation"),
            epoch: attempt.context_epoch_id.clone(),
        }
    }
}

fn seed_context(repo: &mut TrustedCoreStore) {
    let a = admit(repo, StateWriteOperationV1::LogicalRole);
    repo.create_logical_role(
        a,
        LogicalRole {
            role_id: "role-1".to_string(),
            project_id: PROJECT.to_string(),
            role_type: LogicalRoleType::RuntimeA2,
            status: LogicalRoleStatus::Active,
            current_context_epoch: StateEpochValueV1::try_from("0").unwrap(),
            name: None,
            workstream_id: None,
            ownership_paths: Vec::new(),
            integration_branch: None,
            context_manifest_id: Some("manifest-1".to_string()),
            active_binding_id: None,
            created_at: Some("2026-08-17T09:00:00Z".to_string()),
        },
    )
    .expect("role");
    let a = admit(repo, StateWriteOperationV1::ContextEpoch);
    repo.append_context_epoch(
        a,
        ContextEpoch {
            project_id: PROJECT.to_string(),
            epoch: StateEpochValueV1::try_from("0").unwrap(),
            advanced_at: "2026-08-17T09:00:00Z".to_string(),
            trigger: ContextEpochTrigger::A1Init,
        },
    )
    .expect("epoch");
    let a = admit(repo, StateWriteOperationV1::ContextManifest);
    repo.create_context_manifest(
        a,
        ContextManifest {
            manifest_id: "manifest-1".to_string(),
            role_id: "role-1".to_string(),
            project_id: PROJECT.to_string(),
            epoch: StateEpochValueV1::try_from("0").unwrap(),
            sources: vec![ContextManifestSource {
                r#ref: ContextSourceRef {
                    ref_type: ContextSourceRefType::RepoPath,
                    target: "spec".into(),
                },
                source_class: SourceClass::Mandatory,
                digest:
                    "sha256:v1:0000000000000000000000000000000000000000000000000000000000000000"
                        .into(),
                last_read_at: None,
                required_for: vec![],
            }],
            created_at: "2026-08-17T09:00:00Z".to_string(),
            last_rehydrated_at: None,
        },
    )
    .expect("manifest");
}

fn admit(store: &mut TrustedCoreStore, operation: StateWriteOperationV1) -> StateWriteAdmission {
    store.admit(StateWriteSubjectV1 {
        operation,
        project_id: PROJECT.into(),
        graph_id: None,
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    })
}
