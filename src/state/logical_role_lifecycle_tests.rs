use crate::CanonicalTimestampV1;
use crate::context_epoch::{ContextEpoch, ContextEpochTrigger};
use crate::context_manifest::{
    ContextManifest, ContextManifestSource, ContextSourceRef, ContextSourceRefType, SourceClass,
};
use crate::error::StateError;
use crate::executor_binding::{ExecutorBinding, ReleaseReason};
use crate::logical_role::{LogicalRole, LogicalRoleStatus as S, LogicalRoleType};
use crate::logical_role_lifecycle::{RoleChangeEvidenceV1, RoleChangeKind, RoleChangeRefusal as R};
use crate::repository::SqliteStateRepository;
use crate::tests::{TempDir, state_epoch};

pub(crate) const ROLE: &str = "role-a2";

pub(crate) fn evidence() -> RoleChangeEvidenceV1 {
    RoleChangeEvidenceV1 {
        changed_at: CanonicalTimestampV1::parse("2026-10-29T10:00:00.000000000Z").unwrap(),
        reason: Some("orchestrator action".into()),
    }
}

pub(crate) fn role(role_id: &str, project: &str) -> LogicalRole {
    LogicalRole {
        role_id: role_id.into(),
        project_id: project.into(),
        role_type: LogicalRoleType::RuntimeA2,
        status: S::Active,
        current_context_epoch: state_epoch(0),
        name: Some("auth".into()),
        workstream_id: Some("ws-auth".into()),
        ownership_paths: vec!["src/auth/**".into()],
        integration_branch: Some("ws/auth".into()),
        context_manifest_id: None,
        active_binding_id: None,
        created_at: Some("2026-10-29T09:00:00Z".into()),
    }
}

