use super::*;
use receipts_orchestration::orchestration::{
    OrchestrationDateTimeV1, OrchestrationJsonObjectV1, OrchestrationJsonValueV1,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

struct EmissionAdapter<'a, T> {
    expected_event: &'a NormalizedHostEvent,
    calls: Cell<usize>,
    outcome: RefCell<Option<T>>,
}

impl<T> HostAdapter for EmissionAdapter<'_, T> {
    type DetectOutcome = ();
    type InstallPlan = ();
    type InstallOutcome = ();
    type CoreHandle = ();
    type EmitOutcome = T;
    type CoreView = ();
    type PresentOutcome = ();
    type UserPrompt = ();
    type UserResponse = ();
    type UserInputPending = std::future::Ready<()>;
    type ShutdownReason = ();
    type ShutdownOutcome = ();

    fn id(&self) -> HostId {
        panic!("validated emission must not consult adapter identity")
    }

    fn detect(&self) {
        panic!("unexpected detect")
    }

    fn install(&self, _: &()) {
        panic!("unexpected install")
    }

    fn start(&self) {
        panic!("unexpected start")
    }

    fn emit(&self, event: &NormalizedHostEvent) -> T {
        self.calls.set(self.calls.get() + 1);
        assert!(std::ptr::eq(event, self.expected_event));
        self.outcome.borrow_mut().take().expect("emit only once")
    }

    fn present(&self, _: &()) {
        panic!("unexpected present")
    }

    fn request_user_input(&mut self, _: ()) -> Self::UserInputPending {
        panic!("unexpected request_user_input")
    }

    fn capabilities(&self) -> HostCapabilityReport {
        panic!("unexpected capabilities")
    }

    fn shutdown(self, _: ()) {
        panic!("unexpected shutdown")
    }
}

// No Clone, Debug, Eq, Send, Sync, Future, or Result contract; borrows are allowed.
struct OpaqueOutcome<'a> {
    marker: &'a str,
    identity: Rc<Cell<u32>>,
}

fn assert_validated_emission(event: &NormalizedHostEvent) -> (usize, usize) {
    let before = event.clone();
    let marker = String::from("distinctive adapter outcome: A3-028");
    let mut allowed = 0;
    let mut rejected = 0;
    for source_class in NormalizedHostEventSourceClass::ALL {
        let identity = Rc::new(Cell::new(0xA3028));
        let adapter = EmissionAdapter {
            expected_event: event,
            calls: Cell::new(0),
            outcome: RefCell::new(Some(OpaqueOutcome {
                marker: &marker,
                identity: Rc::clone(&identity),
            })),
        };
        let expected = validate_normalized_host_event_source(source_class, event);
        let actual = emit_validated_normalized_host_event(&adapter, source_class, event);
        match (expected, actual) {
            (Ok(()), Ok(outcome)) => {
                assert_eq!(adapter.calls.get(), 1);
                assert!(adapter.outcome.borrow().is_none());
                assert!(std::ptr::eq(outcome.marker, marker.as_str()));
                assert!(Rc::ptr_eq(&outcome.identity, &identity));
                assert_eq!(outcome.identity.get(), 0xA3028);
                allowed += 1;
            }
            (Err(expected), Err(actual)) => {
                assert_eq!(actual, expected);
                assert_eq!(adapter.calls.get(), 0);
                assert!(adapter.outcome.borrow().is_some());
                rejected += 1;
            }
            _ => panic!("bridge disagrees with validator: {source_class:?}"),
        }
        assert_eq!(event, &before);
    }
    (allowed, rejected)
}

#[test]
fn validated_emit_preserves_reference_outcome_and_event_for_all_64_pairs() {
    let mut totals = (0, 0);
    for event_type in NormalizedHostEventType::ALL {
        let event = NormalizedHostEvent::try_new(inputs(event_type)).unwrap();
        let (allowed, rejected) = assert_validated_emission(&event);
        totals.0 += allowed;
        totals.1 += rejected;
    }
    assert_eq!(totals, (17, 47));
}

#[test]
fn validated_emit_uses_explicit_source_despite_source_like_event_contents() {
    for confidence in NormalizedHostEventConfidence::ALL {
        for source_hint in NormalizedHostEventSourceClass::ALL {
            let mut fields = inputs(NormalizedHostEventType::ToolExecuted);
            fields.confidence = confidence;
            fields.host = NormalizedHostEventHost::try_new(source_hint.as_str()).unwrap();
            fields.payload = OrchestrationJsonObjectV1::new(std::collections::BTreeMap::from([(
                "source_class".to_owned(),
                OrchestrationJsonValueV1::String(source_hint.as_str().to_owned()),
            )]));
            fields.raw_ref = Some(
                NormalizedHostEventRawRef::try_new(
                    NormalizedHostEventRawRefType::ArtifactId,
                    source_hint.as_str().to_owned(),
                    Some("unchanged-digest".to_owned()),
                    Some("unchanged-section".to_owned()),
                )
                .unwrap(),
            );
            let event = NormalizedHostEvent::try_new(fields).unwrap();
            assert_eq!(assert_validated_emission(&event), (2, 2));
        }
    }
}

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
