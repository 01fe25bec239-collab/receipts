//! Trusted-time lease renewal and explicit lease-expiry release persistence.

use std::fmt::Write as _;

use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::canonical_timestamp::CanonicalTimestampV1;
use crate::error::StateError;
use crate::event::{
    EventActor, EventEnvelope, EventPayloadReference, EventType, SubjectKind, validate_for_append,
};
use crate::executor_binding::{
    ExecutorBinding, ReleaseReason, apply_lease_renewal, apply_release, ensure_identifier,
    read_executor_binding,
};
use crate::repository::SqliteStateRepository;
use crate::trusted_time::{
    TrustedClockV1, TrustedTimeSampleV1, accept_sample, require_current_sample, validate_sample,
};

const DIGEST_DOMAIN: &[u8] = b"multiagent:event-payload-reference:EXECUTOR_RELEASED:v1";
const DIGEST_PREFIX: &str = "sha256:executor-released-provenance:v1:";

/// Orchestration-owned lifecycle authority that State validates exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutorReleasedLifecycleAuthorityV1 {
    pub actor: EventActor,
    pub correlation_id: String,
}

/// The bounded semantic request for explicit lease-expiry evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutorLeaseExpiryRequestV2 {
    pub binding_id: String,
    pub lifecycle_authority: ExecutorReleasedLifecycleAuthorityV1,
    pub executor_released_event: EventEnvelope,
}

/// Observable lease-expiry decision without inventing lifecycle behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutorLeaseExpiryOutcomeV1 {
    NotExpired,
    Released,
    AlreadyLeaseExpired,
}

/// Immutable binding-qualified executor identity used only in local digesting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoundExecutorIdentityV1<'a> {
    provider_id: &'a str,
    model_id: &'a str,
    runtime_id: &'a str,
}

#[derive(Debug, Clone)]
pub(crate) struct FencedTrustedTimeV1 {
    project_id: String,
    sample: TrustedTimeSampleV1,
    timestamp: CanonicalTimestampV1,
}

impl SqliteStateRepository {
    /// Evaluates and, when expired, atomically records release plus event.
    pub fn expire_executor_binding_lease(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: ExecutorLeaseExpiryRequestV2,
    ) -> Result<ExecutorLeaseExpiryOutcomeV1, StateError> {
        let fenced = fence_binding_time(self, clock, &request.binding_id)?;
        apply_expiry_with_fenced_time(self, fenced, request)
    }

    /// Renews only while trusted time is strictly before the current deadline.
    pub fn renew_executor_binding_lease(
        &mut self,
        clock: &dyn TrustedClockV1,
        binding_id: &str,
        lease_expires_at: &str,
    ) -> Result<(), StateError> {
        let fenced = fence_binding_time(self, clock, binding_id)?;
        apply_renewal_with_fenced_time(self, fenced, binding_id, lease_expires_at)
    }
}

pub(crate) fn fence_binding_time(
    repository: &mut SqliteStateRepository,
    clock: &dyn TrustedClockV1,
    binding_id: &str,
) -> Result<FencedTrustedTimeV1, StateError> {
    let (sample, timestamp) = validate_sample(clock.sample()?)?;
    ensure_identifier("binding_id", binding_id)?;
    let project_id = repository.run_serialized_transaction(|uow| {
        let binding = require_binding(uow.tx(), binding_id)?;
        let project_id = binding_project_id(uow.tx(), &binding)?;
        accept_sample(uow.tx(), &project_id, &sample, &timestamp)?;
        Ok(project_id)
    })?;
    Ok(FencedTrustedTimeV1 {
        project_id,
        sample,
        timestamp,
    })
}

