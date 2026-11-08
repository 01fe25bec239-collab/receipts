use receipts_runtime_adapters::{
    CapabilityStatus, CodexCapabilityEvidence, CodexCapabilityProbeReport,
    ExtensionCapabilityReport, RuntimeCapabilities,
};
use receipts_state::{DecisionRecordKind, EventProposalOriginV1, EventQuery, EventType};

use super::*;
use crate::persistence::{
    RoutingPersistError, RoutingStatePort, fallback_chain_length, load_decision, persist_decision,
    persist_request, request_reference, sha256_hex,
};
use crate::test_state::*;

pub(crate) fn persist_ops(core: &mut CoreHarness, specs: &[Spec]) -> PersistentRegistry {
    let scope = CoreHarness::scope();
    let mut reg = PersistentRegistry::new(PROJECT);
    let o = || obs(EvidenceConfidence::Unverified, FRESH);
    for op in registry_ops(specs) {
        match op {
            RegistryOp::Provider(p) => reg.insert_provider(core, &scope, p, o()),
            RegistryOp::Model(p, m) => reg.discover_model(core, &scope, p, m, o()),
            RegistryOp::Runtime(p, r) => reg.insert_runtime(core, &scope, p, r, o()),
            RegistryOp::Associate(p, m, r) => reg.associate_runtime(core, &scope, p, m, r, o()),
            RegistryOp::Evidence(e) => reg.record_capability(core, &scope, e),
            RegistryOp::Transition(p, m, t) => reg.try_transition(core, &scope, &p, &m, t),
        }
        .unwrap();
    }
    reg
}

fn specs() -> Vec<Spec> {
    vec![
        Spec::frontier("prov-a", "model-a", "rt-a"),
        Spec::frontier("prov-b", "model-b", "rt-b").tier(crate::RoutingQualityFloor::Balanced),
    ]
}

fn stored(core: &CoreHarness) -> usize {
    core.reader()
        .list_decision_records(PROJECT, DecisionRecordKind::RegistryCapability, None)
        .unwrap()
        .len()
}

#[test]
fn registry_reload_equals_before_restart() {
    let mut core = CoreHarness::new("registry-reload");
    let mut reg = persist_ops(&mut core, &specs());
    let (p, _, r) = ids("prov-a", "model-a", "rt-a");
    let probe = RuntimeCapabilities::Codex(CodexCapabilityProbeReport {
        version: "synthetic".into(),
        json: CodexCapabilityEvidence::Supported,
        output_schema: CodexCapabilityEvidence::Unknown,
        sandbox: CodexCapabilityEvidence::Supported,
    });
    let receipts = reg
        .record_runtime_probe(
            &mut core,
            &CoreHarness::scope(),
            &p,
            &r,
            &probe,
            ts(FRESH),
            source(),
        )
        .unwrap();
    assert_eq!(receipts.len(), 3);
    assert!(
        receipts
            .iter()
            .all(|x| x.origin == EventProposalOriginV1::Routing)
    );
    // In-memory equivalence with the unpersisted service and history order.
    assert_eq!(
        reg.registry(),
        &build(&specs())
            .registry()
            .clone()
            .with_probe(&p, &r, &probe)
    );
    let before = reg.registry().clone();
    let count = stored(&core);

    core.reopen();
    let reloaded = PersistentRegistry::reload::<()>(core.reader(), PROJECT).unwrap();
    assert_eq!(reloaded.registry(), &before);
    let model = reloaded.registry().model("prov-a", "model-a").unwrap();
    assert_eq!(model.lifecycle_state(), LifecycleState::Routable);
    assert_eq!(model.transitions().len(), 4, "transition history survives");
    // The reloaded registry keeps appending in order.
    let mut reloaded = reloaded;
    let (p2, m2, _) = ids("prov-a", "model-new", "rt-a");
    reloaded
        .discover_model(
            &mut core,
            &CoreHarness::scope(),
            p2,
            m2,
            obs(EvidenceConfidence::Unverified, FRESH),
        )
        .unwrap();
    assert_eq!(stored(&core), count + 1);
    let again = PersistentRegistry::reload::<()>(core.reader(), PROJECT).unwrap();
    assert_eq!(again.registry(), reloaded.registry());
}

