//! ST-W2-1/2 recovery controls: trusted-time expiry, atomic CAS rebind,
//! no dual binding or dual A1, mandatory rehydration, changed-source
//! succession, orphan listing, rollback, busy, corrupt input, reopen and
//! idempotence. Real temporary SQLite stores; synthetic trusted clocks.

use std::time::Duration;

use crate::context_epoch::{ContextEpoch, ContextEpochTrigger};
use crate::context_manifest::{
    ContextManifest, ContextManifestSource, ContextSourceRef, ContextSourceRefType, SourceClass,
};
use crate::context_rehydration::{
    ArtifactMaterializer, ArtifactRefV1, BoundContextSourceRefV1, ContextRehydratedEventSupplier,
    ContextRehydrationAttempt, ContextRehydrationRequest, ContextRehydrationStatus,
    ContextSourceBindingV1, ExternalSourceMaterialization, RepositorySnapshotMaterializer,
    RepositorySnapshotRefV1, SourceDigestComparison, SourceDisposition,
    SourceMaterializationFailure, context_rehydration_event_payload, context_source_digest_v1,
};
use crate::error::StateError;
use crate::event::{ActorKind, EventActor, EventEnvelope, EventSubject, EventType, SubjectKind};
use crate::executor_binding::{ExecutorBinding, ReleaseReason};
use crate::executor_binding_lease_expiry::ExecutorReleasedLifecycleAuthorityV1;
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::logical_role_lifecycle::{RoleChangeEvidenceV1, RoleChangeRefusal};
use crate::recovery::{
    ChangedSourceSuccessionRequestV1, RecoveryRefusalV1 as R, RoleRebindRequestV1, derived_event_id,
};
use crate::repository::SqliteStateRepository;
use crate::tests::{TempDir, state_epoch, trusted_clock_at};
use crate::{AttemptFinish, AttemptId, AttemptRole, AttemptStart, CanonicalTimestampV1};

pub(crate) const PROJECT: &str = "project-1";
pub(crate) const T_BOUND: &str = "2026-11-08T09:00:00.000000000Z";
pub(crate) const T_DEADLINE: &str = "2026-11-08T10:00:00.000000000Z";
pub(crate) const T_BEFORE: &str = "2026-11-08T09:30:00.000000000Z";
pub(crate) const T_AFTER: &str = "2026-11-08T10:05:00.000000000Z";
pub(crate) const T_LATER: &str = "2026-11-08T10:10:00.000000000Z";
pub(crate) const T_LEASE2: &str = "2026-11-08T12:00:00.000000000Z";
pub(crate) const ARTIFACT_BYTES: &[u8] = b"authoritative architecture v1";

pub(crate) fn authority() -> ExecutorReleasedLifecycleAuthorityV1 {
    ExecutorReleasedLifecycleAuthorityV1 {
        actor: EventActor {
            kind: ActorKind::System,
            id: Some("orchestrator-core".into()),
        },
        correlation_id: "recovery-correlation-1".into(),
    }
}