pub(crate) fn apply_renewal_with_fenced_time(
    repository: &mut SqliteStateRepository,
    fenced: FencedTrustedTimeV1,
    binding_id: &str,
    lease_expires_at: &str,
) -> Result<(), StateError> {
    repository.run_serialized_transaction(|uow| {
        require_current_sample(
            uow.tx(),
            &fenced.project_id,
            &fenced.sample,
            &fenced.timestamp,
        )?;
        let binding = require_binding(uow.tx(), binding_id)?;
        require_same_project(uow.tx(), &binding, &fenced.project_id)?;
        require_unreleased(&binding)?;
        let current_deadline = CanonicalTimestampV1::parse(&binding.lease_expires_at)?;
        if fenced.timestamp >= current_deadline {
            return Err(StateError::ExecutorLeaseRenewalRefused {
                binding_id: binding_id.to_string(),
                trusted_now: fenced.sample.canonical_utc_timestamp.clone(),
                deadline: binding.lease_expires_at,
            });
        }
        CanonicalTimestampV1::parse(lease_expires_at)?;
        apply_lease_renewal(uow.tx(), binding_id, lease_expires_at)
    })
}

pub(crate) fn apply_expiry_with_fenced_time(
    repository: &mut SqliteStateRepository,
    fenced: FencedTrustedTimeV1,
    request: ExecutorLeaseExpiryRequestV2,
) -> Result<ExecutorLeaseExpiryOutcomeV1, StateError> {
    repository.run_serialized_transaction(|uow| {
        require_current_sample(
            uow.tx(),
            &fenced.project_id,
            &fenced.sample,
            &fenced.timestamp,
        )?;
        let binding = require_binding(uow.tx(), &request.binding_id)?;
        let project_id = binding_project_id(uow.tx(), &binding)?;
        if project_id != fenced.project_id {
            return Err(provenance("binding project changed after temporal fence"));
        }
        match (binding.released_at.as_deref(), binding.release_reason) {
            (Some(_), Some(ReleaseReason::LeaseExpired)) => {
                return Ok(ExecutorLeaseExpiryOutcomeV1::AlreadyLeaseExpired);
            }
            (None, None) => {}
            _ => {
                return Err(StateError::ExecutorBindingAlreadyReleased {
                    binding_id: binding.binding_id,
                });
            }
        }

        let deadline = CanonicalTimestampV1::parse(&binding.lease_expires_at)?;
        if fenced.timestamp < deadline {
            return Ok(ExecutorLeaseExpiryOutcomeV1::NotExpired);
        }

        validate_expiry_event(
            &request.executor_released_event,
            &request.lifecycle_authority,
            &binding,
            &project_id,
            &fenced.sample.canonical_utc_timestamp,
        )?;
        apply_release(
            uow.tx(),
            &binding.binding_id,
            &fenced.sample.canonical_utc_timestamp,
            ReleaseReason::LeaseExpired,
        )?;
        uow.insert_event(&request.executor_released_event)?;
        Ok(ExecutorLeaseExpiryOutcomeV1::Released)
    })
}

fn validate_expiry_event(
    event: &EventEnvelope,
    authority: &ExecutorReleasedLifecycleAuthorityV1,
    binding: &ExecutorBinding,
    project_id: &str,
    released_at: &str,
) -> Result<(), StateError> {
    validate_for_append(event)?;
    if event.event_type != EventType::ExecutorReleased {
        return Err(provenance("event_type must be EXECUTOR_RELEASED"));
    }
    if event.project_id != project_id {
        return Err(provenance(
            "event project_id does not match binding project",
        ));
    }
    if event.subject.kind != SubjectKind::Role || event.subject.id != binding.role_id {
        return Err(provenance("event subject must be the exact bound role"));
    }
    if event.actor != authority.actor {
        return Err(provenance(
            "event actor does not exactly match lifecycle authority",
        ));
    }
    if authority.correlation_id.is_empty() || event.correlation_id != authority.correlation_id {
        return Err(provenance(
            "event correlation_id does not exactly match valid lifecycle authority",
        ));
    }
    if event.occurred_at != released_at {
        return Err(provenance(
            "event occurred_at does not match trusted release time",
        ));
    }

    let canonical = serde_json_canonicalizer::pipe(&event.payload.reference)
        .map_err(|_| provenance("payload reference is not valid JSON"))?;
    if canonical != event.payload.reference {
        return Err(provenance(
            "payload reference is not RFC8785 canonical JSON",
        ));
    }
    let expected = expected_provenance(binding, project_id, released_at)?;
    if event.payload.reference != expected.reference {
        return Err(provenance(
            "payload reference is not the exact binding/project v1 reference",
        ));
    }
    if event.payload.digest != expected.digest {
        return Err(provenance(
            "payload digest does not match State-local projection",
        ));
    }
    Ok(())
}

