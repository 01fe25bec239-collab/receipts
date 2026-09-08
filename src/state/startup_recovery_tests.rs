use crate::context_epoch::{ContextEpoch, ContextEpochTrigger};
use crate::context_manifest::{
    ContextManifest, ContextManifestSource, ContextSourceRef, ContextSourceRefType, SourceClass,
};
use crate::context_rehydration::{
    ContextRehydrationAttempt, ContextRehydrationSourceEvidence, ContextRehydrationStatus,
    SourceDigestComparison, SourceDisposition,
};
use crate::event::{ActorKind, EventActor};
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

fn attempt() -> ContextRehydrationAttempt {
    ContextRehydrationAttempt {
        rehydration_attempt_id: "attempt-1".into(),
        project_id: "project-1".into(),
        durable_role_id: "role-1".into(),
        context_manifest_id: "manifest-1".into(),
        context_epoch_id: 3,
        repository_snapshot_references: Vec::new(),
        requested_by_actor: EventActor {
            kind: ActorKind::System,
            id: None,
        },
        executor_binding_id: None,
        session_reference: None,
        task_id: None,
        correlation_reference: None,
        trigger_kind: ContextEpochTrigger::A2Init,
        trigger_reference: None,
        started_at: "2026-09-01T00:00:00Z".into(),
        completed_at: "2026-09-01T00:00:01Z".into(),
        status: ContextRehydrationStatus::Succeeded,
        source_evidence: vec![ContextRehydrationSourceEvidence {
            source_id: "source-1".into(),
            source_ordinal: 0,
            ref_type: "REPO_PATH".into(),
            source_class: "MANDATORY".into(),
            canonical_source_identity: "repository-snapshot-source-1".into(),
            materializer_id: Some("materializer-1".into()),
            provenance: Some("durable-source-evidence-1".into()),
            expected_digest: format!("sha256:v1:{}", "a".repeat(64)),
            observed_digest: Some(format!("sha256:v1:{}", "a".repeat(64))),
            comparison: SourceDigestComparison::Matched,
            touch_evidence: None,
            disposition: SourceDisposition::Unchanged,
            materialized_at: Some("2026-09-01T00:00:00Z".into()),
            failure_code: None,
            failure_detail: None,
        }],
        failure_code: None,
    }
}

fn assert_inconsistency(
    result: &crate::startup_recovery::StartupRecoveryClassification,
    expected: Option<StartupDurableInconsistency>,
) {
    assert_eq!(
        result.inconsistencies,
        expected.into_iter().collect::<Vec<_>>()
    );
    assert_eq!(result.is_fail_closed(), expected.is_some());
    if expected.is_some() {
        assert!(requires(
            result,
            StartupReconciliationRequirement::DurableFactReconciliation
        ));
    }
}

#[test]
fn repair_01_to_03_rehydration_manifest_evidence() {
    for (case, supplied_id, mismatch) in [
        ("01 matching", Some("manifest-1"), false),
        ("02 contradictory", Some("manifest-other"), true),
        ("03 unsupplied", None, false),
    ] {
        let role = role(LogicalRoleStatus::Active);
        let attempt = attempt();
        let manifest = supplied_id.map(|id| ContextManifest {
            manifest_id: id.into(),
            ..manifest()
        });
        let result = classify_startup_recovery(StartupRecoverySnapshot {
            role: &role,
            binding: None,
            context_manifest: manifest.as_ref(),
            latest_context_epoch: None,
            rehydration_attempt: Some(&attempt),
        });
        assert_inconsistency(
            &result,
            mismatch.then_some(StartupDurableInconsistency::RehydrationManifestMismatch),
        );
        assert!(
            requires(
                &result,
                StartupReconciliationRequirement::ContextReconciliation
            ),
            "{case}"
        );
    }
}

