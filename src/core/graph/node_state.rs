//! `GraphNodeState` — the closed, frozen state vocabulary for a graph node.
//!
//! This slice defines **only** the vocabulary itself plus strict
//! deterministic string conversion/parsing. State transitions, admission,
//! scheduling, recovery, and cancellation semantics are out of scope and are
//! deliberately not present here.
//!
//! Contrast with [`crate::node::GraphNodeKind`]: the node *kind* is an
//! extensible string, while the node *state* is a closed enum — exactly the
//! fifteen frozen values below, with no aliases, no fallbacks, and no legacy
//! TaskDag compatibility.

/// Why a node-state string could not be parsed. Unknown input fails
/// explicitly: it is never trimmed, never case-normalized, never aliased onto
/// a valid state, and never silently mapped to a permissive fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNodeStateParseError {
    /// The rejected input, byte-for-byte as given.
    input: String,
}

impl GraphNodeStateParseError {
    /// The rejected input, verbatim.
    pub fn input(&self) -> &str {
        &self.input
    }
}

impl std::fmt::Display for GraphNodeStateParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown graph node state {input:?}: expected one of the \
{CANONICAL_STATE_COUNT} canonical uppercase forms",
            input = self.input,
            CANONICAL_STATE_COUNT = GraphNodeState::ALL.len(),
        )
    }
}

impl std::error::Error for GraphNodeStateParseError {}

/// The closed, frozen node-state vocabulary.
///
/// Exactly these fifteen states exist; the set can never be extended by
/// callers. Declaration order carries no meaning: it is not priority,
/// scheduler order, transition legality, or lifecycle ranking, and numeric
/// discriminants carry no business meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphNodeState {
    /// `PLANNED`.
    Planned,
    /// `READY`.
    Ready,
    /// `ADMITTED`.
    Admitted,
    /// `DISPATCHED`.
    Dispatched,
    /// `RUNNING`.
    Running,
    /// `AWAITING_REVIEW`.
    AwaitingReview,
    /// `PASSED`.
    Passed,
    /// `REJECTED`.
    Rejected,
    /// `REPAIRING`.
    Repairing,
    /// `ACCEPTED`.
    Accepted,
    /// `INTEGRATED`.
    Integrated,
    /// `BLOCKED`.
    Blocked,
    /// `LOCKED_REQUIRES_PRO`.
    LockedRequiresPro,
    /// `CANCELLED`.
    Cancelled,
    /// `HUMAN_REQUIRED`.
    HumanRequired,
}

impl GraphNodeState {
    /// Every state, listed once each. Iteration order is vocabulary-listing
    /// order only and must never be interpreted as lifecycle sequence,
    /// priority, or transition legality.
    pub const ALL: [Self; 15] = [
        Self::Planned,
        Self::Ready,
        Self::Admitted,
        Self::Dispatched,
        Self::Running,
        Self::AwaitingReview,
        Self::Passed,
        Self::Rejected,
        Self::Repairing,
        Self::Accepted,
        Self::Integrated,
        Self::Blocked,
        Self::LockedRequiresPro,
        Self::Cancelled,
        Self::HumanRequired,
    ];

    /// The exact canonical uppercase wire string for this state,
    /// byte-for-byte the frozen spelling.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Planned => "PLANNED",
            Self::Ready => "READY",
            Self::Admitted => "ADMITTED",
            Self::Dispatched => "DISPATCHED",
            Self::Running => "RUNNING",
            Self::AwaitingReview => "AWAITING_REVIEW",
            Self::Passed => "PASSED",
            Self::Rejected => "REJECTED",
            Self::Repairing => "REPAIRING",
            Self::Accepted => "ACCEPTED",
            Self::Integrated => "INTEGRATED",
            Self::Blocked => "BLOCKED",
            Self::LockedRequiresPro => "LOCKED_REQUIRES_PRO",
            Self::Cancelled => "CANCELLED",
            Self::HumanRequired => "HUMAN_REQUIRED",
        }
    }

    /// Strictly parses a canonical state string.
    ///
    /// Parsing is exact and case-sensitive: input is matched byte-for-byte
    /// against the frozen spellings. Whitespace, different casing, empty
    /// input, and legacy TaskDag names (`IN_PROGRESS`, `REVIEW_PASSED`,
    /// `REVIEW_REJECTED`) are all rejected with an explicit
    /// [`GraphNodeStateParseError`] — never trimmed, normalized, or aliased.
    pub fn parse(value: &str) -> Result<Self, GraphNodeStateParseError> {
        match value {
            "PLANNED" => Ok(Self::Planned),
            "READY" => Ok(Self::Ready),
            "ADMITTED" => Ok(Self::Admitted),
            "DISPATCHED" => Ok(Self::Dispatched),
            "RUNNING" => Ok(Self::Running),
            "AWAITING_REVIEW" => Ok(Self::AwaitingReview),
            "PASSED" => Ok(Self::Passed),
            "REJECTED" => Ok(Self::Rejected),
            "REPAIRING" => Ok(Self::Repairing),
            "ACCEPTED" => Ok(Self::Accepted),
            "INTEGRATED" => Ok(Self::Integrated),
            "BLOCKED" => Ok(Self::Blocked),
            "LOCKED_REQUIRES_PRO" => Ok(Self::LockedRequiresPro),
            "CANCELLED" => Ok(Self::Cancelled),
            "HUMAN_REQUIRED" => Ok(Self::HumanRequired),
            other => Err(GraphNodeStateParseError {
                input: other.to_owned(),
            }),
        }
    }
}
