use super::driver::*;
use super::engine::PreparedGoalDispatchV1;
use super::engine::tests::seed;
use super::evaluate::*;
use super::persistence::*;
use crate::compiler::CompiledGraphV1;
use crate::policy::FreePolicyV1;
use crate::scheduler::*;
use crate::test_support::{PORT, envelope, passing};
use crate::*;
use receipts_state::{AttemptId, StateError};
use std::collections::{BTreeMap, BTreeSet};

struct Controlled {
    limits: GoalBudgetLimitsV1,
    usage: GoalBudgetUsageV1,
    drift_after_check: bool,
    route_provider: String,
    compiled_specs: Vec<Vec<u8>>,
    gate_pass: bool,
    checked_shas: Vec<String>,
    review_gates: Vec<super::GoalEvaluationState>,
    artifact_root: Option<std::path::PathBuf>,
    crash_launch: bool,
    graph: ExecutionGraphEnvelope,
    results: GraphResultLedgerV1,
    criteria: Vec<crate::capsules::Criterion>,
    artifacts: BTreeMap<String, Vec<u8>>,
    recovery: usize,
    routes: usize,
    launches: usize,
    checks: usize,
    reviews: BTreeSet<String>,
    events: Vec<GoalLifecycleEventV1>,
    changes: Vec<GoalGraphChangeV1>,
    epoch: String,
    allow: bool,
    wall: bool,
    spec: Vec<u8>,
    budget: u64,
}
impl Controlled {
    fn new(state: GraphNodeState) -> Self {
        Self {
            limits: GoalBudgetLimitsV1 {
                wall_seconds: Some(100),
                ..Default::default()
            },
            usage: GoalBudgetUsageV1 {
                cost_units: Some(0),
                ..Default::default()
            },
            drift_after_check: false,
            route_provider: "host".into(),
            compiled_specs: vec![],
            gate_pass: true,
            checked_shas: vec![],
            review_gates: vec![],
            artifact_root: None,
            crash_launch: false,
            graph: envelope(&[("n", "TASK", state, Some("w"))], &[]),
            results: GraphResultLedgerV1::new(),
            criteria: vec![
                crate::capsules::Criterion::try_new(
                    "c".into(),
                    "required".into(),
                    crate::capsules::CriterionKind::Deterministic,
                    Some(vec!["test".into()]),
                    None,
                )
                .unwrap(),
            ],
            artifacts: BTreeMap::new(),
            recovery: 0,
            routes: 0,
            launches: 0,
            checks: 0,
            reviews: BTreeSet::new(),
            events: vec![],
            changes: vec![],
            epoch: "0".into(),
            allow: true,
            wall: false,
            spec: b"spec".to_vec(),
            budget: 2,
        }
    }
}
impl CriterionCheckPortV1 for Controlled {
    fn check(&mut self, _: &crate::capsules::Criterion, sha: &str) -> bool {
        self.checks += 1;
        self.checked_shas.push(sha.into());
        self.gate_pass
    }
}
impl WriteScopePort for Controlled {
    fn validate(&self, paths: &[String]) -> bool {
        PORT.validate(paths)
    }
    fn overlap(&self, a: &[String], b: &[String]) -> WriteScopeOverlapV1 {
        PORT.overlap(a, b)
    }
}
impl GoalConsumerPortV1 for Controlled {
    fn emit(
        &mut self,
        _: &GoalRecordV1,
        _: &str,
        event: GoalLifecycleEventV1,
    ) -> Result<(), StateError> {
        self.events.push(event);
        Ok(())
    }
    fn task_criteria(
        &mut self,
        _: &GoalRecordV1,
        _: &str,
    ) -> Result<Vec<crate::capsules::Criterion>, StateError> {
        Ok(self.criteria.clone())
    }
    fn persist_compilation(&mut self, compiled: &CompiledGraphV1) -> Result<(), StateError> {
        self.compiled_specs.push(compiled.spec.clone());
        self.criteria = compiled
            .criteria
            .iter()
            .map(|c| c.criterion.clone())
            .collect();
        Ok(())
    }
    fn recover(&mut self, _: &GoalRecordV1) -> Result<(), StateError> {
        self.recovery += 1;
        Ok(())
    }
    fn snapshot(&mut self, _: &GoalRecordV1) -> Result<GoalRuntimeSnapshotV1, StateError> {
        Ok(GoalRuntimeSnapshotV1 {
            graph: self.graph.clone(),
            results: self.results.clone(),
            current_epoch: self.epoch.clone(),
            spec: self.spec.clone(),
            criteria: self.criteria.clone(),
            completion: GoalCompletionFactsV1 {
                required_work_integrated: true,
                no_blocking_tasks_or_findings: true,
                security_review_complete: true,
                dependencies_resolved: true,
                epoch_reconciled: true,
                integrated_sha: Some(if self.drift_after_check && self.checks > 0 {
                    "b".repeat(40)
                } else {
                    "a".repeat(40)
                }),
            },
            concurrency: ConcurrencyLimitsV1 {
                global: 1,
                per_workstream: BTreeMap::from([("w".into(), 1)]),
                per_provider: BTreeMap::from([("host".into(), 1)]),
            },
            in_flight: vec![],
            remaining_budget: DispatchBudgetV1 {
                remaining_worker_starts: self.budget,
                remaining_cost: None,
            },
            budget_limits: self.limits.clone(),
            budget_usage: GoalBudgetUsageV1 {
                elapsed_seconds: if self.wall {
                    100
                } else {
                    self.usage.elapsed_seconds
                },
                ..self.usage.clone()
            },
        })
    }
    fn read_artifact(&self, r: &str) -> Result<Vec<u8>, StateError> {
        if let Some(root) = &self.artifact_root {
            return Ok(std::fs::read(root.join(r)).unwrap());
        }
        Ok(self.artifacts[r].clone())
    }
    fn put_artifact(&mut self, bytes: &[u8]) -> Result<String, StateError> {
        use sha2::{Digest, Sha256};
        let r: String = Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if let Some(root) = &self.artifact_root {
            std::fs::write(root.join(&r), bytes).unwrap();
        }
        self.artifacts.insert(r.clone(), bytes.to_vec());
        Ok(r)
    }
    fn completion_changes(
        &mut self,
        _: &GoalRecordV1,
    ) -> Result<Vec<GoalGraphChangeV1>, StateError> {
        Ok(std::mem::take(&mut self.changes))
    }
    fn publish_change(
        &mut self,
        _: &ExecutionGraphEnvelope,
        _: &GoalGraphChangeV1,
        outcome: &GraphMutationOutcomeV1,
    ) -> Result<(), StateError> {
        self.graph = outcome.envelope.clone();
        self.results = outcome.results.clone();
        Ok(())
    }
    fn request_review(
        &mut self,
        _: &GoalRecordV1,
        node: &str,
        gate: &GoalAssessmentV1,
    ) -> Result<(), StateError> {
        self.review_gates.push(gate.state());
        self.reviews.insert(node.into());
        Ok(())
    }
    fn candidate(
        &mut self,
        _: &GoalRecordV1,
        node: &str,
    ) -> Result<DispatchCandidateV1, StateError> {
        let mut admission = passing(node, "host");
        admission.binding.context_epoch = "0".into();
        admission
            .entitlement
            .as_mut()
            .unwrap()
            .binding
            .context_epoch = "0".into();
        admission
            .provider_auth
            .as_mut()
            .unwrap()
            .binding
            .context_epoch = "0".into();
        admission
            .provider_policy
            .as_mut()
            .unwrap()
            .binding
            .context_epoch = "0".into();
        admission
            .provider_availability
            .as_mut()
            .unwrap()
            .binding
            .context_epoch = "0".into();
        admission.safety.as_mut().unwrap().binding.context_epoch = "0".into();
        admission
            .quality_floor
            .as_mut()
            .unwrap()
            .binding
            .context_epoch = "0".into();
        if !self.allow {
            admission.safety = None;
        }
        Ok(DispatchCandidateV1 {
            node_id: node.into(),
            write_paths: vec!["src/**".into()],
            cost_estimate: Some(1),
            ready_sequence: 0,
            user_priority: 0,
            admission,
        })
    }
    fn route(&mut self, _: &GoalRecordV1, _: &str) -> Result<GoalRouteV1, StateError> {
        self.routes += 1;
        Ok(GoalRouteV1 {
            dispatch_id: "d".into(),
            attempt_id: AttemptId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
            attempt_number: "1".into(),
            provider_id: self.route_provider.clone(),
            runtime_id: "runtime".into(),
            eligible_host_native: true,
            workspace_id: "w".into(),
            start_sha: "a".repeat(40),
            started_at: "2026-11-08T10:00:00.000000000Z".into(),
        })
    }
    fn launch(
        &mut self,
        _: &GoalRecordV1,
        _: &PreparedGoalDispatchV1,
    ) -> Result<(LaunchResolutionV1, String), StateError> {
        self.launches += 1;
        if self.crash_launch {
            return Err(StateError::SetupRefused {
                reason: receipts_state::SetupRefusalV1::MissingEvidence,
            });
        }
        Ok((LaunchResolutionV1::Launched, "launch:evidence".into()))
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
fn policy() -> FreePolicyV1 {
    FreePolicyV1::new("host".into(), "runtime".into()).unwrap()
}

#[test]
fn facade_runs_recovery_admission_routing_and_real_state_reservation() {
    let (_dir, mut state) = seed("2");
    let mut port = Controlled::new(GraphNodeState::Planned);
    let policy = policy();
    let mut facade = CoreFacade {
        state: &mut state,
        consumers: &mut port,
        policy: &policy,
        evidence: ApplyEvidenceV1::default(),
    };
    assert_eq!(
        facade.tick("p", "goal").unwrap().state(),
        super::GoalEvaluationState::Incomplete
    );
    assert_eq!(
        facade.snapshot("p", "goal").unwrap().goal.dispatches_used,
        "1"
    );
    assert_eq!((port.recovery, port.routes, port.launches), (1, 1, 1));
}
#[test]
fn single_runtime_selection_survives_reopen_and_refuses_policy_switch() {
    let (dir, mut state) = seed("2");
    let mut port = Controlled::new(GraphNodeState::Integrated);
    let policy = policy();
    initialize_goal_continuation_v1(&mut state, &mut port, &policy, "p", "goal").unwrap();
    drop(state);
    let mut state = receipts_state::TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let other = FreePolicyV1::new("other".into(), "runtime".into()).unwrap();
    let mut facade = CoreFacade {
        state: &mut state,
        consumers: &mut port,
        policy: &other,
        evidence: ApplyEvidenceV1::default(),
    };
    assert!(facade.resume_goal("p", "goal").is_err());
    assert_eq!(port.launches, 0);
}
#[test]
fn empty_graph_semantic_gap_budget_and_missing_admission_never_complete() {
    for scenario in 0..4 {
        let (_dir, mut state) = seed("2");
        let mut port = Controlled::new(GraphNodeState::Planned);
        let policy = policy();
        match scenario {
            0 => {
                port.graph = envelope(
                    &[("n", "TASK", GraphNodeState::HumanRequired, Some("w"))],
                    &[],
                )
            }
            1 => {
                port.graph = envelope(&[("n", "TASK", GraphNodeState::Integrated, Some("w"))], &[]);
                port.criteria = vec![
                    crate::capsules::Criterion::try_new(
                        "c".into(),
                        "semantic".into(),
                        crate::capsules::CriterionKind::Semantic,
                        None,
                        None,
                    )
                    .unwrap(),
                ];
            }
            2 => port.budget = 0,
            _ => port.allow = false,
        }
        let mut facade = CoreFacade {
            state: &mut state,
            consumers: &mut port,
            policy: &policy,
            evidence: ApplyEvidenceV1::default(),
        };
        let outcome = facade.tick("p", "goal").unwrap();
        assert_ne!(outcome.state(), super::GoalEvaluationState::Complete);
        if scenario >= 2 {
            assert_eq!(outcome.state(), super::GoalEvaluationState::Blocked);
        }
        assert_eq!(port.launches, 0);
    }
}
#[test]
fn core_applies_completion_mutations_and_requests_review_once_by_identity() {
    use crate::wire::GraphMutationWireV1;
    let (_dir, mut state) = seed("2");
    let mut port = Controlled::new(GraphNodeState::Running);
    let policy = policy();
    // A forbidden completion shortcut must be refused before publication.
    let mutation = GraphMutationWireV1::from_json(r#"{"mutation_id":"m","graph_id":"g","parent_version":1,"resulting_version":2,"actor":{"role":"RUNTIME_A1"},"reason":"completion","created_at":"2026-11-08T00:00:00Z","operations":[{"op":"SET_NODE_STATE","node_id":"n","detail":{"from":"RUNNING","to":"ACCEPTED","cause":{"kind":"ACCEPT"}}}]}"#).unwrap().to_mutation().unwrap();
    port.changes.push(GoalGraphChangeV1 {
        mutation,
        scope: GraphMutationScopeV1::new(
            "p",
            "g",
            port.graph.graph_version().clone(),
            ScopedActorV1::RuntimeA1 { role_id: None },
        ),
    });
    let mut facade = CoreFacade {
        state: &mut state,
        consumers: &mut port,
        policy: &policy,
        evidence: ApplyEvidenceV1::default(),
    };
    assert!(facade.tick("p", "goal").is_err());
    assert_eq!(
        port.graph.graph().node_state("n"),
        Some(GraphNodeState::Running)
    );
    port.graph = envelope(
        &[("n", "TASK", GraphNodeState::AwaitingReview, Some("w"))],
        &[],
    );
    for _ in 0..2 {
        CoreFacade {
            state: &mut state,
            consumers: &mut port,
            policy: &policy,
            evidence: ApplyEvidenceV1::default(),
        }
        .tick("p", "goal")
        .unwrap();
    }
    assert_eq!(port.reviews, BTreeSet::from(["n".into()]));
}

fn change(port: &Controlled, operations: serde_json::Value) -> GoalGraphChangeV1 {
    let parent: u64 = port.graph.graph_version().as_str().parse().unwrap();
    let json = serde_json::json!({"mutation_id":format!("m{parent}"),"graph_id":"g","parent_version":parent,"resulting_version":parent+1,"actor":{"role":"RUNTIME_A1"},"reason":"controlled completion","created_at":"2026-11-08T00:00:00Z","operations":operations});
    GoalGraphChangeV1 {
        mutation: crate::wire::GraphMutationWireV1::from_json(&json.to_string())
            .unwrap()
            .to_mutation()
            .unwrap(),
        scope: GraphMutationScopeV1::new(
            "p",
            "g",
            port.graph.graph_version().clone(),
            ScopedActorV1::RuntimeA1 { role_id: None },
        ),
    }
}
fn apply_control(port: &mut Controlled, operations: serde_json::Value) {
    let change = change(port, operations);
    let outcome = apply_graph_mutation_v1(
        &port.graph,
        &port.results,
        &change.mutation,
        &change.scope,
        None,
    )
    .unwrap();
    port.graph = outcome.envelope;
    port.results = outcome.results;
}
#[test]
fn explicit_failed_only_retry_expands_fresh_round_and_retains_accepted_history() {
    use serde_json::json;
    for (requested, repair_limit) in [
        (false, None),
        (true, None),
        (true, Some(0)),
        (true, Some(1)),
    ] {
        let (_dir, mut state) = seed("2");
        let mut port = Controlled::new(GraphNodeState::Accepted);
        let policy = policy();
        port.limits.repairs = repair_limit;
        port.graph = envelope(
            &[
                ("n", "IMPLEMENTATION", GraphNodeState::Accepted, Some("w")),
                ("r1", "REVIEW", GraphNodeState::Planned, Some("w")),
            ],
            &[("e", "n", "r1")],
        );
        let set = |from: &str, to: &str, cause: serde_json::Value| json!({"op":"SET_NODE_STATE","node_id":"r1","detail":{"from":from,"to":to,"cause":cause}});
        apply_control(
            &mut port,
            json!([
                set("PLANNED", "READY", json!({"kind":"DEPENDENCIES_SATISFIED"})),
                set(
                    "READY",
                    "ADMITTED",
                    json!({"kind":"ADMISSION","admission_decision_id":"ad"})
                ),
                set(
                    "ADMITTED",
                    "DISPATCHED",
                    json!({"kind":"DISPATCH","attempt_id":"ar1"})
                ),
                set(
                    "DISPATCHED",
                    "RUNNING",
                    json!({"kind":"START","attempt_id":"ar1"})
                ),
                set(
                    "RUNNING",
                    "AWAITING_REVIEW",
                    json!({"kind":"SUBMIT","attempt_id":"ar1"})
                )
            ]),
        );
        port.results = port
            .results
            .admit_attempt_non_code("p", "g", "r1", "ar1")
            .unwrap();
        apply_control(
            &mut port,
            json!([
                {"op":"ATTACH_RESULT","node_id":"r1","detail":{"result_ref":"r-result","result":{"node_id":"r1","graph_id":"g","outcome":"REJECTED","attempt_id":"ar1","review_id":"deterministic-r1","produced_at":"2026-11-08T00:00:00Z"}}},
                set("AWAITING_REVIEW","REJECTED",json!({"kind":"REVIEW_VERDICT","result_node_id":"r1"}))
            ]),
        );
        let accepted = port.graph.graph().node("n").unwrap().clone();
        initialize_goal_continuation_v1(&mut state, &mut port, &policy, "p", "goal").unwrap();
        let expansion = change(
            &port,
            json!([{"op":"EXPAND_REPAIR","node_id":"r1","detail":{
                "nodes":[{"node_id":"repair","graph_id":"g","kind":"REPAIR","state":"PLANNED","workstream_id":"w","parent_node_id":"n","attempt_number":2,"created_in_version":4},
                    {"node_id":"review2","graph_id":"g","kind":"REVIEW","state":"PLANNED","workstream_id":"w","attempt_number":2,"created_in_version":4}],
                "edges":[{"edge_id":"reject","graph_id":"g","from_node":"r1","to_node":"repair","edge_class":"CONTROL","control_kind":"ON_REJECT"},
                    {"edge_id":"review-next","graph_id":"g","from_node":"repair","to_node":"review2","edge_class":"PRECEDENCE","precedence_kind":"REQUIRES_ACCEPTED"}]
            }}]),
        );
        port.changes.push(expansion);
        let requests = if requested {
            BTreeSet::from(["r1".into()])
        } else {
            BTreeSet::new()
        };
        let goal = state.load_goal("p", "goal").unwrap().unwrap();
        let mut driver = GoalDriverV1 {
            consumers: &mut port,
            policy: &policy,
            evidence: ApplyEvidenceV1::default(),
            retry_failed: &requests,
            evaluation_binding: None,
        };
        let round = super::engine::GoalLoopPortV1::reconcile(&mut driver, &goal).unwrap();
        if requested && repair_limit != Some(0) {
            assert_eq!(round.dispatch.unwrap().node_id, "repair");
            assert_eq!(
                port.graph.graph().node("repair").unwrap().parent_node_id(),
                Some("n")
            );
            assert_eq!(
                port.graph.graph().node_state("r1"),
                Some(GraphNodeState::Rejected)
            );
        } else {
            assert_eq!(
                round.assessment.state(),
                super::GoalEvaluationState::Blocked
            );
            assert!(port.graph.graph().node("repair").is_none());
        }
        assert_eq!(port.graph.graph_id(), "g");
        assert_eq!(port.graph.graph().node("n"), Some(&accepted));
        if requested && repair_limit != Some(0) {
            // Controlled post-integration read projection. The retained rejected
            // review is history; fresh repair/review states still gate completion.
            let graph = ExecutionGraph::from_parts(
                "g",
                port.graph
                    .graph()
                    .nodes()
                    .cloned()
                    .map(|n| match n.node_id() {
                        "repair" => n.with_state(GraphNodeState::Integrated),
                        "review2" => n.with_state(GraphNodeState::Passed),
                        _ => n,
                    })
                    .collect::<Vec<_>>(),
                port.graph.graph().edges().cloned().collect::<Vec<_>>(),
            )
            .unwrap();
            port.graph = ExecutionGraphEnvelope::bind(
                &graph,
                ExecutionGraphEnvelopeFields::new(
                    "p",
                    "goal",
                    port.graph.graph_version().clone(),
                    crate::test_support::time("2026-11-08T00:00:00Z"),
                    "policy",
                ),
            )
            .unwrap();
            let mut driver = GoalDriverV1 {
                consumers: &mut port,
                policy: &policy,
                evidence: ApplyEvidenceV1::default(),
                retry_failed: &requests,
                evaluation_binding: None,
            };
            assert_eq!(
                super::engine::GoalLoopPortV1::reconcile(&mut driver, &goal)
                    .unwrap()
                    .assessment
                    .state(),
                super::GoalEvaluationState::Complete
            );
        }
    }
}

#[test]
fn restart_uses_disk_continuation_and_real_state_unknown_launch_without_duplicate() {
    let (dir, mut state) = seed("2");
    let root = dir.db().parent().unwrap().join("artifacts");
    std::fs::create_dir(&root).unwrap();
    let mut port = Controlled::new(GraphNodeState::Planned);
    port.artifact_root = Some(root.clone());
    port.crash_launch = true;
    let policy = policy();
    assert!(
        CoreFacade {
            state: &mut state,
            consumers: &mut port,
            policy: &policy,
            evidence: ApplyEvidenceV1::default()
        }
        .tick("p", "goal")
        .is_err()
    );
    assert_eq!(port.launches, 1);
    drop(port);
    drop(state);
    // New adapter instance has no memory of artifacts, graph progress or tickets.
    // Its conservative fixture still sees planned work; State's reservation
    // alone must prevent a second launch across this interrupted publication.
    let mut state = receipts_state::TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let mut port = Controlled::new(GraphNodeState::Planned);
    port.artifact_root = Some(root);
    let mut facade = CoreFacade {
        state: &mut state,
        consumers: &mut port,
        policy: &policy,
        evidence: ApplyEvidenceV1::default(),
    };
    let outcome = facade.resume_goal("p", "goal").unwrap();
    assert_eq!(outcome.state(), super::GoalEvaluationState::Blocked);
    assert_eq!(
        facade.snapshot("p", "goal").unwrap().state,
        super::GoalEvaluationState::Blocked
    );
    assert_eq!(
        facade.snapshot("p", "goal").unwrap().goal.dispatches_used,
        "1"
    );
    assert_eq!(port.launches, 0);
}

#[test]
fn completion_is_persisted_and_measured_against_original_spec() {
    let (_dir, mut state) = seed("2");
    let mut port = Controlled::new(GraphNodeState::Integrated);
    let policy = policy();
    let mut facade = CoreFacade {
        state: &mut state,
        consumers: &mut port,
        policy: &policy,
        evidence: ApplyEvidenceV1::default(),
    };
    assert_eq!(
        facade.tick("p", "goal").unwrap().state(),
        super::GoalEvaluationState::Complete
    );
    let saved = facade.snapshot("p", "goal").unwrap();
    assert_eq!(saved.state, super::GoalEvaluationState::Complete);
    assert!(saved.gaps.is_empty());
    assert_eq!(saved.integrated_sha, Some("a".repeat(40)));
    assert_eq!(port.checks, 1);
    port.spec = b"different objective".to_vec();
    assert!(
        CoreFacade {
            state: &mut state,
            consumers: &mut port,
            policy: &policy,
            evidence: ApplyEvidenceV1::default()
        }
        .resume_goal("p", "goal")
        .is_err()
    );
}
#[test]
fn admitted_new_epoch_takes_a_separate_no_dispatch_reconciliation_step() {
    let (_dir, mut state) = seed("2");
    let mut port = Controlled::new(GraphNodeState::Integrated);
    let policy = policy();
    initialize_goal_continuation_v1(&mut state, &mut port, &policy, "p", "goal").unwrap();
    let admission = state.admit(receipts_state::StateWriteSubjectV1 {
        operation: receipts_state::StateWriteOperationV1::ContextEpoch,
        project_id: "p".into(),
        graph_id: None,
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    });
    state
        .append_context_epoch(
            admission,
            receipts_state::ContextEpoch {
                project_id: "p".into(),
                epoch: receipts_state::StateEpochValueV1::try_from("1").unwrap(),
                advanced_at: "opaque".into(),
                trigger: receipts_state::ContextEpochTrigger::A1Init,
            },
        )
        .unwrap();
    port.epoch = "1".into();
    let mut facade = CoreFacade {
        state: &mut state,
        consumers: &mut port,
        policy: &policy,
        evidence: ApplyEvidenceV1::default(),
    };
    assert_eq!(
        facade.resume_goal("p", "goal").unwrap().state(),
        super::GoalEvaluationState::Incomplete
    );
    assert_eq!(
        facade.snapshot("p", "goal").unwrap().goal.context_epoch,
        "1"
    );
    assert_eq!(
        facade.tick("p", "goal").unwrap().state(),
        super::GoalEvaluationState::Complete
    );
    assert_eq!(port.launches, 0);
}

#[test]
fn cost_unknown_cost_exhaustion_and_wall_exhaustion_block_before_routing() {
    for scenario in 0..3 {
        let (_dir, mut state) = seed("2");
        let mut port = Controlled::new(GraphNodeState::Planned);
        let policy = policy();
        if scenario < 2 {
            port.limits.cost_units = Some(1);
            port.usage.cost_units = if scenario == 0 { None } else { Some(1) };
        } else {
            port.wall = true;
        }
        let outcome = CoreFacade {
            state: &mut state,
            consumers: &mut port,
            policy: &policy,
            evidence: ApplyEvidenceV1::default(),
        }
        .tick("p", "goal")
        .unwrap();
        assert_eq!(outcome.state(), super::GoalEvaluationState::Blocked);
        assert!(!outcome.gaps().is_empty());
        assert_eq!((port.routes, port.launches), (0, 0));
    }
}
#[test]
fn route_must_match_admitted_provider_and_completion_must_match_current_sha() {
    for drift in [false, true] {
        let (_dir, mut state) = seed("2");
        let mut port = Controlled::new(if drift {
            GraphNodeState::Integrated
        } else {
            GraphNodeState::Planned
        });
        let policy = policy();
        if drift {
            port.drift_after_check = true;
        } else {
            port.route_provider = "different".into();
        }
        let mut facade = CoreFacade {
            state: &mut state,
            consumers: &mut port,
            policy: &policy,
            evidence: ApplyEvidenceV1::default(),
        };
        assert!(facade.tick("p", "goal").is_err());
        assert_ne!(
            facade.snapshot("p", "goal").unwrap().state,
            super::GoalEvaluationState::Complete
        );
        assert_eq!(port.launches, 0);
        assert!(!port.events.contains(&GoalLifecycleEventV1::GoalCompleted));
    }
}
#[test]
fn start_facade_compiles_verbatim_spec_and_emits_creation_events() {
    use crate::compiler::*;
    use crate::orchestration::CoreContentResolverV1;
    struct Content;
    impl CoreContentResolverV1 for Content {
        fn resolve(&self, _: &str, _: &str) -> Option<Vec<u8>> {
            Some(b"spec".to_vec())
        }
    }
    let (_dir, mut state) = seed("2");
    let mut port = Controlled::new(GraphNodeState::Planned);
    let policy = policy();
    let criteria = port
        .criteria
        .iter()
        .cloned()
        .map(|criterion| ProposedCriterionV1 {
            criterion,
            origin: CriterionOriginV1::Stated,
        })
        .collect();
    let admission = BTreeMap::new();
    let confirmed = BTreeSet::new();
    let context = CompileContextV1 {
        project_id: "p",
        graph_id: "g",
        goal_id: "goal",
        policy_id: "policy",
        created_at: crate::test_support::time("2026-11-08T00:00:00Z"),
        context_epoch: Some(super::GoalEvaluationContextEpoch::from_decimal("0").unwrap()),
        compiler_id: receipts_state::GraphCompilerRefV1::parse(crate::test_support::COMPILER)
            .unwrap(),
        source: receipts_state::GraphSourceEvidenceRefV1::parse(crate::test_support::SOURCE)
            .unwrap(),
        spec_ref: "artifact:verbatim",
        content: &Content,
        capability_admission: &admission,
        entitlement_state: receipts_state::entitlement::ProductEntitlementState::Free,
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
        criteria,
    };
    let outcome = CoreFacade {
        state: &mut state,
        consumers: &mut port,
        policy: &policy,
        evidence: ApplyEvidenceV1::default(),
    }
    .start_goal(&context, &proposal, &PORT, "2".into())
    .unwrap();
    assert!(matches!(outcome, super::StartGoalOutcomeV1::Accepted(_)));
    assert_eq!(port.compiled_specs, vec![b"spec".to_vec()]);
    assert!(port.events.contains(&GoalLifecycleEventV1::GoalCreated));
    assert!(port.events.contains(&GoalLifecycleEventV1::GoalDecomposed));
}

fn submitted_code(port: &mut Controlled) {
    use serde_json::json;
    let set = |from: &str, to: &str, cause: serde_json::Value| json!({"op":"SET_NODE_STATE","node_id":"n","detail":{"from":from,"to":to,"cause":cause}});
    apply_control(
        port,
        json!([
            set("PLANNED", "READY", json!({"kind":"DEPENDENCIES_SATISFIED"})),
            set(
                "READY",
                "ADMITTED",
                json!({"kind":"ADMISSION","admission_decision_id":"ad"})
            ),
            set(
                "ADMITTED",
                "DISPATCHED",
                json!({"kind":"DISPATCH","attempt_id":"at1"})
            ),
            set(
                "DISPATCHED",
                "RUNNING",
                json!({"kind":"START","attempt_id":"at1"})
            ),
            set(
                "RUNNING",
                "AWAITING_REVIEW",
                json!({"kind":"SUBMIT","attempt_id":"at1"})
            )
        ]),
    );
    port.results = port
        .results
        .admit_attempt_code_sha("p", "g", "n", "at1", &"b".repeat(40))
        .unwrap();
}
#[test]
fn free_quality_gate_checks_admitted_final_sha_and_retains_failed_result() {
    for pass in [false, true] {
        let (_dir, mut state) = seed("2");
        let mut port = Controlled::new(GraphNodeState::Planned);
        let policy = policy();
        submitted_code(&mut port);
        port.gate_pass = pass;
        let outcome = CoreFacade {
            state: &mut state,
            consumers: &mut port,
            policy: &policy,
            evidence: ApplyEvidenceV1::default(),
        }
        .tick("p", "goal")
        .unwrap();
        assert_eq!(port.checked_shas, vec!["b".repeat(40)]);
        assert_eq!(
            port.review_gates,
            vec![if pass {
                super::GoalEvaluationState::Complete
            } else {
                super::GoalEvaluationState::Incomplete
            }]
        );
        assert_ne!(outcome.state(), super::GoalEvaluationState::Complete);
        assert_eq!(
            port.graph.graph().node_state("n"),
            Some(GraphNodeState::AwaitingReview)
        );
        assert_eq!(port.launches, 0);
    }
}

struct GateReview;
impl ReviewEvidenceSource for GateReview {
    fn review_record(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<Option<ReviewEvidenceRecord>, ReviewSourceUnavailable> {
        use sha2::{Digest, Sha256};
        Ok(Some(ReviewEvidenceRecord {
            project_id: "p".into(),
            graph_id: "g".into(),
            review_id: "gate".into(),
            subject_node_id: "n".into(),
            subject_attempt_id: "at1".into(),
            code_sha: Some("b".repeat(40)),
            round: None,
            outcome: ReviewOutcome::Pass,
            content_sha256: Sha256::digest(b"gate evidence")
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            provenance: ReviewProvenance::Admitted {
                admission_id: "admitted-gate".into(),
            },
        }))
    }
    fn review_content(&self, _: &str) -> Result<Option<Vec<u8>>, ReviewSourceUnavailable> {
        Ok(Some(b"gate evidence".to_vec()))
    }
}
#[test]
fn a_passing_review_proposal_cannot_override_a_failed_free_check() {
    use serde_json::json;
    for pass in [false, true] {
        let (_dir, mut state) = seed("2");
        let mut port = Controlled::new(GraphNodeState::Planned);
        let policy = policy();
        submitted_code(&mut port);
        port.gate_pass = pass;
        let proposal = change(
            &port,
            json!([
                {"op":"ATTACH_RESULT","node_id":"n","detail":{"result_ref":"result","result":{"node_id":"n","graph_id":"g","outcome":"PASS","attempt_id":"at1","review_id":"gate","code_sha":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","produced_at":"2026-11-08T00:00:00Z"}}},
                {"op":"SET_NODE_STATE","node_id":"n","detail":{"from":"AWAITING_REVIEW","to":"PASSED","cause":{"kind":"REVIEW_VERDICT","result_node_id":"n","review_id":"gate"}}}
            ]),
        );
        let evidence = ApplyEvidenceV1 {
            interface: None,
            review: Some(ReviewEvidenceVerifier::new(&GateReview)),
        };
        // Establish that the ordinary admitted review path allows the proposal.
        assert!(
            apply_graph_mutation_with_evidence_v1(
                &port.graph,
                &port.results,
                &proposal.mutation,
                &proposal.scope,
                evidence
            )
            .is_ok()
        );
        port.changes.push(proposal);
        let outcome = CoreFacade {
            state: &mut state,
            consumers: &mut port,
            policy: &policy,
            evidence,
        }
        .tick("p", "goal")
        .unwrap();
        assert_eq!(
            port.graph.graph().node_state("n"),
            Some(if pass {
                GraphNodeState::Passed
            } else {
                GraphNodeState::AwaitingReview
            })
        );
        assert_ne!(outcome.state(), super::GoalEvaluationState::Complete);
        assert_eq!(port.checked_shas, vec!["b".repeat(40)]);
    }
}