#[test]
fn repair_04_to_08_rehydration_binding_evidence() {
    for (case, reference, supplied_id, mismatch, durable) in [
        (
            "04 same",
            Some("binding-1"),
            Some("binding-1"),
            false,
            false,
        ),
        (
            "05 different",
            Some("binding-1"),
            Some("binding-other"),
            true,
            true,
        ),
        ("06 unsupplied", Some("binding-1"), None, false, true),
        ("07 no association", None, Some("binding-1"), false, false),
        ("08 neither", None, None, false, false),
    ] {
        let role = role(LogicalRoleStatus::Active);
        let attempt = ContextRehydrationAttempt {
            executor_binding_id: reference.map(str::to_owned),
            ..attempt()
        };
        let binding = supplied_id.map(|id| ExecutorBinding {
            binding_id: id.into(),
            ..binding()
        });
        let manifest = manifest();
        let result = classify_startup_recovery(StartupRecoverySnapshot {
            role: &role,
            binding: binding.as_ref(),
            context_manifest: Some(&manifest),
            latest_context_epoch: None,
            rehydration_attempt: Some(&attempt),
        });
        assert_inconsistency(
            &result,
            mismatch.then_some(StartupDurableInconsistency::RehydrationBindingMismatch),
        );
        assert_eq!(
            requires(
                &result,
                StartupReconciliationRequirement::DurableFactReconciliation
            ),
            durable,
            "{case}"
        );
        assert_eq!(
            requires(
                &result,
                StartupReconciliationRequirement::ExternalExecutorObservation
            ),
            supplied_id.is_some(),
            "{case}"
        );
        assert_eq!(
            requires(&result, StartupReconciliationRequirement::LeaseEvaluation),
            supplied_id.is_some(),
            "{case}"
        );
        assert_eq!(
            result.binding_state,
            if supplied_id.is_some() {
                DurableBindingState::UnreleasedHistoricalEvidence
            } else {
                DurableBindingState::NoExternalExecutorEvidence
            }
        );
    }
}

#[test]
fn repair_09_to_11_role_manifest_evidence() {
    for (supplied_id, mismatch) in [
        (Some("manifest-1"), false),    // 09
        (Some("manifest-other"), true), // 10
        (None, false),                  // 11
    ] {
        let role = LogicalRole {
            context_manifest_id: Some("manifest-1".into()),
            ..role(LogicalRoleStatus::Active)
        };
        let manifest = supplied_id.map(|id| ContextManifest {
            manifest_id: id.into(),
            ..manifest()
        });
        let result = classify(&role, None, manifest.as_ref(), None);
        assert_inconsistency(
            &result,
            mismatch.then_some(StartupDurableInconsistency::RoleManifestReferenceMismatch),
        );
        assert!(requires(
            &result,
            StartupReconciliationRequirement::ContextReconciliation
        ));
    }
}

#[test]
fn repair_12_to_14_role_binding_evidence() {
    for (supplied_id, mismatch, durable) in [
        (Some("binding-1"), false, false),   // 12
        (Some("binding-other"), true, true), // 13
        (None, false, true),                 // 14
    ] {
        let role = LogicalRole {
            active_binding_id: Some("binding-1".into()),
            ..role(LogicalRoleStatus::Active)
        };
        let binding = supplied_id.map(|id| ExecutorBinding {
            binding_id: id.into(),
            ..binding()
        });
        let result = classify(&role, binding.as_ref(), None, None);
        assert_inconsistency(
            &result,
            mismatch.then_some(StartupDurableInconsistency::RoleBindingReferenceMismatch),
        );
        assert_eq!(
            requires(
                &result,
                StartupReconciliationRequirement::DurableFactReconciliation
            ),
            durable
        );
    }
}

#[test]
fn repair_15_to_17_missing_evidence_only_retains_reconciliation() {
    let role = LogicalRole {
        active_binding_id: Some("binding-1".into()),
        context_manifest_id: Some("manifest-1".into()),
        ..role(LogicalRoleStatus::Active)
    };
    let attempt = ContextRehydrationAttempt {
        executor_binding_id: Some("binding-1".into()),
        ..attempt()
    };
    let result = classify_startup_recovery(StartupRecoverySnapshot {
        role: &role,
        binding: None,
        context_manifest: None,
        latest_context_epoch: None,
        rehydration_attempt: Some(&attempt),
    });
    assert_inconsistency(&result, None); // 15: no unrelated contradiction
    assert_eq!(
        result.requirements,
        vec![
            StartupReconciliationRequirement::ContextReconciliation, // 16
            StartupReconciliationRequirement::DurableFactReconciliation, // 17
        ]
    );
}

