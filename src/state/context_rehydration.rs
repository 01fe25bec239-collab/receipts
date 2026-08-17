//! Context rehydration from typed authoritative-source bindings.
//!
//! Repository blobs and artifacts cross explicit materializer ports. State
//! queries use a closed registry implemented here. Raw source bodies are
//! returned to the caller for context construction and are never persisted.

use std::collections::HashSet;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::context_epoch::ContextEpochTrigger;
use crate::context_manifest::{ContextManifest, ContextSourceRefType, SourceClass};
use crate::error::StateError;
use crate::event::{
    ActorKind, EventActor, EventEnvelope, EventPayloadReference, EventType, SubjectKind,
};
use crate::repository::{SqliteStateRepository, UnitOfWork};

const SOURCE_DIGEST_DOMAIN: &[u8] = b"multiagent-context-source-digest\0v1\0";
const ATTEMPT_DIGEST_DOMAIN: &[u8] =
    b"multiagent:event-payload-reference:ContextRehydrationAttempt:v1";
const MAX_IDENTIFIER_LENGTH: usize = 200;
pub const MAX_SOURCE_EVIDENCE_RECORDS_PER_ATTEMPT: usize = 256;
pub const MAX_REPOSITORY_SNAPSHOT_REFERENCES_PER_ATTEMPT: usize = 64;
pub const MAX_REHYDRATION_ATTEMPT_RECORD_CANONICAL_BYTES: usize = 4_194_304;
pub const MAX_SINGLE_SOURCE_EVIDENCE_RECORD_CANONICAL_BYTES: usize = 8_192;
pub const MAX_MATERIALIZED_BYTES_PER_SOURCE: u64 = 8_388_608;
pub const MAX_MATERIALIZED_BYTES_PER_ATTEMPT: u64 = 33_554_432;
pub const MAX_STREAM_CHUNK_BYTES: usize = 65_536;
pub const MAX_SOURCE_IDENTITY_CANONICAL_BYTES: usize = 2_048;
pub const MAX_MATERIALIZER_PROVENANCE_CANONICAL_BYTES: usize = 4_096;
pub const MAX_AUTHORITATIVE_BINDING_CANONICAL_BYTES: usize = 4_096;
pub const MAX_TOUCH_EVIDENCE_REFERENCE_BYTES: usize = 2_048;
pub const MAX_TIMESTAMP_BYTES: usize = 64;
pub const MAX_ACTOR_REFERENCE_BYTES: usize = 256;
pub const MAX_SESSION_REFERENCE_BYTES: usize = 512;
pub const MAX_TASK_REFERENCE_BYTES: usize = 256;
pub const MAX_CORRELATION_REFERENCE_BYTES: usize = 256;
pub const MAX_TRIGGER_REFERENCE_BYTES: usize = 512;
pub const MAX_FAILURE_CODE_BYTES: usize = 64;
pub const MAX_OTHER_EXTERNAL_REFERENCE_BYTES: usize = 1_024;
pub const MAX_EXTERNAL_ERROR_CAPTURE_BYTES: usize = 65_536;
pub const MAX_FAILURE_DETAIL_PERSISTED_BYTES: usize = 4_096;
pub const MAX_EVIDENCE_STRUCTURED_DEPTH: usize = 16;
pub const MAX_EVIDENCE_OBJECT_MEMBERS: usize = 128;
pub const MAX_EVIDENCE_ARRAY_ELEMENTS: usize = 256;
pub const MAX_EVIDENCE_STRUCTURED_TOTAL_NODES: usize = 8_192;
pub const MAX_EXTERNAL_STRUCTURED_REFERENCE_CANONICAL_BYTES: usize = 4_096;
pub const MAX_STATE_QUERY_RESULT_CANONICAL_BYTES: usize = 8_388_608;
pub const MAX_STATE_QUERY_RESULT_DEPTH: usize = 16;
pub const MAX_STATE_QUERY_OBJECT_MEMBERS: usize = 256;
pub const MAX_STATE_QUERY_ARRAY_ELEMENTS: usize = 4_096;
pub const MAX_STATE_QUERY_TOTAL_NODES: usize = 65_536;
pub const MAX_SINGLE_REPOSITORY_SNAPSHOT_REFERENCE_CANONICAL_BYTES: usize = 4_096;
pub const MAX_REPOSITORY_RELATIVE_PATH_BYTES: usize = 1_024;
pub const MAX_REPOSITORY_ID_BYTES: usize = 256;
pub const MAX_COMMIT_SHA_TEXT_BYTES: usize = 128;

/// Exact immutable repository blob identity supplied to Workspace-Execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepositorySnapshotRefV1 {
    pub project_id: String,
    pub repository_id: String,
    pub commit_sha: String,
    pub logical_relative_path: String,
}

/// Project-scoped immutable artifact identity supplied to Orchestration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArtifactRefV1 {
    pub project_id: String,
    pub artifact_id: String,
}

/// Closed typed parameter sets for the State query registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateQueryRefV1 {
    LogicalRole { role_id: String },
    ContextManifest { manifest_id: String },
    ContextEpoch { project_id: String, epoch: i64 },
    ExecutorBinding { binding_id: String },
    Event { event_id: String },
}

impl StateQueryRefV1 {
    pub const VERSION: u32 = 1;

    pub fn query_id(&self) -> &'static str {
        match self {
            Self::LogicalRole { .. } => "logical-role-by-id",
            Self::ContextManifest { .. } => "context-manifest-by-id",
            Self::ContextEpoch { .. } => "context-epoch-by-id",
            Self::ExecutorBinding { .. } => "executor-binding-by-id",
            Self::Event { .. } => "event-by-id",
        }
    }

    fn project_id<'a>(&'a self, attempt_project_id: &'a str) -> &'a str {
        match self {
            Self::ContextEpoch { project_id, .. } => project_id,
            _ => attempt_project_id,
        }
    }

    fn validate_parameters(&self, attempt_project_id: &str) -> Result<(), StateError> {
        ensure_bytes(
            "STATE_QUERY project_id",
            attempt_project_id,
            MAX_OTHER_EXTERNAL_REFERENCE_BYTES,
        )?;
        match self {
            Self::LogicalRole { role_id } => ensure_identifier("STATE_QUERY role_id", role_id),
            Self::ContextManifest { manifest_id } => {
                ensure_identifier("STATE_QUERY manifest_id", manifest_id)
            }
            Self::ContextEpoch { project_id, epoch } => {
                ensure_identifier("STATE_QUERY project_id", project_id)?;
                if project_id != attempt_project_id {
                    return validation_failure("STATE_QUERY project scope mismatch");
                }
                if *epoch < 0 {
                    return validation_failure("STATE_QUERY epoch must be non-negative");
                }
                Ok(())
            }
            Self::ExecutorBinding { binding_id } => {
                ensure_identifier("STATE_QUERY binding_id", binding_id)
            }
            Self::Event { event_id } => ensure_identifier("STATE_QUERY event_id", event_id),
        }
    }

    fn canonical_identity(&self, project_id: &str) -> Result<String, StateError> {
        #[derive(Serialize)]
        struct Identity<'a, T: Serialize> {
            project_id: &'a str,
            query_id: &'a str,
            query_version: u32,
            parameters: T,
        }
        #[derive(Serialize)]
        struct Id<'a> {
            id: &'a str,
        }
        #[derive(Serialize)]
        struct Epoch<'a> {
            epoch: i64,
            project_id: &'a str,
        }

        let bytes = match self {
            Self::LogicalRole { role_id } => serde_json_canonicalizer::to_vec(&Identity {
                project_id,
                query_id: self.query_id(),
                query_version: Self::VERSION,
                parameters: Id { id: role_id },
            }),
            Self::ContextManifest { manifest_id } => serde_json_canonicalizer::to_vec(&Identity {
                project_id,
                query_id: self.query_id(),
                query_version: Self::VERSION,
                parameters: Id { id: manifest_id },
            }),
            Self::ContextEpoch { project_id, epoch } => {
                serde_json_canonicalizer::to_vec(&Identity {
                    project_id,
                    query_id: self.query_id(),
                    query_version: Self::VERSION,
                    parameters: Epoch {
                        epoch: *epoch,
                        project_id,
                    },
                })
            }
            Self::ExecutorBinding { binding_id } => serde_json_canonicalizer::to_vec(&Identity {
                project_id,
                query_id: self.query_id(),
                query_version: Self::VERSION,
                parameters: Id { id: binding_id },
            }),
            Self::Event { event_id } => serde_json_canonicalizer::to_vec(&Identity {
                project_id,
                query_id: self.query_id(),
                query_version: Self::VERSION,
                parameters: Id { id: event_id },
            }),
        }
        .map_err(canonicalization_failure)?;
        String::from_utf8(bytes).map_err(|error| StateError::ContextRehydrationValidation {
            detail: format!("canonical STATE_QUERY identity was not UTF-8: {error}"),
        })
    }
}

/// A manifest source bound to the typed identity required to materialize it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoundContextSourceRefV1 {
    RepositorySnapshot(RepositorySnapshotRefV1),
    StateQuery(StateQueryRefV1),
    Artifact(ArtifactRefV1),
}

/// Trusted binding of one manifest ordinal to one stable evidence identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSourceBindingV1 {
    pub source_ordinal: usize,
    pub source_id: String,
    pub source_ref: BoundContextSourceRefV1,
}

/// Trusted task-touch relation supplied by Orchestration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSourceTouchEvidence {
    pub source_id: String,
    pub project_id: String,
    pub durable_role_id: String,
    pub context_manifest_id: String,
    pub context_epoch_id: i64,
    pub task_id: String,
    pub correlation_reference: String,
}

/// Typed on-demand authorization for a `REFERENCE` source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSourceDemand {
    pub source_id: String,
    pub project_id: String,
    pub durable_role_id: String,
    pub context_manifest_id: String,
    pub context_epoch_id: i64,
    pub task_id: Option<String>,
    pub correlation_reference: String,
}

/// Bounded metadata returned after an external port streams source bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalSourceMaterialization {
    pub materializer_id: String,
    pub provenance: String,
    pub materialized_at: String,
}

/// Bounded external materialization failure safe for durable evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMaterializationFailure {
    pub code: String,
    pub materializer_id: Option<String>,
    pub provenance: Option<String>,
    pub materialized_at: Option<String>,
    pub failure_detail: Option<String>,
}

/// Workspace-owned repository snapshot materializer port.
pub trait RepositorySnapshotMaterializer {
    fn known_length(
        &mut self,
        _reference: &RepositorySnapshotRefV1,
    ) -> Result<Option<u64>, SourceMaterializationFailure> {
        Ok(None)
    }

    fn materialize(
        &mut self,
        reference: &RepositorySnapshotRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure>;
}

/// Orchestration-owned immutable artifact materializer port.
pub trait ArtifactMaterializer {
    fn known_length(
        &mut self,
        _reference: &ArtifactRefV1,
    ) -> Result<Option<u64>, SourceMaterializationFailure> {
        Ok(None)
    }

