use std::collections::HashMap;

use crate::context_epoch::{ContextEpoch, ContextEpochTrigger};
use crate::context_manifest::{
    ContextManifest, ContextManifestSource, ContextSourceRef, ContextSourceRefType, SourceClass,
};
use crate::context_rehydration::{
    ArtifactMaterializer, ArtifactRefV1, BoundContextSourceRefV1, ContextRehydratedEventSupplier,
    ContextRehydrationRequest, ContextRehydrationStatus, ContextSourceBindingV1,
    ContextSourceDemand, ContextSourceTouchEvidence, ExternalSourceMaterialization,
    MAX_ACTOR_REFERENCE_BYTES, MAX_AUTHORITATIVE_BINDING_CANONICAL_BYTES,
    MAX_CORRELATION_REFERENCE_BYTES, MAX_EXTERNAL_ERROR_CAPTURE_BYTES,
    MAX_FAILURE_DETAIL_PERSISTED_BYTES, MAX_MATERIALIZED_BYTES_PER_ATTEMPT,
    MAX_MATERIALIZED_BYTES_PER_SOURCE, MAX_MATERIALIZER_PROVENANCE_CANONICAL_BYTES,
    MAX_SESSION_REFERENCE_BYTES, MAX_SINGLE_SOURCE_EVIDENCE_RECORD_CANONICAL_BYTES,
    MAX_STREAM_CHUNK_BYTES, MAX_TASK_REFERENCE_BYTES, MAX_TIMESTAMP_BYTES,
    MAX_TRIGGER_REFERENCE_BYTES, RepositorySnapshotMaterializer, RepositorySnapshotRefV1,
    SourceDisposition, SourceMaterializationFailure, StateQueryRefV1,
    context_rehydration_event_payload, context_source_digest_v1,
};
use crate::event::{ActorKind, EventActor, EventEnvelope, EventSubject, EventType, SubjectKind};
use crate::executor_binding::ExecutorBinding;
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

const PRIOR_DIGEST: &str =
    "sha256:v1:0000000000000000000000000000000000000000000000000000000000000000";

struct RepoMaterializer {
    values: HashMap<String, Result<Vec<u8>, &'static str>>,
    calls: usize,
}

impl RepositorySnapshotMaterializer for RepoMaterializer {
    fn known_length(
        &mut self,
        reference: &RepositorySnapshotRefV1,
    ) -> Result<Option<u64>, SourceMaterializationFailure> {
        Ok(self
            .values
            .get(&reference.logical_relative_path)
            .and_then(|value| value.as_ref().ok())
            .map(|bytes| bytes.len() as u64))
    }

    fn materialize(
        &mut self,
        reference: &RepositorySnapshotRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        self.calls += 1;
        match self.values.get(&reference.logical_relative_path) {
            Some(Ok(bytes)) => {
                accept_chunk(bytes)?;
                Ok(ExternalSourceMaterialization {
                    materializer_id: "fake-workspace-v1".to_string(),
                    provenance: format!("{}:{}", reference.repository_id, reference.commit_sha),
                    materialized_at: "2026-08-17T10:00:01Z".to_string(),
                })
            }
            Some(Err(code)) => Err(SourceMaterializationFailure {
                code: (*code).to_string(),
                materializer_id: Some("fake-workspace-v1".to_string()),
                provenance: Some(reference.logical_relative_path.clone()),
                materialized_at: Some("2026-08-17T10:00:01Z".to_string()),
                failure_detail: None,
            }),
            None => Err(SourceMaterializationFailure {
                code: "SOURCE_MISSING".to_string(),
                materializer_id: Some("fake-workspace-v1".to_string()),
                provenance: None,
                materialized_at: Some("2026-08-17T10:00:01Z".to_string()),
                failure_detail: None,
            }),
        }
    }
}

struct ArtifactFake {
    values: HashMap<String, Vec<u8>>,
    calls: usize,
}

struct StreamingRepoMaterializer {
    known: bool,
    calls: usize,
    known_calls: usize,
    chunk_size: usize,
    fail_on_call: Option<usize>,
    lengths_by_call: Vec<u64>,
    fills_by_call: Vec<u8>,
    materializer_id: String,
    provenance: String,
    materialized_at: String,
}

impl StreamingRepoMaterializer {
    fn new(known: bool, chunk_size: usize) -> Self {
        Self {
            known,
            calls: 0,
            known_calls: 0,
            chunk_size,
            fail_on_call: None,
            lengths_by_call: Vec::new(),
            fills_by_call: Vec::new(),
            materializer_id: "streaming-workspace-v1".to_string(),
            provenance: "streaming-test".to_string(),
            materialized_at: "2026-08-17T10:00:01Z".to_string(),
        }
    }

    fn reference_length(reference: &RepositorySnapshotRefV1) -> u64 {
        reference
            .logical_relative_path
            .rsplit('/')
            .next()
            .expect("size path")
            .parse()
            .expect("numeric size path")
    }
}

impl RepositorySnapshotMaterializer for StreamingRepoMaterializer {
    fn known_length(
        &mut self,
        reference: &RepositorySnapshotRefV1,
    ) -> Result<Option<u64>, SourceMaterializationFailure> {
        self.known_calls += 1;
        Ok(self.known.then(|| Self::reference_length(reference)))
    }

    fn materialize(
        &mut self,
        reference: &RepositorySnapshotRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        self.calls += 1;
        if self.fail_on_call == Some(self.calls) {
            return Err(SourceMaterializationFailure {
                code: "SOURCE_REREAD_FAILED".to_string(),
                materializer_id: Some(self.materializer_id.clone()),
                provenance: Some(self.provenance.clone()),
                materialized_at: Some(self.materialized_at.clone()),
                failure_detail: Some("bounded reread failure".to_string()),
            });
        }
        let length = self
            .lengths_by_call
            .get(self.calls - 1)
            .copied()
            .unwrap_or_else(|| Self::reference_length(reference));
        let fill = self
            .fills_by_call
            .get(self.calls - 1)
            .copied()
            .unwrap_or(b'x');
        let mut remaining = usize::try_from(length).expect("test length fits usize");
        while remaining != 0 {
            let length = remaining.min(self.chunk_size);
            accept_chunk(&vec![fill; length])?;
            remaining -= length;
        }
        Ok(ExternalSourceMaterialization {
            materializer_id: self.materializer_id.clone(),
            provenance: self.provenance.clone(),
            materialized_at: self.materialized_at.clone(),
        })
    }
}

impl ArtifactMaterializer for ArtifactFake {
    fn known_length(
        &mut self,
        reference: &ArtifactRefV1,
    ) -> Result<Option<u64>, SourceMaterializationFailure> {
        Ok(self
            .values
            .get(&reference.artifact_id)
            .map(|bytes| bytes.len() as u64))
    }

    fn materialize(
        &mut self,
        reference: &ArtifactRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        self.calls += 1;
        let bytes = self
            .values
            .get(&reference.artifact_id)
            .cloned()
            .ok_or_else(|| SourceMaterializationFailure {
                code: "ARTIFACT_MISSING".to_string(),
                materializer_id: Some("fake-artifact-v1".to_string()),
                provenance: None,
                materialized_at: Some("2026-08-17T10:00:01Z".to_string()),
                failure_detail: None,
            })?;
        accept_chunk(&bytes)?;
        Ok(ExternalSourceMaterialization {
            materializer_id: "fake-artifact-v1".to_string(),
            provenance: reference.artifact_id.clone(),
            materialized_at: "2026-08-17T10:00:01Z".to_string(),
        })
    }
}

struct EventSupplier {
    calls: usize,
    corrupt_digest: bool,
}

impl ContextRehydratedEventSupplier for EventSupplier {
    fn event_for(
        &mut self,
        attempt: &crate::context_rehydration::ContextRehydrationAttempt,
    ) -> EventEnvelope {
        self.calls += 1;
        let mut payload = context_rehydration_event_payload(attempt).expect("payload");
        if self.corrupt_digest {
            payload.digest.pop();
            payload.digest.push('0');
        }
        EventEnvelope {
            event_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            project_id: attempt.project_id.clone(),
            goal_id: None,
            event_type: EventType::ContextRehydrated,
            actor: attempt.requested_by_actor.clone(),
            subject: EventSubject {
                kind: SubjectKind::Role,
                id: attempt.durable_role_id.clone(),
            },
            occurred_at: attempt.completed_at.clone(),
            payload,
            correlation_id: attempt
                .correlation_reference
                .clone()
                .expect("test correlation"),
            epoch: attempt.context_epoch_id,
        }
    }
}