#[test]
fn repair_18_and_21_all_supplied_relation_mismatches_preserve_all_requirements() {
    let role = LogicalRole {
        active_binding_id: Some("binding-other".into()),
        context_manifest_id: Some("manifest-other".into()),
        ..role(LogicalRoleStatus::Active)
    };
    let attempt = ContextRehydrationAttempt {
        context_manifest_id: "manifest-other".into(),
        executor_binding_id: Some("binding-other".into()),
        ..attempt()
    };
    let result = classify_startup_recovery(StartupRecoverySnapshot {
        role: &role,
        binding: Some(&binding()),
        context_manifest: Some(&manifest()),
        latest_context_epoch: Some(&epoch()),
        rehydration_attempt: Some(&attempt),
    });
    assert!(result.is_fail_closed());
    assert_eq!(
        result.inconsistencies,
        vec![
            StartupDurableInconsistency::RoleBindingReferenceMismatch,
            StartupDurableInconsistency::RoleManifestReferenceMismatch,
            StartupDurableInconsistency::RehydrationManifestMismatch,
            StartupDurableInconsistency::RehydrationBindingMismatch,
        ]
    );
    assert_eq!(
        result.requirements,
        vec![
            StartupReconciliationRequirement::ContextReconciliation,
            StartupReconciliationRequirement::ExternalExecutorObservation,
            StartupReconciliationRequirement::LeaseEvaluation,
            StartupReconciliationRequirement::DurableFactReconciliation,
        ]
    );
}

