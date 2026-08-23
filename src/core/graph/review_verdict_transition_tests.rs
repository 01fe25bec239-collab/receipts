//! Exhaustive deterministic coverage for the bounded review-verdict fork
//! slice: the two authorized edges, full 15×15 matrix classification
//! (2 successes / 13 unsupported targets / 210 outside-fork-scope pairs),
//! exact error payloads, repeat-evaluation determinism, and coexistence with
//! the untouched pre-review prefix validator.
//!
//! Nothing here asserts global lifecycle legality for any rejected pair:
//! unsupported targets are only ever reported as outside this capsule's
//! two-edge authority, and foreign sources as beyond its scope entirely.

use crate::node_state::GraphNodeState;
use crate::node_state_transition::{GraphNodeStateTransitionError, validate_prefix_transition};
use crate::review_verdict_transition::{
    AUTHORIZED_REVIEW_VERDICT_TRANSITIONS, ReviewVerdictTransitionError,
    validate_review_verdict_transition,
};

/// The fourteen source states this capsule does not speak about: every
/// `GraphNodeState` other than `AWAITING_REVIEW`.
const OUTSIDE_FORK_SOURCES: [GraphNodeState; 14] = [
    GraphNodeState::Planned,
    GraphNodeState::Ready,
    GraphNodeState::Admitted,
    GraphNodeState::Dispatched,
    GraphNodeState::Running,
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
fn exactly_two_authorized_edges_are_declared() {
    assert_eq!(AUTHORIZED_REVIEW_VERDICT_TRANSITIONS.len(), 2);
    for (from, to) in AUTHORIZED_REVIEW_VERDICT_TRANSITIONS {
        assert_eq!(from, GraphNodeState::AwaitingReview);
        assert!(
            matches!(to, GraphNodeState::Passed | GraphNodeState::Rejected),
            "only PASSED and REJECTED may be declared targets, got {:?}",
            to,
        );
        assert!(
            validate_review_verdict_transition(from, to).is_ok(),
            "{:?} → {:?} must be an authorized edge",
            from,
            to,
        );
    }
}

#[test]
fn awaiting_review_to_passed_succeeds() {
    assert_eq!(
        validate_review_verdict_transition(GraphNodeState::AwaitingReview, GraphNodeState::Passed),
        Ok(()),
        "AWAITING_REVIEW → PASSED is an authorized review-verdict edge",
    );
}

#[test]
fn awaiting_review_to_rejected_succeeds() {
    assert_eq!(
        validate_review_verdict_transition(
            GraphNodeState::AwaitingReview,
            GraphNodeState::Rejected,
        ),
        Ok(()),
        "AWAITING_REVIEW → REJECTED is an authorized review-verdict edge",
    );
}

#[test]
fn exhaustive_matrix_classifies_exactly_2_13_210() {
    let mut success_count = 0usize;
    let mut unsupported_count = 0usize;
    let mut outside_scope_count = 0usize;

    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            match validate_review_verdict_transition(from, to) {
                Ok(()) => {
                    success_count += 1;
                    assert!(
                        AUTHORIZED_REVIEW_VERDICT_TRANSITIONS.contains(&(from, to)),
                        "success outside the declared edge set: {:?} → {:?}",
                        from,
                        to,
                    );
                }
                Err(error) if error.is_unsupported_review_verdict_target() => {
                    unsupported_count += 1;
                    assert_eq!(
                        error,
                        ReviewVerdictTransitionError::UnsupportedReviewVerdictTarget { from, to },
                    );
                }
                Err(error) => {
                    assert!(error.is_outside_review_verdict_fork_scope());
                    outside_scope_count += 1;
                    assert_eq!(
                        error,
                        ReviewVerdictTransitionError::OutsideReviewVerdictForkScope { from, to },
                    );
                }
            }
        }
    }

    let total = success_count + unsupported_count + outside_scope_count;
    assert_eq!(total, 15 * 15, "the matrix must be total over all pairs");
    assert_eq!(success_count, 2, "exactly the two fork edges may succeed");
    assert_eq!(
        unsupported_count, 13,
        "AWAITING_REVIEW × (15 targets − 2 verdicts) = 13",
    );
    assert_eq!(
        outside_scope_count, 210,
        "14 non-AWAITING_REVIEW sources × 15 targets = 210",
    );
}