fn opened(tag: &str) -> (TempDir, SqliteStateRepository) {
    let tmp = TempDir::new(tag);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("open");
    (tmp, repo)
}

fn seed(repo: &mut SqliteStateRepository, sources: Vec<ContextManifestSource>) {
    repo.create_logical_role(LogicalRole {
        role_id: "role-1".to_string(),
        project_id: "project-1".to_string(),
        role_type: LogicalRoleType::RuntimeA2,
        status: LogicalRoleStatus::Active,
        current_context_epoch: 0,
        name: None,
        workstream_id: None,
        ownership_paths: Vec::new(),
        integration_branch: None,
        context_manifest_id: Some("manifest-1".to_string()),
        active_binding_id: None,
        created_at: Some("2026-08-17T09:00:00Z".to_string()),
    })
    .expect("role");
    repo.append_context_epoch(ContextEpoch {
        project_id: "project-1".to_string(),
        epoch: 0,
        advanced_at: "2026-08-17T09:00:00Z".to_string(),
        trigger: ContextEpochTrigger::A1Init,
    })
    .expect("epoch");
    repo.create_context_manifest(ContextManifest {
        manifest_id: "manifest-1".to_string(),
        role_id: "role-1".to_string(),
        project_id: "project-1".to_string(),
        epoch: 0,
        sources,
        created_at: "2026-08-17T09:00:00Z".to_string(),
        last_rehydrated_at: None,
    })
    .expect("manifest");
}

fn source(
    ref_type: ContextSourceRefType,
    target: &str,
    source_class: SourceClass,
) -> ContextManifestSource {
    ContextManifestSource {
        r#ref: ContextSourceRef {
            ref_type,
            target: target.to_string(),
        },
        source_class,
        digest: PRIOR_DIGEST.to_string(),
        last_read_at: None,
        required_for: Vec::new(),
    }
}

fn request(bindings: Vec<ContextSourceBindingV1>) -> ContextRehydrationRequest {
    ContextRehydrationRequest {
        rehydration_attempt_id: "attempt-1".to_string(),
        project_id: "project-1".to_string(),
        durable_role_id: "role-1".to_string(),
        context_manifest_id: "manifest-1".to_string(),
        context_epoch_id: 0,
        requested_by_actor: EventActor {
            kind: ActorKind::System,
            id: Some("orchestrator".to_string()),
        },
        executor_binding_id: None,
        session_reference: None,
        task_id: Some("task-1".to_string()),
        correlation_reference: Some("correlation-1".to_string()),
        trigger_kind: ContextEpochTrigger::HostSwitch,
        trigger_reference: Some("host-switch-1".to_string()),
        started_at: "2026-08-17T10:00:00Z".to_string(),
        completed_at: "2026-08-17T10:00:02Z".to_string(),
        source_bindings: bindings,
        touch_evidence: Vec::new(),
        demands: Vec::new(),
    }
}

fn repo_binding(ordinal: usize, source_id: &str, path: &str) -> ContextSourceBindingV1 {
    ContextSourceBindingV1 {
        source_ordinal: ordinal,
        source_id: source_id.to_string(),
        source_ref: BoundContextSourceRefV1::RepositorySnapshot(RepositorySnapshotRefV1 {
            project_id: "project-1".to_string(),
            repository_id: "repo-1".to_string(),
            commit_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            logical_relative_path: path.to_string(),
        }),
    }
}

fn artifact_binding(ordinal: usize, source_id: &str, id: &str) -> ContextSourceBindingV1 {
    ContextSourceBindingV1 {
        source_ordinal: ordinal,
        source_id: source_id.to_string(),
        source_ref: BoundContextSourceRefV1::Artifact(ArtifactRefV1 {
            project_id: "project-1".to_string(),
            artifact_id: id.to_string(),
        }),
    }
}

#[test]
fn success_rereads_required_sources_defers_reference_and_commits_event_atomically() {
    let (_tmp, mut repo) = opened("rehydration-success");
    seed(
        &mut repo,
        vec![
            source(
                ContextSourceRefType::RepoPath,
                "docs/spec.md",
                SourceClass::Mandatory,
            ),
            source(
                ContextSourceRefType::ArtifactId,
                "artifact-1",
                SourceClass::Consumed,
            ),
            source(
                ContextSourceRefType::ArtifactId,
                "artifact-2",
                SourceClass::Reference,
            ),
        ],
    );
    let mut repository = RepoMaterializer {
        values: HashMap::from([(
            "docs/spec.md".to_string(),
            Ok(b"authoritative spec".to_vec()),
        )]),
        calls: 0,
    };
    let mut artifacts = ArtifactFake {
        values: HashMap::from([
            ("artifact-1".to_string(), b"changed artifact".to_vec()),
            ("artifact-2".to_string(), b"on demand only".to_vec()),
        ]),
        calls: 0,
    };
    let mut events = EventSupplier {
        calls: 0,
        corrupt_digest: false,
    };
    let manifest_before = repo
        .find_context_manifest("manifest-1")
        .expect("manifest read");
    let epoch_before = repo.find_context_epoch("project-1", 0).expect("epoch read");
    let outcome = repo
        .rehydrate_context(
            request(vec![
                repo_binding(0, "source-repo", "docs/spec.md"),
                artifact_binding(1, "source-consumed", "artifact-1"),
                artifact_binding(2, "source-reference", "artifact-2"),
            ]),
            &mut repository,
            &mut artifacts,
            &mut events,
        )
        .expect("rehydration succeeds");

    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Succeeded);
    assert_eq!(outcome.sources.len(), 2);
    assert_eq!(repository.calls, 1);
    assert_eq!(
        artifacts.calls, 2,
        "changed CONSUMED is genuinely reread; REFERENCE stays deferred"
    );
    assert_eq!(events.calls, 1);
    assert_eq!(
        repo.find_context_manifest("manifest-1")
            .expect("manifest read"),
        manifest_before
    );
    assert_eq!(
        repo.find_context_epoch("project-1", 0).expect("epoch read"),
        epoch_before
    );
    assert_eq!(
        outcome.attempt.source_evidence[2].disposition,
        SourceDisposition::Deferred
    );
    assert_eq!(
        repo.find_context_rehydration_attempt("project-1", "attempt-1")
            .expect("read"),
        Some(outcome.attempt.clone())
    );
    assert!(
        repo.find_event("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .expect("event read")
            .is_some()
    );
    let payload = context_rehydration_event_payload(&outcome.attempt).expect("payload");
    assert_eq!(
        payload.reference,
        r#"{"attempt_id":"attempt-1","entity":"ContextRehydrationAttempt","project_id":"project-1","v":1}"#
    );
}

#[test]
fn materialization_failure_is_durable_without_event_or_partial_context() {
    let (_tmp, mut repo) = opened("rehydration-failed");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "docs/spec.md",
            SourceClass::Mandatory,
        )],
    );
    let mut repository = RepoMaterializer {
        values: HashMap::from([("docs/spec.md".to_string(), Err("SOURCE_UNREADABLE"))]),
        calls: 0,
    };
    let mut artifacts = ArtifactFake {
        values: HashMap::new(),
        calls: 0,
    };
    let mut events = EventSupplier {
        calls: 0,
        corrupt_digest: false,
    };
    let outcome = repo
        .rehydrate_context(
            request(vec![repo_binding(0, "source-repo", "docs/spec.md")]),
            &mut repository,
            &mut artifacts,
            &mut events,
        )
        .expect("failed outcome is durably returned");

    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert_eq!(
        outcome.attempt.failure_code.as_deref(),
        Some("SOURCE_UNREADABLE")
    );
    assert!(outcome.sources.is_empty());
    assert_eq!(events.calls, 0, "failed attempts emit no event");
    assert!(
        repo.find_context_rehydration_attempt("project-1", "attempt-1")
            .expect("read")
            .is_some()
    );
    assert!(
        repo.find_event("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .expect("event read")
            .is_none()
    );
}

