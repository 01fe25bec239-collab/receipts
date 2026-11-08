use receipts_runtime_adapters::{
    CapabilityStatus, CodexCapabilityEvidence, CodexCapabilityProbeReport,
    ExtensionCapabilityReport, RuntimeCapabilities,
};
use receipts_state::{DecisionRecordKind, EventProposalOriginV1};

use super::*;
use crate::persistence::{
    RoutingPersistError, RoutingStatePort, fallback_chain_length, load_decision, persist_decision,
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
    assert_eq!(receipts.len(), 2, "ROUTING_DECIDED + EXECUTOR_SELECTED");
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
    // Decisions are immutable: the same id cannot be written twice.
    assert!(matches!(
        persist_decision(&mut core, &CoreHarness::scope(), &first),
        Err(RoutingPersistError::Port(_))
    ));
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
        .find_event_receipt(&receipts[0].event_id)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.origin, EventProposalOriginV1::Routing);
}
