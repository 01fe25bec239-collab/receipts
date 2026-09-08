//! Pure startup classification of already-read durable State facts.
//!
//! This module does not inspect external reality or mutate durable history.

use crate::context_epoch::ContextEpoch;
use crate::context_manifest::ContextManifest;
use crate::context_rehydration::ContextRehydrationAttempt;
use crate::executor_binding::{ExecutorBinding, ReleaseReason};
use crate::logical_role::{LogicalRole, LogicalRoleStatus};

/// Typed durable facts supplied by a higher-level startup scanner.
/// Optional records set to `None` are unsupplied evidence, not proven absence.
#[derive(Debug, Clone, Copy)]
pub struct StartupRecoverySnapshot<'a> {
    pub role: &'a LogicalRole,
    pub binding: Option<&'a ExecutorBinding>,
    pub context_manifest: Option<&'a ContextManifest>,
    pub latest_context_epoch: Option<&'a ContextEpoch>,
    pub rehydration_attempt: Option<&'a ContextRehydrationAttempt>,
}

/// What the durable binding record proves, without implying external liveness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurableBindingState {
    NoExternalExecutorEvidence,
    UnreleasedHistoricalEvidence,
    TerminalHistory(ReleaseReason),
    AmbiguousTerminalShape,
}

/// Work that State deliberately leaves to an authority with the required evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupReconciliationRequirement {
    ExternalExecutorObservation,
    LeaseEvaluation,
    ContextReconciliation,
    DurableFactReconciliation,
}

/// Contradictory durable relationships that must fail closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupDurableInconsistency {
    BindingRoleMismatch,
    RoleBindingReferenceMismatch,
    AmbiguousBindingTerminalShape,
    ContextManifestRoleMismatch,
    ContextManifestProjectMismatch,
    RoleManifestReferenceMismatch,
    ContextEpochProjectMismatch,
    RehydrationRoleMismatch,
    RehydrationProjectMismatch,
    RehydrationManifestMismatch,
    RehydrationEpochMismatch,
    RehydrationBindingMismatch,
}

/// State's complete, non-authoritative startup conclusion for one durable role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupRecoveryClassification {
    pub role_status: LogicalRoleStatus,
    pub binding_state: DurableBindingState,
    pub requirements: Vec<StartupReconciliationRequirement>,
    pub inconsistencies: Vec<StartupDurableInconsistency>,
}

impl StartupRecoveryClassification {
    pub fn is_fail_closed(&self) -> bool {
        !self.inconsistencies.is_empty()
    }
}

/// Classifies only supplied durable facts. It performs no I/O and no mutation.
pub fn classify_startup_recovery(
    snapshot: StartupRecoverySnapshot<'_>,
) -> StartupRecoveryClassification {
    let mut result = StartupRecoveryClassification {
        role_status: snapshot.role.status,
        binding_state: DurableBindingState::NoExternalExecutorEvidence,
        requirements: vec![StartupReconciliationRequirement::ContextReconciliation],
        inconsistencies: Vec::new(),
    };

    classify_binding(snapshot.role, snapshot.binding, &mut result);
    classify_context(
        snapshot.role,
        snapshot.binding,
        snapshot.context_manifest,
        snapshot.latest_context_epoch,
        snapshot.rehydration_attempt,
        &mut result,
    );

    if result.is_fail_closed() {
        require(
            &mut result,
            StartupReconciliationRequirement::DurableFactReconciliation,
        );
    }
    result
}

fn classify_binding(
    role: &LogicalRole,
    binding: Option<&ExecutorBinding>,
    result: &mut StartupRecoveryClassification,
) {
    let Some(binding) = binding else {
        if role.active_binding_id.is_some() {
            require(
                result,
                StartupReconciliationRequirement::DurableFactReconciliation,
            );
        }
        return;
    };

    if binding.role_id != role.role_id {
        inconsistent(result, StartupDurableInconsistency::BindingRoleMismatch);
    }
    if role
        .active_binding_id
        .as_ref()
        .is_some_and(|id| id != &binding.binding_id)
    {
        inconsistent(
            result,
            StartupDurableInconsistency::RoleBindingReferenceMismatch,
        );
    }

    result.binding_state = match (binding.released_at.as_ref(), binding.release_reason) {
        (None, None) => {
            require(
                result,
                StartupReconciliationRequirement::ExternalExecutorObservation,
            );
            require(result, StartupReconciliationRequirement::LeaseEvaluation);
            DurableBindingState::UnreleasedHistoricalEvidence
        }
        (Some(_), Some(reason)) => DurableBindingState::TerminalHistory(reason),
        _ => {
            inconsistent(
                result,
                StartupDurableInconsistency::AmbiguousBindingTerminalShape,
            );
            require(
                result,
                StartupReconciliationRequirement::ExternalExecutorObservation,
            );
            require(result, StartupReconciliationRequirement::LeaseEvaluation);
            DurableBindingState::AmbiguousTerminalShape
        }
    };
}

fn classify_context(
    role: &LogicalRole,
    binding: Option<&ExecutorBinding>,
    manifest: Option<&ContextManifest>,
    latest_epoch: Option<&ContextEpoch>,
    attempt: Option<&ContextRehydrationAttempt>,
    result: &mut StartupRecoveryClassification,
) {
    if let Some(manifest) = manifest {
        check(
            manifest.role_id == role.role_id,
            StartupDurableInconsistency::ContextManifestRoleMismatch,
            result,
        );
        check(
            manifest.project_id == role.project_id,
            StartupDurableInconsistency::ContextManifestProjectMismatch,
            result,
        );
        check(
            role.context_manifest_id
                .as_ref()
                .is_none_or(|id| id == &manifest.manifest_id),
            StartupDurableInconsistency::RoleManifestReferenceMismatch,
            result,
        );
    }

    if let Some(epoch) = latest_epoch {
        check(
            epoch.project_id == role.project_id,
            StartupDurableInconsistency::ContextEpochProjectMismatch,
            result,
        );
    }

    if let Some(attempt) = attempt {
        check(
            attempt.durable_role_id == role.role_id,
            StartupDurableInconsistency::RehydrationRoleMismatch,
            result,
        );
        check(
            attempt.project_id == role.project_id,
            StartupDurableInconsistency::RehydrationProjectMismatch,
            result,
        );
        if let Some(manifest) = manifest {
            check(
                manifest.manifest_id == attempt.context_manifest_id,
                StartupDurableInconsistency::RehydrationManifestMismatch,
                result,
            );
        }
        check(
            attempt.context_epoch_id == role.current_context_epoch,
            StartupDurableInconsistency::RehydrationEpochMismatch,
            result,
        );
        if let Some(id) = &attempt.executor_binding_id {
            if let Some(binding) = binding {
                check(
                    id == &binding.binding_id,
                    StartupDurableInconsistency::RehydrationBindingMismatch,
                    result,
                );
            } else {
                require(
                    result,
                    StartupReconciliationRequirement::DurableFactReconciliation,
                );
            }
        }
    }
}

fn check(
    condition: bool,
    failure: StartupDurableInconsistency,
    result: &mut StartupRecoveryClassification,
) {
    if !condition {
        inconsistent(result, failure);
    }
}

fn inconsistent(
    result: &mut StartupRecoveryClassification,
    inconsistency: StartupDurableInconsistency,
) {
    if !result.inconsistencies.contains(&inconsistency) {
        result.inconsistencies.push(inconsistency);
    }
}

fn require(
    result: &mut StartupRecoveryClassification,
    requirement: StartupReconciliationRequirement,
) {
    if !result.requirements.contains(&requirement) {
        result.requirements.push(requirement);
    }
}
