//! Bounded accepted-integration transition over [`GraphNodeState`].
//!
//! This capsule answers only whether `(ACCEPTED → INTEGRATED)` is the
//! single transition it authorizes. It does not decide whether integration
//! should happen or inspect any policy, evidence, CI, PR, provenance, or Git
//! state.

use crate::node_state::GraphNodeState;

/// Why a transition command was rejected by this bounded validator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AcceptedIntegrationTransitionError {
    /// The source was `ACCEPTED`, but the target was not `INTEGRATED`.
    UnsupportedAcceptedIntegrationTarget {
        /// The source state of the rejected command (`ACCEPTED`).
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
    /// The source was not `ACCEPTED`, so the command is outside this capsule.
    OutsideAcceptedIntegrationScope {
        /// The source state of the rejected command.
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
}

impl AcceptedIntegrationTransitionError {
    /// The source state of the rejected command.
    pub fn from(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedAcceptedIntegrationTarget { from, to: _ }
            | Self::OutsideAcceptedIntegrationScope { from, to: _ } => from,
        }
    }

    /// The target state of the rejected command.
    pub fn to(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedAcceptedIntegrationTarget { from: _, to }
            | Self::OutsideAcceptedIntegrationScope { from: _, to } => to,
        }
    }

    /// Whether `ACCEPTED` was aimed at a target other than `INTEGRATED`.
    pub fn is_unsupported_accepted_integration_target(&self) -> bool {
        matches!(self, Self::UnsupportedAcceptedIntegrationTarget { .. })
    }

    /// Whether the source lay outside this capsule's `ACCEPTED` scope.
    pub fn is_outside_accepted_integration_scope(&self) -> bool {
        matches!(self, Self::OutsideAcceptedIntegrationScope { .. })
    }
}

impl std::fmt::Display for AcceptedIntegrationTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (from, to) = (self.from().as_str(), self.to().as_str());
        match self {
            Self::UnsupportedAcceptedIntegrationTarget { .. } => write!(
                f,
                "{from} → {to} is not the accepted-integration transition \
                 authorized by this capsule"
            ),
            Self::OutsideAcceptedIntegrationScope { .. } => write!(
                f,
                "{from} → {to} starts outside this capsule's \
                 accepted-integration scope"
            ),
        }
    }
}

impl std::error::Error for AcceptedIntegrationTransitionError {}

/// Exactly the one authorized transition: `ACCEPTED → INTEGRATED`.
pub const AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS: [(GraphNodeState, GraphNodeState); 1] =
    [(GraphNodeState::Accepted, GraphNodeState::Integrated)];

/// Validates one command against the bounded `ACCEPTED → INTEGRATED` edge.
pub const fn validate_accepted_integration_transition(
    from: GraphNodeState,
    to: GraphNodeState,
) -> Result<(), AcceptedIntegrationTransitionError> {
    if !matches!(from, GraphNodeState::Accepted) {
        return Err(
            AcceptedIntegrationTransitionError::OutsideAcceptedIntegrationScope { from, to },
        );
    }
    if matches!(to, GraphNodeState::Integrated) {
        Ok(())
    } else {
        Err(AcceptedIntegrationTransitionError::UnsupportedAcceptedIntegrationTarget { from, to })
    }
}
