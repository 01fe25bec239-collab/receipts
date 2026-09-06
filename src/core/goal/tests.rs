use super::*;

fn condition(text: &str, result: DeterministicConditionResult) -> DeterministicCondition {
    DeterministicCondition {
        condition: text.into(),
        result,
    }
}

fn core(
    state: GoalEvaluationState,
    layer: DeterministicLayer,
) -> Result<DeterministicGoalEvaluationCore, GoalEvaluationCoreError> {
    DeterministicGoalEvaluationCore::try_new(
        "evaluation".into(),
        "goal".into(),
        None,
        state,
        layer,
        None,
        None,
    )
}

#[test]
fn exact_owned_vocabularies() {
    assert_eq!(
        GoalEvaluationState::ALL.map(|value| value.as_str()),
        ["COMPLETE", "INCOMPLETE", "BLOCKED", "HUMAN_REQUIRED"]
    );
    assert_eq!(
        DeterministicConditionResult::ALL.map(|value| value.as_str()),
        ["PASS", "FAIL", "NOT_APPLICABLE"]
    );
}

#[test]
fn all_state_layer_combinations_validate_without_deriving_state() {
    for state in GoalEvaluationState::ALL {
        for passed in [false, true] {
            for result in DeterministicConditionResult::ALL {
                let conditions = Some(vec![condition("exact supplied fact", result)]);
                let layer = DeterministicLayer::try_new(passed, conditions.clone());
                assert_eq!(layer, DeterministicLayer::try_new(passed, conditions));
                if passed && result == DeterministicConditionResult::Fail {
                    assert_eq!(
                        layer,
                        Err(GoalEvaluationCoreError::PassedWithFailedCondition {
                            index: 0,
                            condition: "exact supplied fact".into(),
                        })
                    );
                    continue;
                }
                let layer = layer.unwrap();
                let actual = core(state, layer.clone());
                assert_eq!(actual, core(state, layer.clone()));
                if state == GoalEvaluationState::Complete && !passed {
                    assert_eq!(
                        actual,
                        Err(GoalEvaluationCoreError::CompleteWithFailedLayer)
                    );
                } else {
                    let actual = actual.unwrap();
                    assert_eq!(actual.state(), state);
                    assert_eq!(actual.deterministic_layer(), &layer);
                    assert_eq!(actual.deterministic_layer().passed(), passed);
                }
            }
        }
    }
}

#[test]
fn optional_or_empty_conditions_never_override_supplied_failure() {
    for conditions in [None, Some(vec![])] {
        for passed in [false, true] {
            let layer = DeterministicLayer::try_new(passed, conditions.clone()).unwrap();
            assert_eq!(layer.conditions(), conditions.as_deref());
            assert_eq!(layer.passed(), passed);
            let evaluated = core(GoalEvaluationState::Complete, layer);
            assert_eq!(evaluated.is_ok(), passed);
        }
    }
}

#[test]
fn blocking_and_missing_evidence_cannot_complete_and_reasons_remain_exact() {
    for reason in [
        "no blocking DAG tasks",
        "no unresolved blocking A4 findings",
        "required workstreams complete",
        "required checks green at integrated SHA — evidence missing",
        "integration SHA current; accepted work remains unintegrated",
        "required security review complete",
        "no unresolved critical dependency",
        "context epoch reconciled",
    ] {
        let layer = DeterministicLayer::try_new(
            false,
            Some(vec![condition(reason, DeterministicConditionResult::Fail)]),
        )
        .unwrap();
        assert_eq!(
            core(GoalEvaluationState::Complete, layer.clone()),
            Err(GoalEvaluationCoreError::CompleteWithFailedLayer)
        );
        let blocked = core(GoalEvaluationState::Blocked, layer).unwrap();
        assert_eq!(blocked.state(), GoalEvaluationState::Blocked);
        assert_eq!(
            blocked.deterministic_layer().conditions().unwrap()[0].condition,
            reason
        );
    }
}