pub(crate) fn binding(id: &str, role: &str, lease: &str) -> ExecutorBinding {
    ExecutorBinding {
        binding_id: id.into(),
        role_id: role.into(),
        provider_id: "provider-p".into(),
        model_id: "model-m".into(),
        runtime_id: "runtime-r".into(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: T_BOUND.into(),
        lease_expires_at: lease.into(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    }
}

pub(crate) fn artifact_ref(id: &str) -> BoundContextSourceRefV1 {
    BoundContextSourceRefV1::Artifact(ArtifactRefV1 {
        project_id: PROJECT.into(),
        artifact_id: id.into(),
    })
}

pub(crate) fn digest_of(id: &str, bytes: &[u8]) -> String {
    context_source_digest_v1(&artifact_ref(id), PROJECT, bytes).unwrap()
}

pub(crate) fn manifest(id: &str, role: &str, epoch: i64, digest: &str) -> ContextManifest {
    ContextManifest {
        manifest_id: id.into(),
        role_id: role.into(),
        project_id: PROJECT.into(),
        epoch: state_epoch(epoch),
        sources: vec![ContextManifestSource {
            r#ref: ContextSourceRef {
                ref_type: ContextSourceRefType::ArtifactId,
                target: "artifact-arch".into(),
            },
            source_class: SourceClass::Mandatory,
            digest: digest.into(),
            last_read_at: None,
            required_for: Vec::new(),
        }],
        created_at: T_BOUND.into(),
        last_rehydrated_at: None,
    }
}

fn evidence(at: &str) -> RoleChangeEvidenceV1 {
    RoleChangeEvidenceV1 {
        changed_at: CanonicalTimestampV1::parse(at).unwrap(),
        reason: None,
    }
}

/// One project at epoch 0 with role `role-1` (`role_type`) bound to `b1`
/// (lease `T_DEADLINE`) and a one-source manifest recording `digest`.
pub(crate) fn seeded(
    tag: &str,
    role_type: LogicalRoleType,
    digest: &str,
) -> (TempDir, SqliteStateRepository) {
    let tmp = TempDir::new(tag);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    repo.append_context_epoch(ContextEpoch {
        project_id: PROJECT.into(),
        epoch: state_epoch(0),
        advanced_at: T_BOUND.into(),
        trigger: ContextEpochTrigger::A1Init,
    })
    .unwrap();
    add_role(&mut repo, "role-1", role_type, digest);
    repo.create_executor_binding(binding("b1", "role-1", T_DEADLINE))
        .unwrap();
    repo.set_logical_role_active_binding("role-1", None, Some("b1"), &evidence(T_BOUND))
        .unwrap();
    (tmp, repo)
}

pub(crate) fn add_role(
    repo: &mut SqliteStateRepository,
    role_id: &str,
    role_type: LogicalRoleType,
    digest: &str,
) {
    let manifest_id = format!("manifest-{role_id}");
    repo.create_logical_role(LogicalRole {
        role_id: role_id.into(),
        project_id: PROJECT.into(),
        role_type,
        status: LogicalRoleStatus::Active,
        current_context_epoch: state_epoch(0),
        name: None,
        workstream_id: None,
        ownership_paths: Vec::new(),
        integration_branch: None,
        context_manifest_id: Some(manifest_id.clone()),
        active_binding_id: None,
        created_at: Some(T_BOUND.into()),
    })
    .unwrap();
    repo.create_context_manifest(manifest(&manifest_id, role_id, 0, digest))
        .unwrap();
}

pub(crate) struct Artifacts(pub(crate) Vec<u8>);
impl ArtifactMaterializer for Artifacts {
    fn materialize(
        &mut self,
        _reference: &ArtifactRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        accept_chunk(&self.0)?;
        Ok(ExternalSourceMaterialization {
            materializer_id: "test-artifacts".into(),
            provenance: "test".into(),
            materialized_at: T_BEFORE.into(),
        })
    }
}

pub(crate) struct NoRepository;
impl RepositorySnapshotMaterializer for NoRepository {
    fn materialize(
        &mut self,
        _reference: &RepositorySnapshotRefV1,
        _accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        panic!("no repository source in this manifest")
    }
}

pub(crate) struct Supplier(pub(crate) &'static str);
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
            correlation_id: "rehydration-correlation".into(),
            epoch: attempt.context_epoch_id.clone(),
        }
    }
}

pub(crate) fn rehydration_request(
    attempt_id: &str,
    manifest_id: &str,
    epoch: i64,
    binding_id: &str,
) -> ContextRehydrationRequest {
    ContextRehydrationRequest {
        rehydration_attempt_id: attempt_id.into(),
        project_id: PROJECT.into(),
        durable_role_id: "role-1".into(),
        context_manifest_id: manifest_id.into(),
        context_epoch_id: state_epoch(epoch),
        requested_by_actor: EventActor {
            kind: ActorKind::System,
            id: Some("orchestrator-core".into()),
        },
        executor_binding_id: Some(binding_id.into()),
        session_reference: None,
        task_id: None,
        correlation_reference: Some("rehydration-correlation".into()),
        trigger_kind: ContextEpochTrigger::ProviderReplacement,
        trigger_reference: None,
        started_at: T_BEFORE.into(),
        completed_at: T_BEFORE.into(),
        source_bindings: vec![ContextSourceBindingV1 {
            source_ordinal: 0,
            source_id: "source-arch".into(),
            source_ref: artifact_ref("artifact-arch"),
        }],
        touch_evidence: Vec::new(),
        demands: Vec::new(),
    }
}

fn rehydrate(
    repo: &mut SqliteStateRepository,
    request: ContextRehydrationRequest,
    bytes: &[u8],
    event_id: &'static str,
) -> ContextRehydrationAttempt {
    repo.rehydrate_context(
        request,
        &mut NoRepository,
        &mut Artifacts(bytes.to_vec()),
        &mut Supplier(event_id),
    )
    .unwrap()
    .attempt
}

fn rebind(
    expected: Option<&str>,
    next: &str,
    release: Option<ReleaseReason>,
) -> RoleRebindRequestV1 {
    RoleRebindRequestV1 {
        role_id: "role-1".into(),
        expected_binding_id: expected.map(str::to_string),
        release_current: release,
        binding: binding(next, "role-1", T_LEASE2),
        authority: authority(),
        reason: Some("failover".into()),
    }
}

fn refusal(error: StateError) -> R {
    match error {
        StateError::RecoveryRefused { reason, .. } => reason,
        other => panic!("expected a recovery refusal, got {other:?}"),
    }
}

