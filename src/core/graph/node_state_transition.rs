//! Bounded pre-review lifecycle transition prefix over [`GraphNodeState`].
//!
//! This slice authorizes exactly one deterministic legality question:
//! is `(from, to)` one of the five forward edges of the capsule prefix
//!
//! ```text
//! PLANNED → READY → ADMITTED → DISPATCHED → RUNNING → AWAITING_REVIEW
//! ```
//!
//! Three outcomes exist and are kept explicitly distinct:
//!
//! * an allowed prefix edge succeeds;
//! * a pair where **both** endpoints lie inside the six-state prefix but
//!   which is not one of the five edges fails with
//!   [`GraphNodeStateTransitionError::UnsupportedPrefixTransition`] (this
//!   includes self-pairs, shortcuts, and reversals);
//! * any pair with an endpoint outside the prefix fails with
//!   [`GraphNodeStateTransitionError::OutsidePrefixScope`]. This is *not* a
//!   claim that such a pair is globally illegal — it is simply beyond this
//!   capsule's authority. Later lifecycle stages define their own slices.
//!
//! Validation is pure and total: no storage, scheduler, persistence,
//! entitlement, routing, or runtime input exists here, so the same
//! `(from, to)` pair always yields the same result. This is a
//! transition-command validator, not an idempotent state-assignment API:
//! re-commanding the current state is rejected like any other non-edge.

use crate::node_state::GraphNodeState;

/// Why a `(from, to)` transition command was rejected by this bounded
/// prefix validator. Both endpoints are always carried verbatim so every
/// rejection is explicit and reproducible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphNodeStateTransitionError {
    /// Both states belong to this capsule's six-state prefix, but the pair
    /// is not one of the five authorized forward edges.
    UnsupportedPrefixTransition {
        /// The source state of the rejected command.
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
    /// At least one endpoint lies outside this capsule's prefix. No global
    /// legality judgment is made or implied.
    OutsidePrefixScope {
        /// The source state of the rejected command.
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
}

impl GraphNodeStateTransitionError {
    /// The source state of the rejected command.
    pub fn from(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedPrefixTransition { from, to: _ }
            | Self::OutsidePrefixScope { from, to: _ } => from,
        }
    }

    /// The target state of the rejected command.
    pub fn to(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedPrefixTransition { from: _, to }
            | Self::OutsidePrefixScope { from: _, to } => to,
        }
    }

    /// Whether both endpoints were inside the prefix yet the pair was not an
    /// authorized edge.
    pub fn is_unsupported_prefix_transition(&self) -> bool {
        matches!(self, Self::UnsupportedPrefixTransition { .. })
    }

    /// Whether at least one endpoint was outside this capsule's prefix.
    pub fn is_outside_prefix_scope(&self) -> bool {
        matches!(self, Self::OutsidePrefixScope { .. })
    }
}

impl std::fmt::Display for GraphNodeStateTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (from, to) = (self.from().as_str(), self.to().as_str());
        match self {
            Self::UnsupportedPrefixTransition { .. } => write!(
                f,
                "{from} → {to} is not one of the authorized \
                 pre-review prefix transitions"
            ),
            Self::OutsidePrefixScope { .. } => write!(
                f,
                "{from} → {to} involves a state outside this capsule's \
                 prefix scope"
            ),
        }
    }
}

impl std::error::Error for GraphNodeStateTransitionError {}

/// The six prefix states this capsule speaks about, in canonical lifecycle
/// order. Listing order carries no hidden meaning beyond documentation.
pub const PREFIX_STATES: [GraphNodeState; 6] = [
    GraphNodeState::Planned,
    GraphNodeState::Ready,
    GraphNodeState::Admitted,
    GraphNodeState::Dispatched,
    GraphNodeState::Running,
    GraphNodeState::AwaitingReview,
];

/// Exactly the five authorized prefix transitions, as `(from, to)` pairs:
///
/// `PLANNED → READY`, `READY → ADMITTED`, `ADMITTED → DISPATCHED`,
/// `DISPATCHED → RUNNING`, `RUNNING → AWAITING_REVIEW`.
pub const AUTHORIZED_PREFIX_TRANSITIONS: [(GraphNodeState, GraphNodeState); 5] = [
    (GraphNodeState::Planned, GraphNodeState::Ready),
    (GraphNodeState::Ready, GraphNodeState::Admitted),
    (GraphNodeState::Admitted, GraphNodeState::Dispatched),
    (GraphNodeState::Dispatched, GraphNodeState::Running),
    (GraphNodeState::Running, GraphNodeState::AwaitingReview),
];

/// Whether `state` belongs to this capsule's six-state prefix.
pub const fn is_prefix_state(state: GraphNodeState) -> bool {
    matches!(
        state,
        GraphNodeState::Planned
            | GraphNodeState::Ready
            | GraphNodeState::Admitted
            | GraphNodeState::Dispatched
            | GraphNodeState::Running
            | GraphNodeState::AwaitingReview
    )
}

/// Validates one transition command against the bounded pre-review prefix.
///
/// Returns `Ok(())` only when `(from, to)` is exactly one of the five
/// authorized prefix edges. A same-prefix non-edge yields
/// [`GraphNodeStateTransitionError::UnsupportedPrefixTransition`]; anything
/// touching a non-prefix state yields
/// [`GraphNodeStateTransitionError::OutsidePrefixScope`].
pub const fn validate_prefix_transition(
    from: GraphNodeState,
    to: GraphNodeState,
) -> Result<(), GraphNodeStateTransitionError> {
    if !is_prefix_state(from) || !is_prefix_state(to) {
        return Err(GraphNodeStateTransitionError::OutsidePrefixScope { from, to });
    }
    if matches!(
        (from, to),
        (GraphNodeState::Planned, GraphNodeState::Ready)
            | (GraphNodeState::Ready, GraphNodeState::Admitted)
            | (GraphNodeState::Admitted, GraphNodeState::Dispatched)
            | (GraphNodeState::Dispatched, GraphNodeState::Running)
            | (GraphNodeState::Running, GraphNodeState::AwaitingReview),
    ) {
        Ok(())
    } else {
        Err(GraphNodeStateTransitionError::UnsupportedPrefixTransition { from, to })
    }
}
