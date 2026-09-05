use crate::{
    RoutingPriority, RoutingQualityFloor, RoutingRequestConstraints,
    RoutingRequestConstraintsError, RoutingRequestCoreError, RoutingRequestNonTemporalCore,
    RoutingRequestRole, RoutingTaskClass,
};

#[test]
fn closed_vocabularies_have_exact_order_and_canonical_strings() {
    assert_eq!(
        RoutingRequestRole::ALL,
        [
            RoutingRequestRole::Implementer,
            RoutingRequestRole::Reviewer,
            RoutingRequestRole::Manager,
            RoutingRequestRole::Evaluator,
            RoutingRequestRole::Renderer,
        ]
    );
    assert_eq!(
        RoutingTaskClass::ALL,
        [
            RoutingTaskClass::FrontierImplementation,
            RoutingTaskClass::FrontierReview,
            RoutingTaskClass::FrontierArchitecture,
            RoutingTaskClass::SecurityCriticalCode,
            RoutingTaskClass::BalancedReasoning,
            RoutingTaskClass::EconomyDocs,
            RoutingTaskClass::EconomySummary,
            RoutingTaskClass::EconomyStatus,
            RoutingTaskClass::PresentationOnly,
        ]
    );
    assert_eq!(
        RoutingQualityFloor::ALL,
        [
            RoutingQualityFloor::Frontier,
            RoutingQualityFloor::Balanced,
            RoutingQualityFloor::Economy
        ]
    );
    assert_eq!(
        RoutingPriority::ALL,
        [
            RoutingPriority::Highest,
            RoutingPriority::High,
            RoutingPriority::Secondary,
            RoutingPriority::Low
        ]
    );
    assert_eq!(
        RoutingRequestRole::ALL.map(|v| v.as_str()),
        [
            "IMPLEMENTER",
            "REVIEWER",
            "MANAGER",
            "EVALUATOR",
            "RENDERER",
        ]
    );
    assert_eq!(
        RoutingTaskClass::ALL.map(|v| v.as_str()),
        [
            "FRONTIER_IMPLEMENTATION",
            "FRONTIER_REVIEW",
            "FRONTIER_ARCHITECTURE",
            "SECURITY_CRITICAL_CODE",
            "BALANCED_REASONING",
            "ECONOMY_DOCS",
            "ECONOMY_SUMMARY",
            "ECONOMY_STATUS",
            "PRESENTATION_ONLY",
        ]
    );
    assert_eq!(
        RoutingQualityFloor::ALL.map(|v| v.as_str()),
        ["FRONTIER", "BALANCED", "ECONOMY"]
    );
    assert_eq!(
        RoutingPriority::ALL.map(|v| v.as_str()),
        ["HIGHEST", "HIGH", "SECONDARY", "LOW"]
    );
}

fn request(
    request_id: String,
    task_id: Option<String>,
    required: Vec<String>,
    preferred: Option<Vec<String>>,
) -> Result<RoutingRequestNonTemporalCore, RoutingRequestCoreError> {
    RoutingRequestNonTemporalCore::try_new(
        request_id,
        task_id,
        RoutingRequestRole::Reviewer,
        RoutingTaskClass::SecurityCriticalCode,
        RoutingQualityFloor::Economy,
        required,
        preferred,
        None,
        None,
        None,
        None,
    )
}

