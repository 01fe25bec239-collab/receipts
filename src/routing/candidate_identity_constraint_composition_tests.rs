use crate::intelligence::{LifecycleState, ModelIntelligenceService};
use crate::policy_eligibility::ModelRoutingDateTimeV1;
use crate::registry::{ModelId, Observation, ProviderId, Registry, RuntimeId};
use crate::{
    CandidateIdentityConstraintOutcome::{Rejected, Satisfied},
    CandidateIdentityConstraintRejection::*,
    EvidenceConfidence, EvidenceSourceRef, EvidenceSourceRefType,
    RegistryCandidateIdentityConstraintAssessment, RoutingRequestConstraints,
    enumerate_registry_candidates, evaluate_candidate_identity_constraints,
    evaluate_registry_candidate_identity_constraints,
};

fn fixture(rows: &[(&str, &str, &str)]) -> ModelIntelligenceService {
    let observation = Observation {
        confidence: EvidenceConfidence::Unverified,
        source_ref: Some(
            EvidenceSourceRef::try_new(
                EvidenceSourceRefType::ArtifactId,
                "synthetic-identity-composition-evidence".into(),
                None,
                None,
            )
            .unwrap(),
        ),
        observed_at: ModelRoutingDateTimeV1::try_new("2026-09-13T00:00:00Z".into()).unwrap(),
    };
    let mut service = ModelIntelligenceService::new();
    for &(provider, model, runtime) in rows {
        let p = ProviderId::try_new(provider.into()).unwrap();
        let m = ModelId::try_new(model.into()).unwrap();
        let r = RuntimeId::try_new(runtime.into()).unwrap();
        if service.registry().provider(provider).is_none() {
            service
                .insert_provider(p.clone(), observation.clone())
                .unwrap();
        }
        if service.registry().model(provider, model).is_none() {
            service
                .discover_model(p.clone(), m.clone(), observation.clone())
                .unwrap();
        }
        if service.registry().runtime(provider, runtime).is_none() {
            service
                .insert_runtime(p.clone(), r.clone(), observation.clone())
                .unwrap();
        }
        service
            .associate_runtime(p, m, r, observation.clone())
            .unwrap();
    }
    service
}

const ROWS: [(&str, &str, &str); 7] = [
    ("provider-b", "model-z", "runtime-a"),
    ("provider-a", "model-z", "runtime-2"),
    ("provider-a", "model-a", "runtime-a"),
    ("provider-a", "model-a", "runtime-A"),
    ("provider-b", "model-a", "runtime-10"),
    ("provider-a", "model-a", "runtime-2"),
    ("provider-a", "model-a", "runtime-10"),
];

fn constraints(
    distinct: Option<&str>,
    avoid: Option<&[&str]>,
    provider: Option<&str>,
    model: Option<&str>,
    cost: Option<f64>,
) -> RoutingRequestConstraints {
    RoutingRequestConstraints::try_new(
        distinct.map(String::from),
        avoid.map(|ids| ids.iter().map(|id| String::from(*id)).collect()),
        model.map(String::from),
        provider.map(String::from),
        cost,
    )
    .unwrap()
}

fn mixed_constraints() -> RoutingRequestConstraints {
    constraints(
        Some("provider-b"),
        None,
        Some("provider-a"),
        Some("model-a"),
        None,
    )
}

fn assert_delegation(
    registry: &Registry,
    constraints: Option<&RoutingRequestConstraints>,
) -> Vec<RegistryCandidateIdentityConstraintAssessment> {
    let raw = enumerate_registry_candidates(registry);
    let assessments = evaluate_registry_candidate_identity_constraints(registry, constraints);
    assert_eq!(assessments.len(), raw.len());
    for (assessment, candidate) in assessments.iter().zip(&raw) {
        assert_eq!(assessment.candidate(), candidate);
        assert_eq!(
            assessment.outcome(),
            evaluate_candidate_identity_constraints(candidate, constraints)
        );
    }
    assessments
}

#[test]
fn t01_empty_registry() {
    let service = ModelIntelligenceService::new();
    let assessments = assert_delegation(service.registry(), None);
    assert_eq!(assessments, []);
    assert_eq!(assessments.len(), 0);
}

#[test]
fn t02_one_candidate_without_constraints() {
    let service = fixture(&[("P", "M", "R")]);
    let assessments = assert_delegation(service.registry(), None);
    assert_eq!(assessments.len(), 1);
    assert_eq!(assessments[0].candidate().provider_id().as_str(), "P");
    assert_eq!(assessments[0].candidate().model_id().as_str(), "M");
    assert_eq!(assessments[0].candidate().runtime_id().as_str(), "R");
    assert_eq!(assessments[0].outcome(), Satisfied);
}

