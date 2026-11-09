//! The three recovery forwarders through the real Core→State path:
//! `recover_startup`, `rebind_role` and `succeed_changed_context_sources`.
//! The Core resolves the real project, role and operation context for every
//! call; State mints nothing here and unknown or foreign subjects are refused
//! before it. Real SQLite stores, a synthetic trusted clock.

use receipts_state::{
    ActorKind, ArtifactMaterializer, ArtifactRefV1, BoundContextSourceRefV1,
    ChangedSourceSuccessionRequestV1, ContextEpoch, ContextEpochTrigger, ContextManifest,
    ContextManifestSource, ContextRehydratedEventSupplier, ContextRehydrationAttempt,
    ContextRehydrationRequest, ContextSourceBindingV1, ContextSourceRef, ContextSourceRefType,
    EventActor, EventEnvelope, EventSubject, EventType, ExecutorBinding,
    ExecutorReleasedLifecycleAuthorityV1, ExternalSourceMaterialization, LogicalRole,
    LogicalRoleStatus, LogicalRoleType, RecoveryRefusalV1, ReleaseReason,
    RepositorySnapshotMaterializer, RepositorySnapshotRefV1, RoleChangeEvidenceV1,
    RoleRebindRequestV1, SourceClass, SourceDigestComparison, SourceMaterializationFailure,
    StateEpochValueV1, StateError, SubjectKind, context_rehydration_event_payload,
    context_source_digest_v1,
};

use super::integration_fixtures::{At, policy, state};
use super::{CoreAdmissionIdentity, CorePolicyRefusal, CoreStateWriter};
use crate::test_support::{COMPILER, Dir, SOURCE};

const BOUND: &str = "2026-11-08T09:00:00.000000000Z";
const DEADLINE: &str = "2026-11-08T10:00:00.000000000Z";
const BEFORE: &str = "2026-11-08T09:30:00.000000000Z";
const AFTER: &str = "2026-11-08T10:05:00.000000000Z";
const LEASE2: &str = "2026-11-08T12:00:00.000000000Z";
const BYTES: &[u8] = b"authoritative architecture v1";

fn e(n: i64) -> StateEpochValueV1 {
    StateEpochValueV1::try_from(n).unwrap()
}

fn artifact(project: &str) -> BoundContextSourceRefV1 {
    BoundContextSourceRefV1::Artifact(ArtifactRefV1 {
        project_id: project.into(),
        artifact_id: "artifact-arch".into(),
    })
}

fn identity(projects: &[&str]) -> CoreAdmissionIdentity {
    projects.iter().fold(
        CoreAdmissionIdentity::new(
            receipts_state::GraphCompilerRefV1::parse(COMPILER).unwrap(),
            "policy".into(),
        )
        .unwrap(),
        |id, project| {
            id.with_project(
                *project,
                receipts_state::GraphSourceEvidenceRefV1::parse(SOURCE).unwrap(),
            )
        },
    )
}

fn authority() -> ExecutorReleasedLifecycleAuthorityV1 {
    ExecutorReleasedLifecycleAuthorityV1 {
        actor: EventActor {
            kind: ActorKind::System,
            id: Some("orchestration-core".into()),
        },
        correlation_id: "recovery-corr".into(),
    }
}

