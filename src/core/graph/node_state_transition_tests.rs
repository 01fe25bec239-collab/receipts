//! Exhaustive deterministic coverage for the bounded pre-review prefix
//! transition slice: the five authorized edges, full 6×6 prefix-matrix
//! rejection, outside-prefix scope rejection, and repeat-evaluation
//! determinism over the complete `GraphNodeState::ALL` cross product.
//!
//! Nothing here asserts global lifecycle legality for non-prefix states:
//! those pairs are only ever reported as outside this capsule's authority.

use crate::node_state::GraphNodeState;
use crate::node_state_transition::{
    AUTHORIZED_PREFIX_TRANSITIONS, GraphNodeStateTransitionError, PREFIX_STATES, is_prefix_state,
    validate_prefix_transition,
};

/// The nine states this capsule deliberately does not speak about.
const OUTSIDE_PREFIX: [GraphNodeState; 9] = [
    GraphNodeState::Passed,
    GraphNodeState::Rejected,
    GraphNodeState::Repairing,
    GraphNodeState::Accepted,
    GraphNodeState::Integrated,
    GraphNodeState::Blocked,
    GraphNodeState::LockedRequiresPro,
    GraphNodeState::Cancelled,
    GraphNodeState::HumanRequired,
];

#[test]
fn prefix_is_exactly_six_states_and_partition_of_vocabulary() {
    assert_eq!(PREFIX_STATES.len(), 6);

    // Every prefix state reports itself as a prefix state...
    for state in PREFIX_STATES {
        assert!(is_prefix_state(state));
    }
    // ...every outside state denies it...
    for state in OUTSIDE_PREFIX {
        assert!(!is_prefix_state(state));
    }
    // ...and together they partition `ALL` exactly once each.
    let total = PREFIX_STATES.len() + OUTSIDE_PREFIX.len();
    assert_eq!(total, GraphNodeState::ALL.len());
}

#[test]
fn exactly_five_authorized_edges_are_declared() {
    assert_eq!(AUTHORIZED_PREFIX_TRANSITIONS.len(), 5);
    for (from, to) in AUTHORIZED_PREFIX_TRANSITIONS {
        assert!(
            validate_prefix_transition(from, to).is_ok(),
            "{:?} → {:?} must be an authorized edge",
            from,
            to,
        );
    }
}

#[test]
fn the_five_named_lifecycle_edges_succeed_individually() {
    let named = [
        ("PLANNED", "READY"),
        ("READY", "ADMITTED"),
        ("ADMITTED", "DISPATCHED"),
        ("DISPATCHED", "RUNNING"),
        ("RUNNING", "AWAITING_REVIEW"),
    ];
    for (from_name, to_name) in named {
        let from = GraphNodeState::parse(from_name).expect("canonical spelling parses");
        let to = GraphNodeState::parse(to_name).expect("canonical spelling parses");
        assert_eq!(
            validate_prefix_transition(from, to),
            Ok(()),
            "{from_name} → {to_name} must succeed",
        );
    }
}

#[test]
fn exhaustive_prefix_matrix_yields_exactly_five_successes() {
    let mut success_count = 0usize;

    for from in PREFIX_STATES {
        for to in PREFIX_STATES {
            match validate_prefix_transition(from, to) {
                Ok(()) => {
                    success_count += 1;
                    assert!(
                        AUTHORIZED_PREFIX_TRANSITIONS.contains(&(from, to)),
                        "success outside the declared edge set: {:?} → {:?}",
                        from,
                        to,
                    );
                }
                Err(error) => {
                    assert_eq!(
                        error,
                        GraphNodeStateTransitionError::UnsupportedPrefixTransition { from, to },
                        "same-prefix non-edge must be UnsupportedPrefixTransition",
                    );
                    assert!(error.is_unsupported_prefix_transition());
                }
            }
        }
    }

    assert_eq!(success_count, 5, "exactly five pairs may succeed");
}

