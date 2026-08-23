//! Exhaustive deterministic coverage for the bounded rejected-repair entry
//! slice: the single authorized edge, full 15×15 matrix classification
//! (1 success / 14 unsupported targets / 210 outside-repair-entry-scope
//! pairs), exact error payloads, repeat-evaluation determinism, and
//! coexistence with the untouched pre-review prefix and review-verdict-fork
//! validators.
//!
//! Nothing here asserts global lifecycle legality for any rejected pair:
//! unsupported targets are only ever reported as outside this capsule's
//! one-edge authority, foreign sources as beyond its scope entirely, and no
//! transition out of `REPAIRING` is given any semantics.

use crate::node_state::GraphNodeState;
use crate::node_state_transition::{GraphNodeStateTransitionError, validate_prefix_transition};
use crate::rejected_repair_transition::{
    AUTHORIZED_REJECTED_REPAIR_TRANSITIONS, RejectedRepairTransitionError,
    validate_rejected_repair_transition,
};
use crate::review_verdict_transition::{
    ReviewVerdictTransitionError, validate_review_verdict_transition,
};

/// The fourteen source states this capsule does not speak about: every
/// `GraphNodeState` other than `REJECTED`.
const OUTSIDE_REPAIR_ENTRY_SOURCES: [GraphNodeState; 14] = [
    GraphNodeState::Planned,
    GraphNodeState::Ready,
    GraphNodeState::Admitted,
    GraphNodeState::Dispatched,
    GraphNodeState::Running,
    GraphNodeState::AwaitingReview,
    GraphNodeState::Passed,
    GraphNodeState::Repairing,
    GraphNodeState::Accepted,
    GraphNodeState::Integrated,
    GraphNodeState::Blocked,
    GraphNodeState::LockedRequiresPro,
    GraphNodeState::Cancelled,
    GraphNodeState::HumanRequired,
];

#[test]
fn exactly_one_authorized_edge_is_declared() {
    assert_eq!(AUTHORIZED_REJECTED_REPAIR_TRANSITIONS.len(), 1);
    for (from, to) in AUTHORIZED_REJECTED_REPAIR_TRANSITIONS {
        assert_eq!(from, GraphNodeState::Rejected);
        assert_eq!(
            to,
            GraphNodeState::Repairing,
            "only REPAIRING may be declared the repair-entry target",
        );
        assert!(
            validate_rejected_repair_transition(from, to).is_ok(),
            "{:?} → {:?} must be the authorized edge",
            from,
            to,
        );
    }
}

#[test]
fn rejected_to_repairing_succeeds() {
    assert_eq!(
        validate_rejected_repair_transition(GraphNodeState::Rejected, GraphNodeState::Repairing),
        Ok(()),
        "REJECTED → REPAIRING is the authorized repair-entry edge",
    );
}

#[test]
fn exhaustive_matrix_classifies_exactly_1_14_210() {
    let mut success_count = 0usize;
    let mut unsupported_count = 0usize;
    let mut outside_scope_count = 0usize;

    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            match validate_rejected_repair_transition(from, to) {
                Ok(()) => {
                    success_count += 1;
                    assert!(
                        AUTHORIZED_REJECTED_REPAIR_TRANSITIONS.contains(&(from, to)),
                        "success outside the declared edge set: {:?} → {:?}",
                        from,
                        to,
                    );
                }
                Err(error) if error.is_unsupported_rejected_repair_target() => {
                    unsupported_count += 1;
                    assert_eq!(
                        error,
                        RejectedRepairTransitionError::UnsupportedRejectedRepairTarget { from, to },
                    );
                }
                Err(error) => {
                    assert!(error.is_outside_rejected_repair_entry_scope());
                    outside_scope_count += 1;
                    assert_eq!(
                        error,
                        RejectedRepairTransitionError::OutsideRejectedRepairEntryScope { from, to },
                    );
                }
            }
        }
    }

    let total = success_count + unsupported_count + outside_scope_count;
    assert_eq!(total, 15 * 15, "the matrix must be total over all pairs");
    assert_eq!(
        success_count, 1,
        "exactly the repair-entry edge may succeed"
    );
    assert_eq!(
        unsupported_count, 14,
        "REJECTED × (15 targets − REPAIRING) = 14",
    );
    assert_eq!(
        outside_scope_count, 210,
        "14 non-REJECTED sources × 15 targets = 210",
    );
}

#[test]
fn every_non_rejected_source_is_outside_scope_for_every_target() {
    for from in OUTSIDE_REPAIR_ENTRY_SOURCES {
        assert_ne!(from, GraphNodeState::Rejected);
        for to in GraphNodeState::ALL {
            let error = validate_rejected_repair_transition(from, to)
                .expect_err("a non-REJECTED source can never be validated here");
            assert_eq!(
                error,
                RejectedRepairTransitionError::OutsideRejectedRepairEntryScope { from, to },
            );
            assert_eq!(error.from(), from);
            assert_eq!(error.to(), to);
            assert!(error.is_outside_rejected_repair_entry_scope());
        }
    }
}

