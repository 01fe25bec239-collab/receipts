//! Bounded repair-completion review re-entry transition over
//! [`GraphNodeState`].
//!
//! This slice authorizes exactly one deterministic legality question: once a
//! repair has completed, is the transition command
//! `(REPAIRING → AWAITING_REVIEW)` the single authorized re-entry edge back
//! into independent review?
//!
//! ```text
//! REPAIRING → AWAITING_REVIEW
//! ```
//!
//! A repaired implementation must never bypass fresh review, so this is the
//! only successful outgoing command this capsule recognizes for `REPAIRING`.
//!
//! Three outcomes exist and are kept explicitly distinct:
//!
//! * the repair-completion re-entry edge succeeds;
//! * a command whose source is [`GraphNodeState::Repairing`] but whose target
//!   is anything other than [`GraphNodeState::AwaitingReview`] fails with
//!   [`RepairCompletionTransitionError::UnsupportedRepairCompletionTarget`].
//!   This means only that the pair is not the single repair-completion edge
//!   authorized by this capsule — it is *not* a claim that the pair is
//!   globally architecturally illegal;
//! * a command whose source is anything other than
//!   [`GraphNodeState::Repairing`] fails with
//!   [`RepairCompletionTransitionError::OutsideRepairCompletionScope`]. Those
//!   transitions belong to other bounded validators or later lifecycle
//!   slices; no global legality judgment is made or implied here.
//!
//! This slice does not create or validate a repair capsule, does not decide
//! whether a repair actually finished, does not construct, dispatch, or
//! execute any review, and produces no verdict: it receives only typed states
//! that other authorized subsystems have already decided.
//!
//! Validation is pure and total over every `GraphNodeState × GraphNodeState`
//! pair: no storage, scheduler, persistence, entitlement, routing, runtime,
//! artifact, clock, or randomness input exists here, so the same `(from, to)`
//! always yields the same result.

use crate::node_state::GraphNodeState;

/// Why a `(from, to)` repair-completion transition command was rejected by
/// this bounded validator. Both endpoints are always carried verbatim so
/// every rejection is explicit and reproducible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RepairCompletionTransitionError {
    /// The source was `REPAIRING`, but the target was not `AWAITING_REVIEW`:
    /// not the single repair-completion edge authorized by this capsule.
    UnsupportedRepairCompletionTarget {
        /// The source state of the rejected command (`REPAIRING`).
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
    /// The source was not `REPAIRING`, so the command lies entirely beyond
    /// this capsule's repair-completion authority. No global legality
    /// judgment is made or implied.
    OutsideRepairCompletionScope {
        /// The source state of the rejected command.
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
}

impl RepairCompletionTransitionError {
    /// The source state of the rejected command.
    pub fn from(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedRepairCompletionTarget { from, to: _ }
            | Self::OutsideRepairCompletionScope { from, to: _ } => from,
        }
    }

    /// The target state of the rejected command.
    pub fn to(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedRepairCompletionTarget { from: _, to }
            | Self::OutsideRepairCompletionScope { from: _, to } => to,
        }
    }

    /// Whether the source was inside the repair-completion scope
    /// (`REPAIRING`) yet the target was not the one authorized review
    /// re-entry state (`AWAITING_REVIEW`).
    pub fn is_unsupported_repair_completion_target(&self) -> bool {
        matches!(self, Self::UnsupportedRepairCompletionTarget { .. })
    }

    /// Whether the source lay outside this capsule's repair-completion
    /// authority.
    pub fn is_outside_repair_completion_scope(&self) -> bool {
        matches!(self, Self::OutsideRepairCompletionScope { .. })
    }
}

impl std::fmt::Display for RepairCompletionTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (from, to) = (self.from().as_str(), self.to().as_str());
        match self {
            Self::UnsupportedRepairCompletionTarget { .. } => write!(
                f,
                "{from} → {to} is not the repair-completion review re-entry \
                 transition authorized by this capsule"
            ),
            Self::OutsideRepairCompletionScope { .. } => write!(
                f,
                "{from} → {to} starts outside this capsule's \
                 repair-completion scope"
            ),
        }
    }
}

impl std::error::Error for RepairCompletionTransitionError {}

/// Exactly the one authorized repair-completion transition, as a `(from, to)`
/// pair: `REPAIRING → AWAITING_REVIEW`.
pub const AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS: [(GraphNodeState, GraphNodeState); 1] =
    [(GraphNodeState::Repairing, GraphNodeState::AwaitingReview)];

/// Validates one transition command against the bounded repair-completion
/// review re-entry edge.
///
/// Returns `Ok(())` only when `(from, to)` is exactly the single authorized
/// edge out of `REPAIRING`. A `REPAIRING` command aimed anywhere other than
/// `AWAITING_REVIEW` yields
/// [`RepairCompletionTransitionError::UnsupportedRepairCompletionTarget`];
/// any command whose source is not `REPAIRING` yields
/// [`RepairCompletionTransitionError::OutsideRepairCompletionScope`].
pub const fn validate_repair_completion_transition(
    from: GraphNodeState,
    to: GraphNodeState,
) -> Result<(), RepairCompletionTransitionError> {
    if !matches!(from, GraphNodeState::Repairing) {
        return Err(RepairCompletionTransitionError::OutsideRepairCompletionScope { from, to });
    }
    if matches!(to, GraphNodeState::AwaitingReview) {
        Ok(())
    } else {
        Err(RepairCompletionTransitionError::UnsupportedRepairCompletionTarget { from, to })
    }
}
