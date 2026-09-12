//! Tests for normalized host event physical boundaries and closed vocabularies.
//!
//! Both contract tables below are exhaustive matches, so adding or removing
//! a variant breaks compilation here until the contract coverage is updated,
//! and the position check then fails until `ALL` is updated too.

use std::collections::HashSet;

use super::*;

/// Contract table for event types: position in `ALL` and canonical string.
fn event_type_contract(event_type: NormalizedHostEventType) -> (usize, &'static str) {
    match event_type {
        NormalizedHostEventType::HostSessionStarted => (0, "HOST_SESSION_STARTED"),
        NormalizedHostEventType::HostSessionEnding => (1, "HOST_SESSION_ENDING"),
        NormalizedHostEventType::UserGoalSubmitted => (2, "USER_GOAL_SUBMITTED"),
        NormalizedHostEventType::UserInputProvided => (3, "USER_INPUT_PROVIDED"),
        NormalizedHostEventType::RoleExecutorStarted => (4, "ROLE_EXECUTOR_STARTED"),
        NormalizedHostEventType::RoleExecutorStopped => (5, "ROLE_EXECUTOR_STOPPED"),
        NormalizedHostEventType::TaskStarted => (6, "TASK_STARTED"),
        NormalizedHostEventType::TaskCompleted => (7, "TASK_COMPLETED"),
        NormalizedHostEventType::TaskFailed => (8, "TASK_FAILED"),
        NormalizedHostEventType::ToolExecuted => (9, "TOOL_EXECUTED"),
        NormalizedHostEventType::WorkspaceCreated => (10, "WORKSPACE_CREATED"),
        NormalizedHostEventType::WorkspaceChanged => (11, "WORKSPACE_CHANGED"),
        NormalizedHostEventType::WorkspaceRemoved => (12, "WORKSPACE_REMOVED"),
        NormalizedHostEventType::ContextCompacted => (13, "CONTEXT_COMPACTED"),
        NormalizedHostEventType::ProviderSignal => (14, "PROVIDER_SIGNAL"),
        NormalizedHostEventType::HostError => (15, "HOST_ERROR"),
    }
}

/// Contract table for confidence values.
fn confidence_contract(confidence: NormalizedHostEventConfidence) -> (usize, &'static str) {
    match confidence {
        NormalizedHostEventConfidence::Observed => (0, "OBSERVED"),
        NormalizedHostEventConfidence::Inferred => (1, "INFERRED"),
    }
}

/// All sixteen event types render their exact canonical string.
#[test]
fn every_event_type_maps_to_its_canonical_string() {
    for event_type in NormalizedHostEventType::ALL {
        let (_, canonical) = event_type_contract(event_type);
        assert_eq!(event_type.as_str(), canonical);
    }
}

/// The event vocabulary is exactly sixteen distinct values, and every value
/// the contract table knows sits at its declared position in `ALL`.
#[test]
fn event_type_vocabulary_is_exactly_sixteen_values() {
    assert_eq!(NormalizedHostEventType::ALL.len(), 16);

    let distinct: HashSet<NormalizedHostEventType> =
        NormalizedHostEventType::ALL.iter().copied().collect();
    assert_eq!(distinct.len(), 16, "event types must be pairwise distinct");

    let distinct_strings: HashSet<&'static str> = NormalizedHostEventType::ALL
        .iter()
        .map(|event_type| event_type.as_str())
        .collect();
    assert_eq!(
        distinct_strings.len(),
        16,
        "canonical strings must be pairwise distinct"
    );

    for event_type in NormalizedHostEventType::ALL {
        let (index, _) = event_type_contract(event_type);
        assert_eq!(
            NormalizedHostEventType::ALL[index],
            event_type,
            "{} is missing from ALL at its contract position",
            event_type.as_str()
        );
    }
}

/// Both confidence values render their exact canonical string.
#[test]
fn every_confidence_maps_to_its_canonical_string() {
    assert_eq!(NormalizedHostEventConfidence::Observed.as_str(), "OBSERVED");
    assert_eq!(NormalizedHostEventConfidence::Inferred.as_str(), "INFERRED");

    for confidence in NormalizedHostEventConfidence::ALL {
        let (_, canonical) = confidence_contract(confidence);
        assert_eq!(confidence.as_str(), canonical);
    }
}

/// The confidence vocabulary is exactly two distinct values.
#[test]
fn confidence_vocabulary_is_exactly_two_values() {
    assert_eq!(NormalizedHostEventConfidence::ALL.len(), 2);

    let distinct: HashSet<NormalizedHostEventConfidence> =
        NormalizedHostEventConfidence::ALL.iter().copied().collect();
    assert_eq!(distinct.len(), 2, "confidence values must be distinct");

    for confidence in NormalizedHostEventConfidence::ALL {
        let (index, _) = confidence_contract(confidence);
        assert_eq!(
            NormalizedHostEventConfidence::ALL[index],
            confidence,
            "{} is missing from ALL at its contract position",
            confidence.as_str()
        );
    }
}

