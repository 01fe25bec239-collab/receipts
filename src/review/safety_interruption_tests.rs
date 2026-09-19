use crate::{
    ReviewDateTimeV1, SafetyInterruption, SafetyInterruptionDetectionConfidence as Confidence,
    SafetyInterruptionNonTemporalCore as Core, SafetyInterruptionState as State,
    SafetyInterruptionTerminalOutcome as Outcome,
};
use receipts_workspace_execution::{WorkspaceCheckpointRef, WorkspaceCheckpointRefType as RefType};

fn core(
    state: State,
    confidence: Option<Confidence>,
    booleans: [Option<bool>; 4],
    outcome: Option<Outcome>,
    refs: Option<Vec<WorkspaceCheckpointRef>>,
) -> Core {
    Core::new(
        " interruption 界 ".into(),
        " task\n".into(),
        "attempt\t".into(),
        " Unknown Provider/界\n".into(),
        Some(" Unknown Model/🦀 ".into()),
        state,
        confidence,
        booleans[0],
        booleans[1],
        booleans[2],
        Some(" Opaque Retry Provider/界 ".into()),
        booleans[3],
        outcome,
        refs,
    )
    .unwrap()
}

fn timestamp() -> ReviewDateTimeV1 {
    ReviewDateTimeV1::try_new("0000-02-29t23:59:60.100000000-00:00").unwrap()
}

#[test]
fn required_inputs_and_all_fourteen_core_fields_are_preserved() {
    let evidence = WorkspaceCheckpointRef::new(RefType::ArtifactId, "opaque", None, None).unwrap();
    let supplied_core = core(
        State::PolicyBlocked,
        Some(Confidence::Heuristic),
        [Some(false), None, Some(true), Some(false)],
        Some(Outcome::HumanRequired),
        Some(vec![evidence.clone()]),
    );
    let supplied_time = timestamp();
    let value = SafetyInterruption::new(supplied_core.clone(), supplied_time.clone());
    let actual: &Core = value.core();
    let observed_at: &ReviewDateTimeV1 = value.observed_at();
    assert_eq!(actual, &supplied_core);
    assert_eq!(observed_at, &supplied_time);
    assert_eq!(observed_at.as_str(), "0000-02-29t23:59:60.100000000-00:00");
    assert_eq!(actual.interruption_id(), " interruption 界 ");
    assert_eq!(actual.task_id(), " task\n");
    assert_eq!(actual.attempt_id(), "attempt\t");
    assert_eq!(actual.provider_id(), " Unknown Provider/界\n");
    assert_eq!(actual.model_id(), Some(" Unknown Model/🦀 "));
    assert_eq!(actual.state(), State::PolicyBlocked);
    assert_eq!(actual.detection_confidence(), Some(Confidence::Heuristic));
    assert_eq!(actual.task_classified_defensive(), Some(false));
    assert_eq!(actual.capsule_narrowed(), None);
    assert_eq!(actual.retry_attempted(), Some(true));
    assert_eq!(
        actual.retry_provider_id(),
        Some(" Opaque Retry Provider/界 ")
    );
    assert_eq!(actual.deterministic_tooling_used(), Some(false));
    assert_eq!(actual.terminal_outcome(), Some(Outcome::HumanRequired));
    assert_eq!(
        actual.preserved_evidence_refs(),
        Some([evidence].as_slice())
    );
    assert_eq!(value, value.clone());
}

