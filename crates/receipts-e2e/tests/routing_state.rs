//! Real Routing → Core admissions → SQLite controls; no publication journal.
use std::collections::BTreeMap;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use receipts_e2e::CoreRoutingState;
use receipts_model_routing::availability::{
    AvailabilityManager, AvailabilityObservation, BackoffPolicy, FixedJitter,
};
use receipts_model_routing::persistence::{
    EventScope, Publication, PublicationReceipt, RoutingStatePort, load_decision, persist_decision,
    persist_request, record_reference, request_reference, sha256_hex,
};
use receipts_model_routing::policy_eligibility::ModelRoutingDateTimeV1;
use receipts_model_routing::registry::{Observation, PersistentRegistry, ProviderId, RuntimeId};
use receipts_model_routing::wire::{
    decode_routing_decision, decode_routing_request, encode_routing_request,
};
use receipts_model_routing::{AvailabilitySignalSource, EvidenceConfidence};
use receipts_orchestration::orchestration::{
    CoreAdmissionIdentity, CoreContentResolverV1, CorePolicyRefusal, CoreStateWriteError,
    CoreStateWriter,
};
use receipts_review_integration::runtime::{
    CodexCapabilityEvidence, CodexCapabilityProbeReport, FailureClass, RuntimeCapabilities,
};
use receipts_review_integration::state::{
    AvailabilityFactKindV1, AvailabilityFactSourceV1, AvailabilityFactV1, DecisionRecordKind,
    DecisionRecordV1, EventProposalV1, EventQuery, GraphCompilerRefV1, GraphSourceEvidenceRefV1,
    PublicationFailureCode, SqliteStateRepository, StateEpochValueV1, StateError, TrustedClockV1,
    TrustedTimeSampleV1,
};