#[test]
fn every_non_repairing_target_from_rejected_is_unsupported() {
    for to in GraphNodeState::ALL {
        if matches!(to, GraphNodeState::Repairing) {
            continue;
        }
        let error = validate_rejected_repair_transition(GraphNodeState::Rejected, to)
            .expect_err("only REPAIRING is the authorized repair-entry target");
        assert_eq!(
            error,
            RejectedRepairTransitionError::UnsupportedRejectedRepairTarget {
                from: GraphNodeState::Rejected,
                to,
            },
        );
        assert_eq!(error.from(), GraphNodeState::Rejected);
        assert_eq!(error.to(), to);
        assert!(error.is_unsupported_rejected_repair_target());
    }
}

#[test]
fn rejected_self_transition_is_an_unsupported_target() {
    let error =
        validate_rejected_repair_transition(GraphNodeState::Rejected, GraphNodeState::Rejected)
            .expect_err("re-commanding REJECTED is not a repair-entry edge");
    assert_eq!(
        error,
        RejectedRepairTransitionError::UnsupportedRejectedRepairTarget {
            from: GraphNodeState::Rejected,
            to: GraphNodeState::Rejected,
        },
    );
    assert!(error.is_unsupported_rejected_repair_target());
    assert!(!error.is_outside_rejected_repair_entry_scope());
}

#[test]
fn named_unsupported_targets_do_not_succeed() {
    let named = [
        ("REJECTED", "REJECTED"),
        ("REJECTED", "PASSED"),
        ("REJECTED", "ACCEPTED"),
        ("REJECTED", "INTEGRATED"),
        ("REJECTED", "BLOCKED"),
        ("REJECTED", "CANCELLED"),
        ("REJECTED", "HUMAN_REQUIRED"),
        ("REJECTED", "LOCKED_REQUIRES_PRO"),
        ("REJECTED", "AWAITING_REVIEW"),
        ("REJECTED", "PLANNED"),
        ("REJECTED", "READY"),
        ("REJECTED", "ADMITTED"),
        ("REJECTED", "DISPATCHED"),
        ("REJECTED", "RUNNING"),
    ];
    assert_eq!(named.len(), 14, "all fourteen targets must be covered");
    for (from_name, to_name) in named {
        let from = GraphNodeState::parse(from_name).expect("canonical spelling parses");
        let to = GraphNodeState::parse(to_name).expect("canonical spelling parses");
        let error = validate_rejected_repair_transition(from, to)
            .expect_err("{to_name} is not a repair-entry target");
        assert_eq!(error.from(), from);
        assert_eq!(error.to(), to);
        assert!(error.is_unsupported_rejected_repair_target());
    }
}

#[test]
fn named_foreign_source_pairs_are_outside_scope() {
    // Post-review and unrelated transitions belong to other slices; this
    // validator reports them as out of scope without judging them. In
    // particular `REPAIRING → …` carries no outgoing semantics here at all.
    let named = [
        ("PASSED", "ACCEPTED"),
        ("REPAIRING", "AWAITING_REVIEW"),
        ("REPAIRING", "ACCEPTED"),
        ("ACCEPTED", "INTEGRATED"),
        ("RUNNING", "REPAIRING"),
        ("BLOCKED", "REPAIRING"),
    ];
    for (from_name, to_name) in named {
        let from = GraphNodeState::parse(from_name).expect("canonical spelling parses");
        let to = GraphNodeState::parse(to_name).expect("canonical spelling parses");
        let error = validate_rejected_repair_transition(from, to)
            .expect_err("a non-REJECTED source is beyond repair-entry scope");
        assert_eq!(
            error,
            RejectedRepairTransitionError::OutsideRejectedRepairEntryScope { from, to },
        );
        assert_eq!(error.from(), from);
        assert_eq!(error.to(), to);
        assert!(error.is_outside_rejected_repair_entry_scope());
        assert!(!error.is_unsupported_rejected_repair_target());
    }
}

#[test]
fn repairing_has_no_outgoing_semantics_in_this_capsule() {
    // Every REPAIRING-sourced command — including into states that later
    // slices may authorize — must be reported as outside scope, never as a
    // success and never as an unsupported-target classification.
    for to in GraphNodeState::ALL {
        let error = validate_rejected_repair_transition(GraphNodeState::Repairing, to)
            .expect_err("no REPAIRING successor semantics exist in this slice");
        assert_eq!(
            error,
            RejectedRepairTransitionError::OutsideRejectedRepairEntryScope {
                from: GraphNodeState::Repairing,
                to,
            },
        );
        assert!(error.is_outside_rejected_repair_entry_scope());
    }
}

