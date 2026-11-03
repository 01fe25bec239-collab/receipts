//! The single legality oracle over all 15 × 15 [`GraphNodeState`] pairs.
//!
//! Every pair has exactly one explicit answer: either it is a lifecycle edge
//! that requires one specific kind of typed cause, or it is refused with a
//! named reason. The six bounded validators (prefix, review verdict, rejected
//! repair, repair completion, passed acceptance, accepted integration) remain
//! public and unchanged; their authorized pairs are internal cases here.
//!
//! A pair being allowed proves nothing by itself: the caller (the apply-v1
//! interpreter) must still verify the cause evidence against the graph. The
//! oracle never treats a state pair as a review, gate, human instruction or
//! licence.

use crate::node_state::GraphNodeState;

use GraphNodeState as S;

/// The kind of evidence a lifecycle edge requires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionCauseKind {
    /// `PLANNED → READY`: every precedence dependency is satisfied.
    DependenciesSatisfied,
    /// `READY → ADMITTED`: an admission decision reference.
    Admission,
    /// `ADMITTED → DISPATCHED`: a fresh attempt identity.
    Dispatch,
    /// `DISPATCHED → RUNNING`.
    Start,
    /// `RUNNING → AWAITING_REVIEW`.
    Submit,
    /// `AWAITING_REVIEW → PASSED | REJECTED`: a review verdict in the graph.
    ReviewVerdict,
    /// `REJECTED → REPAIRING`: an expanded repair node.
    RepairEntry,
    /// `REPAIRING → AWAITING_REVIEW`: the repair node was submitted.
    RepairSubmitted,
    /// `PASSED → ACCEPTED`: an explicit acceptance decision.
    Acceptance,
    /// `ACCEPTED → INTEGRATED`: an explicit integration decision.
    Integration,
    /// Nonterminal work → `BLOCKED` with a recorded reason.
    Blocked,
    /// Nonterminal work → `HUMAN_REQUIRED` with a recorded reason.
    HumanRequired,
    /// `BLOCKED → READY` after explicit resolution/replanning.
    Resolved,
    /// `HUMAN_REQUIRED → READY` after an explicit human instruction.
    HumanResolved,
    /// `READY → LOCKED_REQUIRES_PRO`: a required capability is locked.
    FeatureLocked,
    /// `LOCKED_REQUIRES_PRO → READY`: current verified capability admission.
    CapabilityAdmission,
    /// Nonterminal work → `CANCELLED` by admitted amendment (`CANCEL_NODE`).
    Cancellation,
}

/// Why a pair is never a lifecycle edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransitionRefusal {
    /// Re-commanding the current state.
    SelfTransition,
    /// `INTEGRATED` and `CANCELLED` are final; `ACCEPTED` only integrates.
    Terminal,
    /// A review verdict is never revised (`PASSED ↔ REJECTED`).
    VerdictRevision,
    /// A locked node only leaves through capability admission or cancellation.
    LockedGate,
    /// Any other pair: a shortcut, reversal or claimed-state restoration.
    NotALifecycleEdge,
}

/// The oracle's answer for one ordered pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeTransitionRule {
    Allowed(TransitionCauseKind),
    Refused(TransitionRefusal),
}

/// A refused transition command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NodeTransitionError {
    Refused {
        from: GraphNodeState,
        to: GraphNodeState,
        reason: TransitionRefusal,
    },
    WrongCause {
        from: GraphNodeState,
        to: GraphNodeState,
        required: TransitionCauseKind,
        supplied: TransitionCauseKind,
    },
}

impl std::fmt::Display for NodeTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { from, to, reason } => write!(
                f,
                "{} → {} is refused: {reason:?}",
                from.as_str(),
                to.as_str()
            ),
            Self::WrongCause {
                from,
                to,
                required,
                supplied,
            } => write!(
                f,
                "{} → {} requires {required:?} evidence, not {supplied:?}",
                from.as_str(),
                to.as_str()
            ),
        }
    }
}

impl std::error::Error for NodeTransitionError {}

/// `ACCEPTED`, `INTEGRATED` and `CANCELLED` are never reopened.
pub const fn is_terminal_state(state: GraphNodeState) -> bool {
    matches!(state, S::Accepted | S::Integrated | S::Cancelled)
}

/// The rule for `(from, to)`. Total and pure.
pub const fn node_transition_rule(from: GraphNodeState, to: GraphNodeState) -> NodeTransitionRule {
    use NodeTransitionRule::{Allowed, Refused};
    use TransitionCauseKind as C;
    if from as u8 == to as u8 {
        return Refused(TransitionRefusal::SelfTransition);
    }
    match (from, to) {
        (S::Planned, S::Ready) => Allowed(C::DependenciesSatisfied),
        (S::Ready, S::Admitted) => Allowed(C::Admission),
        (S::Admitted, S::Dispatched) => Allowed(C::Dispatch),
        (S::Dispatched, S::Running) => Allowed(C::Start),
        (S::Running, S::AwaitingReview) => Allowed(C::Submit),
        (S::AwaitingReview, S::Passed | S::Rejected) => Allowed(C::ReviewVerdict),
        (S::Rejected, S::Repairing) => Allowed(C::RepairEntry),
        (S::Repairing, S::AwaitingReview) => Allowed(C::RepairSubmitted),
        (S::Passed, S::Accepted) => Allowed(C::Acceptance),
        (S::Accepted, S::Integrated) => Allowed(C::Integration),
        (S::Accepted | S::Integrated | S::Cancelled, _) => Refused(TransitionRefusal::Terminal),
        (S::Passed, S::Rejected) | (S::Rejected, S::Passed) => {
            Refused(TransitionRefusal::VerdictRevision)
        }
        (S::Ready, S::LockedRequiresPro) => Allowed(C::FeatureLocked),
        (S::LockedRequiresPro, S::Ready) => Allowed(C::CapabilityAdmission),
        (S::LockedRequiresPro, S::Cancelled) => Allowed(C::Cancellation),
        (S::LockedRequiresPro, _) | (_, S::LockedRequiresPro) => {
            Refused(TransitionRefusal::LockedGate)
        }
        (S::Blocked, S::Ready) => Allowed(C::Resolved),
        (S::HumanRequired, S::Ready) => Allowed(C::HumanResolved),
        // Only a human instruction leaves HUMAN_REQUIRED for more work.
        (S::HumanRequired, S::Blocked) => Refused(TransitionRefusal::NotALifecycleEdge),
        (_, S::Blocked) => Allowed(C::Blocked),
        (_, S::HumanRequired) => Allowed(C::HumanRequired),
        (_, S::Cancelled) => Allowed(C::Cancellation),
        _ => Refused(TransitionRefusal::NotALifecycleEdge),
    }
}

/// Checks a command against the oracle with the supplied cause kind.
pub const fn check_node_transition(
    from: GraphNodeState,
    to: GraphNodeState,
    cause: TransitionCauseKind,
) -> Result<(), NodeTransitionError> {
    match node_transition_rule(from, to) {
        NodeTransitionRule::Refused(reason) => {
            Err(NodeTransitionError::Refused { from, to, reason })
        }
        NodeTransitionRule::Allowed(required) if required as u8 == cause as u8 => Ok(()),
        NodeTransitionRule::Allowed(required) => Err(NodeTransitionError::WrongCause {
            from,
            to,
            required,
            supplied: cause,
        }),
    }
}