    fn materialize(
        &mut self,
        reference: &ArtifactRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure>;
}

/// Orchestration-owned supplier for the successful attempt's event envelope.
pub trait ContextRehydratedEventSupplier {
    fn event_for(&mut self, attempt: &ContextRehydrationAttempt) -> EventEnvelope;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextRehydrationStatus {
    Succeeded,
    Failed,
}

impl ContextRehydrationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "SUCCEEDED",
            Self::Failed => "FAILED",
        }
    }

    fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "SUCCEEDED" => Ok(Self::Succeeded),
            "FAILED" => Ok(Self::Failed),
            other => Err(decode_failure(format!("unknown attempt status {other:?}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceDigestComparison {
    Matched,
    Changed,
    NotChecked,
    Failed,
}

impl SourceDigestComparison {
    fn as_str(self) -> &'static str {
        match self {
            Self::Matched => "MATCHED",
            Self::Changed => "CHANGED",
            Self::NotChecked => "NOT_CHECKED",
            Self::Failed => "FAILED",
        }
    }

    fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "MATCHED" => Ok(Self::Matched),
            "CHANGED" => Ok(Self::Changed),
            "NOT_CHECKED" => Ok(Self::NotChecked),
            "FAILED" => Ok(Self::Failed),
            other => Err(decode_failure(format!(
                "unknown digest comparison {other:?}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceDisposition {
    Reread,
    Unchanged,
    Deferred,
    Failed,
}

impl SourceDisposition {
    fn as_str(self) -> &'static str {
        match self {
            Self::Reread => "REREAD",
            Self::Unchanged => "UNCHANGED",
            Self::Deferred => "DEFERRED",
            Self::Failed => "FAILED",
        }
    }

    fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "REREAD" => Ok(Self::Reread),
            "UNCHANGED" => Ok(Self::Unchanged),
            "DEFERRED" => Ok(Self::Deferred),
            "FAILED" => Ok(Self::Failed),
            other => Err(decode_failure(format!(
                "unknown source disposition {other:?}"
            ))),
        }
    }
}

/// Immutable bounded evidence for one manifest source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextRehydrationSourceEvidence {
    pub source_id: String,
    pub source_ordinal: usize,
    pub ref_type: String,
    pub source_class: String,
    pub canonical_source_identity: String,
    pub materializer_id: Option<String>,
    pub provenance: Option<String>,
    pub expected_digest: String,
    pub observed_digest: Option<String>,
    pub comparison: SourceDigestComparison,
    pub touch_evidence: Option<String>,
    pub disposition: SourceDisposition,
    pub materialized_at: Option<String>,
    pub failure_code: Option<String>,
    pub failure_detail: Option<String>,
}

/// One immutable terminal rehydration receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRehydrationAttempt {
    pub rehydration_attempt_id: String,
    pub project_id: String,
    pub durable_role_id: String,
    pub context_manifest_id: String,
    pub context_epoch_id: i64,
    pub repository_snapshot_references: Vec<RepositorySnapshotRefV1>,
    pub requested_by_actor: EventActor,
    pub executor_binding_id: Option<String>,
    pub session_reference: Option<String>,
    pub task_id: Option<String>,
    pub correlation_reference: Option<String>,
    pub trigger_kind: ContextEpochTrigger,
    pub trigger_reference: Option<String>,
    pub started_at: String,
    pub completed_at: String,
    pub status: ContextRehydrationStatus,
    pub source_evidence: Vec<ContextRehydrationSourceEvidence>,
    pub failure_code: Option<String>,
}

/// Caller-supplied immutable attempt identity and materialization scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRehydrationRequest {
    pub rehydration_attempt_id: String,
    pub project_id: String,
    pub durable_role_id: String,
    pub context_manifest_id: String,
    pub context_epoch_id: i64,
    pub requested_by_actor: EventActor,
    pub executor_binding_id: Option<String>,
    pub session_reference: Option<String>,
    pub task_id: Option<String>,
    pub correlation_reference: Option<String>,
    pub trigger_kind: ContextEpochTrigger,
    pub trigger_reference: Option<String>,
    pub started_at: String,
    pub completed_at: String,
    pub source_bindings: Vec<ContextSourceBindingV1>,
    pub touch_evidence: Vec<ContextSourceTouchEvidence>,
    pub demands: Vec<ContextSourceDemand>,
}

/// Ephemeral genuinely reread source body; never persisted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RehydratedContextSource {
    pub source_id: String,
    pub bytes: Vec<u8>,
}

/// Terminal outcome. Failed outcomes never expose partial source bodies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextRehydrationOutcome {
    pub attempt: ContextRehydrationAttempt,
    pub sources: Vec<RehydratedContextSource>,
}

impl SqliteStateRepository {
    /// Rehydrates one exact immutable manifest/epoch scope.
    pub fn rehydrate_context(
        &mut self,
        request: ContextRehydrationRequest,
        repository_materializer: &mut impl RepositorySnapshotMaterializer,
        artifact_materializer: &mut impl ArtifactMaterializer,
        event_supplier: &mut impl ContextRehydratedEventSupplier,
    ) -> Result<ContextRehydrationOutcome, StateError> {
        let manifest = match self.validate_request_scope(&request) {
            Ok(manifest) => manifest,
            Err(error) => {
                return self.persist_request_failure(request, closed_request_error(&error));
            }
        };
        if let Err(error) = validate_boundary_a(&request, &manifest) {
            return self.persist_request_failure(request, closed_request_error(&error));
        }
        let bindings = match validate_bindings(&request, &manifest) {
            Ok(bindings) => bindings,
            Err(error) => {
                return self.persist_request_failure(request, closed_request_error(&error));
            }
        };
        if let Err(error) = validate_touch_and_demands(&request, &manifest, &bindings) {
            return self.persist_request_failure(request, closed_request_error(&error));
        }

        let mut evidence = Vec::with_capacity(manifest.sources.len());
        let mut sources = Vec::new();
        let mut failure_code = None;
        let mut total_materialized_bytes = 0_u64;

        for (ordinal, source) in manifest.sources.iter().enumerate() {
            let binding = bindings[ordinal];
            let expected_digest_valid = valid_source_digest(&source.digest);
            let demanded = request
                .demands
                .iter()
                .any(|item| item.source_id == binding.source_id);
            let touched = request
                .touch_evidence
                .iter()
                .find(|item| item.source_id == binding.source_id);

            if source.source_class == SourceClass::Reference && !demanded {
                evidence.push(deferred_evidence(
                    binding,
                    source,
                    ordinal,
                    &request.project_id,
                )?);
                continue;
            }
            if !expected_digest_valid {
                let code = "INVALID_EXPECTED_DIGEST".to_string();
                evidence.push(failed_evidence(
                    binding,
                    source,
                    ordinal,
                    &code,
                    None,
                    &request.project_id,
                )?);
                failure_code = Some(code);
                break;
            }

            let identity = canonical_source_identity(&binding.source_ref, &request.project_id)?;
            let pre_reread = source.source_class != SourceClass::Consumed || touched.is_some();
            let materialization = match &binding.source_ref {
                BoundContextSourceRefV1::RepositorySnapshot(reference) => {
                    let known = repository_materializer.known_length(reference);
                    known.and_then(|known| {
                        stream_external_source(
                            known,
                            source.r#ref.ref_type,
                            identity.as_bytes(),
                            pre_reread,
                            &mut total_materialized_bytes,
                            |sink| repository_materializer.materialize(reference, sink),
                        )
                    })
                }
                BoundContextSourceRefV1::StateQuery(query) => self
                    .execute_registered_state_query(query, &request)
                    .and_then(|bytes| {
                        account_materialized_bytes(
                            u64::try_from(bytes.len()).map_err(|_| {
                                StateError::ContextRehydrationValidation {
                                    detail: "REHYDRATION_SIZE_COUNTER_OVERFLOW".to_string(),
                                }
                            })?,
                            &mut total_materialized_bytes,
                        )?;
                        Ok((
                            ExternalSourceMaterialization {
                                materializer_id: "state-context:state-query-registry:v1"
                                    .to_string(),
                                provenance: query.query_id().to_string(),
                                materialized_at: request.completed_at.clone(),
                            },
                            context_source_digest(
                                source.r#ref.ref_type,
                                identity.as_bytes(),
                                &bytes,
                            ),
                            Some(bytes),
                        ))
                    })
                    .map_err(|error| SourceMaterializationFailure {
                        code: closed_error_code(&error),
                        materializer_id: Some("state-context:state-query-registry:v1".to_string()),
                        provenance: Some(query.query_id().to_string()),
                        materialized_at: Some(request.completed_at.clone()),
                        failure_detail: Some(error.to_string()),
                    }),
                BoundContextSourceRefV1::Artifact(reference) => {
                    let known = artifact_materializer.known_length(reference);
                    known.and_then(|known| {
                        stream_external_source(
                            known,
                            source.r#ref.ref_type,
                            identity.as_bytes(),
                            pre_reread,
                            &mut total_materialized_bytes,
                            |sink| artifact_materializer.materialize(reference, sink),
                        )
                    })
                }
            };

            let (materialization, observed_digest, mut reread_bytes) = match materialization {
                Ok(value) => value,
                Err(mut error) => {
                    if validate_failure(&error).is_err() {
                        error = SourceMaterializationFailure {
                            code: "INVALID_MATERIALIZER_FAILURE".to_string(),
                            materializer_id: None,
                            provenance: None,
                            materialized_at: None,
                            failure_detail: None,
                        };
                    }
                    evidence.push(failed_evidence(
                        binding,
                        source,
                        ordinal,
                        &error.code,
                        Some(&error),
                        &request.project_id,
                    )?);
                    failure_code = Some(error.code);
                    break;
                }
            };
            if validate_materialization(&materialization).is_err() {
                let code = "INVALID_MATERIALIZER_RESPONSE".to_string();
                evidence.push(failed_evidence(
                    binding,
                    source,
                    ordinal,
                    &code,
                    None,
                    &request.project_id,
                )?);
                failure_code = Some(code);
                break;
            }
            let changed = observed_digest != source.digest;
            let reread = match source.source_class {
                SourceClass::Mandatory | SourceClass::Reference => true,
                SourceClass::Consumed => touched.is_some() || changed,
            };
            if reread && reread_bytes.is_none() {
                reread_bytes = Some(reread_source_body(
                    &binding.source_ref,
                    repository_materializer,
                    artifact_materializer,
                    self,
                    &request,
                )?);
            }
            evidence.push(ContextRehydrationSourceEvidence {
                source_id: binding.source_id.clone(),
                source_ordinal: ordinal,
                ref_type: source.r#ref.ref_type.as_str().to_string(),
                source_class: source.source_class.as_str().to_string(),
                canonical_source_identity: identity,
                materializer_id: Some(materialization.materializer_id.clone()),
                provenance: Some(materialization.provenance.clone()),
                expected_digest: source.digest.clone(),
                observed_digest: Some(observed_digest),
                comparison: if changed {
                    SourceDigestComparison::Changed
                } else {
                    SourceDigestComparison::Matched
                },
                touch_evidence: touched.map(canonical_touch_evidence).transpose()?,
                disposition: if reread {
                    SourceDisposition::Reread
                } else {
                    SourceDisposition::Unchanged
                },
                materialized_at: Some(materialization.materialized_at),
                failure_code: None,
                failure_detail: None,
            });
            if reread {
                sources.push(RehydratedContextSource {
                    source_id: binding.source_id.clone(),
                    bytes: reread_bytes.expect("reread source body is present"),
                });
            }
        }

        for (ordinal, binding) in bindings.iter().enumerate().skip(evidence.len()) {
            evidence.push(deferred_evidence(
                binding,
                &manifest.sources[ordinal],
                ordinal,
                &request.project_id,
            )?);
        }

        let status = if failure_code.is_some() {
            ContextRehydrationStatus::Failed
        } else {
            ContextRehydrationStatus::Succeeded
        };
        let attempt = attempt_from_request(&request, status, evidence, failure_code);
        validate_attempt(&attempt)?;
        validate_boundary_b(&attempt)?;

        if status == ContextRehydrationStatus::Failed {
            self.run_transaction(|uow| uow.insert_context_rehydration_attempt(&attempt))?;
            return Ok(ContextRehydrationOutcome {
                attempt,
                sources: Vec::new(),
            });
        }

        let success_event = event_supplier.event_for(&attempt);
        if validate_success_event(&attempt, &success_event).is_err() {
            let failed = ContextRehydrationAttempt {
                status: ContextRehydrationStatus::Failed,
                failure_code: Some("CONTEXT_REHYDRATED_EVENT_INVALID".to_string()),
                ..attempt
            };
            self.run_transaction(|uow| uow.insert_context_rehydration_attempt(&failed))?;
            return Ok(ContextRehydrationOutcome {
                attempt: failed,
                sources: Vec::new(),
            });
        }
        if let Err(success_error) = self.run_transaction(|uow| {
            let latest = uow
                .read_latest_context_epoch(&attempt.project_id)?
                .ok_or_else(|| StateError::ContextRehydrationValidation {
                    detail: "terminal latest context epoch does not exist".to_string(),
                })?;
            if latest.epoch != attempt.context_epoch_id {
                return validation_failure("STALE_CONTEXT_EPOCH");
            }
            uow.insert_context_rehydration_attempt(&attempt)?;
            uow.insert_event(&success_event)
        }) {
            let failed = ContextRehydrationAttempt {
                status: ContextRehydrationStatus::Failed,
                failure_code: Some(
                    if matches!(&success_error, StateError::ContextRehydrationValidation { detail } if detail == "STALE_CONTEXT_EPOCH") {
                        "STALE_CONTEXT_EPOCH"
                    } else {
                        "CONTEXT_REHYDRATED_TRANSACTION_FAILED"
                    }
                    .to_string(),
                ),
                ..attempt
            };
            if self
                .run_transaction(|uow| uow.insert_context_rehydration_attempt(&failed))
                .is_ok()
            {
                return Ok(ContextRehydrationOutcome {
                    attempt: failed,
                    sources: Vec::new(),
                });
            }
            return Err(success_error);
        }
        Ok(ContextRehydrationOutcome { attempt, sources })
    }