#[test]
fn invalid_attempt_digest_persists_neither_success_attempt_nor_event() {
    let (_tmp, mut repo) = opened("rehydration-atomic");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "docs/spec.md",
            SourceClass::Mandatory,
        )],
    );
    let mut repository = RepoMaterializer {
        values: HashMap::from([("docs/spec.md".to_string(), Ok(b"body".to_vec()))]),
        calls: 0,
    };
    let mut artifacts = ArtifactFake {
        values: HashMap::new(),
        calls: 0,
    };
    let mut events = EventSupplier {
        calls: 0,
        corrupt_digest: true,
    };
    let outcome = repo
        .rehydrate_context(
            request(vec![repo_binding(0, "source-repo", "docs/spec.md")]),
            &mut repository,
            &mut artifacts,
            &mut events,
        )
        .expect("invalid success event becomes a durable failed attempt");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert!(
        repo.find_context_rehydration_attempt("project-1", "attempt-1")
            .expect("read")
            .is_some()
    );
    assert!(
        repo.find_event("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .expect("event read")
            .is_none()
    );
}

#[test]
fn closed_state_query_is_canonical_materialized_context() {
    let (_tmp, mut repo) = opened("rehydration-query");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::StateQuery,
            "logical-role-by-id",
            SourceClass::Mandatory,
        )],
    );
    let mut repository = RepoMaterializer {
        values: HashMap::new(),
        calls: 0,
    };
    let mut artifacts = ArtifactFake {
        values: HashMap::new(),
        calls: 0,
    };
    let mut events = EventSupplier {
        calls: 0,
        corrupt_digest: false,
    };
    let outcome = repo
        .rehydrate_context(
            request(vec![ContextSourceBindingV1 {
                source_ordinal: 0,
                source_id: "source-query".to_string(),
                source_ref: BoundContextSourceRefV1::StateQuery(StateQueryRefV1::LogicalRole {
                    role_id: "role-1".to_string(),
                }),
            }]),
            &mut repository,
            &mut artifacts,
            &mut events,
        )
        .expect("query rehydration");
    let body = std::str::from_utf8(&outcome.sources[0].bytes).expect("UTF-8 JSON");
    assert!(body.contains(r#""query_id":"logical-role-by-id""#));
    assert_eq!(serde_json_canonicalizer::pipe(body).expect("parse"), body);
    assert_eq!(repository.calls, 0);
    assert_eq!(artifacts.calls, 0);
}

#[test]
fn consumed_unchanged_is_not_reread_without_trusted_touch_evidence() {
    for touched in [false, true] {
        let (_tmp, mut repo) = opened(if touched {
            "rehydration-consumed-touch"
        } else {
            "rehydration-consumed-unchanged"
        });
        let binding = artifact_binding(0, "source-consumed", "artifact-1");
        let bytes = b"unchanged artifact".to_vec();
        let digest =
            context_source_digest_v1(&binding.source_ref, "project-1", &bytes).expect("digest");
        let mut manifest_source = source(
            ContextSourceRefType::ArtifactId,
            "artifact-1",
            SourceClass::Consumed,
        );
        manifest_source.digest = digest;
        seed(&mut repo, vec![manifest_source]);
        let mut request = request(vec![binding]);
        if touched {
            request.touch_evidence.push(ContextSourceTouchEvidence {
                source_id: "source-consumed".to_string(),
                project_id: "project-1".to_string(),
                durable_role_id: "role-1".to_string(),
                context_manifest_id: "manifest-1".to_string(),
                context_epoch_id: 0,
                task_id: "task-1".to_string(),
                correlation_reference: "correlation-1".to_string(),
            });
        }
        let mut repository = RepoMaterializer {
            values: HashMap::new(),
            calls: 0,
        };
        let mut artifacts = ArtifactFake {
            values: HashMap::from([("artifact-1".to_string(), bytes)]),
            calls: 0,
        };
        let mut events = EventSupplier {
            calls: 0,
            corrupt_digest: false,
        };
        let outcome = repo
            .rehydrate_context(request, &mut repository, &mut artifacts, &mut events)
            .expect("rehydration");
        assert_eq!(artifacts.calls, 1, "CONSUMED is digest-checked");
        assert_eq!(outcome.sources.len(), usize::from(touched));
        assert_eq!(
            outcome.attempt.source_evidence[0].disposition,
            if touched {
                SourceDisposition::Reread
            } else {
                SourceDisposition::Unchanged
            }
        );
    }
}

#[test]
fn reference_is_materialized_only_with_scoped_demand() {
    let (_tmp, mut repo) = opened("rehydration-reference-demand");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::ArtifactId,
            "artifact-1",
            SourceClass::Reference,
        )],
    );
    let mut request = request(vec![artifact_binding(0, "source-reference", "artifact-1")]);
    request.demands.push(ContextSourceDemand {
        source_id: "source-reference".to_string(),
        project_id: "project-1".to_string(),
        durable_role_id: "role-1".to_string(),
        context_manifest_id: "manifest-1".to_string(),
        context_epoch_id: 0,
        task_id: Some("task-1".to_string()),
        correlation_reference: "correlation-1".to_string(),
    });
    let mut repository = RepoMaterializer {
        values: HashMap::new(),
        calls: 0,
    };
    let mut artifacts = ArtifactFake {
        values: HashMap::from([("artifact-1".to_string(), b"reference body".to_vec())]),
        calls: 0,
    };
    let mut events = EventSupplier {
        calls: 0,
        corrupt_digest: false,
    };
    let outcome = repo
        .rehydrate_context(request, &mut repository, &mut artifacts, &mut events)
        .expect("demanded reference");
    assert_eq!(artifacts.calls, 1);
    assert_eq!(outcome.sources.len(), 1);
    assert_eq!(
        outcome.attempt.source_evidence[0].disposition,
        SourceDisposition::Reread
    );
}

#[test]
fn migration_v9_is_registered_with_immutable_attempt_tables() {
    let (_tmp, repo) = opened("rehydration-schema");
    assert_eq!(repo.schema_version().expect("version"), 10);
    for table in [
        "context_rehydration_attempt",
        "context_rehydration_repository_snapshot",
        "context_rehydration_source_evidence",
    ] {
        assert!(repo.table_exists(table).expect("table check"), "{table}");
    }
}

#[test]
fn repository_paths_are_platform_independently_fail_closed_before_materialization() {
    for (index, path) in [
        "../secret",
        "..\\secret",
        "a/../../secret",
        "a\\..\\..\\secret",
        "/foo",
        "\\foo",
        "C:\\secret",
        "C:/secret",
        "C:secret",
        "//server/share",
        "\\\\server\\share",
        "bad\0path",
    ]
    .into_iter()
    .enumerate()
    {
        let (_tmp, mut repo) = opened(&format!("unsafe-path-{index}"));
        seed(
            &mut repo,
            vec![source(
                ContextSourceRefType::RepoPath,
                path,
                SourceClass::Mandatory,
            )],
        );
        let mut materializer = RepoMaterializer {
            values: HashMap::new(),
            calls: 0,
        };
        let outcome = repo
            .rehydrate_context(
                request(vec![repo_binding(0, "source-1", path)]),
                &mut materializer,
                &mut ArtifactFake {
                    values: HashMap::new(),
                    calls: 0,
                },
                &mut EventSupplier {
                    calls: 0,
                    corrupt_digest: false,
                },
            )
            .expect("bounded failed attempt persists");
        assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
        assert_eq!(materializer.calls, 0, "unsafe path {path:?} reached port");
    }

    let oversized = "a".repeat(1_025);
    let (_tmp, mut repo) = opened("unsafe-path-oversized");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            &oversized,
            SourceClass::Mandatory,
        )],
    );
    let mut materializer = RepoMaterializer {
        values: HashMap::new(),
        calls: 0,
    };
    let outcome = repo
        .rehydrate_context(
            request(vec![repo_binding(0, "source-1", &oversized)]),
            &mut materializer,
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("bounded failure");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert_eq!(materializer.calls, 0);
}

