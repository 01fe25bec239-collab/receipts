use std::fmt::Debug;

use crate::{
    AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS, AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS,
    AUTHORIZED_PREFIX_TRANSITIONS, AUTHORIZED_REJECTED_REPAIR_TRANSITIONS,
    AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS, AUTHORIZED_REVIEW_VERDICT_TRANSITIONS,
    GraphMutationOperationKind, GraphNodeCheckResult, GraphNodeResultOutcome, GraphNodeState,
};

fn assert_pairwise_distinct<T: Debug + PartialEq>(values: &[T]) {
    for (index, value) in values.iter().enumerate() {
        for other in &values[index + 1..] {
            assert_ne!(value, other);
        }
    }
}

#[test]
fn operation_vocabulary_matches_the_frozen_schema() {
    use GraphMutationOperationKind::{
        AddEdge, AddNode, AttachResult, CancelNode, ExpandRepair, SetNodeState,
    };

    const VALUES: [GraphMutationOperationKind; 6] = [
        AddNode,
        AddEdge,
        SetNodeState,
        AttachResult,
        CancelNode,
        ExpandRepair,
    ];
    const STRINGS: [&str; 6] = [
        "ADD_NODE",
        "ADD_EDGE",
        "SET_NODE_STATE",
        "ATTACH_RESULT",
        "CANCEL_NODE",
        "EXPAND_REPAIR",
    ];

    assert_eq!(GraphMutationOperationKind::ALL.len(), 6);
    assert_eq!(GraphMutationOperationKind::ALL, VALUES);
    assert_eq!(VALUES.map(|value| value.as_str()), STRINGS);
    for value in VALUES {
        assert_eq!(
            GraphMutationOperationKind::ALL
                .iter()
                .filter(|candidate| **candidate == value)
                .count(),
            1,
        );
    }
    assert_pairwise_distinct(&VALUES);
    assert_pairwise_distinct(&STRINGS);
}

#[test]
fn existing_result_vocabularies_are_unchanged() {
    use GraphNodeCheckResult::{Error, Fail as CheckFail, Pass as CheckPass, Skipped, Unknown};
    use GraphNodeResultOutcome::{
        Blocked, Cancelled, Fail, HumanRequired, Pass, Rejected, Skipped as OutcomeSkipped,
    };

    assert_eq!(
        GraphNodeResultOutcome::ALL,
        [
            Pass,
            Fail,
            Rejected,
            Blocked,
            Cancelled,
            OutcomeSkipped,
            HumanRequired,
        ]
    );
    assert_eq!(GraphNodeResultOutcome::ALL.len(), 7);
    assert_eq!(
        GraphNodeResultOutcome::ALL.map(|value| value.as_str()),
        [
            "PASS",
            "FAIL",
            "REJECTED",
            "BLOCKED",
            "CANCELLED",
            "SKIPPED",
            "HUMAN_REQUIRED",
        ]
    );

    assert_eq!(
        GraphNodeCheckResult::ALL,
        [CheckPass, CheckFail, Error, Skipped, Unknown]
    );
    assert_eq!(GraphNodeCheckResult::ALL.len(), 5);
    assert_eq!(
        GraphNodeCheckResult::ALL.map(|value| value.as_str()),
        ["PASS", "FAIL", "ERROR", "SKIPPED", "UNKNOWN"]
    );
}

#[test]
fn graph_state_and_lifecycle_edges_are_unchanged() {
    use GraphNodeState::{
        Accepted, Admitted, AwaitingReview, Dispatched, Integrated, Passed, Planned, Ready,
        Rejected, Repairing, Running,
    };

    assert_eq!(GraphNodeState::ALL.len(), 15);
    for state in GraphNodeState::ALL {
        assert_eq!(GraphNodeState::parse(state.as_str()), Ok(state));
    }
    assert_eq!(
        AUTHORIZED_PREFIX_TRANSITIONS,
        [
            (Planned, Ready),
            (Ready, Admitted),
            (Admitted, Dispatched),
            (Dispatched, Running),
            (Running, AwaitingReview),
        ]
    );
    assert_eq!(
        AUTHORIZED_REVIEW_VERDICT_TRANSITIONS,
        [(AwaitingReview, Passed), (AwaitingReview, Rejected)]
    );
    assert_eq!(
        AUTHORIZED_REJECTED_REPAIR_TRANSITIONS,
        [(Rejected, Repairing)]
    );
    assert_eq!(
        AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS,
        [(Repairing, AwaitingReview)]
    );
    assert_eq!(
        AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS,
        [(Passed, Accepted)]
    );
    assert_eq!(
        AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS,
        [(Accepted, Integrated)]
    );
    assert_eq!(
        AUTHORIZED_PREFIX_TRANSITIONS.len()
            + AUTHORIZED_REVIEW_VERDICT_TRANSITIONS.len()
            + AUTHORIZED_REJECTED_REPAIR_TRANSITIONS.len()
            + AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS.len()
            + AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS.len()
            + AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS.len(),
        11
    );
}