pub(crate) fn binding(binding_id: &str, role_id: &str) -> ExecutorBinding {
    ExecutorBinding {
        binding_id: binding_id.into(),
        role_id: role_id.into(),
        provider_id: format!("provider-{binding_id}"),
        model_id: "model".into(),
        runtime_id: "runtime".into(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: "2026-10-29T09:00:00.000000000Z".into(),
        lease_expires_at: "2026-10-29T11:00:00.000000000Z".into(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    }
}

fn seeded(tag: &str) -> (TempDir, SqliteStateRepository) {
    let tmp = TempDir::new(tag);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    repo.create_logical_role(role(ROLE, "p")).unwrap();
    repo.create_logical_role(role("role-other", "p")).unwrap();
    (tmp, repo)
}

fn refusal(result: Result<(), StateError>) -> R {
    match result.unwrap_err() {
        StateError::LogicalRoleChangeRefused { reason, .. } => reason,
        other => panic!("unexpected {other:?}"),
    }
}

/// Role row, ownership paths and history count: the full durable image.
fn image(repo: &SqliteStateRepository) -> (LogicalRole, usize) {
    (
        repo.find_logical_role(ROLE).unwrap().unwrap(),
        repo.list_logical_role_history(ROLE).unwrap().len(),
    )
}

fn release(repo: &mut SqliteStateRepository, binding_id: &str) {
    repo.release_executor_binding(
        binding_id,
        "2026-10-29T10:30:00.000000000Z",
        ReleaseReason::RateLimited,
    )
    .unwrap();
}

#[test]
fn role_status_legal_transitions_append_history() {
    let (_tmp, mut repo) = seeded("role-status-legal");
    for (from, to) in [
        (S::Active, S::Suspended),
        (S::Suspended, S::Active),
        (S::Active, S::Suspended),
        (S::Suspended, S::Retired),
    ] {
        repo.transition_logical_role_status(ROLE, from, to, &evidence())
            .unwrap();
        assert_eq!(repo.find_logical_role(ROLE).unwrap().unwrap().status, to);
    }
    let history = repo.list_logical_role_history(ROLE).unwrap();
    assert_eq!(
        history.iter().map(|h| h.sequence).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert!(
        history
            .iter()
            .all(|h| h.change_kind == RoleChangeKind::Status)
    );
    assert_eq!(
        (history[3].from_status, history[3].to_status),
        (S::Suspended, S::Retired)
    );
    assert_eq!(history[0].reason.as_deref(), Some("orchestrator action"));
    let (_tmp, mut repo) = seeded("role-status-direct-retire");
    repo.transition_logical_role_status(ROLE, S::Active, S::Retired, &evidence())
        .unwrap();
}

#[test]
fn role_status_illegal_transition_refused() {
    let (_tmp, mut repo) = seeded("role-status-illegal");
    let before = image(&repo);
    for (expected, to, want) in [
        (S::Active, S::Active, R::NoChange),
        (S::Suspended, S::Active, R::StaleExpectation),
        (S::Retired, S::Active, R::StaleExpectation),
    ] {
        assert_eq!(
            refusal(repo.transition_logical_role_status(ROLE, expected, to, &evidence())),
            want
        );
    }
    assert_eq!(
        refusal(repo.transition_logical_role_status(
            "absent",
            S::Active,
            S::Suspended,
            &evidence()
        )),
        R::RoleNotFound
    );
    assert_eq!(image(&repo), before, "refusals leave no partial write");
    repo.transition_logical_role_status(ROLE, S::Active, S::Retired, &evidence())
        .unwrap();
    let retired = image(&repo);
    for to in [S::Active, S::Suspended, S::Retired] {
        assert_eq!(
            refusal(repo.transition_logical_role_status(ROLE, S::Retired, to, &evidence())),
            R::RoleRetired
        );
    }
    repo.create_executor_binding(binding("b1", ROLE)).unwrap();
    assert_eq!(
        refusal(repo.set_logical_role_active_binding(ROLE, None, Some("b1"), &evidence())),
        R::RoleRetired
    );
    assert_eq!(
        refusal(repo.set_logical_role_context(
            ROLE,
            &state_epoch(0),
            None,
            &state_epoch(0),
            None,
            &evidence()
        )),
        R::RoleRetired
    );
    assert_eq!(image(&repo), retired);
    // Input refusals happen before storage.
    let empty_reason = RoleChangeEvidenceV1 {
        reason: Some(String::new()),
        ..evidence()
    };
    assert!(matches!(
        repo.transition_logical_role_status("role-other", S::Active, S::Suspended, &empty_reason),
        Err(StateError::LogicalRoleValidation { .. })
    ));
}

#[test]
fn role_identity_survives_three_binding_replacements() {
    let (_tmp, mut repo) = seeded("role-three-bindings");
    let original = repo.find_logical_role(ROLE).unwrap().unwrap();
    let mut current: Option<String> = None;
    for id in ["b1", "b2", "b3", "b4"] {
        if let Some(old) = &current {
            release(&mut repo, old);
        }
        repo.create_executor_binding(binding(id, ROLE)).unwrap();
        repo.set_logical_role_active_binding(ROLE, current.as_deref(), Some(id), &evidence())
            .unwrap();
        current = Some(id.into());
    }
    let role = repo.find_logical_role(ROLE).unwrap().unwrap();
    assert_eq!(role.active_binding_id.as_deref(), Some("b4"));
    let identity = |r: &LogicalRole| {
        (
            r.role_id.clone(),
            r.project_id.clone(),
            r.role_type,
            r.name.clone(),
            r.workstream_id.clone(),
            r.ownership_paths.clone(),
            r.integration_branch.clone(),
            r.created_at.clone(),
        )
    };
    assert_eq!(identity(&role), identity(&original));
    let history = repo.list_logical_role_history(ROLE).unwrap();
    assert_eq!(
        history
            .iter()
            .map(|h| (h.from_binding_id.clone(), h.to_binding_id.clone()))
            .collect::<Vec<_>>(),
        vec![
            (None, Some("b1".into())),
            (Some("b1".into()), Some("b2".into())),
            (Some("b2".into()), Some("b3".into())),
            (Some("b3".into()), Some("b4".into())),
        ]
    );
    // Historical executor rows are retained unchanged.
    for id in ["b1", "b2", "b3"] {
        let b = repo.find_executor_binding(id).unwrap().unwrap();
        assert_eq!(
            (b.role_id.as_str(), b.release_reason),
            (ROLE, Some(ReleaseReason::RateLimited))
        );
    }
    // Clear requires the matching terminal release.
    assert_eq!(
        refusal(repo.set_logical_role_active_binding(ROLE, Some("b4"), None, &evidence())),
        R::CurrentBindingUnreleased
    );
    release(&mut repo, "b4");
    repo.set_logical_role_active_binding(ROLE, Some("b4"), None, &evidence())
        .unwrap();
    repo.transition_logical_role_status(ROLE, S::Active, S::Retired, &evidence())
        .unwrap();
}

#[test]
fn binding_pointer_refusals_leave_no_partial_write() {
    let (_tmp, mut repo) = seeded("role-binding-refusals");
    repo.create_executor_binding(binding("other-b", "role-other"))
        .unwrap();
    let mut released = binding("released", ROLE);
    released.released_at = Some("2026-10-29T09:30:00.000000000Z".into());
    released.release_reason = Some(ReleaseReason::Completed);
    repo.create_executor_binding(released).unwrap();
    let before = image(&repo);
    for (expected, new, want) in [
        (None, Some("absent"), R::BindingNotFound),
        (None, Some("other-b"), R::BindingRoleMismatch),
        (None, Some("released"), R::BindingNotUnreleased),
        (Some("x"), Some("released"), R::StaleExpectation),
        (None, None, R::NoChange),
    ] {
        assert_eq!(
            refusal(repo.set_logical_role_active_binding(ROLE, expected, new, &evidence())),
            want
        );
    }
    assert_eq!(image(&repo), before);
    // A partial terminal pair is not conclusively unreleased (nor released).
    repo.connection()
        .execute_batch("INSERT INTO executor_binding(binding_id,role_id,provider_id,model_id,runtime_id,bound_at,lease_expires_at,released_at) VALUES ('partial','role-a2','p','m','r','b','l','2026-10-29T09:30:00.000000000Z')")
        .unwrap();
    assert_eq!(
        refusal(repo.set_logical_role_active_binding(ROLE, None, Some("partial"), &evidence())),
        R::BindingNotUnreleased
    );
    // Do not replace an unreleased different binding; do not retire with one.
    let (_tmp, mut repo) = seeded("role-binding-unreleased");
    repo.create_executor_binding(binding("live", ROLE)).unwrap();
    repo.set_logical_role_active_binding(ROLE, None, Some("live"), &evidence())
        .unwrap();
    assert_eq!(
        refusal(repo.set_logical_role_active_binding(
            ROLE,
            Some("live"),
            Some("released"),
            &evidence()
        )),
        R::CurrentBindingUnreleased
    );
    assert_eq!(
        refusal(repo.transition_logical_role_status(ROLE, S::Active, S::Retired, &evidence())),
        R::RoleHasBinding
    );
    // An unreleased binding without a pointer also blocks retirement.
    let (_tmp, mut repo) = seeded("role-retire-unpointed");
    repo.create_executor_binding(binding("loose", ROLE))
        .unwrap();
    assert_eq!(
        refusal(repo.transition_logical_role_status(ROLE, S::Active, S::Retired, &evidence())),
        R::RoleHasBinding
    );
}

#[test]
fn dangling_legacy_pointer_is_refused_not_repaired() {
    let tmp = TempDir::new("role-dangling");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let mut legacy = role(ROLE, "p");
    legacy.active_binding_id = Some("ghost".into());
    repo.create_logical_role(legacy).unwrap();
    repo.create_executor_binding(binding("b1", ROLE)).unwrap();
    assert_eq!(
        refusal(repo.set_logical_role_active_binding(ROLE, Some("ghost"), Some("b1"), &evidence())),
        R::CurrentBindingDangling
    );
    assert_eq!(
        repo.find_logical_role(ROLE)
            .unwrap()
            .unwrap()
            .active_binding_id
            .as_deref(),
        Some("ghost")
    );
}

fn manifest(id: &str, role_id: &str, project: &str, epoch: i64) -> ContextManifest {
    ContextManifest {
        manifest_id: id.into(),
        role_id: role_id.into(),
        project_id: project.into(),
        epoch: state_epoch(epoch),
        sources: vec![ContextManifestSource {
            r#ref: ContextSourceRef {
                ref_type: ContextSourceRefType::RepoPath,
                target: "spec.md".into(),
            },
            source_class: SourceClass::Mandatory,
            digest: "d".into(),
            last_read_at: None,
            required_for: Vec::new(),
        }],
        created_at: "2026-10-29T09:00:00Z".into(),
        last_rehydrated_at: None,
    }
}

#[test]
fn context_epoch_and_manifest_changes_use_cas_and_never_regress() {
    let (_tmp, mut repo) = seeded("role-context");
    for epoch in [1, 2] {
        repo.append_context_epoch(ContextEpoch {
            project_id: "p".into(),
            epoch: state_epoch(epoch),
            advanced_at: "2026-10-29T09:00:00Z".into(),
            trigger: ContextEpochTrigger::NewWave,
        })
        .unwrap();
    }
    repo.create_context_manifest(manifest("m-other-role", "role-other", "p", 1))
        .unwrap();
    repo.create_context_manifest(manifest("m1", ROLE, "p", 1))
        .unwrap();
    let e = state_epoch;
    let before = image(&repo);
    for (exp_e, exp_m, new_e, new_m, want) in [
        (0, None, 0, None, R::NoChange),
        (1, None, 1, None, R::StaleExpectation),
        (0, Some("m1"), 1, Some("m1"), R::StaleExpectation),
        (0, None, 7, None, R::EpochUnknown),
        (0, None, 1, Some("absent"), R::ManifestNotFound),
        (0, None, 1, Some("m-other-role"), R::ManifestMismatch),
        (0, None, 2, Some("m1"), R::ManifestMismatch),
    ] {
        assert_eq!(
            refusal(repo.set_logical_role_context(
                ROLE,
                &e(exp_e),
                exp_m,
                &e(new_e),
                new_m,
                &evidence()
            )),
            want,
            "{exp_e}/{exp_m:?} -> {new_e}/{new_m:?}"
        );
    }
    assert_eq!(image(&repo), before);
    repo.set_logical_role_context(ROLE, &e(0), None, &e(1), Some("m1"), &evidence())
        .unwrap();
    assert_eq!(
        refusal(repo.set_logical_role_context(
            ROLE,
            &e(1),
            Some("m1"),
            &e(0),
            Some("m1"),
            &evidence()
        )),
        R::EpochRegression
    );
    assert_eq!(
        refusal(repo.set_logical_role_context(ROLE, &e(1), Some("m1"), &e(2), None, &evidence())),
        R::ManifestClear
    );
    repo.set_logical_role_context(ROLE, &e(1), Some("m1"), &e(2), Some("m1"), &evidence())
        .expect_err("manifest epoch must match the new epoch");
    let role = repo.find_logical_role(ROLE).unwrap().unwrap();
    assert_eq!(
        (
            role.current_context_epoch,
            role.context_manifest_id.as_deref()
        ),
        (e(1), Some("m1"))
    );
    let last = repo.list_logical_role_history(ROLE).unwrap().pop().unwrap();
    assert_eq!(
        (last.change_kind, last.from_epoch, last.to_epoch),
        (RoleChangeKind::Context, e(0), e(1))
    );
}

#[test]
fn storage_backstop_requires_matching_history_for_pointer_updates() {
    let (tmp, mut repo) = seeded("role-backstop");
    for sql in [
        "UPDATE logical_role SET status='SUSPENDED' WHERE role_id='role-a2'",
        "UPDATE logical_role SET project_id='q' WHERE role_id='role-a2'",
        "UPDATE logical_role SET name='x' WHERE role_id='role-a2'",
        "DELETE FROM logical_role WHERE role_id='role-a2'",
        "DELETE FROM logical_role_ownership_path",
        "UPDATE logical_role_ownership_path SET path='x'",
        // History that does not start from the current row is refused.
        "INSERT INTO logical_role_history VALUES ('role-a2','p',1,'STATUS','SUSPENDED','ACTIVE',NULL,NULL,'0','0',NULL,NULL,'2026-10-29T10:00:00.000000000Z',NULL)",
        // A non-contiguous sequence is refused.
        "INSERT INTO logical_role_history VALUES ('role-a2','p',2,'STATUS','ACTIVE','SUSPENDED',NULL,NULL,'0','0',NULL,NULL,'2026-10-29T10:00:00.000000000Z',NULL)",
    ] {
        assert!(repo.connection().execute_batch(sql).is_err(), "{sql}");
    }
    repo.transition_logical_role_status(ROLE, S::Active, S::Suspended, &evidence())
        .unwrap();
    // A stale history row cannot authorize a second raw pointer update.
    assert!(
        repo.connection()
            .execute_batch("UPDATE logical_role SET status='ACTIVE' WHERE role_id='role-a2'")
            .is_err()
    );
    for sql in [
        "UPDATE logical_role_history SET reason='x'",
        "DELETE FROM logical_role_history",
    ] {
        assert!(repo.connection().execute_batch(sql).is_err(), "{sql}");
    }
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert_eq!(
        repo.find_logical_role(ROLE).unwrap().unwrap().status,
        S::Suspended
    );
    assert_eq!(repo.list_logical_role_history(ROLE).unwrap().len(), 1);
}
