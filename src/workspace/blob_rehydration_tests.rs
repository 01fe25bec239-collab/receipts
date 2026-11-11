use receipts_state::*;

use crate::{
    GitBlobMaterializer,
    test_support::{TempDir, TestRepo},
};
const PRIOR_DIGEST: &str =
    "sha256:v1:0000000000000000000000000000000000000000000000000000000000000000";
fn state_epoch(n: i64) -> StateEpochValueV1 {
    StateEpochValueV1::try_from(n.to_string().as_str()).unwrap()
}
fn admit(store: &mut TrustedCoreStore, operation: StateWriteOperationV1) -> StateWriteAdmission {
    store.admit(StateWriteSubjectV1 {
        operation,
        project_id: "project-1".into(),
        graph_id: None,
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    })
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

fn seed(repo: &mut TrustedCoreStore, sources: Vec<ContextManifestSource>) {
    let a = admit(repo, StateWriteOperationV1::LogicalRole);
    repo.create_logical_role(
        a,
        LogicalRole {
            role_id: "role-1".to_string(),
            project_id: "project-1".to_string(),
            role_type: LogicalRoleType::RuntimeA2,
            status: LogicalRoleStatus::Active,
            current_context_epoch: state_epoch(0),
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
            project_id: "project-1".to_string(),
            epoch: state_epoch(0),
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
            project_id: "project-1".to_string(),
            epoch: state_epoch(0),
            sources,
            created_at: "2026-08-17T09:00:00Z".to_string(),
            last_rehydrated_at: None,
        },
    )
    .expect("manifest");
}

fn source(
    ref_type: ContextSourceRefType,
    target: &str,
    source_class: SourceClass,
) -> ContextManifestSource {
    ContextManifestSource {
        r#ref: ContextSourceRef {
            ref_type,
            target: target.to_string(),
        },
        source_class,
        digest: PRIOR_DIGEST.to_string(),
        last_read_at: None,
        required_for: Vec::new(),
    }
}

fn request(bindings: Vec<ContextSourceBindingV1>) -> ContextRehydrationRequest {
    ContextRehydrationRequest {
        rehydration_attempt_id: "attempt-1".to_string(),
        project_id: "project-1".to_string(),
        durable_role_id: "role-1".to_string(),
        context_manifest_id: "manifest-1".to_string(),
        context_epoch_id: state_epoch(0),
        requested_by_actor: EventActor {
            kind: ActorKind::System,
            id: Some("orchestrator".to_string()),
        },
        executor_binding_id: None,
        session_reference: None,
        task_id: Some("task-1".to_string()),
        correlation_reference: Some("correlation-1".to_string()),
        trigger_kind: ContextEpochTrigger::HostSwitch,
        trigger_reference: Some("host-switch-1".to_string()),
        started_at: "2026-08-17T10:00:00Z".to_string(),
        completed_at: "2026-08-17T10:00:02Z".to_string(),
        source_bindings: bindings,
        touch_evidence: Vec::new(),
        demands: Vec::new(),
    }
}

#[test]
fn real_git_rehydration_persists_success_and_refuses_without_partial_context() {
    let git = TestRepo::new("rehydrate-git");
    git.commit_file("source", "immutable bytes\n");
    let sha = git.commit_file("second", "second bytes\n");
    let oversized = git.commit_file("second", &"x".repeat(1_048_577));
    let db = TempDir::new("rehydrate-state");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(db.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut store = TrustedCoreStore::open_exclusive(db.path().join("state.db")).unwrap();
    seed(
        &mut store,
        vec![
            source(
                ContextSourceRefType::RepoPath,
                "source",
                SourceClass::Mandatory,
            ),
            source(
                ContextSourceRefType::RepoPath,
                "second",
                SourceClass::Mandatory,
            ),
        ],
    );
    for (i, commit) in [sha.clone(), "f".repeat(40), oversized]
        .into_iter()
        .enumerate()
    {
        let bindings = [("source", sha.clone()), ("second", commit)]
            .into_iter()
            .enumerate()
            .map(
                |(source_ordinal, (path, commit_sha))| ContextSourceBindingV1 {
                    source_ordinal,
                    source_id: path.into(),
                    source_ref: BoundContextSourceRefV1::RepositorySnapshot(
                        RepositorySnapshotRefV1 {
                            project_id: "project-1".into(),
                            repository_id: "repo-1".into(),
                            commit_sha,
                            logical_relative_path: path.into(),
                        },
                    ),
                },
            )
            .collect();
        let mut input = request(bindings);
        input.rehydration_attempt_id = format!("read-{i}");
        let a = admit(&mut store, StateWriteOperationV1::ContextRehydration);
        let mut reader =
            GitBlobMaterializer::new(git.path(), "project-1", "repo-1", "2026-08-17T10:00:01Z")
                .unwrap();
        let outcome = store
            .rehydrate_context(a, input, &mut reader, &mut NoArtifacts, &mut EventSupplier)
            .unwrap();
        if i == 0 {
            assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Succeeded);
            assert_eq!(outcome.sources[0].bytes, b"immutable bytes\n");
            assert_eq!(outcome.sources[1].bytes, b"second bytes\n");
        } else {
            assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
            assert!(outcome.sources.is_empty());
        }
    }
    drop(store);
    let store = TrustedCoreStore::open_exclusive(db.path().join("state.db")).unwrap();
    assert_eq!(
        store
            .reader()
            .find_context_rehydration_attempt("project-1", "read-0")
            .unwrap()
            .unwrap()
            .status,
        ContextRehydrationStatus::Succeeded
    );
    assert_eq!(
        store
            .reader()
            .find_context_rehydration_attempt("project-1", "read-1")
            .unwrap()
            .unwrap()
            .status,
        ContextRehydrationStatus::Failed
    );
}
