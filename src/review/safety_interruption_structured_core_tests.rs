use crate::{
    SafetyInterruptionConstructionError as Error,
    SafetyInterruptionDetectionConfidence as Confidence, SafetyInterruptionNonTemporalCore as Core,
    SafetyInterruptionState as State, SafetyInterruptionTerminalOutcome as Outcome,
};
use receipts_workspace_execution::{
    WorkspaceCheckpointRef, WorkspaceCheckpointRefError, WorkspaceCheckpointRefType as RefType,
};

// Test inputs are deliberately unvalidated; every assertion goes through the public constructor.
struct Input {
    ids: [String; 3],
    provider: String,
    model: Option<String>,
    state: State,
    confidence: Option<Confidence>,
    booleans: [Option<bool>; 4],
    retry_provider: Option<String>,
    outcome: Option<Outcome>,
    refs: Option<Vec<WorkspaceCheckpointRef>>,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            ids: ["i".into(), "t".into(), "a".into()],
            provider: "opaque-provider".into(),
            model: None,
            state: State::SafetyCheckPending,
            confidence: None,
            booleans: [None; 4],
            retry_provider: None,
            outcome: None,
            refs: None,
        }
    }
}

impl Input {
    fn build(self) -> Result<Core, Error> {
        let [interruption, task, attempt] = self.ids;
        Core::new(
            interruption,
            task,
            attempt,
            self.provider,
            self.model,
            self.state,
            self.confidence,
            self.booleans[0],
            self.booleans[1],
            self.booleans[2],
            self.retry_provider,
            self.booleans[3],
            self.outcome,
            self.refs,
        )
    }
}

#[test]
fn minimum_and_deterministic_clone() {
    let record = Input::default().build().unwrap();
    assert_eq!(record.interruption_id(), "i");
    assert_eq!(record.task_id(), "t");
    assert_eq!(record.attempt_id(), "a");
    assert_eq!(record.provider_id(), "opaque-provider");
    assert_eq!(record.state(), State::SafetyCheckPending);
    assert_eq!(record.model_id(), None);
    assert_eq!(record.detection_confidence(), None);
    assert_eq!(record.task_classified_defensive(), None);
    assert_eq!(record.capsule_narrowed(), None);
    assert_eq!(record.retry_attempted(), None);
    assert_eq!(record.retry_provider_id(), None);
    assert_eq!(record.deterministic_tooling_used(), None);
    assert_eq!(record.terminal_outcome(), None);
    assert_eq!(record.preserved_evidence_refs(), None);
    assert_eq!(record, Input::default().build().unwrap());
    assert_eq!(record, record.clone());
}

#[test]
fn exact_closed_vocabularies() {
    assert_eq!(
        State::ALL.map(State::as_str),
        ["SAFETY_CHECK_PENDING", "POLICY_BLOCKED", "UNKNOWN"]
    );
    assert_eq!(
        Confidence::ALL.map(Confidence::as_str),
        ["EXPLICIT_PROVIDER_SIGNAL", "HEURISTIC", "UNKNOWN"]
    );
    assert_eq!(
        Outcome::ALL.map(Outcome::as_str),
        [
            "RESUMED",
            "HUMAN_REQUIRED",
            "ROUTED_ELSEWHERE_COMPLIANTLY",
            "ABANDONED",
            "PENDING"
        ]
    );
}

#[test]
fn each_identifier_has_character_boundaries_without_rewriting() {
    for (index, field) in ["interruption_id", "task_id", "attempt_id"]
        .into_iter()
        .enumerate()
    {
        for character in ["x", "界", "🦀"] {
            for count in [0, 1, 200, 201] {
                let value = character.repeat(count);
                let mut input = Input::default();
                input.ids[index] = value.clone();
                let result = input.build();
                if count == 0 || count == 201 {
                    assert_eq!(result, Err(Error::InvalidIdentifier(field)));
                } else {
                    let record = result.unwrap();
                    assert_eq!(
                        [
                            record.interruption_id(),
                            record.task_id(),
                            record.attempt_id()
                        ][index],
                        value
                    );
                }
            }
        }
        for value in [" ", " Mixed\t\n", "e\u{301}"] {
            let mut input = Input::default();
            input.ids[index] = value.into();
            let record = input.build().unwrap();
            assert_eq!(
                [
                    record.interruption_id(),
                    record.task_id(),
                    record.attempt_id()
                ][index],
                value
            );
        }
    }
}

