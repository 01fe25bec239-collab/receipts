//! Exhaustive deterministic coverage for the bounded repair-completion review
//! re-entry slice: the single authorized edge, full 15×15 matrix
//! classification (1 success / 14 unsupported targets / 210
//! outside-repair-completion-scope pairs), exact error payloads,
//! directionality, repeat-evaluation determinism, and coexistence with the
//! untouched prefix, review-verdict-fork, and rejected-repair-entry
//! validators.
//!
//! Nothing here asserts global lifecycle legality for any pair: unsupported
//! targets are only ever reported as outside this capsule's one-edge
//! authority, and foreign sources as beyond its scope entirely.

use crate::node_state::GraphNodeState;
use crate::node_state_transition::{
    AUTHORIZED_PREFIX_TRANSITIONS, GraphNodeStateTransitionError, validate_prefix_transition,
};
use crate::rejected_repair_transition::{
    AUTHORIZED_REJECTED_REPAIR_TRANSITIONS, RejectedRepairTransitionError,
    validate_rejected_repair_transition,
};
use crate::repair_completion_transition::{
    AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS, RepairCompletionTransitionError,
    validate_repair_completion_transition,
};
use crate::review_verdict_transition::{
    AUTHORIZED_REVIEW_VERDICT_TRANSITIONS, ReviewVerdictTransitionError,
    validate_review_verdict_transition,
};

/// The fourteen source states this capsule does not speak about: every
/// `GraphNodeState` other than `REPAIRING`.
const OUTSIDE_REPAIR_COMPLETION_SOURCES: [GraphNodeState; 14] = [
    GraphNodeState::Planned,
    GraphNodeState::Ready,
    GraphNodeState::Admitted,
    GraphNodeState::Dispatched,
    GraphNodeState::Running,
    GraphNodeState::AwaitingReview,
    GraphNodeState::Passed,
    GraphNodeState::Rejected,
    GraphNodeState::Accepted,
    GraphNodeState::Integrated,
    GraphNodeState::Blocked,
    GraphNodeState::LockedRequiresPro,
    GraphNodeState::Cancelled,
    GraphNodeState::HumanRequired,
];

#[test]
fn exactly_one_authorized_edge_is_declared() {
    assert_eq!(AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS.len(), 1);
    for (from, to) in AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS {
        assert_eq!(from, GraphNodeState::Repairing);
        assert_eq!(
            to,
            GraphNodeState::AwaitingReview,
            "repair completion may only re-enter independent review",
        );
        assert!(
            validate_repair_completion_transition(from, to).is_ok(),
            "{:?} → {:?} must be the authorized edge",
            from,
            to,
        );
    }
}

#[test]
fn repairing_to_awaiting_review_succeeds() {
    assert_eq!(
        validate_repair_completion_transition(
            GraphNodeState::Repairing,
            GraphNodeState::AwaitingReview,
        ),
        Ok(()),
        "REPAIRING → AWAITING_REVIEW is the authorized repair-completion edge",
    );
}

#[test]
fn exhaustive_matrix_classifies_exactly_1_14_210() {
    let mut success_count = 0usize;
    let mut unsupported_count = 0usize;
    let mut outside_scope_count = 0usize;

    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            match validate_repair_completion_transition(from, to) {
                Ok(()) => {
                    success_count += 1;
                    assert!(
                        AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS.contains(&(from, to)),
                        "success outside the declared edge set: {:?} → {:?}",
                        from,
                        to,
                    );
                }
                Err(error) if error.is_unsupported_repair_completion_target() => {
                    unsupported_count += 1;
                    assert_eq!(
                        error,
                        RepairCompletionTransitionError::UnsupportedRepairCompletionTarget {
                            from,
                            to,
                        },
                    );
                }
                Err(error) => {
                    assert!(error.is_outside_repair_completion_scope());
                    outside_scope_count += 1;
                    assert_eq!(
                        error,
                        RepairCompletionTransitionError::OutsideRepairCompletionScope { from, to },
                    );
                }
            }
        }
    }

    let total = success_count + unsupported_count + outside_scope_count;
    assert_eq!(total, 15 * 15, "the matrix must be total over all pairs");
    assert_eq!(
        success_count, 1,
        "exactly the repair-completion edge may succeed",
    );
    assert_eq!(
        unsupported_count, 14,
        "REPAIRING × (15 targets − AWAITING_REVIEW) = 14",
    );
    assert_eq!(
        outside_scope_count, 210,
        "14 non-REPAIRING sources × 15 targets = 210",
    );
}