#[test]
fn optional_absence_is_preserved_without_defaults() {
    let supplied_core = Core::new(
        "i".into(),
        "t".into(),
        "a".into(),
        "p".into(),
        None,
        State::Unknown,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let value = SafetyInterruption::new(supplied_core.clone(), timestamp());
    assert_eq!(value.core(), &supplied_core);
    assert_eq!(value.core().model_id(), None);
    assert_eq!(value.core().detection_confidence(), None);
    assert_eq!(value.core().task_classified_defensive(), None);
    assert_eq!(value.core().capsule_narrowed(), None);
    assert_eq!(value.core().retry_attempted(), None);
    assert_eq!(value.core().retry_provider_id(), None);
    assert_eq!(value.core().deterministic_tooling_used(), None);
    assert_eq!(value.core().terminal_outcome(), None);
    assert_eq!(value.core().preserved_evidence_refs(), None);
}

#[test]
fn all_vocabularies_and_boolean_tristates_compose_without_policy_or_transitions() {
    let tristates = [None, Some(false), Some(true)];
    for state in State::ALL {
        for confidence in [None].into_iter().chain(Confidence::ALL.map(Some)) {
            for outcome in [None].into_iter().chain(Outcome::ALL.map(Some)) {
                for defensive in tristates {
                    for narrowed in tristates {
                        for retry in tristates {
                            for tooling in tristates {
                                let supplied = core(
                                    state,
                                    confidence,
                                    [defensive, narrowed, retry, tooling],
                                    outcome,
                                    None,
                                );
                                let value = SafetyInterruption::new(supplied.clone(), timestamp());
                                assert_eq!(value.core(), &supplied);
                                assert_eq!(value.core().state(), state);
                                assert_eq!(value.core().detection_confidence(), confidence);
                                assert_eq!(value.core().terminal_outcome(), outcome);
                                assert_eq!(value.core().task_classified_defensive(), defensive);
                                assert_eq!(value.core().capsule_narrowed(), narrowed);
                                assert_eq!(value.core().retry_attempted(), retry);
                                assert_eq!(value.core().deterministic_tooling_used(), tooling);
                                assert_eq!(value.observed_at(), &timestamp());
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn absent_confidence_remains_distinct_from_explicit_unknown() {
    let absent = SafetyInterruption::new(
        core(State::PolicyBlocked, None, [None; 4], None, None),
        timestamp(),
    );
    let unknown = SafetyInterruption::new(
        core(
            State::PolicyBlocked,
            Some(Confidence::Unknown),
            [None; 4],
            None,
            None,
        ),
        timestamp(),
    );
    assert_eq!(absent.core().detection_confidence(), None);
    assert_eq!(
        unknown.core().detection_confidence(),
        Some(Confidence::Unknown)
    );
    assert_ne!(absent, unknown);
}

#[test]
fn evidence_presence_order_duplicates_and_empty_metadata_survive() {
    let absent = SafetyInterruption::new(
        core(State::Unknown, None, [None; 4], None, None),
        timestamp(),
    );
    let empty = SafetyInterruption::new(
        core(State::Unknown, None, [None; 4], None, Some(vec![])),
        timestamp(),
    );
    assert_eq!(absent.core().preserved_evidence_refs(), None);
    assert_eq!(empty.core().preserved_evidence_refs(), Some([].as_slice()));
    assert_ne!(absent, empty);

    let mut refs = Vec::new();
    for ref_type in RefType::ALL {
        for digest in [None, Some(""), Some(" digest 界 ")] {
            for section in [None, Some(""), Some(" section\n")] {
                refs.push(
                    WorkspaceCheckpointRef::new(
                        ref_type,
                        " opaque:unresolved/界 ",
                        digest.map(String::from),
                        section.map(String::from),
                    )
                    .unwrap(),
                );
            }
        }
    }
    refs.reverse();
    refs.push(refs[0].clone());
    let value = SafetyInterruption::new(
        core(State::Unknown, None, [None; 4], None, Some(refs.clone())),
        timestamp(),
    );
    let actual: &[WorkspaceCheckpointRef] = value.core().preserved_evidence_refs().unwrap();
    assert_eq!(actual, refs);
    assert_eq!(actual.first(), actual.last());
    for (actual, expected) in actual.iter().zip(&refs) {
        assert_eq!(actual.ref_type(), expected.ref_type());
        assert_eq!(actual.target(), expected.target());
        assert_eq!(actual.digest(), expected.digest());
        assert_eq!(actual.section(), expected.section());
    }
}

#[test]
fn caller_timestamps_are_preserved_without_normalization_or_temporal_comparison() {
    let supplied = core(
        State::SafetyCheckPending,
        None,
        [None; 4],
        Some(Outcome::HumanRequired),
        None,
    );
    let inputs = [
        "9999-12-31T23:59:60.0z",
        "0000-01-01T00:00:00Z",
        "2026-09-08T00:00:00Z",
        "2026-09-08t00:00:00Z",
        "2026-09-08T00:00:00z",
        "2026-09-08t00:00:00z",
        "2026-09-08T00:00:00+00:00",
        "2026-09-08T00:00:00-00:00",
        "2026-09-08T00:00:00.1Z",
        "2026-09-08T00:00:00.100000000Z",
    ];
    let values: Vec<_> = inputs
        .iter()
        .map(|input| {
            let observed_at = ReviewDateTimeV1::try_new(*input).unwrap();
            let value = SafetyInterruption::new(supplied.clone(), observed_at);
            assert_eq!(value.core(), &supplied);
            assert_eq!(value.observed_at().as_str(), *input);
            value
        })
        .collect();
    for (index, value) in values.iter().enumerate() {
        for other in &values[index + 1..] {
            assert_ne!(value, other);
        }
    }
}