fn role_refusal(error: StateError) -> RoleChangeRefusal {
    match error {
        StateError::LogicalRoleChangeRefused { reason, .. } => reason,
        other => panic!("expected a role-change refusal, got {other:?}"),
    }
}

/// Row counts of every table a recovery action can touch.
fn image(repo: &SqliteStateRepository) -> Vec<i64> {
    [
        "executor_binding",
        "logical_role_history",
        "event",
        "context_epoch",
        "context_manifest",
        "context_epoch_invalidated_role",
        "context_epoch_changed_source",
    ]
    .iter()
    .map(|t| repo.count_table_rows(t).unwrap())
    .collect()
}

fn clear(repo: &SqliteStateRepository, at: &str) -> Result<crate::ContinuationClearanceV1, R> {
    repo.continuation_clearance(&trusted_clock_at(at), "role-1")
        .map_err(refusal)
}

#[test]
fn expired_a1_lease_rebinds_without_two_active_a1s() {
    let (_tmp, mut repo) = seeded("st-w2-a1", LogicalRoleType::RuntimeA1, "sha256:v1:00");
    // A second A1 role whose history ended; it must never gain a binding
    // while role-1 holds one.
    add_role(
        &mut repo,
        "role-2",
        LogicalRoleType::RuntimeA1,
        "sha256:v1:00",
    );

    let before = repo
        .recover_startup(&trusted_clock_at(T_BEFORE), PROJECT, &authority())
        .unwrap();
    assert!(
        before.lease_expired_binding_ids.is_empty(),
        "lease still valid"
    );

    let report = repo
        .recover_startup(&trusted_clock_at(T_AFTER), PROJECT, &authority())
        .unwrap();
    assert_eq!(report.lease_expired_binding_ids, vec!["b1".to_string()]);
    let role1 = &report.roles[0];
    assert_eq!(role1.continuation, Err(R::BindingReleased));
    assert!(R::BindingReleased.requires_rebind());
    let b1 = repo.find_executor_binding("b1").unwrap().unwrap();
    assert_eq!(b1.release_reason, Some(ReleaseReason::LeaseExpired));
    assert_eq!(b1.released_at.as_deref(), Some(T_AFTER));
    let released = repo
        .find_event(&derived_event_id(&["EXECUTOR_RELEASED", PROJECT, "b1"]))
        .unwrap()
        .expect("EXECUTOR_RELEASED appended with the release");
    assert_eq!(released.occurred_at, T_AFTER);
    assert_eq!(released.subject.id, "role-1");

    let required = repo
        .rebind_role(&trusted_clock_at(T_AFTER), &rebind(Some("b1"), "b2", None))
        .unwrap();
    assert_eq!(required.binding_id, "b2");
    assert_eq!(
        required.context_manifest_id.as_deref(),
        Some("manifest-role-1")
    );
    let replaced = repo.find_event(&required.bound_event_id).unwrap().unwrap();
    assert_eq!(replaced.event_type, EventType::ExecutorReplaced);
    let role = repo.find_logical_role("role-1").unwrap().unwrap();
    assert_eq!(role.active_binding_id.as_deref(), Some("b2"));
    assert_eq!(
        role.status,
        LogicalRoleStatus::Active,
        "identity and status survive"
    );

    // The other A1 role cannot become a second active A1 authority.
    let snapshot = image(&repo);
    let mut second = rebind(None, "b9", None);
    second.role_id = "role-2".into();
    second.binding.role_id = "role-2".into();
    assert_eq!(
        refusal(
            repo.rebind_role(&trusted_clock_at(T_AFTER), &second)
                .unwrap_err()
        ),
        R::OtherA1Bound
    );
    assert_eq!(image(&repo), snapshot, "refusal writes nothing");
    assert!(repo.find_executor_binding("b9").unwrap().is_none());
}

