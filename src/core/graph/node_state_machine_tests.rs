//! Exhaustive 15 × 15 oracle table and compatibility with the six bounded
//! validators.

use crate::node_state::GraphNodeState as S;
use crate::node_state_machine::{
    NodeTransitionError, NodeTransitionRule, TransitionCauseKind as C, TransitionRefusal as R,
    check_node_transition, node_transition_rule,
};
use crate::{
    validate_accepted_integration_transition, validate_passed_acceptance_transition,
    validate_prefix_transition, validate_rejected_repair_transition,
    validate_repair_completion_transition, validate_review_verdict_transition,
};

/// The expected table, written independently of the implementation: one
/// row per source state listing every allowed target and its cause; every
/// other target's refusal reason follows the stated rule.
fn expected(from: S, to: S) -> NodeTransitionRule {
    use NodeTransitionRule::{Allowed, Refused};
    let allowed: &[(S, C)] = match from {
        S::Planned => &[
            (S::Ready, C::DependenciesSatisfied),
            (S::Blocked, C::Blocked),
            (S::HumanRequired, C::HumanRequired),
            (S::Cancelled, C::Cancellation),
        ],
        S::Ready => &[
            (S::Admitted, C::Admission),
            (S::LockedRequiresPro, C::FeatureLocked),
            (S::Blocked, C::Blocked),
            (S::HumanRequired, C::HumanRequired),
            (S::Cancelled, C::Cancellation),
        ],
        S::Admitted => &[
            (S::Dispatched, C::Dispatch),
            (S::Blocked, C::Blocked),
            (S::HumanRequired, C::HumanRequired),
            (S::Cancelled, C::Cancellation),
        ],
        S::Dispatched => &[
            (S::Running, C::Start),
            (S::Blocked, C::Blocked),
            (S::HumanRequired, C::HumanRequired),
            (S::Cancelled, C::Cancellation),
        ],
        S::Running => &[
            (S::AwaitingReview, C::Submit),
            (S::Blocked, C::Blocked),
            (S::HumanRequired, C::HumanRequired),
            (S::Cancelled, C::Cancellation),
        ],
        S::AwaitingReview => &[
            (S::Passed, C::ReviewVerdict),
            (S::Rejected, C::ReviewVerdict),
            (S::Blocked, C::Blocked),
            (S::HumanRequired, C::HumanRequired),
            (S::Cancelled, C::Cancellation),
        ],
        S::Passed => &[
            (S::Accepted, C::Acceptance),
            (S::Blocked, C::Blocked),
            (S::HumanRequired, C::HumanRequired),
            (S::Cancelled, C::Cancellation),
        ],
        S::Rejected => &[
            (S::Repairing, C::RepairEntry),
            (S::Blocked, C::Blocked),
            (S::HumanRequired, C::HumanRequired),
            (S::Cancelled, C::Cancellation),
        ],
        S::Repairing => &[
            (S::AwaitingReview, C::RepairSubmitted),
            (S::Blocked, C::Blocked),
            (S::HumanRequired, C::HumanRequired),
            (S::Cancelled, C::Cancellation),
        ],
        S::Accepted => &[(S::Integrated, C::Integration)],
        S::Integrated | S::Cancelled => &[],
        S::Blocked => &[
            (S::Ready, C::Resolved),
            (S::HumanRequired, C::HumanRequired),
            (S::Cancelled, C::Cancellation),
        ],
        S::HumanRequired => &[
            (S::Ready, C::HumanResolved),
            (S::Cancelled, C::Cancellation),
        ],
        S::LockedRequiresPro => &[
            (S::Ready, C::CapabilityAdmission),
            (S::Cancelled, C::Cancellation),
        ],
    };
    if let Some((_, cause)) = allowed.iter().find(|(target, _)| *target == to) {
        return Allowed(*cause);
    }
    Refused(if from == to {
        R::SelfTransition
    } else if matches!(from, S::Accepted | S::Integrated | S::Cancelled) {
        R::Terminal
    } else if matches!(
        (from, to),
        (S::Passed, S::Rejected) | (S::Rejected, S::Passed)
    ) {
        R::VerdictRevision
    } else if from == S::LockedRequiresPro || to == S::LockedRequiresPro {
        R::LockedGate
    } else {
        R::NotALifecycleEdge
    })
}

#[test]
fn every_pair_of_15_states_has_explicit_verdict() {
    let mut allowed = 0;
    for from in S::ALL {
        for to in S::ALL {
            let rule = node_transition_rule(from, to);
            assert_eq!(rule, expected(from, to), "{from:?} -> {to:?}");
            if let NodeTransitionRule::Allowed(cause) = rule {
                allowed += 1;
                assert_eq!(check_node_transition(from, to, cause), Ok(()));
                for other in [C::Blocked, C::Acceptance, C::DependenciesSatisfied] {
                    if other != cause {
                        assert!(matches!(
                            check_node_transition(from, to, other),
                            Err(NodeTransitionError::WrongCause { .. })
                        ));
                    }
                }
            } else {
                for cause in [C::Acceptance, C::Cancellation, C::Blocked] {
                    assert!(matches!(
                        check_node_transition(from, to, cause),
                        Err(NodeTransitionError::Refused { .. })
                    ));
                }
            }
        }
    }
    assert_eq!(allowed, 46);
}

#[test]
fn forbidden_shortcuts_and_reopenings_are_refused() {
    for (from, to) in [
        (S::Planned, S::Accepted),
        (S::Planned, S::Integrated),
        (S::Running, S::Accepted),
        (S::AwaitingReview, S::Accepted),
        (S::Rejected, S::Passed),
        (S::Passed, S::Rejected),
        (S::Accepted, S::Planned),
        (S::Integrated, S::Ready),
        (S::Cancelled, S::Planned),
        (S::Blocked, S::AwaitingReview),
        (S::HumanRequired, S::Passed),
        (S::HumanRequired, S::Blocked),
        (S::LockedRequiresPro, S::Admitted),
        (S::LockedRequiresPro, S::Blocked),
        (S::Planned, S::LockedRequiresPro),
    ] {
        assert!(
            matches!(
                node_transition_rule(from, to),
                NodeTransitionRule::Refused(_)
            ),
            "{from:?} -> {to:?}"
        );
    }
}

#[test]
fn bounded_wrappers_agree_with_the_oracle() {
    type Wrapper = fn(S, S) -> bool;
    let wrappers: [Wrapper; 6] = [
        |f, t| validate_prefix_transition(f, t).is_ok(),
        |f, t| validate_review_verdict_transition(f, t).is_ok(),
        |f, t| validate_rejected_repair_transition(f, t).is_ok(),
        |f, t| validate_repair_completion_transition(f, t).is_ok(),
        |f, t| validate_passed_acceptance_transition(f, t).is_ok(),
        |f, t| validate_accepted_integration_transition(f, t).is_ok(),
    ];
    let mut covered = 0;
    for from in S::ALL {
        for to in S::ALL {
            if wrappers.iter().any(|w| w(from, to)) {
                covered += 1;
                assert!(matches!(
                    node_transition_rule(from, to),
                    NodeTransitionRule::Allowed(_)
                ));
            }
        }
    }
    assert_eq!(covered, 11);
}