    /// Reads one immutable terminal attempt and its ordered evidence.
    pub fn find_context_rehydration_attempt(
        &self,
        project_id: &str,
        attempt_id: &str,
    ) -> Result<Option<ContextRehydrationAttempt>, StateError> {
        read_attempt(self.connection(), project_id, attempt_id)
    }

    fn persist_request_failure(
        &mut self,
        request: ContextRehydrationRequest,
        failure_code: String,
    ) -> Result<ContextRehydrationOutcome, StateError> {
        let mut failed = attempt_from_request(
            &request,
            ContextRehydrationStatus::Failed,
            Vec::new(),
            Some(failure_code),
        );
        failed.repository_snapshot_references.clear();
        validate_attempt(&failed)?;
        validate_boundary_b(&failed)?;
        self.run_transaction(|uow| uow.insert_context_rehydration_attempt(&failed))?;
        Ok(ContextRehydrationOutcome {
            attempt: failed,
            sources: Vec::new(),
        })
    }

    fn validate_request_scope(
        &self,
        request: &ContextRehydrationRequest,
    ) -> Result<ContextManifest, StateError> {
        ensure_identifier("rehydration_attempt_id", &request.rehydration_attempt_id)?;
        ensure_identifier("project_id", &request.project_id)?;
        ensure_identifier("durable_role_id", &request.durable_role_id)?;
        ensure_identifier("context_manifest_id", &request.context_manifest_id)?;
        ensure_non_empty("started_at", &request.started_at)?;
        ensure_non_empty("completed_at", &request.completed_at)?;
        if request.context_epoch_id < 0 {
            return validation_failure("context_epoch_id must be non-negative");
        }
        validate_actor(&request.requested_by_actor)?;
        validate_optional("executor_binding_id", &request.executor_binding_id)?;
        validate_optional("session_reference", &request.session_reference)?;
        validate_optional("task_id", &request.task_id)?;
        validate_optional("correlation_reference", &request.correlation_reference)?;
        validate_optional("trigger_reference", &request.trigger_reference)?;

        let role = self
            .find_logical_role(&request.durable_role_id)?
            .ok_or_else(|| StateError::ContextRehydrationValidation {
                detail: "durable role does not exist".to_string(),
            })?;
        if role.project_id != request.project_id {
            return validation_failure("durable role project mismatch");
        }
        if let Some(binding_id) = &request.executor_binding_id {
            let binding = self.find_executor_binding(binding_id)?.ok_or_else(|| {
                StateError::ContextRehydrationValidation {
                    detail: "executor binding does not exist".to_string(),
                }
            })?;
            if binding.role_id != request.durable_role_id {
                return validation_failure("executor binding role mismatch");
            }
            if let (Some(persisted), Some(requested)) = (
                binding.session_ref.as_deref(),
                request.session_reference.as_deref(),
            ) && persisted.as_bytes() != requested.as_bytes()
            {
                return validation_failure("executor binding session mismatch");
            }
        }
        let manifest = self
            .find_context_manifest(&request.context_manifest_id)?
            .ok_or_else(|| StateError::ContextRehydrationValidation {
                detail: "context manifest does not exist".to_string(),
            })?;
        if manifest.project_id != request.project_id || manifest.role_id != request.durable_role_id
        {
            return validation_failure("context manifest scope mismatch");
        }
        let epoch = self
            .find_context_epoch(&request.project_id, request.context_epoch_id)?
            .ok_or_else(|| StateError::ContextRehydrationValidation {
                detail: "context epoch does not exist".to_string(),
            })?;
        let latest = self
            .find_latest_context_epoch(&request.project_id)?
            .ok_or_else(|| StateError::ContextRehydrationValidation {
                detail: "latest context epoch does not exist".to_string(),
            })?;
        if epoch != latest {
            return validation_failure("attempt does not target the latest context epoch");
        }
        Ok(manifest)
    }

    fn execute_registered_state_query(
        &self,
        query: &StateQueryRefV1,
        request: &ContextRehydrationRequest,
    ) -> Result<Vec<u8>, StateError> {
        #[derive(Serialize)]
        struct ResultV1 {
            found: bool,
            query_id: &'static str,
            query_version: u32,
            values: Vec<Field>,
        }
        #[derive(Serialize)]
        struct Field {
            name: &'static str,
            value: Value,
        }
        #[derive(Serialize)]
        #[serde(tag = "type", content = "value")]
        enum Value {
            String(String),
            Integer(i64),
            Boolean(bool),
            Null,
            Strings(Vec<String>),
        }
        fn string(name: &'static str, value: impl Into<String>) -> Field {
            Field {
                name,
                value: Value::String(value.into()),
            }
        }
        fn optional(name: &'static str, value: Option<String>) -> Field {
            Field {
                name,
                value: value.map(Value::String).unwrap_or(Value::Null),
            }
        }

        query.validate_parameters(&request.project_id)?;
        if query.project_id(&request.project_id) != request.project_id {
            return validation_failure("STATE_QUERY project scope mismatch");
        }
        let values = match query {
            StateQueryRefV1::LogicalRole { role_id } => {
                let value = self.find_logical_role(role_id)?;
                if value
                    .as_ref()
                    .is_some_and(|role| role.project_id != request.project_id)
                {
                    return validation_failure("STATE_QUERY LogicalRole project mismatch");
                }
                value.map(|value| {
                    vec![
                        string("role_id", value.role_id),
                        string("project_id", value.project_id),
                        string("role_type", value.role_type.as_str()),
                        string("status", value.status.as_str()),
                        Field {
                            name: "current_context_epoch",
                            value: Value::Integer(value.current_context_epoch),
                        },
                        optional("name", value.name),
                        optional("workstream_id", value.workstream_id),
                        Field {
                            name: "ownership_paths",
                            value: Value::Strings(value.ownership_paths),
                        },
                        optional("integration_branch", value.integration_branch),
                        optional("context_manifest_id", value.context_manifest_id),
                        optional("active_binding_id", value.active_binding_id),
                        optional("created_at", value.created_at),
                    ]
                })
            }
            StateQueryRefV1::ContextManifest { manifest_id } => {
                let value = self.find_context_manifest(manifest_id)?;
                if value
                    .as_ref()
                    .is_some_and(|manifest| manifest.project_id != request.project_id)
                {
                    return validation_failure("STATE_QUERY ContextManifest project mismatch");
                }
                value.map(|value| {
                    vec![
                        string("manifest_id", value.manifest_id),
                        string("role_id", value.role_id),
                        string("project_id", value.project_id),
                        Field {
                            name: "epoch",
                            value: Value::Integer(value.epoch),
                        },
                        string("created_at", value.created_at),
                        optional("last_rehydrated_at", value.last_rehydrated_at),
                    ]
                })
            }
            StateQueryRefV1::ContextEpoch { project_id, epoch } => {
                self.find_context_epoch(project_id, *epoch)?.map(|value| {
                    vec![
                        string("project_id", value.project_id),
                        Field {
                            name: "epoch",
                            value: Value::Integer(value.epoch),
                        },
                        string("advanced_at", value.advanced_at),
                        string("trigger", value.trigger.as_str()),
                    ]
                })
            }
            StateQueryRefV1::ExecutorBinding { binding_id } => {
                let value = self.find_executor_binding(binding_id)?;
                if let Some(binding) = &value {
                    let role = self.find_logical_role(&binding.role_id)?.ok_or_else(|| {
                        StateError::ContextRehydrationValidation {
                            detail: "STATE_QUERY ExecutorBinding role does not exist".to_string(),
                        }
                    })?;
                    if role.project_id != request.project_id {
                        return validation_failure("STATE_QUERY ExecutorBinding project mismatch");
                    }
                }
                value.map(|value| {
                    vec![
                        string("binding_id", value.binding_id),
                        string("role_id", value.role_id),
                        string("provider_id", value.provider_id),
                        string("model_id", value.model_id),
                        string("runtime_id", value.runtime_id),
                        optional("session_ref", value.session_ref),
                        optional("routing_decision_id", value.routing_decision_id),
                        string("bound_at", value.bound_at),
                        string("lease_expires_at", value.lease_expires_at),
                        optional("released_at", value.released_at),
                        optional(
                            "release_reason",
                            value.release_reason.map(|item| item.as_str().to_string()),
                        ),
                        Field {
                            name: "rehydration_completed",
                            value: value
                                .rehydration_completed
                                .map(Value::Boolean)
                                .unwrap_or(Value::Null),
                        },
                    ]
                })
            }
            StateQueryRefV1::Event { event_id } => {
                let value = self.find_event(event_id)?;
                if value
                    .as_ref()
                    .is_some_and(|event| event.project_id != request.project_id)
                {
                    return validation_failure("STATE_QUERY Event project mismatch");
                }
                value.map(|value| {
                    vec![
                        string("event_id", value.event_id),
                        string("project_id", value.project_id),
                        optional("goal_id", value.goal_id),
                        string("event_type", value.event_type.as_str()),
                        string("actor_kind", value.actor.kind.as_str()),
                        optional("actor_id", value.actor.id),
                        string("subject_kind", value.subject.kind.as_str()),
                        string("subject_id", value.subject.id),
                        string("occurred_at", value.occurred_at),
                        string("payload_reference", value.payload.reference),
                        string("payload_digest", value.payload.digest),
                        string("correlation_id", value.correlation_id),
                        Field {
                            name: "epoch",
                            value: Value::Integer(value.epoch),
                        },
                    ]
                })
            }
        };
        if values.as_ref().is_some_and(|fields| {
            fields.len() > MAX_STATE_QUERY_OBJECT_MEMBERS
                || fields.iter().any(
                    |field| matches!(&field.value, Value::Strings(items) if items.len() > MAX_STATE_QUERY_ARRAY_ELEMENTS),
                )
        }) {
            return validation_failure("STATE_QUERY_RESULT_STRUCTURE_LIMIT_EXCEEDED");
        }
        let result = serde_json_canonicalizer::to_vec(&ResultV1 {
            found: values.is_some(),
            query_id: query.query_id(),
            query_version: StateQueryRefV1::VERSION,
            values: values.unwrap_or_default(),
        })
        .map_err(canonicalization_failure)?;
        if result.len() > MAX_STATE_QUERY_RESULT_CANONICAL_BYTES {
            return validation_failure("STATE_QUERY_RESULT_SIZE_LIMIT_EXCEEDED");
        }
        Ok(result)
    }
}

