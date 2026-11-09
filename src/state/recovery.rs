//! Acting startup recovery, atomic failover rebind, the mandatory
//! rehydration gate and changed-source epoch succession (ST-W2-1/2).
//!
//! Every action here decides only from durable State facts read inside one
//! serialized transaction plus one trusted-clock sample that the same
//! transaction fences against the project's monotonic watermark. A missing
//! record is never read as proof of absence elsewhere, and nothing here
//! observes external reality: State runs no Git command, kills no process,
//! resets no checkout and does not decide routing or failover policy. The
//! trusted core (holding a [`crate::StateWriteAdmission`]) asks for each
//! action; State re-checks the facts and either commits the whole action
//! (rows, role history and its frozen event) or writes nothing.
//!
//! The frozen role status vocabulary (`ACTIVE`, `SUSPENDED`, `RETIRED`) is
//! unchanged. "Needs rebind" is not a status: it is the typed continuation
//! refusal [`RecoveryRefusalV1::NoActiveBinding`] /
//! [`RecoveryRefusalV1::BindingReleased`] derived from the durable binding
//! pointer, and rehydration-before-continuation is
//! [`SqliteStateRepository::continuation_clearance`].
//!
//! Events use only the frozen set: `EXECUTOR_RELEASED`, `EXECUTOR_BOUND`,
//! `EXECUTOR_REPLACED` and `CONTEXT_EPOCH_ADVANCED`, with State-derived
//! canonical references and domain-separated digests. Their identifiers are
//! derived deterministically from the action's durable subject (a ULID-shaped
//! SHA-256 prefix), so a repeated action can never mint a second event.

use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::attempt::{AttemptRecord, verified_unfinished_attempts};
use crate::canonical_timestamp::CanonicalTimestampV1;
use crate::context_epoch::{
    ChangedSource, ChangedSourceRefType, ChangedSources, ContextEpoch, ContextEpochTrigger,
    advance_in_transaction,
};
use crate::context_manifest::{
    ContextSourceRefType, insert_context_manifest, read_context_manifest, validate_for_create,
};
use crate::context_rehydration::{ContextRehydrationStatus, SourceDigestComparison, read_attempt};
use crate::epoch_value::StateEpochValueV1;
use crate::error::StateError;
use crate::event::{
    EventEnvelope, EventPayloadReference, EventSubject, EventType, SubjectKind, insert_event,
    validate_for_append,
};
use crate::executor_binding::{
    ExecutorBinding, ReleaseReason, apply_release, ensure_identifier, read_executor_binding,
};
use crate::executor_binding_lease_expiry::{
    ExecutorReleasedLifecycleAuthorityV1, expected_release_provenance,
};
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType, read_logical_role};
use crate::logical_role_lifecycle::{RoleChangeEvidenceV1, change_binding, change_context};
use crate::repository::SqliteStateRepository;
use crate::startup_recovery::{
    StartupRecoveryClassification, StartupRecoverySnapshot, classify_startup_recovery,
};
use crate::trusted_time::{
    TrustedClockV1, TrustedTimeSampleV1, accept_sample, read_watermark, validate_sample,
};

/// Why State refused continuation or a recovery action. Closed and
/// content-free; these are recovery requirements, not role statuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryRefusalV1 {
    RoleNotFound,
    /// The role is SUSPENDED or RETIRED.
    RoleNotActive,
    /// No binding pointer: the core must route and rebind first.
    NoActiveBinding,
    /// The pointer names a released binding: rebind first.
    BindingReleased,
    /// Trusted now is at or past the binding's lease deadline.
    LeaseExpired,
    NoContextManifest,
    /// A later context epoch invalidated this role; its context must advance.
    ContextInvalidated,
    /// The role's epoch is not the project's latest epoch.
    StaleContextEpoch,
    /// No SUCCEEDED rehydration exists for the current binding, manifest and epoch.
    RehydrationRequired,
    /// The only matching rehydration observed changed sources; the
    /// changed-source succession must run, then rehydration again.
    ChangedSourcesPending,
    /// Durable records contradict each other (see the startup classification).
    DurableInconsistency,
    /// The supplied successor binding is released, claims completed
    /// rehydration, is bound in the future or names another role.
    BindingShapeInvalid,
    /// The successor's lease deadline is not after trusted now.
    LeaseNotAfterNow,
    /// Another RUNTIME_A1 role in the project still holds an unreleased binding.
    OtherA1Bound,
    /// `release_current` was requested but there is no current binding, or
    /// it is already released.
    NothingToRelease,
    AttemptNotFound,
    /// The rehydration attempt is not this role's SUCCEEDED attempt for its
    /// current manifest and epoch.
    AttemptScopeMismatch,
    /// The attempt observed no changed source.
    NoChangedSources,
}