fn binding(id: &str, role: &str, lease: &str) -> ExecutorBinding {
    ExecutorBinding {
        binding_id: id.into(),
        role_id: role.into(),
        provider_id: "provider-p".into(),
        model_id: "model-m".into(),
        runtime_id: "runtime-r".into(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: BOUND.into(),
        lease_expires_at: lease.into(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    }
}

/// Epoch 0, role `role` with a one-source manifest and binding `b1` (lease
/// `DEADLINE`) in `project`, all through the Core.
fn seed(writer: &mut CoreStateWriter, project: &str, role: &str, binding_id: &str) {
    let evidence = RoleChangeEvidenceV1 {
        changed_at: receipts_state::CanonicalTimestampV1::parse(BOUND).unwrap(),
        reason: None,
    };
    writer
        .append_context_epoch(ContextEpoch {
            project_id: project.into(),
            epoch: e(0),
            advanced_at: BOUND.into(),
            trigger: ContextEpochTrigger::A1Init,
        })
        .unwrap();
    let digest = context_source_digest_v1(&artifact(project), project, BYTES).unwrap();
    writer
        .create_logical_role(LogicalRole {
            role_id: role.into(),
            project_id: project.into(),
            role_type: LogicalRoleType::RuntimeA2,
            status: LogicalRoleStatus::Active,
            current_context_epoch: e(0),
            name: None,
            workstream_id: None,
            ownership_paths: Vec::new(),
            integration_branch: None,
            context_manifest_id: Some(format!("manifest-{role}")),
            active_binding_id: None,
            created_at: Some(BOUND.into()),
        })
        .unwrap();
    writer
        .create_context_manifest(ContextManifest {
            manifest_id: format!("manifest-{role}"),
            role_id: role.into(),
            project_id: project.into(),
            epoch: e(0),
            sources: vec![ContextManifestSource {
                r#ref: ContextSourceRef {
                    ref_type: ContextSourceRefType::ArtifactId,
                    target: "artifact-arch".into(),
                },
                source_class: SourceClass::Mandatory,
                digest,
                last_read_at: None,
                required_for: Vec::new(),
            }],
            created_at: BOUND.into(),
            last_rehydrated_at: None,
        })
        .unwrap();
    writer
        .create_executor_binding(binding(binding_id, role, DEADLINE))
        .unwrap();
    writer
        .set_logical_role_active_binding(role, None, Some(binding_id), &evidence)
        .unwrap();
}

fn bare_role(project: &str, role: &str) -> LogicalRole {
    LogicalRole {
        role_id: role.into(),
        project_id: project.into(),
        role_type: LogicalRoleType::RuntimeA2,
        status: LogicalRoleStatus::Active,
        current_context_epoch: e(0),
        name: None,
        workstream_id: None,
        ownership_paths: Vec::new(),
        integration_branch: None,
        context_manifest_id: None,
        active_binding_id: None,
        created_at: Some(BOUND.into()),
    }
}

/// A second project `r` with one role, stored through a core that admits it.
fn seed_foreign_role(writer: &mut CoreStateWriter) {
    writer
        .append_context_epoch(ContextEpoch {
            project_id: "r".into(),
            epoch: e(0),
            advanced_at: BOUND.into(),
            trigger: ContextEpochTrigger::A1Init,
        })
        .unwrap();
    writer
        .create_logical_role(bare_role("r", "role-r"))
        .unwrap();
}

struct Artifacts(Vec<u8>);
impl ArtifactMaterializer for Artifacts {
    fn materialize(
        &mut self,
        _: &ArtifactRefV1,
        accept: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        accept(&self.0)?;
        Ok(ExternalSourceMaterialization {
            materializer_id: "synthetic-artifacts".into(),
            provenance: "synthetic".into(),
            materialized_at: BEFORE.into(),
        })
    }
}

struct NoRepository;
impl RepositorySnapshotMaterializer for NoRepository {
    fn materialize(
        &mut self,
        _: &RepositorySnapshotRefV1,
        _: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        panic!("no repository source in this manifest")
    }
}

struct Supplier(&'static str);
impl ContextRehydratedEventSupplier for Supplier {
    fn event_for(&mut self, attempt: &ContextRehydrationAttempt) -> EventEnvelope {
        EventEnvelope {
            event_id: self.0.into(),
            project_id: attempt.project_id.clone(),
            goal_id: None,
            event_type: EventType::ContextRehydrated,
            actor: attempt.requested_by_actor.clone(),
            subject: EventSubject {
                kind: SubjectKind::Role,
                id: attempt.durable_role_id.clone(),
            },
            occurred_at: attempt.completed_at.clone(),
            payload: context_rehydration_event_payload(attempt).unwrap(),
            correlation_id: "rehydrate-corr".into(),
            epoch: attempt.context_epoch_id.clone(),
        }
    }
}

fn rehydration(
    attempt: &str,
    project: &str,
    role: &str,
    manifest: &str,
    epoch: i64,
    binding_id: &str,
) -> ContextRehydrationRequest {
    ContextRehydrationRequest {
        rehydration_attempt_id: attempt.into(),
        project_id: project.into(),
        durable_role_id: role.into(),
        context_manifest_id: manifest.into(),
        context_epoch_id: e(epoch),
        requested_by_actor: EventActor {
            kind: ActorKind::System,
            id: Some("orchestration-core".into()),
        },
        executor_binding_id: Some(binding_id.into()),
        session_reference: None,
        task_id: None,
        correlation_reference: Some("rehydrate-corr".into()),
        trigger_kind: ContextEpochTrigger::ProviderReplacement,
        trigger_reference: None,
        started_at: BEFORE.into(),
        completed_at: BEFORE.into(),
        source_bindings: vec![ContextSourceBindingV1 {
            source_ordinal: 0,
            source_id: "source-arch".into(),
            source_ref: artifact(project),
        }],
        touch_evidence: Vec::new(),
        demands: Vec::new(),
    }
}

fn rebind(role: &str, expected: &str, next: &str) -> RoleRebindRequestV1 {
    RoleRebindRequestV1 {
        role_id: role.into(),
        expected_binding_id: Some(expected.into()),
        release_current: None,
        binding: binding(next, role, LEASE2),
        authority: authority(),
        reason: Some("failover".into()),
    }
}

fn succession(role: &str, attempt: &str, manifest: &str) -> ChangedSourceSuccessionRequestV1 {
    ChangedSourceSuccessionRequestV1 {
        role_id: role.into(),
        rehydration_attempt_id: attempt.into(),
        successor_manifest_id: manifest.into(),
        trigger: ContextEpochTrigger::ArchitectureChange,
        authority: authority(),
    }
}

fn recovery_refusal<T: std::fmt::Debug>(
    result: Result<T, super::CoreStateWriteError>,
) -> RecoveryRefusalV1 {
    match state(result) {
        StateError::RecoveryRefused { reason, .. } => reason,
        other => panic!("expected a recovery refusal, got {other:?}"),
    }
}

#[test]
fn recover_startup_runs_for_an_admitted_project_and_refuses_a_foreign_one() {
    let dir = Dir::new();
    let mut writer = CoreStateWriter::open(dir.db(), identity(&["p", "r"])).unwrap();
    seed(&mut writer, "p", "role-1", "b1");

    // Valid: the lease is still running, then expired.
    let early = writer
        .recover_startup(&At(BEFORE), "p", &authority())
        .unwrap();
    assert!(early.lease_expired_binding_ids.is_empty());
    let late = writer
        .recover_startup(&At(AFTER), "p", &authority())
        .unwrap();
    assert_eq!(late.lease_expired_binding_ids, ["b1"]);
    assert_eq!(
        late.roles[0].continuation,
        Err(RecoveryRefusalV1::BindingReleased)
    );
    let released = writer
        .reader()
        .find_executor_binding("b1")
        .unwrap()
        .unwrap();
    assert_eq!(released.release_reason, Some(ReleaseReason::LeaseExpired));
    // A repeat finds nothing left to release.
    let repeat = writer
        .recover_startup(&At(AFTER), "p", &authority())
        .unwrap();
    assert!(repeat.lease_expired_binding_ids.is_empty());

    // Unknown to State but admitted: an empty, valid report.
    let empty = writer
        .recover_startup(&At(AFTER), "r", &authority())
        .unwrap();
    assert!(empty.roles.is_empty() && empty.unfinished_attempts.is_empty());
    // Foreign: a project the composition never admitted is refused before
    // State, and the trusted watermark of the store is untouched.
    assert_eq!(
        policy(writer.recover_startup(&At(AFTER), "q", &authority())),
        CorePolicyRefusal::ProjectNotAdmitted
    );
    assert!(
        writer
            .reader()
            .find_trusted_time_watermark("q")
            .unwrap()
            .is_none()
    );
    // A malformed authority is State's refusal and writes nothing.
    let mut bad = authority();
    bad.correlation_id.clear();
    assert!(writer.recover_startup(&At(AFTER), "p", &bad).is_err());
}

#[test]
fn rebind_role_resolves_the_roles_own_project_and_refuses_foreign_or_unknown_roles() {
    let dir = Dir::new();
    let mut writer = CoreStateWriter::open(dir.db(), identity(&["p", "r"])).unwrap();
    seed(&mut writer, "p", "role-1", "b1");
    // A role in a second project, then a reopen that no longer admits it.
    seed_foreign_role(&mut writer);
    drop(writer);
    let mut writer = CoreStateWriter::open(dir.db(), identity(&["p"])).unwrap();

    writer
        .recover_startup(&At(AFTER), "p", &authority())
        .unwrap();
    // Unknown role: refused before State, nothing bound.
    assert_eq!(
        policy(writer.rebind_role(&At(AFTER), &rebind("missing-role", "b1", "b9"))),
        CorePolicyRefusal::UnknownSubject
    );
    // A stored role of a project this core does not admit.
    assert_eq!(
        policy(writer.rebind_role(&At(AFTER), &rebind("role-r", "b1", "b9"))),
        CorePolicyRefusal::ProjectNotAdmitted
    );
    assert!(
        writer
            .reader()
            .find_executor_binding("b9")
            .unwrap()
            .is_none()
    );
    // A wrong expected binding is State's compare-and-swap refusal.
    assert!(
        writer
            .rebind_role(&At(AFTER), &rebind("role-1", "not-current", "b9"))
            .is_err()
    );
    assert!(
        writer
            .reader()
            .find_executor_binding("b9")
            .unwrap()
            .is_none()
    );

    // Valid: the successor binding is created and must rehydrate first.
    let required = writer
        .rebind_role(&At(AFTER), &rebind("role-1", "b1", "b2"))
        .unwrap();
    assert_eq!(required.binding_id, "b2");
    assert_eq!(
        required.context_manifest_id.as_deref(),
        Some("manifest-role-1")
    );
    let role = writer
        .reader()
        .find_logical_role("role-1")
        .unwrap()
        .unwrap();
    assert_eq!(role.active_binding_id.as_deref(), Some("b2"));
    assert!(
        writer
            .reader()
            .find_event(&required.bound_event_id)
            .unwrap()
            .is_some()
    );
}

#[test]
fn changed_source_succession_resolves_the_role_and_refuses_foreign_or_unknown_subjects() {
    let dir = Dir::new();
    let mut writer = CoreStateWriter::open(dir.db(), identity(&["p", "r"])).unwrap();
    seed(&mut writer, "p", "role-1", "b1");
    // A role of project r, then reopen without r.
    seed_foreign_role(&mut writer);
    writer
        .recover_startup(&At(AFTER), "p", &authority())
        .unwrap();
    writer
        .rebind_role(&At(AFTER), &rebind("role-1", "b1", "b2"))
        .unwrap();
    // The source changed since the manifest recorded it.
    let outcome = writer
        .rehydrate_context(
            rehydration("attempt-1", "p", "role-1", "manifest-role-1", 0, "b2"),
            &mut NoRepository,
            &mut Artifacts(b"authoritative architecture v2".to_vec()),
            &mut Supplier("01ARZ3NDEKTSV4RRFFQ69G5FA3"),
        )
        .unwrap();
    assert_eq!(
        outcome.attempt.source_evidence[0].comparison,
        SourceDigestComparison::Changed
    );
    drop(writer);
    let mut writer = CoreStateWriter::open(dir.db(), identity(&["p"])).unwrap();
    let before_epoch = writer
        .reader()
        .find_context_epoch_value("p", &e(1))
        .unwrap();
    assert!(before_epoch.is_none());

    // Unknown role, foreign role and unknown attempt: nothing succeeds.
    assert_eq!(
        policy(writer.succeed_changed_context_sources(
            &At(AFTER),
            &succession("missing-role", "attempt-1", "manifest-x")
        )),
        CorePolicyRefusal::UnknownSubject
    );
    assert_eq!(
        policy(writer.succeed_changed_context_sources(
            &At(AFTER),
            &succession("role-r", "attempt-1", "manifest-x")
        )),
        CorePolicyRefusal::ProjectNotAdmitted
    );
    assert_eq!(
        recovery_refusal(writer.succeed_changed_context_sources(
            &At(AFTER),
            &succession("role-1", "attempt-unknown", "manifest-x")
        )),
        RecoveryRefusalV1::AttemptNotFound
    );
    assert!(
        writer
            .reader()
            .find_context_epoch_value("p", &e(1))
            .unwrap()
            .is_none()
    );
    assert!(
        writer
            .reader()
            .find_context_manifest("manifest-x")
            .unwrap()
            .is_none()
    );

    // Valid: a new epoch, an immutable successor manifest and one event.
    let done = writer
        .succeed_changed_context_sources(
            &At(AFTER),
            &succession("role-1", "attempt-1", "manifest-role-1-e1"),
        )
        .unwrap();
    assert_eq!(done.epoch.epoch, e(1));
    assert_eq!(done.successor_manifest_id, "manifest-role-1-e1");
    let role = writer
        .reader()
        .find_logical_role("role-1")
        .unwrap()
        .unwrap();
    assert_eq!(role.current_context_epoch, e(1));
    assert_eq!(
        role.context_manifest_id.as_deref(),
        Some("manifest-role-1-e1")
    );
    assert!(
        writer
            .reader()
            .find_event(&done.event_id)
            .unwrap()
            .is_some()
    );
    // A repeat is refused: the role already moved on.
    assert_eq!(
        recovery_refusal(writer.succeed_changed_context_sources(
            &At(AFTER),
            &succession("role-1", "attempt-1", "manifest-role-1-e1b")
        )),
        RecoveryRefusalV1::AttemptScopeMismatch
    );
}