impl UnitOfWork<'_> {
    fn insert_context_rehydration_attempt(
        &self,
        attempt: &ContextRehydrationAttempt,
    ) -> Result<(), StateError> {
        validate_attempt(attempt)?;
        validate_boundary_b(attempt)?;
        insert_attempt(self.tx(), attempt)
    }
}

/// Builds the only authorized `CONTEXT_REHYDRATED` payload reference.
pub fn context_rehydration_event_payload(
    attempt: &ContextRehydrationAttempt,
) -> Result<EventPayloadReference, StateError> {
    if attempt.status != ContextRehydrationStatus::Succeeded {
        return validation_failure("only a SUCCEEDED attempt can be event-referenced");
    }
    #[derive(Serialize)]
    struct Identity<'a> {
        attempt_id: &'a str,
        entity: &'static str,
        project_id: &'a str,
        v: u8,
    }
    let reference = serde_json_canonicalizer::to_string(&Identity {
        attempt_id: &attempt.rehydration_attempt_id,
        entity: "ContextRehydrationAttempt",
        project_id: &attempt.project_id,
        v: 1,
    })
    .map_err(canonicalization_failure)?;
    let projection = attempt_digest_projection(attempt)?;
    let mut hasher = Sha256::new();
    hasher.update(ATTEMPT_DIGEST_DOMAIN);
    hasher.update([0]);
    hasher.update(projection);
    Ok(EventPayloadReference {
        reference,
        digest: format!(
            "sha256:context-rehydration-attempt:v1:{}",
            lowercase_hex(hasher.finalize().as_slice())
        ),
    })
}

/// Computes the approved domain-separated digest for canonical source bytes.
pub fn context_source_digest_v1(
    source_ref: &BoundContextSourceRefV1,
    project_id: &str,
    canonical_content: &[u8],
) -> Result<String, StateError> {
    if canonical_content.len() > MAX_MATERIALIZED_BYTES_PER_SOURCE as usize {
        return validation_failure("SOURCE_MATERIALIZATION_SIZE_LIMIT_EXCEEDED");
    }
    ensure_identifier("project_id", project_id)?;
    match source_ref {
        BoundContextSourceRefV1::RepositorySnapshot(reference) => {
            if reference.project_id != project_id {
                return validation_failure("REPO_PATH digest project mismatch");
            }
            ensure_bytes(
                "repository_id",
                &reference.repository_id,
                MAX_REPOSITORY_ID_BYTES,
            )?;
            if reference.commit_sha.len() != 40 || !reference.commit_sha.bytes().all(is_lower_hex) {
                return validation_failure("commit_sha must be 40 lowercase hexadecimal bytes");
            }
            validate_relative_path(&reference.logical_relative_path)?;
        }
        BoundContextSourceRefV1::StateQuery(query) => query.validate_parameters(project_id)?,
        BoundContextSourceRefV1::Artifact(reference) => {
            if reference.project_id != project_id {
                return validation_failure("ARTIFACT_ID digest project mismatch");
            }
            ensure_identifier("artifact_id", &reference.artifact_id)?;
        }
    }
    let ref_type = match source_ref {
        BoundContextSourceRefV1::RepositorySnapshot(_) => ContextSourceRefType::RepoPath,
        BoundContextSourceRefV1::StateQuery(_) => ContextSourceRefType::StateQuery,
        BoundContextSourceRefV1::Artifact(_) => ContextSourceRefType::ArtifactId,
    };
    let identity = canonical_source_identity(source_ref, project_id)?;
    ensure_bytes(
        "canonical_source_identity",
        &identity,
        MAX_SOURCE_IDENTITY_CANONICAL_BYTES,
    )?;
    Ok(context_source_digest(
        ref_type,
        identity.as_bytes(),
        canonical_content,
    ))
}

fn validate_success_event(
    attempt: &ContextRehydrationAttempt,
    event: &EventEnvelope,
) -> Result<(), StateError> {
    crate::event::validate_for_append(event)?;
    if event.event_type != EventType::ContextRehydrated
        || event.project_id != attempt.project_id
        || event.subject.kind != SubjectKind::Role
        || event.subject.id != attempt.durable_role_id
        || event.epoch != attempt.context_epoch_id
        || event.actor != attempt.requested_by_actor
        || event.occurred_at != attempt.completed_at
    {
        return validation_failure("CONTEXT_REHYDRATED event scope/subject mismatch");
    }
    if let Some(correlation) = &attempt.correlation_reference
        && event.correlation_id != *correlation
    {
        return validation_failure("CONTEXT_REHYDRATED correlation mismatch");
    }
    let expected = context_rehydration_event_payload(attempt)?;
    if !valid_attempt_digest(&event.payload.digest) || event.payload != expected {
        return validation_failure("CONTEXT_REHYDRATED payload reference/digest mismatch");
    }
    // Parsing/canonicalization is performed by the approved RFC 8785 crate;
    // equality above also rejects extra keys and identity substitutions.
    if serde_json_canonicalizer::pipe(&event.payload.reference).map_err(canonicalization_failure)?
        != event.payload.reference
    {
        return validation_failure("CONTEXT_REHYDRATED reference is not canonical JSON");
    }
    Ok(())
}

fn attempt_digest_projection(attempt: &ContextRehydrationAttempt) -> Result<Vec<u8>, StateError> {
    #[derive(Serialize)]
    struct Actor<'a> {
        id: &'a Option<String>,
        kind: &'static str,
    }
    #[derive(Serialize)]
    struct Projection<'a> {
        completed_at: &'a str,
        context_epoch_id: i64,
        context_manifest_id: &'a str,
        correlation_reference: &'a Option<String>,
        durable_role_id: &'a str,
        entity: &'static str,
        executor_binding_id: &'a Option<String>,
        project_id: &'a str,
        rehydration_attempt_id: &'a str,
        repository_snapshot_references: &'a [RepositorySnapshotRefV1],
        requested_by_actor: Actor<'a>,
        session_reference: &'a Option<String>,
        source_evidence: &'a [ContextRehydrationSourceEvidence],
        started_at: &'a str,
        status: &'static str,
        task_id: &'a Option<String>,
        trigger_kind: &'static str,
        trigger_reference: &'a Option<String>,
        v: u8,
    }
    let mut sources = attempt.source_evidence.clone();
    sources.sort_by(|left, right| left.source_id.cmp(&right.source_id));
    if sources
        .windows(2)
        .any(|pair| pair[0].source_id == pair[1].source_id)
    {
        return validation_failure("duplicate source_id evidence is ambiguous");
    }
    let mut snapshots = attempt.repository_snapshot_references.clone();
    snapshots.sort_by(|left, right| {
        (
            &left.repository_id,
            &left.commit_sha,
            &left.logical_relative_path,
        )
            .cmp(&(
                &right.repository_id,
                &right.commit_sha,
                &right.logical_relative_path,
            ))
    });
    serde_json_canonicalizer::to_vec(&Projection {
        completed_at: &attempt.completed_at,
        context_epoch_id: attempt.context_epoch_id,
        context_manifest_id: &attempt.context_manifest_id,
        correlation_reference: &attempt.correlation_reference,
        durable_role_id: &attempt.durable_role_id,
        entity: "ContextRehydrationAttempt",
        executor_binding_id: &attempt.executor_binding_id,
        project_id: &attempt.project_id,
        rehydration_attempt_id: &attempt.rehydration_attempt_id,
        repository_snapshot_references: &snapshots,
        requested_by_actor: Actor {
            id: &attempt.requested_by_actor.id,
            kind: attempt.requested_by_actor.kind.as_str(),
        },
        session_reference: &attempt.session_reference,
        source_evidence: &sources,
        started_at: &attempt.started_at,
        status: attempt.status.as_str(),
        task_id: &attempt.task_id,
        trigger_kind: attempt.trigger_kind.as_str(),
        trigger_reference: &attempt.trigger_reference,
        v: 1,
    })
    .map_err(canonicalization_failure)
}

