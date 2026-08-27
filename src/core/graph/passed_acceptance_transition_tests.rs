//! Exhaustive coverage for the bounded `PASSED → ACCEPTED` validator.

use crate::node_state::GraphNodeState;
use crate::node_state_transition::{AUTHORIZED_PREFIX_TRANSITIONS, validate_prefix_transition};
use crate::passed_acceptance_transition::{
    AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS, PassedAcceptanceTransitionError,
    validate_passed_acceptance_transition,
};
use crate::rejected_repair_transition::{
    AUTHORIZED_REJECTED_REPAIR_TRANSITIONS, validate_rejected_repair_transition,
};
use crate::repair_completion_transition::{
    AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS, validate_repair_completion_transition,
};
use crate::review_verdict_transition::{
    AUTHORIZED_REVIEW_VERDICT_TRANSITIONS, validate_review_verdict_transition,
};

#[test]
fn exactly_passed_to_accepted_is_declared_and_succeeds() {
    assert_eq!(AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS.len(), 1);
    assert_eq!(
        AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS[0],
        (GraphNodeState::Passed, GraphNodeState::Accepted),
    );
    assert_eq!(
        validate_passed_acceptance_transition(GraphNodeState::Passed, GraphNodeState::Accepted),
        Ok(()),
    );
}

#[test]
fn exhaustive_matrix_classifies_exactly_1_14_210_and_preserves_payloads() {
    let mut success_count = 0usize;
    let mut unsupported_count = 0usize;
    let mut outside_scope_count = 0usize;

    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            match validate_passed_acceptance_transition(from, to) {
                Ok(()) => {
                    success_count += 1;
                    assert_eq!(
                        (from, to),
                        (GraphNodeState::Passed, GraphNodeState::Accepted)
                    );
                }
                Err(error) => {
                    assert_eq!(error.from(), from);
                    assert_eq!(error.to(), to);
                    if matches!(from, GraphNodeState::Passed) {
                        unsupported_count += 1;
                        assert_eq!(
                            error,
                            PassedAcceptanceTransitionError::UnsupportedPassedAcceptanceTarget {
                                from,
                                to,
                            },
                        );
                        assert!(error.is_unsupported_passed_acceptance_target());
                        assert!(!error.is_outside_passed_acceptance_scope());
                    } else {
                        outside_scope_count += 1;
                        assert_eq!(
                            error,
                            PassedAcceptanceTransitionError::OutsidePassedAcceptanceScope {
                                from,
                                to,
                            },
                        );
                        assert!(error.is_outside_passed_acceptance_scope());
                        assert!(!error.is_unsupported_passed_acceptance_target());
                    }
                }
            }
        }
    }

    assert_eq!(success_count, 1);
    assert_eq!(unsupported_count, 14);
    assert_eq!(outside_scope_count, 210);
    assert_eq!(success_count + unsupported_count + outside_scope_count, 225);
}

#[test]
fn every_non_accepted_target_from_passed_is_unsupported() {
    let mut checked = 0usize;
    for to in GraphNodeState::ALL {
        if matches!(to, GraphNodeState::Accepted) {
            continue;
        }
        checked += 1;
        assert_eq!(
            validate_passed_acceptance_transition(GraphNodeState::Passed, to),
            Err(
                PassedAcceptanceTransitionError::UnsupportedPassedAcceptanceTarget {
                    from: GraphNodeState::Passed,
                    to,
                }
            ),
        );
    }
    assert_eq!(checked, 14);
}

#[test]
fn every_non_passed_source_is_outside_scope_for_every_target() {
    let mut checked = 0usize;
    for from in GraphNodeState::ALL {
        if matches!(from, GraphNodeState::Passed) {
            continue;
        }
        for to in GraphNodeState::ALL {
            checked += 1;
            assert_eq!(
                validate_passed_acceptance_transition(from, to),
                Err(PassedAcceptanceTransitionError::OutsidePassedAcceptanceScope { from, to }),
            );
        }
    }
    assert_eq!(checked, 210);
}