#[test]
fn the_edge_is_directional_and_payloads_do_not_swap() {
    assert!(
        validate_rejected_repair_transition(GraphNodeState::Repairing, GraphNodeState::Rejected)
            .is_err(),
        "the reverse of the authorized edge must not succeed",
    );
    // If a swapped implementation ever inverted the payload fields, the
    // exact-variant comparisons below (with distinct endpoints) would fail.
    let reversed =
        validate_rejected_repair_transition(GraphNodeState::Repairing, GraphNodeState::Rejected)
            .expect_err("REPAIRING → REJECTED is beyond repair-entry scope");
    assert_eq!(
        reversed,
        RejectedRepairTransitionError::OutsideRejectedRepairEntryScope {
            from: GraphNodeState::Repairing,
            to: GraphNodeState::Rejected,
        },
    );

    let wrong_target =
        validate_rejected_repair_transition(GraphNodeState::Rejected, GraphNodeState::Accepted)
            .expect_err("REJECTED → ACCEPTED is not a repair-entry edge");
    assert_eq!(
        wrong_target,
        RejectedRepairTransitionError::UnsupportedRejectedRepairTarget {
            from: GraphNodeState::Rejected,
            to: GraphNodeState::Accepted,
        },
    );
}

#[test]
fn both_error_classifications_preserve_exact_endpoints() {
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            if let Err(error) = validate_rejected_repair_transition(from, to) {
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
            let first = validate_rejected_repair_transition(from, to);
            let second = validate_rejected_repair_transition(from, to);
            assert_eq!(
                first, second,
                "{:?} → {:?} must be stable across calls",
                from, to
            );
        }
    }

    // Spot-check each outcome class once more after the sweep.
    assert_eq!(
        validate_rejected_repair_transition(GraphNodeState::Rejected, GraphNodeState::Repairing),
        Ok(())
    );
    assert!(
        validate_rejected_repair_transition(GraphNodeState::Rejected, GraphNodeState::Passed)
            .unwrap_err()
            .is_unsupported_rejected_repair_target()
    );
    assert!(
        validate_rejected_repair_transition(GraphNodeState::Running, GraphNodeState::Repairing)
            .unwrap_err()
            .is_outside_rejected_repair_entry_scope()
    );
}

#[test]
fn the_repair_entry_coexists_with_the_untouched_previous_validators() {
    // The same pair classifies differently per validator: the fork slice
    // still owns AWAITING_REVIEW → PASSED/REJECTED and still rejects
    // REJECTED → REPAIRING as an unsupported verdict target; the prefix
    // slice still rejects everything past AWAITING_REVIEW — proof that the
    // repair-entry edge was not merged into either previous validator and
    // vice versa.
    assert_eq!(
        validate_review_verdict_transition(GraphNodeState::Rejected, GraphNodeState::Repairing),
        Err(
            ReviewVerdictTransitionError::OutsideReviewVerdictForkScope {
                from: GraphNodeState::Rejected,
                to: GraphNodeState::Repairing,
            }
        ),
    );
    assert_eq!(
        validate_prefix_transition(GraphNodeState::Rejected, GraphNodeState::Repairing),
        Err(GraphNodeStateTransitionError::OutsidePrefixScope {
            from: GraphNodeState::Rejected,
            to: GraphNodeState::Repairing,
        }),
    );
    assert_eq!(
        validate_review_verdict_transition(
            GraphNodeState::AwaitingReview,
            GraphNodeState::Rejected,
        ),
        Ok(()),
    );
    assert_eq!(
        validate_rejected_repair_transition(
            GraphNodeState::AwaitingReview,
            GraphNodeState::Rejected,
        ),
        Err(
            RejectedRepairTransitionError::OutsideRejectedRepairEntryScope {
                from: GraphNodeState::AwaitingReview,
                to: GraphNodeState::Rejected,
            }
        ),
    );

    // The three capsules are pairwise disjoint: no ordered pair succeeds in
    // more than one validator.
    for from in GraphNodeState::ALL {
        for to in GraphNodeState::ALL {
            let prefix_ok = validate_prefix_transition(from, to).is_ok();
            let fork_ok = validate_review_verdict_transition(from, to).is_ok();
            let repair_ok = validate_rejected_repair_transition(from, to).is_ok();
            assert!(
                !(prefix_ok && repair_ok),
                "{:?} → {:?} succeeded in both the prefix and repair capsules",
                from,
                to,
            );
            assert!(
                !(fork_ok && repair_ok),
                "{:?} → {:?} succeeded in both the fork and repair capsules",
                from,
                to,
            );
        }
    }

    // The success sets are disjoint and stable in aggregate: five prefix
    // edges + two fork edges + one repair-entry edge = exactly eight pairs
    // succeed across all three validators combined.
    let total_successes = GraphNodeState::ALL
        .iter()
        .flat_map(|from| {
            GraphNodeState::ALL.iter().map(move |to| {
                u32::from(validate_prefix_transition(*from, *to).is_ok())
                    + u32::from(validate_review_verdict_transition(*from, *to).is_ok())
                    + u32::from(validate_rejected_repair_transition(*from, *to).is_ok())
            })
        })
        .sum::<u32>();
    assert_eq!(
        total_successes, 8,
        "prefix + fork + repair-entry together authorize exactly eight pairs",
    );
}