fn stream_external_source(
    known_length: Option<u64>,
    ref_type: ContextSourceRefType,
    identity: &[u8],
    capture_body: bool,
    attempt_total: &mut u64,
    mut materialize: impl FnMut(
        &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    )
        -> Result<ExternalSourceMaterialization, SourceMaterializationFailure>,
) -> Result<(ExternalSourceMaterialization, String, Option<Vec<u8>>), SourceMaterializationFailure>
{
    if known_length.is_some_and(|length| length > MAX_MATERIALIZED_BYTES_PER_SOURCE) {
        return Err(limit_failure("SOURCE_MATERIALIZATION_SIZE_LIMIT_EXCEEDED"));
    }
    if let Some(length) = known_length {
        ensure_total_will_fit(*attempt_total, length)?;
    }

    let length = match known_length {
        Some(length) => length,
        None => {
            let mut observed = 0_u64;
            let mut count = |chunk: &[u8]| {
                observed = checked_source_count(observed, chunk.len())?;
                Ok(())
            };
            materialize(&mut count)?;
            ensure_total_will_fit(*attempt_total, observed)?;
            observed
        }
    };

    let mut hasher = source_digest_hasher(ref_type, identity, length)?;
    let mut observed = 0_u64;
    let mut body = capture_body.then(Vec::new);
    let mut accept = |chunk: &[u8]| {
        observed = checked_source_count(observed, chunk.len())?;
        if observed > length {
            return Err(limit_failure("INVALID_MATERIALIZER_RESPONSE"));
        }
        hasher.update(chunk);
        if let Some(bytes) = &mut body {
            bytes.extend_from_slice(chunk);
        }
        Ok(())
    };
    let metadata = materialize(&mut accept)?;
    if observed != length {
        return Err(limit_failure("INVALID_MATERIALIZER_RESPONSE"));
    }
    *attempt_total = attempt_total
        .checked_add(length)
        .ok_or_else(|| limit_failure("REHYDRATION_SIZE_COUNTER_OVERFLOW"))?;
    Ok((
        metadata,
        format!("sha256:v1:{}", lowercase_hex(hasher.finalize().as_slice())),
        body,
    ))
}

fn reread_source_body(
    source: &BoundContextSourceRefV1,
    repository_materializer: &mut impl RepositorySnapshotMaterializer,
    artifact_materializer: &mut impl ArtifactMaterializer,
    repository: &SqliteStateRepository,
    request: &ContextRehydrationRequest,
) -> Result<Vec<u8>, StateError> {
    if let BoundContextSourceRefV1::StateQuery(query) = source {
        return repository.execute_registered_state_query(query, request);
    }
    let mut body = Vec::new();
    let mut observed = 0_u64;
    let mut accept = |chunk: &[u8]| {
        observed = checked_source_count(observed, chunk.len())?;
        body.extend_from_slice(chunk);
        Ok(())
    };
    let result = match source {
        BoundContextSourceRefV1::RepositorySnapshot(reference) => {
            repository_materializer.materialize(reference, &mut accept)
        }
        BoundContextSourceRefV1::Artifact(reference) => {
            artifact_materializer.materialize(reference, &mut accept)
        }
        BoundContextSourceRefV1::StateQuery(_) => unreachable!(),
    };
    result.map_err(|error| StateError::ContextRehydrationValidation { detail: error.code })?;
    Ok(body)
}

fn checked_source_count(
    current: u64,
    chunk_len: usize,
) -> Result<u64, SourceMaterializationFailure> {
    if chunk_len > MAX_STREAM_CHUNK_BYTES {
        return Err(limit_failure("SOURCE_STREAM_CHUNK_SIZE_LIMIT_EXCEEDED"));
    }
    let chunk_len =
        u64::try_from(chunk_len).map_err(|_| limit_failure("REHYDRATION_SIZE_COUNTER_OVERFLOW"))?;
    let next = current
        .checked_add(chunk_len)
        .ok_or_else(|| limit_failure("REHYDRATION_SIZE_COUNTER_OVERFLOW"))?;
    if next > MAX_MATERIALIZED_BYTES_PER_SOURCE {
        return Err(limit_failure("SOURCE_MATERIALIZATION_SIZE_LIMIT_EXCEEDED"));
    }
    Ok(next)
}

fn ensure_total_will_fit(
    current: u64,
    additional: u64,
) -> Result<(), SourceMaterializationFailure> {
    let total = current
        .checked_add(additional)
        .ok_or_else(|| limit_failure("REHYDRATION_SIZE_COUNTER_OVERFLOW"))?;
    if total > MAX_MATERIALIZED_BYTES_PER_ATTEMPT {
        return Err(limit_failure(
            "REHYDRATION_MATERIALIZATION_TOTAL_LIMIT_EXCEEDED",
        ));
    }
    Ok(())
}

fn account_materialized_bytes(length: u64, total: &mut u64) -> Result<(), StateError> {
    if length > MAX_MATERIALIZED_BYTES_PER_SOURCE {
        return validation_failure("SOURCE_MATERIALIZATION_SIZE_LIMIT_EXCEEDED");
    }
    let next =
        total
            .checked_add(length)
            .ok_or_else(|| StateError::ContextRehydrationValidation {
                detail: "REHYDRATION_SIZE_COUNTER_OVERFLOW".to_string(),
            })?;
    if next > MAX_MATERIALIZED_BYTES_PER_ATTEMPT {
        return validation_failure("REHYDRATION_MATERIALIZATION_TOTAL_LIMIT_EXCEEDED");
    }
    *total = next;
    Ok(())
}

fn source_digest_hasher(
    ref_type: ContextSourceRefType,
    identity: &[u8],
    content_length: u64,
) -> Result<Sha256, SourceMaterializationFailure> {
    let identity_length = u64::try_from(identity.len())
        .map_err(|_| limit_failure("REHYDRATION_SIZE_COUNTER_OVERFLOW"))?;
    let mut hasher = Sha256::new();
    hasher.update(SOURCE_DIGEST_DOMAIN);
    hasher.update(ref_type.as_str().as_bytes());
    hasher.update(identity_length.to_be_bytes());
    hasher.update(identity);
    hasher.update(content_length.to_be_bytes());
    Ok(hasher)
}

fn limit_failure(code: &str) -> SourceMaterializationFailure {
    SourceMaterializationFailure {
        code: code.to_string(),
        materializer_id: None,
        provenance: None,
        materialized_at: None,
        failure_detail: None,
    }
}

fn closed_error_code(error: &StateError) -> String {
    match error {
        StateError::ContextRehydrationValidation { detail }
            if detail
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte == b'_') =>
        {
            detail.clone()
        }
        _ => "STATE_QUERY_FAILED".to_string(),
    }
}

fn context_source_digest(
    ref_type: ContextSourceRefType,
    identity: &[u8],
    content: &[u8],
) -> String {
    let mut hasher = source_digest_hasher(
        ref_type,
        identity,
        u64::try_from(content.len()).expect("slice length fits u64"),
    )
    .expect("in-memory identity length fits u64");
    hasher.update(content);
    format!("sha256:v1:{}", lowercase_hex(hasher.finalize().as_slice()))
}

fn lowercase_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;

    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

fn canonical_source_identity(
    source_ref: &BoundContextSourceRefV1,
    project_id: &str,
) -> Result<String, StateError> {
    match source_ref {
        BoundContextSourceRefV1::RepositorySnapshot(value) => {
            serde_json_canonicalizer::to_string(value).map_err(canonicalization_failure)
        }
        BoundContextSourceRefV1::StateQuery(value) => value.canonical_identity(project_id),
        BoundContextSourceRefV1::Artifact(value) => {
            serde_json_canonicalizer::to_string(value).map_err(canonicalization_failure)
        }
    }
}

fn validate_bindings<'a>(
    request: &'a ContextRehydrationRequest,
    manifest: &ContextManifest,
) -> Result<Vec<&'a ContextSourceBindingV1>, StateError> {
    if request.source_bindings.len() != manifest.sources.len() {
        return validation_failure("source bindings must cover every manifest source exactly once");
    }
    let mut by_ordinal = vec![None; manifest.sources.len()];
    let mut source_ids = HashSet::new();
    for binding in &request.source_bindings {
        ensure_identifier("source_id", &binding.source_id)?;
        if !source_ids.insert(binding.source_id.as_str()) {
            return validation_failure("duplicate source_id binding");
        }
        let slot = by_ordinal.get_mut(binding.source_ordinal).ok_or_else(|| {
            StateError::ContextRehydrationValidation {
                detail: "source binding ordinal is out of range".to_string(),
            }
        })?;
        if slot.replace(binding).is_some() {
            return validation_failure("duplicate source binding ordinal");
        }
        validate_source_binding(request, &manifest.sources[binding.source_ordinal], binding)?;
    }
    by_ordinal
        .into_iter()
        .map(|value| {
            value.ok_or_else(|| StateError::ContextRehydrationValidation {
                detail: "source binding ordinal gap".to_string(),
            })
        })
        .collect()
}

fn validate_source_binding(
    request: &ContextRehydrationRequest,
    source: &crate::context_manifest::ContextManifestSource,
    binding: &ContextSourceBindingV1,
) -> Result<(), StateError> {
    match (&source.r#ref.ref_type, &binding.source_ref) {
        (ContextSourceRefType::RepoPath, BoundContextSourceRefV1::RepositorySnapshot(value)) => {
            if value.project_id != request.project_id
                || value.logical_relative_path != source.r#ref.target
            {
                return validation_failure("REPO_PATH source binding mismatch");
            }
            ensure_identifier("repository_id", &value.repository_id)?;
            if value.commit_sha.len() != 40
                || !value
                    .commit_sha
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return validation_failure("commit_sha must be 40 lowercase hexadecimal bytes");
            }
            validate_relative_path(&value.logical_relative_path)?;
        }
        (ContextSourceRefType::StateQuery, BoundContextSourceRefV1::StateQuery(value)) => {
            if value.query_id() != source.r#ref.target
                || value.project_id(&request.project_id) != request.project_id
            {
                return validation_failure("STATE_QUERY source binding mismatch");
            }
        }
        (ContextSourceRefType::ArtifactId, BoundContextSourceRefV1::Artifact(value)) => {
            if value.project_id != request.project_id || value.artifact_id != source.r#ref.target {
                return validation_failure("ARTIFACT_ID source binding mismatch");
            }
            ensure_identifier("artifact_id", &value.artifact_id)?;
        }
        _ => return validation_failure("source binding type mismatch"),
    }
    Ok(())
}