const PROJECT: &str = "routing-project";
const AT: &str = "2026-11-10T10:00:00Z";
struct Clock;
impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: "2026-11-10T10:00:00.000000000Z".into(),
            clock_source_id: "routing-control".into(),
            clock_contract_version: "1".into(),
        })
    }
}
fn identity() -> CoreAdmissionIdentity {
    CoreAdmissionIdentity::new(
        GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        "policy".into(),
    )
    .unwrap()
    .with_project(
        PROJECT,
        GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
    )
}
struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "receipts-routing-real-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        Self(path)
    }
    fn open(&self) -> CoreStateWriter {
        CoreStateWriter::open(self.0.join("state.db"), identity()).unwrap()
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
#[derive(Default)]
struct Requests(BTreeMap<(String, String), Vec<u8>>);
impl Requests {
    fn admit(&mut self, project: &str, request: &receipts_model_routing::RoutingRequest) {
        self.0.insert(
            (
                project.into(),
                request_reference(request.core().request_id()),
            ),
            encode_routing_request(request).into_bytes(),
        );
    }
}
impl CoreContentResolverV1 for Requests {
    fn resolve(&self, project: &str, reference: &str) -> Option<Vec<u8>> {
        self.0.get(&(project.into(), reference.into())).cloned()
    }
}
fn scope() -> EventScope {
    EventScope {
        project_id: PROJECT.into(),
        goal_id: None,
        epoch: StateEpochValueV1::try_from(1_i64).unwrap(),
    }
}
fn observed() -> Observation {
    Observation {
        confidence: EvidenceConfidence::Unverified,
        source_ref: None,
        observed_at: ts(AT),
    }
}
fn ts(s: &str) -> ModelRoutingDateTimeV1 {
    ModelRoutingDateTimeV1::try_new(s.into()).unwrap()
}
fn provider(s: &str) -> ProviderId {
    ProviderId::try_new(s.into()).unwrap()
}
fn request(extra: &str) -> receipts_model_routing::RoutingRequest {
    decode_routing_request(&format!(r#"{{"request_id":"request-1","task_id":"task-1","role":"IMPLEMENTER","task_class":"FRONTIER_IMPLEMENTATION","quality_floor":"FRONTIER","required_capabilities":["coding"{extra}]}}"#)).unwrap()
}
fn decision() -> receipts_model_routing::RoutingDecisionNonTemporalCore {
    decode_routing_decision(include_str!(
        "../../../src/routing/wire/fixtures/RoutingDecision.valid.full.json"
    ))
    .unwrap()
}
fn counts(reader: &SqliteStateRepository) -> (usize, usize, usize, usize) {
    let records = |kind| {
        reader
            .list_decision_records(PROJECT, kind, None)
            .unwrap()
            .len()
    };
    (
        records(DecisionRecordKind::RegistryCapability),
        records(DecisionRecordKind::RoutingDecision),
        records(DecisionRecordKind::AvailabilityObservation),
        reader
            .list_events(&EventQuery {
                project_id: PROJECT.into(),
                subject: None,
                event_types: None,
                limit: 1000,
            })
            .unwrap()
            .len(),
    )
}
fn image(reader: &SqliteStateRepository) -> String {
    format!(
        "{:?}|{:?}|{:?}",
        counts(reader),
        reader.find_trusted_time_watermark(PROJECT).unwrap(),
        reader
            .availability_lifecycle(PROJECT, "prov-a", "rt-a", "model-a")
            .unwrap()
    )
}

#[derive(Default)]
enum Fault {
    #[default]
    None,
    AckLost,
    Collision(DecisionRecordV1),
    StaleFact,
    BadProposalReceipt,
    BadOriginReceipt,
    BadStoreReceipt,
    BadEventReceipt,
}
struct Controlled<'a> {
    port: CoreRoutingState<'a>,
    fault: Fault,
    last: Option<Publication>,
    receipt: Option<PublicationReceipt>,
}
impl<'a> Controlled<'a> {
    fn new(writer: &'a mut CoreStateWriter, requests: &'a Requests) -> Self {
        Self {
            port: CoreRoutingState::new(writer, &Clock, requests, PROJECT),
            fault: Fault::None,
            last: None,
            receipt: None,
        }
    }
}
impl RoutingStatePort for Controlled<'_> {
    type Error = String;
    fn reader(&self) -> &SqliteStateRepository {
        self.port.reader()
    }
    fn publish(&mut self, p: &Publication) -> Result<PublicationReceipt, String> {
        self.last = Some(p.clone());
        let fault = std::mem::take(&mut self.fault);
        let mut sent = p.clone();
        match &fault {
            Fault::Collision(record) => sent.records.push(record.clone()),
            Fault::StaleFact => {
                sent.fact = Some(
                    AvailabilityFactV1::new(
                        PROJECT.into(),
                        "prov-a".into(),
                        "rt-a".into(),
                        "model-a".into(),
                        AvailabilityFactKindV1::RetryAfterElapsed,
                        "elapsed".into(),
                        AvailabilityFactSourceV1::RetryAfterExpiry,
                        "synthetic".into(),
                        Some(999),
                    )
                    .unwrap(),
                )
            }
            _ => {}
        }
        let mut receipt = self.port.publish(&sent).map_err(|e| format!("{e:?}"))?;
        self.receipt = Some(receipt.clone());
        match fault {
            Fault::AckLost => return Err("committed; acknowledgement lost".into()),
            Fault::BadProposalReceipt => receipt.events[0].proposal_sha256 = "0".repeat(64),
            Fault::BadOriginReceipt => {
                receipt.events[0].origin =
                    receipts_review_integration::state::EventProposalOriginV1::Host
            }
            Fault::BadStoreReceipt => receipt.events[0].store_id = "foreign-store".into(),
            Fault::BadEventReceipt => {
                receipt.events[0].event_id = "00000000000000000000000000".into()
            }
            _ => {}
        }
        Ok(receipt)
    }
}

#[test]
fn request_content_is_admitted_exactly_project_bound_and_conflict_safe_after_reopen() {
    let dir = Dir::new();
    let mut writer = dir.open();
    let mut requests = Requests::default();
    let req = request("");
    for project in ["foreign", PROJECT] {
        if project == "foreign" {
            requests.admit(project, &req);
        }
        let before = image(writer.reader());
        let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, PROJECT);
        assert!(matches!(
            persist_request(&mut port, &scope(), &req),
            Err(
                receipts_model_routing::persistence::RoutingPersistError::Port(
                    CoreStateWriteError::Policy(CorePolicyRefusal::ContentUnavailable)
                )
            )
        ));
        assert_eq!(image(port.reader()), before);
    }
    requests.admit(PROJECT, &request(",\"extra\""));
    let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, PROJECT);
    assert!(matches!(
        persist_request(&mut port, &scope(), &req),
        Err(
            receipts_model_routing::persistence::RoutingPersistError::Port(
                CoreStateWriteError::Policy(CorePolicyRefusal::ContentDigestMismatch)
            )
        )
    ));
    requests.admit(PROJECT, &req);
    let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, PROJECT);
    let original = persist_request(&mut port, &scope(), &req).unwrap();
    let event = port
        .reader()
        .find_event(&original.events[0].event_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        event.payload.digest,
        format!(
            "sha256:{}",
            sha256_hex(encode_routing_request(&req).as_bytes())
        )
    );
    assert_eq!(event.payload.reference, request_reference("request-1"));
    assert_eq!(
        (event.subject.id.as_str(), event.correlation_id.as_str()),
        ("task-1", "request-1")
    );
    drop(writer);
    let mut writer = dir.open();
    let before = image(writer.reader());
    let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, PROJECT);
    let again = persist_request(&mut port, &scope(), &req).unwrap();
    assert!(again.replayed);
    assert_eq!(again.events, original.events);
    assert_eq!(image(port.reader()), before);
    // Even when the trusted resolver admits changed bytes, identity stays the
    // logical request: State refuses instead of publishing another event.
    let changed = request(",\"extra\"");
    requests.admit(PROJECT, &changed);
    let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, PROJECT);
    assert!(matches!(
        persist_request(&mut port, &scope(), &changed),
        Err(
            receipts_model_routing::persistence::RoutingPersistError::Port(
                CoreStateWriteError::State(StateError::Publication {
                    code: PublicationFailureCode::ConflictingRetry,
                    ..
                })
            )
        )
    ));
    assert_eq!(image(port.reader()), before);
}