#[test]
fn forward_shortcuts_inside_the_prefix_are_rejected() {
    let shortcuts = [
        (GraphNodeState::Planned, GraphNodeState::Admitted),
        (GraphNodeState::Planned, GraphNodeState::Dispatched),
        (GraphNodeState::Planned, GraphNodeState::Running),
        (GraphNodeState::Ready, GraphNodeState::Running),
        (GraphNodeState::Admitted, GraphNodeState::AwaitingReview),
        (GraphNodeState::Dispatched, GraphNodeState::AwaitingReview),
    ];
    for (from, to) in shortcuts {
        let error = validate_prefix_transition(from, to)
            .expect_err("a forward shortcut is never an authorized edge");
        assert_eq!(error.from(), from);
        assert_eq!(error.to(), to);
        assert!(error.is_unsupported_prefix_transition());
    }
}

#[test]
fn reverse_prefix_transitions_are_rejected() {
    let reversals = [
        (GraphNodeState::Ready, GraphNodeState::Planned),
        (GraphNodeState::AwaitingReview, GraphNodeState::Running),
        (GraphNodeState::Running, GraphNodeState::Dispatched),
        (GraphNodeState::Dispatched, GraphNodeState::Admitted),
        (GraphNodeState::Admitted, GraphNodeState::Ready),
    ];
    for (from, to) in reversals {
        let error = validate_prefix_transition(from, to)
            .expect_err("no backward edge exists inside the prefix");
        assert_eq!(error.from(), from);
        assert_eq!(error.to(), to);
        assert!(error.is_unsupported_prefix_transition());
    }
}

#[test]
fn every_prefix_self_transition_is_rejected() {
    for state in PREFIX_STATES {
        let error = validate_prefix_transition(state, state)
            .expect_err("this validator commands transitions, it is not idempotent assignment");
        assert_eq!(error.from(), state);
        assert_eq!(error.to(), state);
        assert!(error.is_unsupported_prefix_transition());
    }
}

#[test]
fn any_pair_with_an_outside_prefix_source_is_outside_scope() {
    for from in OUTSIDE_PREFIX {
        for to in GraphNodeState::ALL {
            let error = validate_prefix_transition(from, to)
                .expect_err("an outside source can never be validated here");
            assert_eq!(
                error,
                GraphNodeStateTransitionError::OutsidePrefixScope { from, to },
            );
            assert_eq!(error.from(), from);
            assert_eq!(error.to(), to);
            assert!(error.is_outside_prefix_scope());
        }
    }
}

#[test]
fn any_pair_with_an_outside_prefix_target_is_outside_scope() {
    for from in GraphNodeState::ALL {
        for to in OUTSIDE_PREFIX {
            let error = validate_prefix_transition(from, to)
                .expect_err("an outside target can never be validated here");
            assert_eq!(
                error,
                GraphNodeStateTransitionError::OutsidePrefixScope { from, to },
            );
            assert_eq!(error.from(), from);
            assert_eq!(error.to(), to);
            assert!(error.is_outside_prefix_scope());
        }
    }
}

#[test]
fn pairs_where_both_states_are_outside_are_outside_scope() {
    for from in OUTSIDE_PREFIX {
        for to in OUTSIDE_PREFIX {
            // Includes self-pairs such as PASSED → PASSED: still no global claim.
            let error = validate_prefix_transition(from, to)
                .expect_err("both-outside pairs are beyond capsule authority");
            assert!(error.is_outside_prefix_scope());
        }
    }
}

#[test]
fn repeated_evaluation_over_the_full_all_by_all_matrix_is_deterministic() {
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            let first = validate_prefix_transition(from, to);
            let second = validate_prefix_transition(from, to);
            assert_eq!(
                first, second,
                "{:?} → {:?} must be stable across calls",
                from, to
            );
        }
    }

    // Spot-check the canonical outcomes once more after the sweep.
    assert_eq!(
        validate_prefix_transition(GraphNodeState::Planned, GraphNodeState::Ready),
        Ok(())
    );
    assert!(
        validate_prefix_transition(GraphNodeState::Passed, GraphNodeState::Integrated)
            .unwrap_err()
            .is_outside_prefix_scope()
    );
}