#[test]
fn malformed_state_query_parameters_fail_before_registry_execution() {
    let (_tmp, mut repo) = opened("bad-state-query");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::StateQuery,
            "logical-role-by-id",
            SourceClass::Mandatory,
        )],
    );
    let binding = ContextSourceBindingV1 {
        source_ordinal: 0,
        source_id: "query-1".to_string(),
        source_ref: BoundContextSourceRefV1::StateQuery(StateQueryRefV1::LogicalRole {
            role_id: String::new(),
        }),
    };
    let outcome = repo
        .rehydrate_context(
            request(vec![binding]),
            &mut RepoMaterializer {
                values: HashMap::new(),
                calls: 0,
            },
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("failed attempt persists");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert_eq!(
        outcome.attempt.failure_code.as_deref(),
        Some("REHYDRATION_REQUEST_INVALID")
    );
}

#[test]
fn negative_state_query_epoch_fails_before_execution_or_materialization() {
    let (_tmp, mut repo) = opened("negative-state-query-epoch");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::StateQuery,
            "context-epoch-by-id",
            SourceClass::Mandatory,
        )],
    );
    let binding = ContextSourceBindingV1 {
        source_ordinal: 0,
        source_id: "query-1".to_string(),
        source_ref: BoundContextSourceRefV1::StateQuery(StateQueryRefV1::ContextEpoch {
            project_id: "project-1".to_string(),
            epoch: -1,
        }),
    };
    let mut repository = RepoMaterializer {
        values: HashMap::new(),
        calls: 0,
    };
    let mut artifacts = ArtifactFake {
        values: HashMap::new(),
        calls: 0,
    };
    let outcome = repo
        .rehydrate_context(
            request(vec![binding]),
            &mut repository,
            &mut artifacts,
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("bounded failure");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert!(outcome.attempt.source_evidence.is_empty());
    assert_eq!(repository.calls, 0);
    assert_eq!(artifacts.calls, 0);
}

#[test]
fn invalid_expected_digest_persists_bounded_failure_before_materialization() {
    let (_tmp, mut repo) = opened("invalid-expected-digest");
    let mut invalid = source(
        ContextSourceRefType::RepoPath,
        "safe/path",
        SourceClass::Mandatory,
    );
    invalid.digest = format!("sha256:v1:{}", "A".repeat(64));
    seed(&mut repo, vec![invalid]);
    let mut materializer = RepoMaterializer {
        values: HashMap::from([("safe/path".to_string(), Ok(b"body".to_vec()))]),
        calls: 0,
    };
    let outcome = repo
        .rehydrate_context(
            request(vec![repo_binding(0, "source-1", "safe/path")]),
            &mut materializer,
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("bounded failed attempt");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert_eq!(
        outcome.attempt.failure_code.as_deref(),
        Some("INVALID_SOURCE_DIGEST")
    );
    assert!(outcome.attempt.source_evidence.is_empty());
    assert_eq!(materializer.calls, 0);
}

#[test]
fn executor_session_mismatch_fails_before_materialization() {
    let (_tmp, mut repo) = opened("session-mismatch");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "safe/path",
            SourceClass::Mandatory,
        )],
    );
    repo.create_executor_binding(ExecutorBinding {
        binding_id: "binding-1".to_string(),
        role_id: "role-1".to_string(),
        provider_id: "provider".to_string(),
        model_id: "model".to_string(),
        runtime_id: "runtime".to_string(),
        session_ref: Some("session-exact".to_string()),
        routing_decision_id: None,
        bound_at: "bound".to_string(),
        lease_expires_at: "lease".to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    })
    .expect("binding");
    let mut candidate = request(vec![repo_binding(0, "source-1", "safe/path")]);
    candidate.executor_binding_id = Some("binding-1".to_string());
    candidate.session_reference = Some("wrong-session".to_string());
    let mut materializer = RepoMaterializer {
        values: HashMap::from([("safe/path".to_string(), Ok(b"body".to_vec()))]),
        calls: 0,
    };
    let outcome = repo
        .rehydrate_context(
            candidate,
            &mut materializer,
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("failed attempt persists");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert_eq!(materializer.calls, 0);
}

#[test]
fn executor_session_presence_truth_table_is_byte_exact() {
    let cases = [
        ("none-none", None, None, true),
        ("some-equal", Some("session-a"), Some("session-a"), true),
        (
            "some-different",
            Some("session-a"),
            Some("session-b"),
            false,
        ),
        ("some-none", Some("session-a"), None, false),
        ("none-some", None, Some("session-b"), false),
        ("space-exact", Some("session-a "), Some("session-a"), false),
        ("case-exact", Some("Session-A"), Some("session-a"), false),
        ("unicode-exact", Some("é"), Some("e\u{301}"), false),
    ];
    for (tag, persisted, requested, accepted) in cases {
        let (_tmp, mut repo) = opened(tag);
        seed(
            &mut repo,
            vec![source(
                ContextSourceRefType::RepoPath,
                "safe/path",
                SourceClass::Mandatory,
            )],
        );
        repo.create_executor_binding(ExecutorBinding {
            binding_id: "binding-1".to_string(),
            role_id: "role-1".to_string(),
            provider_id: "provider".to_string(),
            model_id: "model".to_string(),
            runtime_id: "runtime".to_string(),
            session_ref: persisted.map(str::to_string),
            routing_decision_id: None,
            bound_at: "bound".to_string(),
            lease_expires_at: "lease".to_string(),
            released_at: None,
            release_reason: None,
            rehydration_completed: None,
        })
        .expect("binding");
        let mut candidate = request(vec![repo_binding(0, "source-1", "safe/path")]);
        candidate.executor_binding_id = Some("binding-1".to_string());
        candidate.session_reference = requested.map(str::to_string);
        let mut materializer = RepoMaterializer {
            values: HashMap::from([("safe/path".to_string(), Ok(b"body".to_vec()))]),
            calls: 0,
        };
        let outcome = repo
            .rehydrate_context(
                candidate,
                &mut materializer,
                &mut ArtifactFake {
                    values: HashMap::new(),
                    calls: 0,
                },
                &mut EventSupplier {
                    calls: 0,
                    corrupt_digest: false,
                },
            )
            .expect("terminal receipt");
        assert_eq!(
            outcome.attempt.status,
            if accepted {
                ContextRehydrationStatus::Succeeded
            } else {
                ContextRehydrationStatus::Failed
            }
        );
        assert_eq!(materializer.calls, usize::from(accepted), "case {tag}");
    }
}

struct EpochAdvancingMaterializer {
    db_path: std::path::PathBuf,
    advanced: bool,
}

impl RepositorySnapshotMaterializer for EpochAdvancingMaterializer {
    fn known_length(
        &mut self,
        _reference: &RepositorySnapshotRefV1,
    ) -> Result<Option<u64>, SourceMaterializationFailure> {
        Ok(Some(4))
    }

    fn materialize(
        &mut self,
        _reference: &RepositorySnapshotRefV1,
        accept_chunk: &mut dyn FnMut(&[u8]) -> Result<(), SourceMaterializationFailure>,
    ) -> Result<ExternalSourceMaterialization, SourceMaterializationFailure> {
        accept_chunk(b"body")?;
        if !self.advanced {
            let mut other = SqliteStateRepository::open(&self.db_path).expect("concurrent writer");
            other
                .append_context_epoch(ContextEpoch {
                    project_id: "project-1".to_string(),
                    epoch: 1,
                    advanced_at: "later".to_string(),
                    trigger: ContextEpochTrigger::ContractChange,
                })
                .expect("advance epoch");
            self.advanced = true;
        }
        Ok(ExternalSourceMaterialization {
            materializer_id: "race-materializer".to_string(),
            provenance: "snapshot".to_string(),
            materialized_at: "2026-08-17T10:00:01Z".to_string(),
        })
    }
}

#[test]
fn terminal_transaction_fences_stale_epoch_and_persists_failed_only() {
    let (tmp, mut repo) = opened("stale-epoch-race");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "safe/path",
            SourceClass::Mandatory,
        )],
    );
    let mut events = EventSupplier {
        calls: 0,
        corrupt_digest: false,
    };
    let outcome = repo
        .rehydrate_context(
            request(vec![repo_binding(0, "source-1", "safe/path")]),
            &mut EpochAdvancingMaterializer {
                db_path: tmp.db_path(),
                advanced: false,
            },
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut events,
        )
        .expect("bounded stale failure persists");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert_eq!(
        outcome.attempt.failure_code.as_deref(),
        Some("STALE_CONTEXT_EPOCH")
    );
    assert!(
        repo.find_event("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .expect("read")
            .is_none()
    );
}