#[test]
fn registry_rolls_back_after_a_record_write_and_does_not_advance_memory() {
    let dir = Dir::new();
    let mut writer = dir.open();
    let requests = Requests::default();
    let mut port = Controlled::new(&mut writer, &requests);
    let mut reg = PersistentRegistry::new(PROJECT);
    reg.insert_provider(&mut port, &scope(), provider("prov-a"), observed())
        .unwrap();
    let collision = port.last.as_ref().unwrap().records[0].clone();
    let before = image(port.reader());
    let memory = reg.registry().clone();
    port.fault = Fault::Collision(collision);
    let failure = reg
        .insert_provider(&mut port, &scope(), provider("prov-b"), observed())
        .unwrap_err();
    assert!(
        format!("{failure:?}").contains("AlreadyExists"),
        "{failure:?}"
    );
    assert_eq!(image(port.reader()), before);
    assert_eq!(reg.registry(), &memory);
    drop(port);
    drop(writer);
    let mut writer = dir.open();
    assert_eq!(image(writer.reader()), before);
    assert_eq!(
        PersistentRegistry::reload::<()>(writer.reader(), PROJECT)
            .unwrap()
            .registry(),
        &memory
    );
    let mut port = Controlled::new(&mut writer, &requests);
    reg.insert_provider(&mut port, &scope(), provider("prov-b"), observed())
        .unwrap();
    assert_eq!(counts(port.reader()), (2, 0, 0, 2));
}

