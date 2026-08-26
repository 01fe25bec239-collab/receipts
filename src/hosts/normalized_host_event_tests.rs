//! Tests for the closed normalized host event vocabularies.
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