#[test]
fn session_limit_release_keeps_role_active_and_requires_rehydration() {
    let digest = digest_of("artifact-arch", ARTIFACT_BYTES);
    let (_tmp, mut repo) = seeded("st-w2-s7", LogicalRoleType::RuntimeA2, &digest);
    let required = repo
        .rebind_role(
            &trusted_clock_at(T_BEFORE),
            &rebind(Some("b1"), "b2", Some(ReleaseReason::SessionExhausted)),
        )
        .unwrap();
    let b1 = repo.find_executor_binding("b1").unwrap().unwrap();
    assert_eq!(b1.release_reason, Some(ReleaseReason::SessionExhausted));
    assert!(required.released_event_id.is_some());
    let role = repo.find_logical_role("role-1").unwrap().unwrap();
    assert_eq!(role.status, LogicalRoleStatus::Active);
    assert_eq!(role.role_id, "role-1");

    // Mandatory rehydration before continuation.
    assert_eq!(clear(&repo, T_BEFORE), Err(R::RehydrationRequired));
    // A rehydration recorded for the *old* binding does not clear the new one.
    let stale = rehydrate(
        &mut repo,
        rehydration_request("attempt-old", "manifest-role-1", 0, "b1"),
        ARTIFACT_BYTES,
        "01ARZ3NDEKTSV4RRFFQ69G5FA1",
    );
    assert_eq!(stale.status, ContextRehydrationStatus::Succeeded);
    assert_eq!(clear(&repo, T_BEFORE), Err(R::RehydrationRequired));

    let attempt = rehydrate(
        &mut repo,
        rehydration_request("attempt-new", "manifest-role-1", 0, "b2"),
        ARTIFACT_BYTES,
        "01ARZ3NDEKTSV4RRFFQ69G5FA2",
    );
    assert_eq!(
        attempt.source_evidence[0].disposition,
        SourceDisposition::Reread,
        "the source was genuinely re-read, not summarised"
    );
    let cleared = clear(&repo, T_BEFORE).unwrap();
    assert_eq!(cleared.binding_id, "b2");
    assert_eq!(cleared.rehydration_attempt_id, "attempt-new");
    // The same clearance lapses with the lease.
    assert_eq!(
        clear(&repo, "2026-11-08T12:00:00.000000000Z"),
        Err(R::LeaseExpired)
    );
}

#[test]
fn rebind_refused_while_binding_active() {
    let (_tmp, mut repo) = seeded("st-w2-active", LogicalRoleType::RuntimeA2, "sha256:v1:00");
    let snapshot = image(&repo);
    let error = repo
        .rebind_role(&trusted_clock_at(T_BEFORE), &rebind(Some("b1"), "b2", None))
        .unwrap_err();
    assert!(
        matches!(
            error,
            StateError::ExecutorBindingUnreleasedConflict { ref blocking_binding_id, .. }
                if blocking_binding_id == "b1"
        ),
        "{error:?}"
    );
    // Stale CAS: the pointer is b1, not None.
    assert_eq!(
        role_refusal(
            repo.rebind_role(&trusted_clock_at(T_BEFORE), &rebind(None, "b2", None))
                .unwrap_err()
        ),
        RoleChangeRefusal::StaleExpectation
    );
    // Release requested for an already released binding is refused too.
    assert_eq!(image(&repo), snapshot);
    assert!(repo.find_executor_binding("b2").unwrap().is_none());
    assert_eq!(
        repo.find_logical_role("role-1")
            .unwrap()
            .unwrap()
            .active_binding_id
            .as_deref(),
        Some("b1")
    );
}

#[test]
fn rebind_shape_and_time_refusals_write_nothing() {
    let (_tmp, mut repo) = seeded("st-w2-shape", LogicalRoleType::RuntimeA2, "sha256:v1:00");
    let snapshot = image(&repo);
    let clock = trusted_clock_at(T_BEFORE);
    let mut claims_rehydrated = rebind(Some("b1"), "b2", Some(ReleaseReason::RateLimited));
    claims_rehydrated.binding.rehydration_completed = Some(true);
    assert_eq!(
        refusal(repo.rebind_role(&clock, &claims_rehydrated).unwrap_err()),
        R::BindingShapeInvalid
    );
    let mut short = rebind(Some("b1"), "b2", Some(ReleaseReason::RateLimited));
    short.binding.lease_expires_at = T_BEFORE.into();
    assert_eq!(
        refusal(repo.rebind_role(&clock, &short).unwrap_err()),
        R::LeaseNotAfterNow
    );
    let mut future = rebind(Some("b1"), "b2", Some(ReleaseReason::RateLimited));
    future.binding.bound_at = T_AFTER.into();
    assert_eq!(
        refusal(repo.rebind_role(&clock, &future).unwrap_err()),
        R::BindingShapeInvalid
    );
    assert!(matches!(
        repo.rebind_role(
            &clock,
            &rebind(Some("b1"), "b2", Some(ReleaseReason::LeaseExpired))
        )
        .unwrap_err(),
        StateError::ExecutorBindingValidation { .. }
    ));
    let mut noncanonical = rebind(Some("b1"), "b2", Some(ReleaseReason::RateLimited));
    noncanonical.binding.lease_expires_at = "tomorrow".into();
    assert!(matches!(
        repo.rebind_role(&clock, &noncanonical).unwrap_err(),
        StateError::CanonicalTimestampInvalid { .. }
    ));
    repo.transition_logical_role_status(
        "role-1",
        LogicalRoleStatus::Active,
        LogicalRoleStatus::Suspended,
        &evidence(T_BEFORE),
    )
    .unwrap();
    let snapshot_suspended = image(&repo);
    assert_eq!(
        refusal(
            repo.rebind_role(
                &clock,
                &rebind(Some("b1"), "b2", Some(ReleaseReason::RateLimited))
            )
            .unwrap_err()
        ),
        R::RoleNotActive
    );
    assert_eq!(clear(&repo, T_BEFORE), Err(R::RoleNotActive));
    assert_eq!(image(&repo), snapshot_suspended);
    assert_eq!(
        snapshot_suspended[0], snapshot[0],
        "no binding was ever created"
    );
}