#[test]
fn opaque_strings_reject_only_present_empty_values() {
    for field in ["provider_id", "model_id", "retry_provider_id"] {
        for value in [
            String::new(),
            " ".into(),
            " Previously-Unknown/界\n".into(),
            "🦀".repeat(10_000),
        ] {
            let mut input = Input::default();
            match field {
                "provider_id" => input.provider = value.clone(),
                "model_id" => input.model = Some(value.clone()),
                _ => input.retry_provider = Some(value.clone()),
            }
            let result = input.build();
            if value.is_empty() {
                assert_eq!(result, Err(Error::EmptyString(field)));
            } else {
                let record = result.unwrap();
                let actual = match field {
                    "provider_id" => Some(record.provider_id()),
                    "model_id" => record.model_id(),
                    _ => record.retry_provider_id(),
                };
                assert_eq!(actual, Some(value.as_str()));
            }
        }
    }
}

#[test]
fn confidence_omission_is_distinct_from_explicit_unknown() {
    let absent = Input {
        state: State::PolicyBlocked,
        ..Input::default()
    }
    .build()
    .unwrap();
    let unknown = Input {
        state: State::PolicyBlocked,
        confidence: Some(Confidence::Unknown),
        ..Input::default()
    }
    .build()
    .unwrap();
    assert_eq!(absent.detection_confidence(), None);
    assert_eq!(unknown.detection_confidence(), Some(Confidence::Unknown));
    assert_ne!(absent, unknown);
}

#[test]
fn optional_values_are_independent_without_cross_field_policy() {
    let tristates = [None, Some(false), Some(true)];
    for state in State::ALL {
        for confidence in [
            None,
            Some(Confidence::ExplicitProviderSignal),
            Some(Confidence::Heuristic),
            Some(Confidence::Unknown),
        ] {
            for outcome in [None].into_iter().chain(Outcome::ALL.map(Some)) {
                for defensive in tristates {
                    for narrowed in tristates {
                        for retry in tristates {
                            for tooling in tristates {
                                for provider in [None, Some("provider-x")] {
                                    let record = Input {
                                        state,
                                        confidence,
                                        outcome,
                                        booleans: [defensive, narrowed, retry, tooling],
                                        retry_provider: provider.map(String::from),
                                        ..Input::default()
                                    }
                                    .build()
                                    .unwrap();
                                    assert_eq!(record.state(), state);
                                    assert_eq!(record.detection_confidence(), confidence);
                                    assert_eq!(record.terminal_outcome(), outcome);
                                    assert_eq!(record.task_classified_defensive(), defensive);
                                    assert_eq!(record.capsule_narrowed(), narrowed);
                                    assert_eq!(record.retry_attempted(), retry);
                                    assert_eq!(record.deterministic_tooling_used(), tooling);
                                    assert_eq!(record.retry_provider_id(), provider);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn canonical_evidence_mapping_and_array_presence_order_duplicates() {
    let absent = Input::default().build().unwrap();
    let empty = Input {
        refs: Some(vec![]),
        ..Input::default()
    }
    .build()
    .unwrap();
    assert_eq!(absent.preserved_evidence_refs(), None);
    assert_eq!(empty.preserved_evidence_refs(), Some([].as_slice()));
    assert_ne!(absent, empty);
    assert_eq!(
        RefType::ALL.map(RefType::as_str),
        ["REPO_PATH", "STATE_QUERY", "ARTIFACT_ID", "URL"]
    );
    let mut refs = Vec::new();
    for ref_type in RefType::ALL {
        assert_eq!(
            WorkspaceCheckpointRef::new(ref_type, "", None, None),
            Err(WorkspaceCheckpointRefError::EmptyTarget)
        );
        for target in [" ", "opaque:unresolved/界"] {
            for digest in [None, Some(""), Some("digest")] {
                for section in [None, Some(""), Some("section")] {
                    let evidence = WorkspaceCheckpointRef::new(
                        ref_type,
                        target,
                        digest.map(String::from),
                        section.map(String::from),
                    )
                    .unwrap();
                    assert_eq!(evidence.ref_type(), ref_type);
                    assert_eq!(evidence.target(), target);
                    assert_eq!(evidence.digest(), digest);
                    assert_eq!(evidence.section(), section);
                    refs.push(evidence);
                }
            }
        }
    }
    refs.reverse();
    refs.push(refs[0].clone());
    let record = Input {
        refs: Some(refs.clone()),
        model: Some("model-x".into()),
        ..Input::default()
    }
    .build()
    .unwrap();
    let actual: &[WorkspaceCheckpointRef] = record.preserved_evidence_refs().unwrap();
    assert_eq!(actual, refs);
    assert_eq!(actual.first(), actual.last());
    assert_eq!(record, record.clone());
    assert_eq!(record.clone().model_id(), Some("model-x"));
}