fn validate_touch_and_demands(
    request: &ContextRehydrationRequest,
    manifest: &ContextManifest,
    bindings: &[&ContextSourceBindingV1],
) -> Result<(), StateError> {
    let source_ids: HashSet<&str> = bindings
        .iter()
        .map(|item| item.source_id.as_str())
        .collect();
    let mut touched = HashSet::new();
    for item in &request.touch_evidence {
        let Some(binding) = bindings
            .iter()
            .find(|binding| binding.source_id == item.source_id)
        else {
            return validation_failure("touch evidence names an unknown source");
        };
        if !source_ids.contains(item.source_id.as_str())
            || !touched.insert(item.source_id.as_str())
            || manifest.sources[binding.source_ordinal].source_class != SourceClass::Consumed
            || item.project_id != request.project_id
            || item.durable_role_id != request.durable_role_id
            || item.context_manifest_id != request.context_manifest_id
            || item.context_epoch_id != request.context_epoch_id
            || request.task_id.as_deref() != Some(item.task_id.as_str())
            || request.correlation_reference.as_deref() != Some(item.correlation_reference.as_str())
        {
            return validation_failure("touch evidence scope mismatch or duplicate");
        }
    }
    let mut demanded = HashSet::new();
    for item in &request.demands {
        let Some(binding) = bindings
            .iter()
            .find(|binding| binding.source_id == item.source_id)
        else {
            return validation_failure("source demand names an unknown source");
        };
        if !source_ids.contains(item.source_id.as_str())
            || !demanded.insert(item.source_id.as_str())
            || manifest.sources[binding.source_ordinal].source_class != SourceClass::Reference
            || item.project_id != request.project_id
            || item.durable_role_id != request.durable_role_id
            || item.context_manifest_id != request.context_manifest_id
            || item.context_epoch_id != request.context_epoch_id
            || item.task_id != request.task_id
            || request.correlation_reference.as_deref() != Some(item.correlation_reference.as_str())
        {
            return validation_failure("source demand scope mismatch or duplicate");
        }
    }
    Ok(())
}

fn attempt_from_request(
    request: &ContextRehydrationRequest,
    status: ContextRehydrationStatus,
    source_evidence: Vec<ContextRehydrationSourceEvidence>,
    failure_code: Option<String>,
) -> ContextRehydrationAttempt {
    let mut snapshots: Vec<_> = request
        .source_bindings
        .iter()
        .filter_map(|binding| match &binding.source_ref {
            BoundContextSourceRefV1::RepositorySnapshot(value) => Some(value.clone()),
            _ => None,
        })
        .collect();
    snapshots.sort_by(|left, right| {
        (
            &left.repository_id,
            &left.commit_sha,
            &left.logical_relative_path,
        )
            .cmp(&(
                &right.repository_id,
                &right.commit_sha,
                &right.logical_relative_path,
            ))
    });
    ContextRehydrationAttempt {
        rehydration_attempt_id: request.rehydration_attempt_id.clone(),
        project_id: request.project_id.clone(),
        durable_role_id: request.durable_role_id.clone(),
        context_manifest_id: request.context_manifest_id.clone(),
        context_epoch_id: request.context_epoch_id,
        repository_snapshot_references: snapshots,
        requested_by_actor: request.requested_by_actor.clone(),
        executor_binding_id: request.executor_binding_id.clone(),
        session_reference: request.session_reference.clone(),
        task_id: request.task_id.clone(),
        correlation_reference: request.correlation_reference.clone(),
        trigger_kind: request.trigger_kind,
        trigger_reference: request.trigger_reference.clone(),
        started_at: request.started_at.clone(),
        completed_at: request.completed_at.clone(),
        status,
        source_evidence,
        failure_code,
    }
}

fn validate_attempt(attempt: &ContextRehydrationAttempt) -> Result<(), StateError> {
    ensure_identifier("rehydration_attempt_id", &attempt.rehydration_attempt_id)?;
    ensure_identifier("project_id", &attempt.project_id)?;
    ensure_identifier("durable_role_id", &attempt.durable_role_id)?;
    ensure_identifier("context_manifest_id", &attempt.context_manifest_id)?;
    ensure_non_empty("started_at", &attempt.started_at)?;
    ensure_non_empty("completed_at", &attempt.completed_at)?;
    if (attempt.status == ContextRehydrationStatus::Succeeded) != attempt.failure_code.is_none() {
        return validation_failure("attempt status/failure_code mismatch");
    }
    if attempt.status == ContextRehydrationStatus::Succeeded && attempt.source_evidence.is_empty() {
        return validation_failure("successful attempt source evidence cannot be empty");
    }
    let mut ids = HashSet::new();
    for item in &attempt.source_evidence {
        ensure_identifier("source_id", &item.source_id)?;
        if !ids.insert(item.source_id.as_str()) {
            return validation_failure("duplicate source evidence identity");
        }
    }
    Ok(())
}

fn validate_boundary_a(
    request: &ContextRehydrationRequest,
    manifest: &ContextManifest,
) -> Result<(), StateError> {
    if request.source_bindings.len() > MAX_SOURCE_EVIDENCE_RECORDS_PER_ATTEMPT {
        return validation_failure("REHYDRATION_SOURCE_COUNT_LIMIT_EXCEEDED");
    }
    let snapshot_count = request
        .source_bindings
        .iter()
        .filter(|binding| {
            matches!(
                binding.source_ref,
                BoundContextSourceRefV1::RepositorySnapshot(_)
            )
        })
        .count();
    if snapshot_count > MAX_REPOSITORY_SNAPSHOT_REFERENCES_PER_ATTEMPT {
        return validation_failure("REPOSITORY_SNAPSHOT_REFERENCE_LIMIT_EXCEEDED");
    }
    ensure_bytes(
        "rehydration_attempt_id",
        &request.rehydration_attempt_id,
        MAX_OTHER_EXTERNAL_REFERENCE_BYTES,
    )?;
    ensure_bytes(
        "project_id",
        &request.project_id,
        MAX_OTHER_EXTERNAL_REFERENCE_BYTES,
    )?;
    ensure_bytes(
        "durable_role_id",
        &request.durable_role_id,
        MAX_OTHER_EXTERNAL_REFERENCE_BYTES,
    )?;
    ensure_bytes(
        "context_manifest_id",
        &request.context_manifest_id,
        MAX_OTHER_EXTERNAL_REFERENCE_BYTES,
    )?;
    ensure_bytes("started_at", &request.started_at, MAX_TIMESTAMP_BYTES)?;
    ensure_bytes("completed_at", &request.completed_at, MAX_TIMESTAMP_BYTES)?;
    validate_optional_bytes(
        "executor_binding_id",
        &request.executor_binding_id,
        MAX_OTHER_EXTERNAL_REFERENCE_BYTES,
    )?;
    validate_optional_bytes(
        "session_reference",
        &request.session_reference,
        MAX_SESSION_REFERENCE_BYTES,
    )?;
    validate_optional_bytes("task_id", &request.task_id, MAX_TASK_REFERENCE_BYTES)?;
    validate_optional_bytes(
        "correlation_reference",
        &request.correlation_reference,
        MAX_CORRELATION_REFERENCE_BYTES,
    )?;
    validate_optional_bytes(
        "trigger_reference",
        &request.trigger_reference,
        MAX_TRIGGER_REFERENCE_BYTES,
    )?;
    if let Some(actor_id) = &request.requested_by_actor.id {
        ensure_bytes("requested_by_actor.id", actor_id, MAX_ACTOR_REFERENCE_BYTES)?;
    }
    if manifest.sources.len() > MAX_SOURCE_EVIDENCE_RECORDS_PER_ATTEMPT {
        return validation_failure("REHYDRATION_SOURCE_COUNT_LIMIT_EXCEEDED");
    }
    if manifest
        .sources
        .iter()
        .any(|source| !valid_source_digest(&source.digest))
    {
        return validation_failure("INVALID_SOURCE_DIGEST");
    }
    for (ordinal, binding) in request.source_bindings.iter().enumerate() {
        ensure_bytes(
            "source_id",
            &binding.source_id,
            MAX_OTHER_EXTERNAL_REFERENCE_BYTES,
        )?;
        let identity = canonical_source_identity(&binding.source_ref, &request.project_id)?;
        if identity.len() > MAX_SOURCE_IDENTITY_CANONICAL_BYTES {
            return validation_failure(format!(
                "SOURCE_IDENTITY_SIZE_LIMIT_EXCEEDED ordinal={ordinal} observed={} allowed={MAX_SOURCE_IDENTITY_CANONICAL_BYTES}",
                identity.len()
            ));
        }
        match &binding.source_ref {
            BoundContextSourceRefV1::RepositorySnapshot(reference) => {
                ensure_bytes(
                    "repository_id",
                    &reference.repository_id,
                    MAX_REPOSITORY_ID_BYTES,
                )?;
                ensure_bytes(
                    "commit_sha",
                    &reference.commit_sha,
                    MAX_COMMIT_SHA_TEXT_BYTES,
                )?;
                validate_relative_path(&reference.logical_relative_path)?;
            }
            BoundContextSourceRefV1::StateQuery(query) => {
                query.validate_parameters(&request.project_id)?;
            }
            BoundContextSourceRefV1::Artifact(reference) => {
                ensure_bytes(
                    "artifact_id",
                    &reference.artifact_id,
                    MAX_OTHER_EXTERNAL_REFERENCE_BYTES,
                )?;
            }
        }
    }
    for touch in &request.touch_evidence {
        let canonical = canonical_touch_evidence(touch)?;
        ensure_bytes(
            "touch_evidence",
            &canonical,
            MAX_TOUCH_EVIDENCE_REFERENCE_BYTES,
        )?;
        ensure_bytes("touch task_id", &touch.task_id, MAX_TASK_REFERENCE_BYTES)?;
        ensure_bytes(
            "touch correlation_reference",
            &touch.correlation_reference,
            MAX_CORRELATION_REFERENCE_BYTES,
        )?;
    }
    for demand in &request.demands {
        ensure_bytes(
            "demand source_id",
            &demand.source_id,
            MAX_OTHER_EXTERNAL_REFERENCE_BYTES,
        )?;
        validate_optional_bytes("demand task_id", &demand.task_id, MAX_TASK_REFERENCE_BYTES)?;
        ensure_bytes(
            "demand correlation_reference",
            &demand.correlation_reference,
            MAX_CORRELATION_REFERENCE_BYTES,
        )?;
    }
    Ok(())
}