#[test]
fn rebind_failure_rolls_back_the_release_binding_pointer_and_events() {
    let (_tmp, mut repo) = seeded("st-w2-rollback", LogicalRoleType::RuntimeA2, "sha256:v1:00");
    // Occupy the exact event id the rebind would mint last.
    let taken = derived_event_id(&["EXECUTOR_REPLACED", PROJECT, "b2"]);
    repo.append_event(EventEnvelope {
        event_id: taken,
        project_id: PROJECT.into(),
        goal_id: None,
        event_type: EventType::HumanRequired,
        actor: authority().actor,
        subject: EventSubject {
            kind: SubjectKind::Role,
            id: "role-1".into(),
        },
        occurred_at: T_BEFORE.into(),
        payload: crate::EventPayloadReference {
            reference: "{}".into(),
            digest: "sha256:x".into(),
        },
        correlation_id: "c".into(),
        epoch: state_epoch(0),
    })
    .unwrap();
    let snapshot = image(&repo);
    let watermark = repo.find_trusted_time_watermark(PROJECT).unwrap();
    let error = repo
        .rebind_role(
            &trusted_clock_at(T_BEFORE),
            &rebind(Some("b1"), "b2", Some(ReleaseReason::RateLimited)),
        )
        .unwrap_err();
    assert!(
        matches!(error, StateError::EventAlreadyExists { .. }),
        "{error:?}"
    );
    assert_eq!(
        image(&repo),
        snapshot,
        "release, binding, history and events rolled back"
    );
    let b1 = repo.find_executor_binding("b1").unwrap().unwrap();
    assert!(b1.released_at.is_none() && b1.release_reason.is_none());
    assert!(repo.find_executor_binding("b2").unwrap().is_none());
    assert_eq!(
        repo.find_trusted_time_watermark(PROJECT).unwrap(),
        watermark
    );
}

#[test]
fn recovery_scan_is_idempotent() {
    let (_tmp, mut repo) = seeded("st-w2-idem", LogicalRoleType::RuntimeA2, "sha256:v1:00");
    let first = repo
        .recover_startup(&trusted_clock_at(T_AFTER), PROJECT, &authority())
        .unwrap();
    assert_eq!(first.lease_expired_binding_ids.len(), 1);
    let snapshot = image(&repo);
    let again = repo
        .recover_startup(&trusted_clock_at(T_AFTER), PROJECT, &authority())
        .unwrap();
    assert!(again.lease_expired_binding_ids.is_empty());
    assert_eq!(again.roles, first.roles, "same conclusions");
    let later = repo
        .recover_startup(&trusted_clock_at(T_LATER), PROJECT, &authority())
        .unwrap();
    assert!(later.lease_expired_binding_ids.is_empty());
    assert_eq!(
        image(&repo),
        snapshot,
        "repeat scans change no durable record"
    );
}

#[test]
fn trusted_clock_regression_and_lease_races_are_fenced() {
    let (_tmp, mut repo) = seeded("st-w2-clock", LogicalRoleType::RuntimeA2, "sha256:v1:00");
    repo.recover_startup(&trusted_clock_at(T_BEFORE), PROJECT, &authority())
        .unwrap();
    // A renewal that commits first moves the deadline; the scan then keeps b1.
    repo.renew_executor_binding_lease(&trusted_clock_at(T_BEFORE), "b1", T_LATER)
        .unwrap();
    let report = repo
        .recover_startup(&trusted_clock_at(T_AFTER), PROJECT, &authority())
        .unwrap();
    assert!(report.lease_expired_binding_ids.is_empty());
    // Exactly at the deadline the lease is expired (now >= deadline).
    let at_deadline = repo
        .recover_startup(&trusted_clock_at(T_LATER), PROJECT, &authority())
        .unwrap();
    assert_eq!(
        at_deadline.lease_expired_binding_ids,
        vec!["b1".to_string()]
    );
    // A renewal after expiry cannot reopen the released binding.
    assert!(matches!(
        repo.renew_executor_binding_lease(&trusted_clock_at(T_LATER), "b1", T_LEASE2)
            .unwrap_err(),
        StateError::ExecutorBindingAlreadyReleased { .. }
    ));

    let snapshot = image(&repo);
    let watermark = repo.find_trusted_time_watermark(PROJECT).unwrap();
    assert!(matches!(
        repo.recover_startup(&trusted_clock_at(T_BEFORE), PROJECT, &authority())
            .unwrap_err(),
        StateError::TrustedClockRegression { .. }
    ));
    assert!(matches!(
        repo.continuation_clearance(&trusted_clock_at(T_BEFORE), "role-1")
            .unwrap_err(),
        StateError::TrustedClockRegression { .. }
    ));
    let mut foreign = trusted_clock_at(T_LATER);
    foreign.0.clock_source_id = "other-clock".into();
    assert!(matches!(
        repo.recover_startup(&foreign, PROJECT, &authority())
            .unwrap_err(),
        StateError::TrustedClockContinuityUnbound { .. }
    ));
    assert_eq!(image(&repo), snapshot);
    assert_eq!(
        repo.find_trusted_time_watermark(PROJECT).unwrap(),
        watermark
    );
}