#[test]
fn whole_probe_lost_ack_reconciles_original_receipts_after_reopen() {
    let dir = Dir::new();
    let mut writer = dir.open();
    let requests = Requests::default();
    let mut port = Controlled::new(&mut writer, &requests);
    let mut reg = PersistentRegistry::new(PROJECT);
    let p = provider("prov-a");
    let r = RuntimeId::try_new("rt-a".into()).unwrap();
    reg.insert_provider(&mut port, &scope(), p.clone(), observed())
        .unwrap();
    reg.insert_runtime(&mut port, &scope(), p.clone(), r.clone(), observed())
        .unwrap();
    let probe = RuntimeCapabilities::Codex(CodexCapabilityProbeReport {
        version: "synthetic".into(),
        json: CodexCapabilityEvidence::Supported,
        output_schema: CodexCapabilityEvidence::Unknown,
        sandbox: CodexCapabilityEvidence::Supported,
    });
    let source = receipts_model_routing::EvidenceSourceRef::try_new(
        receipts_model_routing::EvidenceSourceRefType::ArtifactId,
        "synthetic-probe".into(),
        None,
        None,
    )
    .unwrap();
    let memory = reg.registry().clone();
    // A collision after the probe's three records also rolls back all three.
    port.fault = Fault::Collision(port.last.as_ref().unwrap().records[0].clone());
    let before = image(port.reader());
    assert!(
        reg.record_runtime_probe(&mut port, &scope(), &p, &r, &probe, ts(AT), source.clone())
            .is_err()
    );
    assert_eq!(image(port.reader()), before);
    assert_eq!(reg.registry(), &memory);
    port.fault = Fault::AckLost;
    assert!(
        reg.record_runtime_probe(&mut port, &scope(), &p, &r, &probe, ts(AT), source.clone())
            .is_err()
    );
    assert_eq!(reg.registry(), &memory);
    assert_eq!(counts(port.reader()), (5, 0, 0, 5));
    let original = port.receipt.clone().unwrap();
    let done = image(port.reader());
    let mut changed = probe.clone();
    if let RuntimeCapabilities::Codex(report) = &mut changed {
        report.json = CodexCapabilityEvidence::Unknown;
    }
    let failure = reg
        .record_runtime_probe(
            &mut port,
            &scope(),
            &p,
            &r,
            &changed,
            ts(AT),
            source.clone(),
        )
        .unwrap_err();
    assert!(
        format!("{failure:?}").contains("ConflictingRetry"),
        "{failure:?}"
    );
    assert_eq!(reg.registry(), &memory);
    assert_eq!(image(port.reader()), done);

    drop(port);
    drop(writer);
    let mut writer = dir.open();
    let mut port = Controlled::new(&mut writer, &requests);
    let again = reg
        .record_runtime_probe(&mut port, &scope(), &p, &r, &probe, ts(AT), source)
        .unwrap();
    assert_eq!(again, original.events);
    assert!(port.receipt.as_ref().unwrap().replayed);
    assert_eq!(image(port.reader()), done);
    assert_eq!(
        PersistentRegistry::reload::<()>(port.reader(), PROJECT)
            .unwrap()
            .registry(),
        reg.registry()
    );
}

#[test]
fn decision_records_and_both_events_replay_and_conflicting_content_refuses() {
    let dir = Dir::new();
    let mut writer = dir.open();
    let requests = Requests::default();
    let store_id = writer.store_id().to_owned();
    let mut port = Controlled::new(&mut writer, &requests);
    port.fault = Fault::AckLost;
    let d = decision();
    assert!(persist_decision(&mut port, &scope(), &d).is_err());
    let original = port.receipt.clone().unwrap();
    let publication = port.last.clone().unwrap();
    assert_eq!(counts(port.reader()), (0, 1, 0, 2));
    for receipt in &original.events {
        assert_eq!(
            port.reader()
                .find_event_receipt(&receipt.event_id)
                .unwrap()
                .as_ref(),
            Some(receipt)
        );
        assert_eq!(receipt.store_id, store_id);
    }
    drop(port);
    drop(writer);
    let mut writer = dir.open();
    let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, PROJECT);
    let before = image(port.reader());
    let again = persist_decision(&mut port, &scope(), &d).unwrap();
    assert!(again.replayed);
    assert_eq!(again.events, original.events);
    assert_eq!(
        load_decision(port.reader(), PROJECT, d.decision_id()).unwrap(),
        Some(d)
    );
    let mut conflicting = publication;
    let old = &conflicting.records[0];
    conflicting.records[0] = DecisionRecordV1::parse(
        PROJECT,
        old.kind(),
        old.record_id(),
        &old.record_json().replace("AVAILABILITY", "CHANGED"),
    )
    .unwrap();
    // Keep event bytes/digest coherent with the altered typed record.
    let record = &conflicting.records[0];
    for event in &mut conflicting.events {
        let envelope = port
            .reader()
            .find_event(&original.events[0].event_id)
            .unwrap()
            .unwrap();
        event.proposal = EventProposalV1::new(
            event.proposal.origin(),
            PROJECT.into(),
            None,
            event.proposal.event_type(),
            envelope.actor.clone(),
            envelope.subject.clone(),
            receipts_review_integration::state::EventPayloadReference {
                reference: record_reference(record),
                digest: format!("sha256:{}", record.record_sha256()),
            },
            envelope.correlation_id.clone(),
            envelope.epoch.clone(),
            None,
        )
        .unwrap();
        event.content = record.record_json().as_bytes().to_vec();
    }
    assert!(matches!(
        port.publish(&conflicting),
        Err(CoreStateWriteError::State(StateError::Publication {
            code: PublicationFailureCode::ConflictingRetry,
            ..
        }))
    ));
    assert_eq!(image(port.reader()), before);
}