#[test]
fn repair_19_successful_rehydration_never_proves_external_liveness() {
    let binding = ExecutorBinding {
        session_ref: Some("session-1".into()),
        rehydration_completed: Some(true),
        lease_expires_at: "2099-09-01T00:10:00Z".into(),
        ..binding()
    };
    let attempt = ContextRehydrationAttempt {
        executor_binding_id: Some(binding.binding_id.clone()),
        ..attempt()
    };
    let result = classify_startup_recovery(StartupRecoverySnapshot {
        role: &role(LogicalRoleStatus::Active),
        binding: Some(&binding),
        context_manifest: Some(&manifest()),
        latest_context_epoch: Some(&epoch()),
        rehydration_attempt: Some(&attempt),
    });
    assert_inconsistency(&result, None);
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
fn repair_20_rehydration_does_not_reactivate_terminal_history() {
    for reason in [ReleaseReason::Completed, ReleaseReason::LeaseExpired] {
        let binding = ExecutorBinding {
            released_at: Some("2026-09-01T00:05:00Z".into()),
            release_reason: Some(reason),
            ..binding()
        };
        let attempt = ContextRehydrationAttempt {
            executor_binding_id: Some(binding.binding_id.clone()),
            ..attempt()
        };
        let result = classify_startup_recovery(StartupRecoverySnapshot {
            role: &role(LogicalRoleStatus::Active),
            binding: Some(&binding),
            context_manifest: Some(&manifest()),
            latest_context_epoch: None,
            rehydration_attempt: Some(&attempt),
        });
        assert_inconsistency(&result, None);
        assert_eq!(
            result.binding_state,
            DurableBindingState::TerminalHistory(reason)
        );
        assert_eq!(
            result.requirements,
            vec![StartupReconciliationRequirement::ContextReconciliation]
        );
    }
}

#[test]
fn repair_22_and_23_deterministic_without_mutating_any_supplied_history() {
    let role = role(LogicalRoleStatus::Active);
    let binding = binding();
    let manifest = manifest();
    let epoch = epoch();
    let attempt = attempt();
    let before = (
        role.clone(),
        binding.clone(),
        manifest.clone(),
        epoch.clone(),
        attempt.clone(),
    );
    let snapshot = StartupRecoverySnapshot {
        role: &role,
        binding: Some(&binding),
        context_manifest: Some(&manifest),
        latest_context_epoch: Some(&epoch),
        rehydration_attempt: Some(&attempt),
    };
    assert_eq!(
        classify_startup_recovery(snapshot),
        classify_startup_recovery(snapshot)
    );
    assert_eq!(before, (role, binding, manifest, epoch, attempt));
}

#[test]
fn repair_24_source_authority_boundary() {
    // Source review: the classifier calls only its local classify/check/require/
    // inconsistent helpers and value/collection methods. All five record inputs
    // are shared references. No repository, external service, or mutation handle
    // enters the call graph. This guard catches common authority additions;
    // review of that complete call graph remains the authoritative static check.
    let source = include_str!("startup_recovery.rs");
    for forbidden in [
        "std::",
        "tokio::",
        "rusqlite",
        "reqwest",
        "Command",
        "unsafe",
        "extern ",
        "include!",
        "repository::",
        "Connection",
        "UnitOfWork",
        "process::",
        "git2",
        "worktree::",
        "runtime::",
        "provider::",
        "orchestration::",
        "review::",
        "integration::",
        "TrustedTimeWatermark",
        "SELECT ",
        "INSERT ",
        "UPDATE ",
        "DELETE ",
    ] {
        assert!(
            !source.contains(forbidden),
            "review new authority: {forbidden}"
        );
    }
}

#[test]
fn supplied_identity_contradictions_remain_individually_fail_closed() {
    for expected in [
        StartupDurableInconsistency::BindingRoleMismatch,
        StartupDurableInconsistency::ContextManifestRoleMismatch,
        StartupDurableInconsistency::ContextManifestProjectMismatch,
        StartupDurableInconsistency::ContextEpochProjectMismatch,
        StartupDurableInconsistency::RehydrationRoleMismatch,
        StartupDurableInconsistency::RehydrationProjectMismatch,
        StartupDurableInconsistency::RehydrationEpochMismatch,
    ] {
        let role = role(LogicalRoleStatus::Active);
        let mut binding = binding();
        let mut manifest = manifest();
        let mut epoch = epoch();
        let mut attempt = attempt();
        match expected {
            StartupDurableInconsistency::BindingRoleMismatch => binding.role_id = "other".into(),
            StartupDurableInconsistency::ContextManifestRoleMismatch => {
                manifest.role_id = "other".into()
            }
            StartupDurableInconsistency::ContextManifestProjectMismatch => {
                manifest.project_id = "other".into()
            }
            StartupDurableInconsistency::ContextEpochProjectMismatch => {
                epoch.project_id = "other".into()
            }
            StartupDurableInconsistency::RehydrationRoleMismatch => {
                attempt.durable_role_id = "other".into()
            }
            StartupDurableInconsistency::RehydrationProjectMismatch => {
                attempt.project_id = "other".into()
            }
            StartupDurableInconsistency::RehydrationEpochMismatch => attempt.context_epoch_id += 1,
            _ => unreachable!(),
        }
        let result = classify_startup_recovery(StartupRecoverySnapshot {
            role: &role,
            binding: Some(&binding),
            context_manifest: Some(&manifest),
            latest_context_epoch: Some(&epoch),
            rehydration_attempt: Some(&attempt),
        });
        assert_inconsistency(&result, Some(expected));
    }
}

#[test]
fn release_reason_without_timestamp_remains_ambiguous() {
    let binding = ExecutorBinding {
        release_reason: Some(ReleaseReason::Completed),
        ..binding()
    };
    let result = classify(&role(LogicalRoleStatus::Active), Some(&binding), None, None);
    assert_inconsistency(
        &result,
        Some(StartupDurableInconsistency::AmbiguousBindingTerminalShape),
    );
    assert_eq!(
        result.binding_state,
        DurableBindingState::AmbiguousTerminalShape
    );
}