#[test]
fn named_unsupported_passed_targets_fail() {
    let targets = [
        GraphNodeState::Passed,
        GraphNodeState::Rejected,
        GraphNodeState::Repairing,
        GraphNodeState::Integrated,
        GraphNodeState::AwaitingReview,
        GraphNodeState::Blocked,
        GraphNodeState::Cancelled,
        GraphNodeState::HumanRequired,
        GraphNodeState::LockedRequiresPro,
    ];
    for to in targets {
        assert_eq!(
            validate_passed_acceptance_transition(GraphNodeState::Passed, to),
            Err(
                PassedAcceptanceTransitionError::UnsupportedPassedAcceptanceTarget {
                    from: GraphNodeState::Passed,
                    to,
                }
            ),
        );
    }
}

#[test]
fn directionality_and_future_integration_edge_are_excluded() {
    assert_eq!(
        validate_passed_acceptance_transition(GraphNodeState::Accepted, GraphNodeState::Passed),
        Err(
            PassedAcceptanceTransitionError::OutsidePassedAcceptanceScope {
                from: GraphNodeState::Accepted,
                to: GraphNodeState::Passed,
            }
        ),
    );
    assert_eq!(
        validate_passed_acceptance_transition(GraphNodeState::Accepted, GraphNodeState::Integrated,),
        Err(
            PassedAcceptanceTransitionError::OutsidePassedAcceptanceScope {
                from: GraphNodeState::Accepted,
                to: GraphNodeState::Integrated,
            }
        ),
    );
}

#[test]
fn repeated_full_matrix_evaluation_is_deterministic() {
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            let expected = validate_passed_acceptance_transition(from, to);
            for _ in 0..4 {
                assert_eq!(validate_passed_acceptance_transition(from, to), expected);
            }
        }
    }
}

#[test]
fn previous_bounded_edges_remain_accepted() {
    const PREFIX: [(GraphNodeState, GraphNodeState); 5] = [
        (GraphNodeState::Planned, GraphNodeState::Ready),
        (GraphNodeState::Ready, GraphNodeState::Admitted),
        (GraphNodeState::Admitted, GraphNodeState::Dispatched),
        (GraphNodeState::Dispatched, GraphNodeState::Running),
        (GraphNodeState::Running, GraphNodeState::AwaitingReview),
    ];

    assert_eq!(AUTHORIZED_PREFIX_TRANSITIONS, PREFIX);
    for (from, to) in PREFIX {
        assert_eq!(validate_prefix_transition(from, to), Ok(()));
    }
    assert_eq!(AUTHORIZED_REVIEW_VERDICT_TRANSITIONS.len(), 2);
    assert_eq!(
        validate_review_verdict_transition(GraphNodeState::AwaitingReview, GraphNodeState::Passed),
        Ok(()),
    );
    assert_eq!(
        validate_review_verdict_transition(
            GraphNodeState::AwaitingReview,
            GraphNodeState::Rejected,
        ),
        Ok(()),
    );
    assert_eq!(AUTHORIZED_REJECTED_REPAIR_TRANSITIONS.len(), 1);
    assert_eq!(
        validate_rejected_repair_transition(GraphNodeState::Rejected, GraphNodeState::Repairing),
        Ok(()),
    );
    assert_eq!(AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS.len(), 1);
    assert_eq!(
        validate_repair_completion_transition(
            GraphNodeState::Repairing,
            GraphNodeState::AwaitingReview,
        ),
        Ok(()),
    );
}

#[test]
fn five_validator_success_sets_are_disjoint_and_total_exactly_ten_edges() {
    let mut aggregate = 0usize;
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            let successes = [
                validate_prefix_transition(from, to).is_ok(),
                validate_review_verdict_transition(from, to).is_ok(),
                validate_rejected_repair_transition(from, to).is_ok(),
                validate_repair_completion_transition(from, to).is_ok(),
                validate_passed_acceptance_transition(from, to).is_ok(),
            ];
            let count = successes.iter().filter(|success| **success).count();
            assert!(
                count <= 1,
                "{from:?} → {to:?} succeeded in multiple validators"
            );
            aggregate += count;
        }
    }
    assert_eq!(aggregate, 10);
}

#[test]
fn state_vocabulary_and_parsing_remain_frozen() {
    assert_eq!(GraphNodeState::ALL.len(), 15);
    for state in GraphNodeState::ALL {
        assert_eq!(GraphNodeState::parse(state.as_str()), Ok(state));
    }
    assert!(GraphNodeState::parse("FAILED").is_err());
}