#[test]
fn historical_role_and_manifest_epoch_snapshots_do_not_block_current_epoch() {
    let (_tmp, mut repo) = opened("historical-owner-epoch");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "safe/path",
            SourceClass::Mandatory,
        )],
    );
    repo.append_context_epoch(ContextEpoch {
        project_id: "project-1".to_string(),
        epoch: 1,
        advanced_at: "later".to_string(),
        trigger: ContextEpochTrigger::ArchitectureChange,
    })
    .expect("epoch 1");
    let mut candidate = request(vec![repo_binding(0, "source-1", "safe/path")]);
    candidate.context_epoch_id = 1;
    let outcome = repo
        .rehydrate_context(
            candidate,
            &mut RepoMaterializer {
                values: HashMap::from([("safe/path".to_string(), Ok(b"body".to_vec()))]),
                calls: 0,
            },
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("rehydrate current epoch");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Succeeded);
}

#[test]
fn provider_actor_round_trips_and_scheduler_is_rejected_by_v9() {
    let (_tmp, mut repo) = opened("provider-actor");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "safe/path",
            SourceClass::Mandatory,
        )],
    );
    let mut candidate = request(vec![repo_binding(0, "source-1", "safe/path")]);
    candidate.requested_by_actor = EventActor {
        kind: ActorKind::Provider,
        id: Some("provider-1".to_string()),
    };
    let outcome = repo
        .rehydrate_context(
            candidate,
            &mut RepoMaterializer {
                values: HashMap::from([("safe/path".to_string(), Ok(b"body".to_vec()))]),
                calls: 0,
            },
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("provider succeeds");
    assert_eq!(outcome.attempt.requested_by_actor.kind, ActorKind::Provider);
    assert_eq!(
        repo.find_context_rehydration_attempt("project-1", "attempt-1")
            .expect("read")
            .expect("attempt")
            .requested_by_actor
            .kind,
        ActorKind::Provider
    );

    let error = repo.connection().execute(
        "UPDATE context_rehydration_attempt SET requested_by_actor_kind = 'SCHEDULER' WHERE project_id = 'project-1' AND rehydration_attempt_id = 'attempt-1'",
        [],
    );
    assert!(error.is_err(), "v9 CHECK must reject SCHEDULER");
}

#[test]
fn source_digest_is_exact_and_domain_separated() {
    let source = BoundContextSourceRefV1::Artifact(ArtifactRefV1 {
        project_id: "project-1".to_string(),
        artifact_id: "artifact-1".to_string(),
    });
    let first = context_source_digest_v1(&source, "project-1", b"ab").expect("digest");
    let second = context_source_digest_v1(&source, "project-1", b"a").expect("digest");
    assert_eq!(first.len(), 74);
    assert!(first.starts_with("sha256:v1:"));
    assert!(
        first[10..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    assert_ne!(first, second);
    let changed_identity = BoundContextSourceRefV1::Artifact(ArtifactRefV1 {
        project_id: "project-1".to_string(),
        artifact_id: "artifact-2".to_string(),
    });
    assert_ne!(
        first,
        context_source_digest_v1(&changed_identity, "project-1", b"ab").expect("digest")
    );
}

#[test]
fn known_oversized_source_is_rejected_without_body_read() {
    let (_tmp, mut repo) = opened("known-oversized");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "large",
            SourceClass::Mandatory,
        )],
    );
    let mut materializer = RepoMaterializer {
        values: HashMap::from([("large".to_string(), Ok(vec![0; 8_388_609]))]),
        calls: 0,
    };
    let outcome = repo
        .rehydrate_context(
            request(vec![repo_binding(0, "source-1", "large")]),
            &mut materializer,
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("failed attempt");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert_eq!(materializer.calls, 0);
    assert_eq!(
        outcome.attempt.failure_code.as_deref(),
        Some("SOURCE_MATERIALIZATION_SIZE_LIMIT_EXCEEDED")
    );
}

#[test]
fn failure_detail_is_utf8_safe_redacted_and_only_field_truncated() {
    let detail = format!("password=do-not-store\n{}🙂", "x".repeat(5_000));
    let sanitized = crate::context_rehydration::sanitize_failure_detail(&detail);
    assert!(sanitized.len() <= 4_096);
    assert!(!sanitized.contains("do-not-store"));
    assert!(sanitized.ends_with("...[TRUNCATED]"));
    assert!(std::str::from_utf8(sanitized.as_bytes()).is_ok());
}

struct NoncanonicalEventSupplier;

impl ContextRehydratedEventSupplier for NoncanonicalEventSupplier {
    fn event_for(
        &mut self,
        attempt: &crate::context_rehydration::ContextRehydrationAttempt,
    ) -> EventEnvelope {
        let mut event = EventSupplier {
            calls: 0,
            corrupt_digest: false,
        }
        .event_for(attempt);
        event.payload.reference.insert(0, ' ');
        event
    }
}

#[test]
fn noncanonical_attempt_reference_produces_failed_attempt_and_no_event() {
    let (_tmp, mut repo) = opened("noncanonical-event-reference");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "safe/path",
            SourceClass::Mandatory,
        )],
    );
    let outcome = repo
        .rehydrate_context(
            request(vec![repo_binding(0, "source-1", "safe/path")]),
            &mut RepoMaterializer {
                values: HashMap::from([("safe/path".to_string(), Ok(b"body".to_vec()))]),
                calls: 0,
            },
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut NoncanonicalEventSupplier,
        )
        .expect("failed attempt persists");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert!(
        repo.find_event("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .expect("read")
            .is_none()
    );
}

