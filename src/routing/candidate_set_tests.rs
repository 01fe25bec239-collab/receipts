use std::collections::BTreeSet;

use crate::intelligence::{
    LifecycleEvidence, LifecycleState, LifecycleTransition, ModelIntelligenceService,
};
use crate::policy_eligibility::ModelRoutingDateTimeV1;
use crate::registry::{ModelId, Observation, ProviderId, RuntimeId};
use crate::{
    EvidenceConfidence, EvidenceSourceRef, EvidenceSourceRefType, RegistryCandidateIdentity,
    enumerate_registry_candidates,
};

fn p(value: &str) -> ProviderId {
    ProviderId::try_new(value.into()).unwrap()
}

fn m(value: &str) -> ModelId {
    ModelId::try_new(value.into()).unwrap()
}

fn r(value: &str) -> RuntimeId {
    RuntimeId::try_new(value.into()).unwrap()
}

fn observation() -> Observation {
    Observation {
        confidence: EvidenceConfidence::Unverified,
        source_ref: Some(
            EvidenceSourceRef::try_new(
                EvidenceSourceRefType::ArtifactId,
                "synthetic-candidate-set-evidence".into(),
                None,
                None,
            )
            .unwrap(),
        ),
        observed_at: ModelRoutingDateTimeV1::try_new("2026-09-13T00:00:00Z".into()).unwrap(),
    }
}

fn fixture(
    providers: &[&str],
    models: &[(&str, &str)],
    runtimes: &[(&str, &str)],
    associations: &[(&str, &str, &str)],
) -> ModelIntelligenceService {
    let mut service = ModelIntelligenceService::new();
    for provider in providers {
        service.insert_provider(p(provider), observation()).unwrap();
    }
    for (provider, model) in models {
        service
            .discover_model(p(provider), m(model), observation())
            .unwrap();
    }
    for (provider, runtime) in runtimes {
        service
            .insert_runtime(p(provider), r(runtime), observation())
            .unwrap();
    }
    for (provider, model, runtime) in associations {
        service
            .associate_runtime(p(provider), m(model), r(runtime), observation())
            .unwrap();
    }
    service
}

fn triples(candidates: &[RegistryCandidateIdentity]) -> Vec<(&str, &str, &str)> {
    candidates
        .iter()
        .map(|candidate| {
            (
                candidate.provider_id().as_str(),
                candidate.model_id().as_str(),
                candidate.runtime_id().as_str(),
            )
        })
        .collect()
}

fn one_candidate() -> ModelIntelligenceService {
    fixture(&["P"], &[("P", "M")], &[("P", "R")], &[("P", "M", "R")])
}

fn multiple_candidates(reverse: bool) -> ModelIntelligenceService {
    let mut providers = ["P2", "P1"];
    let mut models = [("P2", "M2"), ("P1", "M2"), ("P2", "M1"), ("P1", "M1")];
    let mut runtimes = [("P2", "R2"), ("P1", "R2"), ("P2", "R1"), ("P1", "R1")];
    let mut associations = [
        ("P2", "M2", "R1"),
        ("P1", "M2", "R2"),
        ("P2", "M1", "R2"),
        ("P1", "M1", "R2"),
        ("P1", "M1", "R1"),
    ];
    if reverse {
        providers.reverse();
        models.reverse();
        runtimes.reverse();
        associations.reverse();
    }
    fixture(&providers, &models, &runtimes, &associations)
}

#[test]
fn empty_registry_yields_no_candidates() {
    let service = ModelIntelligenceService::new();
    assert!(enumerate_registry_candidates(service.registry()).is_empty());
}

#[test]
fn model_without_associations_yields_no_candidates() {
    let service = fixture(&["P"], &[("P", "M")], &[], &[]);
    assert!(service.registry().model("P", "M").is_some());
    assert!(enumerate_registry_candidates(service.registry()).is_empty());
}

#[test]
fn one_association_preserves_exact_typed_identities() {
    let service = one_candidate();
    let candidates = enumerate_registry_candidates(service.registry());
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].provider_id(), &p("P"));
    assert_eq!(candidates[0].model_id(), &m("M"));
    assert_eq!(candidates[0].runtime_id(), &r("R"));
}

