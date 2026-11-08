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

use super::{CoreAdmissionIdentity, CorePolicyRefusal, CoreStateWriteError, CoreStateWriter};

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

fn identity(policy: &str) -> CoreAdmissionIdentity {
    CoreAdmissionIdentity::new(GraphCompilerRefV1::parse(COMPILER).unwrap(), policy.into()).unwrap()
}

fn genesis(compiler: &str, policy: &str) -> StateGraphGenesisV1 {
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
        "p".into(),
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
            GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
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
            digest: "sha256:aa".into(),
        },
        "corr".into(),
        StateEpochValueV1::try_from(0i64).unwrap(),
        None,
    )
    .unwrap();
    let receipt = writer
        .persist_event_proposal(&Clock, &proposal, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
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