#[test]
fn corrupt_deadline_and_busy_store_refuse_the_whole_scan() {
    let (tmp, mut repo) = seeded("st-w2-corrupt", LogicalRoleType::RuntimeA2, "sha256:v1:00");
    add_role(
        &mut repo,
        "role-2",
        LogicalRoleType::RuntimeA2,
        "sha256:v1:00",
    );
    repo.create_executor_binding(binding("b2", "role-2", "not-a-timestamp"))
        .unwrap();
    let snapshot = image(&repo);
    assert!(matches!(
        repo.recover_startup(&trusted_clock_at(T_AFTER), PROJECT, &authority())
            .unwrap_err(),
        StateError::CanonicalTimestampInvalid { .. }
    ));
    assert_eq!(
        image(&repo),
        snapshot,
        "b1 is not released by a partial scan"
    );
    assert!(repo.find_trusted_time_watermark(PROJECT).unwrap().is_none());

    repo.connection().busy_timeout(Duration::ZERO).unwrap();
    let holder = rusqlite::Connection::open(tmp.db_path()).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(matches!(
        repo.rebind_role(
            &trusted_clock_at(T_BEFORE),
            &rebind(Some("b1"), "b3", Some(ReleaseReason::Crash))
        )
        .unwrap_err(),
        StateError::TransactionBeginFailed { .. }
    ));
    holder.execute_batch("ROLLBACK").unwrap();
    assert_eq!(image(&repo), snapshot);
}

#[test]
fn orphaned_attempt_listed_after_crash() {
    let (tmp, mut repo) = seeded("st-w2-orphan", LogicalRoleType::RuntimeA2, "sha256:v1:00");
    let start = |id: &str, at: &str| {
        AttemptStart::new(
            AttemptId::parse(id).unwrap(),
            PROJECT,
            "graph-1",
            "node-1",
            "1",
            AttemptRole::RuntimeA3,
            "runtime:r",
            "workspace-1",
            &"a".repeat(40),
            at,
        )
        .unwrap()
    };
    repo.append_attempt_started(start("01ARZ3NDEKTSV4RRFFQ69G5FB2", T_BEFORE))
        .unwrap();
    repo.append_attempt_started(start("01ARZ3NDEKTSV4RRFFQ69G5FB1", T_BOUND))
        .unwrap();
    repo.append_attempt_started(start("01ARZ3NDEKTSV4RRFFQ69G5FB3", T_BOUND))
        .unwrap();
    repo.append_attempt_finished(
        AttemptFinish::new(
            AttemptId::parse("01ARZ3NDEKTSV4RRFFQ69G5FB3").unwrap(),
            T_BEFORE,
            None,
            None,
            None,
        )
        .unwrap(),
    )
    .unwrap();
    // Simulated crash: the process is gone; reopen from disk.
    drop(repo);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let ids = |records: &[crate::AttemptRecord]| {
        records
            .iter()
            .map(|r| r.attempt_id().as_str().to_string())
            .collect::<Vec<_>>()
    };
    let expected = vec![
        "01ARZ3NDEKTSV4RRFFQ69G5FB1".to_string(),
        "01ARZ3NDEKTSV4RRFFQ69G5FB2".to_string(),
    ];
    assert_eq!(
        ids(&repo.list_unfinished_attempts(PROJECT).unwrap()),
        expected
    );
    let report = repo
        .recover_startup(&trusted_clock_at(T_AFTER), PROJECT, &authority())
        .unwrap();
    assert_eq!(ids(&report.unfinished_attempts), expected);
    assert!(
        repo.list_unfinished_attempts("project-other")
            .unwrap()
            .is_empty()
    );
    assert!(repo.list_unfinished_attempts("").is_err());
}