#[test]
fn every_non_fork_source_is_outside_scope_for_every_target() {
    for from in OUTSIDE_FORK_SOURCES {
        assert_ne!(from, GraphNodeState::AwaitingReview);
        for to in GraphNodeState::ALL {
            let error = validate_review_verdict_transition(from, to)
                .expect_err("a non-AWAITING_REVIEW source can never be validated here");
            assert_eq!(
                error,
                ReviewVerdictTransitionError::OutsideReviewVerdictForkScope { from, to },
            );
            assert_eq!(error.from(), from);
            assert_eq!(error.to(), to);
            assert!(error.is_outside_review_verdict_fork_scope());
        }
    }
}

#[test]
fn every_non_verdict_target_from_awaiting_review_is_unsupported() {
    for to in GraphNodeState::ALL {
        if matches!(to, GraphNodeState::Passed | GraphNodeState::Rejected) {
            continue;
        }
        let error = validate_review_verdict_transition(GraphNodeState::AwaitingReview, to)
            .expect_err("only PASSED and REJECTED are authorized targets");
        assert_eq!(
            error,
            ReviewVerdictTransitionError::UnsupportedReviewVerdictTarget {
                from: GraphNodeState::AwaitingReview,
                to,
            },
        );
        assert_eq!(error.from(), GraphNodeState::AwaitingReview);
        assert_eq!(error.to(), to);
        assert!(error.is_unsupported_review_verdict_target());
    }
}

#[test]
fn awaiting_review_self_transition_is_an_unsupported_target() {
    let error = validate_review_verdict_transition(
        GraphNodeState::AwaitingReview,
        GraphNodeState::AwaitingReview,
    )
    .expect_err("re-commanding AWAITING_REVIEW is not a verdict fork edge");
    assert_eq!(
        error,
        ReviewVerdictTransitionError::UnsupportedReviewVerdictTarget {
            from: GraphNodeState::AwaitingReview,
            to: GraphNodeState::AwaitingReview,
        },
    );
    assert!(error.is_unsupported_review_verdict_target());
    assert!(!error.is_outside_review_verdict_fork_scope());
}

#[test]
fn named_unsupported_targets_do_not_succeed() {
    let named = [
        ("AWAITING_REVIEW", "AWAITING_REVIEW"),
        ("AWAITING_REVIEW", "ACCEPTED"),
        ("AWAITING_REVIEW", "REPAIRING"),
        ("AWAITING_REVIEW", "INTEGRATED"),
        ("AWAITING_REVIEW", "BLOCKED"),
        ("AWAITING_REVIEW", "CANCELLED"),
        ("AWAITING_REVIEW", "HUMAN_REQUIRED"),
        ("AWAITING_REVIEW", "LOCKED_REQUIRES_PRO"),
        ("AWAITING_REVIEW", "PLANNED"),
        ("AWAITING_REVIEW", "READY"),
        ("AWAITING_REVIEW", "ADMITTED"),
        ("AWAITING_REVIEW", "DISPATCHED"),
        ("AWAITING_REVIEW", "RUNNING"),
    ];
    assert_eq!(named.len(), 13, "all thirteen targets must be covered");
    for (from_name, to_name) in named {
        let from = GraphNodeState::parse(from_name).expect("canonical spelling parses");
        let to = GraphNodeState::parse(to_name).expect("canonical spelling parses");
        let error = validate_review_verdict_transition(from, to)
            .expect_err("{to_name} is not a verdict target");
        assert_eq!(error.from(), from);
        assert_eq!(error.to(), to);
        assert!(error.is_unsupported_review_verdict_target());
    }
}

#[test]
fn accepted_and_repairing_are_not_verdict_targets() {
    for to in [GraphNodeState::Accepted, GraphNodeState::Repairing] {
        let result = validate_review_verdict_transition(GraphNodeState::AwaitingReview, to);
        assert!(
            result.is_err(),
            "AWAITING_REVIEW → {:?} must never succeed in this capsule",
            to,
        );
        assert!(result.unwrap_err().is_unsupported_review_verdict_target());
    }
}