impl RecoveryRefusalV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RoleNotFound => "ROLE_NOT_FOUND",
            Self::RoleNotActive => "ROLE_NOT_ACTIVE",
            Self::NoActiveBinding => "NO_ACTIVE_BINDING",
            Self::BindingReleased => "BINDING_RELEASED",
            Self::LeaseExpired => "LEASE_EXPIRED",
            Self::NoContextManifest => "NO_CONTEXT_MANIFEST",
            Self::ContextInvalidated => "CONTEXT_INVALIDATED",
            Self::StaleContextEpoch => "STALE_CONTEXT_EPOCH",
            Self::RehydrationRequired => "REHYDRATION_REQUIRED",
            Self::ChangedSourcesPending => "CHANGED_SOURCES_PENDING",
            Self::DurableInconsistency => "DURABLE_INCONSISTENCY",
            Self::BindingShapeInvalid => "BINDING_SHAPE_INVALID",
            Self::LeaseNotAfterNow => "LEASE_NOT_AFTER_NOW",
            Self::OtherA1Bound => "OTHER_A1_BOUND",
            Self::NothingToRelease => "NOTHING_TO_RELEASE",
            Self::AttemptNotFound => "ATTEMPT_NOT_FOUND",
            Self::AttemptScopeMismatch => "ATTEMPT_SCOPE_MISMATCH",
            Self::NoChangedSources => "NO_CHANGED_SOURCES",
        }
    }

    /// True when the core must bind a new executor before anything else.
    pub fn requires_rebind(self) -> bool {
        matches!(
            self,
            Self::NoActiveBinding | Self::BindingReleased | Self::LeaseExpired
        )
    }
}

fn refused(role_id: &str, reason: RecoveryRefusalV1) -> StateError {
    StateError::RecoveryRefused {
        role_id: role_id.to_string(),
        reason,
    }
}

/// Proof that a role may continue now: an unreleased in-lease binding and
/// a SUCCEEDED rehydration with no changed source for exactly that binding,
/// the role's current manifest and its current, non-invalidated epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinuationClearanceV1 {
    pub role_id: String,
    pub binding_id: String,
    pub context_manifest_id: String,
    pub context_epoch: StateEpochValueV1,
    pub rehydration_attempt_id: String,
}

/// Returned by every successful rebind: the new executor may not act until
/// a rehydration for this binding/manifest/epoch succeeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RehydrationRequiredV1 {
    pub role_id: String,
    pub binding_id: String,
    pub context_manifest_id: Option<String>,
    pub context_epoch: StateEpochValueV1,
    pub released_event_id: Option<String>,
    pub bound_event_id: String,
}

/// One explicit-CAS failover rebind.
#[derive(Debug, Clone, PartialEq)]
pub struct RoleRebindRequestV1 {
    pub role_id: String,
    /// Expected current binding pointer (compare-and-set).
    pub expected_binding_id: Option<String>,
    /// When set, the expected binding is still unreleased and is released
    /// with this reason in the same transaction (`LEASE_EXPIRED` is reserved
    /// to the trusted lease-expiry path).
    pub release_current: Option<ReleaseReason>,
    /// The successor binding: unreleased, not yet rehydrated, bound no later
    /// than trusted now, with a lease deadline after trusted now.
    pub binding: ExecutorBinding,
    /// Orchestration lifecycle authority recorded on the events.
    pub authority: ExecutorReleasedLifecycleAuthorityV1,
    /// Opaque role-history reason; never a prompt, output or credential.
    pub reason: Option<String>,
}

/// One startup role conclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRecoveryV1 {
    pub role_id: String,
    pub classification: StartupRecoveryClassification,
    pub continuation: Result<ContinuationClearanceV1, RecoveryRefusalV1>,
}

/// What one startup scan did and found, from one snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupRecoveryReportV1 {
    pub project_id: String,
    pub trusted_now: String,
    /// Bindings this scan released with `LEASE_EXPIRED` (empty on a repeat).
    pub lease_expired_binding_ids: Vec<String>,
    pub roles: Vec<RoleRecoveryV1>,
    /// Started-but-unfinished attempts, for the Workspace recovery owner.
    pub unfinished_attempts: Vec<AttemptRecord>,
}

/// One changed-source succession: advance the epoch, invalidate the role,
/// attach an immutable successor manifest carrying the observed digests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedSourceSuccessionRequestV1 {
    pub role_id: String,
    pub rehydration_attempt_id: String,
    pub successor_manifest_id: String,
    pub trigger: ContextEpochTrigger,
    pub authority: ExecutorReleasedLifecycleAuthorityV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedSourceSuccessionV1 {
    pub epoch: ContextEpoch,
    pub successor_manifest_id: String,
    pub changed_sources: Vec<ChangedSource>,
    pub event_id: String,
}

struct Now {
    sample: TrustedTimeSampleV1,
    at: CanonicalTimestampV1,
}

fn sample(clock: &dyn TrustedClockV1) -> Result<Now, StateError> {
    let (sample, at) = validate_sample(clock.sample()?)?;
    Ok(Now { sample, at })
}

fn validate_authority(authority: &ExecutorReleasedLifecycleAuthorityV1) -> Result<(), StateError> {
    if authority.correlation_id.is_empty() || authority.actor.id.as_deref() == Some("") {
        return Err(StateError::ExecutorReleasedProvenanceInvalid {
            detail: "lifecycle authority needs a correlation id and a non-empty actor id"
                .to_string(),
        });
    }
    Ok(())
}