fn validate_boundary_b(attempt: &ContextRehydrationAttempt) -> Result<(), StateError> {
    if attempt.source_evidence.len() > MAX_SOURCE_EVIDENCE_RECORDS_PER_ATTEMPT {
        return validation_failure("REHYDRATION_SOURCE_COUNT_LIMIT_EXCEEDED");
    }
    if attempt.repository_snapshot_references.len() > MAX_REPOSITORY_SNAPSHOT_REFERENCES_PER_ATTEMPT
    {
        return validation_failure("REPOSITORY_SNAPSHOT_REFERENCE_LIMIT_EXCEEDED");
    }
    ensure_bytes("started_at", &attempt.started_at, MAX_TIMESTAMP_BYTES)?;
    ensure_bytes("completed_at", &attempt.completed_at, MAX_TIMESTAMP_BYTES)?;
    validate_optional_bytes(
        "session_reference",
        &attempt.session_reference,
        MAX_SESSION_REFERENCE_BYTES,
    )?;
    validate_optional_bytes("task_id", &attempt.task_id, MAX_TASK_REFERENCE_BYTES)?;
    validate_optional_bytes(
        "correlation_reference",
        &attempt.correlation_reference,
        MAX_CORRELATION_REFERENCE_BYTES,
    )?;
    validate_optional_bytes(
        "trigger_reference",
        &attempt.trigger_reference,
        MAX_TRIGGER_REFERENCE_BYTES,
    )?;
    if let Some(actor_id) = &attempt.requested_by_actor.id {
        ensure_bytes("requested_by_actor.id", actor_id, MAX_ACTOR_REFERENCE_BYTES)?;
    }
    if let Some(code) = &attempt.failure_code {
        ensure_failure_code(code)?;
    }
    for reference in &attempt.repository_snapshot_references {
        validate_relative_path(&reference.logical_relative_path)?;
        let size = serde_json_canonicalizer::to_vec(reference)
            .map_err(canonicalization_failure)?
            .len();
        if size > MAX_SINGLE_REPOSITORY_SNAPSHOT_REFERENCE_CANONICAL_BYTES {
            return validation_failure("REPOSITORY_SNAPSHOT_REFERENCE_SIZE_LIMIT_EXCEEDED");
        }
    }
    for item in &attempt.source_evidence {
        ensure_bytes(
            "canonical_source_identity",
            &item.canonical_source_identity,
            MAX_SOURCE_IDENTITY_CANONICAL_BYTES,
        )?;
        validate_optional_bytes(
            "materializer_id",
            &item.materializer_id,
            MAX_AUTHORITATIVE_BINDING_CANONICAL_BYTES,
        )?;
        validate_optional_bytes(
            "provenance",
            &item.provenance,
            MAX_MATERIALIZER_PROVENANCE_CANONICAL_BYTES,
        )?;
        validate_optional_bytes(
            "touch_evidence",
            &item.touch_evidence,
            MAX_TOUCH_EVIDENCE_REFERENCE_BYTES,
        )?;
        validate_optional_bytes(
            "materialized_at",
            &item.materialized_at,
            MAX_TIMESTAMP_BYTES,
        )?;
        validate_optional_bytes(
            "failure_detail",
            &item.failure_detail,
            MAX_FAILURE_DETAIL_PERSISTED_BYTES,
        )?;
        if !valid_source_digest(&item.expected_digest)
            || item
                .observed_digest
                .as_ref()
                .is_some_and(|digest| !valid_source_digest(digest))
        {
            return validation_failure("INVALID_SOURCE_DIGEST");
        }
        if let Some(code) = &item.failure_code {
            ensure_failure_code(code)?;
        }
        let size = serde_json_canonicalizer::to_vec(item)
            .map_err(canonicalization_failure)?
            .len();
        if size > MAX_SINGLE_SOURCE_EVIDENCE_RECORD_CANONICAL_BYTES {
            return validation_failure("REHYDRATION_SOURCE_EVIDENCE_RECORD_SIZE_LIMIT_EXCEEDED");
        }
    }
    if attempt_digest_projection(attempt)?.len() > MAX_REHYDRATION_ATTEMPT_RECORD_CANONICAL_BYTES {
        return validation_failure("REHYDRATION_EVIDENCE_RECORD_SIZE_LIMIT_EXCEEDED");
    }
    Ok(())
}

fn deferred_evidence(
    binding: &ContextSourceBindingV1,
    source: &crate::context_manifest::ContextManifestSource,
    ordinal: usize,
    project_id: &str,
) -> Result<ContextRehydrationSourceEvidence, StateError> {
    Ok(ContextRehydrationSourceEvidence {
        source_id: binding.source_id.clone(),
        source_ordinal: ordinal,
        ref_type: source.r#ref.ref_type.as_str().to_string(),
        source_class: source.source_class.as_str().to_string(),
        canonical_source_identity: canonical_source_identity(&binding.source_ref, project_id)?,
        materializer_id: None,
        provenance: None,
        expected_digest: source.digest.clone(),
        observed_digest: None,
        comparison: SourceDigestComparison::NotChecked,
        touch_evidence: None,
        disposition: SourceDisposition::Deferred,
        materialized_at: None,
        failure_code: None,
        failure_detail: None,
    })
}

fn failed_evidence(
    binding: &ContextSourceBindingV1,
    source: &crate::context_manifest::ContextManifestSource,
    ordinal: usize,
    code: &str,
    failure: Option<&SourceMaterializationFailure>,
    project_id: &str,
) -> Result<ContextRehydrationSourceEvidence, StateError> {
    Ok(ContextRehydrationSourceEvidence {
        source_id: binding.source_id.clone(),
        source_ordinal: ordinal,
        ref_type: source.r#ref.ref_type.as_str().to_string(),
        source_class: source.source_class.as_str().to_string(),
        canonical_source_identity: canonical_source_identity(&binding.source_ref, project_id)?,
        materializer_id: failure.and_then(|value| value.materializer_id.clone()),
        provenance: failure.and_then(|value| value.provenance.clone()),
        expected_digest: source.digest.clone(),
        observed_digest: None,
        comparison: SourceDigestComparison::Failed,
        touch_evidence: None,
        disposition: SourceDisposition::Failed,
        materialized_at: failure.and_then(|value| value.materialized_at.clone()),
        failure_code: Some(code.to_string()),
        failure_detail: failure
            .and_then(|value| value.failure_detail.as_deref())
            .map(sanitize_failure_detail),
    })
}

fn canonical_touch_evidence(item: &ContextSourceTouchEvidence) -> Result<String, StateError> {
    #[derive(Serialize)]
    struct Evidence<'a> {
        context_epoch_id: i64,
        context_manifest_id: &'a str,
        correlation_reference: &'a str,
        durable_role_id: &'a str,
        project_id: &'a str,
        relation: &'static str,
        source_id: &'a str,
        task_id: &'a str,
        v: u8,
    }
    serde_json_canonicalizer::to_string(&Evidence {
        context_epoch_id: item.context_epoch_id,
        context_manifest_id: &item.context_manifest_id,
        correlation_reference: &item.correlation_reference,
        durable_role_id: &item.durable_role_id,
        project_id: &item.project_id,
        relation: "CONSUMES",
        source_id: &item.source_id,
        task_id: &item.task_id,
        v: 1,
    })
    .map_err(canonicalization_failure)
}

fn validate_relative_path(path: &str) -> Result<(), StateError> {
    ensure_non_empty("logical_relative_path", path)?;
    ensure_bytes(
        "logical_relative_path",
        path,
        MAX_REPOSITORY_RELATIVE_PATH_BYTES,
    )?;
    let bytes = path.as_bytes();
    if path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\0')
        || bytes.get(1) == Some(&b':')
        || path
            .split(['/', '\\'])
            .any(|part| part.is_empty() || part == "..")
    {
        return validation_failure("repository path must be a safe logical relative path");
    }
    Ok(())
}

fn validate_materialization(value: &ExternalSourceMaterialization) -> Result<(), StateError> {
    ensure_non_empty("materializer_id", &value.materializer_id)?;
    ensure_non_empty("provenance", &value.provenance)?;
    ensure_non_empty("materialized_at", &value.materialized_at)?;
    ensure_bounded_evidence("materializer_id", &value.materializer_id)?;
    ensure_bounded_evidence("provenance", &value.provenance)
}

fn validate_failure(value: &SourceMaterializationFailure) -> Result<(), StateError> {
    ensure_failure_code(&value.code)?;
    validate_optional("materializer_id", &value.materializer_id)?;
    validate_optional("provenance", &value.provenance)?;
    validate_optional("materialized_at", &value.materialized_at)?;
    if let Some(materializer_id) = &value.materializer_id {
        ensure_bounded_evidence("materializer_id", materializer_id)?;
    }
    if let Some(provenance) = &value.provenance {
        ensure_bounded_evidence("provenance", provenance)?;
    }
    Ok(())
}

fn ensure_bounded_evidence(field: &str, value: &str) -> Result<(), StateError> {
    ensure_bytes(field, value, MAX_MATERIALIZER_PROVENANCE_CANONICAL_BYTES)
}

fn validate_actor(actor: &EventActor) -> Result<(), StateError> {
    if let Some(id) = &actor.id {
        ensure_non_empty("requested_by_actor.id", id)?;
    }
    Ok(())
}

fn valid_source_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:v1:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(is_lower_hex))
}

fn valid_attempt_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:context-rehydration-attempt:v1:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(is_lower_hex))
}

fn is_lower_hex(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
}

fn ensure_identifier(field: &str, value: &str) -> Result<(), StateError> {
    ensure_non_empty(field, value)?;
    if value.chars().count() > MAX_IDENTIFIER_LENGTH {
        return validation_failure(format!(
            "{field} exceeds {MAX_IDENTIFIER_LENGTH} characters"
        ));
    }
    Ok(())
}

fn ensure_non_empty(field: &str, value: &str) -> Result<(), StateError> {
    if value.is_empty() {
        return validation_failure(format!("{field} must be non-empty"));
    }
    Ok(())
}

fn validate_optional(field: &str, value: &Option<String>) -> Result<(), StateError> {
    if let Some(value) = value {
        ensure_non_empty(field, value)?;
    }
    Ok(())
}

fn ensure_bytes(field: &str, value: &str, maximum: usize) -> Result<(), StateError> {
    ensure_non_empty(field, value)?;
    if value.len() > maximum {
        return validation_failure(format!(
            "{field} exceeds {maximum} UTF-8 bytes (observed {})",
            value.len()
        ));
    }
    Ok(())
}

fn validate_optional_bytes(
    field: &str,
    value: &Option<String>,
    maximum: usize,
) -> Result<(), StateError> {
    if let Some(value) = value {
        ensure_bytes(field, value, maximum)?;
    }
    Ok(())
}

fn ensure_failure_code(value: &str) -> Result<(), StateError> {
    if value.is_empty()
        || value.len() > MAX_FAILURE_CODE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return validation_failure("failure code must be closed bounded ASCII");
    }
    Ok(())
}

/// Captures, redacts, sanitizes, and UTF-8-safely bounds external error text.
pub fn sanitize_failure_detail(external: &str) -> String {
    const SUFFIX: &str = "...[TRUNCATED]";
    let captured = utf8_prefix(external, MAX_EXTERNAL_ERROR_CAPTURE_BYTES);
    let mut safe = String::new();
    for segment in captured.split_inclusive('\n') {
        let lower = segment.to_ascii_lowercase();
        if ["token", "secret", "password", "api_key", "authorization"]
            .iter()
            .any(|word| lower.contains(word))
        {
            safe.push_str("[REDACTED]");
            if segment.ends_with('\n') {
                safe.push('\n');
            }
            continue;
        }
        safe.extend(segment.chars().map(|character| {
            if character.is_control() && character != '\n' && character != '\t' {
                ' '
            } else {
                character
            }
        }));
    }
    if safe.len() <= MAX_FAILURE_DETAIL_PERSISTED_BYTES {
        return safe;
    }
    let prefix = utf8_prefix(&safe, MAX_FAILURE_DETAIL_PERSISTED_BYTES - SUFFIX.len());
    format!("{prefix}{SUFFIX}")
}

