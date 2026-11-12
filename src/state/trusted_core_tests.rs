//! Trusted-core boundary: secure exclusive open, single-use admissions bound
//! to the actual payload, core-only event persistence with receipts, and D5
//! availability ordering/refusals. Synthetic same-user controls only; no
//! separate-principal deployment proof is claimed here.

use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::Connection;

use crate::error::{GraphFailureCode as Code, StateError};
use crate::graph_repository_tests::request;
use crate::{
    ActorKind, AvailabilityFactKindV1 as Kind, AvailabilityFactSourceV1 as Source,
    AvailabilityFactV1, AvailabilityLifecycleStatusV1 as Status, EventActor, EventPayloadReference,
    EventProposalOriginV1, EventProposalV1, EventSubject, EventType, SqliteStateRepository,
    StateEpochValueV1, StateReader, StateWriteOperationV1 as Op, StateWriteRefusalV1 as R,
    StateWriteSubjectV1, SubjectKind, TrustedClockV1, TrustedCoreStore, TrustedTimeSampleV1,
};

const EVENT_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const EVENT_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";

struct Clock(&'static str);
impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: self.0.into(),
            clock_source_id: "synthetic-clock".into(),
            clock_contract_version: "1".into(),
        })
    }
}
const T1: Clock = Clock("2026-11-08T10:00:00.000000000Z");
const T0: Clock = Clock("2026-11-08T09:00:00.000000000Z");

/// A private 0700 directory removed on drop.
struct Dir(PathBuf);
impl Dir {
    fn new(tag: &str) -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "receipts-trusted-core-{tag}-{}-{}",
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
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn refusal(error: StateError) -> R {
    match error {
        StateError::WriteAuthorityRefused { reason } => reason,
        other => panic!("expected an authority refusal, got {other:?}"),
    }
}

fn graph_code(error: StateError) -> Code {
    match error {
        StateError::GraphPersistence { code, .. } => code,
        other => panic!("expected a graph persistence code, got {other:?}"),
    }
}

fn graph_subject(op: Op, project: &str, graph: &str) -> StateWriteSubjectV1 {
    StateWriteSubjectV1 {
        operation: op,
        project_id: project.into(),
        graph_id: Some(graph.into()),
        source_ref: Some("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW".into()),
        compiler_id: Some("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
        policy_id: Some("policy".into()),
    }
}

fn project_subject(op: Op, project: &str) -> StateWriteSubjectV1 {
    StateWriteSubjectV1 {
        operation: op,
        project_id: project.into(),
        graph_id: None,
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    }
}

fn graph_count(store: &TrustedCoreStore) -> i64 {
    store.reader().count_table_rows("graphs").unwrap()
}

#[test]
fn exclusive_open_creates_identified_store_and_holds_the_writer_lock() {
    let dir = Dir::new("lock");
    let store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let id = store.store_id().to_owned();
    assert_eq!(id.len(), 32);
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
    assert_eq!(store.reader().schema_version().unwrap(), 25);
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(dir.db()).unwrap_err()),
        R::WriterLockHeld
    );
    drop(store);
    let reopened = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert_eq!(reopened.store_id(), id, "identity is durable");
    // Lower-trust readers still work read-only on the v20 store.
    let reader = StateReader::open_existing(dir.db()).unwrap();
    assert!(reader.find_event(EVENT_A).unwrap().is_none());
}