impl SqliteStateRepository {
    /// Read-only continuation gate. Writes nothing (not even the watermark):
    /// a sample behind the project's watermark is refused as a regression.
    pub fn continuation_clearance(
        &self,
        clock: &dyn TrustedClockV1,
        role_id: &str,
    ) -> Result<ContinuationClearanceV1, StateError> {
        ensure_identifier("role_id", role_id)?;
        let now = sample(clock)?;
        let tx = self
            .connection()
            .unchecked_transaction()
            .map_err(query_failure)?;
        let role = read_logical_role(&tx, role_id)?
            .ok_or_else(|| refused(role_id, RecoveryRefusalV1::RoleNotFound))?;
        check_not_behind_watermark(&tx, &role.project_id, &now)?;
        let outcome = clearance(&tx, &role, &now.at)?;
        tx.commit().map_err(query_failure)?;
        outcome.map_err(|reason| refused(role_id, reason))
    }

    /// Startup scan for one project: releases every unreleased binding whose
    /// lease has expired against trusted now (with its `EXECUTOR_RELEASED`
    /// event), then reports each role's classification and continuation
    /// requirement plus the project's unfinished attempts. One serialized
    /// transaction; any refusal or corrupt fact writes nothing. A repeat
    /// scan finds nothing left to release and changes nothing.
    pub(crate) fn recover_startup(
        &mut self,
        clock: &dyn TrustedClockV1,
        project_id: &str,
        authority: &ExecutorReleasedLifecycleAuthorityV1,
    ) -> Result<StartupRecoveryReportV1, StateError> {
        ensure_identifier("project_id", project_id)?;
        validate_authority(authority)?;
        let now = sample(clock)?;
        self.run_serialized_transaction(|uow| {
            let conn = uow.tx();
            accept_sample(conn, project_id, &now.sample, &now.at)?;
            let role_ids = project_role_ids(conn, project_id)?;
            let mut released = Vec::new();
            for role_id in &role_ids {
                let Some(binding) = blocking_binding(conn, role_id)? else {
                    continue;
                };
                if binding.released_at.is_some() || binding.release_reason.is_some() {
                    continue; // partial terminal shape: reported, never repaired
                }
                let deadline = CanonicalTimestampV1::parse(&binding.lease_expires_at)?;
                if now.at < deadline {
                    continue;
                }
                let role = read_logical_role(conn, role_id)?.ok_or_else(query_corrupt)?;
                release_with_event(
                    conn,
                    &role,
                    &binding,
                    ReleaseReason::LeaseExpired,
                    &now,
                    authority,
                )?;
                released.push(binding.binding_id);
            }
            let mut roles = Vec::with_capacity(role_ids.len());
            for role_id in &role_ids {
                let role = read_logical_role(conn, role_id)?.ok_or_else(query_corrupt)?;
                let classification = classify(conn, &role)?;
                let continuation = if classification.is_fail_closed() {
                    Err(RecoveryRefusalV1::DurableInconsistency)
                } else {
                    clearance(conn, &role, &now.at)?
                };
                roles.push(RoleRecoveryV1 {
                    role_id: role_id.clone(),
                    classification,
                    continuation,
                });
            }
            Ok(StartupRecoveryReportV1 {
                project_id: project_id.to_string(),
                trusted_now: now.sample.canonical_utc_timestamp.clone(),
                lease_expired_binding_ids: released,
                roles,
                unfinished_attempts: verified_unfinished_attempts(conn, project_id)?,
            })
        })
    }