fn utf8_prefix(value: &str, maximum: usize) -> &str {
    if value.len() <= maximum {
        return value;
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

fn closed_request_error(error: &StateError) -> String {
    match error {
        StateError::ContextRehydrationValidation { detail } => {
            const CODES: [&str; 11] = [
                "REHYDRATION_SOURCE_COUNT_LIMIT_EXCEEDED",
                "REPOSITORY_SNAPSHOT_REFERENCE_LIMIT_EXCEEDED",
                "SOURCE_IDENTITY_SIZE_LIMIT_EXCEEDED",
                "REPOSITORY_SNAPSHOT_REFERENCE_SIZE_LIMIT_EXCEEDED",
                "REHYDRATION_SOURCE_EVIDENCE_RECORD_SIZE_LIMIT_EXCEEDED",
                "REHYDRATION_EVIDENCE_RECORD_SIZE_LIMIT_EXCEEDED",
                "SOURCE_MATERIALIZATION_SIZE_LIMIT_EXCEEDED",
                "REHYDRATION_MATERIALIZATION_TOTAL_LIMIT_EXCEEDED",
                "REHYDRATION_SIZE_COUNTER_OVERFLOW",
                "STATE_QUERY_RESULT_SIZE_LIMIT_EXCEEDED",
                "INVALID_SOURCE_DIGEST",
            ];
            CODES
                .iter()
                .find(|code| detail.starts_with(**code))
                .copied()
                .unwrap_or("REHYDRATION_REQUEST_INVALID")
                .to_string()
        }
        _ => "REHYDRATION_REQUEST_INVALID".to_string(),
    }
}

fn validation_failure<T>(detail: impl Into<String>) -> Result<T, StateError> {
    Err(StateError::ContextRehydrationValidation {
        detail: detail.into(),
    })
}

fn canonicalization_failure(error: impl std::fmt::Display) -> StateError {
    StateError::ContextRehydrationValidation {
        detail: format!("RFC 8785 canonicalization failed: {error}"),
    }
}

fn decode_failure(detail: impl Into<String>) -> StateError {
    StateError::ContextRehydrationDecodeFailed {
        detail: detail.into(),
    }
}

fn insert_attempt(
    conn: &Connection,
    attempt: &ContextRehydrationAttempt,
) -> Result<(), StateError> {
    let inserted = conn.execute(
        "INSERT INTO context_rehydration_attempt (
            project_id, rehydration_attempt_id, durable_role_id, context_manifest_id,
            context_epoch_id, trigger_kind, trigger_reference, task_id,
            correlation_reference, requested_by_actor_kind, requested_by_actor_id,
            executor_binding_id, session_reference, started_at, completed_at, status,
            failure_code
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            attempt.project_id,
            attempt.rehydration_attempt_id,
            attempt.durable_role_id,
            attempt.context_manifest_id,
            attempt.context_epoch_id,
            attempt.trigger_kind.as_str(),
            attempt.trigger_reference,
            attempt.task_id,
            attempt.correlation_reference,
            attempt.requested_by_actor.kind.as_str(),
            attempt.requested_by_actor.id,
            attempt.executor_binding_id,
            attempt.session_reference,
            attempt.started_at,
            attempt.completed_at,
            attempt.status.as_str(),
            attempt.failure_code,
        ],
    );
    if let Err(error) = inserted {
        if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
            let exists: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM context_rehydration_attempt WHERE project_id = ?1 AND rehydration_attempt_id = ?2)",
                    params![attempt.project_id, attempt.rehydration_attempt_id],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            if exists {
                return Err(StateError::ContextRehydrationAttemptAlreadyExists {
                    project_id: attempt.project_id.clone(),
                    rehydration_attempt_id: attempt.rehydration_attempt_id.clone(),
                });
            }
        }
        return Err(StateError::ContextRehydrationWriteFailed {
            detail: error.to_string(),
        });
    }
    for (ordinal, reference) in attempt.repository_snapshot_references.iter().enumerate() {
        conn.execute(
            "INSERT INTO context_rehydration_repository_snapshot (
                project_id, rehydration_attempt_id, snapshot_ordinal, repository_id,
                commit_sha, logical_relative_path
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                attempt.project_id,
                attempt.rehydration_attempt_id,
                ordinal as i64,
                reference.repository_id,
                reference.commit_sha,
                reference.logical_relative_path,
            ],
        )
        .map_err(write_failure)?;
    }
    for item in &attempt.source_evidence {
        conn.execute(
            "INSERT INTO context_rehydration_source_evidence (
                project_id, rehydration_attempt_id, source_ordinal, source_id, ref_type,
                source_class, canonical_source_identity, materializer_id, provenance,
                expected_digest, observed_digest, comparison, touch_evidence, disposition,
                materialized_at, failure_code, failure_detail
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
            params![
                attempt.project_id,
                attempt.rehydration_attempt_id,
                item.source_ordinal as i64,
                item.source_id,
                item.ref_type,
                item.source_class,
                item.canonical_source_identity,
                item.materializer_id,
                item.provenance,
                item.expected_digest,
                item.observed_digest,
                item.comparison.as_str(),
                item.touch_evidence,
                item.disposition.as_str(),
                item.materialized_at,
                item.failure_code,
                item.failure_detail,
            ],
        )
        .map_err(write_failure)?;
    }
    Ok(())
}

fn read_attempt(
    conn: &Connection,
    project_id: &str,
    attempt_id: &str,
) -> Result<Option<ContextRehydrationAttempt>, StateError> {
    type Row = (
        String,
        String,
        String,
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
        Option<String>,
    );
    let row: Option<Row> = conn
        .query_row(
            "SELECT durable_role_id, context_manifest_id, project_id, context_epoch_id,
                trigger_kind, trigger_reference, task_id, correlation_reference,
                requested_by_actor_kind, requested_by_actor_id, executor_binding_id,
                session_reference, started_at, completed_at, status, failure_code
             FROM context_rehydration_attempt
             WHERE project_id = ?1 AND rehydration_attempt_id = ?2",
            params![project_id, attempt_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get(9)?,
                    row.get(10)?,
                    row.get(11)?,
                    row.get(12)?,
                    row.get(13)?,
                    row.get(14)?,
                    row.get(15)?,
                ))
            },
        )
        .optional()
        .map_err(read_failure)?;
    let Some(row) = row else { return Ok(None) };
    let snapshots = read_snapshots(conn, project_id, attempt_id)?;
    let evidence = read_evidence(conn, project_id, attempt_id)?;
    let attempt = ContextRehydrationAttempt {
        rehydration_attempt_id: attempt_id.to_string(),
        durable_role_id: row.0,
        context_manifest_id: row.1,
        project_id: row.2,
        context_epoch_id: row.3,
        trigger_kind: ContextEpochTrigger::from_storage(&row.4)?,
        trigger_reference: row.5,
        task_id: row.6,
        correlation_reference: row.7,
        requested_by_actor: EventActor {
            kind: ActorKind::from_storage(&row.8)?,
            id: row.9,
        },
        executor_binding_id: row.10,
        session_reference: row.11,
        started_at: row.12,
        completed_at: row.13,
        status: ContextRehydrationStatus::from_storage(&row.14)?,
        failure_code: row.15,
        repository_snapshot_references: snapshots,
        source_evidence: evidence,
    };
    validate_attempt(&attempt).map_err(|error| decode_failure(error.to_string()))?;
    validate_boundary_b(&attempt).map_err(|error| decode_failure(error.to_string()))?;
    Ok(Some(attempt))
}

fn read_snapshots(
    conn: &Connection,
    project_id: &str,
    attempt_id: &str,
) -> Result<Vec<RepositorySnapshotRefV1>, StateError> {
    let mut statement = conn
        .prepare("SELECT repository_id, commit_sha, logical_relative_path FROM context_rehydration_repository_snapshot WHERE project_id = ?1 AND rehydration_attempt_id = ?2 ORDER BY snapshot_ordinal")
        .map_err(read_failure)?;
    statement
        .query_map(params![project_id, attempt_id], |row| {
            Ok(RepositorySnapshotRefV1 {
                project_id: project_id.to_string(),
                repository_id: row.get(0)?,
                commit_sha: row.get(1)?,
                logical_relative_path: row.get(2)?,
            })
        })
        .map_err(read_failure)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(read_failure)
}

fn read_evidence(
    conn: &Connection,
    project_id: &str,
    attempt_id: &str,
) -> Result<Vec<ContextRehydrationSourceEvidence>, StateError> {
    let mut statement = conn
        .prepare("SELECT source_ordinal, source_id, ref_type, source_class, canonical_source_identity, materializer_id, provenance, expected_digest, observed_digest, comparison, touch_evidence, disposition, materialized_at, failure_code, failure_detail FROM context_rehydration_source_evidence WHERE project_id = ?1 AND rehydration_attempt_id = ?2 ORDER BY source_ordinal")
        .map_err(read_failure)?;
    statement
        .query_map(params![project_id, attempt_id], |row| {
            let ordinal: i64 = row.get(0)?;
            Ok((
                ordinal,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, Option<String>>(10)?,
                row.get::<_, String>(11)?,
                row.get::<_, Option<String>>(12)?,
                row.get::<_, Option<String>>(13)?,
                row.get::<_, Option<String>>(14)?,
            ))
        })
        .map_err(read_failure)?
        .map(|row| {
            let row = row.map_err(read_failure)?;
            Ok(ContextRehydrationSourceEvidence {
                source_ordinal: usize::try_from(row.0)
                    .map_err(|_| decode_failure("negative source ordinal"))?,
                source_id: row.1,
                ref_type: row.2,
                source_class: row.3,
                canonical_source_identity: row.4,
                materializer_id: row.5,
                provenance: row.6,
                expected_digest: row.7,
                observed_digest: row.8,
                comparison: SourceDigestComparison::from_storage(&row.9)?,
                touch_evidence: row.10,
                disposition: SourceDisposition::from_storage(&row.11)?,
                materialized_at: row.12,
                failure_code: row.13,
                failure_detail: row.14,
            })
        })
        .collect()
}

fn write_failure(error: rusqlite::Error) -> StateError {
    StateError::ContextRehydrationWriteFailed {
        detail: error.to_string(),
    }
}

fn read_failure(error: rusqlite::Error) -> StateError {
    StateError::ContextRehydrationDecodeFailed {
        detail: error.to_string(),
    }
}
