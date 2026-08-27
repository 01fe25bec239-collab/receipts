//! Bounded passed-review acceptance transition over [`GraphNodeState`].
//!
//! This capsule answers only whether `(PASSED → ACCEPTED)` is the single
//! transition it authorizes. It does not inspect review evidence, decide
//! whether acceptance is deserved, or authorize integration.

use crate::node_state::GraphNodeState;

/// Why a transition command was rejected by this bounded validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PassedAcceptanceTransitionError {
    /// The source was `PASSED`, but the target was not `ACCEPTED`.
    UnsupportedPassedAcceptanceTarget {
        /// The source state of the rejected command (`PASSED`).
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
    /// The source was not `PASSED`, so the command is outside this capsule.
    OutsidePassedAcceptanceScope {
        /// The source state of the rejected command.
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
}

impl PassedAcceptanceTransitionError {
    /// The source state of the rejected command.
    pub fn from(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedPassedAcceptanceTarget { from, to: _ }
            | Self::OutsidePassedAcceptanceScope { from, to: _ } => from,
        }
    }

    /// The target state of the rejected command.
    pub fn to(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedPassedAcceptanceTarget { from: _, to }
            | Self::OutsidePassedAcceptanceScope { from: _, to } => to,
        }
    }

    /// Whether `PASSED` was aimed at a target other than `ACCEPTED`.
    pub fn is_unsupported_passed_acceptance_target(&self) -> bool {
        matches!(self, Self::UnsupportedPassedAcceptanceTarget { .. })
    }

    /// Whether the source lay outside this capsule's `PASSED` scope.
    pub fn is_outside_passed_acceptance_scope(&self) -> bool {
        matches!(self, Self::OutsidePassedAcceptanceScope { .. })
    }
}

impl std::fmt::Display for PassedAcceptanceTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (from, to) = (self.from().as_str(), self.to().as_str());
        match self {
            Self::UnsupportedPassedAcceptanceTarget { .. } => write!(
                f,
                "{from} → {to} is not the passed-acceptance transition \
                 authorized by this capsule"
            ),
            Self::OutsidePassedAcceptanceScope { .. } => write!(
                f,
                "{from} → {to} starts outside this capsule's \
                 passed-acceptance scope"
            ),
        }
    }
}

impl std::error::Error for PassedAcceptanceTransitionError {}

/// Exactly the one authorized transition: `PASSED → ACCEPTED`.
pub const AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS: [(GraphNodeState, GraphNodeState); 1] =
    [(GraphNodeState::Passed, GraphNodeState::Accepted)];

/// Validates one command against the bounded `PASSED → ACCEPTED` edge.
pub const fn validate_passed_acceptance_transition(
    from: GraphNodeState,
    to: GraphNodeState,
) -> Result<(), PassedAcceptanceTransitionError> {
    if !matches!(from, GraphNodeState::Passed) {
        return Err(PassedAcceptanceTransitionError::OutsidePassedAcceptanceScope { from, to });
    }
    if matches!(to, GraphNodeState::Accepted) {
        Ok(())
    } else {
        Err(PassedAcceptanceTransitionError::UnsupportedPassedAcceptanceTarget { from, to })
    }
}