    /// Atomic failover: optionally release the expected binding, create the
    /// successor binding, CAS the role pointer with history and append
    /// `EXECUTOR_RELEASED` / `EXECUTOR_BOUND` or `EXECUTOR_REPLACED`, all in
    /// one serialized transaction. The role keeps its identity and status.
    pub(crate) fn rebind_role(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: &RoleRebindRequestV1,
    ) -> Result<RehydrationRequiredV1, StateError> {
        let role_id = request.role_id.as_str();
        ensure_identifier("role_id", role_id)?;
        validate_authority(&request.authority)?;
        if request.release_current == Some(ReleaseReason::LeaseExpired) {
            return Err(StateError::ExecutorBindingValidation {
                detail: "LEASE_EXPIRED is reserved to the trusted lease-expiry transaction"
                    .to_string(),
            });
        }
        if request.reason.as_deref() == Some("") {
            return Err(StateError::LogicalRoleValidation {
                detail: "reason must not be empty when present".to_string(),
            });
        }
        let binding = &request.binding;
        if binding.role_id != role_id
            || binding.released_at.is_some()
            || binding.release_reason.is_some()
            || binding.rehydration_completed == Some(true)
        {
            return Err(refused(role_id, RecoveryRefusalV1::BindingShapeInvalid));
        }
        let bound_at = CanonicalTimestampV1::parse(&binding.bound_at)?;
        let deadline = CanonicalTimestampV1::parse(&binding.lease_expires_at)?;
        let now = sample(clock)?;
        if bound_at > now.at {
            return Err(refused(role_id, RecoveryRefusalV1::BindingShapeInvalid));
        }
        if deadline <= now.at {
            return Err(refused(role_id, RecoveryRefusalV1::LeaseNotAfterNow));
        }
        self.run_serialized_transaction(|uow| {
            let conn = uow.tx();
            let role = read_logical_role(conn, role_id)?
                .ok_or_else(|| refused(role_id, RecoveryRefusalV1::RoleNotFound))?;
            accept_sample(conn, &role.project_id, &now.sample, &now.at)?;
            if role.status != LogicalRoleStatus::Active {
                return Err(refused(role_id, RecoveryRefusalV1::RoleNotActive));
            }
            let expected = request.expected_binding_id.as_deref();
            if role.active_binding_id.as_deref() != expected {
                return Err(crate::logical_role_lifecycle::refused(
                    role_id,
                    crate::RoleChangeRefusal::StaleExpectation,
                ));
            }
            let mut released_event_id = None;
            if let Some(reason) = request.release_current {
                let current = expected
                    .map(|id| read_executor_binding(conn, id))
                    .transpose()?
                    .flatten()
                    .filter(|b| b.released_at.is_none() && b.release_reason.is_none())
                    .ok_or_else(|| refused(role_id, RecoveryRefusalV1::NothingToRelease))?;
                released_event_id = Some(release_with_event(
                    conn,
                    &role,
                    &current,
                    reason,
                    &now,
                    &request.authority,
                )?);
            }
            if role.role_type == LogicalRoleType::RuntimeA1
                && other_a1_bound(conn, &role.project_id, role_id)?
            {
                return Err(refused(role_id, RecoveryRefusalV1::OtherA1Bound));
            }
            uow.insert_executor_binding(binding)?;
            let evidence = RoleChangeEvidenceV1 {
                changed_at: now.at.clone(),
                reason: request.reason.clone(),
            };
            change_binding(
                conn,
                role_id,
                expected,
                Some(&binding.binding_id),
                &evidence,
            )?;
            let event_type = if expected.is_some() {
                EventType::ExecutorReplaced
            } else {
                EventType::ExecutorBound
            };
            let payload = bound_payload(&role, binding, expected, event_type)?;
            let bound_event_id =
                derived_event_id(&[event_type.as_str(), &role.project_id, &binding.binding_id]);
            append_role_event(
                conn,
                &role,
                event_type,
                &bound_event_id,
                &now,
                &request.authority,
                payload,
                role.current_context_epoch.clone(),
            )?;
            Ok(RehydrationRequiredV1 {
                role_id: role_id.to_string(),
                binding_id: binding.binding_id.clone(),
                context_manifest_id: role.context_manifest_id.clone(),
                context_epoch: role.current_context_epoch.clone(),
                released_event_id,
                bound_event_id,
            })
        })
    }

    /// Changed-source succession after a SUCCEEDED rehydration that observed
    /// changed digests: advances the project epoch (changed sources recorded,
    /// this role invalidated), inserts an immutable successor manifest with
    /// the observed digests at the new epoch, CAS-moves the role's context
    /// with history and appends `CONTEXT_EPOCH_ADVANCED`. One serialized
    /// transaction. The role must rehydrate again at the new epoch before
    /// [`Self::continuation_clearance`] clears it.
    pub(crate) fn succeed_changed_context_sources(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: &ChangedSourceSuccessionRequestV1,
    ) -> Result<ChangedSourceSuccessionV1, StateError> {
        let role_id = request.role_id.as_str();
        ensure_identifier("role_id", role_id)?;
        ensure_identifier("rehydration_attempt_id", &request.rehydration_attempt_id)?;
        ensure_identifier("successor_manifest_id", &request.successor_manifest_id)?;
        validate_authority(&request.authority)?;
        let now = sample(clock)?;
        self.run_serialized_transaction(|uow| {
            let conn = uow.tx();
            let role = read_logical_role(conn, role_id)?
                .ok_or_else(|| refused(role_id, RecoveryRefusalV1::RoleNotFound))?;
            accept_sample(conn, &role.project_id, &now.sample, &now.at)?;
            if role.status == LogicalRoleStatus::Retired {
                return Err(refused(role_id, RecoveryRefusalV1::RoleNotActive));
            }
            let manifest_id = role
                .context_manifest_id
                .clone()
                .ok_or_else(|| refused(role_id, RecoveryRefusalV1::NoContextManifest))?;
            let latest = latest_epoch(conn, &role.project_id)?;
            if latest.as_ref() != Some(&role.current_context_epoch) {
                return Err(refused(role_id, RecoveryRefusalV1::StaleContextEpoch));
            }
            let attempt = read_attempt(conn, &role.project_id, &request.rehydration_attempt_id)?
                .ok_or_else(|| refused(role_id, RecoveryRefusalV1::AttemptNotFound))?;
            if attempt.durable_role_id != role_id
                || attempt.context_manifest_id != manifest_id
                || attempt.context_epoch_id != role.current_context_epoch
                || attempt.status != ContextRehydrationStatus::Succeeded
            {
                return Err(refused(role_id, RecoveryRefusalV1::AttemptScopeMismatch));
            }
            let mut manifest = read_context_manifest(conn, &manifest_id)?.ok_or_else(|| {
                StateError::ContextManifestDecodeFailed {
                    detail: "role manifest pointer is dangling".to_string(),
                }
            })?;
            let mut changed = Vec::new();
            for evidence in &attempt.source_evidence {
                if evidence.comparison != SourceDigestComparison::Changed {
                    continue;
                }
                let observed = evidence.observed_digest.clone().ok_or_else(|| {
                    StateError::ContextRehydrationDecodeFailed {
                        detail: "CHANGED evidence without an observed digest".to_string(),
                    }
                })?;
                let source = manifest
                    .sources
                    .get_mut(evidence.source_ordinal)
                    .ok_or_else(|| refused(role_id, RecoveryRefusalV1::AttemptScopeMismatch))?;
                changed.push(ChangedSource {
                    ref_type: match source.r#ref.ref_type {
                        ContextSourceRefType::RepoPath => ChangedSourceRefType::RepoPath,
                        ContextSourceRefType::StateQuery => ChangedSourceRefType::StateQuery,
                        ContextSourceRefType::ArtifactId => ChangedSourceRefType::ArtifactId,
                    },
                    target: source.r#ref.target.clone(),
                    digest: Some(observed.clone()),
                    section: None,
                });
                source.digest = observed;
                source.last_read_at = Some(attempt.completed_at.clone());
            }
            if changed.is_empty() {
                return Err(refused(role_id, RecoveryRefusalV1::NoChangedSources));
            }
            let epoch = advance_in_transaction(
                conn,
                &role.project_id,
                now.at.as_str(),
                request.trigger,
                std::slice::from_ref(&request.role_id),
                &ChangedSources::Present(changed.clone()),
            )?;
            manifest.manifest_id = request.successor_manifest_id.clone();
            manifest.epoch = epoch.epoch.clone();
            manifest.created_at = now.at.as_str().to_string();
            manifest.last_rehydrated_at = Some(attempt.completed_at.clone());
            validate_for_create(&manifest)?;
            insert_context_manifest(conn, &manifest, false)?;
            let evidence = RoleChangeEvidenceV1 {
                changed_at: now.at.clone(),
                reason: None,
            };
            change_context(
                conn,
                role_id,
                &role.current_context_epoch,
                Some(&manifest_id),
                &epoch.epoch,
                Some(&manifest.manifest_id),
                &evidence,
            )?;
            let payload = succession_payload(
                &role,
                &epoch,
                &manifest_id,
                &manifest.manifest_id,
                &request.rehydration_attempt_id,
                &changed,
            )?;
            let event_id = derived_event_id(&[
                EventType::ContextEpochAdvanced.as_str(),
                &role.project_id,
                epoch.epoch.as_str(),
            ]);
            append_role_event(
                conn,
                &role,
                EventType::ContextEpochAdvanced,
                &event_id,
                &now,
                &request.authority,
                payload,
                epoch.epoch.clone(),
            )?;
            Ok(ChangedSourceSuccessionV1 {
                epoch,
                successor_manifest_id: manifest.manifest_id,
                changed_sources: changed,
                event_id,
            })
        })
    }
}