#[test]
fn every_non_awaiting_review_target_from_repairing_is_unsupported() {
    let mut checked = 0usize;
    for to in GraphNodeState::ALL {
        if matches!(to, GraphNodeState::AwaitingReview) {
            continue;
        }
        checked += 1;
        let error = validate_repair_completion_transition(GraphNodeState::Repairing, to)
            .expect_err("only AWAITING_REVIEW is the authorized repair-completion target");
        assert_eq!(
            error,
            RepairCompletionTransitionError::UnsupportedRepairCompletionTarget {
                from: GraphNodeState::Repairing,
                to,
            },
        );
        assert_eq!(error.from(), GraphNodeState::Repairing);
        assert_eq!(error.to(), to);
        assert!(error.is_unsupported_repair_completion_target());
        assert!(!error.is_outside_repair_completion_scope());
    }
    assert_eq!(checked, 14, "all fourteen unsupported targets are covered");
}

#[test]
fn every_non_repairing_source_is_outside_scope_for_every_target() {
    for from in OUTSIDE_REPAIR_COMPLETION_SOURCES {
        assert_ne!(from, GraphNodeState::Repairing);
        for to in GraphNodeState::ALL {
            let error = validate_repair_completion_transition(from, to)
                .expect_err("a non-REPAIRING source can never be validated here");
            assert_eq!(
                error,
                RepairCompletionTransitionError::OutsideRepairCompletionScope { from, to },
            );
            assert_eq!(error.from(), from);
            assert_eq!(error.to(), to);
            assert!(error.is_outside_repair_completion_scope());
            assert!(!error.is_unsupported_repair_completion_target());
        }
    }
}

#[test]
fn named_unsupported_targets_do_not_succeed() {
    // Every explicitly de-authorized repair completion path, including the
    // review-bypassing ones (PASSED / ACCEPTED / INTEGRATED) and the
    // REPAIRING self-loop.
    let named = [
        "REPAIRING",
        "PASSED",
        "REJECTED",
        "ACCEPTED",
        "INTEGRATED",
        "PLANNED",
        "READY",
        "ADMITTED",
        "DISPATCHED",
        "RUNNING",
        "BLOCKED",
        "LOCKED_REQUIRES_PRO",
        "CANCELLED",
        "HUMAN_REQUIRED",
    ];
    assert_eq!(named.len(), 14, "all fourteen targets must be covered");
    for to_name in named {
        let to = GraphNodeState::parse(to_name).expect("canonical spelling parses");
        assert_ne!(to, GraphNodeState::AwaitingReview);
        let error = validate_repair_completion_transition(GraphNodeState::Repairing, to)
            .expect_err("this target is not a repair-completion edge");
        assert_eq!(
            error,
            RepairCompletionTransitionError::UnsupportedRepairCompletionTarget {
                from: GraphNodeState::Repairing,
                to,
            },
        );
        assert_eq!(error.from(), GraphNodeState::Repairing);
        assert_eq!(error.to(), to);
        assert!(error.is_unsupported_repair_completion_target());
    }
}

#[test]
fn named_foreign_source_pairs_are_outside_scope() {
    // Transitions owned by other bounded validators or by future slices are
    // reported as out of scope here, never judged.
    let named = [
        ("REJECTED", "REPAIRING"),
        ("AWAITING_REVIEW", "PASSED"),
        ("AWAITING_REVIEW", "REJECTED"),
        ("PASSED", "ACCEPTED"),
        ("ACCEPTED", "INTEGRATED"),
        ("RUNNING", "AWAITING_REVIEW"),
        ("BLOCKED", "AWAITING_REVIEW"),
    ];
    for (from_name, to_name) in named {
        let from = GraphNodeState::parse(from_name).expect("canonical spelling parses");
        let to = GraphNodeState::parse(to_name).expect("canonical spelling parses");
        let error = validate_repair_completion_transition(from, to)
            .expect_err("a non-REPAIRING source is beyond repair-completion scope");
        assert_eq!(
            error,
            RepairCompletionTransitionError::OutsideRepairCompletionScope { from, to },
        );
        assert_eq!(error.from(), from);
        assert_eq!(error.to(), to);
        assert!(error.is_outside_repair_completion_scope());
    }
}

