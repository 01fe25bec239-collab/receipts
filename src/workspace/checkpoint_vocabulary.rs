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

/// Closed vocabulary for caller-supplied crash classification; never inferred by Workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCheckpointCrashClassification {
    RateLimited,
    SessionExhausted,
    AuthRequired,
    ProviderDown,
    Timeout,
    SandboxDenied,
    SafetyCheckPending,
    PolicyBlocked,
    RuntimeCrash,
    InvalidOutput,
    UserCancelled,
    Unknown,
}

impl WorkspaceCheckpointCrashClassification {
    pub const ALL: [Self; 12] = [
        Self::RateLimited,
        Self::SessionExhausted,
        Self::AuthRequired,
        Self::ProviderDown,
        Self::Timeout,
        Self::SandboxDenied,
        Self::SafetyCheckPending,
        Self::PolicyBlocked,
        Self::RuntimeCrash,
        Self::InvalidOutput,
        Self::UserCancelled,
        Self::Unknown,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RateLimited => "RATE_LIMITED",
            Self::SessionExhausted => "SESSION_EXHAUSTED",
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::ProviderDown => "PROVIDER_DOWN",
            Self::Timeout => "TIMEOUT",
            Self::SandboxDenied => "SANDBOX_DENIED",
            Self::SafetyCheckPending => "SAFETY_CHECK_PENDING",
            Self::PolicyBlocked => "POLICY_BLOCKED",
            Self::RuntimeCrash => "RUNTIME_CRASH",
            Self::InvalidOutput => "INVALID_OUTPUT",
            Self::UserCancelled => "USER_CANCELLED",
            Self::Unknown => "UNKNOWN",
        }
    }
}
