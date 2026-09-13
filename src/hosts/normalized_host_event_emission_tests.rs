use super::*;
use receipts_orchestration::orchestration::{
    OrchestrationDateTimeV1, OrchestrationJsonObjectV1, OrchestrationJsonValueV1,
};

fn inputs(event_type: NormalizedHostEventType) -> NormalizedHostEventInputs {
    NormalizedHostEventInputs {
        event_id: NormalizedHostEventId::try_new("Z123456789ABCDEFGHJKMNPQRS").unwrap(),
        event_type,
        host: HostId::Headless.into(),
        host_session_id: "session".to_owned(),
        project_id: None,
        occurred_at: OrchestrationDateTimeV1::try_new("2026-09-12T00:00:00Z").unwrap(),
        payload: OrchestrationJsonObjectV1::default(),
        raw_ref: None,
        confidence: NormalizedHostEventConfidence::Observed,
    }
}

#[test]
fn all_64_pairs_delegate_exactly_and_repeat_without_mutation() {
    assert_eq!(NormalizedHostEventType::ALL.len(), 16);
    assert_eq!(NormalizedHostEventSourceClass::ALL.len(), 4);
    let mut combinations = 0;
    for event_type in NormalizedHostEventType::ALL {
        let event = NormalizedHostEvent::try_new(inputs(event_type)).unwrap();
        let original = event.clone();
        for source_class in NormalizedHostEventSourceClass::ALL {
            let expected = if source_class_allowed(event_type, source_class) {
                Ok(())
            } else {
                Err(NormalizedHostEventEmissionSourceError {
                    event_type,
                    source_class,
                })
            };
            for _ in 0..3 {
                assert_eq!(
                    validate_normalized_host_event_source(source_class, &event),
                    expected,
                    "{event_type:?} + {source_class:?}"
                );
                assert_eq!(event, original);
            }
            combinations += 1;
        }
    }
    assert_eq!(combinations, 64);
}

#[test]
fn representative_acceptance_and_rejection_for_each_source_class() {
    use NormalizedHostEventSourceClass::*;
    use NormalizedHostEventType::*;
    for (source_class, accepted, rejected) in [
        (HostHook, HostSessionStarted, TaskStarted),
        (WorkerDispatch, TaskStarted, HostSessionStarted),
        (Elicitation, UserInputProvided, RoleExecutorStarted),
        (CoreDriven, RoleExecutorStarted, UserInputProvided),
    ] {
        let event = NormalizedHostEvent::try_new(inputs(accepted)).unwrap();
        assert_eq!(
            validate_normalized_host_event_source(source_class, &event),
            Ok(())
        );
        let event = NormalizedHostEvent::try_new(inputs(rejected)).unwrap();
        let error = validate_normalized_host_event_source(source_class, &event).unwrap_err();
        assert_eq!(
            error,
            NormalizedHostEventEmissionSourceError {
                event_type: rejected,
                source_class,
            }
        );
        let error: &dyn std::error::Error = &error;
        assert_eq!(
            error.to_string(),
            format!(
                "source class {} is not semantically permitted for event type {}",
                source_class.as_str(),
                rejected.as_str()
            )
        );
        assert!(error.source().is_none());
    }
}

#[test]
fn every_non_event_type_field_is_irrelevant() {
    for event_type in NormalizedHostEventType::ALL {
        let original_inputs = inputs(event_type);
        let original = NormalizedHostEvent::try_new(original_inputs.clone()).unwrap();
        // Change each field independently so one change cannot mask another.
        let mut variants = vec![original_inputs.clone(); 9];
        variants[0].confidence = NormalizedHostEventConfidence::Inferred;
        variants[1].host = NormalizedHostEventHost::try_new("arbitrary-host").unwrap();
        variants[2].payload = OrchestrationJsonObjectV1::new(std::collections::BTreeMap::from([(
            "arbitrary".to_owned(),
            OrchestrationJsonValueV1::String("contents".to_owned()),
        )]));
        variants[3].raw_ref = Some(
            NormalizedHostEventRawRef::try_new(
                NormalizedHostEventRawRefType::ArtifactId,
                "artifact".to_owned(),
                Some("digest".to_owned()),
                Some("section".to_owned()),
            )
            .unwrap(),
        );
        variants[4].occurred_at = OrchestrationDateTimeV1::try_new("2020-01-01T12:34:56Z").unwrap();
        variants[5].event_id =
            NormalizedHostEventId::try_new("Y123456789ABCDEFGHJKMNPQRS").unwrap();
        variants[6].host_session_id = "other-session".to_owned();
        variants[7].project_id = Some("project".to_owned());
        variants[8].host = HostId::Codex.into();
        for variant in variants {
            assert_ne!(variant, original_inputs);
            let event = NormalizedHostEvent::try_new(variant).unwrap();
            assert_eq!(event.event_type(), original.event_type());
            let before = event.clone();
            for source_class in NormalizedHostEventSourceClass::ALL {
                assert_eq!(
                    validate_normalized_host_event_source(source_class, &event),
                    validate_normalized_host_event_source(source_class, &original),
                    "{event_type:?} + {source_class:?}"
                );
                assert_eq!(event, before);
            }
        }
    }
}
