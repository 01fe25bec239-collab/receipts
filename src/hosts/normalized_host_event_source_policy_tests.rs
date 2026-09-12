use super::*;
use receipts_orchestration::orchestration::{OrchestrationDateTimeV1, OrchestrationJsonObjectV1};

#[test]
fn source_policy_matches_all_64_frozen_pairs() {
    use NormalizedHostEventSourceClass::*;
    use NormalizedHostEventType::*;

    // Independent oracle: columns are HostHook, WorkerDispatch, Elicitation, CoreDriven.
    let expected = [
        (HostSessionStarted, [true, false, false, false]),
        (HostSessionEnding, [true, false, false, false]),
        (UserGoalSubmitted, [false, false, true, false]),
        (UserInputProvided, [false, false, true, false]),
        (RoleExecutorStarted, [false, false, false, true]),
        (RoleExecutorStopped, [false, false, false, true]),
        (TaskStarted, [false, true, false, false]),
        (TaskCompleted, [false, true, false, false]),
        (TaskFailed, [false, true, false, false]),
        (ToolExecuted, [true, true, false, false]),
        (WorkspaceCreated, [false, false, false, true]),
        (WorkspaceChanged, [false, true, false, false]),
        (WorkspaceRemoved, [false, false, false, true]),
        (ContextCompacted, [true, false, false, false]),
        (ProviderSignal, [false, true, false, false]),
        (HostError, [true, false, false, false]),
    ];
    assert_eq!(
        NormalizedHostEventType::ALL,
        expected.map(|(event, _)| event)
    );
    assert_eq!(
        NormalizedHostEventSourceClass::ALL,
        [HostHook, WorkerDispatch, Elicitation, CoreDriven]
    );

    let (mut allowed, mut rejected) = (0, 0);
    for (row, event_type) in NormalizedHostEventType::ALL.into_iter().enumerate() {
        let mut event_allowed = 0;
        for (column, source_class) in NormalizedHostEventSourceClass::ALL.into_iter().enumerate() {
            let actual = source_class_allowed(event_type, source_class);
            assert_eq!(
                actual, expected[row].1[column],
                "{event_type:?} + {source_class:?}"
            );
            if actual {
                allowed += 1;
                event_allowed += 1;
            } else {
                rejected += 1;
            }
        }
        assert_eq!(
            event_allowed,
            if event_type == ToolExecuted { 2 } else { 1 }
        );
    }
    assert_eq!(allowed + rejected, 64);
    assert_eq!(allowed, 17);
    assert_eq!(rejected, 47);
}

#[test]
fn tool_executed_accepts_both_and_only_hook_and_worker() {
    use NormalizedHostEventSourceClass::*;
    let event = NormalizedHostEventType::ToolExecuted;
    assert!(source_class_allowed(event, HostHook));
    assert!(source_class_allowed(event, WorkerDispatch));
    assert!(!source_class_allowed(event, Elicitation));
    assert!(!source_class_allowed(event, CoreDriven));
}

#[test]
fn source_policy_consumes_envelope_type_without_changing_envelope() {
    for confidence in NormalizedHostEventConfidence::ALL {
        let inputs = NormalizedHostEventInputs {
            event_id: NormalizedHostEventId::try_new("Z123456789ABCDEFGHJKMNPQRS").unwrap(),
            event_type: NormalizedHostEventType::ToolExecuted,
            host: HostId::Headless.into(),
            host_session_id: "session".to_owned(),
            project_id: None,
            occurred_at: OrchestrationDateTimeV1::try_new("2026-09-12T00:00:00Z").unwrap(),
            payload: OrchestrationJsonObjectV1::default(),
            raw_ref: None,
            confidence,
        };
        let event = NormalizedHostEvent::try_new(inputs).unwrap();
        let original = event.clone();
        for source in NormalizedHostEventSourceClass::ALL {
            assert_eq!(
                source_class_allowed(event.event_type(), source),
                source_class_allowed(NormalizedHostEventType::ToolExecuted, source)
            );
            assert_eq!(event, original);
            assert_eq!(event.confidence(), confidence);
        }
    }
}
