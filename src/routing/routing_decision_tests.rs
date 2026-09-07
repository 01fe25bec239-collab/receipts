use crate::{
    AlternativeCandidate, AlternativeCandidateError, CapabilityEvidenceNonTemporalCore,
    DecisionConfidence, EstimatedCostClass, EvidenceConfidence, EvidenceSourceRef,
    EvidenceSourceRefError, EvidenceSourceRefType, RegistryFreshness, RoutingDecisionCoreError,
    RoutingDecisionMode, RoutingDecisionNonTemporalCore, RoutingDecisionOutcome,
    RoutingQualityFloor, RoutingScoreComponents, RoutingScoreComponentsError, SelectionReason,
};

fn minimal_decision(
    decision_id: String,
    request_id: String,
    task_id: Option<String>,
    fallback_from: Option<String>,
) -> Result<RoutingDecisionNonTemporalCore, RoutingDecisionCoreError> {
    RoutingDecisionNonTemporalCore::try_new(
        decision_id,
        request_id,
        task_id,
        RoutingDecisionOutcome::Selected,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        RegistryFreshness::new(None, None, None, false),
        RoutingDecisionMode::AutoCurrent,
        None,
        None,
        fallback_from,
    )
}

#[test]
fn closed_vocabularies_have_exact_order_and_canonical_strings() {
    assert_eq!(
        RoutingDecisionOutcome::ALL,
        [
            RoutingDecisionOutcome::Selected,
            RoutingDecisionOutcome::NoEligibleCandidate,
            RoutingDecisionOutcome::UserInputRequired,
            RoutingDecisionOutcome::Blocked,
        ]
    );
    assert_eq!(
        RoutingDecisionOutcome::ALL.map(RoutingDecisionOutcome::as_str),
        [
            "SELECTED",
            "NO_ELIGIBLE_CANDIDATE",
            "USER_INPUT_REQUIRED",
            "BLOCKED",
        ]
    );
    assert_eq!(
        EvidenceConfidence::ALL,
        [
            EvidenceConfidence::OfficialVerified,
            EvidenceConfidence::IndependentVerified,
            EvidenceConfidence::LocalEmpirical,
            EvidenceConfidence::UserDeclared,
            EvidenceConfidence::Unverified,
        ]
    );
    assert_eq!(
        EvidenceConfidence::ALL.map(EvidenceConfidence::as_str),
        [
            "OFFICIAL_VERIFIED",
            "INDEPENDENT_VERIFIED",
            "LOCAL_EMPIRICAL",
            "USER_DECLARED",
            "UNVERIFIED",
        ]
    );
    assert_eq!(
        EvidenceSourceRefType::ALL.map(EvidenceSourceRefType::as_str),
        ["REPO_PATH", "STATE_QUERY", "ARTIFACT_ID", "URL"]
    );
    assert_eq!(
        DecisionConfidence::ALL.map(DecisionConfidence::as_str),
        ["HIGH", "MEDIUM", "LOW"]
    );
    assert_eq!(
        EstimatedCostClass::ALL.map(EstimatedCostClass::as_str),
        ["LOW", "MEDIUM", "HIGH", "UNKNOWN"]
    );
    assert_eq!(
        RoutingDecisionMode::ALL.map(RoutingDecisionMode::as_str),
        ["AUTO_CURRENT", "ASK_ON_UNCERTAINTY", "USER_CONTROLLED"]
    );
}