#[test]
fn t03_multiple_candidates_without_constraints() {
    let service = fixture(&ROWS);
    let assessments = assert_delegation(service.registry(), None);
    assert_eq!(assessments.len(), ROWS.len());
    assert!(assessments.iter().all(|a| a.outcome() == Satisfied));
}

#[test]
fn t04_assessment_count_equals_raw_count() {
    let service = fixture(&ROWS);
    let assessments = evaluate_registry_candidate_identity_constraints(
        service.registry(),
        Some(&mixed_constraints()),
    );
    assert_eq!(
        assessments.len(),
        enumerate_registry_candidates(service.registry()).len()
    );
}

#[test]
fn t05_rejected_candidates_retained_even_when_all_reject() {
    let service = fixture(&ROWS);
    let constraints = constraints(None, Some(&["provider-a", "provider-b"]), None, None, None);
    let assessments = assert_delegation(service.registry(), Some(&constraints));
    assert_eq!(assessments.len(), ROWS.len());
    assert!(
        assessments
            .iter()
            .all(|a| a.outcome() == Rejected(AvoidedProvider))
    );
}

#[test]
fn t06_direct_outcome_equivalence_and_precedence() {
    let service = fixture(&ROWS);
    for constraints in [
        constraints(
            Some("provider-a"),
            Some(&["provider-a", "provider-b"]),
            Some("absent"),
            Some("absent"),
            None,
        ),
        constraints(
            None,
            Some(&["provider-a"]),
            Some("absent"),
            Some("absent"),
            None,
        ),
        constraints(None, None, Some("provider-b"), Some("model-a"), None),
        mixed_constraints(),
    ] {
        assert_delegation(service.registry(), Some(&constraints));
    }
}

#[test]
fn t07_mixed_outcomes_preserve_order() {
    let service = fixture(&ROWS);
    let assessments = assert_delegation(service.registry(), Some(&mixed_constraints()));
    assert_eq!(
        assessments.iter().map(|a| a.outcome()).collect::<Vec<_>>(),
        [
            Satisfied,
            Satisfied,
            Satisfied,
            Satisfied,
            Rejected(PinnedModelMismatch),
            Rejected(DistinctProviderFromViolation),
            Rejected(DistinctProviderFromViolation),
        ]
    );
    // Rejections before satisfaction must not be moved behind it either.
    let constraints = constraints(None, Some(&["provider-a"]), None, None, None);
    let assessments = assert_delegation(service.registry(), Some(&constraints));
    assert_eq!(
        assessments.first().unwrap().outcome(),
        Rejected(AvoidedProvider)
    );
    assert_eq!(assessments.last().unwrap().outcome(), Satisfied);
}

#[test]
fn t08_canonical_lexical_order_preserved() {
    let service = fixture(&ROWS);
    let assessments = assert_delegation(service.registry(), Some(&mixed_constraints()));
    let triples: Vec<_> = assessments
        .iter()
        .map(|a| {
            (
                a.candidate().provider_id().as_str(),
                a.candidate().model_id().as_str(),
                a.candidate().runtime_id().as_str(),
            )
        })
        .collect();
    assert_eq!(
        triples,
        [
            ("provider-a", "model-a", "runtime-10"),
            ("provider-a", "model-a", "runtime-2"),
            ("provider-a", "model-a", "runtime-A"),
            ("provider-a", "model-a", "runtime-a"),
            ("provider-a", "model-z", "runtime-2"),
            ("provider-b", "model-a", "runtime-10"),
            ("provider-b", "model-z", "runtime-a"),
        ]
    );
}

#[test]
fn t09_insertion_order_independent() {
    let a = fixture(&ROWS);
    let mut reversed = ROWS;
    reversed.reverse();
    let b = fixture(&reversed);
    assert_eq!(a.registry(), b.registry());
    let constraints = mixed_constraints();
    assert_eq!(
        evaluate_registry_candidate_identity_constraints(a.registry(), Some(&constraints)),
        evaluate_registry_candidate_identity_constraints(b.registry(), Some(&constraints)),
    );
}

#[test]
fn t10_duplicate_avoid_input_does_not_duplicate_assessments() {
    let service = fixture(&ROWS);
    let duplicated = constraints(
        None,
        Some(&["provider-b", "provider-b", "provider-b"]),
        None,
        None,
        None,
    );
    let single = constraints(None, Some(&["provider-b"]), None, None, None);
    let assessments = assert_delegation(service.registry(), Some(&duplicated));
    assert_eq!(
        assessments,
        assert_delegation(service.registry(), Some(&single))
    );
    let unique: std::collections::BTreeSet<_> = assessments
        .iter()
        .map(|a| {
            (
                a.candidate().provider_id(),
                a.candidate().model_id(),
                a.candidate().runtime_id(),
            )
        })
        .collect();
    assert_eq!(unique.len(), assessments.len());
}