fn check_not_behind_watermark(
    conn: &Connection,
    project_id: &str,
    now: &Now,
) -> Result<(), StateError> {
    let Some(mark) = read_watermark(conn, project_id)? else {
        return Ok(());
    };
    if mark.clock_source_id != now.sample.clock_source_id
        || mark.clock_contract_version != now.sample.clock_contract_version
    {
        return Err(StateError::TrustedClockContinuityUnbound {
            project_id: project_id.to_string(),
        });
    }
    let accepted = CanonicalTimestampV1::parse(&mark.last_accepted_trusted_time)?;
    if now.at < accepted {
        return Err(StateError::TrustedClockRegression {
            project_id: project_id.to_string(),
            sample: now.sample.canonical_utc_timestamp.clone(),
            watermark: mark.last_accepted_trusted_time,
        });
    }
    Ok(())
}

/// The continuation decision for one role at trusted `now` (inner `Err` is
/// a refusal; outer `Err` is a storage or decode failure).
fn clearance(
    conn: &Connection,
    role: &LogicalRole,
    now: &CanonicalTimestampV1,
) -> Result<Result<ContinuationClearanceV1, RecoveryRefusalV1>, StateError> {
    use RecoveryRefusalV1 as R;
    if role.status != LogicalRoleStatus::Active {
        return Ok(Err(R::RoleNotActive));
    }
    let Some(binding_id) = role.active_binding_id.as_deref() else {
        return Ok(Err(R::NoActiveBinding));
    };
    let Some(binding) = read_executor_binding(conn, binding_id)? else {
        return Ok(Err(R::DurableInconsistency));
    };
    if binding.role_id != role.role_id {
        return Ok(Err(R::DurableInconsistency));
    }
    if binding.released_at.is_some() || binding.release_reason.is_some() {
        return Ok(Err(R::BindingReleased));
    }
    if *now >= CanonicalTimestampV1::parse(&binding.lease_expires_at)? {
        return Ok(Err(R::LeaseExpired));
    }
    let Some(manifest_id) = role.context_manifest_id.as_deref() else {
        return Ok(Err(R::NoContextManifest));
    };
    if invalidated_after(conn, role)? {
        return Ok(Err(R::ContextInvalidated));
    }
    let mut attempts = scope_attempts(conn, role, manifest_id, binding_id)?;
    let Some(manifest) = read_context_manifest(conn, manifest_id)? else {
        return Ok(Err(R::DurableInconsistency));
    };
    for attempt in &mut attempts {
        if !attempt.failed && !attempt.changed {
            attempt.failed = match read_attempt(conn, &role.project_id, &attempt.attempt_id) {
                Ok(Some(evidence)) => {
                    !crate::context_rehydration::supports_continuation(&evidence, &manifest)
                }
                Ok(None) | Err(StateError::ContextRehydrationDecodeFailed { .. }) => true,
                Err(error) => return Err(error),
            };
        }
    }
    // An attempt with no State-assigned order cannot be placed against the
    // others: fail closed rather than guess.
    if attempts.iter().any(|a| a.order.is_none()) {
        return Ok(Err(R::DurableInconsistency));
    }
    // CHANGED evidence stays pending for this epoch whatever came later: only
    // the changed-source succession (a new epoch + manifest) reconciles it.
    if attempts.iter().any(|a| a.changed) {
        return Ok(Err(R::ChangedSourcesPending));
    }
    // A FAILED reread invalidates every success that began reading before it
    // committed (`fence < sequence`). Ordering is State's commit sequence only.
    let newest_failure = attempts
        .iter()
        .filter(|a| a.failed)
        .filter_map(|a| a.order)
        .map(|o| o.sequence)
        .max()
        .unwrap_or(0);
    let legacy_failure = attempts
        .iter()
        .any(|a| a.failed && a.order.is_some_and(|o| o.sequence == 0));
    let best = attempts
        .iter()
        .filter(|a| !a.failed)
        .filter_map(|a| a.order.map(|o| (a, o)))
        .filter(|(_, o)| {
            if o.sequence == 0 {
                // Upgraded history has no recorded order: it clears only when
                // the scope holds no failure at all (nothing ambiguous).
                !legacy_failure && newest_failure == 0
            } else {
                o.fence >= newest_failure
            }
        })
        .max_by(|(a, x), (b, y)| {
            (x.sequence, a.attempt_id.as_str()).cmp(&(y.sequence, b.attempt_id.as_str()))
        });
    let Some((attempt, _)) = best else {
        return Ok(Err(R::RehydrationRequired));
    };
    // Re-validates the stored attempt and its evidence before relying on it.
    read_attempt(conn, &role.project_id, &attempt.attempt_id)?.ok_or_else(query_corrupt)?;
    Ok(Ok(ContinuationClearanceV1 {
        role_id: role.role_id.clone(),
        binding_id: binding_id.to_string(),
        context_manifest_id: manifest_id.to_string(),
        context_epoch: role.current_context_epoch.clone(),
        rehydration_attempt_id: attempt.attempt_id.clone(),
    }))
}