#[test]
fn identifiers_use_unicode_character_boundaries_and_preserve_exact_text() {
    use RoutingDecisionCoreError::*;

    assert_eq!(
        minimal_decision("".into(), "r".into(), None, None),
        Err(EmptyDecisionId)
    );
    assert_eq!(
        minimal_decision("d".into(), "".into(), None, None),
        Err(EmptyRequestId)
    );
    assert_eq!(
        minimal_decision("d".into(), "r".into(), Some("".into()), None),
        Err(EmptyTaskId)
    );
    assert_eq!(
        minimal_decision("d".into(), "r".into(), None, Some("".into())),
        Err(EmptyFallbackFrom)
    );

    for text in [
        "界".into(),
        "界".repeat(200),
        "e\u{301}".repeat(100),
        " \t\n".into(),
        " Opaque/界/e\u{301} ".into(),
    ] {
        let value = minimal_decision(
            text.clone(),
            text.clone(),
            Some(text.clone()),
            Some(text.clone()),
        )
        .unwrap();
        assert_eq!(value.decision_id(), text);
        assert_eq!(value.request_id(), text);
        assert_eq!(value.task_id(), Some(text.as_str()));
        assert_eq!(value.fallback_from(), Some(text.as_str()));
    }

    for text in [
        "a".repeat(201),
        "界".repeat(201),
        format!("{}x", "e\u{301}".repeat(100)),
    ] {
        assert_eq!(
            minimal_decision(text.clone(), "r".into(), None, None),
            Err(DecisionIdTooLong)
        );
        assert_eq!(
            minimal_decision("d".into(), text.clone(), None, None),
            Err(RequestIdTooLong)
        );
        assert_eq!(
            minimal_decision("d".into(), "r".into(), Some(text.clone()), None),
            Err(TaskIdTooLong)
        );
        assert_eq!(
            minimal_decision("d".into(), "r".into(), None, Some(text)),
            Err(FallbackFromTooLong)
        );
    }

    let omitted = minimal_decision("d".into(), "r".into(), None, None).unwrap();
    assert_eq!(omitted.task_id(), None);
    assert_eq!(omitted.fallback_from(), None);
}

#[test]
fn selected_identities_are_optional_open_unbounded_strings() {
    use RoutingDecisionCoreError::*;

    let make = |provider: Option<&str>, model: Option<&str>, runtime: Option<&str>| {
        RoutingDecisionNonTemporalCore::try_new(
            "d".into(),
            "r".into(),
            None,
            RoutingDecisionOutcome::Blocked,
            provider.map(String::from),
            model.map(String::from),
            runtime.map(String::from),
            None,
            None,
            None,
            None,
            None,
            None,
            RegistryFreshness::new(None, None, None, true),
            RoutingDecisionMode::UserControlled,
            None,
            None,
            None,
        )
    };
    assert_eq!(make(Some(""), None, None), Err(EmptySelectedProvider));
    assert_eq!(make(None, Some(""), None), Err(EmptySelectedModel));
    assert_eq!(make(None, None, Some("")), Err(EmptySelectedRuntime));

    let long = "界".repeat(10_000);
    for text in [" \t\n", "future-id/Ω:v99+beta", long.as_str()] {
        let value = make(Some(text), Some(text), Some(text)).unwrap();
        assert_eq!(value.selected_provider(), Some(text));
        assert_eq!(value.selected_model(), Some(text));
        assert_eq!(value.selected_runtime(), Some(text));
    }
    let absent = make(None, None, None).unwrap();
    assert_eq!(absent.selected_provider(), None);
    assert_eq!(absent.selected_model(), None);
    assert_eq!(absent.selected_runtime(), None);
}

#[test]
fn selection_reason_preserves_open_filters_quality_floor_and_decisive_factor() {
    let filters = vec![
        "".into(),
        " \t".into(),
        "能力/Ω".into(),
        "future-filter".into(),
        "future-filter".into(),
    ];
    let reason = SelectionReason::new(
        Some(RoutingQualityFloor::Frontier),
        Some(filters.clone()),
        None,
        Some(String::new()),
    );
    assert_eq!(
        reason.quality_floor_applied(),
        Some(RoutingQualityFloor::Frontier)
    );
    assert_eq!(reason.hard_filters_passed(), Some(filters.as_slice()));
    assert_eq!(reason.score_components(), None);
    assert_eq!(reason.decisive_factor(), Some(""));

    for filters in [None, Some(vec![])] {
        let reason = SelectionReason::new(None, filters.clone(), None, None);
        assert_eq!(reason.hard_filters_passed(), filters.as_deref());
        assert_eq!(reason.decisive_factor(), None);
    }
}