use receipts_orchestration::orchestration::{
    OrchestrationDateTimeV1, OrchestrationJsonObjectV1, OrchestrationJsonValueV1,
};

fn envelope_inputs() -> NormalizedHostEventInputs {
    NormalizedHostEventInputs {
        event_id: NormalizedHostEventId::try_new("Z123456789ABCDEFGHJKMNPQRS").unwrap(),
        event_type: NormalizedHostEventType::HostSessionStarted,
        host: HostId::ClaudeCode.into(),
        host_session_id: "session".to_owned(),
        project_id: None,
        occurred_at: OrchestrationDateTimeV1::try_new("2026-09-12t00:00:00.100z").unwrap(),
        payload: OrchestrationJsonObjectV1::default(),
        raw_ref: None,
        confidence: NormalizedHostEventConfidence::Observed,
    }
}

#[test]
fn event_id_accepts_exact_alphabet_at_every_position_without_normalization() {
    // Every allowed byte can also be first, including 8, 9, and Z.
    for byte in b"0123456789ABCDEFGHJKMNPQRSTVWXYZ" {
        let supplied = char::from(*byte).to_string().repeat(26);
        let event_id = NormalizedHostEventId::try_new(supplied.clone()).unwrap();
        assert_eq!(event_id.as_str().as_bytes(), supplied.as_bytes());
    }
    let supplied = "Z123456789ABCDEFGHJKMNPQRS";
    assert_eq!(
        NormalizedHostEventId::try_new(supplied).unwrap().as_str(),
        supplied
    );
}

#[test]
fn event_id_rejects_wrong_lengths_and_non_ascii() {
    for invalid in [
        String::new(),
        "0".repeat(25),
        "0".repeat(27),
        "é".repeat(13), // Exactly 26 bytes, but not ASCII.
        "é".repeat(26), // Exactly 26 characters, but not 26 ASCII bytes.
    ] {
        assert_eq!(
            NormalizedHostEventId::try_new(invalid),
            Err(NormalizedHostEventError::InvalidEventId)
        );
    }
}

#[test]
fn event_id_rejects_every_disallowed_ascii_byte_at_every_position() {
    // Includes all lowercase letters, I/L/O/U, whitespace and punctuation.
    for byte in 0..=127u8 {
        if b"0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(&byte) {
            continue;
        }
        for position in 0..26 {
            let mut invalid = vec![b'0'; 26];
            invalid[position] = byte;
            assert_eq!(
                NormalizedHostEventId::try_new(String::from_utf8(invalid).unwrap()),
                Err(NormalizedHostEventError::InvalidEventId),
                "byte {byte}, position {position}",
            );
        }
    }
}

#[test]
fn event_host_is_open_non_empty_and_preserved() {
    for supplied in [
        "future-host/v2",
        "未来のホスト",
        " ",
        " Future Host ",
        "CODEX",
    ] {
        let host = NormalizedHostEventHost::try_new(supplied).unwrap();
        assert_eq!(host.as_str(), supplied);
        let mut inputs = envelope_inputs();
        inputs.host = host;
        assert_eq!(
            NormalizedHostEvent::try_new(inputs)
                .unwrap()
                .host()
                .as_str(),
            supplied
        );
    }
    assert_eq!(
        NormalizedHostEventHost::try_new(""),
        Err(NormalizedHostEventError::EmptyHost)
    );
}

#[test]
fn current_host_ids_convert_to_event_values_not_diagnostics() {
    for (id, canonical) in [
        (HostId::ClaudeCode, "claude-code"),
        (HostId::Codex, "codex"),
        (HostId::Headless, "headless"),
    ] {
        let host = NormalizedHostEventHost::from(id);
        assert_eq!(host.as_str(), canonical);
        assert_ne!(host.as_str(), id.as_str());
    }
}

#[test]
fn session_and_project_boundaries_count_characters_not_bytes() {
    for character in ["a", "界", "🦀"] {
        for length in [0, 1, 200, 201] {
            let supplied = character.repeat(length);
            let mut inputs = envelope_inputs();
            inputs.host_session_id = supplied.clone();
            let session = NormalizedHostEvent::try_new(inputs);
            let mut inputs = envelope_inputs();
            inputs.project_id = Some(supplied.clone());
            let project = NormalizedHostEvent::try_new(inputs);
            if length == 1 || length == 200 {
                assert_eq!(session.unwrap().host_session_id(), supplied);
                assert_eq!(project.unwrap().project_id(), Some(supplied.as_str()));
            } else {
                assert_eq!(session, Err(NormalizedHostEventError::InvalidHostSessionId));
                assert_eq!(project, Err(NormalizedHostEventError::InvalidProjectId));
            }
        }
    }
    let mut inputs = envelope_inputs();
    inputs.host_session_id = " ".to_owned();
    inputs.project_id = Some(" ".to_owned());
    let event = NormalizedHostEvent::try_new(inputs).unwrap();
    assert_eq!(event.host_session_id(), " ");
    assert_eq!(event.project_id(), Some(" "));
}