#[test]
fn core_resolves_typed_record_content_and_refuses_missing_altered_or_foreign_payloads() {
    let dir = Dir::new();
    let mut writer = dir.open();
    let requests = Requests::default();
    let mut port = Controlled::new(&mut writer, &requests);
    port.fault = Fault::AckLost;
    assert!(persist_decision(&mut port, &scope(), &decision()).is_err());
    let good = port.last.take().unwrap();
    // Use a new empty store, with no prior writes to mask a resolution error.
    drop(port);
    drop(writer);
    let empty = Dir::new();
    let mut writer = empty.open();
    let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, PROJECT);
    let before = image(port.reader());
    let mut missing = good.clone();
    let record = &missing.records[0];
    missing.records[0] = DecisionRecordV1::parse(
        PROJECT,
        record.kind(),
        "unrelated",
        &record.record_json().replace("decision-1", "unrelated"),
    )
    .unwrap();
    assert!(matches!(
        port.publish(&missing),
        Err(CoreStateWriteError::Policy(
            CorePolicyRefusal::ContentUnavailable
        ))
    ));
    let mut altered_attachment = good.clone();
    altered_attachment.events[1].content.push(b' ');
    assert!(matches!(
        port.publish(&altered_attachment),
        Err(CoreStateWriteError::Policy(
            CorePolicyRefusal::ContentDigestMismatch
        ))
    ));
    let mut altered = good.clone();
    let record = &altered.records[0];
    altered.records[0] = DecisionRecordV1::parse(
        PROJECT,
        record.kind(),
        record.record_id(),
        &record.record_json().replace("AVAILABILITY", "ALTERED"),
    )
    .unwrap();
    assert!(matches!(
        port.publish(&altered),
        Err(CoreStateWriteError::Policy(
            CorePolicyRefusal::ContentDigestMismatch
        ))
    ));
    let mut foreign = good.clone();
    let record = &foreign.records[0];
    foreign.records[0] = DecisionRecordV1::parse(
        "foreign",
        record.kind(),
        record.record_id(),
        record.record_json(),
    )
    .unwrap();
    assert!(port.publish(&foreign).is_err());
    assert_eq!(image(port.reader()), before);
    let ok = port.publish(&good).unwrap();
    assert_eq!(ok.events.len(), 2);
}