/// State's durable position of one rehydration attempt.
#[derive(Clone, Copy)]
struct Order {
    /// Per-project commit sequence (0 = recorded before ordering existed).
    sequence: i64,
    /// The highest sequence committed when the attempt began reading.
    fence: i64,
}

/// One attempt of exactly the role's project/role/manifest/epoch/binding.
struct ScopeAttempt {
    attempt_id: String,
    failed: bool,
    changed: bool,
    order: Option<Order>,
}

fn scope_attempts(
    conn: &Connection,
    role: &LogicalRole,
    manifest_id: &str,
    binding_id: &str,
) -> Result<Vec<ScopeAttempt>, StateError> {
    let mut statement = conn
        .prepare(
            "SELECT a.rehydration_attempt_id, a.status = 'FAILED',
                    EXISTS (SELECT 1 FROM context_rehydration_source_evidence e
                            WHERE e.project_id = a.project_id
                              AND e.rehydration_attempt_id = a.rehydration_attempt_id
                              AND e.comparison = 'CHANGED'),
                    o.sequence, o.fence_sequence
             FROM context_rehydration_attempt a
             LEFT JOIN context_rehydration_order o
               ON o.project_id = a.project_id AND o.rehydration_attempt_id = a.rehydration_attempt_id
             WHERE a.project_id = ?1 AND a.durable_role_id = ?2 AND a.context_manifest_id = ?3
               AND a.context_epoch_id = ?4 AND a.executor_binding_id = ?5
             ORDER BY a.rehydration_attempt_id COLLATE BINARY",
        )
        .map_err(query_failure)?;
    statement
        .query_map(
            params![
                role.project_id,
                role.role_id,
                manifest_id,
                role.current_context_epoch.as_str(),
                binding_id
            ],
            |r| {
                let sequence: Option<i64> = r.get(3)?;
                let fence: Option<i64> = r.get(4)?;
                Ok(ScopeAttempt {
                    attempt_id: r.get(0)?,
                    failed: r.get(1)?,
                    changed: r.get(2)?,
                    order: sequence
                        .zip(fence)
                        .map(|(sequence, fence)| Order { sequence, fence }),
                })
            },
        )
        .map_err(query_failure)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(query_failure)
}

/// The highest rehydration commit sequence in `project_id` (0 when none).
/// A reread records it BEFORE it reads, as the fence of its own attempt.
pub(crate) fn rehydration_high_water(
    conn: &Connection,
    project_id: &str,
) -> Result<i64, StateError> {
    conn.query_row(
        "SELECT COALESCE(MAX(sequence), 0) FROM context_rehydration_order WHERE project_id = ?1",
        [project_id],
        |r| r.get(0),
    )
    .map_err(query_failure)
}