#[test]
fn score_components_distinguish_absent_null_and_finite_values() {
    for state in [None, Some(None), Some(Some(-17.25)), Some(Some(f64::MAX))] {
        let values =
            RoutingScoreComponents::try_new(state, Some(Some(0.5)), state, state, state).unwrap();
        assert_eq!(values.expected_implementation_cost(), state);
        assert_eq!(values.expected_repair_cost(), state);
        assert_eq!(values.expected_review_cost(), state);
        assert_eq!(values.latency_estimate_seconds(), state);
    }
    for probability in [
        None,
        Some(None),
        Some(Some(0.0)),
        Some(Some(0.5)),
        Some(Some(1.0)),
    ] {
        let values = RoutingScoreComponents::try_new(None, probability, None, None, None).unwrap();
        assert_eq!(values.expected_rejection_probability(), probability);
    }
    for probability in [Some(Some(-f64::MIN_POSITIVE)), Some(Some(1.000_001))] {
        assert_eq!(
            RoutingScoreComponents::try_new(None, probability, None, None, None),
            Err(RoutingScoreComponentsError::ExpectedRejectionProbabilityOutOfRange)
        );
    }
}

#[test]
fn every_numeric_score_position_rejects_nonfinite_values() {
    use RoutingScoreComponentsError::*;

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            RoutingScoreComponents::try_new(Some(Some(value)), None, None, None, None),
            Err(NonFiniteExpectedImplementationCost)
        );
        assert_eq!(
            RoutingScoreComponents::try_new(None, Some(Some(value)), None, None, None),
            Err(NonFiniteExpectedRejectionProbability)
        );
        assert_eq!(
            RoutingScoreComponents::try_new(None, None, Some(Some(value)), None, None),
            Err(NonFiniteExpectedRepairCost)
        );
        assert_eq!(
            RoutingScoreComponents::try_new(None, None, None, Some(Some(value)), None),
            Err(NonFiniteExpectedReviewCost)
        );
        assert_eq!(
            RoutingScoreComponents::try_new(None, None, None, None, Some(Some(value))),
            Err(NonFiniteLatencyEstimateSeconds)
        );
    }
}

#[test]
fn evidence_preserves_open_capabilities_sources_and_nullable_sample_sizes() {
    assert_eq!(
        EvidenceSourceRef::try_new(EvidenceSourceRefType::Url, "".into(), None, None),
        Err(EvidenceSourceRefError::EmptyTarget)
    );
    for ref_type in EvidenceSourceRefType::ALL {
        let source = EvidenceSourceRef::try_new(
            ref_type,
            " \t/opaque/能力".into(),
            Some(String::new()),
            Some(String::new()),
        )
        .unwrap();
        assert_eq!(source.ref_type(), ref_type);
        assert_eq!(source.target(), " \t/opaque/能力");
        assert_eq!(source.digest(), Some(""));
        assert_eq!(source.section(), Some(""));
    }

    for capability in ["", " \t", "能力", "future.capability/v2+beta"] {
        for sample_size in [None, Some(None), Some(Some(0)), Some(Some(u64::MAX))] {
            let evidence = CapabilityEvidenceNonTemporalCore::new(
                capability.into(),
                EvidenceConfidence::Unverified,
                None,
                sample_size,
            );
            assert_eq!(evidence.capability(), capability);
            assert_eq!(evidence.confidence(), EvidenceConfidence::Unverified);
            assert_eq!(evidence.source_ref(), None);
            assert_eq!(evidence.sample_size(), sample_size);
        }
    }
}

#[test]
fn alternatives_preserve_open_ids_empty_reasons_nullable_scores_order_and_duplicates() {
    use AlternativeCandidateError::*;

    let make = |provider: &str, model: &str, runtime: &str, score| {
        AlternativeCandidate::try_new(
            provider.into(),
            model.into(),
            runtime.into(),
            score,
            String::new(),
        )
    };
    assert_eq!(make("", "m", "r", None), Err(EmptyProviderId));
    assert_eq!(make("p", "", "r", None), Err(EmptyModelId));
    assert_eq!(make("p", "m", "", None), Err(EmptyRuntimeId));
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(make("p", "m", "r", Some(Some(value))), Err(NonFiniteScore));
    }

    let long = "界".repeat(10_000);
    for text in [" \t", "future/Ω:v2", long.as_str()] {
        for score in [None, Some(None), Some(Some(-9.5)), Some(Some(f64::MAX))] {
            let candidate = make(text, text, text, score).unwrap();
            assert_eq!(candidate.provider_id(), text);
            assert_eq!(candidate.model_id(), text);
            assert_eq!(candidate.runtime_id(), text);
            assert_eq!(candidate.score(), score);
            assert_eq!(candidate.rejection_reason(), "");
        }
    }

    let first = make("first", "m1", "r1", Some(Some(-1.0))).unwrap();
    let second = make("second", "m2", "r2", None).unwrap();
    let alternatives = vec![first.clone(), second, first.clone()];
    let decision = RoutingDecisionNonTemporalCore::try_new(
        "d".into(),
        "r".into(),
        None,
        RoutingDecisionOutcome::Selected,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        Some(alternatives.clone()),
        RegistryFreshness::new(None, None, None, false),
        RoutingDecisionMode::AskOnUncertainty,
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        decision.alternative_candidates(),
        Some(alternatives.as_slice())
    );
}