fn policy() -> BackoffPolicy {
    BackoffPolicy::try_new(10, 120, 4).unwrap()
}
fn manager() -> AvailabilityManager {
    AvailabilityManager::new(policy()).with_jitter(std::sync::Arc::new(FixedJitter(u64::MAX)))
}
fn observation() -> AvailabilityObservation {
    AvailabilityObservation {
        provider_id: provider("prov-a"),
        model_id: receipts_model_routing::registry::ModelId::try_new("model-a".into()).unwrap(),
        runtime_id: RuntimeId::try_new("rt-a".into()).unwrap(),
        failure: FailureClass::RateLimited,
        retry_after_seconds: None,
        signal_source: AvailabilitySignalSource::StderrClassification,
        observed_at: ts(AT),
        quota: None,
    }
}
#[test]
fn availability_late_transaction_failure_rolls_back_then_lost_ack_reconciles_after_reopen() {
    let dir = Dir::new();
    let mut writer = dir.open();
    let requests = Requests::default();
    let mut port = Controlled::new(&mut writer, &requests);
    let mut manager = manager();
    let effect = manager.assess(&observation()).unwrap();
    let before = image(port.reader());
    // State rejects this stale fact AFTER inserting records and both events.
    port.fault = Fault::StaleFact;
    let failure = manager
        .persist_and_commit(&mut port, &scope(), &effect)
        .unwrap_err();
    assert!(
        format!("{failure:?}").contains("ParentConflict"),
        "{failure:?}"
    );
    assert!(manager.records().is_empty());
    assert_eq!(image(port.reader()), before);
    drop(port);
    drop(writer);
    let mut writer = dir.open();
    assert_eq!(image(writer.reader()), before);
    let mut port = Controlled::new(&mut writer, &requests);
    port.fault = Fault::AckLost;
    assert!(
        manager
            .persist_and_commit(&mut port, &scope(), &effect)
            .is_err()
    );
    assert!(manager.records().is_empty());
    assert_eq!(counts(port.reader()), (0, 0, 2, 2));
    let original = port.receipt.clone().unwrap();
    let done = image(port.reader());
    let mut changed = observation();
    changed.retry_after_seconds = Some(11);
    let conflicting = manager.assess(&changed).unwrap();
    let failure = manager
        .persist_and_commit(&mut port, &scope(), &conflicting)
        .unwrap_err();
    assert!(
        format!("{failure:?}").contains("ConflictingRetry"),
        "{failure:?}"
    );
    assert!(manager.records().is_empty());
    assert_eq!(image(port.reader()), done);

    drop(port);
    drop(writer);
    let mut writer = dir.open();
    let mut port = Controlled::new(&mut writer, &requests);
    let again = manager
        .persist_and_commit(&mut port, &scope(), &effect)
        .unwrap();
    assert!(again.replayed);
    assert_eq!(again.events, original.events);
    assert_eq!(again.demotion, original.fact);
    assert_eq!(image(port.reader()), done);
    let reloaded = AvailabilityManager::reload(port.reader(), PROJECT, policy())
        .unwrap()
        .with_jitter(std::sync::Arc::new(FixedJitter(u64::MAX)));
    assert_eq!(reloaded.records(), manager.records());
    let mut next = observation();
    next.runtime_id = RuntimeId::try_new("rt-b".into()).unwrap();
    assert!(matches!(
        reloaded.assess(&next).unwrap(),
        receipts_model_routing::availability::ObservationEffect::Limited {
            attempt: 2,
            window_seconds: 20,
            ..
        }
    ));
}

#[test]
fn wrong_receipt_binding_never_advances_registry_memory() {
    let dir = Dir::new();
    let mut writer = dir.open();
    let requests = Requests::default();
    let mut port = Controlled::new(&mut writer, &requests);
    let mut reg = PersistentRegistry::new(PROJECT);
    let memory = reg.registry().clone();
    for fault in [
        Fault::BadProposalReceipt,
        Fault::BadOriginReceipt,
        Fault::BadStoreReceipt,
        Fault::BadEventReceipt,
    ] {
        port.fault = fault;
        let failure = reg
            .insert_provider(&mut port, &scope(), provider("prov-a"), observed())
            .unwrap_err();
        assert!(matches!(
            failure,
            receipts_model_routing::registry::RegistryPersistError::Persist(
                receipts_model_routing::persistence::RoutingPersistError::ReceiptShape
            )
        ));
        assert_eq!(reg.registry(), &memory);
        assert_eq!(counts(port.reader()), (1, 0, 0, 1));
    }
    reg.insert_provider(&mut port, &scope(), provider("prov-a"), observed())
        .unwrap();
    assert_eq!(
        PersistentRegistry::reload::<()>(port.reader(), PROJECT)
            .unwrap()
            .registry(),
        reg.registry()
    );
}

#[test]
fn identical_logical_keys_are_isolated_by_project_and_unadmitted_projects_refuse() {
    let dir = Dir::new();
    let db = dir.0.join("state.db");
    let admitted = identity().with_project(
        "second",
        GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
    );
    let mut writer = CoreStateWriter::open(&db, admitted).unwrap();
    let req = request("");
    let mut requests = Requests::default();
    for project in [PROJECT, "second", "unadmitted"] {
        requests.admit(project, &req);
    }
    let mut ids = Vec::new();
    for project in [PROJECT, "second"] {
        let scoped = EventScope {
            project_id: project.into(),
            ..scope()
        };
        let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, project);
        let receipt = persist_request(&mut port, &scoped, &req).unwrap();
        ids.push(receipt.events[0].event_id.clone());
        assert!(persist_request(&mut port, &scoped, &req).unwrap().replayed);
    }
    assert_ne!(ids[0], ids[1]);
    let scoped = EventScope {
        project_id: "unadmitted".into(),
        ..scope()
    };
    let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, "unadmitted");
    assert!(matches!(
        persist_request(&mut port, &scoped, &req),
        Err(
            receipts_model_routing::persistence::RoutingPersistError::Port(
                CoreStateWriteError::Policy(CorePolicyRefusal::ProjectNotAdmitted)
            )
        )
    ));
    // An admitted project cannot publish through the other project's adapter.
    let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, PROJECT);
    let scoped = EventScope {
        project_id: "second".into(),
        ..scope()
    };
    assert!(persist_request(&mut port, &scoped, &req).is_err());
}