#[test]
fn only_actual_runtime_associations_contribute_candidates() {
    let service = fixture(
        &["P"],
        &[("P", "M")],
        &[("P", "R3"), ("P", "R2"), ("P", "R1")],
        &[("P", "M", "R3"), ("P", "M", "R1")],
    );
    assert!(service.registry().runtime("P", "R2").is_some());
    assert_eq!(
        triples(&enumerate_registry_candidates(service.registry())),
        [("P", "M", "R1"), ("P", "M", "R3")]
    );
}

#[test]
fn candidates_are_ordered_by_provider_then_model_then_runtime() {
    let service = multiple_candidates(false);
    assert_eq!(
        triples(&enumerate_registry_candidates(service.registry())),
        [
            ("P1", "M1", "R1"),
            ("P1", "M1", "R2"),
            ("P1", "M2", "R2"),
            ("P2", "M1", "R2"),
            ("P2", "M2", "R1"),
        ]
    );
}

#[test]
fn insertion_order_does_not_change_candidate_sequence() {
    let a = multiple_candidates(false);
    let b = multiple_candidates(true);
    assert_eq!(a.registry(), b.registry());
    assert_eq!(
        enumerate_registry_candidates(a.registry()),
        enumerate_registry_candidates(b.registry())
    );
}

#[test]
fn models_and_runtimes_do_not_form_a_synthetic_cross_product() {
    let service = fixture(
        &["P"],
        &[("P", "M2"), ("P", "M1")],
        &[("P", "R3"), ("P", "R2"), ("P", "R1")],
        &[("P", "M2", "R2"), ("P", "M1", "R3"), ("P", "M1", "R1")],
    );
    assert_eq!(
        triples(&enumerate_registry_candidates(service.registry())),
        [("P", "M1", "R1"), ("P", "M1", "R3"), ("P", "M2", "R2")]
    );
}

#[test]
fn legal_associations_emit_no_duplicate_triples() {
    let service = multiple_candidates(false);
    let candidates = enumerate_registry_candidates(service.registry());
    let unique: BTreeSet<_> = candidates
        .iter()
        .map(|candidate| {
            (
                candidate.provider_id(),
                candidate.model_id(),
                candidate.runtime_id(),
            )
        })
        .collect();
    assert_eq!(candidates.len(), 5);
    assert_eq!(candidates.len(), unique.len());
}

#[test]
fn lifecycle_transition_does_not_filter_raw_candidates() {
    let mut service = one_candidate();
    assert_eq!(
        service
            .registry()
            .model("P", "M")
            .unwrap()
            .lifecycle_state(),
        LifecycleState::Discovered
    );
    let before = enumerate_registry_candidates(service.registry());
    assert_eq!(triples(&before), [("P", "M", "R")]);
    service
        .try_transition(
            "P",
            "M",
            LifecycleTransition {
                target: LifecycleState::Unassessed,
                evidence: LifecycleEvidence::ReadyForAssessment,
                observation: observation(),
            },
        )
        .unwrap();
    assert_eq!(
        service
            .registry()
            .model("P", "M")
            .unwrap()
            .lifecycle_state(),
        LifecycleState::Unassessed
    );
    assert_eq!(before, enumerate_registry_candidates(service.registry()));
}

#[test]
fn repeated_enumeration_preserves_exact_registry_snapshot() {
    let service = multiple_candidates(false);
    let before = service.registry().clone();
    let first = enumerate_registry_candidates(service.registry());
    assert_eq!(service.registry(), &before);
    assert_eq!(first, enumerate_registry_candidates(service.registry()));
    assert_eq!(service.registry(), &before);
}

#[test]
fn exact_identity_order_preserves_case_and_whitespace() {
    let service = fixture(
        &["p", "P", " P"],
        &[("p", "m"), ("P", "m"), ("P", "M"), (" P", " M ")],
        &[("p", "r"), ("P", "r"), ("P", "R"), (" P", " R ")],
        &[
            ("p", "m", "r"),
            ("P", "m", "r"),
            ("P", "M", "r"),
            ("P", "M", "R"),
            (" P", " M ", " R "),
        ],
    );
    assert_eq!(
        triples(&enumerate_registry_candidates(service.registry())),
        [
            (" P", " M ", " R "),
            ("P", "M", "R"),
            ("P", "M", "r"),
            ("P", "m", "r"),
            ("p", "m", "r"),
        ]
    );
}