#[test]
fn the_edge_is_directional_and_payloads_do_not_swap() {
    let reversed = validate_repair_completion_transition(
        GraphNodeState::AwaitingReview,
        GraphNodeState::Repairing,
    )
    .expect_err("the reverse of the authorized edge must not succeed");
    assert_eq!(
        reversed,
        RepairCompletionTransitionError::OutsideRepairCompletionScope {
            from: GraphNodeState::AwaitingReview,
            to: GraphNodeState::Repairing,
        },
    );
    assert_eq!(reversed.from(), GraphNodeState::AwaitingReview);
    assert_eq!(reversed.to(), GraphNodeState::Repairing);

    let wrong_target =
        validate_repair_completion_transition(GraphNodeState::Repairing, GraphNodeState::Passed)
            .expect_err("REPAIRING → PASSED must never bypass fresh review");
    assert_eq!(
        wrong_target,
        RepairCompletionTransitionError::UnsupportedRepairCompletionTarget {
            from: GraphNodeState::Repairing,
            to: GraphNodeState::Passed,
        },
    );
    assert_eq!(wrong_target.from(), GraphNodeState::Repairing);
    assert_eq!(wrong_target.to(), GraphNodeState::Passed);
}

#[test]
fn both_error_classifications_preserve_exact_endpoints() {
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            if let Err(error) = validate_repair_completion_transition(from, to) {
                assert_eq!(error.from(), from, "source must round-trip verbatim");
                assert_eq!(error.to(), to, "target must round-trip verbatim");
                assert!(
                    error.is_unsupported_repair_completion_target()
                        ^ error.is_outside_repair_completion_scope(),
                    "exactly one classification must hold",
                );
            }
        }
    }
}

#[test]
fn repeated_evaluation_over_the_full_all_by_all_matrix_is_deterministic() {
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            let first = validate_repair_completion_transition(from, to);
            for _ in 0..4 {
                assert_eq!(
                    validate_repair_completion_transition(from, to),
                    first,
                    "{:?} → {:?} must be stable across calls",
                    from,
                    to,
                );
            }
        }
    }

    // Spot-check each outcome class once more after the sweep.
    assert_eq!(
        validate_repair_completion_transition(
            GraphNodeState::Repairing,
            GraphNodeState::AwaitingReview,
        ),
        Ok(()),
    );
    assert!(
        validate_repair_completion_transition(
            GraphNodeState::Repairing,
            GraphNodeState::Integrated,
        )
        .unwrap_err()
        .is_unsupported_repair_completion_target()
    );
    assert!(
        validate_repair_completion_transition(GraphNodeState::Running, GraphNodeState::Repairing)
            .unwrap_err()
            .is_outside_repair_completion_scope()
    );
}

#[test]
fn previous_rejected_repair_entry_behavior_is_unchanged() {
    assert_eq!(AUTHORIZED_REJECTED_REPAIR_TRANSITIONS.len(), 1);
    assert_eq!(
        AUTHORIZED_REJECTED_REPAIR_TRANSITIONS[0],
        (GraphNodeState::Rejected, GraphNodeState::Repairing),
    );
    assert_eq!(
        validate_rejected_repair_transition(GraphNodeState::Rejected, GraphNodeState::Repairing),
        Ok(()),
        "B1B4 REJECTED → REPAIRING must remain accepted",
    );
    // The new capsule does not absorb the repair-entry edge.
    assert_eq!(
        validate_repair_completion_transition(GraphNodeState::Rejected, GraphNodeState::Repairing),
        Err(
            RepairCompletionTransitionError::OutsideRepairCompletionScope {
                from: GraphNodeState::Rejected,
                to: GraphNodeState::Repairing,
            }
        ),
    );
    // …and the repair-entry capsule still declines the new edge.
    assert_eq!(
        validate_rejected_repair_transition(
            GraphNodeState::Repairing,
            GraphNodeState::AwaitingReview,
        ),
        Err(
            RejectedRepairTransitionError::OutsideRejectedRepairEntryScope {
                from: GraphNodeState::Repairing,
                to: GraphNodeState::AwaitingReview,
            }
        ),
    );
}

