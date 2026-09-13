use crate::intelligence::ModelIntelligenceService;
use crate::policy_eligibility::ModelRoutingDateTimeV1;
use crate::registry::{ModelId, Observation, ProviderId, RuntimeId};
use crate::{
    CandidateIdentityConstraintOutcome,
    CandidateIdentityConstraintOutcome::{Rejected, Satisfied},
    CandidateIdentityConstraintRejection::*,
    EvidenceConfidence, EvidenceSourceRef, EvidenceSourceRefType, RegistryCandidateIdentity,
    RoutingRequestConstraints, enumerate_registry_candidates,
    evaluate_candidate_identity_constraints,
};

fn candidate(provider: &str, model: &str, runtime: &str) -> RegistryCandidateIdentity {
    let provider = ProviderId::try_new(provider.into()).unwrap();
    let model = ModelId::try_new(model.into()).unwrap();
    let runtime = RuntimeId::try_new(runtime.into()).unwrap();
    let observation = Observation {
        confidence: EvidenceConfidence::Unverified,
        source_ref: Some(
            EvidenceSourceRef::try_new(
                EvidenceSourceRefType::ArtifactId,
                "synthetic-identity-constraint-evidence".into(),
                None,
                None,
            )
            .unwrap(),
        ),
        observed_at: ModelRoutingDateTimeV1::try_new("2026-09-13T00:00:00Z".into()).unwrap(),
    };
    let mut service = ModelIntelligenceService::new();
    service
        .insert_provider(provider.clone(), observation.clone())
        .unwrap();
    service
        .discover_model(provider.clone(), model.clone(), observation.clone())
        .unwrap();
    service
        .insert_runtime(provider.clone(), runtime.clone(), observation.clone())
        .unwrap();
    service
        .associate_runtime(provider, model, runtime, observation)
        .unwrap();
    let candidates = enumerate_registry_candidates(service.registry());
    assert_eq!(candidates.len(), 1);
    candidates.into_iter().next().unwrap()
}

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

fn check(constraints: RoutingRequestConstraints, expected: CandidateIdentityConstraintOutcome) {
    assert_eq!(
        evaluate_candidate_identity_constraints(
            &candidate("provider-a", "model-a", "runtime-a"),
            Some(&constraints),
        ),
        expected,
    );
}

#[test]
fn t01_no_constraints() {
    assert_eq!(
        evaluate_candidate_identity_constraints(
            &candidate("provider-a", "model-a", "runtime-a"),
            None
        ),
        Satisfied
    );
}

#[test]
fn t02_distinct_provider_pass() {
    check(
        constraints(Some("provider-b"), None, None, None, None),
        Satisfied,
    );
}

#[test]
fn t03_distinct_provider_reject() {
    check(
        constraints(Some("provider-a"), None, None, None, None),
        Rejected(DistinctProviderFromViolation),
    );
}

#[test]
fn t04_avoid_list_pass() {
    check(
        constraints(None, Some(&["provider-b", "provider-c"]), None, None, None),
        Satisfied,
    );
}

#[test]
fn t05_avoid_list_exact_reject() {
    check(
        constraints(None, Some(&["provider-b", "provider-a"]), None, None, None),
        Rejected(AvoidedProvider),
    );
}

#[test]
fn t06_duplicate_avoid_entries() {
    let c = candidate("provider-a", "model-a", "runtime-a");
    let constraints = constraints(
        None,
        Some(&["provider-a", "provider-a", "provider-a"]),
        None,
        None,
        None,
    );
    for _ in 0..10 {
        assert_eq!(
            evaluate_candidate_identity_constraints(&c, Some(&constraints)),
            Rejected(AvoidedProvider)
        );
    }
}

#[test]
fn t07_pinned_provider_match() {
    check(
        constraints(None, None, Some("provider-a"), None, None),
        Satisfied,
    );
}

#[test]
fn t08_pinned_provider_case_difference() {
    check(
        constraints(None, None, Some("Provider-a"), None, None),
        Rejected(PinnedProviderMismatch),
    );
}

#[test]
fn t09_pinned_provider_whitespace_difference() {
    check(
        constraints(None, None, Some("provider-a "), None, None),
        Rejected(PinnedProviderMismatch),
    );
}

#[test]
fn t10_pinned_model_match() {
    check(
        constraints(None, None, None, Some("model-a"), None),
        Satisfied,
    );
}

#[test]
fn t11_pinned_model_case_difference() {
    check(
        constraints(None, None, None, Some("Model-a"), None),
        Rejected(PinnedModelMismatch),
    );
}

#[test]
fn t12_provider_and_model_both_match() {
    check(
        constraints(None, None, Some("provider-a"), Some("model-a"), None),
        Satisfied,
    );
}

#[test]
fn t13_provider_and_model_both_fail() {
    check(
        constraints(None, None, Some("provider-b"), Some("model-b"), None),
        Rejected(PinnedProviderMismatch),
    );
}

#[test]
fn t14_provider_matches_model_fails() {
    check(
        constraints(None, None, Some("provider-a"), Some("model-b"), None),
        Rejected(PinnedModelMismatch),
    );
}

#[test]
fn t15_combined_all_pass() {
    check(
        constraints(
            Some("provider-old"),
            Some(&["provider-b", "provider-c"]),
            Some("provider-a"),
            Some("model-a"),
            None,
        ),
        Satisfied,
    );
}

#[test]
fn t16_distinct_precedes_avoid_and_pins() {
    for (provider, model) in [(None, None), (Some("provider-b"), Some("model-b"))] {
        check(
            constraints(
                Some("provider-a"),
                Some(&["provider-a"]),
                provider,
                model,
                None,
            ),
            Rejected(DistinctProviderFromViolation),
        );
    }
}