trait WithProbe {
    fn with_probe(self, p: &ProviderId, r: &RuntimeId, probe: &RuntimeCapabilities) -> Registry;
}
impl WithProbe for Registry {
    fn with_probe(self, p: &ProviderId, r: &RuntimeId, probe: &RuntimeCapabilities) -> Registry {
        let mut s = ModelIntelligenceService { registry: self };
        for (name, value) in super::persistence::probe_entries(probe) {
            s.record_capability(CapabilityEvidence {
                subject: CapabilitySubject::Runtime {
                    provider_id: p.clone(),
                    runtime_id: r.clone(),
                },
                capability: CapabilityId::try_new(name).unwrap(),
                value: Some(value),
                observation: Observation {
                    confidence: EvidenceConfidence::LocalEmpirical,
                    source_ref: Some(source()),
                    observed_at: ts(FRESH),
                },
                sample_size: None,
            })
            .unwrap();
        }
        s.registry
    }
}

#[test]
fn refused_changes_write_nothing() {
    let mut core = CoreHarness::new("registry-refusals");
    let mut reg = persist_ops(&mut core, &specs());
    let count = stored(&core);
    let scope = CoreHarness::scope();
    let (p, m, r) = ids("prov-a", "model-a", "rt-a");
    let o = obs(EvidenceConfidence::Unverified, FRESH);
    assert!(matches!(
        reg.insert_provider(&mut core, &scope, p.clone(), o.clone()),
        Err(RegistryPersistError::Registry(
            RegistryError::DuplicateProvider
        ))
    ));
    assert!(matches!(
        reg.discover_model(
            &mut core,
            &scope,
            ids("prov-missing", "m", "r").0,
            m.clone(),
            o.clone()
        ),
        Err(RegistryPersistError::Registry(
            RegistryError::ProviderNotFound
        ))
    ));
    // Side states are terminal for normal promotion (pre-demotion proofs
    // cannot re-promote through the ordinary path either).
    let illegal = crate::intelligence::LifecycleTransition {
        target: LifecycleState::Unassessed,
        evidence: crate::intelligence::LifecycleEvidence::ReadyForAssessment,
        observation: o.clone(),
    };
    assert!(matches!(
        reg.try_transition(&mut core, &scope, &p, &m, illegal),
        Err(RegistryPersistError::Registry(RegistryError::Lifecycle(
            crate::intelligence::LifecycleError::IllegalTransition
        )))
    ));
    let reserved = CapabilityEvidence {
        subject: CapabilitySubject::Model {
            provider_id: p.clone(),
            model_id: m.clone(),
        },
        capability: CapabilityId::try_new("receipts.registry.lifecycle".into()).unwrap(),
        value: Some(CapabilityValue::Boolean(true)),
        observation: o.clone(),
        sample_size: None,
    };
    assert!(matches!(
        reg.record_capability(&mut core, &scope, reserved),
        Err(RegistryPersistError::ReservedCapability)
    ));
    let foreign = RuntimeCapabilities::Extension(
        ExtensionCapabilityReport::new("rt-other", [("x", CapabilityStatus::Supported)]).unwrap(),
    );
    assert!(matches!(
        reg.record_runtime_probe(&mut core, &scope, &p, &r, &foreign, ts(FRESH), source()),
        Err(RegistryPersistError::ProbeRuntimeMismatch)
    ));
    assert!(
        reg.record_runtime_probe(
            &mut core,
            &scope,
            &p,
            &r,
            &RuntimeCapabilities::Unknown,
            ts(FRESH),
            source()
        )
        .unwrap()
        .is_empty(),
        "a whole-report Unknown records nothing"
    );
    assert_eq!(stored(&core), count, "no refused change reached State");
}

