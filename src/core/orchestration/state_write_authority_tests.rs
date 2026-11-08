//! The core issuer is the real path to State writes: it resolves subjects from
//! the payload and its own admitted identity and refuses foreign compiler or
//! policy work before State sees it. Synthetic same-user controls only.

use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use receipts_state::{
    ActorKind, AvailabilityFactKindV1, AvailabilityFactSourceV1, AvailabilityFactV1,
    AvailabilityLifecycleStatusV1, EventActor, EventPayloadReference, EventProposalOriginV1,
    EventProposalV1, EventSubject, EventType, GraphCompilerRefV1, GraphGenesisReasonV1,
    GraphSourceEvidenceRefV1, StateEpochValueV1, StateError, StateGraphGenesisProvenanceV1,
    StateGraphGenesisV1, StateGraphNodeV1, StateReader, StateWriteRefusalV1, SubjectKind,
    TrustedClockV1, TrustedTimeSampleV1,
};

use super::{
    CoreAdmissionIdentity, CoreContentResolverV1, CorePolicyRefusal, CoreStateWriteError,
    CoreStateWriter,
};

const COMPILER: &str = "compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV";

struct Clock;
impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: "2026-11-08T10:00:00.000000000Z".into(),
            clock_source_id: "synthetic-clock".into(),
            clock_contract_version: "1".into(),
        })
    }
}

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "receipts-core-writer-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
    fn db(&self) -> PathBuf {
        self.0.join("state.db")
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const SOURCE: &str = "evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW";

/// Admits project `p` with the source its genesis fixture cites.
fn identity(policy: &str) -> CoreAdmissionIdentity {
    identity_for(policy, &[("p", SOURCE)])
}

fn identity_for(policy: &str, projects: &[(&str, &str)]) -> CoreAdmissionIdentity {
    projects.iter().fold(
        CoreAdmissionIdentity::new(GraphCompilerRefV1::parse(COMPILER).unwrap(), policy.into())
            .unwrap(),
        |identity, (project, source)| {
            identity.with_project(*project, GraphSourceEvidenceRefV1::parse(*source).unwrap())
        },
    )
}

/// Synthetic trusted content: exact bytes per (project, reference).
struct Content(Vec<(&'static str, &'static str, &'static [u8])>);
impl CoreContentResolverV1 for Content {
    fn resolve(&self, project_id: &str, reference: &str) -> Option<Vec<u8>> {
        self.0
            .iter()
            .find(|(p, r, _)| *p == project_id && *r == reference)
            .map(|(_, _, bytes)| bytes.to_vec())
    }
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    let hex: String = sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("sha256:{hex}")
}

fn policy_refusal<T: std::fmt::Debug>(result: Result<T, CoreStateWriteError>) -> CorePolicyRefusal {
    match result {
        Err(CoreStateWriteError::Policy(refusal)) => refusal,
        other => panic!("expected a core policy refusal, got {other:?}"),
    }
}

fn state_refusal<T: std::fmt::Debug>(
    result: Result<T, CoreStateWriteError>,
) -> StateWriteRefusalV1 {
    match result {
        Err(CoreStateWriteError::State(StateError::WriteAuthorityRefused { reason })) => reason,
        other => panic!("expected a State authority refusal, got {other:?}"),
    }
}

fn genesis(compiler: &str, policy: &str) -> StateGraphGenesisV1 {
    genesis_in("p", compiler, policy, SOURCE)
}

fn genesis_in(project: &str, compiler: &str, policy: &str, source: &str) -> StateGraphGenesisV1 {
    let node = StateGraphNodeV1::new(
        "n".into(),
        "g".into(),
        "TASK".into(),
        "PLANNED".into(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some("1".into()),
    )
    .unwrap();
    StateGraphGenesisV1::new(
        project.into(),
        "g".into(),
        "goal".into(),
        policy.into(),
        Some(None),
        None,
        None,
        "a".repeat(64),
        vec![node],
        Vec::new(),
        StateGraphGenesisProvenanceV1::new(
            GraphCompilerRefV1::parse(compiler).unwrap(),
            GraphSourceEvidenceRefV1::parse(source).unwrap(),
            GraphGenesisReasonV1::InitialCompilation,
        ),
    )
    .unwrap()
}

fn graphs(dir: &Dir) -> usize {
    let reader = receipts_state::ReadOnlyGraphReader::open_existing(dir.db()).unwrap();
    usize::from(reader.read_current_v1("p", "g").unwrap().is_some())
}

#[test]
fn admitted_genesis_persists_through_the_core_path() {
    let dir = Dir::new();
    let mut writer = CoreStateWriter::open(dir.db(), identity("policy")).unwrap();
    writer
        .create_initial_graph(&Clock, genesis(COMPILER, "policy"))
        .unwrap();
    drop(writer);
    assert_eq!(graphs(&dir), 1);
}

#[test]
fn foreign_compiler_or_policy_is_refused_before_state_and_writes_nothing() {
    let dir = Dir::new();
    let mut writer = CoreStateWriter::open(dir.db(), identity("policy")).unwrap();
    for (compiler, policy, expected) in [
        (
            "compiler:01ARZ3NDEKTSV4RRFFQ69G5FAX",
            "policy",
            CorePolicyRefusal::CompilerNotAdmitted,
        ),
        (
            COMPILER,
            "other-policy",
            CorePolicyRefusal::PolicyNotAdmitted,
        ),
    ] {
        match writer.create_initial_graph(&Clock, genesis(compiler, policy)) {
            Err(CoreStateWriteError::Policy(refusal)) => assert_eq!(refusal, expected),
            other => panic!("expected a core policy refusal, got {other:?}"),
        }
    }
    drop(writer);
    assert_eq!(graphs(&dir), 0);
}

#[test]
fn only_one_core_writer_holds_a_store() {
    let dir = Dir::new();
    let _writer = CoreStateWriter::open(dir.db(), identity("policy")).unwrap();
    match CoreStateWriter::open(dir.db(), identity("policy")) {
        Err(StateError::WriteAuthorityRefused { reason }) => {
            assert_eq!(reason, StateWriteRefusalV1::WriterLockHeld)
        }
        other => panic!("expected the writer lock to be held, got {other:?}"),
    }
}

#[test]
fn lower_trust_proposals_and_availability_facts_persist_only_via_the_core() {
    let dir = Dir::new();
    let mut writer = CoreStateWriter::open(dir.db(), identity("policy")).unwrap();
    let proposal = EventProposalV1::new(
        EventProposalOriginV1::Review,
        "p".into(),
        None,
        EventType::ReviewDispatched,
        EventActor {
            kind: ActorKind::System,
            id: None,
        },
        EventSubject {
            kind: SubjectKind::Task,
            id: "task".into(),
        },
        EventPayloadReference {
            reference: "artifact:review-request".into(),
            digest: sha256(b"review request"),
        },
        "corr".into(),
        StateEpochValueV1::try_from(0i64).unwrap(),
        None,
    )
    .unwrap();
    let receipt = writer
        .persist_event_proposal(
            &Clock,
            &Content(vec![("p", "artifact:review-request", b"review request")]),
            &proposal,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
        )
        .unwrap();
    assert_eq!(receipt.store_id, writer.store_id());
    let fact = AvailabilityFactV1::new(
        "p".into(),
        "anthropic".into(),
        "claude".into(),
        String::new(),
        AvailabilityFactKindV1::Demotion,
        "PROVIDER_DOWN".into(),
        AvailabilityFactSourceV1::RuntimeFailureClass,
        "attempt:1".into(),
        None,
    )
    .unwrap();
    let applied = writer.append_availability_fact(&Clock, &fact).unwrap();
    assert_eq!(applied.status, AvailabilityLifecycleStatusV1::Demoted);
    drop(writer);
    let reader = StateReader::open_existing(dir.db()).unwrap();
    let event = reader
        .find_event("01ARZ3NDEKTSV4RRFFQ69G5FAV")
        .unwrap()
        .unwrap();
    assert_eq!(event.event_type, EventType::ReviewDispatched);
}

const OTHER_SOURCE: &str = "evidence:01ARZ3NDEKTSV4RRFFQ69G5FAX";

#[test]
fn genesis_source_comes_from_the_admitted_context_not_the_payload() {
    // Payload cites SOURCE, but the composition admitted OTHER_SOURCE.
    let dir = Dir::new();
    let mut writer =
        CoreStateWriter::open(dir.db(), identity_for("policy", &[("p", OTHER_SOURCE)])).unwrap();
    assert_eq!(
        state_refusal(writer.create_initial_graph(&Clock, genesis(COMPILER, "policy"))),
        StateWriteRefusalV1::SourceMismatch
    );
    // A project the composition never admitted is refused before State.
    assert_eq!(
        policy_refusal(
            writer.create_initial_graph(&Clock, genesis_in("q", COMPILER, "policy", SOURCE))
        ),
        CorePolicyRefusal::ProjectNotAdmitted
    );
    drop(writer);
    assert_eq!(graphs(&dir), 0);
}

fn mutation(graph: &str) -> receipts_state::StateGraphMutationV1 {
    receipts_state::StateGraphMutationV1::parse_json(&format!(
        r#"{{"mutation_id":"m2","graph_id":"{graph}","parent_version":1,"resulting_version":2,"actor":{{"role":"USER"}},"reason":"change","created_at":"2026-11-08T10:00:00Z","operations":[{{"op":"SET_NODE_STATE"}}]}}"#
    ))
    .unwrap()
}

fn snapshot(source: &str) -> receipts_state::StateGraphSnapshotV1 {
    receipts_state::StateGraphSnapshotV1::new(
        "2".into(),
        "1".into(),
        genesis_in("p", COMPILER, "policy", source),
    )
    .unwrap()
}

#[test]
fn mutations_bind_the_stored_graph_source_and_refuse_unknown_graphs() {
    let dir = Dir::new();
    let mut writer = CoreStateWriter::open(dir.db(), identity("policy")).unwrap();
    // No stored graph yet: unknown subject, nothing written.
    assert_eq!(
        policy_refusal(writer.append_graph_mutation(&Clock, mutation("g"), snapshot(SOURCE))),
        CorePolicyRefusal::UnknownSubject
    );
    writer
        .create_initial_graph(&Clock, genesis(COMPILER, "policy"))
        .unwrap();
    // A successor citing another source than the stored genesis is refused.
    assert_eq!(
        state_refusal(writer.append_graph_mutation(&Clock, mutation("g"), snapshot(OTHER_SOURCE))),
        StateWriteRefusalV1::SourceMismatch
    );
    writer
        .append_graph_mutation(&Clock, mutation("g"), snapshot(SOURCE))
        .unwrap();
}

#[test]
fn every_project_family_goes_through_the_core_and_refuses_foreign_or_unknown_subjects() {
    use receipts_state::{
        AttemptFinish, AttemptId, AttemptRole, AttemptStart, ContextEpochTrigger,
        DecisionRecordKind, DecisionRecordV1, ExecutorBinding, LogicalRole, LogicalRoleStatus,
        LogicalRoleType, OutputRef, ProvenanceLinkKind, ReleaseReason, RoleChangeEvidenceV1,
        StateGraphNodeResultV1,
    };
    let dir = Dir::new();
    // `r` is admitted but has no graph; `q` is not admitted at all.
    let mut writer = CoreStateWriter::open(
        dir.db(),
        identity_for("policy", &[("p", SOURCE), ("r", SOURCE)]),
    )
    .unwrap();
    writer
        .create_initial_graph(&Clock, genesis(COMPILER, "policy"))
        .unwrap();
    let not_admitted = CorePolicyRefusal::ProjectNotAdmitted;
    let unknown = CorePolicyRefusal::UnknownSubject;

    // Node results.
    let result = || {
        StateGraphNodeResultV1::parse_json(
            r#"{"node_id":"n","graph_id":"g","outcome":"PASS","produced_at":"2026-11-08T10:00:00Z"}"#,
        )
        .unwrap()
    };
    writer.append_node_result("p", "1", "r1", result()).unwrap();
    assert_eq!(
        policy_refusal(writer.append_node_result("q", "1", "r2", result())),
        not_admitted
    );
    assert_eq!(
        policy_refusal(writer.append_node_result("r", "1", "r2", result())),
        unknown
    );

    // Attempts: start resolves the stored graph, finish the stored attempt.
    let start = |project: &str, id: &str| {
        AttemptStart::new(
            AttemptId::parse(id).unwrap(),
            project,
            "g",
            "n",
            "1",
            AttemptRole::RuntimeA3,
            "runtime",
            "ws",
            &"a".repeat(40),
            "2026-11-08T10:00:00.000000000Z",
        )
        .unwrap()
    };
    writer
        .append_attempt_started(start("p", "01ARZ3NDEKTSV4RRFFQ69G5FA1"))
        .unwrap();
    assert_eq!(
        policy_refusal(writer.append_attempt_started(start("q", "01ARZ3NDEKTSV4RRFFQ69G5FA2"))),
        not_admitted
    );
    assert_eq!(
        policy_refusal(writer.append_attempt_started(start("r", "01ARZ3NDEKTSV4RRFFQ69G5FA3"))),
        unknown
    );
    let finish = |id: &str| {
        AttemptFinish::new(
            AttemptId::parse(id).unwrap(),
            "2026-11-08T10:01:00.000000000Z",
            None,
            None,
            None,
        )
        .unwrap()
    };
    writer
        .append_attempt_finished(finish("01ARZ3NDEKTSV4RRFFQ69G5FA1"))
        .unwrap();
    assert_eq!(
        policy_refusal(writer.append_attempt_finished(finish("01ARZ3NDEKTSV4RRFFQ69G5FA9"))),
        unknown
    );

    // Checkpoints and output refs.
    let checkpoint = |project: &str| {
        receipts_state::CheckpointRecord::parse_json(
            project,
            &format!(
                r#"{{"checkpoint_id":"c","workspace_id":"w","head_sha":"{}","kind":"PROGRESS","captured_at":"2026-11-08T10:00:00Z"}}"#,
                "a".repeat(40)
            ),
        )
        .unwrap()
    };
    writer.append_checkpoint(checkpoint("p")).unwrap();
    assert_eq!(
        policy_refusal(writer.append_checkpoint(checkpoint("q"))),
        not_admitted
    );
    let output =
        |project: &str| OutputRef::new(project, "out", &"b".repeat(64), "uri:x", "1").unwrap();
    writer.append_output_ref(output("p")).unwrap();
    assert_eq!(
        policy_refusal(writer.append_output_ref(output("q"))),
        not_admitted
    );

    // Decision records and provenance links.
    let review = |project: &str| {
        DecisionRecordV1::parse(
            project,
            DecisionRecordKind::A4Review,
            "rev",
            &format!(
                r#"{{"review_id":"rev","task_id":"t","reviewed_sha":"{}","verdict":"PASS","independence_attested":true,"blocking_findings":[],"nonblocking_findings":[],"dimensions":[{{"dimension":"CORRECTNESS","assessment":"SATISFIED"}}]}}"#,
                "c".repeat(40)
            ),
        )
        .unwrap()
    };
    writer.append_decision_record(&review("p")).unwrap();
    assert_eq!(
        policy_refusal(writer.append_decision_record(&review("q"))),
        not_admitted
    );
    let sha = "d".repeat(40);
    writer
        .append_provenance_link("p", "t", ProvenanceLinkKind::ReviewSha, &sha)
        .unwrap();
    assert_eq!(
        policy_refusal(writer.append_provenance_link(
            "q",
            "t",
            ProvenanceLinkKind::ReviewSha,
            &sha
        )),
        not_admitted
    );

    // Context epochs.
    assert_eq!(
        policy_refusal(writer.advance_context_epoch(
            "q",
            "2026-11-08T10:00:00Z",
            ContextEpochTrigger::A1Init,
            &[]
        )),
        not_admitted
    );

    // Roles resolve their stored project; bindings resolve role then project.
    let role = |project: &str, id: &str| LogicalRole {
        role_id: id.into(),
        project_id: project.into(),
        role_type: LogicalRoleType::RuntimeA2,
        status: LogicalRoleStatus::Active,
        current_context_epoch: StateEpochValueV1::try_from(0i64).unwrap(),
        name: None,
        workstream_id: None,
        ownership_paths: Vec::new(),
        integration_branch: None,
        context_manifest_id: None,
        active_binding_id: None,
        created_at: None,
    };
    writer.create_logical_role(role("p", "role-1")).unwrap();
    assert_eq!(
        policy_refusal(writer.create_logical_role(role("q", "role-2"))),
        not_admitted
    );
    let evidence = RoleChangeEvidenceV1 {
        changed_at: receipts_state::CanonicalTimestampV1::parse("2026-11-08T10:00:00.000000000Z")
            .unwrap(),
        reason: None,
    };
    assert_eq!(
        policy_refusal(writer.transition_logical_role_status(
            "missing-role",
            LogicalRoleStatus::Active,
            LogicalRoleStatus::Suspended,
            &evidence
        )),
        unknown
    );
    let binding = |role_id: &str, id: &str| ExecutorBinding {
        binding_id: id.into(),
        role_id: role_id.into(),
        provider_id: "openai".into(),
        model_id: "model".into(),
        runtime_id: "codex".into(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: "2026-11-08T10:00:00Z".into(),
        lease_expires_at: "2026-11-08T11:00:00Z".into(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    };
    writer
        .create_executor_binding(binding("role-1", "b-1"))
        .unwrap();
    assert_eq!(
        policy_refusal(writer.create_executor_binding(binding("missing-role", "b-2"))),
        unknown
    );
    assert_eq!(
        policy_refusal(writer.release_executor_binding(
            "missing-binding",
            "2026-11-08T10:30:00Z",
            ReleaseReason::Completed
        )),
        unknown
    );
    writer
        .release_executor_binding("b-1", "2026-11-08T10:30:00Z", ReleaseReason::Completed)
        .unwrap();
}

fn review_proposal(project: &str, reference: &str, digest: String) -> EventProposalV1 {
    EventProposalV1::new(
        EventProposalOriginV1::Review,
        project.into(),
        None,
        EventType::ReviewDispatched,
        EventActor {
            kind: ActorKind::System,
            id: None,
        },
        EventSubject {
            kind: SubjectKind::Task,
            id: "task".into(),
        },
        EventPayloadReference {
            reference: reference.into(),
            digest,
        },
        "corr".into(),
        StateEpochValueV1::try_from(0i64).unwrap(),
        None,
    )
    .unwrap()
}

#[test]
fn event_referenced_content_is_resolved_and_digest_verified_before_persistence() {
    const EVENT: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let dir = Dir::new();
    let mut writer = CoreStateWriter::open(
        dir.db(),
        identity_for("policy", &[("p", SOURCE), ("r", SOURCE)]),
    )
    .unwrap();
    let content = Content(vec![
        ("p", "artifact:present", b"present bytes"),
        ("r", "artifact:foreign", b"foreign bytes"),
    ]);
    let nothing_persisted = |writer: &CoreStateWriter| {
        assert!(writer.reader().find_event(EVENT).unwrap().is_none());
        assert!(
            writer
                .reader()
                .find_trusted_time_watermark("p")
                .unwrap()
                .is_none()
        );
    };
    let cases = [
        // Nonexistent content with a syntactically valid digest.
        (
            review_proposal("p", "artifact:never-created", sha256(b"x")),
            CorePolicyRefusal::ContentUnavailable,
        ),
        // Existing content, wrong digest.
        (
            review_proposal("p", "artifact:present", sha256(b"other bytes")),
            CorePolicyRefusal::ContentDigestMismatch,
        ),
        // Content that exists only under another project.
        (
            review_proposal("p", "artifact:foreign", sha256(b"foreign bytes")),
            CorePolicyRefusal::ContentUnavailable,
        ),
        // A digest the core cannot verify.
        (
            review_proposal("p", "artifact:present", "md5:abc".into()),
            CorePolicyRefusal::UnsupportedDigest,
        ),
        // A project the composition did not admit.
        (
            review_proposal("q", "artifact:present", sha256(b"present bytes")),
            CorePolicyRefusal::ProjectNotAdmitted,
        ),
    ];
    for (proposal, expected) in cases {
        assert_eq!(
            policy_refusal(writer.persist_event_proposal(&Clock, &content, &proposal, EVENT)),
            expected
        );
        nothing_persisted(&writer);
    }
    // Matching content succeeds; replay remains refused by State.
    let proposal = review_proposal("p", "artifact:present", sha256(b"present bytes"));
    let receipt = writer
        .persist_event_proposal(&Clock, &content, &proposal, EVENT)
        .unwrap();
    assert_eq!(receipt.event_id, EVENT);
    assert!(
        writer
            .persist_event_proposal(&Clock, &content, &proposal, "01ARZ3NDEKTSV4RRFFQ69G5FAW")
            .is_err()
    );
    assert!(
        writer
            .reader()
            .find_event("01ARZ3NDEKTSV4RRFFQ69G5FAW")
            .unwrap()
            .is_none()
    );
}
