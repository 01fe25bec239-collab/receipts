//! Bounded post-review transition fork over [`GraphNodeState`].
//!
//! This slice authorizes exactly one deterministic legality question: given
//! an already-produced review verdict represented as a target state, is the
//! transition command `(AWAITING_REVIEW → verdict)` one of the two
//! authorized review-fork edges?
//!
//! ```text
//! AWAITING_REVIEW → PASSED
//! AWAITING_REVIEW → REJECTED
//! ```
//!
//! Three outcomes exist and are kept explicitly distinct:
//!
//! * either fork edge succeeds;
//! * a command whose source is [`GraphNodeState::AwaitingReview`] but whose
//!   target is anything else fails with
//!   [`ReviewVerdictTransitionError::UnsupportedReviewVerdictTarget`]. This
//!   means only that the pair is not one of the two review-fork transitions
//!   authorized by this capsule — it is *not* a claim that the pair is
//!   globally architecturally illegal;
//! * a command whose source is anything other than
//!   [`GraphNodeState::AwaitingReview`] fails with
//!   [`ReviewVerdictTransitionError::OutsideReviewVerdictForkScope`]. Those
//!   transitions belong to other lifecycle slices or subsystems; no global
//!   legality judgment is made or implied here.
//!
//! This slice does not produce, interpret, verify, accept, reject, or
//! otherwise own any review verdict, and implements none of the post-fork
//! semantics (`PASSED → …`, `REJECTED → …`, repair, acceptance,
//! integration). It receives only typed states: whether a real review passed
//! or failed is decided by another authorized subsystem before this validator
//! is invoked.
//!
//! Validation is pure and total over every `GraphNodeState × GraphNodeState`
//! pair: no storage, scheduler, persistence, entitlement, routing, runtime,
//! artifact, clock, or randomness input exists here, so the same `(from, to)`
//! always yields the same result.

use crate::node_state::GraphNodeState;

/// Why a `(from, to)` review-verdict transition command was rejected by this
/// bounded fork validator. Both endpoints are always carried verbatim so
/// every rejection is explicit and reproducible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReviewVerdictTransitionError {
    /// The source was `AWAITING_REVIEW`, but the target was neither `PASSED`
    /// nor `REJECTED`: not one of the two review-fork transitions authorized
    /// by this capsule.
    UnsupportedReviewVerdictTarget {
        /// The source state of the rejected command (`AWAITING_REVIEW`).
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
    /// The source was not `AWAITING_REVIEW`, so the command lies entirely
    /// beyond this capsule's review-fork authority. No global legality
    /// judgment is made or implied.
    OutsideReviewVerdictForkScope {
        /// The source state of the rejected command.
        from: GraphNodeState,
        /// The target state of the rejected command.
        to: GraphNodeState,
    },
}

impl ReviewVerdictTransitionError {
    /// The source state of the rejected command.
    pub fn from(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedReviewVerdictTarget { from, to: _ }
            | Self::OutsideReviewVerdictForkScope { from, to: _ } => from,
        }
    }

    /// The target state of the rejected command.
    pub fn to(&self) -> GraphNodeState {
        match *self {
            Self::UnsupportedReviewVerdictTarget { from: _, to }
            | Self::OutsideReviewVerdictForkScope { from: _, to } => to,
        }
    }

    /// Whether the source was inside the fork (`AWAITING_REVIEW`) yet the
    /// target was not one of the two authorized verdict states.
    pub fn is_unsupported_review_verdict_target(&self) -> bool {
        matches!(self, Self::UnsupportedReviewVerdictTarget { .. })
    }

    /// Whether the source lay outside this capsule's review-fork authority.
    pub fn is_outside_review_verdict_fork_scope(&self) -> bool {
        matches!(self, Self::OutsideReviewVerdictForkScope { .. })
    }
}

impl std::fmt::Display for ReviewVerdictTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (from, to) = (self.from().as_str(), self.to().as_str());
        match self {
            Self::UnsupportedReviewVerdictTarget { .. } => write!(
                f,
                "{from} → {to} is not one of the two review-verdict fork \
                 transitions authorized by this capsule"
            ),
            Self::OutsideReviewVerdictForkScope { .. } => write!(
                f,
                "{from} → {to} starts outside this capsule's \
                 review-verdict fork scope"
            ),
        }
    }
}

impl std::error::Error for ReviewVerdictTransitionError {}

/// Exactly the two authorized review-fork transitions, as `(from, to)` pairs:
///
/// `AWAITING_REVIEW → PASSED`, `AWAITING_REVIEW → REJECTED`.
pub const AUTHORIZED_REVIEW_VERDICT_TRANSITIONS: [(GraphNodeState, GraphNodeState); 2] = [
    (GraphNodeState::AwaitingReview, GraphNodeState::Passed),
    (GraphNodeState::AwaitingReview, GraphNodeState::Rejected),
];

/// Validates one transition command against the bounded review-verdict fork.
///
/// Returns `Ok(())` only when `(from, to)` is exactly one of the two
/// authorized fork edges out of `AWAITING_REVIEW`. An `AWAITING_REVIEW`
/// command aimed anywhere else yields
/// [`ReviewVerdictTransitionError::UnsupportedReviewVerdictTarget`]; any
/// command whose source is not `AWAITING_REVIEW` yields
/// [`ReviewVerdictTransitionError::OutsideReviewVerdictForkScope`].
pub const fn validate_review_verdict_transition(
    from: GraphNodeState,
    to: GraphNodeState,
) -> Result<(), ReviewVerdictTransitionError> {
    if !matches!(from, GraphNodeState::AwaitingReview) {
        return Err(ReviewVerdictTransitionError::OutsideReviewVerdictForkScope { from, to });
    }
    if matches!(to, GraphNodeState::Passed | GraphNodeState::Rejected) {
        Ok(())
    } else {
        Err(ReviewVerdictTransitionError::UnsupportedReviewVerdictTarget { from, to })
    }
}
