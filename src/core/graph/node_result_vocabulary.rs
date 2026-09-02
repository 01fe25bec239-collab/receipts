//! Closed vocabularies embedded in the frozen `GraphNodeResult` contract.

/// The exact outcomes a graph-node result may report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphNodeResultOutcome {
    Pass,
    Fail,
    Rejected,
    Blocked,
    Cancelled,
    Skipped,
    HumanRequired,
}

impl GraphNodeResultOutcome {
    /// Every outcome in frozen schema order.
    pub const ALL: [Self; 7] = [
        Self::Pass,
        Self::Fail,
        Self::Rejected,
        Self::Blocked,
        Self::Cancelled,
        Self::Skipped,
        Self::HumanRequired,
    ];

    /// The exact canonical schema string for this outcome.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Rejected => "REJECTED",
            Self::Blocked => "BLOCKED",
            Self::Cancelled => "CANCELLED",
            Self::Skipped => "SKIPPED",
            Self::HumanRequired => "HUMAN_REQUIRED",
        }
    }
}

/// The exact results an individual graph-node check may report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GraphNodeCheckResult {
    Pass,
    Fail,
    Error,
    Skipped,
    Unknown,
}

impl GraphNodeCheckResult {
    /// Every check result in frozen schema order.
    pub const ALL: [Self; 5] = [
        Self::Pass,
        Self::Fail,
        Self::Error,
        Self::Skipped,
        Self::Unknown,
    ];

    /// The exact canonical schema string for this check result.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::Error => "ERROR",
            Self::Skipped => "SKIPPED",
            Self::Unknown => "UNKNOWN",
        }
    }
}