#[test]
fn changed_source_succession_then_mandatory_reread_survives_reopen() {
    let old = digest_of("artifact-arch", ARTIFACT_BYTES);
    let (tmp, mut repo) = seeded("st-w2-changed", LogicalRoleType::RuntimeA2, &old);
    repo.recover_startup(&trusted_clock_at(T_AFTER), PROJECT, &authority())
        .unwrap();
    repo.rebind_role(&trusted_clock_at(T_AFTER), &rebind(Some("b1"), "b2", None))
        .unwrap();
    let changed_bytes: &[u8] = b"authoritative architecture v2";
    let attempt = rehydrate(
        &mut repo,
        rehydration_request("attempt-1", "manifest-role-1", 0, "b2"),
        changed_bytes,
        "01ARZ3NDEKTSV4RRFFQ69G5FA3",
    );
    assert_eq!(
        attempt.source_evidence[0].comparison,
        SourceDigestComparison::Changed
    );
    assert_eq!(clear(&repo, T_AFTER), Err(R::ChangedSourcesPending));

    let succession = repo
        .succeed_changed_context_sources(
            &trusted_clock_at(T_AFTER),
            &ChangedSourceSuccessionRequestV1 {
                role_id: "role-1".into(),
                rehydration_attempt_id: "attempt-1".into(),
                successor_manifest_id: "manifest-role-1-e1".into(),
                trigger: ContextEpochTrigger::ArchitectureChange,
                authority: authority(),
            },
        )
        .unwrap();
    assert_eq!(succession.epoch.epoch, state_epoch(1));
    let new_digest = digest_of("artifact-arch", changed_bytes);
    assert_eq!(
        succession.changed_sources[0].digest.as_deref(),
        Some(new_digest.as_str())
    );
    assert_eq!(
        repo.find_context_epoch_invalidated_role_ids(PROJECT, 1)
            .unwrap(),
        Some(vec!["role-1".to_string()])
    );
    let role = repo.find_logical_role("role-1").unwrap().unwrap();
    assert_eq!(role.current_context_epoch, state_epoch(1));
    assert_eq!(
        role.context_manifest_id.as_deref(),
        Some("manifest-role-1-e1")
    );
    let successor = repo
        .find_context_manifest("manifest-role-1-e1")
        .unwrap()
        .unwrap();
    assert_eq!(successor.sources[0].digest, new_digest);
    assert_eq!(
        repo.find_context_manifest("manifest-role-1")
            .unwrap()
            .unwrap()
            .sources[0]
            .digest,
        old,
        "the predecessor manifest stays immutable history"
    );
    assert!(repo.find_event(&succession.event_id).unwrap().is_some());
    // A repeat of the same succession is refused: the role moved on.
    let snapshot = image(&repo);
    let repeat = ChangedSourceSuccessionRequestV1 {
        role_id: "role-1".into(),
        rehydration_attempt_id: "attempt-1".into(),
        successor_manifest_id: "manifest-role-1-e1b".into(),
        trigger: ContextEpochTrigger::ArchitectureChange,
        authority: authority(),
    };
    assert_eq!(
        refusal(
            repo.succeed_changed_context_sources(&trusted_clock_at(T_AFTER), &repeat)
                .unwrap_err()
        ),
        R::AttemptScopeMismatch
    );
    assert_eq!(image(&repo), snapshot);

    // The old attempt no longer clears; reopen and re-read at the new epoch.
    assert_eq!(clear(&repo, T_AFTER), Err(R::RehydrationRequired));
    drop(repo);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert_eq!(clear(&repo, T_AFTER), Err(R::RehydrationRequired));
    let attempt = rehydrate(
        &mut repo,
        rehydration_request("attempt-2", "manifest-role-1-e1", 1, "b2"),
        changed_bytes,
        "01ARZ3NDEKTSV4RRFFQ69G5FA4",
    );
    assert_eq!(
        attempt.source_evidence[0].comparison,
        SourceDigestComparison::Matched
    );
    assert_eq!(
        clear(&repo, T_AFTER).unwrap().rehydration_attempt_id,
        "attempt-2"
    );
}

#[test]
fn compaction_event_forces_reread_not_summary() {
    let digest = digest_of("artifact-arch", ARTIFACT_BYTES);
    let (_tmp, mut repo) = seeded("st-w2-s17", LogicalRoleType::RuntimeA2, &digest);
    rehydrate(
        &mut repo,
        rehydration_request("attempt-1", "manifest-role-1", 0, "b1"),
        ARTIFACT_BYTES,
        "01ARZ3NDEKTSV4RRFFQ69G5FA5",
    );
    assert!(clear(&repo, T_BEFORE).is_ok());
    // Host compaction: the trusted core advances the epoch invalidating the role.
    repo.advance_context_epoch(
        PROJECT,
        T_BEFORE,
        ContextEpochTrigger::ContextCompaction,
        &["role-1".to_string()],
    )
    .unwrap();
    assert_eq!(clear(&repo, T_BEFORE), Err(R::ContextInvalidated));
    // No succession without a changed source: compaction alone changed none.
    assert_eq!(
        refusal(
            repo.succeed_changed_context_sources(
                &trusted_clock_at(T_BEFORE),
                &ChangedSourceSuccessionRequestV1 {
                    role_id: "role-1".into(),
                    rehydration_attempt_id: "attempt-1".into(),
                    successor_manifest_id: "m2".into(),
                    trigger: ContextEpochTrigger::ContextCompaction,
                    authority: authority(),
                },
            )
            .unwrap_err()
        ),
        R::StaleContextEpoch
    );
    // Move the role to the new epoch with an immutable successor manifest
    // and genuinely re-read the authoritative source there.
    repo.supersede_context_manifest(
        "role-1",
        &state_epoch(0),
        "manifest-role-1",
        manifest("manifest-role-1-e1", "role-1", 1, &digest),
        &evidence(T_BEFORE),
    )
    .unwrap();
    assert_eq!(clear(&repo, T_BEFORE), Err(R::RehydrationRequired));
    let attempt = rehydrate(
        &mut repo,
        rehydration_request("attempt-2", "manifest-role-1-e1", 1, "b1"),
        ARTIFACT_BYTES,
        "01ARZ3NDEKTSV4RRFFQ69G5FA6",
    );
    assert_eq!(
        attempt.source_evidence[0].disposition,
        SourceDisposition::Reread
    );
    assert_eq!(
        clear(&repo, T_BEFORE).unwrap().rehydration_attempt_id,
        "attempt-2"
    );
}