#[test]
fn identifier_boundaries_count_unicode_characters_and_preserve_text() {
    assert_eq!(
        request(String::new(), None, vec!["coding".into()], None),
        Err(RoutingRequestCoreError::EmptyRequestId)
    );
    assert_eq!(
        request("r".into(), Some(String::new()), vec!["coding".into()], None),
        Err(RoutingRequestCoreError::EmptyTaskId)
    );
    for text in [
        "a".into(),
        "a".repeat(200),
        "界".repeat(200),
        "🦀".repeat(200),
        "e\u{301}".repeat(100),
        " \t\n".into(),
        " Opaque/界/e\u{301} ".into(),
    ] {
        let core = request(
            text.clone(),
            Some(text.clone()),
            vec!["coding".into()],
            None,
        )
        .unwrap();
        assert_eq!(core.request_id(), text);
        assert_eq!(core.task_id(), Some(text.as_str()));
    }
    for text in [
        "a".repeat(201),
        "界".repeat(201),
        format!("{}x", "e\u{301}".repeat(100)),
    ] {
        assert_eq!(
            request(text.clone(), None, vec!["coding".into()], None),
            Err(RoutingRequestCoreError::RequestIdTooLong)
        );
        assert_eq!(
            request("r".into(), Some(text), vec!["coding".into()], None),
            Err(RoutingRequestCoreError::TaskIdTooLong)
        );
    }
    assert_eq!(
        request("r".into(), None, vec!["coding".into()], None)
            .unwrap()
            .task_id(),
        None
    );
}

#[test]
fn capability_arrays_preserve_omission_empty_arrays_duplicates_order_and_exact_strings() {
    assert_eq!(
        request("r".into(), None, vec![], None),
        Err(RoutingRequestCoreError::EmptyRequiredCapabilities)
    );
    let values = vec![
        "coding".into(),
        "structured_output".into(),
        "future_capability".into(),
        "future_new_capability".into(),
        "☃".into(),
        "能力".into(),
        "Ω".into(),
        " ".into(),
        "   ".into(),
        "\t".into(),
        "\n".into(),
        " \t ".into(),
        "CoDiNg".into(),
        "future/capability:v2+beta".into(),
        "  coding  ".into(),
        "future-capability/Ω".into(),
        " \t\n".into(),
        "界/e\u{301}".into(),
        "future-capability/Ω".into(),
    ];
    for required in [vec!["coding".into()], values.clone()] {
        for preferred in [
            None,
            Some(vec![]),
            Some(vec!["coding".into()]),
            Some(values.clone()),
        ] {
            let core = request("r".into(), None, required.clone(), preferred.clone()).unwrap();
            assert_eq!(core.required_capabilities(), required);
            assert_eq!(core.preferred_capabilities(), preferred.as_deref());
            assert_eq!(core.constraints(), None);
            assert_eq!(core.quality_priority(), None);
            assert_eq!(core.cost_priority(), None);
        }
    }
}

#[test]
fn empty_capability_entries_are_rejected_at_every_position() {
    for (values, index) in [
        (vec![""], 0),
        (vec!["", "coding"], 0),
        (vec!["coding", ""], 1),
        (vec!["coding", "", "review"], 1),
        (vec!["coding", "review", ""], 2),
        (vec!["coding", "", ""], 1),
    ] {
        let values: Vec<String> = values.into_iter().map(String::from).collect();
        assert_eq!(
            request("r".into(), None, values.clone(), None),
            Err(RoutingRequestCoreError::EmptyRequiredCapability { index })
        );
        assert_eq!(
            request("r".into(), None, vec!["coding".into()], Some(values)),
            Err(RoutingRequestCoreError::EmptyPreferredCapability { index })
        );
    }
}

#[test]
fn constraint_errors_are_only_physical_schema_invariants() {
    use RoutingRequestConstraintsError::*;
    for (distinct, avoid, model, provider, cost, error) in [
        (Some(""), None, None, None, None, EmptyDistinctProviderFrom),
        (
            None,
            Some(vec!["future-provider", ""]),
            None,
            None,
            None,
            EmptyAvoidProvider { index: 1 },
        ),
        (
            None,
            Some(vec![""]),
            None,
            None,
            None,
            EmptyAvoidProvider { index: 0 },
        ),
        (None, None, Some(""), None, None, EmptyUserPinnedModel),
        (None, None, None, Some(""), None, EmptyUserPinnedProvider),
        (None, None, None, None, Some(f64::NAN), NonFiniteMaxCost),
        (
            None,
            None,
            None,
            None,
            Some(f64::INFINITY),
            NonFiniteMaxCost,
        ),
        (
            None,
            None,
            None,
            None,
            Some(f64::NEG_INFINITY),
            NonFiniteMaxCost,
        ),
    ] {
        assert_eq!(
            RoutingRequestConstraints::try_new(
                distinct.map(String::from),
                avoid.map(|v| v.into_iter().map(String::from).collect()),
                model.map(String::from),
                provider.map(String::from),
                cost
            ),
            Err(error)
        );
    }
}