#[test]
fn insecure_paths_and_permissions_are_refused_without_repair() {
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive("relative/state.db").unwrap_err()),
        R::InsecurePath
    );

    let open_dir = Dir::new("mode-dir");
    std::fs::set_permissions(&open_dir.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(open_dir.db()).unwrap_err()),
        R::InsecureMode
    );
    assert!(
        !open_dir.db().exists(),
        "nothing created in a shared directory"
    );
    let mode = std::fs::metadata(&open_dir.0).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o755, "permissions are never silently repaired");

    let dir = Dir::new("mode-file");
    drop(TrustedCoreStore::open_exclusive(dir.db()).unwrap());
    std::fs::set_permissions(dir.db(), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(dir.db()).unwrap_err()),
        R::InsecureMode
    );

    let link_dir = Dir::new("symlink");
    let target = Dir::new("symlink-target");
    drop(TrustedCoreStore::open_exclusive(target.db()).unwrap());
    std::os::unix::fs::symlink(target.db(), link_dir.db()).unwrap();
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(link_dir.db()).unwrap_err()),
        R::InsecurePath
    );

    let hard = Dir::new("hardlink");
    drop(TrustedCoreStore::open_exclusive(hard.db()).unwrap());
    std::fs::hard_link(hard.db(), hard.0.join("alias.db")).unwrap();
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(hard.db()).unwrap_err()),
        R::InsecurePath
    );
}

#[test]
fn graph_admission_binds_every_payload_field_and_refusals_write_nothing() {
    let dir = Dir::new("graph");
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let genesis = || request("p", "g");
    let base = || graph_subject(Op::GraphGenesis, "p", "g");
    type Case = (fn(&mut StateWriteSubjectV1), R);
    let cases: [Case; 7] = [
        (|s| s.operation = Op::GraphMutation, R::WrongOperation),
        (|s| s.project_id = "other".into(), R::ProjectMismatch),
        (|s| s.graph_id = Some("other".into()), R::GraphMismatch),
        (|s| s.graph_id = None, R::GraphMismatch),
        (
            |s| s.source_ref = Some("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAX".into()),
            R::SourceMismatch,
        ),
        (
            |s| s.compiler_id = Some("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAX".into()),
            R::CompilerMismatch,
        ),
        (|s| s.policy_id = None, R::PolicyMismatch),
    ];
    for (mutate, expected) in cases {
        let mut subject = base();
        mutate(&mut subject);
        let admission = store.admit(subject);
        let error = store
            .create_initial_graph_v1(admission, &T1, genesis())
            .unwrap_err();
        assert_eq!(refusal(error), expected);
        assert_eq!(graph_count(&store), 0);
        assert!(
            store
                .reader()
                .find_trusted_time_watermark("p")
                .unwrap()
                .is_none()
        );
    }
    let admission = store.admit(base());
    store
        .create_initial_graph_v1(admission, &T1, genesis())
        .unwrap();
    assert_eq!(graph_count(&store), 1);
}

#[test]
fn an_admission_from_another_store_is_refused() {
    let (a, b) = (Dir::new("store-a"), Dir::new("store-b"));
    let store_a = TrustedCoreStore::open_exclusive(a.db()).unwrap();
    let mut store_b = TrustedCoreStore::open_exclusive(b.db()).unwrap();
    assert_ne!(store_a.store_id(), store_b.store_id());
    let foreign = store_a.admit(graph_subject(Op::GraphGenesis, "p", "g"));
    let error = store_b
        .create_initial_graph_v1(foreign, &T1, request("p", "g"))
        .unwrap_err();
    assert_eq!(refusal(error), R::WrongStore);
    assert_eq!(graph_count(&store_b), 0);
}

fn provenance_count(store: &TrustedCoreStore) -> i64 {
    store.reader().count_table_rows("provenance_links").unwrap()
}

fn link(
    store: &mut TrustedCoreStore,
    admission: crate::StateWriteAdmission,
    task: &str,
) -> Result<(), StateError> {
    store.append_provenance_link(
        admission,
        "p",
        task,
        crate::ProvenanceLinkKind::ReviewSha,
        "0123456789012345678901234567890123456789",
    )
}

