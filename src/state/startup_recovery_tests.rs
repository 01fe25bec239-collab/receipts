use crate::context_epoch::{ContextEpoch, ContextEpochTrigger};
use crate::context_manifest::{
    ContextManifest, ContextManifestSource, ContextSourceRef, ContextSourceRefType, SourceClass,
};
use crate::executor_binding::{ExecutorBinding, ReleaseReason};
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::startup_recovery::{
    DurableBindingState, StartupDurableInconsistency, StartupReconciliationRequirement,
    StartupRecoverySnapshot, classify_startup_recovery,
};

fn role(status: LogicalRoleStatus) -> LogicalRole {
    LogicalRole {
        role_id: "role-1".into(),
        project_id: "project-1".into(),
        role_type: LogicalRoleType::RuntimeA2,
        status,
        current_context_epoch: 3,
        name: None,
        workstream_id: None,
        ownership_paths: vec!["src/state".into()],
        integration_branch: None,
        context_manifest_id: None,
        active_binding_id: None,
        created_at: Some("2026-09-01T00:00:00Z".into()),
    }
}

fn binding() -> ExecutorBinding {
    ExecutorBinding {
        binding_id: "binding-1".into(),
        role_id: "role-1".into(),
        provider_id: "provider".into(),
        model_id: "model".into(),
        runtime_id: "runtime".into(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: "2026-09-01T00:00:00Z".into(),
        lease_expires_at: "2026-09-01T00:10:00Z".into(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    }
}

fn manifest() -> ContextManifest {
    ContextManifest {
        manifest_id: "manifest-1".into(),
        role_id: "role-1".into(),
        project_id: "project-1".into(),
        epoch: 3,
        sources: vec![ContextManifestSource {
            r#ref: ContextSourceRef {
                ref_type: ContextSourceRefType::RepoPath,
                target: "src/state/lib.rs".into(),
            },
            source_class: SourceClass::Mandatory,
            digest: "digest".into(),
            last_read_at: None,
            required_for: Vec::new(),
        }],
        created_at: "2026-09-01T00:00:00Z".into(),
        last_rehydrated_at: None,
    }
}

fn epoch() -> ContextEpoch {
    ContextEpoch {
        project_id: "project-1".into(),
        epoch: 3,
        advanced_at: "2026-09-01T00:00:00Z".into(),
        trigger: ContextEpochTrigger::A2Init,
    }
}

fn classify<'a>(
    role: &'a LogicalRole,
    binding: Option<&'a ExecutorBinding>,
    manifest: Option<&'a ContextManifest>,
    epoch: Option<&'a ContextEpoch>,
) -> crate::startup_recovery::StartupRecoveryClassification {
    classify_startup_recovery(StartupRecoverySnapshot {
        role,
        binding,
        context_manifest: manifest,
        latest_context_epoch: epoch,
        rehydration_attempt: None,
    })
}

fn requires(
    result: &crate::startup_recovery::StartupRecoveryClassification,
    requirement: StartupReconciliationRequirement,
) -> bool {
    result.requirements.contains(&requirement)
}

#[test]
fn active_role_without_binding_proves_no_external_executor() {
    let result = classify(&role(LogicalRoleStatus::Active), None, None, None);

    assert_eq!(
        result.binding_state,
        DurableBindingState::NoExternalExecutorEvidence
    );
    assert!(!requires(
        &result,
        StartupReconciliationRequirement::ExternalExecutorObservation
    ));
}

#[test]
fn active_role_with_unreleased_binding_requires_observation_and_lease_evaluation() {
    let result = classify(
        &role(LogicalRoleStatus::Active),
        Some(&binding()),
        None,
        None,
    );

    assert_eq!(
        result.binding_state,
        DurableBindingState::UnreleasedHistoricalEvidence
    );
    assert!(requires(
        &result,
        StartupReconciliationRequirement::ExternalExecutorObservation
    ));
    assert!(requires(
        &result,
        StartupReconciliationRequirement::LeaseEvaluation
    ));
}

#[test]
fn completed_and_lease_expired_bindings_remain_terminal_history() {
    for reason in [ReleaseReason::Completed, ReleaseReason::LeaseExpired] {
        let mut historical = binding();
        historical.released_at = Some("2026-09-01T00:05:00Z".into());
        historical.release_reason = Some(reason);

        let result = classify(
            &role(LogicalRoleStatus::Active),
            Some(&historical),
            None,
            None,
        );
        assert_eq!(
            result.binding_state,
            DurableBindingState::TerminalHistory(reason)
        );
        assert!(!requires(
            &result,
            StartupReconciliationRequirement::ExternalExecutorObservation
        ));
        assert!(!requires(
            &result,
            StartupReconciliationRequirement::LeaseEvaluation
        ));
    }
}