#[test]
fn t11_max_cost_neutrality() {
    let service = fixture(&ROWS);
    for (distinct, avoid, provider, model) in [
        (None, None, None, None),
        (
            Some("provider-b"),
            None,
            Some("provider-a"),
            Some("model-a"),
        ),
        (
            None,
            Some(["provider-b"].as_slice()),
            Some("provider-a"),
            Some("model-a"),
        ),
        (None, None, Some("provider-b"), Some("model-a")),
    ] {
        let baseline = assert_delegation(
            service.registry(),
            Some(&constraints(distinct, avoid, provider, model, None)),
        );
        for cost in [0.0, -0.0, 17.25, -17.25, f64::MIN, f64::MAX] {
            assert_eq!(
                baseline,
                assert_delegation(
                    service.registry(),
                    Some(&constraints(distinct, avoid, provider, model, Some(cost)))
                )
            );
        }
    }
}

#[test]
fn t12_registry_unchanged() {
    let service = fixture(&ROWS);
    let before = service.registry().clone();
    for _ in 0..10 {
        assert_delegation(service.registry(), None);
        assert_delegation(service.registry(), Some(&mixed_constraints()));
        assert_eq!(service.registry(), &before);
    }
}

#[test]
fn t13_constraints_unchanged() {
    let service = fixture(&ROWS);
    let constraints = constraints(
        Some("provider-b"),
        Some(&["provider-b", "provider-b"]),
        Some("provider-a"),
        Some("model-a"),
        Some(-17.25),
    );
    let before = constraints.clone();
    for _ in 0..10 {
        assert_delegation(service.registry(), Some(&constraints));
        assert_eq!(constraints, before);
    }
}

#[test]
fn t14_repeated_determinism() {
    let service = fixture(&ROWS);
    for constraints in [None, Some(mixed_constraints())] {
        let first = evaluate_registry_candidate_identity_constraints(
            service.registry(),
            constraints.as_ref(),
        );
        for _ in 0..100 {
            assert_eq!(
                first,
                evaluate_registry_candidate_identity_constraints(
                    service.registry(),
                    constraints.as_ref()
                )
            );
        }
    }
}

#[test]
fn t15_runtime_ids_remain_distinct_without_new_constraint_semantics() {
    let service = fixture(&[
        ("P", "M", "runtime-a"),
        ("P", "M", "runtime-A"),
        ("P", "M", " runtime-a "),
    ]);
    for (pin, expected) in [("M", Satisfied), ("m", Rejected(PinnedModelMismatch))] {
        let constraints = constraints(None, None, Some("P"), Some(pin), None);
        let assessments = assert_delegation(service.registry(), Some(&constraints));
        assert_eq!(
            assessments
                .iter()
                .map(|a| a.candidate().runtime_id().as_str())
                .collect::<Vec<_>>(),
            [" runtime-a ", "runtime-A", "runtime-a"]
        );
        assert!(assessments.iter().all(|a| a.outcome() == expected));
    }
}

#[test]
fn t16_non_routable_lifecycle_does_not_filter() {
    let service = fixture(&ROWS);
    for model in service.registry().models() {
        assert_eq!(model.lifecycle_state(), LifecycleState::Discovered);
        assert!(!model.lifecycle_state().passes_normal_lifecycle_gate());
    }
    let assessments = assert_delegation(service.registry(), None);
    assert_eq!(assessments.len(), ROWS.len());
    assert!(assessments.iter().all(|a| a.outcome() == Satisfied));
    assert_delegation(service.registry(), Some(&mixed_constraints()));
}

#[test]
fn exact_case_and_whitespace_identities_are_not_normalized() {
    let service = fixture(&[
        ("p", "M", "R"),
        ("P", "m", "R"),
        ("P", "M ", "R"),
        ("P", "M", "R"),
        (" P", "M", "R"),
    ]);
    let constraints = constraints(None, None, Some("P"), Some("M"), None);
    let assessments = assert_delegation(service.registry(), Some(&constraints));
    assert_eq!(
        assessments
            .iter()
            .map(|a| (
                a.candidate().provider_id().as_str(),
                a.candidate().model_id().as_str(),
                a.outcome(),
            ))
            .collect::<Vec<_>>(),
        [
            (" P", "M", Rejected(PinnedProviderMismatch)),
            ("P", "M", Satisfied),
            ("P", "M ", Rejected(PinnedModelMismatch)),
            ("P", "m", Rejected(PinnedModelMismatch)),
            ("p", "M", Rejected(PinnedProviderMismatch)),
        ]
    );
}