#[test]
fn t17_avoid_precedes_pins() {
    for model in [None, Some("model-b")] {
        check(
            constraints(None, Some(&["provider-a"]), Some("provider-b"), model, None),
            Rejected(AvoidedProvider),
        );
    }
}

#[test]
fn t18_provider_pin_precedes_model_pin() {
    check(
        constraints(
            Some("provider-old"),
            Some(&["provider-c"]),
            Some("provider-b"),
            Some("model-b"),
            None,
        ),
        Rejected(PinnedProviderMismatch),
    );
}

#[test]
fn t19_max_cost_only() {
    for cost in [
        None,
        Some(0.0),
        Some(-0.0),
        Some(17.25),
        Some(f64::MIN),
        Some(f64::MAX),
    ] {
        check(constraints(None, None, None, None, cost), Satisfied);
    }
}

#[test]
fn t20_max_cost_preserves_every_identity_outcome() {
    for cost in [None, Some(0.0), Some(-17.25), Some(f64::MAX)] {
        for (distinct, avoid, provider, model, expected) in [
            (
                Some("provider-a"),
                None,
                None,
                None,
                Rejected(DistinctProviderFromViolation),
            ),
            (
                None,
                Some(["provider-a"].as_slice()),
                None,
                None,
                Rejected(AvoidedProvider),
            ),
            (
                None,
                None,
                Some("provider-b"),
                None,
                Rejected(PinnedProviderMismatch),
            ),
            (
                None,
                None,
                None,
                Some("model-b"),
                Rejected(PinnedModelMismatch),
            ),
            (
                Some("provider-old"),
                Some(["provider-b"].as_slice()),
                Some("provider-a"),
                Some("model-a"),
                Satisfied,
            ),
        ] {
            check(
                constraints(distinct, avoid, provider, model, cost),
                expected,
            );
        }
    }
}

#[test]
fn t21_exact_opaque_ids_on_every_axis() {
    for (actual, different) in [
        ("provider-A", "provider-a"),
        ("provider-x ", "provider-x"),
        ("provider-x", "provider"),
        ("provider-x", "x"),
        ("é", "e\u{301}"),
    ] {
        let c = candidate(actual, "model-a", "runtime-a");
        for (constraints, expected) in [
            (
                constraints(Some(different), None, None, None, None),
                Satisfied,
            ),
            (
                constraints(None, Some(&[different]), None, None, None),
                Satisfied,
            ),
            (
                constraints(None, None, Some(different), None, None),
                Rejected(PinnedProviderMismatch),
            ),
            (constraints(None, None, Some(actual), None, None), Satisfied),
        ] {
            assert_eq!(
                evaluate_candidate_identity_constraints(&c, Some(&constraints)),
                expected
            );
        }
    }
    for (actual, different) in [
        ("model-A", "model-a"),
        ("model-a ", "model-a"),
        ("model-10", "model-010"),
        ("model-10", "model-9"),
        ("model-a", "model"),
        ("model-a", "a"),
        ("é", "e\u{301}"),
    ] {
        let c = candidate("provider-a", actual, "runtime-a");
        for (pin, expected) in [
            (actual, Satisfied),
            (different, Rejected(PinnedModelMismatch)),
        ] {
            assert_eq!(
                evaluate_candidate_identity_constraints(
                    &c,
                    Some(&constraints(None, None, None, Some(pin), None))
                ),
                expected
            );
        }
    }
}

#[test]
fn t22_inputs_unchanged() {
    let c = candidate("provider-a", "model-a", "runtime-a");
    for constraints in [
        constraints(
            Some("provider-a"),
            Some(&["provider-b", "provider-a", "provider-a"]),
            Some("Provider-a "),
            Some("Model-a "),
            Some(-17.25),
        ),
        constraints(
            Some("provider-old"),
            Some(&["provider-b", "provider-b"]),
            Some("provider-a"),
            Some("model-a"),
            Some(17.25),
        ),
    ] {
        let before_candidate = c.clone();
        let before_constraints = constraints.clone();
        for _ in 0..10 {
            evaluate_candidate_identity_constraints(&c, Some(&constraints));
            assert_eq!(c, before_candidate);
            assert_eq!(constraints, before_constraints);
        }
    }
}

#[test]
fn t23_repeated_determinism() {
    let c = candidate("provider-a", "model-a", "runtime-a");
    for constraints in [
        None,
        Some(constraints(None, None, None, None, None)),
        Some(constraints(Some("provider-a"), None, None, None, None)),
        Some(constraints(None, Some(&["provider-a"]), None, None, None)),
        Some(constraints(None, None, Some("provider-b"), None, None)),
        Some(constraints(None, None, None, Some("model-b"), None)),
    ] {
        let first = evaluate_candidate_identity_constraints(&c, constraints.as_ref());
        for _ in 0..100 {
            assert_eq!(
                evaluate_candidate_identity_constraints(&c, constraints.as_ref()),
                first
            );
        }
    }
}

#[test]
fn empty_avoid_list_and_runtime_identity_do_not_constrain() {
    let constraints = constraints(None, Some(&[]), Some("provider-a"), Some("model-a"), None);
    for runtime in ["runtime-a", "runtime-b", "Runtime-a "] {
        assert_eq!(
            evaluate_candidate_identity_constraints(
                &candidate("provider-a", "model-a", runtime),
                Some(&constraints)
            ),
            Satisfied
        );
    }
}