#[test]
fn probe_evidence_never_confirms_a_pair_capability() {
    let mut core = CoreHarness::new("registry-probe");
    let mut s = specs();
    s[0].caps.clear();
    s[0].routable = false;
    let mut reg = persist_ops(&mut core, &s);
    let (p, m, r) = ids("prov-a", "model-a", "rt-a");
    let probe = RuntimeCapabilities::Extension(
        ExtensionCapabilityReport::new(
            "rt-a",
            [
                ("coding", CapabilityStatus::Supported),
                ("vision", CapabilityStatus::Unsupported),
            ],
        )
        .unwrap(),
    );
    reg.record_runtime_probe(
        &mut core,
        &CoreHarness::scope(),
        &p,
        &r,
        &probe,
        ts(FRESH),
        source(),
    )
    .unwrap();
    let cap = CapabilityId::try_new("coding".into()).unwrap();
    assert_eq!(
        reg.registry().compatibility(&p, &m, &r, &cap),
        Compatibility::Unknown
    );
    let runtime = CapabilitySubject::Runtime {
        provider_id: p,
        runtime_id: r,
    };
    let vision = reg
        .registry()
        .capability_evidence(&runtime, "runtime_probe.vision")
        .unwrap();
    assert_eq!(vision[0].value, Some(CapabilityValue::Boolean(false)));
    assert_eq!(
        vision[0].observation.confidence,
        EvidenceConfidence::LocalEmpirical
    );
}

