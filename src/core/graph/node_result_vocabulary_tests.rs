use std::fmt::Debug;

use crate::{
    AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS, AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS,
    AUTHORIZED_PREFIX_TRANSITIONS, AUTHORIZED_REJECTED_REPAIR_TRANSITIONS,
    AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS, AUTHORIZED_REVIEW_VERDICT_TRANSITIONS,
    GraphNodeCheckResult, GraphNodeResultOutcome, GraphNodeState,
};

const OUTCOMES: [GraphNodeResultOutcome; 7] = [
    GraphNodeResultOutcome::Pass,
    GraphNodeResultOutcome::Fail,
    GraphNodeResultOutcome::Rejected,
    GraphNodeResultOutcome::Blocked,
    GraphNodeResultOutcome::Cancelled,
    GraphNodeResultOutcome::Skipped,
    GraphNodeResultOutcome::HumanRequired,
];
const OUTCOME_STRINGS: [&str; 7] = [
    "PASS",
    "FAIL",
    "REJECTED",
    "BLOCKED",
    "CANCELLED",
    "SKIPPED",
    "HUMAN_REQUIRED",
];
const CHECK_RESULTS: [GraphNodeCheckResult; 5] = [
    GraphNodeCheckResult::Pass,
    GraphNodeCheckResult::Fail,
    GraphNodeCheckResult::Error,
    GraphNodeCheckResult::Skipped,
    GraphNodeCheckResult::Unknown,
];
const CHECK_RESULT_STRINGS: [&str; 5] = ["PASS", "FAIL", "ERROR", "SKIPPED", "UNKNOWN"];

fn assert_pairwise_distinct<T: Debug + PartialEq>(values: &[T]) {
    for (index, value) in values.iter().enumerate() {
        for other in &values[index + 1..] {
            assert_ne!(value, other);
        }
    }
}

#[test]
fn outcome_vocabulary_matches_the_frozen_schema() {
    assert_eq!(GraphNodeResultOutcome::ALL.len(), 7);
    assert_eq!(GraphNodeResultOutcome::ALL, OUTCOMES);

    for (outcome, expected) in OUTCOMES.iter().zip(OUTCOME_STRINGS) {
        let exhaustive = match outcome {
            GraphNodeResultOutcome::Pass => "PASS",
            GraphNodeResultOutcome::Fail => "FAIL",
            GraphNodeResultOutcome::Rejected => "REJECTED",
            GraphNodeResultOutcome::Blocked => "BLOCKED",
            GraphNodeResultOutcome::Cancelled => "CANCELLED",
            GraphNodeResultOutcome::Skipped => "SKIPPED",
            GraphNodeResultOutcome::HumanRequired => "HUMAN_REQUIRED",
        };
        assert_eq!(outcome.as_str(), expected);
        assert_eq!(exhaustive, expected);
    }

    assert_pairwise_distinct(&GraphNodeResultOutcome::ALL);
    assert_pairwise_distinct(&OUTCOME_STRINGS);
}

#[test]
fn check_result_vocabulary_matches_the_frozen_schema() {
    assert_eq!(GraphNodeCheckResult::ALL.len(), 5);
    assert_eq!(GraphNodeCheckResult::ALL, CHECK_RESULTS);
    assert_eq!(GraphNodeCheckResult::ALL[2], GraphNodeCheckResult::Error);
    assert_eq!(GraphNodeCheckResult::ALL[4], GraphNodeCheckResult::Unknown);

    for (result, expected) in CHECK_RESULTS.iter().zip(CHECK_RESULT_STRINGS) {
        let exhaustive = match result {
            GraphNodeCheckResult::Pass => "PASS",
            GraphNodeCheckResult::Fail => "FAIL",
            GraphNodeCheckResult::Error => "ERROR",
            GraphNodeCheckResult::Skipped => "SKIPPED",
            GraphNodeCheckResult::Unknown => "UNKNOWN",
        };
        assert_eq!(result.as_str(), expected);
        assert_eq!(exhaustive, expected);
    }

    assert_pairwise_distinct(&GraphNodeCheckResult::ALL);
    assert_pairwise_distinct(&CHECK_RESULT_STRINGS);
}

#[test]
fn total_closed_value_count_is_twelve() {
    assert_eq!(
        GraphNodeResultOutcome::ALL.len() + GraphNodeCheckResult::ALL.len(),
        12
    );
}

#[test]
fn graph_state_and_lifecycle_contracts_are_unchanged() {
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
