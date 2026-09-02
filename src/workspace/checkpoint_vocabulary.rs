/// Closed vocabulary describing why a workspace checkpoint was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCheckpointKind {
    Progress,
    PreTermination,
    RecoveryCapture,
}

impl WorkspaceCheckpointKind {
    pub const ALL: [Self; 3] = [Self::Progress, Self::PreTermination, Self::RecoveryCapture];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Progress => "PROGRESS",
            Self::PreTermination => "PRE_TERMINATION",
            Self::RecoveryCapture => "RECOVERY_CAPTURE",
        }
    }
}

/// Closed vocabulary for an explicitly selected workspace recovery decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceRecoveryDecision {
    ResetToLastAccepted,
    ContinueFromCheckpoint,
    InspectAndSalvage,
}

impl WorkspaceRecoveryDecision {
    pub const ALL: [Self; 3] = [
        Self::ResetToLastAccepted,
        Self::ContinueFromCheckpoint,
        Self::InspectAndSalvage,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResetToLastAccepted => "RESET_TO_LAST_ACCEPTED",
            Self::ContinueFromCheckpoint => "CONTINUE_FROM_CHECKPOINT",
            Self::InspectAndSalvage => "INSPECT_AND_SALVAGE",
        }
    }
}