#[test]
fn exact_text_order_duplicates_and_empty_text_are_preserved() {
    let entries = vec![
        condition(" z\n é e\u{301}\t", DeterministicConditionResult::Pass),
        condition("", DeterministicConditionResult::NotApplicable),
        condition(" a\n", DeterministicConditionResult::Fail),
        condition(" a\n", DeterministicConditionResult::Fail),
    ];
    let layer = DeterministicLayer::try_new(false, Some(entries.clone())).unwrap();
    assert_eq!(layer.conditions(), Some(entries.as_slice()));
    assert_eq!(
        DeterministicLayer::try_new(true, Some(entries)),
        Err(GoalEvaluationCoreError::PassedWithFailedCondition {
            index: 2,
            condition: " a\n".into(),
        })
    );
    // Neither keyword matching nor interpretation of human language occurs.
    let layer = DeterministicLayer::try_new(
        true,
        Some(vec![condition(
            "FAIL BLOCKED",
            DeterministicConditionResult::Pass,
        )]),
    )
    .unwrap();
    assert!(layer.passed());
}

#[test]
fn identifiers_use_exact_scalar_bounds_and_preserve_values() {
    for field in ["evaluation_id", "goal_id", "project_id"] {
        for text in [
            String::new(),
            "x".into(),
            " ".into(),
            " É e\u{301}\n".into(),
            "é".repeat(200),
            "é".repeat(201),
        ] {
            let evaluation_id = if field == "evaluation_id" { &text } else { "e" };
            let goal_id = if field == "goal_id" { &text } else { "g" };
            let project_id = if field == "project_id" { &text } else { "p" };
            let actual = DeterministicGoalEvaluationCore::try_new(
                evaluation_id.into(),
                goal_id.into(),
                Some(project_id.into()),
                GoalEvaluationState::Incomplete,
                DeterministicLayer::try_new(false, None).unwrap(),
                None,
                None,
            );
            let character_count = text.chars().count();
            if !(1..=200).contains(&character_count) {
                assert_eq!(
                    actual,
                    Err(GoalEvaluationCoreError::IdentifierLength {
                        field,
                        character_count
                    })
                );
            } else {
                let actual = actual.unwrap();
                assert_eq!(actual.evaluation_id(), evaluation_id);
                assert_eq!(actual.goal_id(), goal_id);
                assert_eq!(actual.project_id(), Some(project_id));
            }
        }
    }
    assert_eq!(
        core(
            GoalEvaluationState::Incomplete,
            DeterministicLayer::try_new(false, None).unwrap()
        )
        .unwrap()
        .project_id(),
        None
    );
}

fn metadata(
    sha: Option<String>,
    epoch: Option<i64>,
) -> Result<DeterministicGoalEvaluationCore, GoalEvaluationCoreError> {
    DeterministicGoalEvaluationCore::try_new(
        "e".into(),
        "g".into(),
        None,
        GoalEvaluationState::HumanRequired,
        DeterministicLayer::try_new(true, None).unwrap(),
        sha,
        epoch,
    )
}

#[test]
fn integrated_sha_is_exact_lowercase_forty_hex_without_lookup() {
    for sha in ["0123456789abcdef0123456789abcdef01234567", &"0".repeat(40)] {
        let actual = metadata(Some(sha.into()), None).unwrap();
        assert_eq!(actual.integrated_sha(), Some(sha));
    }
    for sha in [
        String::new(),
        "a".repeat(39),
        "a".repeat(41),
        "A".repeat(40),
        "g".repeat(40),
        "é".repeat(20),
        format!("{}\n", "a".repeat(40)),
        format!(" {}", "a".repeat(39)),
    ] {
        assert_eq!(
            metadata(Some(sha), None),
            Err(GoalEvaluationCoreError::InvalidIntegratedSha)
        );
    }
    assert_eq!(metadata(None, None).unwrap().integrated_sha(), None);
}

#[test]
fn context_epoch_is_optional_nonnegative_i64_data_only() {
    for epoch in [None, Some(0), Some(1), Some(i64::MAX)] {
        let actual = metadata(None, epoch).unwrap();
        assert_eq!(actual.context_epoch(), epoch);
        assert_eq!(actual.state(), GoalEvaluationState::HumanRequired);
    }
    for value in [-1, i64::MIN] {
        assert_eq!(
            metadata(None, Some(value)),
            Err(GoalEvaluationCoreError::NegativeContextEpoch { value })
        );
    }
}

#[test]
fn passing_layer_does_not_promote_supplied_noncomplete_states() {
    for state in [
        GoalEvaluationState::Incomplete,
        GoalEvaluationState::Blocked,
        GoalEvaluationState::HumanRequired,
    ] {
        let actual = core(state, DeterministicLayer::try_new(true, None).unwrap()).unwrap();
        assert_eq!(actual.state(), state);
    }
}
