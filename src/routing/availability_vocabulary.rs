#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityStateKind {
    Available,
    Degraded,
    RateLimited,
    SessionExhausted,
    AuthRequired,
    ProviderDown,
    SafetyCheckPending,
    PolicyBlocked,
    Unknown,
}

impl AvailabilityStateKind {
    pub const ALL: [Self; 9] = [
        Self::Available,
        Self::Degraded,
        Self::RateLimited,
        Self::SessionExhausted,
        Self::AuthRequired,
        Self::ProviderDown,
        Self::SafetyCheckPending,
        Self::PolicyBlocked,
        Self::Unknown,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "AVAILABLE",
            Self::Degraded => "DEGRADED",
            Self::RateLimited => "RATE_LIMITED",
            Self::SessionExhausted => "SESSION_EXHAUSTED",
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::ProviderDown => "PROVIDER_DOWN",
            Self::SafetyCheckPending => "SAFETY_CHECK_PENDING",
            Self::PolicyBlocked => "POLICY_BLOCKED",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilitySignalSource {
    RateLimitHeader,
    ExitCode,
    StderrClassification,
    LocalUsageView,
    LatencyTrend,
    Probe,
    Unknown,
}

impl AvailabilitySignalSource {
    pub const ALL: [Self; 7] = [
        Self::RateLimitHeader,
        Self::ExitCode,
        Self::StderrClassification,
        Self::LocalUsageView,
        Self::LatencyTrend,
        Self::Probe,
        Self::Unknown,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RateLimitHeader => "RATE_LIMIT_HEADER",
            Self::ExitCode => "EXIT_CODE",
            Self::StderrClassification => "STDERR_CLASSIFICATION",
            Self::LocalUsageView => "LOCAL_USAGE_VIEW",
            Self::LatencyTrend => "LATENCY_TREND",
            Self::Probe => "PROBE",
            Self::Unknown => "UNKNOWN",
        }
    }
}