fn classify(
    conn: &Connection,
    role: &LogicalRole,
) -> Result<StartupRecoveryClassification, StateError> {
    let binding = match role.active_binding_id.as_deref() {
        Some(id) => read_executor_binding(conn, id)?,
        None => blocking_binding(conn, &role.role_id)?,
    };
    let manifest = role
        .context_manifest_id
        .as_deref()
        .map(|id| read_context_manifest(conn, id))
        .transpose()?
        .flatten();
    let latest = crate::context_epoch::query_latest_context_epoch(conn, &role.project_id)?;
    Ok(classify_startup_recovery(StartupRecoverySnapshot {
        role,
        binding: binding.as_ref(),
        context_manifest: manifest.as_ref(),
        latest_context_epoch: latest.as_ref(),
        rehydration_attempt: None,
    }))
}

fn latest_epoch(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<StateEpochValueV1>, StateError> {
    Ok(crate::context_epoch::query_latest_context_epoch(conn, project_id)?.map(|e| e.epoch))
}

fn project_role_ids(conn: &Connection, project_id: &str) -> Result<Vec<String>, StateError> {
    let mut statement = conn
        .prepare(
            "SELECT role_id FROM logical_role WHERE project_id=?1 ORDER BY role_id COLLATE BINARY",
        )
        .map_err(query_failure)?;
    statement
        .query_map([project_id], |r| r.get::<_, String>(0))
        .map_err(query_failure)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(query_failure)
}

/// The one binding that is not conclusively released, if any; more than one
/// is corruption the single-active index should make impossible.
fn blocking_binding(
    conn: &Connection,
    role_id: &str,
) -> Result<Option<ExecutorBinding>, StateError> {
    let ids = {
        let mut statement = conn
            .prepare("SELECT binding_id FROM executor_binding WHERE role_id=?1 AND (released_at IS NULL OR release_reason IS NULL) ORDER BY binding_id COLLATE BINARY LIMIT 2")
            .map_err(query_failure)?;
        statement
            .query_map([role_id], |r| r.get::<_, String>(0))
            .map_err(query_failure)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(query_failure)?
    };
    match ids.as_slice() {
        [] => Ok(None),
        [id] => read_executor_binding(conn, id),
        _ => Err(StateError::InternalQueryFailed {
            detail: format!("multiple blocking executor bindings for role_id {role_id:?}"),
        }),
    }
}

fn other_a1_bound(conn: &Connection, project_id: &str, role_id: &str) -> Result<bool, StateError> {
    conn.query_row(
        "SELECT 1 FROM logical_role r JOIN executor_binding b ON b.role_id=r.role_id WHERE r.project_id=?1 AND r.role_type='RUNTIME_A1' AND r.role_id<>?2 AND (b.released_at IS NULL OR b.release_reason IS NULL) LIMIT 1",
        params![project_id, role_id],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
    .map_err(query_failure)
}

fn invalidated_after(conn: &Connection, role: &LogicalRole) -> Result<bool, StateError> {
    let mut statement = conn
        .prepare(
            "SELECT epoch FROM context_epoch_invalidated_role WHERE project_id=?1 AND role_id=?2",
        )
        .map_err(query_failure)?;
    let epochs = statement
        .query_map(params![role.project_id, role.role_id], |r| {
            r.get::<_, String>(0)
        })
        .map_err(query_failure)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(query_failure)?;
    for epoch in epochs {
        let epoch = StateEpochValueV1::try_from(epoch).map_err(|_| query_corrupt())?;
        if epoch > role.current_context_epoch {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Releases `binding` with `reason` at trusted now and appends its exact
/// `EXECUTOR_RELEASED` event; returns the event id.
fn release_with_event(
    conn: &Connection,
    role: &LogicalRole,
    binding: &ExecutorBinding,
    reason: ReleaseReason,
    now: &Now,
    authority: &ExecutorReleasedLifecycleAuthorityV1,
) -> Result<String, StateError> {
    let at = now.sample.canonical_utc_timestamp.as_str();
    apply_release(conn, &binding.binding_id, at, reason)?;
    let payload = expected_release_provenance(binding, &role.project_id, at, reason)?;
    let event_id = derived_event_id(&[
        EventType::ExecutorReleased.as_str(),
        &role.project_id,
        &binding.binding_id,
    ]);
    append_role_event(
        conn,
        role,
        EventType::ExecutorReleased,
        &event_id,
        now,
        authority,
        payload,
        role.current_context_epoch.clone(),
    )?;
    Ok(event_id)
}

#[allow(clippy::too_many_arguments)]
fn append_role_event(
    conn: &Connection,
    role: &LogicalRole,
    event_type: EventType,
    event_id: &str,
    now: &Now,
    authority: &ExecutorReleasedLifecycleAuthorityV1,
    payload: EventPayloadReference,
    epoch: StateEpochValueV1,
) -> Result<(), StateError> {
    let event = EventEnvelope {
        event_id: event_id.to_string(),
        project_id: role.project_id.clone(),
        goal_id: None,
        event_type,
        actor: authority.actor.clone(),
        subject: EventSubject {
            kind: SubjectKind::Role,
            id: role.role_id.clone(),
        },
        occurred_at: now.sample.canonical_utc_timestamp.clone(),
        payload,
        correlation_id: authority.correlation_id.clone(),
        epoch,
    };
    validate_for_append(&event)?;
    insert_event(conn, &event)
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A canonical-ULID-shaped id from the first 128 bits of a domain-separated
/// SHA-256 of the action's durable subject.
pub(crate) fn derived_event_id(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"receipts.state.recovery-event-id.v1");
    for part in parts {
        hasher.update([0]);
        hasher.update(part.as_bytes());
    }
    let digest = hasher.finalize();
    let mut bits = u128::from_be_bytes(digest[..16].try_into().expect("16 bytes"));
    let mut out = [0_u8; 26];
    for slot in out.iter_mut().rev() {
        *slot = CROCKFORD[(bits & 31) as usize];
        bits >>= 5;
    }
    String::from_utf8(out.to_vec()).expect("ASCII alphabet")
}

fn payload(
    domain: &str,
    prefix: &str,
    reference: &impl Serialize,
    projection: &impl Serialize,
) -> Result<EventPayloadReference, StateError> {
    let canonical = |error: serde_json::Error| StateError::ExecutorReleasedProvenanceInvalid {
        detail: format!("canonicalization failed: {error}"),
    };
    let reference = serde_json_canonicalizer::to_string(reference).map_err(canonical)?;
    let projection = serde_json_canonicalizer::to_vec(projection).map_err(canonical)?;
    let mut hasher = Sha256::new();
    hasher.update(domain.as_bytes());
    hasher.update([0]);
    hasher.update(projection);
    let mut digest = String::from(prefix);
    for byte in hasher.finalize() {
        write!(&mut digest, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(EventPayloadReference { reference, digest })
}

fn bound_payload(
    role: &LogicalRole,
    binding: &ExecutorBinding,
    previous: Option<&str>,
    event_type: EventType,
) -> Result<EventPayloadReference, StateError> {
    #[derive(Serialize)]
    struct Ref<'a> {
        binding_id: &'a str,
        project_id: &'a str,
        v: u8,
    }
    #[derive(Serialize)]
    struct Projection<'a> {
        binding_id: &'a str,
        bound_at: &'a str,
        lease_expires_at: &'a str,
        model_id: &'a str,
        previous_binding_id: Option<&'a str>,
        project_id: &'a str,
        provider_id: &'a str,
        role_id: &'a str,
        runtime_id: &'a str,
        v: u8,
    }
    let (domain, prefix) = match event_type {
        EventType::ExecutorReplaced => (
            "multiagent:event-payload-reference:EXECUTOR_REPLACED:v1",
            "sha256:executor-replaced-provenance:v1:",
        ),
        _ => (
            "multiagent:event-payload-reference:EXECUTOR_BOUND:v1",
            "sha256:executor-bound-provenance:v1:",
        ),
    };
    payload(
        domain,
        prefix,
        &Ref {
            binding_id: &binding.binding_id,
            project_id: &role.project_id,
            v: 1,
        },
        &Projection {
            binding_id: &binding.binding_id,
            bound_at: &binding.bound_at,
            lease_expires_at: &binding.lease_expires_at,
            model_id: &binding.model_id,
            previous_binding_id: previous,
            project_id: &role.project_id,
            provider_id: &binding.provider_id,
            role_id: &role.role_id,
            runtime_id: &binding.runtime_id,
            v: 1,
        },
    )
}

fn succession_payload(
    role: &LogicalRole,
    epoch: &ContextEpoch,
    from_manifest: &str,
    to_manifest: &str,
    attempt_id: &str,
    changed: &[ChangedSource],
) -> Result<EventPayloadReference, StateError> {
    #[derive(Serialize)]
    struct Ref<'a> {
        epoch: &'a str,
        project_id: &'a str,
        v: u8,
    }
    #[derive(Serialize)]
    struct Item<'a> {
        digest: Option<&'a str>,
        ref_type: &'a str,
        target: &'a str,
    }
    #[derive(Serialize)]
    struct Projection<'a> {
        changed_sources: Vec<Item<'a>>,
        epoch: &'a str,
        from_manifest_id: &'a str,
        project_id: &'a str,
        rehydration_attempt_id: &'a str,
        role_id: &'a str,
        to_manifest_id: &'a str,
        trigger: &'a str,
        v: u8,
    }
    payload(
        "multiagent:event-payload-reference:CONTEXT_EPOCH_ADVANCED:v1",
        "sha256:context-epoch-advanced-provenance:v1:",
        &Ref {
            epoch: epoch.epoch.as_str(),
            project_id: &role.project_id,
            v: 1,
        },
        &Projection {
            changed_sources: changed
                .iter()
                .map(|c| Item {
                    digest: c.digest.as_deref(),
                    ref_type: c.ref_type.as_str(),
                    target: &c.target,
                })
                .collect(),
            epoch: epoch.epoch.as_str(),
            from_manifest_id: from_manifest,
            project_id: &role.project_id,
            rehydration_attempt_id: attempt_id,
            role_id: &role.role_id,
            to_manifest_id: to_manifest,
            trigger: epoch.trigger.as_str(),
            v: 1,
        },
    )
}

fn query_failure(error: rusqlite::Error) -> StateError {
    StateError::InternalQueryFailed {
        detail: error.to_string(),
    }
}

fn query_corrupt() -> StateError {
    StateError::InternalQueryFailed {
        detail: "row disappeared inside one serialized snapshot".to_string(),
    }
}