#[test]
fn reloaded_registry_routes_and_persists_then_a_durable_limit_produces_wait() {
    use receipts_model_routing::intelligence::{
        LifecycleEvidence, LifecycleState, LifecycleTransition, RoutablePromotionEvidence,
        VerificationPath,
    };
    use receipts_model_routing::policy_eligibility::{
        PolicyEvidenceLabel, PolicyStatus, ProviderPolicyEligibility, TechnicalStatus,
    };
    use receipts_model_routing::registry::{
        CapabilityEvidence, CapabilityId, CapabilitySubject, CapabilityValue, ModelId,
    };
    use receipts_model_routing::{
        AvailabilityState, AvailabilityStateKind, AvailabilityStateNonTemporalCore,
        BoundedPreScoreCandidateEvidence, EvidenceSourceRef, EvidenceSourceRefType, RouteContext,
        RoutingDecisionMode, RoutingDecisionOutcome, RoutingQualityFloor,
        enumerate_registry_candidates, route,
    };
    let dir = Dir::new();
    let mut writer = dir.open();
    let mut requests = Requests::default();
    let req = request("");
    requests.admit(PROJECT, &req);
    let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, PROJECT);
    let mut reg = PersistentRegistry::new(PROJECT);
    let p = provider("prov-a");
    let m = ModelId::try_new("model-a".into()).unwrap();
    let r = RuntimeId::try_new("rt-a".into()).unwrap();
    reg.insert_provider(&mut port, &scope(), p.clone(), observed())
        .unwrap();
    reg.discover_model(&mut port, &scope(), p.clone(), m.clone(), observed())
        .unwrap();
    reg.insert_runtime(&mut port, &scope(), p.clone(), r.clone(), observed())
        .unwrap();
    reg.associate_runtime(
        &mut port,
        &scope(),
        p.clone(),
        m.clone(),
        r.clone(),
        observed(),
    )
    .unwrap();
    let provenance = EvidenceSourceRef::try_new(
        EvidenceSourceRefType::ArtifactId,
        "synthetic-assessment".into(),
        None,
        None,
    )
    .unwrap();
    let evidence_observation = |confidence| Observation {
        confidence,
        source_ref: Some(provenance.clone()),
        observed_at: ts(AT),
    };
    let model = CapabilitySubject::Model {
        provider_id: p.clone(),
        model_id: m.clone(),
    };
    let pair = CapabilitySubject::ModelRuntimePair {
        provider_id: p.clone(),
        model_id: m.clone(),
        runtime_id: r.clone(),
    };
    for (subject, capability, confidence) in [
        (
            model.clone(),
            "quality_tier.FRONTIER",
            EvidenceConfidence::OfficialVerified,
        ),
        (model, "coding", EvidenceConfidence::OfficialVerified),
        (pair.clone(), "coding", EvidenceConfidence::OfficialVerified),
        (pair, "coding", EvidenceConfidence::LocalEmpirical),
    ] {
        reg.record_capability(
            &mut port,
            &scope(),
            CapabilityEvidence {
                subject,
                capability: CapabilityId::try_new(capability.into()).unwrap(),
                value: Some(CapabilityValue::Boolean(true)),
                observation: evidence_observation(confidence),
                sample_size: None,
            },
        )
        .unwrap();
    }
    for (target, evidence, confidence) in [
        (
            LifecycleState::Unassessed,
            LifecycleEvidence::ReadyForAssessment,
            EvidenceConfidence::OfficialVerified,
        ),
        (
            LifecycleState::CapabilityVerified,
            LifecycleEvidence::CapabilityVerification(VerificationPath {
                runtime_id: r,
                required_capabilities: vec![CapabilityId::try_new("coding".into()).unwrap()],
            }),
            EvidenceConfidence::OfficialVerified,
        ),
        (
            LifecycleState::Calibrating,
            LifecycleEvidence::BoundedCalibrationAdmission { authorized: true },
            EvidenceConfidence::OfficialVerified,
        ),
        (
            LifecycleState::Routable,
            LifecycleEvidence::RoutablePromotion(RoutablePromotionEvidence::LocalCalibration {
                sufficient_acceptable_observations: true,
            }),
            EvidenceConfidence::LocalEmpirical,
        ),
    ] {
        reg.try_transition(
            &mut port,
            &scope(),
            &p,
            &m,
            LifecycleTransition {
                target,
                evidence,
                observation: evidence_observation(confidence),
            },
        )
        .unwrap();
    }
    let memory = reg.registry().clone();
    drop(writer);
    let mut writer = dir.open();
    let mut port = CoreRoutingState::new(&mut writer, &Clock, &requests, PROJECT);
    let reg = PersistentRegistry::reload::<()>(port.reader(), PROJECT).unwrap();
    assert_eq!(reg.registry(), &memory);
    let base = AvailabilityState::new(
        AvailabilityStateNonTemporalCore::try_new(
            "prov-a".into(),
            Some("model-a".into()),
            Some("rt-a".into()),
            AvailabilityStateKind::Available,
            None,
            Some(AvailabilitySignalSource::Probe),
            None,
        )
        .unwrap(),
        ts(AT),
    );
    let policy = ProviderPolicyEligibility::try_new(
        "prov-a".into(),
        "rt-a".into(),
        "USER_API".into(),
        TechnicalStatus::Connected,
        PolicyStatus::VerifiedAllowed,
        Some(vec!["LOCAL_EXTERNAL_WORKER".into()]),
        ts(AT),
        None,
        PolicyEvidenceLabel::VerifiedCurrentSelfFetched,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let now = ts(AT);
    let cutoff = ts("2026-11-01T00:00:00Z");
    let decide = |availability: &AvailabilityState, id: &str| {
        let evidence = [BoundedPreScoreCandidateEvidence::new(
            enumerate_registry_candidates(reg.registry()).remove(0),
            availability,
            &policy,
            None,
        )];
        route(
            &req,
            &RouteContext {
                registry: reg.registry(),
                evidence: &evidence,
                requested_execution_context: Some("LOCAL_EXTERNAL_WORKER"),
                now: &now,
                freshness_cutoff: &cutoff,
                mode: RoutingDecisionMode::AskOnUncertainty,
                wait_budget_seconds: 30,
            },
            id.into(),
            None,
        )
        .unwrap()
    };
    let requested = persist_request(&mut port, &scope(), &req).unwrap();
    let first = decide(&base, "selected");
    assert_eq!(first.decision.outcome(), RoutingDecisionOutcome::Selected);
    assert_eq!(
        first
            .decision
            .selection_reason()
            .unwrap()
            .quality_floor_applied(),
        Some(RoutingQualityFloor::Frontier)
    );
    let selected = persist_decision(&mut port, &scope(), &first.decision).unwrap();
    assert_eq!(selected.events.len(), 2);
    assert_ne!(selected.events[0].event_id, requested.events[0].event_id);
    let mut manager = manager();
    let effect = manager.assess(&observation()).unwrap();
    manager
        .persist_and_commit(&mut port, &scope(), &effect)
        .unwrap();
    let limited = manager.effective(&base, &now).unwrap();
    let waiting = decide(&limited, "waiting");
    assert_eq!(
        waiting.decision.outcome(),
        RoutingDecisionOutcome::NoEligibleCandidate
    );
    assert_eq!(waiting.wait_after_seconds, Some(10));
    persist_decision(&mut port, &scope(), &waiting.decision).unwrap();
    assert_eq!(
        load_decision(port.reader(), PROJECT, "selected").unwrap(),
        Some(first.decision)
    );
    assert_eq!(
        load_decision(port.reader(), PROJECT, "waiting").unwrap(),
        Some(waiting.decision)
    );
}