#[test]
fn named_foreign_source_pairs_are_outside_scope() {
    // Post-fork and unrelated transitions belong to other slices; this
    // validator reports them as out of scope without judging them.
    let named = [
        ("RUNNING", "PASSED"),
        ("PASSED", "ACCEPTED"),
        ("REJECTED", "REPAIRING"),
        ("ACCEPTED", "INTEGRATED"),
        ("BLOCKED", "READY"),
    ];
    for (from_name, to_name) in named {
        let from = GraphNodeState::parse(from_name).expect("canonical spelling parses");
        let to = GraphNodeState::parse(to_name).expect("canonical spelling parses");
        let error = validate_review_verdict_transition(from, to)
            .expect_err("a non-AWAITING_REVIEW source is beyond fork scope");
        assert_eq!(
            error,
            ReviewVerdictTransitionError::OutsideReviewVerdictForkScope { from, to },
        );
        assert_eq!(error.from(), from);
        assert_eq!(error.to(), to);
        assert!(error.is_outside_review_verdict_fork_scope());
        assert!(!error.is_unsupported_review_verdict_target());
    }
}

#[test]
fn both_error_classifications_preserve_exact_endpoints() {
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            if let Err(error) = validate_review_verdict_transition(from, to) {
                assert_eq!(error.from(), from, "source must round-trip verbatim");
                assert_eq!(error.to(), to, "target must round-trip verbatim");
            }
        }
    }
}

#[test]
fn repeated_evaluation_over_the_full_all_by_all_matrix_is_deterministic() {
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            let first = validate_review_verdict_transition(from, to);
            let second = validate_review_verdict_transition(from, to);
            assert_eq!(
                first, second,
                "{:?} → {:?} must be stable across calls",
                from, to
            );
        }
    }

    // Spot-check each outcome class once more after the sweep.
    assert_eq!(
        validate_review_verdict_transition(GraphNodeState::AwaitingReview, GraphNodeState::Passed),
        Ok(())
    );
    assert_eq!(
        validate_review_verdict_transition(
            GraphNodeState::AwaitingReview,
            GraphNodeState::Rejected,
        ),
        Ok(())
    );
    assert!(
        validate_review_verdict_transition(
            GraphNodeState::AwaitingReview,
            GraphNodeState::Accepted
        )
        .unwrap_err()
        .is_unsupported_review_verdict_target()
    );
    assert!(
        validate_review_verdict_transition(GraphNodeState::Running, GraphNodeState::Passed)
            .unwrap_err()
            .is_outside_review_verdict_fork_scope()
    );
}

#[test]
fn the_fork_coexists_with_the_untouched_prefix_validator() {
    // The same pair classifies differently per validator: the prefix slice
    // still owns PLANNED → … → AWAITING_REVIEW and still rejects everything
    // past AWAITING_REVIEW as outside its own scope — proof that the fork was
    // not merged into the prefix logic and vice versa.
    assert_eq!(
        validate_prefix_transition(GraphNodeState::AwaitingReview, GraphNodeState::Passed),
        Err(GraphNodeStateTransitionError::OutsidePrefixScope {
            from: GraphNodeState::AwaitingReview,
            to: GraphNodeState::Passed,
        }),
    );
    assert_eq!(
        validate_review_verdict_transition(GraphNodeState::Planned, GraphNodeState::Ready),
        Err(
            ReviewVerdictTransitionError::OutsideReviewVerdictForkScope {
                from: GraphNodeState::Planned,
                to: GraphNodeState::Ready,
            }
        ),
    );
    assert_eq!(
        validate_prefix_transition(GraphNodeState::Running, GraphNodeState::AwaitingReview),
        Ok(())
    );
    assert!(
        validate_review_verdict_transition(GraphNodeState::Running, GraphNodeState::AwaitingReview)
            .unwrap_err()
            .is_outside_review_verdict_fork_scope()
    );

    // The only state both validators accept commands *from* into their own
    // edges is disjoint: no pair succeeds in both.
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            let prefix_ok = validate_prefix_transition(from, to).is_ok();
            let fork_ok = validate_review_verdict_transition(from, to).is_ok();
            assert!(
                !(prefix_ok && fork_ok),
                "{:?} → {:?} succeeded in both capsules",
                from,
                to,
            );
        }
    }
}