#[derive(Serialize)]
struct ExecutorReleasedProvenanceRefV1<'a> {
    binding_id: &'a str,
    project_id: &'a str,
    v: u8,
}

#[derive(Serialize)]
struct ExecutorReleasedProvenanceDigestProjectionV1<'a> {
    binding_id: &'a str,
    model_id: &'a str,
    project_id: &'a str,
    provider_id: &'a str,
    release_reason: &'static str,
    released_at: &'a str,
    role_id: &'a str,
    runtime_id: &'a str,
    v: u8,
}

pub(crate) fn expected_provenance(
    binding: &ExecutorBinding,
    project_id: &str,
    released_at: &str,
) -> Result<EventPayloadReference, StateError> {
    let reference = serde_json_canonicalizer::to_string(&ExecutorReleasedProvenanceRefV1 {
        binding_id: &binding.binding_id,
        project_id,
        v: 1,
    })
    .map_err(|error| provenance(&format!("reference canonicalization failed: {error}")))?;
    let identity = BoundExecutorIdentityV1 {
        provider_id: &binding.provider_id,
        model_id: &binding.model_id,
        runtime_id: &binding.runtime_id,
    };
    let projection =
        serde_json_canonicalizer::to_vec(&ExecutorReleasedProvenanceDigestProjectionV1 {
            binding_id: &binding.binding_id,
            model_id: identity.model_id,
            project_id,
            provider_id: identity.provider_id,
            release_reason: ReleaseReason::LeaseExpired.as_str(),
            released_at,
            role_id: &binding.role_id,
            runtime_id: identity.runtime_id,
            v: 1,
        })
        .map_err(|error| provenance(&format!("digest canonicalization failed: {error}")))?;
    let mut hasher = Sha256::new();
    hasher.update(DIGEST_DOMAIN);
    hasher.update([0]);
    hasher.update(projection);
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    Ok(EventPayloadReference {
        reference,
        digest: format!("{DIGEST_PREFIX}{hex}"),
    })
}

fn require_binding(conn: &Connection, binding_id: &str) -> Result<ExecutorBinding, StateError> {
    read_executor_binding(conn, binding_id)?.ok_or_else(|| StateError::ExecutorBindingNotFound {
        binding_id: binding_id.to_string(),
    })
}

fn binding_project_id(conn: &Connection, binding: &ExecutorBinding) -> Result<String, StateError> {
    conn.query_row(
        "SELECT project_id FROM logical_role WHERE role_id = ?1",
        [&binding.role_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(|error| StateError::InternalQueryFailed {
        detail: error.to_string(),
    })?
    .ok_or_else(|| StateError::ExecutorBindingDecodeFailed {
        detail: format!("binding {:?} references a missing role", binding.binding_id),
    })
}

fn require_same_project(
    conn: &Connection,
    binding: &ExecutorBinding,
    fenced_project_id: &str,
) -> Result<(), StateError> {
    if binding_project_id(conn, binding)? != fenced_project_id {
        return Err(provenance("binding project changed after temporal fence"));
    }
    Ok(())
}

fn require_unreleased(binding: &ExecutorBinding) -> Result<(), StateError> {
    if binding.released_at.is_some() || binding.release_reason.is_some() {
        return Err(StateError::ExecutorBindingAlreadyReleased {
            binding_id: binding.binding_id.clone(),
        });
    }
    Ok(())
}

fn provenance(detail: &str) -> StateError {
    StateError::ExecutorReleasedProvenanceInvalid {
        detail: detail.to_string(),
    }
}