#[test]
fn suspended_and_retired_roles_do_not_become_runnable() {
    for status in [LogicalRoleStatus::Suspended, LogicalRoleStatus::Retired] {
        let result = classify(&role(status), None, None, None);
        assert_eq!(result.role_status, status);
        assert_eq!(
            result.binding_state,
            DurableBindingState::NoExternalExecutorEvidence
        );
    }
}

#[test]
fn binding_role_and_role_pointer_mismatches_fail_closed() {
    let mut role = role(LogicalRoleStatus::Active);
    role.active_binding_id = Some("another-binding".into());
    let mut binding = binding();
    binding.role_id = "another-role".into();

    let result = classify(&role, Some(&binding), None, None);

    assert!(result.is_fail_closed());
    assert!(
        result
            .inconsistencies
            .contains(&StartupDurableInconsistency::BindingRoleMismatch)
    );
    assert!(
        result
            .inconsistencies
            .contains(&StartupDurableInconsistency::RoleBindingReferenceMismatch)
    );
}

#[test]
fn partial_terminal_binding_shape_fails_closed_with_all_requirements() {
    let mut ambiguous = binding();
    ambiguous.released_at = Some("2026-09-01T00:05:00Z".into());

    let result = classify(
        &role(LogicalRoleStatus::Active),
        Some(&ambiguous),
        None,
        None,
    );

    assert_eq!(
        result.binding_state,
        DurableBindingState::AmbiguousTerminalShape
    );
    assert!(result.is_fail_closed());
    for requirement in [
        StartupReconciliationRequirement::ExternalExecutorObservation,
        StartupReconciliationRequirement::LeaseEvaluation,
        StartupReconciliationRequirement::ContextReconciliation,
        StartupReconciliationRequirement::DurableFactReconciliation,
    ] {
        assert!(requires(&result, requirement));
    }
}

#[test]
fn context_relationship_mismatches_require_reconciliation_and_fail_closed() {
    let mut manifest = manifest();
    manifest.project_id = "another-project".into();
    let mut epoch = epoch();
    epoch.project_id = "another-project".into();

    let result = classify(
        &role(LogicalRoleStatus::Active),
        None,
        Some(&manifest),
        Some(&epoch),
    );

    assert!(requires(
        &result,
        StartupReconciliationRequirement::ContextReconciliation
    ));
    assert!(result.is_fail_closed());
    assert!(
        result
            .inconsistencies
            .contains(&StartupDurableInconsistency::ContextManifestProjectMismatch)
    );
    assert!(
        result
            .inconsistencies
            .contains(&StartupDurableInconsistency::ContextEpochProjectMismatch)
    );
}

#[test]
fn absent_context_evidence_never_invents_healthy_context() {
    let result = classify(&role(LogicalRoleStatus::Active), None, None, None);

    assert!(requires(
        &result,
        StartupReconciliationRequirement::ContextReconciliation
    ));
}

#[test]
fn superficially_resumable_history_still_requires_external_reality() {
    let mut stale = binding();
    stale.session_ref = Some("opaque-session".into());
    stale.rehydration_completed = Some(true);

    let result = classify(
        &role(LogicalRoleStatus::Active),
        Some(&stale),
        Some(&manifest()),
        Some(&epoch()),
    );

    assert_eq!(
        result.binding_state,
        DurableBindingState::UnreleasedHistoricalEvidence
    );
    assert!(requires(
        &result,
        StartupReconciliationRequirement::ExternalExecutorObservation
    ));
    assert!(requires(
        &result,
        StartupReconciliationRequirement::ContextReconciliation
    ));
}

#[test]
fn classification_is_deterministic_and_does_not_mutate_history() {
    let role = role(LogicalRoleStatus::Active);
    let binding = binding();
    let manifest = manifest();
    let epoch = epoch();
    let before = (
        role.clone(),
        binding.clone(),
        manifest.clone(),
        epoch.clone(),
    );

    let first = classify(&role, Some(&binding), Some(&manifest), Some(&epoch));
    let second = classify(&role, Some(&binding), Some(&manifest), Some(&epoch));

    assert_eq!(first, second);
    assert_eq!(before, (role, binding, manifest, epoch));
}