#[test]
fn admissions_bind_the_issuing_live_writer_instance() {
    let a = Dir::new("copy-a");
    let b = Dir::new("copy-b");
    drop(TrustedCoreStore::open_exclusive(a.db()).unwrap());
    std::fs::copy(a.db(), b.db()).unwrap();
    std::fs::set_permissions(b.db(), std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut store_a = TrustedCoreStore::open_exclusive(a.db()).unwrap();
    let mut store_b = TrustedCoreStore::open_exclusive(b.db()).unwrap();
    assert_eq!(
        store_a.store_id(),
        store_b.store_id(),
        "copy keeps the UUID"
    );

    // Same writer: success.
    let own = store_a.admit(project_subject(Op::ProvenanceLink, "p"));
    link(&mut store_a, own, "t1").unwrap();
    assert_eq!(provenance_count(&store_a), 1);

    // Copied A/B: A's admission is refused by B, B unchanged.
    let from_a = store_a.admit(project_subject(Op::ProvenanceLink, "p"));
    assert_eq!(
        refusal(link(&mut store_b, from_a, "t2").unwrap_err()),
        R::WrongStore
    );
    assert_eq!(provenance_count(&store_b), 0);

    // Old admission after close/reopen of the same file is refused.
    let stale = store_a.admit(project_subject(Op::ProvenanceLink, "p"));
    drop(store_a);
    let mut reopened = TrustedCoreStore::open_exclusive(a.db()).unwrap();
    assert_eq!(
        refusal(link(&mut reopened, stale, "t3").unwrap_err()),
        R::WrongStore
    );
    assert_eq!(provenance_count(&reopened), 1);
    // A newly minted admission after the legitimate reopen succeeds, once.
    let fresh = reopened.admit(project_subject(Op::ProvenanceLink, "p"));
    link(&mut reopened, fresh, "t3").unwrap();
    assert_eq!(provenance_count(&reopened), 2);
}

/// Holds an extra directory whose mode the test controls.
fn private_dir_below(parent: &std::path::Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .unwrap();
    path
}

#[test]
fn protected_ancestry_is_required_and_never_repaired() {
    let root = Dir::new("ancestry");
    // Non-sticky world-writable ancestor: another principal could rename the
    // private directory below it.
    let shared = private_dir_below(&root.0, "shared");
    let state = private_dir_below(&shared, "state");
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(state.join("state.db")).unwrap_err()),
        R::InsecureMode
    );
    assert!(!state.join("state.db").exists());
    assert_eq!(
        std::fs::metadata(&shared).unwrap().permissions().mode() & 0o7777,
        0o777,
        "never repaired"
    );
    // Group-writable ancestor is refused the same way.
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o770)).unwrap();
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(state.join("state.db")).unwrap_err()),
        R::InsecureMode
    );
    // Sticky shared parent with a private directory owned by this user: valid.
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o1777)).unwrap();
    drop(TrustedCoreStore::open_exclusive(state.join("state.db")).unwrap());
    // Protected non-private ancestors (0755, owned by this user) are valid.
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o755)).unwrap();
    drop(TrustedCoreStore::open_exclusive(state.join("state.db")).unwrap());
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn a_symlinked_or_replaced_state_directory_is_refused() {
    let root = Dir::new("dir-link");
    let real = private_dir_below(&root.0, "real");
    drop(TrustedCoreStore::open_exclusive(real.join("state.db")).unwrap());
    // A symlink alias resolves to the real chain (validated as such) ...
    std::os::unix::fs::symlink(&real, root.0.join("alias")).unwrap();
    drop(TrustedCoreStore::open_exclusive(root.0.join("alias/state.db")).unwrap());
    // ... but a symlink below a held, validated directory is never followed.
    let below = private_dir_below(&real, "inner");
    let target = private_dir_below(&root.0, "target");
    std::fs::remove_dir(&below).unwrap();
    std::os::unix::fs::symlink(&target, &below).unwrap();
    let mut path = real.join("inner");
    path.push("state.db");
    // canonicalize follows the alias to `target`, which is valid itself; the
    // refusal case is a symlinked database entry.
    std::os::unix::fs::symlink(real.join("state.db"), target.join("state.db")).unwrap();
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(&path).unwrap_err()),
        R::InsecurePath
    );
    assert!(
        std::fs::symlink_metadata(target.join("state.db"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn unsafe_pre_existing_sidecars_are_refused_before_sqlite_opens() {
    use std::os::unix::fs::OpenOptionsExt;
    let make = |tag: &str| {
        let dir = Dir::new(tag);
        drop(TrustedCoreStore::open_exclusive(dir.db()).unwrap());
        dir
    };
    let sidecar = |dir: &Dir, suffix: &str| {
        let mut name = dir.db().into_os_string();
        name.push(suffix);
        PathBuf::from(name)
    };
    // Missing sidecars and a valid private sidecar both open.
    let ok = make("sidecar-ok");
    drop(TrustedCoreStore::open_exclusive(ok.db()).unwrap());
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(sidecar(&ok, "-journal"))
        .unwrap();
    drop(TrustedCoreStore::open_exclusive(ok.db()).unwrap());

    for suffix in ["-wal", "-shm", "-journal"] {
        // Symlink sidecar pointing at a foreign object: refused, foreign
        // object unchanged.
        let dir = make("sidecar-link");
        let foreign = dir.0.join("foreign");
        std::fs::write(&foreign, b"foreign").unwrap();
        std::os::unix::fs::symlink(&foreign, sidecar(&dir, suffix)).unwrap();
        assert_eq!(
            refusal(TrustedCoreStore::open_exclusive(dir.db()).unwrap_err()),
            R::InsecurePath
        );
        assert_eq!(std::fs::read(&foreign).unwrap(), b"foreign");

        // FIFO sidecar: refused without opening (no blocking).
        let dir = make("sidecar-fifo");
        let fifo = sidecar(&dir, suffix);
        let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
        assert_eq!(
            refusal(TrustedCoreStore::open_exclusive(dir.db()).unwrap_err()),
            R::InsecurePath
        );

        // Hard-linked sidecar: refused, both names retained.
        let dir = make("sidecar-hard");
        let other = dir.0.join("other");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&other)
            .unwrap();
        std::fs::hard_link(&other, sidecar(&dir, suffix)).unwrap();
        assert_eq!(
            refusal(TrustedCoreStore::open_exclusive(dir.db()).unwrap_err()),
            R::InsecurePath
        );
        assert!(other.exists() && sidecar(&dir, suffix).exists());

        // Group-readable sidecar: refused, mode not repaired.
        let dir = make("sidecar-mode");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o640)
            .open(sidecar(&dir, suffix))
            .unwrap();
        std::fs::set_permissions(
            sidecar(&dir, suffix),
            std::fs::Permissions::from_mode(0o640),
        )
        .unwrap();
        assert_eq!(
            refusal(TrustedCoreStore::open_exclusive(dir.db()).unwrap_err()),
            R::InsecureMode
        );
        assert_eq!(
            std::fs::metadata(sidecar(&dir, suffix))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
}

#[test]
fn role_scoped_writes_resolve_the_target_project_and_refuse_unknown_subjects() {
    let dir = Dir::new("role");
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let admission = store.admit(project_subject(Op::ExecutorBinding, "p"));
    let error = store
        .release_executor_binding(
            admission,
            "missing-binding",
            "2026-11-08T10:00:00Z",
            crate::ReleaseReason::RateLimited,
        )
        .unwrap_err();
    assert_eq!(refusal(error), R::UnknownSubject);
}

fn proposal(digest: &str, claim: Option<&str>) -> EventProposalV1 {
    EventProposalV1::new(
        EventProposalOriginV1::Routing,
        "p".into(),
        None,
        EventType::RoutingDecided,
        EventActor {
            kind: ActorKind::System,
            id: None,
        },
        EventSubject {
            kind: SubjectKind::Task,
            id: "task-1".into(),
        },
        EventPayloadReference {
            reference: "artifact:routing-decision".into(),
            digest: digest.into(),
        },
        "corr-1".into(),
        StateEpochValueV1::try_from(0i64).unwrap(),
        claim.map(str::to_owned),
    )
    .unwrap()
}

fn event_rows(store: &TrustedCoreStore) -> (i64, i64) {
    (
        store.reader().count_table_rows("event").unwrap(),
        store
            .reader()
            .count_table_rows("event_proposal_receipt")
            .unwrap(),
    )
}

#[test]
fn proposals_persist_with_trusted_time_and_a_receipt_atomically() {
    let dir = Dir::new("event");
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let claimed = "2001-01-01T00:00:00.000000000Z";
    let p = proposal("sha256:aa", Some(claimed));
    let admission = store.admit(project_subject(Op::EventProposal, "p"));
    let receipt = store
        .persist_event_proposal(admission, &T1, &p, EVENT_A)
        .unwrap();
    assert_eq!(receipt.proposal_sha256, p.sha256());
    assert_eq!(receipt.store_id, store.store_id());
    assert_eq!(receipt.origin, EventProposalOriginV1::Routing);
    assert_eq!(receipt.occurred_at, T1.0);
    let event = store.reader().find_event(EVENT_A).unwrap().unwrap();
    assert_eq!(event.occurred_at, T1.0, "trusted time, never the claim");
    assert_eq!(event.event_type, EventType::RoutingDecided);
    assert_eq!(
        store.reader().find_event_receipt(EVENT_A).unwrap(),
        Some(receipt)
    );

    // Replaying the same proposal under a new id is refused atomically.
    let admission = store.admit(project_subject(Op::EventProposal, "p"));
    let error = store
        .persist_event_proposal(admission, &T1, &p, EVENT_B)
        .unwrap_err();
    assert_eq!(graph_code(error), Code::AlreadyExists);
    assert_eq!(event_rows(&store), (1, 1));

    // A distinct proposal reusing an event id fails and leaves no receipt.
    let admission = store.admit(project_subject(Op::EventProposal, "p"));
    assert!(
        store
            .persist_event_proposal(admission, &T1, &proposal("sha256:bb", None), EVENT_A)
            .is_err()
    );
    assert_eq!(event_rows(&store), (1, 1));

    // A trusted-clock regression rolls back the whole write.
    let admission = store.admit(project_subject(Op::EventProposal, "p"));
    let error = store
        .persist_event_proposal(admission, &T0, &proposal("sha256:cc", None), EVENT_B)
        .unwrap_err();
    assert_eq!(graph_code(error), Code::ClockRegression);
    assert_eq!(event_rows(&store), (1, 1));

    // Another project's admission cannot persist this project's proposal.
    let admission = store.admit(project_subject(Op::EventProposal, "q"));
    let error = store
        .persist_event_proposal(admission, &T1, &proposal("sha256:dd", None), EVENT_B)
        .unwrap_err();
    assert_eq!(refusal(error), R::ProjectMismatch);
    assert_eq!(event_rows(&store), (1, 1));
}

#[test]
fn proposals_validate_shape_and_digest_every_field() {
    let invalid = EventProposalV1::new(
        EventProposalOriginV1::Host,
        String::new(),
        None,
        EventType::TaskStarted,
        EventActor {
            kind: ActorKind::System,
            id: None,
        },
        EventSubject {
            kind: SubjectKind::Task,
            id: "t".into(),
        },
        EventPayloadReference {
            reference: "r".into(),
            digest: "d".into(),
        },
        "c".into(),
        StateEpochValueV1::try_from(0i64).unwrap(),
        None,
    );
    assert!(matches!(invalid, Err(StateError::EventValidation { .. })));
    assert!(matches!(
        EventProposalV1::new(
            EventProposalOriginV1::Host,
            "p".into(),
            None,
            EventType::TaskStarted,
            EventActor {
                kind: ActorKind::System,
                id: None
            },
            EventSubject {
                kind: SubjectKind::Task,
                id: "t".into()
            },
            EventPayloadReference {
                reference: "r".into(),
                digest: "d".into()
            },
            "c".into(),
            StateEpochValueV1::try_from(0i64).unwrap(),
            Some("yesterday".into()),
        ),
        Err(StateError::EventValidation { .. })
    ));
    let a = proposal("sha256:aa", None);
    assert_ne!(a.sha256(), proposal("sha256:ab", None).sha256());
    assert_ne!(a.sha256(), proposal("sha256:aa", Some(T1.0)).sha256());
    assert_eq!(a.sha256(), proposal("sha256:aa", None).sha256());
}

fn fact(kind: Kind, source: Source, follows: Option<u64>) -> AvailabilityFactV1 {
    AvailabilityFactV1::new(
        "p".into(),
        "anthropic".into(),
        "claude".into(),
        String::new(),
        kind,
        "PROVIDER_RATE_LIMITED".into(),
        source,
        "attempt:1".into(),
        follows,
    )
    .unwrap()
}

fn append(
    store: &mut TrustedCoreStore,
    f: &AvailabilityFactV1,
) -> Result<crate::AvailabilityFactReceiptV1, StateError> {
    let admission = store.admit(project_subject(Op::AvailabilityFact, "p"));
    store.append_availability_fact(admission, &T1, f)
}

#[test]
fn availability_facts_are_monotonic_and_never_promote_without_a_producer() {
    let dir = Dir::new("d5");
    let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    // Nothing to follow: an unbound recovery claim is refused.
    assert_eq!(
        graph_code(
            append(
                &mut store,
                &fact(Kind::RetryAfterElapsed, Source::RetryAfterExpiry, Some(1))
            )
            .unwrap_err()
        ),
        Code::ParentConflict
    );
    let demoted = append(
        &mut store,
        &fact(Kind::Demotion, Source::RuntimeFailureClass, None),
    )
    .unwrap();
    assert_eq!((demoted.sequence, demoted.status), (1, Status::Demoted));

    let claim = append(
        &mut store,
        &fact(
            Kind::PromotionUnauthenticated,
            Source::UnauthenticatedClaim,
            Some(1),
        ),
    )
    .unwrap();
    assert_eq!((claim.sequence, claim.status), (2, Status::Demoted));
    let elapsed = append(
        &mut store,
        &fact(Kind::RetryAfterElapsed, Source::RetryAfterExpiry, Some(2)),
    )
    .unwrap();
    assert_eq!((elapsed.sequence, elapsed.status), (3, Status::Demoted));

    // Stale (older) and forged (future) predecessors are refused, nothing written.
    for follows in [2, 9] {
        let error = append(
            &mut store,
            &fact(
                Kind::PromotionUnauthenticated,
                Source::ProbeReport,
                Some(follows),
            ),
        )
        .unwrap_err();
        assert_eq!(graph_code(error), Code::ParentConflict);
    }
    let life = store
        .reader()
        .availability_lifecycle("p", "anthropic", "claude", "")
        .unwrap();
    assert_eq!(life.status, Status::Demoted);
    assert_eq!(life.latest_sequence, 3);
    assert_eq!(life.last_demotion_sequence, Some(1));
    assert_eq!(
        life.last_demotion_reason.as_deref(),
        Some("PROVIDER_RATE_LIMITED")
    );
    assert_eq!(life.unverified_recovery_sequence, Some(3));

    // Other partitions are independent and start Unknown.
    let other = store
        .reader()
        .availability_lifecycle("p", "openai", "codex", "")
        .unwrap();
    assert_eq!((other.status, other.latest_sequence), (Status::Unknown, 0));

    // Shape rules: demotions name no predecessor; recovery facts must.
    assert!(
        AvailabilityFactV1::new(
            "p".into(),
            "x".into(),
            "y".into(),
            String::new(),
            Kind::Demotion,
            "r".into(),
            Source::ProbeReport,
            "s".into(),
            Some(1)
        )
        .is_err()
    );
    assert!(
        AvailabilityFactV1::new(
            "p".into(),
            "x".into(),
            "y".into(),
            String::new(),
            Kind::PromotionUnauthenticated,
            "r".into(),
            Source::UnauthenticatedClaim,
            "s".into(),
            None
        )
        .is_err()
    );
    assert!(
        AvailabilityFactV1::new(
            "p".into(),
            "x".into(),
            "y".into(),
            String::new(),
            Kind::Demotion,
            "r".into(),
            Source::ProbeReport,
            String::new(),
            None
        )
        .is_err()
    );

    // History is immutable and survives reopen.
    drop(store);
    let conn = Connection::open(dir.db()).unwrap();
    assert!(
        conn.execute_batch("UPDATE availability_fact SET kind='DEMOTION' WHERE sequence=2")
            .is_err()
    );
    assert!(conn.execute_batch("DELETE FROM availability_fact").is_err());
    assert!(conn.execute_batch("INSERT INTO availability_fact VALUES ('p','anthropic','claude','',5,'DEMOTION','r','PROBE_REPORT','s',NULL,'t')").is_err(), "sequence gaps are refused");
    drop(conn);
    let reader = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let again = reader
        .reader()
        .availability_lifecycle("p", "anthropic", "claude", "")
        .unwrap();
    assert_eq!(again.latest_sequence, 3);
}

#[test]
fn migrate_exclusive_upgrades_a_v19_store_and_a_failed_v20_leaves_exact_v19() {
    let dir = Dir::new("migrate");
    drop(
        SqliteStateRepository::open_with_migrations(
            dir.db(),
            &crate::migrations::registered()[..19],
        )
        .unwrap(),
    );
    std::fs::set_permissions(dir.db(), std::fs::Permissions::from_mode(0o600)).unwrap();
    // Ordinary open never upgrades.
    assert!(TrustedCoreStore::open_exclusive(dir.db()).is_err());

    let conn = Connection::open(dir.db()).unwrap();
    conn.execute_batch("CREATE TRIGGER fail20 BEFORE INSERT ON state_schema_version WHEN NEW.version=20 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(TrustedCoreStore::migrate_exclusive(dir.db()).is_err());
    crate::graph_repository::verify_v19(&conn, crate::GraphPhase::Read).unwrap();
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('state_store_identity','event_proposal_receipt','availability_fact')", [], |r| r.get::<_, i64>(0)).unwrap(),
        0
    );
    conn.execute_batch("DROP TRIGGER fail20").unwrap();
    drop(conn);

    let (store, version) = TrustedCoreStore::migrate_exclusive(dir.db()).unwrap();
    assert_eq!(version, 25);
    assert_eq!(store.reader().schema_version().unwrap(), 25);
    assert_eq!(store.store_id().len(), 32);
}

#[test]
fn store_identity_is_immutable_and_verified() {
    let dir = Dir::new("identity");
    drop(TrustedCoreStore::open_exclusive(dir.db()).unwrap());
    let conn = Connection::open(dir.db()).unwrap();
    assert!(
        conn.execute_batch(
            "UPDATE state_store_identity SET store_id='00000000000000000000000000000000'"
        )
        .is_err()
    );
    assert!(
        conn.execute_batch("DELETE FROM state_store_identity")
            .is_err()
    );
    assert!(
        conn.execute_batch(
            "INSERT INTO state_store_identity VALUES (1,'00000000000000000000000000000000')"
        )
        .is_err()
    );
    // Removing an immutability trigger is detected as corruption on open.
    conn.execute_batch("DROP TRIGGER state_store_identity_no_update")
        .unwrap();
    drop(conn);
    assert!(TrustedCoreStore::open_exclusive(dir.db()).is_err());
}

/// Holds the descriptors inherited at fork without running Rust or SQLite in
/// the child. Only async-signal-safe calls run there; the parent always reaps it.
struct InheritedDescriptors {
    pid: libc::pid_t,
    _sockets: (
        std::os::unix::net::UnixStream,
        std::os::unix::net::UnixStream,
    ),
}
impl InheritedDescriptors {
    fn fork() -> Self {
        use std::os::fd::AsRawFd;
        let sockets = std::os::unix::net::UnixStream::pair().unwrap();
        let fd = sockets.0.as_raw_fd();
        // SAFETY: the child only polls an inherited fd then calls _exit;
        // it never touches allocator, Rust destructors or inherited SQLite.
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork failed");
        if pid == 0 {
            let mut poll = libc::pollfd {
                fd,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: valid pollfd; bounded lifetime even if the parent fails.
            unsafe {
                libc::poll(&mut poll, 1, 10_000);
                libc::_exit(0);
            }
        }
        Self {
            pid,
            _sockets: sockets,
        }
    }

    fn assert_alive(&mut self) {
        let mut status = 0;
        // SAFETY: wait only for this owned child, without blocking.
        let rc = unsafe { libc::waitpid(self.pid, &mut status, libc::WNOHANG) };
        if rc == self.pid {
            self.pid = 0;
        }
        assert_eq!(rc, 0, "child must still hold its inherited descriptors");
    }
}
impl Drop for InheritedDescriptors {
    fn drop(&mut self) {
        if self.pid == 0 {
            return;
        }
        let mut status = 0;
        // SAFETY: terminate and reap only the child this helper owns.
        let reaped = unsafe {
            libc::kill(self.pid, libc::SIGKILL);
            libc::waitpid(self.pid, &mut status, 0)
        };
        assert_eq!(reaped, self.pid, "owned child was not reaped");
    }
}

#[test]
fn writer_teardown_releases_lock_while_a_forked_child_holds_inherited_descriptors() {
    let dir = Dir::new("fork-lock");
    let store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    let id = store.store_id().to_owned();
    store
        .reader()
        .connection()
        .execute_batch("BEGIN IMMEDIATE; CREATE TABLE lock_teardown_probe(value INTEGER);")
        .unwrap();
    let mut child = InheritedDescriptors::fork();
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(dir.db()).unwrap_err()),
        R::WriterLockHeld
    );
    drop(store);
    child.assert_alive();
    let reopened = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
    assert_eq!(reopened.store_id(), id);
    let probes: i64 = reopened
        .reader()
        .connection()
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='lock_teardown_probe'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        probes, 0,
        "SQLite teardown rolled back the open transaction"
    );
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(dir.db()).unwrap_err()),
        R::WriterLockHeld
    );
    child.assert_alive();
}

#[test]
fn last_writer_reference_controls_lock_lifetime() {
    let dir = Dir::new("retained-owner");
    let store = std::rc::Rc::new(TrustedCoreStore::open_exclusive(dir.db()).unwrap());
    let retained = std::rc::Rc::clone(&store);
    drop(store);
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(dir.db()).unwrap_err()),
        R::WriterLockHeld
    );
    drop(retained);
    drop(TrustedCoreStore::open_exclusive(dir.db()).unwrap());
}

#[test]
fn failed_secure_open_and_sqlite_initialization_release_the_writer_lock() {
    let dir = Dir::new("failed-open-lock");
    std::fs::write(dir.db(), b"not a sqlite database").unwrap();
    std::fs::set_permissions(dir.db(), std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        refusal(TrustedCoreStore::open_exclusive(dir.db()).unwrap_err()),
        R::InsecureMode
    );
    std::fs::set_permissions(dir.db(), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(TrustedCoreStore::open_exclusive(dir.db()).is_err());
    std::fs::remove_file(dir.db()).unwrap();
    drop(TrustedCoreStore::open_exclusive(dir.db()).unwrap());
}