#[test]
fn opaque_constraints_preserve_unbounded_strings_and_independent_pins() {
    let long = "界".repeat(10_000);
    let texts = [
        None,
        Some(" \t\n"),
        Some(" future-opaque/Ω/e\u{301} "),
        Some(long.as_str()),
    ];
    for distinct in texts {
        for model in texts {
            for provider in texts {
                for avoid in [
                    None,
                    Some(vec![]),
                    Some(vec![
                        "future-provider".into(),
                        " \t\n".into(),
                        long.clone(),
                        "界".into(),
                        "future-provider".into(),
                    ]),
                ] {
                    let core = RoutingRequestConstraints::try_new(
                        distinct.map(String::from),
                        avoid.clone(),
                        model.map(String::from),
                        provider.map(String::from),
                        None,
                    )
                    .unwrap();
                    assert_eq!(core.distinct_provider_from(), distinct);
                    assert_eq!(core.avoid_providers(), avoid.as_deref());
                    assert_eq!(core.user_pinned_model(), model);
                    assert_eq!(core.user_pinned_provider(), provider);
                    assert_eq!(core.max_cost(), None);
                }
            }
        }
    }
}

#[test]
fn max_cost_accepts_every_tested_finite_sign_and_preserves_bits() {
    for cost in [
        None,
        Some(0.0),
        Some(-0.0),
        Some(17.25),
        Some(-17.25),
        Some(f64::MAX),
        Some(f64::MIN),
        Some(f64::MIN_POSITIVE),
    ] {
        let core = RoutingRequestConstraints::try_new(None, None, None, None, cost).unwrap();
        assert_eq!(core.max_cost().map(f64::to_bits), cost.map(f64::to_bits));
    }
}

#[test]
fn all_request_vocabularies_priorities_and_context_hints_are_independent_storage() {
    let empty = RoutingRequestConstraints::try_new(None, None, None, None, None).unwrap();
    let conflicting = RoutingRequestConstraints::try_new(
        Some("opaque/Ω".into()),
        Some(vec!["opaque/Ω".into()]),
        Some("future-model/界".into()),
        Some("opaque/Ω".into()),
        Some(-1.0),
    )
    .unwrap();
    let priorities = [
        None,
        Some(RoutingPriority::Highest),
        Some(RoutingPriority::High),
        Some(RoutingPriority::Secondary),
        Some(RoutingPriority::Low),
    ];
    for role in RoutingRequestRole::ALL {
        for task in RoutingTaskClass::ALL {
            for floor in RoutingQualityFloor::ALL {
                for quality in priorities {
                    for cost in priorities {
                        for context in [None, Some(0), Some(73), Some(u64::MAX)] {
                            for constraints in
                                [None, Some(empty.clone()), Some(conflicting.clone())]
                            {
                                let core = RoutingRequestNonTemporalCore::try_new(
                                    "r".into(),
                                    None,
                                    role,
                                    task,
                                    floor,
                                    vec!["coding".into()],
                                    None,
                                    quality,
                                    cost,
                                    constraints.clone(),
                                    context,
                                )
                                .unwrap();
                                assert_eq!(core.role(), role);
                                assert_eq!(core.task_class(), task);
                                assert_eq!(core.quality_floor(), floor);
                                assert_eq!(core.quality_priority(), quality);
                                assert_eq!(core.cost_priority(), cost);
                                assert_eq!(core.context_size_hint(), context);
                                assert_eq!(core.constraints(), constraints.as_ref());
                            }
                        }
                    }
                }
            }
        }
    }
}
