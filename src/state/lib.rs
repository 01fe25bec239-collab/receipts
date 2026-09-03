//! State-layer foundation for the Receipts orchestration core.
//!
//! This crate implements the first slice of `M-STATE-1`: the SQLite-backed
//! State repository boundary, mandatory SQLite configuration, versioned
//! forward-only migrations, and an atomic transaction primitive.
//!
//! Encapsulation rules honored by this crate:
//!
//! * no `rusqlite` type and no raw SQL string crosses the public API;
//! * callers cannot execute arbitrary SQL;
//! * the backing store can be replaced later without other managers ever
//!   depending on SQLite directly.
//!
//! The frozen architecture has a single authoritative State writer (the
//! orchestrator core). This foundation provides no worker-, host-, adapter-,
//! or model-facing mutation path.

pub mod canonical_timestamp;
pub mod context_epoch;
pub mod context_manifest;
pub mod context_rehydration;
pub mod entitlement;
pub mod error;
pub mod event;
pub mod executor_binding;
pub mod executor_binding_lease_expiry;
pub mod logical_role;
pub mod repository;
pub mod trusted_time;

mod migrations;

#[cfg(test)]
mod context_epoch_advance_tests;

#[cfg(test)]
mod canonical_timestamp_tests;

#[cfg(test)]
mod context_epoch_invalidation_tests;

#[cfg(test)]
mod context_epoch_tests;

#[cfg(test)]
mod context_manifest_tests;

#[cfg(test)]
mod context_rehydration_tests;

#[cfg(test)]
mod event_tests;

#[cfg(test)]
mod executor_binding_lease_tests;

#[cfg(test)]
mod executor_binding_lease_expiry_tests;

#[cfg(test)]
mod executor_binding_lease_renewal_temporal_tests;

#[cfg(test)]
mod executor_binding_release_tests;

#[cfg(test)]
mod executor_binding_single_active_tests;

#[cfg(test)]
mod executor_binding_tests;

#[cfg(test)]
mod logical_role_tests;

#[cfg(test)]
mod trusted_time_watermark_tests;

#[cfg(test)]
mod tests;

pub use canonical_timestamp::CanonicalTimestampV1;
pub use context_epoch::{ContextEpoch, ContextEpochTrigger};
pub use context_manifest::{
    ContextManifest, ContextManifestSource, ContextSourceRef, ContextSourceRefType, RequiredFor,
    SourceClass,
};
pub use context_rehydration::{
    ArtifactMaterializer, ArtifactRefV1, BoundContextSourceRefV1, ContextRehydratedEventSupplier,
    ContextRehydrationAttempt, ContextRehydrationOutcome, ContextRehydrationRequest,
    ContextRehydrationSourceEvidence, ContextRehydrationStatus, ContextSourceBindingV1,
    ContextSourceDemand, ContextSourceTouchEvidence, ExternalSourceMaterialization,
    RehydratedContextSource, RepositorySnapshotMaterializer, RepositorySnapshotRefV1,
    SourceDigestComparison, SourceDisposition, SourceMaterializationFailure, StateQueryRefV1,
    context_rehydration_event_payload, context_source_digest_v1,
};
pub use error::StateError;
pub use event::{
    ActorKind, EventActor, EventEnvelope, EventPayloadReference, EventSubject, EventType,
    SubjectKind,
};
pub use executor_binding::{ExecutorBinding, ReleaseReason};
pub use executor_binding_lease_expiry::{
    ExecutorLeaseExpiryOutcomeV1, ExecutorLeaseExpiryRequestV2,
    ExecutorReleasedLifecycleAuthorityV1,
};
pub use logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
pub use repository::SqliteStateRepository;
pub use trusted_time::{TrustedClockV1, TrustedTimeSampleV1, TrustedTimeWatermarkV1};