#[test]
fn required_and_optional_top_level_fields_are_independent_storage_without_policy() {
    for stale in [false, true] {
        for integer in [
            None,
            Some(None),
            Some(Some(i64::MIN)),
            Some(Some(-1)),
            Some(Some(i64::MAX)),
        ] {
            let freshness = RegistryFreshness::new(integer, integer, integer, stale);
            assert_eq!(freshness.model_list_age_seconds(), integer);
            assert_eq!(freshness.capability_age_seconds(), integer);
            assert_eq!(freshness.calibration_sample_size(), integer);
            assert_eq!(freshness.stale(), stale);
        }
    }

    let evidence = vec![
        CapabilityEvidenceNonTemporalCore::new(
            "".into(),
            EvidenceConfidence::UserDeclared,
            None,
            None,
        ),
        CapabilityEvidenceNonTemporalCore::new(
            "".into(),
            EvidenceConfidence::UserDeclared,
            None,
            None,
        ),
    ];
    for outcome in RoutingDecisionOutcome::ALL {
        for mode in RoutingDecisionMode::ALL {
            for confidence in [
                None,
                Some(DecisionConfidence::High),
                Some(DecisionConfidence::Medium),
                Some(DecisionConfidence::Low),
            ] {
                for cost in [
                    None,
                    Some(EstimatedCostClass::Low),
                    Some(EstimatedCostClass::Medium),
                    Some(EstimatedCostClass::High),
                    Some(EstimatedCostClass::Unknown),
                ] {
                    let value = RoutingDecisionNonTemporalCore::try_new(
                        "d".into(),
                        "r".into(),
                        None,
                        outcome,
                        Some("opaque-provider".into()),
                        Some("opaque-model".into()),
                        Some("opaque-runtime".into()),
                        None,
                        Some(evidence.clone()),
                        confidence,
                        Some(" \t可用?".into()),
                        cost,
                        Some(vec![]),
                        RegistryFreshness::new(Some(Some(-1)), None, None, true),
                        mode,
                        Some(false),
                        Some(true),
                        None,
                    )
                    .unwrap();
                    assert_eq!(value.outcome(), outcome);
                    assert_eq!(value.mode(), mode);
                    assert_eq!(value.confidence(), confidence);
                    assert_eq!(value.estimated_cost_class(), cost);
                    assert_eq!(value.availability_at_decision(), Some(" \t可用?"));
                    assert_eq!(value.capability_evidence(), Some(evidence.as_slice()));
                    assert_eq!(value.user_involved(), Some(false));
                    assert_eq!(value.user_pin_applied(), Some(true));
                    assert!(value.registry_freshness().stale());
                }
            }
        }
    }

    let minimal = minimal_decision("d".into(), "r".into(), None, None).unwrap();
    assert_eq!(minimal.selection_reason(), None);
    assert_eq!(minimal.capability_evidence(), None);
    assert_eq!(minimal.confidence(), None);
    assert_eq!(minimal.availability_at_decision(), None);
    assert_eq!(minimal.estimated_cost_class(), None);
    assert_eq!(minimal.alternative_candidates(), None);
    assert_eq!(minimal.user_involved(), None);
    assert_eq!(minimal.user_pin_applied(), None);

    for availability in ["", " \t\n", "可用/未来"] {
        let value = RoutingDecisionNonTemporalCore::try_new(
            "d".into(),
            "r".into(),
            None,
            RoutingDecisionOutcome::NoEligibleCandidate,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(availability.into()),
            None,
            None,
            RegistryFreshness::new(None, None, None, false),
            RoutingDecisionMode::AskOnUncertainty,
            None,
            None,
            None,
        )
        .unwrap();
        assert_eq!(value.availability_at_decision(), Some(availability));
    }
}