#[test]
fn source_and_snapshot_cardinality_overflow_stops_before_materialization() {
    let sources = (0..257)
        .map(|index| {
            source(
                ContextSourceRefType::ArtifactId,
                &format!("artifact-{index}"),
                SourceClass::Mandatory,
            )
        })
        .collect();
    let bindings = (0..257)
        .map(|index| {
            artifact_binding(
                index,
                &format!("source-{index}"),
                &format!("artifact-{index}"),
            )
        })
        .collect();
    let (_tmp, mut repo) = opened("source-count-257");
    seed(&mut repo, sources);
    let mut artifacts = ArtifactFake {
        values: HashMap::new(),
        calls: 0,
    };
    let outcome = repo
        .rehydrate_context(
            request(bindings),
            &mut RepoMaterializer {
                values: HashMap::new(),
                calls: 0,
            },
            &mut artifacts,
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("bounded source-count failure");
    assert_eq!(
        outcome.attempt.failure_code.as_deref(),
        Some("REHYDRATION_SOURCE_COUNT_LIMIT_EXCEEDED")
    );
    assert_eq!(artifacts.calls, 0);

    let sources = (0..65)
        .map(|index| {
            source(
                ContextSourceRefType::RepoPath,
                &format!("path-{index}"),
                SourceClass::Mandatory,
            )
        })
        .collect();
    let bindings = (0..65)
        .map(|index| repo_binding(index, &format!("source-{index}"), &format!("path-{index}")))
        .collect();
    let (_tmp, mut repo) = opened("snapshot-count-65");
    seed(&mut repo, sources);
    let mut materializer = RepoMaterializer {
        values: HashMap::new(),
        calls: 0,
    };
    let outcome = repo
        .rehydrate_context(
            request(bindings),
            &mut materializer,
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("bounded snapshot-count failure");
    assert_eq!(
        outcome.attempt.failure_code.as_deref(),
        Some("REPOSITORY_SNAPSHOT_REFERENCE_LIMIT_EXCEEDED")
    );
    assert_eq!(materializer.calls, 0);
}

#[test]
fn exact_source_and_snapshot_cardinality_boundaries_are_accepted() {
    let sources = (0..256)
        .map(|index| {
            source(
                ContextSourceRefType::ArtifactId,
                &format!("artifact-{index}"),
                SourceClass::Mandatory,
            )
        })
        .collect();
    let bindings = (0..256)
        .map(|index| {
            artifact_binding(
                index,
                &format!("source-{index}"),
                &format!("artifact-{index}"),
            )
        })
        .collect();
    let values = (0..256)
        .map(|index| (format!("artifact-{index}"), vec![b'x']))
        .collect();
    let (_tmp, mut repo) = opened("source-count-256");
    seed(&mut repo, sources);
    let outcome = repo
        .rehydrate_context(
            request(bindings),
            &mut RepoMaterializer {
                values: HashMap::new(),
                calls: 0,
            },
            &mut ArtifactFake { values, calls: 0 },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("256 sources accepted");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Succeeded);
    let original_payload = context_rehydration_event_payload(&outcome.attempt).expect("payload");
    let mut permuted = outcome.attempt.clone();
    permuted.source_evidence.reverse();
    assert_eq!(
        context_rehydration_event_payload(&permuted).expect("permuted payload"),
        original_payload,
        "source evidence input order cannot affect the digest"
    );

    let sources = (0..64)
        .map(|index| {
            source(
                ContextSourceRefType::RepoPath,
                &format!("path-{index}"),
                SourceClass::Mandatory,
            )
        })
        .collect();
    let bindings = (0..64)
        .map(|index| repo_binding(index, &format!("source-{index}"), &format!("path-{index}")))
        .collect();
    let values = (0..64)
        .map(|index| (format!("path-{index}"), Ok(vec![b'x'])))
        .collect();
    let (_tmp, mut repo) = opened("snapshot-count-64");
    seed(&mut repo, sources);
    let outcome = repo
        .rehydrate_context(
            request(bindings),
            &mut RepoMaterializer { values, calls: 0 },
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("64 snapshots accepted");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Succeeded);
    let original_payload = context_rehydration_event_payload(&outcome.attempt).expect("payload");
    let mut permuted = outcome.attempt.clone();
    permuted.repository_snapshot_references.reverse();
    assert_eq!(
        context_rehydration_event_payload(&permuted).expect("permuted payload"),
        original_payload,
        "snapshot input order cannot affect the digest"
    );
}

#[test]
fn mandatory_unchanged_and_trusted_consumed_touch_both_genuinely_reread() {
    for (tag, class, touched) in [
        ("mandatory-unchanged", SourceClass::Mandatory, false),
        ("consumed-touched", SourceClass::Consumed, true),
    ] {
        let source_ref = BoundContextSourceRefV1::RepositorySnapshot(RepositorySnapshotRefV1 {
            project_id: "project-1".to_string(),
            repository_id: "repo-1".to_string(),
            commit_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string(),
            logical_relative_path: "safe/path".to_string(),
        });
        let digest = context_source_digest_v1(&source_ref, "project-1", b"same").expect("digest");
        let mut manifest_source = source(ContextSourceRefType::RepoPath, "safe/path", class);
        manifest_source.digest = digest;
        let (_tmp, mut repo) = opened(tag);
        seed(&mut repo, vec![manifest_source]);
        let mut candidate = request(vec![repo_binding(0, "source-1", "safe/path")]);
        if touched {
            candidate.touch_evidence.push(ContextSourceTouchEvidence {
                source_id: "source-1".to_string(),
                project_id: "project-1".to_string(),
                durable_role_id: "role-1".to_string(),
                context_manifest_id: "manifest-1".to_string(),
                context_epoch_id: 0,
                task_id: "task-1".to_string(),
                correlation_reference: "correlation-1".to_string(),
            });
        }
        let outcome = repo
            .rehydrate_context(
                candidate,
                &mut RepoMaterializer {
                    values: HashMap::from([("safe/path".to_string(), Ok(b"same".to_vec()))]),
                    calls: 0,
                },
                &mut ArtifactFake {
                    values: HashMap::new(),
                    calls: 0,
                },
                &mut EventSupplier {
                    calls: 0,
                    corrupt_digest: false,
                },
            )
            .expect("rehydration");
        assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Succeeded);
        assert_eq!(outcome.sources.len(), 1);
        assert_eq!(outcome.sources[0].bytes, b"same");
        assert_eq!(
            outcome.attempt.source_evidence[0].disposition,
            SourceDisposition::Reread
        );
    }
}

#[test]
fn event_insert_failure_rolls_back_candidate_success_before_failed_receipt() {
    let (_tmp, mut repo) = opened("event-insert-rollback");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "safe/path",
            SourceClass::Mandatory,
        )],
    );
    repo.append_event(EventEnvelope {
        event_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
        project_id: "project-1".to_string(),
        goal_id: None,
        event_type: EventType::GoalCreated,
        actor: EventActor {
            kind: ActorKind::System,
            id: None,
        },
        subject: EventSubject {
            kind: SubjectKind::Role,
            id: "role-1".to_string(),
        },
        occurred_at: "before".to_string(),
        payload: crate::event::EventPayloadReference {
            reference: "existing".to_string(),
            digest: "existing-digest".to_string(),
        },
        correlation_id: "existing-correlation".to_string(),
        epoch: 0,
    })
    .expect("existing event");
    let outcome = repo
        .rehydrate_context(
            request(vec![repo_binding(0, "source-1", "safe/path")]),
            &mut RepoMaterializer {
                values: HashMap::from([("safe/path".to_string(), Ok(b"body".to_vec()))]),
                calls: 0,
            },
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("failed receipt persists");
    assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
    assert_eq!(
        repo.find_context_rehydration_attempt("project-1", "attempt-1")
            .expect("read")
            .expect("attempt")
            .status,
        ContextRehydrationStatus::Failed
    );
    assert_eq!(
        repo.find_event("01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .expect("event")
            .expect("existing")
            .event_type,
        EventType::GoalCreated
    );
}

#[test]
fn second_pass_failures_are_durable_and_never_return_partial_context() {
    for (tag, with_earlier_source, fail_on_call) in [
        ("reread-failure", false, 2),
        ("reread-failure-after-source", true, 3),
    ] {
        let (_tmp, mut repo) = opened(tag);
        let mut sources = Vec::new();
        let mut bindings = Vec::new();
        if with_earlier_source {
            sources.push(source(
                ContextSourceRefType::RepoPath,
                "size/4",
                SourceClass::Mandatory,
            ));
            bindings.push(repo_binding(0, "source-earlier", "size/4"));
        }
        sources.push(source(
            ContextSourceRefType::RepoPath,
            "size/4",
            SourceClass::Consumed,
        ));
        bindings.push(repo_binding(
            usize::from(with_earlier_source),
            "source-consumed",
            "size/4",
        ));
        seed(&mut repo, sources);
        let mut materializer = StreamingRepoMaterializer::new(true, MAX_STREAM_CHUNK_BYTES);
        materializer.fail_on_call = Some(fail_on_call);
        let mut events = EventSupplier {
            calls: 0,
            corrupt_digest: false,
        };
        let outcome = repo
            .rehydrate_context(
                request(bindings),
                &mut materializer,
                &mut ArtifactFake {
                    values: HashMap::new(),
                    calls: 0,
                },
                &mut events,
            )
            .expect("failed receipt persists");

        assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
        assert_eq!(
            outcome.attempt.failure_code.as_deref(),
            Some("SOURCE_REREAD_FAILED")
        );
        assert!(outcome.sources.is_empty());
        assert_eq!(events.calls, 0);
        assert_eq!(
            repo.find_context_rehydration_attempt("project-1", "attempt-1")
                .expect("read")
                .expect("durable failure")
                .status,
            ContextRehydrationStatus::Failed
        );
        assert!(
            repo.find_event("01ARZ3NDEKTSV4RRFFQ69G5FAV")
                .expect("event read")
                .is_none()
        );
    }
}

#[test]
fn failed_receipt_write_failure_returns_err() {
    let (_tmp, mut repo) = opened("failed-receipt-write-failure");
    seed(
        &mut repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "safe/path",
            SourceClass::Mandatory,
        )],
    );
    repo.rehydrate_context(
        request(vec![repo_binding(0, "source-1", "safe/path")]),
        &mut RepoMaterializer {
            values: HashMap::from([("safe/path".to_string(), Ok(b"body".to_vec()))]),
            calls: 0,
        },
        &mut ArtifactFake {
            values: HashMap::new(),
            calls: 0,
        },
        &mut EventSupplier {
            calls: 0,
            corrupt_digest: false,
        },
    )
    .expect("first terminal receipt");

    let mut invalid = request(vec![repo_binding(0, "source-1", "safe/path")]);
    invalid.session_reference = Some("x".repeat(MAX_SESSION_REFERENCE_BYTES + 1));
    let result = repo.rehydrate_context(
        invalid,
        &mut RepoMaterializer {
            values: HashMap::new(),
            calls: 0,
        },
        &mut ArtifactFake {
            values: HashMap::new(),
            calls: 0,
        },
        &mut EventSupplier {
            calls: 0,
            corrupt_digest: false,
        },
    );
    assert!(
        result.is_err(),
        "failed receipt collision cannot become success"
    );
}