#[test]
fn previous_review_verdict_fork_behavior_is_unchanged() {
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
    // The fork capsule still owns no REPAIRING source.
    assert_eq!(
        validate_review_verdict_transition(
            GraphNodeState::Repairing,
            GraphNodeState::AwaitingReview,
        ),
        Err(
            ReviewVerdictTransitionError::OutsideReviewVerdictForkScope {
                from: GraphNodeState::Repairing,
                to: GraphNodeState::AwaitingReview,
            }
        ),
    );
}

#[test]
fn previous_prefix_behavior_is_unchanged() {
    const EXPECTED_PREFIX: [(GraphNodeState, GraphNodeState); 5] = [
        (GraphNodeState::Planned, GraphNodeState::Ready),
        (GraphNodeState::Ready, GraphNodeState::Admitted),
        (GraphNodeState::Admitted, GraphNodeState::Dispatched),
        (GraphNodeState::Dispatched, GraphNodeState::Running),
        (GraphNodeState::Running, GraphNodeState::AwaitingReview),
    ];
    assert_eq!(AUTHORIZED_PREFIX_TRANSITIONS.len(), 5);
    for edge in EXPECTED_PREFIX {
        assert!(
            AUTHORIZED_PREFIX_TRANSITIONS.contains(&edge),
            "{edge:?} must remain an authorized prefix edge",
        );
        assert_eq!(validate_prefix_transition(edge.0, edge.1), Ok(()));
    }
    assert_eq!(
        validate_prefix_transition(GraphNodeState::Repairing, GraphNodeState::AwaitingReview),
        Err(GraphNodeStateTransitionError::OutsidePrefixScope {
            from: GraphNodeState::Repairing,
            to: GraphNodeState::AwaitingReview,
        }),
    );
}

#[test]
fn the_four_validator_success_sets_are_pairwise_disjoint() {
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            let oks = [
                validate_prefix_transition(from, to).is_ok(),
                validate_review_verdict_transition(from, to).is_ok(),
                validate_rejected_repair_transition(from, to).is_ok(),
                validate_repair_completion_transition(from, to).is_ok(),
            ];
            assert!(
                oks.iter().filter(|ok| **ok).count() <= 1,
                "{:?} → {:?} succeeded in more than one bounded validator",
                from,
                to,
            );
        }
    }
}

#[test]
fn aggregate_authorized_edge_count_is_exactly_nine() {
    let total_successes = GraphNodeState::ALL
        .iter()
        .flat_map(|from| {
            GraphNodeState::ALL.iter().map(move |to| {
                u32::from(validate_prefix_transition(*from, *to).is_ok())
                    + u32::from(validate_review_verdict_transition(*from, *to).is_ok())
                    + u32::from(validate_rejected_repair_transition(*from, *to).is_ok())
                    + u32::from(validate_repair_completion_transition(*from, *to).is_ok())
            })
        })
        .sum::<u32>();
    assert_eq!(
        total_successes, 9,
        "5 prefix + 2 fork + 1 repair-entry + 1 repair-completion = 9",
    );
}

#[test]
fn state_vocabulary_is_unchanged_and_failed_is_not_a_state() {
    assert_eq!(GraphNodeState::ALL.len(), 15);
    assert!(
        GraphNodeState::parse("FAILED").is_err(),
        "FAILED is not a member of the frozen GraphNodeState vocabulary",
    );
    assert!(GraphNodeState::parse("REPAIRING").is_ok());
    assert!(GraphNodeState::parse("AWAITING_REVIEW").is_ok());
}
