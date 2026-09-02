use crate::{WorkspaceCheckpointKind, WorkspaceRecoveryDecision};

fn assert_checkpoint_kind_exhaustive(value: WorkspaceCheckpointKind) {
    match value {
        WorkspaceCheckpointKind::Progress
        | WorkspaceCheckpointKind::PreTermination
        | WorkspaceCheckpointKind::RecoveryCapture => {}
    }
}

fn assert_recovery_decision_exhaustive(value: WorkspaceRecoveryDecision) {
    match value {
        WorkspaceRecoveryDecision::ResetToLastAccepted
        | WorkspaceRecoveryDecision::ContinueFromCheckpoint
        | WorkspaceRecoveryDecision::InspectAndSalvage => {}
    }
}

#[test]
fn checkpoint_kind_vocabulary_is_exact_ordered_and_unique() {
    use WorkspaceCheckpointKind::{PreTermination, Progress, RecoveryCapture};

    assert_eq!(
        WorkspaceCheckpointKind::ALL,
        [Progress, PreTermination, RecoveryCapture]
    );
    assert_eq!(
        WorkspaceCheckpointKind::ALL.map(WorkspaceCheckpointKind::as_str),
        ["PROGRESS", "PRE_TERMINATION", "RECOVERY_CAPTURE"]
    );
    for (index, value) in WorkspaceCheckpointKind::ALL.iter().enumerate() {
        assert_checkpoint_kind_exhaustive(*value);
        for other in &WorkspaceCheckpointKind::ALL[index + 1..] {
            assert_ne!(value, other);
            assert_ne!(value.as_str(), other.as_str());
        }
    }
}

#[test]
fn recovery_decision_vocabulary_is_exact_ordered_and_unique() {
    use WorkspaceRecoveryDecision::{
        ContinueFromCheckpoint, InspectAndSalvage, ResetToLastAccepted,
    };

    assert_eq!(
        WorkspaceRecoveryDecision::ALL,
        [
            ResetToLastAccepted,
            ContinueFromCheckpoint,
            InspectAndSalvage
        ]
    );
    assert_eq!(
        WorkspaceRecoveryDecision::ALL.map(WorkspaceRecoveryDecision::as_str),
        [
            "RESET_TO_LAST_ACCEPTED",
            "CONTINUE_FROM_CHECKPOINT",
            "INSPECT_AND_SALVAGE"
        ]
    );
    for (index, value) in WorkspaceRecoveryDecision::ALL.iter().enumerate() {
        assert_recovery_decision_exhaustive(*value);
        for other in &WorkspaceRecoveryDecision::ALL[index + 1..] {
            assert_ne!(value, other);
            assert_ne!(value.as_str(), other.as_str());
        }
    }
}

#[test]
fn total_closed_vocabulary_contains_exactly_six_values() {
    assert_eq!(
        WorkspaceCheckpointKind::ALL.len() + WorkspaceRecoveryDecision::ALL.len(),
        6
    );
}