#[test]
fn every_unknown_length_pass_counts_toward_the_attempt_boundary() {
    for (tag, source_lengths, expected_status, expected_calls) in [
        (
            "aggregate-exact",
            vec![MAX_MATERIALIZED_BYTES_PER_SOURCE; 2],
            ContextRehydrationStatus::Succeeded,
            4,
        ),
        (
            "aggregate-plus-one",
            vec![
                MAX_MATERIALIZED_BYTES_PER_SOURCE,
                MAX_MATERIALIZED_BYTES_PER_SOURCE,
                1,
            ],
            ContextRehydrationStatus::Failed,
            5,
        ),
    ] {
        let (_tmp, mut repo) = opened(tag);
        let sources = source_lengths
            .iter()
            .map(|length| {
                source(
                    ContextSourceRefType::RepoPath,
                    &format!("size/{length}"),
                    SourceClass::Mandatory,
                )
            })
            .collect();
        let bindings = source_lengths
            .iter()
            .enumerate()
            .map(|(ordinal, length)| {
                repo_binding(
                    ordinal,
                    &format!("source-{ordinal}"),
                    &format!("size/{length}"),
                )
            })
            .collect();
        seed(&mut repo, sources);
        let mut materializer = StreamingRepoMaterializer::new(false, MAX_STREAM_CHUNK_BYTES);
        let mut events = EventSupplier {
            calls: 0,
            corrupt_digest: false,
        };
        let outcome = repo
            .rehydrate_context(
                request(bindings),
                &mut materializer,
                &mut ArtifactFake {
                    values: HashMap::new(),
                    calls: 0,
                },
                &mut events,
            )
            .expect("terminal outcome");
        assert_eq!(outcome.attempt.status, expected_status);
        assert_eq!(materializer.calls, expected_calls);
        assert_eq!(materializer.known_calls, source_lengths.len());
        if expected_status == ContextRehydrationStatus::Failed {
            assert_eq!(
                outcome.attempt.failure_code.as_deref(),
                Some("REHYDRATION_MATERIALIZATION_TOTAL_LIMIT_EXCEEDED")
            );
            assert!(outcome.sources.is_empty());
            assert_eq!(events.calls, 0);
        } else {
            assert_eq!(
                source_lengths.iter().sum::<u64>() * 2,
                MAX_MATERIALIZED_BYTES_PER_ATTEMPT
            );
        }
    }
}

#[test]
fn consumed_reread_passes_count_toward_the_attempt_boundary() {
    for (tag, lengths, expected_status) in [
        (
            "reread-aggregate-exact",
            vec![MAX_MATERIALIZED_BYTES_PER_SOURCE; 2],
            ContextRehydrationStatus::Succeeded,
        ),
        (
            "reread-aggregate-plus-one",
            vec![
                MAX_MATERIALIZED_BYTES_PER_SOURCE,
                MAX_MATERIALIZED_BYTES_PER_SOURCE,
                1,
            ],
            ContextRehydrationStatus::Failed,
        ),
    ] {
        let (_tmp, mut repo) = opened(tag);
        seed(
            &mut repo,
            lengths
                .iter()
                .map(|length| {
                    source(
                        ContextSourceRefType::RepoPath,
                        &format!("size/{length}"),
                        SourceClass::Consumed,
                    )
                })
                .collect(),
        );
        let bindings = lengths
            .iter()
            .enumerate()
            .map(|(ordinal, length)| {
                repo_binding(
                    ordinal,
                    &format!("source-{ordinal}"),
                    &format!("size/{length}"),
                )
            })
            .collect();
        let mut materializer = StreamingRepoMaterializer::new(true, MAX_STREAM_CHUNK_BYTES);
        let mut events = EventSupplier {
            calls: 0,
            corrupt_digest: false,
        };
        let outcome = repo
            .rehydrate_context(
                request(bindings),
                &mut materializer,
                &mut ArtifactFake {
                    values: HashMap::new(),
                    calls: 0,
                },
                &mut events,
            )
            .expect("terminal outcome");
        assert_eq!(outcome.attempt.status, expected_status);
        assert_eq!(materializer.calls, 4);
        if expected_status == ContextRehydrationStatus::Succeeded {
            assert_eq!(outcome.sources.len(), 2);
        } else {
            assert_eq!(
                outcome.attempt.failure_code.as_deref(),
                Some("REHYDRATION_MATERIALIZATION_TOTAL_LIMIT_EXCEEDED")
            );
            assert!(outcome.sources.is_empty());
            assert_eq!(events.calls, 0);
        }
    }
}

#[test]
fn stream_chunk_and_unknown_length_source_boundaries_are_behavioral() {
    for (tag, known, source_length, chunk_size, expected_status, expected_code) in [
        (
            "chunk-exact",
            true,
            MAX_STREAM_CHUNK_BYTES as u64,
            MAX_STREAM_CHUNK_BYTES,
            ContextRehydrationStatus::Succeeded,
            None,
        ),
        (
            "chunk-plus-one",
            true,
            (MAX_STREAM_CHUNK_BYTES + 1) as u64,
            MAX_STREAM_CHUNK_BYTES + 1,
            ContextRehydrationStatus::Failed,
            Some("SOURCE_STREAM_CHUNK_SIZE_LIMIT_EXCEEDED"),
        ),
        (
            "unknown-source-exact",
            false,
            MAX_MATERIALIZED_BYTES_PER_SOURCE,
            MAX_STREAM_CHUNK_BYTES,
            ContextRehydrationStatus::Succeeded,
            None,
        ),
        (
            "unknown-source-plus-one",
            false,
            MAX_MATERIALIZED_BYTES_PER_SOURCE + 1,
            MAX_STREAM_CHUNK_BYTES,
            ContextRehydrationStatus::Failed,
            Some("SOURCE_MATERIALIZATION_SIZE_LIMIT_EXCEEDED"),
        ),
    ] {
        let path = format!("size/{source_length}");
        let (_tmp, mut repo) = opened(tag);
        seed(
            &mut repo,
            vec![source(
                ContextSourceRefType::RepoPath,
                &path,
                SourceClass::Mandatory,
            )],
        );
        let mut materializer = StreamingRepoMaterializer::new(known, chunk_size);
        let outcome = repo
            .rehydrate_context(
                request(vec![repo_binding(0, "source-1", &path)]),
                &mut materializer,
                &mut ArtifactFake {
                    values: HashMap::new(),
                    calls: 0,
                },
                &mut EventSupplier {
                    calls: 0,
                    corrupt_digest: false,
                },
            )
            .expect("bounded outcome");
        assert_eq!(outcome.attempt.status, expected_status);
        assert_eq!(outcome.attempt.failure_code.as_deref(), expected_code);
        assert_eq!(materializer.known_calls, 1);
        assert!(
            outcome.sources.is_empty() || expected_status == ContextRehydrationStatus::Succeeded
        );
    }
}

#[test]
fn unknown_length_pass_instability_fails_closed() {
    for (tag, lengths, fills) in [
        ("unstable-length", vec![4, 5], vec![b'x', b'x']),
        ("unstable-bytes", vec![4, 4], vec![b'x', b'y']),
    ] {
        let (_tmp, mut repo) = opened(tag);
        seed(
            &mut repo,
            vec![source(
                ContextSourceRefType::RepoPath,
                "size/4",
                SourceClass::Mandatory,
            )],
        );
        let mut materializer = StreamingRepoMaterializer::new(false, MAX_STREAM_CHUNK_BYTES);
        materializer.lengths_by_call = lengths;
        materializer.fills_by_call = fills;
        let mut events = EventSupplier {
            calls: 0,
            corrupt_digest: false,
        };
        let outcome = repo
            .rehydrate_context(
                request(vec![repo_binding(0, "source-1", "size/4")]),
                &mut materializer,
                &mut ArtifactFake {
                    values: HashMap::new(),
                    calls: 0,
                },
                &mut events,
            )
            .expect("durable failure");
        assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Failed);
        assert_eq!(
            outcome.attempt.failure_code.as_deref(),
            Some("INVALID_MATERIALIZER_RESPONSE")
        );
        assert!(outcome.sources.is_empty());
        assert_eq!(events.calls, 0);
    }
}