#[test]
fn construction_errors_have_deterministic_precedence() {
    let mut inputs = envelope_inputs();
    inputs.host_session_id.clear();
    inputs.project_id = Some(String::new());
    assert_eq!(
        NormalizedHostEvent::try_new(inputs),
        Err(NormalizedHostEventError::InvalidHostSessionId)
    );
}

#[test]
fn envelope_carries_exact_fields_and_orchestration_types() {
    let inputs = envelope_inputs();
    let event = NormalizedHostEvent::try_new(inputs.clone()).unwrap();
    assert_eq!(event.event_id(), &inputs.event_id);
    assert_eq!(event.event_type(), inputs.event_type);
    assert_eq!(event.host(), &inputs.host);
    assert_eq!(event.host_session_id(), inputs.host_session_id);
    assert_eq!(event.project_id(), None);
    let occurred_at: &OrchestrationDateTimeV1 = event.occurred_at();
    assert_eq!(occurred_at, &inputs.occurred_at);
    assert_eq!(occurred_at.as_str(), "2026-09-12t00:00:00.100z");
    let payload: &OrchestrationJsonObjectV1 = event.payload();
    assert!(payload.as_map().is_empty());
    assert_eq!(event.raw_ref(), None);
    assert_eq!(event.confidence(), inputs.confidence);
}

#[test]
fn generic_payload_is_preserved_for_all_event_types_and_confidences() {
    let nested = OrchestrationJsonObjectV1::new(std::collections::BTreeMap::from([(
        "unknown producer field".to_owned(),
        OrchestrationJsonValueV1::Boolean(true),
    )]));
    let mut payload = OrchestrationJsonObjectV1::default();
    payload.insert(
        "".to_owned(),
        OrchestrationJsonValueV1::Array(vec![
            OrchestrationJsonValueV1::Null,
            OrchestrationJsonValueV1::String(" arbitrary data 🦀 ".to_owned()),
            OrchestrationJsonValueV1::Object(nested),
            OrchestrationJsonValueV1::Null,
        ]),
    );
    for event_type in NormalizedHostEventType::ALL {
        for confidence in NormalizedHostEventConfidence::ALL {
            for payload in [OrchestrationJsonObjectV1::default(), payload.clone()] {
                let mut inputs = envelope_inputs();
                inputs.event_type = event_type;
                inputs.confidence = confidence;
                inputs.payload = payload.clone();
                let event = NormalizedHostEvent::try_new(inputs).unwrap();
                assert_eq!(event.event_type(), event_type);
                assert_eq!(event.confidence(), confidence);
                assert_eq!(event.payload(), &payload);
            }
        }
    }
}

fn raw_ref_contract(ref_type: NormalizedHostEventRawRefType) -> &'static str {
    match ref_type {
        NormalizedHostEventRawRefType::RepoPath => "REPO_PATH",
        NormalizedHostEventRawRefType::StateQuery => "STATE_QUERY",
        NormalizedHostEventRawRefType::ArtifactId => "ARTIFACT_ID",
        NormalizedHostEventRawRefType::Url => "URL",
    }
}

#[test]
fn raw_ref_has_four_kinds_non_empty_target_and_independent_optional_strings() {
    for ref_type in [
        NormalizedHostEventRawRefType::RepoPath,
        NormalizedHostEventRawRefType::StateQuery,
        NormalizedHostEventRawRefType::ArtifactId,
        NormalizedHostEventRawRefType::Url,
    ] {
        assert_eq!(ref_type.as_str(), raw_ref_contract(ref_type));
        assert_eq!(
            NormalizedHostEventRawRef::try_new(ref_type, String::new(), None, None),
            Err(NormalizedHostEventError::EmptyRawRefTarget),
        );
        for target in ["raw/material", "https://example.invalid/raw", "参照", " "] {
            for digest in [None, Some(""), Some("digest")] {
                for section in [None, Some(""), Some("section")] {
                    let reference = NormalizedHostEventRawRef::try_new(
                        ref_type,
                        target.to_owned(),
                        digest.map(str::to_owned),
                        section.map(str::to_owned),
                    )
                    .unwrap();
                    assert_eq!(reference.ref_type(), ref_type);
                    assert_eq!(reference.target(), target);
                    assert_eq!(reference.digest(), digest);
                    assert_eq!(reference.section(), section);
                    let mut inputs = envelope_inputs();
                    inputs.raw_ref = Some(reference.clone());
                    let event = NormalizedHostEvent::try_new(inputs).unwrap();
                    assert_eq!(event.raw_ref(), Some(&reference));
                }
            }
        }
    }
}