#[test]
fn trusted_core_admission_binds_recovery_and_reopen_requires_fresh_admission() {
    use crate::{
        StateWriteOperationV1 as Op, StateWriteRefusalV1 as W, StateWriteSubjectV1,
        TrustedCoreStore,
    };
    use std::os::unix::fs::DirBuilderExt;

    let dir = std::env::temp_dir().join(format!("receipts-st-w2-core-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    let db = dir.join("state.db");
    {
        let mut repo = SqliteStateRepository::open(&db).unwrap();
        repo.append_context_epoch(ContextEpoch {
            project_id: PROJECT.into(),
            epoch: state_epoch(0),
            advanced_at: T_BOUND.into(),
            trigger: ContextEpochTrigger::A1Init,
        })
        .unwrap();
        add_role(
            &mut repo,
            "role-1",
            LogicalRoleType::RuntimeA1,
            "sha256:v1:00",
        );
        repo.create_executor_binding(binding("b1", "role-1", T_DEADLINE))
            .unwrap();
        repo.set_logical_role_active_binding("role-1", None, Some("b1"), &evidence(T_BOUND))
            .unwrap();
    }
    for suffix in ["", "-wal", "-shm"] {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(format!("state.db{suffix}"));
        if path.exists() {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
    let subject = |op, project: &str| StateWriteSubjectV1 {
        operation: op,
        project_id: project.into(),
        graph_id: None,
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    };
    let clock = trusted_clock_at(T_AFTER);
    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    let wrong_op = store.admit(subject(Op::LogicalRole, PROJECT));
    let error = store
        .recover_startup(wrong_op, &clock, PROJECT, &authority())
        .unwrap_err();
    assert!(matches!(
        error,
        StateError::WriteAuthorityRefused {
            reason: W::WrongOperation
        }
    ));
    let foreign = store.admit(subject(Op::ExecutorBinding, "project-other"));
    let error = store
        .rebind_role(foreign, &clock, &rebind(Some("b1"), "b2", None))
        .unwrap_err();
    assert!(matches!(
        error,
        StateError::WriteAuthorityRefused {
            reason: W::ProjectMismatch
        }
    ));
    assert!(
        store
            .reader()
            .find_trusted_time_watermark(PROJECT)
            .unwrap()
            .is_none()
    );

    let admission = store.admit(subject(Op::ExecutorBinding, PROJECT));
    let report = store
        .recover_startup(admission, &clock, PROJECT, &authority())
        .unwrap();
    assert_eq!(report.lease_expired_binding_ids, vec!["b1".to_string()]);
    let admission = store.admit(subject(Op::ExecutorBinding, PROJECT));
    store
        .rebind_role(admission, &clock, &rebind(Some("b1"), "b2", None))
        .unwrap();
    let stale_instance = store.admit(subject(Op::ExecutorBinding, PROJECT));
    drop(store);

    let mut store = TrustedCoreStore::open_exclusive(&db).unwrap();
    let error = store
        .recover_startup(stale_instance, &clock, PROJECT, &authority())
        .unwrap_err();
    assert!(matches!(
        error,
        StateError::WriteAuthorityRefused {
            reason: W::WrongStore
        }
    ));
    assert_eq!(
        store
            .reader()
            .continuation_clearance(&clock, "role-1")
            .map_err(refusal),
        Err(R::RehydrationRequired)
    );
    let admission = store.admit(subject(Op::ExecutorBinding, PROJECT));
    let report = store
        .recover_startup(admission, &clock, PROJECT, &authority())
        .unwrap();
    assert!(
        report.lease_expired_binding_ids.is_empty(),
        "idempotent across reopen"
    );
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}