#[test]
fn request_external_fields_accept_exact_bounds_and_reject_the_next_byte() {
    let fields = [
        ("actor", MAX_ACTOR_REFERENCE_BYTES),
        ("timestamp", MAX_TIMESTAMP_BYTES),
        ("session", MAX_SESSION_REFERENCE_BYTES),
        ("task", MAX_TASK_REFERENCE_BYTES),
        ("correlation", MAX_CORRELATION_REFERENCE_BYTES),
        ("trigger", MAX_TRIGGER_REFERENCE_BYTES),
    ];
    for (field, maximum) in fields {
        for extra in 0..=1 {
            let (_tmp, mut repo) = opened(&format!("field-{field}-{extra}"));
            seed(
                &mut repo,
                vec![source(
                    ContextSourceRefType::RepoPath,
                    "safe/path",
                    SourceClass::Mandatory,
                )],
            );
            let value = "x".repeat(maximum + extra);
            let mut candidate = request(vec![repo_binding(0, "source-1", "safe/path")]);
            match field {
                "actor" => candidate.requested_by_actor.id = Some(value),
                "timestamp" => candidate.completed_at = value,
                "session" => candidate.session_reference = Some(value),
                "task" => candidate.task_id = Some(value),
                "correlation" => candidate.correlation_reference = Some(value),
                "trigger" => candidate.trigger_reference = Some(value),
                _ => unreachable!(),
            }
            let mut materializer = RepoMaterializer {
                values: HashMap::from([("safe/path".to_string(), Ok(b"body".to_vec()))]),
                calls: 0,
            };
            let result = repo.rehydrate_context(
                candidate,
                &mut materializer,
                &mut ArtifactFake {
                    values: HashMap::new(),
                    calls: 0,
                },
                &mut EventSupplier {
                    calls: 0,
                    corrupt_digest: false,
                },
            );
            if extra == 0 {
                assert_eq!(
                    result.expect("exact boundary accepted").attempt.status,
                    ContextRehydrationStatus::Succeeded,
                    "field {field}"
                );
            } else {
                assert!(result.is_err(), "field {field} boundary+1 accepted");
            }
            assert_eq!(materializer.calls, usize::from(extra == 0));
        }
    }
}

#[test]
fn materializer_metadata_and_failure_text_boundaries_are_behavioral() {
    for (field, maximum) in [
        ("materializer_id", MAX_AUTHORITATIVE_BINDING_CANONICAL_BYTES),
        ("provenance", MAX_MATERIALIZER_PROVENANCE_CANONICAL_BYTES),
    ] {
        for extra in 0..=1 {
            let (_tmp, mut repo) = opened(&format!("metadata-{field}-{extra}"));
            seed(
                &mut repo,
                vec![source(
                    ContextSourceRefType::RepoPath,
                    "size/1",
                    SourceClass::Mandatory,
                )],
            );
            let mut materializer = StreamingRepoMaterializer::new(true, 1);
            if field == "materializer_id" {
                materializer.materializer_id = "m".repeat(maximum + extra);
            } else {
                materializer.provenance = "p".repeat(maximum + extra);
            }
            let outcome = repo
                .rehydrate_context(
                    request(vec![repo_binding(0, "source-1", "size/1")]),
                    &mut materializer,
                    &mut ArtifactFake {
                        values: HashMap::new(),
                        calls: 0,
                    },
                    &mut EventSupplier {
                        calls: 0,
                        corrupt_digest: false,
                    },
                )
                .expect("terminal receipt");
            assert_eq!(
                outcome.attempt.status,
                if extra == 0 {
                    ContextRehydrationStatus::Succeeded
                } else {
                    ContextRehydrationStatus::Failed
                }
            );
        }
    }

    let exact = crate::context_rehydration::sanitize_failure_detail(
        &"x".repeat(MAX_FAILURE_DETAIL_PERSISTED_BYTES),
    );
    assert_eq!(exact.len(), MAX_FAILURE_DETAIL_PERSISTED_BYTES);
    assert!(!exact.ends_with("...[TRUNCATED]"));
    let plus_one = crate::context_rehydration::sanitize_failure_detail(
        &"x".repeat(MAX_FAILURE_DETAIL_PERSISTED_BYTES + 1),
    );
    assert_eq!(plus_one.len(), MAX_FAILURE_DETAIL_PERSISTED_BYTES);
    assert!(plus_one.ends_with("...[TRUNCATED]"));
    for length in [
        MAX_EXTERNAL_ERROR_CAPTURE_BYTES,
        MAX_EXTERNAL_ERROR_CAPTURE_BYTES + 1,
    ] {
        let sanitized = crate::context_rehydration::sanitize_failure_detail(&"x".repeat(length));
        assert_eq!(sanitized.len(), MAX_FAILURE_DETAIL_PERSISTED_BYTES);
        assert!(sanitized.ends_with("...[TRUNCATED]"));
        assert!(std::str::from_utf8(sanitized.as_bytes()).is_ok());
    }
}

#[test]
fn source_evidence_canonical_record_enforces_8192_byte_boundary() {
    let (_tmp, mut calibration_repo) = opened("evidence-boundary-calibration");
    seed(
        &mut calibration_repo,
        vec![source(
            ContextSourceRefType::RepoPath,
            "size/1",
            SourceClass::Mandatory,
        )],
    );
    let mut calibration_materializer = StreamingRepoMaterializer::new(true, 1);
    calibration_materializer.materializer_id =
        "m".repeat(MAX_AUTHORITATIVE_BINDING_CANONICAL_BYTES);
    calibration_materializer.provenance = "p".to_string();
    let calibration = calibration_repo
        .rehydrate_context(
            request(vec![repo_binding(0, "source-1", "size/1")]),
            &mut calibration_materializer,
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut EventSupplier {
                calls: 0,
                corrupt_digest: false,
            },
        )
        .expect("calibration succeeds");
    let calibration_size =
        serde_json_canonicalizer::to_vec(&calibration.attempt.source_evidence[0])
            .expect("canonical evidence")
            .len();
    let provenance_length = MAX_SINGLE_SOURCE_EVIDENCE_RECORD_CANONICAL_BYTES
        .checked_sub(calibration_size - 1)
        .expect("boundary is constructible");
    assert!(provenance_length <= MAX_MATERIALIZER_PROVENANCE_CANONICAL_BYTES);

    for extra in 0..=1 {
        let (_tmp, mut repo) = opened(&format!("evidence-boundary-{extra}"));
        seed(
            &mut repo,
            vec![source(
                ContextSourceRefType::RepoPath,
                "size/1",
                SourceClass::Mandatory,
            )],
        );
        let mut materializer = StreamingRepoMaterializer::new(true, 1);
        materializer.materializer_id = "m".repeat(MAX_AUTHORITATIVE_BINDING_CANONICAL_BYTES);
        materializer.provenance = "p".repeat(provenance_length + extra);
        let mut events = EventSupplier {
            calls: 0,
            corrupt_digest: false,
        };
        let result = repo.rehydrate_context(
            request(vec![repo_binding(0, "source-1", "size/1")]),
            &mut materializer,
            &mut ArtifactFake {
                values: HashMap::new(),
                calls: 0,
            },
            &mut events,
        );
        if extra == 0 {
            let outcome = result.expect("8,192-byte evidence accepted");
            assert_eq!(outcome.attempt.status, ContextRehydrationStatus::Succeeded);
            assert_eq!(
                serde_json_canonicalizer::to_vec(&outcome.attempt.source_evidence[0])
                    .expect("canonical evidence")
                    .len(),
                MAX_SINGLE_SOURCE_EVIDENCE_RECORD_CANONICAL_BYTES
            );
        } else {
            assert!(result.is_err(), "8,193-byte evidence must be rejected");
            assert_eq!(events.calls, 0);
            assert!(
                repo.find_context_rehydration_attempt("project-1", "attempt-1")
                    .expect("attempt read")
                    .is_none()
            );
            assert!(
                repo.find_event("01ARZ3NDEKTSV4RRFFQ69G5FAV")
                    .expect("event read")
                    .is_none()
            );
        }
    }
}
