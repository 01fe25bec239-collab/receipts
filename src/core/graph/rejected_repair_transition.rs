//! Bounded rejected-review repair-entry transition over [`GraphNodeState`].
//!
//! This slice authorizes exactly one deterministic legality question: given a
//! produced review verdict of rejection represented as the source state, is
//! the transition command `(REJECTED → REPAIRING)` the single authorized
//! repair-entry edge?
//!
//! ```text
//! REJECTED → REPAIRING
//! ```
//!
//! Three outcomes exist and are kept explicitly distinct:
//!
//! * the repair-entry edge succeeds;
//! * a command whose source is [`GraphNodeState::Rejected`] but whose target
//!   is anything other than [`GraphNodeState::Repairing`] fails with
//!   [`RejectedRepairTransitionError::UnsupportedRejectedRepairTarget`]. This
//!   means only that the pair is not the single repair-entry transition
//!   authorized by this capsule — it is *not* a claim that the pair is
//!   globally architecturally illegal;
//! * a command whose source is anything other than
//!   [`GraphNodeState::Rejected`] fails with
//!   [`RejectedRepairTransitionError::OutsideRejectedRepairEntryScope`]. Those
//!   transitions belong to later lifecycle slices or other subsystems; no
//!   global legality judgment is made or implied here.
//!
//! This slice does not construct or authorize any repair attempt, does not
//! select or dispatch fresh execution, does not interpret review findings,
//! and implements no transition out of `REPAIRING`: its outgoing semantics
//! are deliberately deferred to later bounded slices. It receives only typed
//! states: whether a real review rejected and whether a repair is warranted
//! are decided by other authorized subsystems before this validator is
//! invoked.
//!
//! Validation is pure and total over every `GraphNodeState × GraphNodeState`
//! pair: no storage, scheduler, persistence, entitlement, routing, runtime,
//! artifact, clock, or randomness input exists here, so the same `(from, to)`
//! always yields the same result.

use crate::node_state::GraphNodeState;

/// Why a `(from, to)` rejected-repair-entry transition command was rejected by
/// this bounded validator. Both endpoints are always carried verbatim so
/// every rejection is explicit and reproducible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RejectedRepairTransitionError {
    /// The source was `REJECTED`, but the target was not `REPAIRING`: not the
    /// single repair-entry transition authorized by this capsule.
    UnsupportedRejectedRepairTarget {
        /// The source state of the rejected command (`REJECTED`).
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
    /// The source was not `REJECTED`, so the command lies entirely beyond
    /// this capsule's repair-entry authority. No global legality judgment is
    /// made or implied.
    OutsideRejectedRepairEntryScope {
        /// The source state of the rejected command.
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
}

impl RejectedRepairTransitionError {
    /// The source state of the rejected command.
    pub fn from(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedRejectedRepairTarget { from, to: _ }
            | Self::OutsideRejectedRepairEntryScope { from, to: _ } => from,
        }
    }

    /// The target state of the rejected command.
    pub fn to(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedRejectedRepairTarget { from: _, to }
            | Self::OutsideRejectedRepairEntryScope { from: _, to } => to,
        }
    }

    /// Whether the source was inside the repair-entry scope (`REJECTED`) yet
    /// the target was not the one authorized repair state (`REPAIRING`).
    pub fn is_unsupported_rejected_repair_target(&self) -> bool {
        matches!(self, Self::UnsupportedRejectedRepairTarget { .. })
    }

    /// Whether the source lay outside this capsule's repair-entry authority.
    pub fn is_outside_rejected_repair_entry_scope(&self) -> bool {
        matches!(self, Self::OutsideRejectedRepairEntryScope { .. })
    }
}

impl std::fmt::Display for RejectedRepairTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (from, to) = (self.from().as_str(), self.to().as_str());
        match self {
            Self::UnsupportedRejectedRepairTarget { .. } => write!(
                f,
                "{from} → {to} is not the rejected-repair entry transition \
                 authorized by this capsule"
            ),
            Self::OutsideRejectedRepairEntryScope { .. } => write!(
                f,
                "{from} → {to} starts outside this capsule's \
                 rejected-repair entry scope"
            ),
        }
    }
}

impl std::error::Error for RejectedRepairTransitionError {}

/// Exactly the one authorized repair-entry transition, as a `(from, to)`
/// pair: `REJECTED → REPAIRING`.
pub const AUTHORIZED_REJECTED_REPAIR_TRANSITIONS: [(GraphNodeState, GraphNodeState); 1] =
    [(GraphNodeState::Rejected, GraphNodeState::Repairing)];

/// Validates one transition command against the bounded rejected-repair
/// entry edge.
///
/// Returns `Ok(())` only when `(from, to)` is exactly the single authorized
/// edge out of `REJECTED`. A `REJECTED` command aimed anywhere other than
/// `REPAIRING` yields
/// [`RejectedRepairTransitionError::UnsupportedRejectedRepairTarget`]; any
/// command whose source is not `REJECTED` yields
/// [`RejectedRepairTransitionError::OutsideRejectedRepairEntryScope`].
pub const fn validate_rejected_repair_transition(
    from: GraphNodeState,
    to: GraphNodeState,
) -> Result<(), RejectedRepairTransitionError> {
    if !matches!(from, GraphNodeState::Rejected) {
        return Err(RejectedRepairTransitionError::OutsideRejectedRepairEntryScope { from, to });
    }
    if matches!(to, GraphNodeState::Repairing) {
        Ok(())
    } else {
        Err(RejectedRepairTransitionError::UnsupportedRejectedRepairTarget { from, to })
    }
}