#[test]
fn decisions_persist_explain_and_link_failover_chains() {
    let mut core = CoreHarness::new("decisions");
    let w = crate::router_tests::world(&specs());
    let request = |task: Option<&str>| {
        crate::RoutingRequest::new(
            crate::RoutingRequestNonTemporalCore::try_new(
                "request-1".into(),
                task.map(str::to_owned),
                crate::RoutingRequestRole::Implementer,
                crate::RoutingTaskClass::FrontierImplementation,
                crate::RoutingQualityFloor::Frontier,
                vec!["coding".into()],
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap(),
            None,
        )
    };
    let first = w.route(&request(Some("task-1")));
    let receipts = persist_decision(&mut core, &CoreHarness::scope(), &first).unwrap();
    assert_eq!(
        receipts.events.len(),
        2,
        "ROUTING_DECIDED + EXECUTOR_SELECTED"
    );
    let none = std::collections::BTreeMap::new();
    let second = w
        .route_with(
            &request(Some("task-1")),
            0,
            &none,
            "decision-2",
            Some("decision-1".into()),
        )
        .unwrap()
        .decision;
    persist_decision(&mut core, &CoreHarness::scope(), &second).unwrap();
    // Decisions are immutable: an exact retry returns the original receipts,
    // a different decision under the same id is refused.
    let again = persist_decision(&mut core, &CoreHarness::scope(), &first).unwrap();
    assert!(again.replayed);
    assert_eq!(again.events, receipts.events);
    let untasked = w.route(&request(None));
    assert!(matches!(
        persist_decision(&mut core, &CoreHarness::scope(), &untasked),
        Err(RoutingPersistError::TaskIdRequired)
    ));
    core.reopen();
    let explained = load_decision(core.reader(), PROJECT, "decision-1")
        .unwrap()
        .unwrap();
    assert_eq!(
        explained, first,
        "P-05: full record with every alternative and reason"
    );
    assert_eq!(
        load_decision(core.reader(), PROJECT, "decision-x").unwrap(),
        None
    );
    assert_eq!(
        fallback_chain_length(core.reader(), PROJECT, "decision-1").unwrap(),
        0
    );
    assert_eq!(
        fallback_chain_length(core.reader(), PROJECT, "decision-2").unwrap(),
        1
    );
    let receipt = core
        .reader()
        .find_event_receipt(&receipts.events[0].event_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.origin, EventProposalOriginV1::Routing);
}

// ---- repair: atomic publication of registry changes and decisions (F1) ----------

fn provider(name: &str) -> ProviderId {
    ids(name, "m", "r").0
}

fn fresh() -> Observation {
    obs(EvidenceConfidence::Unverified, FRESH)
}

fn empty_registry() -> Registry {
    PersistentRegistry::new(PROJECT).registry().clone()
}

#[test]
fn failed_registry_publication_leaves_live_and_restarted_state_equal_and_retryable() {
    let mut core = CoreHarness::new("registry-rollback");
    let scope = CoreHarness::scope();
    let mut reg = PersistentRegistry::new(PROJECT);

    core.arm(Fault::Rollback);
    assert!(matches!(
        reg.insert_provider(&mut core, &scope, provider("prov-a"), fresh()),
        Err(RegistryPersistError::Persist(RoutingPersistError::Port(
            HarnessError::Rolled
        )))
    ));
    assert_eq!(reg.registry(), &empty_registry(), "memory did not advance");
    assert_eq!(stored(&core), 0);
    assert_eq!(core.events(&[]), 0, "no record without its event");
    core.reopen();
    assert_eq!(
        PersistentRegistry::reload::<()>(core.reader(), PROJECT)
            .unwrap()
            .registry(),
        reg.registry(),
        "restart agrees with live"
    );

    // The same call, retried, now succeeds exactly once (not AlreadyExists).
    reg.insert_provider(&mut core, &scope, provider("prov-a"), fresh())
        .unwrap();
    assert_eq!(
        (stored(&core), core.events(&[EventType::RegistryRefreshed])),
        (1, 1)
    );
    core.reopen();
    assert_eq!(
        PersistentRegistry::reload::<()>(core.reader(), PROJECT)
            .unwrap()
            .registry(),
        reg.registry()
    );
}

#[test]
fn lost_registry_acknowledgement_is_recovered_by_the_identical_retry() {
    let mut core = CoreHarness::new("registry-ack");
    let scope = CoreHarness::scope();
    let mut reg = PersistentRegistry::new(PROJECT);

    core.arm(Fault::AckLost);
    assert!(
        reg.insert_provider(&mut core, &scope, provider("prov-a"), fresh())
            .is_err()
    );
    // The change is durable with its event; live memory is simply behind.
    assert_eq!((stored(&core), core.events(&[])), (1, 1));
    assert_eq!(reg.registry(), &empty_registry());

    let receipt = reg
        .insert_provider(&mut core, &scope, provider("prov-a"), fresh())
        .unwrap();
    assert_eq!(
        (stored(&core), core.events(&[])),
        (1, 1),
        "nothing duplicated"
    );
    assert!(
        core.reader()
            .find_event_receipt(&receipt.event_id)
            .unwrap()
            .is_some(),
        "the original event's receipt"
    );
    core.reopen();
    assert_eq!(
        PersistentRegistry::reload::<()>(core.reader(), PROJECT)
            .unwrap()
            .registry(),
        reg.registry(),
        "live equals restarted"
    );
}

#[test]
fn conflicting_registry_retry_is_refused_and_writes_nothing() {
    let mut core = CoreHarness::new("registry-conflict");
    let scope = CoreHarness::scope();
    let mut reg = PersistentRegistry::new(PROJECT);
    core.arm(Fault::AckLost);
    assert!(
        reg.insert_provider(&mut core, &scope, provider("prov-a"), fresh())
            .is_err()
    );
    // A DIFFERENT change aimed at the stored sequence is not a retry.
    assert!(matches!(
        reg.insert_provider(&mut core, &scope, provider("prov-b"), fresh()),
        Err(RegistryPersistError::Persist(RoutingPersistError::Port(
            HarnessError::Conflict
        )))
    ));
    assert_eq!((stored(&core), core.events(&[])), (1, 1));
    assert_eq!(reg.registry(), &empty_registry());
    // Recovery path: reload what the store really holds and continue.
    let mut reloaded = PersistentRegistry::reload::<HarnessError>(core.reader(), PROJECT).unwrap();
    assert!(reloaded.registry().provider("prov-a").is_some());
    reloaded
        .insert_provider(&mut core, &scope, provider("prov-b"), fresh())
        .unwrap();
    assert_eq!((stored(&core), core.events(&[])), (2, 2));
}

#[test]
fn a_probe_report_is_one_publication_never_partly_recorded() {
    let mut core = CoreHarness::new("registry-probe-atomic");
    let scope = CoreHarness::scope();
    let mut reg = persist_ops(&mut core, &specs());
    let (p, _, r) = ids("prov-a", "model-a", "rt-a");
    let probe = RuntimeCapabilities::Codex(CodexCapabilityProbeReport {
        version: "synthetic".into(),
        json: CodexCapabilityEvidence::Supported,
        output_schema: CodexCapabilityEvidence::Unknown,
        sandbox: CodexCapabilityEvidence::Supported,
    });
    let (count, events) = (stored(&core), core.events(&[]));
    let before = reg.registry().clone();

    core.arm(Fault::Rollback);
    assert!(
        reg.record_runtime_probe(&mut core, &scope, &p, &r, &probe, ts(FRESH), source())
            .is_err()
    );
    assert_eq!((stored(&core), core.events(&[])), (count, events));
    assert_eq!(reg.registry(), &before);

    core.arm(Fault::AckLost);
    assert!(
        reg.record_runtime_probe(&mut core, &scope, &p, &r, &probe, ts(FRESH), source())
            .is_err()
    );
    assert_eq!((stored(&core), core.events(&[])), (count + 3, events + 3));
    let receipts = reg
        .record_runtime_probe(&mut core, &scope, &p, &r, &probe, ts(FRESH), source())
        .unwrap();
    assert_eq!(receipts.len(), 3);
    assert_eq!((stored(&core), core.events(&[])), (count + 3, events + 3));
    core.reopen();
    assert_eq!(
        PersistentRegistry::reload::<()>(core.reader(), PROJECT)
            .unwrap()
            .registry(),
        reg.registry()
    );
}

fn routing_request(task: Option<&str>) -> crate::RoutingRequest {
    crate::RoutingRequest::new(
        crate::RoutingRequestNonTemporalCore::try_new(
            "request-1".into(),
            task.map(str::to_owned),
            crate::RoutingRequestRole::Implementer,
            crate::RoutingTaskClass::FrontierImplementation,
            crate::RoutingQualityFloor::Frontier,
            vec!["coding".into()],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap(),
        None,
    )
}

#[test]
fn decision_events_are_all_or_nothing_and_retry_exactly() {
    let mut core = CoreHarness::new("decision-atomic");
    let scope = CoreHarness::scope();
    let w = crate::router_tests::world(&specs());
    let first = w.route(&routing_request(Some("task-1")));
    let selected = [EventType::RoutingDecided, EventType::ExecutorSelected];
    let decisions = |c: &CoreHarness| c.records(DecisionRecordKind::RoutingDecision);

    // The second SELECTED event cannot fail alone: ROUTING_DECIDED,
    // EXECUTOR_SELECTED and the record are all absent, and the caller holds
    // no half-result.
    core.arm(Fault::Rollback);
    assert!(persist_decision(&mut core, &scope, &first).is_err());
    assert_eq!((decisions(&core), core.events(&selected)), (0, 0));

    // Lost acknowledgement: all three are durable; the retry returns BOTH
    // original receipts and adds nothing.
    core.arm(Fault::AckLost);
    assert!(persist_decision(&mut core, &scope, &first).is_err());
    assert_eq!((decisions(&core), core.events(&selected)), (1, 2));
    let retried = persist_decision(&mut core, &scope, &first).unwrap();
    assert!(retried.replayed);
    assert_eq!(retried.events.len(), 2);
    assert_eq!((decisions(&core), core.events(&selected)), (1, 2));
    core.reopen();
    assert_eq!(
        load_decision(core.reader(), PROJECT, "decision-1")
            .unwrap()
            .unwrap(),
        first
    );
    assert_eq!(core.events(&selected), 2);

    // A different decision under the same id is a conflict: refused, nothing
    // written.
    let conflicting = w
        .route_with(
            &routing_request(Some("task-1")),
            0,
            &std::collections::BTreeMap::new(),
            "decision-1",
            Some("decision-0".into()),
        )
        .unwrap()
        .decision;
    assert!(matches!(
        persist_decision(&mut core, &scope, &conflicting),
        Err(RoutingPersistError::Port(HarnessError::Conflict))
    ));
    assert_eq!((decisions(&core), core.events(&selected)), (1, 2));
}

// ---- repair: ROUTING_REQUESTED through an admitted request reference (F7) ----------

#[test]
fn routing_requested_is_published_from_the_admitted_request_content() {
    let mut core = CoreHarness::new("routing-requested");
    let scope = CoreHarness::scope();
    let request = routing_request(Some("task-1"));
    let requested = [EventType::RoutingRequested];

    // Not admitted by the composition's resolver: nothing is persisted.
    assert!(matches!(
        persist_request(&mut core, &scope, &request),
        Err(RoutingPersistError::Port(HarnessError::ContentMismatch))
    ));
    assert_eq!(core.events(&requested), 0);

    core.admit_request(PROJECT, &request);
    let receipt = persist_request(&mut core, &scope, &request).unwrap();
    assert_eq!(receipt.events.len(), 1);
    assert_eq!(receipt.events[0].origin, EventProposalOriginV1::Routing);
    let event = core
        .reader()
        .list_events(&EventQuery {
            project_id: PROJECT.into(),
            subject: None,
            event_types: Some(requested.to_vec()),
            limit: 10,
        })
        .unwrap()
        .remove(0);
    let body = crate::wire::encode_routing_request(&request);
    assert_eq!(event.payload.reference, request_reference("request-1"));
    assert_eq!(
        event.payload.digest,
        format!("sha256:{}", sha256_hex(body.as_bytes()))
    );
    assert_eq!(
        (event.subject.id.as_str(), event.correlation_id.as_str()),
        ("task-1", "request-1")
    );

    // An exact retry is the same receipt; nothing is duplicated.
    let again = persist_request(&mut core, &scope, &request).unwrap();
    assert!(again.replayed);
    assert_eq!(again.events, receipt.events);
    assert_eq!(core.events(&requested), 1);

    // Another project cannot ride on this admission; a request whose body
    // differs from the admitted one is refused; so is one without a task.
    let other_project = crate::persistence::EventScope {
        project_id: "project-other".into(),
        ..CoreHarness::scope()
    };
    assert!(matches!(
        persist_request(&mut core, &other_project, &request),
        Err(RoutingPersistError::Port(HarnessError::ContentMismatch))
    ));
    let altered = crate::RoutingRequest::new(
        crate::RoutingRequestNonTemporalCore::try_new(
            "request-1".into(),
            Some("task-1".into()),
            crate::RoutingRequestRole::Implementer,
            crate::RoutingTaskClass::FrontierImplementation,
            crate::RoutingQualityFloor::Frontier,
            vec!["coding".into(), "extra".into()],
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap(),
        None,
    );
    assert!(matches!(
        persist_request(&mut core, &scope, &altered),
        Err(RoutingPersistError::Port(HarnessError::ContentMismatch))
    ));
    assert!(matches!(
        persist_request(&mut core, &scope, &routing_request(None)),
        Err(RoutingPersistError::TaskIdRequired)
    ));
    assert_eq!(core.events(&requested), 1);

    // The request event precedes the decision it asked for.
    let w = crate::router_tests::world(&specs());
    persist_decision(&mut core, &scope, &w.route(&request)).unwrap();
    let all = core
        .reader()
        .list_events(&EventQuery {
            project_id: PROJECT.into(),
            subject: None,
            event_types: None,
            limit: 10,
        })
        .unwrap();
    let kinds: Vec<_> = all.iter().map(|e| e.event_type).collect();
    let at = |t| kinds.iter().position(|k| *k == t).unwrap();
    assert!(at(EventType::RoutingRequested) < at(EventType::RoutingDecided));
}

#[test]
fn sha256_matches_the_standard_vectors_and_state_digests() {
    for (input, expected) in [
        (
            "",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            "abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
    ] {
        assert_eq!(sha256_hex(input.as_bytes()), expected, "{input:?}");
    }
    assert_eq!(
        sha256_hex(&vec![b'a'; 1_000_000]),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
    // 55, 56, 63, 64 and 65 bytes straddle the padding boundaries.
    for n in [55, 56, 63, 64, 65, 119, 120, 128] {
        let bytes: Vec<u8> = (0..n as u8).collect();
        let _ = sha256_hex(&bytes);
    }
    // State computes record digests with its own SHA-256: they must agree.
    let w = crate::router_tests::world(&specs());
    let decision = w.route(&routing_request(Some("task-1")));
    let record = receipts_state::DecisionRecordV1::parse(
        PROJECT,
        DecisionRecordKind::RoutingDecision,
        decision.decision_id(),
        &crate::wire::encode_routing_decision(&decision),
    )
    .unwrap();
    assert_eq!(
        sha256_hex(record.record_json().as_bytes()),
        record.record_sha256()
    );
}
