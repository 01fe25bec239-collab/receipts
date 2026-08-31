//! Exhaustive coverage for the bounded `ACCEPTED → INTEGRATED` validator.

use crate::accepted_integration_transition::{
    AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS, AcceptedIntegrationTransitionError,
    validate_accepted_integration_transition,
};
use crate::node_state::GraphNodeState;
use crate::node_state_transition::{AUTHORIZED_PREFIX_TRANSITIONS, validate_prefix_transition};
use crate::passed_acceptance_transition::{
    AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS, validate_passed_acceptance_transition,
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
fn exactly_accepted_to_integrated_is_declared_and_succeeds() {
    assert_eq!(AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS.len(), 1);
    assert_eq!(
        AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS[0],
        (GraphNodeState::Accepted, GraphNodeState::Integrated),
    );
    assert_eq!(
        validate_accepted_integration_transition(
            GraphNodeState::Accepted,
            GraphNodeState::Integrated,
        ),
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
            match validate_accepted_integration_transition(from, to) {
                Ok(()) => {
                    success_count += 1;
                    assert_eq!(
                        (from, to),
                        (GraphNodeState::Accepted, GraphNodeState::Integrated)
                    );
                }
                Err(error) => {
                    assert_eq!(error.from(), from);
                    assert_eq!(error.to(), to);
                    if matches!(from, GraphNodeState::Accepted) {
                        unsupported_count += 1;
                        assert_eq!(
                            error,
                            AcceptedIntegrationTransitionError::UnsupportedAcceptedIntegrationTarget {
                                from,
                                to,
                            },
                        );
                        assert!(error.is_unsupported_accepted_integration_target());
                        assert!(!error.is_outside_accepted_integration_scope());
                    } else {
                        outside_scope_count += 1;
                        assert_eq!(
                            error,
                            AcceptedIntegrationTransitionError::OutsideAcceptedIntegrationScope {
                                from,
                                to,
                            },
                        );
                        assert!(error.is_outside_accepted_integration_scope());
                        assert!(!error.is_unsupported_accepted_integration_target());
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
fn every_non_integrated_target_from_accepted_is_unsupported() {
    let targets = [
        GraphNodeState::Planned,
        GraphNodeState::Ready,
        GraphNodeState::Admitted,
        GraphNodeState::Dispatched,
        GraphNodeState::Running,
        GraphNodeState::AwaitingReview,
        GraphNodeState::Passed,
        GraphNodeState::Rejected,
        GraphNodeState::Repairing,
        GraphNodeState::Accepted,
        GraphNodeState::Blocked,
        GraphNodeState::LockedRequiresPro,
        GraphNodeState::Cancelled,
        GraphNodeState::HumanRequired,
    ];
    assert_eq!(targets.len(), 14);
    for to in targets {
        assert_eq!(
            validate_accepted_integration_transition(GraphNodeState::Accepted, to),
            Err(
                AcceptedIntegrationTransitionError::UnsupportedAcceptedIntegrationTarget {
                    from: GraphNodeState::Accepted,
                    to,
                }
            ),
        );
    }
}

#[test]
fn every_non_accepted_source_is_outside_scope_for_every_target() {
    let mut checked = 0usize;
    for from in GraphNodeState::ALL {
        if matches!(from, GraphNodeState::Accepted) {
            continue;
        }
        for to in GraphNodeState::ALL {
            checked += 1;
            assert_eq!(
                validate_accepted_integration_transition(from, to),
                Err(
                    AcceptedIntegrationTransitionError::OutsideAcceptedIntegrationScope {
                        from,
                        to,
                    }
                ),
            );
        }
    }
    assert_eq!(checked, 210);
}

#[test]
fn no_transition_out_of_integrated_succeeds() {
    for to in GraphNodeState::ALL {
        assert_eq!(
            validate_accepted_integration_transition(GraphNodeState::Integrated, to),
            Err(
                AcceptedIntegrationTransitionError::OutsideAcceptedIntegrationScope {
                    from: GraphNodeState::Integrated,
                    to,
                }
            ),
        );
    }
    assert!(
        validate_accepted_integration_transition(
            GraphNodeState::Integrated,
            GraphNodeState::Accepted,
        )
        .is_err()
    );
    assert!(
        validate_accepted_integration_transition(
            GraphNodeState::Integrated,
            GraphNodeState::Integrated,
        )
        .is_err()
    );
}

#[test]
fn repeated_full_matrix_evaluation_is_deterministic() {
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            let expected = validate_accepted_integration_transition(from, to);
            for _ in 0..4 {
                assert_eq!(validate_accepted_integration_transition(from, to), expected);
            }
        }
    }
}

#[test]
fn previous_bounded_edges_remain_exactly_declared_and_accepted() {
    const PREFIX: [(GraphNodeState, GraphNodeState); 5] = [
        (GraphNodeState::Planned, GraphNodeState::Ready),
        (GraphNodeState::Ready, GraphNodeState::Admitted),
        (GraphNodeState::Admitted, GraphNodeState::Dispatched),
        (GraphNodeState::Dispatched, GraphNodeState::Running),
        (GraphNodeState::Running, GraphNodeState::AwaitingReview),
    ];
    const REVIEW: [(GraphNodeState, GraphNodeState); 2] = [
        (GraphNodeState::AwaitingReview, GraphNodeState::Passed),
        (GraphNodeState::AwaitingReview, GraphNodeState::Rejected),
    ];

    assert_eq!(AUTHORIZED_PREFIX_TRANSITIONS, PREFIX);
    assert_eq!(AUTHORIZED_REVIEW_VERDICT_TRANSITIONS, REVIEW);
    assert_eq!(
        AUTHORIZED_REJECTED_REPAIR_TRANSITIONS,
        [(GraphNodeState::Rejected, GraphNodeState::Repairing)]
    );
    assert_eq!(
        AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS,
        [(GraphNodeState::Repairing, GraphNodeState::AwaitingReview)]
    );
    assert_eq!(
        AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS,
        [(GraphNodeState::Passed, GraphNodeState::Accepted)]
    );

    for (from, to) in PREFIX {
        assert_eq!(validate_prefix_transition(from, to), Ok(()));
    }
    for (from, to) in REVIEW {
        assert_eq!(validate_review_verdict_transition(from, to), Ok(()));
    }
    assert_eq!(
        validate_rejected_repair_transition(GraphNodeState::Rejected, GraphNodeState::Repairing),
        Ok(())
    );
    assert_eq!(
        validate_repair_completion_transition(
            GraphNodeState::Repairing,
            GraphNodeState::AwaitingReview,
        ),
        Ok(())
    );
    assert_eq!(
        validate_passed_acceptance_transition(GraphNodeState::Passed, GraphNodeState::Accepted),
        Ok(())
    );

    assert_eq!(
        AUTHORIZED_PREFIX_TRANSITIONS.len()
            + AUTHORIZED_REVIEW_VERDICT_TRANSITIONS.len()
            + AUTHORIZED_REJECTED_REPAIR_TRANSITIONS.len()
            + AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS.len()
            + AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS.len(),
        10
    );
}

#[test]
fn six_validator_success_sets_are_disjoint_and_total_exactly_eleven_edges() {
    let mut aggregate = 0usize;
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            let successes = [
                validate_prefix_transition(from, to).is_ok(),
                validate_review_verdict_transition(from, to).is_ok(),
                validate_rejected_repair_transition(from, to).is_ok(),
                validate_repair_completion_transition(from, to).is_ok(),
                validate_passed_acceptance_transition(from, to).is_ok(),
                validate_accepted_integration_transition(from, to).is_ok(),
            ];
            let count = successes.iter().filter(|success| **success).count();
            assert!(
                count <= 1,
                "{from:?} → {to:?} succeeded in multiple validators"
            );
            aggregate += count;
        }
    }
    assert_eq!(aggregate, 11);
}

#[test]
fn state_vocabulary_and_parsing_remain_frozen() {
    assert_eq!(GraphNodeState::ALL.len(), 15);
    for state in GraphNodeState::ALL {
        assert_eq!(GraphNodeState::parse(state.as_str()), Ok(state));
    }
    assert!(GraphNodeState::parse("FAILED").is_err());
}
