//! Controlled trusted producer fixtures; no production issuer, process or reset.
use crate::recovery_tests::{PROJECT, T_AFTER, T_BEFORE, T_BOUND, T_DEADLINE, T_LATER, binding};
use crate::tests::{TempDir, trusted_clock_at};
use crate::write_fault::with_fault;
use crate::*;
use std::os::unix::fs::PermissionsExt;
const ACCEPTED: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const CRASHED: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const FRESH: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
fn subject(id: &str) -> WorkspaceRecoverySubjectV1 {
    WorkspaceRecoverySubjectV1 {
        project_id: PROJECT.into(),
        graph_id: "graph".into(),
        attempt_id: id.into(),
    }
}
fn grant(store: &TrustedCoreStore, op: StateWriteOperationV1, graph: bool) -> StateWriteAdmission {
    store.admit(StateWriteSubjectV1 {
        operation: op,
        project_id: PROJECT.into(),
        graph_id: graph.then(|| "graph".into()),
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    })
}
fn recovery_grant(store: &TrustedCoreStore) -> StateWriteAdmission {
    grant(store, StateWriteOperationV1::WorkspaceRecovery, true)
}
fn start(id: &str, sha: &str) -> AttemptStart {
    AttemptStart::new(
        AttemptId::parse(id).unwrap(),
        PROJECT,
        "graph",
        "n2",
        "1",
        AttemptRole::RuntimeA3,
        "runtime-r",
        "workspace",
        sha,
        T_BOUND,
    )
    .unwrap()
}
fn append_start(store: &mut TrustedCoreStore, id: &str, sha: &str) -> Result<(), StateError> {
    let g = grant(store, StateWriteOperationV1::Attempt, true);
    store.append_attempt_started(g, start(id, sha))
}
fn finish(store: &mut TrustedCoreStore, id: &str, sha: &str) -> Result<(), StateError> {
    let g = grant(store, StateWriteOperationV1::Attempt, true);
    store.append_attempt_finished(
        g,
        AttemptFinish::new(
            AttemptId::parse(id).unwrap(),
            T_BOUND,
            Some(sha),
            None,
            None,
        )
        .unwrap(),
    )
}
fn reason<T>(result: Result<T, StateError>) -> WorkspaceRecoveryRefusalV1 {
    match result {
        Err(StateError::WorkspaceRecoveryRefused { reason }) => reason,
        Err(e) => panic!("wrong error {e:?}"),
        Ok(_) => panic!("unexpected success"),
    }
}
fn checkpoint(
    store: &mut TrustedCoreStore,
    id: &str,
    attempt: &str,
    sha: &str,
    kind: &str,
) -> String {
    let mut value = serde_json::json!({"checkpoint_id":id,"workspace_id":"workspace","attempt_id":attempt,"head_sha":sha,"kind":kind,"captured_at":T_BOUND,"modified_files":[],"untracked_files":[]});
    if kind == "RECOVERY_CAPTURE" {
        value["captured_at"] = serde_json::json!(T_LATER);
        value["dirty_diff_ref"] =
            serde_json::json!({"ref_type":"ARTIFACT_ID","target":id,"digest":"3".repeat(64)});
    }
    let cp = CheckpointRecord::parse_json(PROJECT, &value.to_string()).unwrap();
    let json = cp.to_json().unwrap();
    let g = grant(store, StateWriteOperationV1::Checkpoint, false);
    store.append_checkpoint(g, cp).unwrap();
    json
}
struct Fixture {
    _tmp: TempDir,
    store: TrustedCoreStore,
    anchor: WorkspaceAcceptedTargetV1,
    admission: WorkspaceRecoveryAdmissionV1,
    hash: String,
}
impl Fixture {
    fn new() -> Self {
        Self::with_window(T_LATER)
    }
    fn with_window(window: &str) -> Self {
        let (tmp, repo) =
            crate::recovery_tests::seeded("d4", LogicalRoleType::RuntimeA2, "sha256:v1:00");
        drop(repo);
        std::fs::set_permissions(
            tmp.db_path().parent().unwrap(),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        std::fs::set_permissions(tmp.db_path(), std::fs::Permissions::from_mode(0o600)).unwrap();
        let mut store = TrustedCoreStore::open_exclusive(tmp.db_path()).unwrap();
        let graph = crate::graph_repository_tests::request(PROJECT, "graph");
        let provenance = graph.genesis_provenance();
        let g = store.admit(StateWriteSubjectV1 {
            operation: StateWriteOperationV1::GraphGenesis,
            project_id: PROJECT.into(),
            graph_id: Some("graph".into()),
            source_ref: Some(provenance.source_ref().as_str().into()),
            compiler_id: Some(provenance.compiler_id().as_str().into()),
            policy_id: Some("policy".into()),
        });
        store
            .create_initial_graph_v1(g, &trusted_clock_at(T_BOUND), graph)
            .unwrap();
        let definition = GoalDefinitionV1 {
            project_id: PROJECT.into(),
            goal_id: "goal".into(),
            graph_id: "graph".into(),
            spec_ref: "artifact:spec".into(),
            spec_sha256: "a".repeat(64),
            dispatch_limit: "4".into(),
        };
        let g = grant(&store, StateWriteOperationV1::Goal, true);
        store.create_goal_v1(g, &definition, "0").unwrap();

        append_start(&mut store, ACCEPTED, &"a".repeat(40)).unwrap();
        checkpoint(
            &mut store,
            "accepted-cp",
            ACCEPTED,
            &"b".repeat(40),
            "PROGRESS",
        );
        let finalization = FinalizationRecordV1 {
            project_id: PROJECT.into(),
            graph_id: "graph".into(),
            attempt_id: ACCEPTED.into(),
            workspace_id: "workspace".into(),
            checkpoint_id: "accepted-cp".into(),
            final_sha: "b".repeat(40),
            write_scope_verdict: "PASS".into(),
            check_results: vec![],
            output_refs: vec![],
        };
        let finish = AttemptFinish::new(
            AttemptId::parse(ACCEPTED).unwrap(),
            T_BOUND,
            Some(&"b".repeat(40)),
            None,
            None,
        )
        .unwrap();
        let g = grant(&store, StateWriteOperationV1::Finalization, true);
        store
            .finish_with_finalization_v1(g, &finalization, &finish)
            .unwrap();
        let json = serde_json::json!({"decision_id":"accept","request_id":"request","outcome":"ACCEPT","checks_evaluated":[{"check":"fixture","result":"PASS"}],"provenance_chain":[{"task_id":"n2","start_sha":"a".repeat(40),"implementation_sha":"b".repeat(40),"review_sha":"b".repeat(40),"acceptance_sha":"b".repeat(40)}],"decided_at":T_BOUND});
        let decision = DecisionRecordV1::parse(
            PROJECT,
            DecisionRecordKind::IntegrationDecision,
            "accept",
            &json.to_string(),
        )
        .unwrap();
        let settlement = ReviewClaimSettlementV1::completed(
            "ACCEPTANCE_EVALUATED",
            Some("accept"),
            Some(decision.record_sha256()),
        )
        .unwrap();
        let request = ReviewClaimRequestV1::new(
            PROJECT,
            "acceptance-claim",
            "n2",
            Some(&"b".repeat(40)),
            &"c".repeat(64),
        )
        .unwrap();
        let g = grant(&store, StateWriteOperationV1::ReviewClaim, false);
        let ticket = match store
            .claim_review_v1(g, &trusted_clock_at(T_BOUND), &request)
            .unwrap()
        {
            ReviewClaimOutcomeV1::Claimed(t) => t,
            _ => panic!(),
        };
        let package = DecisionPublicationV1::new(
            PROJECT,
            "acceptance-publication",
            vec![decision],
            vec![],
            vec![],
        )
        .unwrap()
        .with_claim_settlement(&ticket, settlement)
        .unwrap();
        let data = b"controlled trusted acceptance producer";
        let seal_request = ReviewSealRequestV1::new(
            &request,
            &package,
            ReviewRecoveryDataV1::new(
                "D4_FIXTURE_V1",
                data.to_vec(),
                &crate::review_claim::sha256_hex(data),
            )
            .unwrap(),
        )
        .unwrap();
        let g = grant(&store, StateWriteOperationV1::ReviewClaim, false);
        let seal = store
            .seal_review_claim_v1(g, &trusted_clock_at(T_BOUND), &ticket, &seal_request)
            .unwrap();
        let g = grant(&store, StateWriteOperationV1::DecisionPublication, false);
        // Rebuild from the persisted decision bytes for the restart publication path.
        let decision = DecisionRecordV1::parse(
            PROJECT,
            DecisionRecordKind::IntegrationDecision,
            "accept",
            &json.to_string(),
        )
        .unwrap();
        let published = DecisionPublicationV1::new(
            PROJECT,
            "acceptance-publication",
            vec![decision],
            vec![],
            vec![],
        )
        .unwrap()
        .with_sealed_claim(&seal)
        .unwrap();
        store
            .publish_decision_package_v1(g, &trusted_clock_at(T_BOUND), &published)
            .unwrap();
        let anchor = WorkspaceAcceptedTargetV1 {
            subject: subject(ACCEPTED),
            anchor_id: "anchor".into(),
            workspace_id: "workspace".into(),
            accepted_commit: "b".repeat(40),
            accepted_tree: "d".repeat(40),
            acceptance_claim_id: "acceptance-claim".into(),
            decision_id: "accept".into(),
            provisioning_identity: "e".repeat(64),
        };
        let g = recovery_grant(&store);
        store.bind_workspace_acceptance_v1(g, &anchor).unwrap();
        let dispatch = GoalDispatchV1 {
            project_id: PROJECT.into(),
            goal_id: "goal".into(),
            dispatch_id: "dispatch".into(),
            expected_revision: "0".into(),
            context_epoch: "0".into(),
            graph_version: "1".into(),
        };
        let g = grant(&store, StateWriteOperationV1::GoalDispatch, true);
        store
            .reserve_goal_dispatch_v1(g, &dispatch, &start(CRASHED, &anchor.accepted_commit))
            .unwrap();
        let g = grant(&store, StateWriteOperationV1::GoalDispatch, true);
        store
            .resolve_goal_dispatch_v1(
                g,
                PROJECT,
                "dispatch",
                LaunchResolutionV1::Launched,
                "fixture:launch",
            )
            .unwrap();
        let admission = WorkspaceRecoveryAdmissionV1 {
            subject: subject(CRASHED),
            workspace_id: "workspace".into(),
            role_id: "role-1".into(),
            binding_id: "b1".into(),
            lease_deadline: T_DEADLINE.into(),
            expected_execution_deadline: window.into(),
            context_epoch: "0".into(),
            provisioning_identity: "f".repeat(64),
            supervisor_identity: "1".repeat(64),
            accepted_anchor_id: "anchor".into(),
        };
        let g = recovery_grant(&store);
        let hash = store
            .admit_workspace_recovery_v1(g, &trusted_clock_at(T_BEFORE), &admission)
            .unwrap();
        Self {
            _tmp: tmp,
            store,
            anchor,
            admission,
            hash,
        }
    }
    fn handoff(
        &mut self,
        d: WorkspaceHandoffDispositionV1,
        revision: i64,
    ) -> Result<i64, StateError> {
        let h = WorkspaceHandoffRecordV1 {
            subject: subject(CRASHED),
            expected_revision: revision,
            disposition: d,
            evidence_ref: "fixture:handoff".into(),
            evidence_sha256: "2".repeat(64),
            provisioning_identity: self.admission.provisioning_identity.clone(),
        };
        let g = recovery_grant(&self.store);
        let at = self
            .store
            .reader()
            .find_trusted_time_watermark(PROJECT)
            .unwrap()
            .map(|w| w.last_accepted_trusted_time)
            .unwrap_or_else(|| T_BEFORE.into());
        self.store
            .record_workspace_handoff_v1(g, &trusted_clock_at(&at), &h)
    }
    fn dispatch(&mut self, id: &str) -> Result<SetupWriteOutcomeV1, StateError> {
        let request = GoalDispatchV1 {
            project_id: PROJECT.into(),
            goal_id: "goal".into(),
            dispatch_id: format!("dispatch-{id}"),
            expected_revision: "1".into(),
            context_epoch: "0".into(),
            graph_version: "1".into(),
        };
        let g = grant(&self.store, StateWriteOperationV1::GoalDispatch, true);
        self.store
            .reserve_goal_dispatch_v1(g, &request, &start(id, &self.anchor.accepted_commit))
    }
    fn claim_at(&mut self, time: &str, id: &str) -> Result<WorkspaceRecoveryRecordV1, StateError> {
        let g = recovery_grant(&self.store);
        self.store.claim_workspace_recovery_v1(
            g,
            &trusted_clock_at(time),
            &WorkspaceRecoveryClaimRequestV1 {
                subject: subject(CRASHED),
                recovery_id: id.into(),
                admission_sha256: self.hash.clone(),
            },
        )
    }
    fn claim(&mut self) -> WorkspaceRecoveryRecordV1 {
        self.handoff(WorkspaceHandoffDispositionV1::ClosedWithoutHandoff, 0)
            .unwrap();
        self.claim_at(T_LATER, "recovery").unwrap()
    }
    fn auth(&self, revision: i64) -> WorkspaceRecoveryAuthorityV1 {
        WorkspaceRecoveryAuthorityV1 {
            subject: subject(CRASHED),
            recovery_id: "recovery".into(),
            expected_revision: revision,
            provisioning_identity: self.admission.provisioning_identity.clone(),
            supervisor_identity: self.admission.supervisor_identity.clone(),
        }
    }
    fn evidence(&mut self, id: &str, sha: &str) -> WorkspaceRecoveryEvidenceV1 {
        let json = checkpoint(&mut self.store, id, CRASHED, sha, "RECOVERY_CAPTURE");
        let g = grant(&self.store, StateWriteOperationV1::OutputRef, false);
        self.store
            .append_output_ref(
                g,
                OutputRef::new(PROJECT, id, &"3".repeat(64), "artifact:fixture", "0").unwrap(),
            )
            .unwrap();
        WorkspaceRecoveryEvidenceV1 {
            recovery_id: "recovery".into(),
            checkpoint_id: id.into(),
            checkpoint_sha256: crate::review_claim::sha256_hex(json.as_bytes()),
            output_refs: vec![id.into()],
        }
    }
    fn prepare_reset(
        &mut self,
    ) -> (
        WorkspaceRecoveryAuthorityV1,
        WorkspaceRecoveryResetObservationV1,
    ) {
        self.claim();
        let evidence = self.evidence("pre-reset", &"b".repeat(40));
        let g = recovery_grant(&self.store);
        self.store
            .acknowledge_workspace_recovery_v1(
                g,
                &trusted_clock_at(T_LATER),
                &self.auth(0),
                WorkspaceRecoveryEvidenceStageV1::Captured,
                &evidence,
            )
            .unwrap();
        let g = recovery_grant(&self.store);
        self.store
            .acknowledge_workspace_recovery_v1(
                g,
                &trusted_clock_at(T_LATER),
                &self.auth(1),
                WorkspaceRecoveryEvidenceStageV1::Quiescent,
                &evidence,
            )
            .unwrap();
        let g = recovery_grant(&self.store);
        let auth = self.auth(2);
        let guard = self
            .store
            .begin_workspace_recovery_action_v1(
                g,
                &trusted_clock_at(T_LATER),
                &auth,
                WorkspaceRecoveryActionKindV1::Reset,
            )
            .unwrap();
        assert_eq!(guard.revision(), 3);
        drop(guard);
        (
            self.auth(3),
            WorkspaceRecoveryResetObservationV1 {
                evidence,
                observed_commit: self.anchor.accepted_commit.clone(),
                index_tree: self.anchor.accepted_tree.clone(),
                tracked_tree: self.anchor.accepted_tree.clone(),
            },
        )
    }
}

#[test]
fn workspace_recovery_expiry_handoff_and_current_identity_refuse_without_partial_claims() {
    let mut f = Fixture::new();
    assert_eq!(
        reason(f.claim_at(T_LATER, "recovery")),
        WorkspaceRecoveryRefusalV1::HandoffUnavailable
    );
    // Refused time sampling rolls back too, permitting this earlier trusted sample.
    f.handoff(WorkspaceHandoffDispositionV1::Pending, 0)
        .unwrap();
    assert_eq!(
        reason(f.claim_at(T_LATER, "recovery")),
        WorkspaceRecoveryRefusalV1::HandoffUnavailable
    );
    f.handoff(WorkspaceHandoffDispositionV1::ClosedWithoutHandoff, 1)
        .unwrap();
    assert_eq!(
        reason(f.claim_at(T_BEFORE, "recovery")),
        WorkspaceRecoveryRefusalV1::NotExpired
    );
    assert_eq!(
        reason(f.claim_at(T_DEADLINE, "recovery")),
        WorkspaceRecoveryRefusalV1::NotExpired,
        "window has not expired"
    );
    assert_eq!(
        reason(f.claim_at(T_AFTER, "recovery")),
        WorkspaceRecoveryRefusalV1::NotExpired
    );
    assert_eq!(f.claim_at(T_LATER, "recovery").unwrap().revision, 0);
    let mut bad = f.auth(0);
    bad.supervisor_identity = "4".repeat(64);
    let evidence = f.evidence("capture", &"b".repeat(40));
    let g = recovery_grant(&f.store);
    assert_eq!(
        reason(f.store.acknowledge_workspace_recovery_v1(
            g,
            &trusted_clock_at(T_LATER),
            &bad,
            WorkspaceRecoveryEvidenceStageV1::Captured,
            &evidence
        )),
        WorkspaceRecoveryRefusalV1::ForeignEvidence
    );
    let g = recovery_grant(&f.store);
    assert_eq!(
        reason(f.store.begin_workspace_recovery_action_v1(
            g,
            &trusted_clock_at(T_LATER),
            &f.auth(0),
            WorkspaceRecoveryActionKindV1::Reset
        )),
        WorkspaceRecoveryRefusalV1::InvalidPhase
    );
}

#[test]
fn workspace_recovery_acceptance_is_exact_completed_and_immutable() {
    let mut f = Fixture::new();
    for changed in 0..5 {
        let mut t = f.anchor.clone();
        t.anchor_id = format!("bad-{changed}");
        match changed {
            0 => t.workspace_id = "foreign".into(),
            1 => t.accepted_commit = "a".repeat(40),
            2 => t.subject.attempt_id = CRASHED.into(),
            3 => t.acceptance_claim_id = "missing".into(),
            _ => t.subject.graph_id = "foreign".into(),
        };
        let g = recovery_grant(&f.store);
        assert!(f.store.bind_workspace_acceptance_v1(g, &t).is_err());
    }
    append_start(&mut f.store, FRESH, &"a".repeat(40)).unwrap();
    finish(&mut f.store, FRESH, &"b".repeat(40)).unwrap();
    let mut unfinalized = f.anchor.clone();
    unfinalized.subject = subject(FRESH);
    unfinalized.anchor_id = "unfinalized".into();
    let g = recovery_grant(&f.store);
    assert_eq!(
        reason(f.store.bind_workspace_acceptance_v1(g, &unfinalized)),
        WorkspaceRecoveryRefusalV1::MissingEvidence
    );
    let g = recovery_grant(&f.store);
    assert_eq!(
        f.store.bind_workspace_acceptance_v1(g, &f.anchor).unwrap(),
        SetupWriteOutcomeV1::Replayed
    );
    let mut conflicting = f.anchor.clone();
    conflicting.accepted_tree = "a".repeat(40);
    let g = recovery_grant(&f.store);
    assert_eq!(
        reason(f.store.bind_workspace_acceptance_v1(g, &conflicting)),
        WorkspaceRecoveryRefusalV1::ConflictingReplay
    );
}

#[test]
fn workspace_recovery_existing_renewal_completion_rebind_and_dispatch_writers_are_fenced() {
    let mut renewed = Fixture::new();
    renewed
        .handoff(WorkspaceHandoffDispositionV1::ClosedWithoutHandoff, 0)
        .unwrap();
    let g = grant(
        &renewed.store,
        StateWriteOperationV1::ExecutorBinding,
        false,
    );
    renewed
        .store
        .renew_executor_binding_lease(
            g,
            &trusted_clock_at(T_BEFORE),
            "b1",
            crate::recovery_tests::T_LEASE2,
        )
        .unwrap();
    assert_eq!(
        reason(renewed.claim_at(T_LATER, "recovery")),
        WorkspaceRecoveryRefusalV1::StaleAuthority
    );
    let mut completed = Fixture::new();
    completed
        .handoff(WorkspaceHandoffDispositionV1::ClosedWithoutHandoff, 0)
        .unwrap();
    finish(&mut completed.store, CRASHED, &"b".repeat(40)).unwrap();
    assert_eq!(
        reason(completed.claim_at(T_LATER, "recovery")),
        WorkspaceRecoveryRefusalV1::StaleAuthority
    );
    let mut f = Fixture::new();
    f.claim();
    let g = grant(&f.store, StateWriteOperationV1::ExecutorBinding, false);
    assert_eq!(
        reason(f.store.renew_executor_binding_lease(
            g,
            &trusted_clock_at(T_LATER),
            "b1",
            crate::recovery_tests::T_LEASE2
        )),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
    assert_eq!(
        reason(finish(&mut f.store, CRASHED, &"b".repeat(40))),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
    assert_eq!(
        reason(append_start(&mut f.store, FRESH, &"b".repeat(40))),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
    assert_eq!(
        reason(f.dispatch(FRESH)),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
    let g = grant(&f.store, StateWriteOperationV1::ExecutorBinding, false);
    assert_eq!(
        reason(
            f.store
                .release_executor_binding(g, "b1", T_LATER, ReleaseReason::Crash)
        ),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
    let g = grant(&f.store, StateWriteOperationV1::ExecutorBinding, false);
    assert_eq!(
        reason(
            f.store.create_executor_binding(
                g,
                binding("b2", "role-1", crate::recovery_tests::T_LEASE2)
            )
        ),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
    let g = grant(&f.store, StateWriteOperationV1::ContextEpoch, false);
    assert_eq!(
        reason(f.store.append_context_epoch(
            g,
            ContextEpoch {
                project_id: PROJECT.into(),
                epoch: StateEpochValueV1::try_from("1").unwrap(),
                advanced_at: T_LATER.into(),
                trigger: ContextEpochTrigger::ArchitectureChange
            }
        )),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
}

#[test]
fn workspace_recovery_interruption_readback_atomic_resolution_and_terminal_replay() {
    for fail in 1..=3 {
        let mut f = Fixture::new();
        let (auth, observation) = f.prepare_reset();
        let before = crate::atomic_publication_tests::fingerprint(&f.store);
        let before_control = f
            .store
            .reader()
            .find_workspace_recovery_v1(&subject(CRASHED), "recovery")
            .unwrap();
        let g = recovery_grant(&f.store);
        let (result, _) = with_fault(Some(fail), || {
            f.store.resolve_workspace_recovery_v1(
                g,
                &trusted_clock_at(T_LATER),
                &auth,
                &observation,
            )
        });
        assert!(result.is_err());
        assert_eq!(
            crate::atomic_publication_tests::fingerprint(&f.store),
            before
        );
        assert_eq!(
            f.store
                .reader()
                .find_workspace_recovery_v1(&subject(CRASHED), "recovery")
                .unwrap(),
            before_control
        );
    }
    let mut f = Fixture::new();
    let (auth, observation) = f.prepare_reset();
    let path = f._tmp.db_path();
    drop(f.store);
    f.store = TrustedCoreStore::open_exclusive(path).unwrap();
    let mut wrong = observation.clone();
    wrong.tracked_tree = "a".repeat(40);
    let g = recovery_grant(&f.store);
    assert_eq!(
        reason(
            f.store
                .resolve_workspace_recovery_v1(g, &trusted_clock_at(T_LATER), &auth, &wrong)
        ),
        WorkspaceRecoveryRefusalV1::UnverifiedReset
    );
    assert_eq!(
        reason(append_start(&mut f.store, FRESH, &"b".repeat(40))),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
    let g = recovery_grant(&f.store);
    let done = f
        .store
        .resolve_workspace_recovery_v1(g, &trusted_clock_at(T_LATER), &auth, &observation)
        .unwrap();
    assert!(!done.active);
    assert_eq!(done.phase, "RESOLVED");
    let g = recovery_grant(&f.store);
    assert_eq!(
        f.store
            .resolve_workspace_recovery_v1(g, &trusted_clock_at(T_LATER), &auth, &observation)
            .unwrap(),
        done
    );
    assert_eq!(
        reason(f.claim_at(T_LATER, "another-recovery")),
        WorkspaceRecoveryRefusalV1::AlreadyResolved
    );
    assert_eq!(
        reason(finish(&mut f.store, CRASHED, &"b".repeat(40))),
        WorkspaceRecoveryRefusalV1::AlreadyResolved
    );
    assert_eq!(f.dispatch(FRESH).unwrap(), SetupWriteOutcomeV1::Applied);
    assert!(
        !f.store
            .reader()
            .find_attempt(&AttemptId::parse(CRASHED).unwrap())
            .unwrap()
            .unwrap()
            .is_finished()
    );
}

#[test]
fn workspace_recovery_exact_lease_boundary_and_clock_continuity() {
    let mut f = Fixture::with_window("2026-11-08T09:45:00.000000000Z");
    f.handoff(WorkspaceHandoffDispositionV1::ClosedWithoutHandoff, 0)
        .unwrap();
    assert_eq!(
        reason(f.claim_at("2026-11-08T09:59:59.999999999Z", "recovery")),
        WorkspaceRecoveryRefusalV1::NotExpired
    );
    assert!(f.claim_at(T_DEADLINE, "recovery").unwrap().active);
    let before = crate::atomic_publication_tests::fingerprint(&f.store);
    assert!(matches!(
        f.claim_at(T_BEFORE, "recovery"),
        Err(StateError::TrustedClockRegression { .. })
    ));
    assert_eq!(
        before,
        crate::atomic_publication_tests::fingerprint(&f.store)
    );
}

#[test]
fn workspace_recovery_eligible_handoff_and_review_publication_conflict_with_recovery() {
    let mut f = Fixture::new();
    let request = ReviewClaimRequestV1::new(
        PROJECT,
        "worker-review",
        "n2",
        Some(&"c".repeat(40)),
        &"a".repeat(64),
    )
    .unwrap();
    let g = grant(&f.store, StateWriteOperationV1::ReviewClaim, false);
    assert_eq!(
        reason(
            f.store
                .claim_review_v1(g, &trusted_clock_at(T_BEFORE), &request)
        ),
        WorkspaceRecoveryRefusalV1::HandoffUnavailable
    );
    f.handoff(WorkspaceHandoffDispositionV1::Pending, 0)
        .unwrap();
    checkpoint(
        &mut f.store,
        "finished",
        CRASHED,
        &"c".repeat(40),
        "PROGRESS",
    );
    let record = FinalizationRecordV1 {
        project_id: PROJECT.into(),
        graph_id: "graph".into(),
        attempt_id: CRASHED.into(),
        workspace_id: "workspace".into(),
        checkpoint_id: "finished".into(),
        final_sha: "c".repeat(40),
        write_scope_verdict: "PASS".into(),
        check_results: vec![],
        output_refs: vec![],
    };
    let finish = AttemptFinish::new(
        AttemptId::parse(CRASHED).unwrap(),
        T_BEFORE,
        Some(&"c".repeat(40)),
        None,
        None,
    )
    .unwrap();
    let g = grant(&f.store, StateWriteOperationV1::Finalization, true);
    f.store
        .finish_with_finalization_v1(g, &record, &finish)
        .unwrap();
    assert_eq!(
        f.handoff(WorkspaceHandoffDispositionV1::Eligible, 1)
            .unwrap(),
        2
    );
    assert_eq!(
        reason(f.claim_at(T_LATER, "recovery")),
        WorkspaceRecoveryRefusalV1::EligibleHandoff
    );
    let g = grant(&f.store, StateWriteOperationV1::ReviewClaim, false);
    assert!(matches!(
        f.store
            .claim_review_v1(g, &trusted_clock_at(T_BEFORE), &request)
            .unwrap(),
        ReviewClaimOutcomeV1::Claimed(_)
    ));
    let mut active = Fixture::new();
    active.claim();
    assert_eq!(
        reason(active.handoff(WorkspaceHandoffDispositionV1::Eligible, 1)),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
    let g = grant(&active.store, StateWriteOperationV1::ReviewClaim, false);
    assert_eq!(
        reason(
            active
                .store
                .claim_review_v1(g, &trusted_clock_at(T_LATER), &request)
        ),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
}

#[test]
fn workspace_recovery_rebind_first_stales_claim_and_lease_token_cannot_repeat() {
    let mut f = Fixture::new();
    f.handoff(WorkspaceHandoffDispositionV1::ClosedWithoutHandoff, 0)
        .unwrap();
    let before = crate::atomic_publication_tests::fingerprint(&f.store);
    let g = grant(&f.store, StateWriteOperationV1::ExecutorBinding, false);
    let mut replacement = binding("b1", "role-1", T_DEADLINE);
    replacement.provider_id = "foreign-provider".into();
    assert!(matches!(
        f.store.create_executor_binding(g, replacement),
        Err(StateError::ExecutorBindingAlreadyExists { .. })
    ));
    assert_eq!(
        before,
        crate::atomic_publication_tests::fingerprint(&f.store)
    );
    assert!(
        crate::graph_repository::write_transaction(f.store.reader().connection(), |conn| {
            crate::executor_binding::apply_lease_renewal(conn, "b1", T_BEFORE)
        })
        .is_err()
    );
    assert_eq!(
        f.store
            .reader()
            .find_executor_binding("b1")
            .unwrap()
            .unwrap()
            .lease_expires_at,
        T_DEADLINE
    );
    let req = RoleRebindRequestV1 {
        role_id: "role-1".into(),
        expected_binding_id: Some("b1".into()),
        release_current: Some(ReleaseReason::Crash),
        binding: binding("b2", "role-1", crate::recovery_tests::T_LEASE2),
        authority: crate::recovery_tests::authority(),
        reason: None,
    };
    let g = grant(&f.store, StateWriteOperationV1::ExecutorBinding, false);
    f.store
        .rebind_role(g, &trusted_clock_at(T_BEFORE), &req)
        .unwrap();
    assert_eq!(
        reason(f.claim_at(T_LATER, "recovery")),
        WorkspaceRecoveryRefusalV1::StaleAuthority
    );
    let g = grant(&f.store, StateWriteOperationV1::ExecutorBinding, false);
    assert!(
        f.store
            .renew_executor_binding_lease(
                g,
                &trusted_clock_at(T_BEFORE),
                "b1",
                crate::recovery_tests::T_LEASE2
            )
            .is_err(),
        "released identities cannot regain authority"
    );
}

#[test]
fn workspace_recovery_missing_stale_or_foreign_evidence_never_prepares_an_action() {
    let mut f = Fixture::new();
    f.claim();
    let e = f.evidence("capture", &"b".repeat(40));
    for changed in 0..4 {
        let mut bad = e.clone();
        match changed {
            0 => bad.output_refs.clear(),
            1 => bad.checkpoint_sha256 = "0".repeat(64),
            2 => bad.recovery_id = "foreign".into(),
            _ => bad.output_refs = vec!["missing".into()],
        };
        let g = recovery_grant(&f.store);
        assert!(
            f.store
                .acknowledge_workspace_recovery_v1(
                    g,
                    &trusted_clock_at(T_LATER),
                    &f.auth(0),
                    WorkspaceRecoveryEvidenceStageV1::Captured,
                    &bad
                )
                .is_err()
        );
        assert_eq!(
            f.store
                .reader()
                .find_workspace_recovery_v1(&subject(CRASHED), "recovery")
                .unwrap()
                .phase,
            "CLAIMED"
        );
    }
    for (id, captured) in [
        ("stale-capture", T_BEFORE),
        ("future-capture", "2026-11-08T12:00:00.000000000Z"),
    ] {
        let cp = serde_json::json!({
            "checkpoint_id":id,"workspace_id":"workspace","attempt_id":CRASHED,
            "head_sha":"b".repeat(40),"kind":"RECOVERY_CAPTURE","captured_at":captured,
            "modified_files":[],"untracked_files":[],
            "dirty_diff_ref":{"ref_type":"ARTIFACT_ID","target":"capture","digest":"3".repeat(64)}
        });
        let cp = CheckpointRecord::parse_json(PROJECT, &cp.to_string()).unwrap();
        let json = cp.to_json().unwrap();
        let g = grant(&f.store, StateWriteOperationV1::Checkpoint, false);
        f.store.append_checkpoint(g, cp).unwrap();
        let bad = WorkspaceRecoveryEvidenceV1 {
            checkpoint_id: id.into(),
            checkpoint_sha256: crate::review_claim::sha256_hex(json.as_bytes()),
            ..e.clone()
        };
        let g = recovery_grant(&f.store);
        assert_eq!(
            reason(f.store.acknowledge_workspace_recovery_v1(
                g,
                &trusted_clock_at(T_LATER),
                &f.auth(0),
                WorkspaceRecoveryEvidenceStageV1::Captured,
                &bad,
            )),
            WorkspaceRecoveryRefusalV1::StaleAuthority
        );
    }
    let g = recovery_grant(&f.store);
    f.store
        .acknowledge_workspace_recovery_v1(
            g,
            &trusted_clock_at(T_LATER),
            &f.auth(0),
            WorkspaceRecoveryEvidenceStageV1::Captured,
            &e,
        )
        .unwrap();
    let g = recovery_grant(&f.store);
    assert_eq!(
        reason(f.store.begin_workspace_recovery_action_v1(
            g,
            &trusted_clock_at(T_LATER),
            &f.auth(0),
            WorkspaceRecoveryActionKindV1::Signal
        )),
        WorkspaceRecoveryRefusalV1::StaleAuthority
    );
    let auth = f.auth(1);
    let g = recovery_grant(&f.store);
    let guard = f
        .store
        .begin_workspace_recovery_action_v1(
            g,
            &trusted_clock_at(T_LATER),
            &auth,
            WorkspaceRecoveryActionKindV1::Signal,
        )
        .unwrap();
    assert_eq!(guard.revision(), 2);
    drop(guard);
    let path = f._tmp.db_path();
    drop(f.store);
    f.store = TrustedCoreStore::open_exclusive(path).unwrap();
    assert_eq!(
        f.store
            .reader()
            .find_workspace_recovery_v1(&subject(CRASHED), "recovery")
            .unwrap()
            .phase,
        "SIGNAL_PREPARED"
    );
    let g = recovery_grant(&f.store);
    assert_eq!(
        reason(f.store.begin_workspace_recovery_action_v1(
            g,
            &trusted_clock_at(T_LATER),
            &f.auth(2),
            WorkspaceRecoveryActionKindV1::Signal
        )),
        WorkspaceRecoveryRefusalV1::InvalidPhase,
        "never blindly repeat a prepared signal"
    );
    let conn = f.store.reader().connection();
    assert!(
        conn.execute(
            "UPDATE workspace_recovery_control SET revision=revision+1,phase='RESOLVED',active=0",
            []
        )
        .is_err()
    );
}

#[test]
fn workspace_recovery_v26_migration_preserves_rows_rolls_back_and_verifies_guards() {
    let dir = crate::atomic_publication_tests::Dir::new("d4-v26");
    let repo = SqliteStateRepository::open_with_migrations(
        dir.db(),
        &crate::migrations::registered()[..26],
    )
    .unwrap();
    let id = repo.store_identity().unwrap();
    repo.connection().execute("INSERT INTO attempt_started VALUES(?1,'legacy-project','legacy-graph','legacy-node','1','RUNTIME_A3','runtime','workspace',?2,?3)",rusqlite::params![ACCEPTED,"a".repeat(40),T_BOUND]).unwrap();
    let legacy =
        crate::attempt::read_attempt_in_tx(repo.connection(), &AttemptId::parse(ACCEPTED).unwrap())
            .unwrap()
            .unwrap();
    drop(repo);
    std::fs::set_permissions(dir.db(), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        TrustedCoreStore::open_exclusive(dir.db()),
        Err(StateError::SchemaVersionMismatch {
            found: 26,
            supported: 27
        })
    ));
    let conn = rusqlite::Connection::open(dir.db()).unwrap();
    conn.execute_batch("CREATE TRIGGER fail27 BEFORE INSERT ON state_schema_version WHEN NEW.version=27 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(TrustedCoreStore::migrate_exclusive(dir.db()).is_err());
    crate::graph_repository::verify_v26(&conn, GraphPhase::Read).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='workspace_recovery_control'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    conn.execute_batch("DROP TRIGGER fail27").unwrap();
    drop(conn);
    let (store, version) = TrustedCoreStore::migrate_exclusive(dir.db()).unwrap();
    assert_eq!(version, 27);
    assert_eq!(store.store_id(), id);
    assert_eq!(
        store.reader().find_attempt(legacy.attempt_id()).unwrap(),
        Some(legacy)
    );
    drop(store);
    let (store, version) = TrustedCoreStore::migrate_exclusive(dir.db()).unwrap();
    assert_eq!(version, 27);
    drop(store);
    let conn = rusqlite::Connection::open(dir.db()).unwrap();
    conn.execute_batch("DROP TRIGGER workspace_recovery_control_update")
        .unwrap();
    drop(conn);
    assert!(TrustedCoreStore::open_exclusive(dir.db()).is_err());
}

#[test]
#[ignore]
fn workspace_recovery_external_v26_upgrade() {
    let path = std::env::var_os("D4_V26_DB").expect("external archived-v26 fixture path");
    let (store, head) = TrustedCoreStore::migrate_exclusive(path).unwrap();
    assert_eq!(head, 27);
    let row = store
        .reader()
        .find_attempt(&AttemptId::parse(ACCEPTED).unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(row.project_id(), "legacy-project");
    assert_eq!(row.start_sha(), "a".repeat(40));
    println!(
        "preserved legacy attempt; schema={head}; store={}",
        store.store_id()
    );
}

/// Strip only v27 successor objects when constructing historical test databases.
pub(crate) fn remove_v27(conn: &rusqlite::Connection) {
    conn.execute_batch("DROP TABLE IF EXISTS workspace_recovery_resolution; DROP TABLE IF EXISTS workspace_recovery_evidence; DROP TABLE IF EXISTS workspace_recovery_control; DROP TABLE IF EXISTS workspace_recovery_handoff; DROP TABLE IF EXISTS workspace_recovery_admission; DROP TABLE IF EXISTS workspace_recovery_acceptance; DELETE FROM state_schema_version WHERE version=27;").unwrap();
}

#[test]
fn workspace_recovery_conflicting_successor_in_another_workspace_is_fenced() {
    let foreign = AttemptStart::new(
        AttemptId::parse(FRESH).unwrap(),
        PROJECT,
        "graph",
        "n2",
        "2",
        AttemptRole::RuntimeA3,
        "runtime-r",
        "other-workspace",
        &"b".repeat(40),
        T_BEFORE,
    )
    .unwrap();
    let mut f = Fixture::new();
    f.handoff(WorkspaceHandoffDispositionV1::ClosedWithoutHandoff, 0)
        .unwrap();
    let g = grant(&f.store, StateWriteOperationV1::Attempt, true);
    f.store.append_attempt_started(g, foreign.clone()).unwrap();
    assert_eq!(
        reason(f.claim_at(T_LATER, "recovery")),
        WorkspaceRecoveryRefusalV1::StaleAuthority
    );
    let mut active = Fixture::new();
    active.claim();
    let g = grant(&active.store, StateWriteOperationV1::Attempt, true);
    assert_eq!(
        reason(active.store.append_attempt_started(g, foreign)),
        WorkspaceRecoveryRefusalV1::ActiveRecovery
    );
}

#[test]
fn workspace_recovery_evidence_and_action_prepare_failures_retain_custody() {
    for fail in 1..=2 {
        let mut f = Fixture::new();
        f.claim();
        let e = f.evidence("capture", &"b".repeat(40));
        let g = recovery_grant(&f.store);
        let (result, seen) = with_fault(Some(fail), || {
            f.store.acknowledge_workspace_recovery_v1(
                g,
                &trusted_clock_at(T_LATER),
                &f.auth(0),
                WorkspaceRecoveryEvidenceStageV1::Captured,
                &e,
            )
        });
        assert!(result.is_err());
        assert_eq!(seen, fail);
        assert_eq!(
            f.store
                .reader()
                .find_workspace_recovery_v1(&subject(CRASHED), "recovery")
                .unwrap()
                .phase,
            "CLAIMED"
        );
        let g = recovery_grant(&f.store);
        assert_eq!(
            reason(f.store.begin_workspace_recovery_action_v1(
                g,
                &trusted_clock_at(T_LATER),
                &f.auth(0),
                WorkspaceRecoveryActionKindV1::Signal,
            )),
            WorkspaceRecoveryRefusalV1::InvalidPhase
        );
        let g = recovery_grant(&f.store);
        f.store
            .acknowledge_workspace_recovery_v1(
                g,
                &trusted_clock_at(T_LATER),
                &f.auth(0),
                WorkspaceRecoveryEvidenceStageV1::Captured,
                &e,
            )
            .unwrap();
        let auth = f.auth(1);
        let g = recovery_grant(&f.store);
        let (result, seen) = with_fault(Some(fail), || {
            f.store
                .begin_workspace_recovery_action_v1(
                    g,
                    &trusted_clock_at(T_LATER),
                    &auth,
                    WorkspaceRecoveryActionKindV1::Signal,
                )
                .map(|guard| guard.revision())
        });
        assert!(result.is_err());
        assert_eq!(seen, fail);
        assert_eq!(
            f.store
                .reader()
                .find_workspace_recovery_v1(&subject(CRASHED), "recovery")
                .unwrap()
                .phase,
            "CAPTURED"
        );
        assert_eq!(
            reason(append_start(&mut f.store, FRESH, &"b".repeat(40))),
            WorkspaceRecoveryRefusalV1::ActiveRecovery
        );
    }
}
