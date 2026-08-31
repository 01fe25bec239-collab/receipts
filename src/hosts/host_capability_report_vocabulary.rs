//! Closed vocabularies used by a future `HostCapabilityReport`.
//!
//! This module defines words only: no report, parsing, probing, freshness,
//! mode selection, override policy, or host-specific behavior.

/// Outcome of a host capability probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilityProbeStatus {
    Complete,
    Partial,
    Failed,
}

impl HostCapabilityProbeStatus {
    pub const ALL: [Self; 3] = [Self::Complete, Self::Partial, Self::Failed];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "COMPLETE",
            Self::Partial => "PARTIAL",
            Self::Failed => "FAILED",
        }
    }
}

/// Why a host capability report is stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilityStaleReason {
    None,
    ValidityFingerprintChanged,
    ValidityFingerprintUnproven,
    ProbeFailed,
}

impl HostCapabilityStaleReason {
    pub const ALL: [Self; 4] = [
        Self::None,
        Self::ValidityFingerprintChanged,
        Self::ValidityFingerprintUnproven,
        Self::ProbeFailed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::ValidityFingerprintChanged => "VALIDITY_FINGERPRINT_CHANGED",
            Self::ValidityFingerprintUnproven => "VALIDITY_FINGERPRINT_UNPROVEN",
            Self::ProbeFailed => "PROBE_FAILED",
        }
    }
}

/// Coverage of the host's hook surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilityHookCoverageClass {
    Full,
    Partial,
    None,
    Unknown,
}

impl HostCapabilityHookCoverageClass {
    pub const ALL: [Self; 4] = [Self::Full, Self::Partial, Self::None, Self::Unknown];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "FULL",
            Self::Partial => "PARTIAL",
            Self::None => "NONE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Integration mode recorded by a host capability report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilitySelectedMode {
    Embedded,
    Hybrid,
    Supervised,
}

impl HostCapabilitySelectedMode {
    pub const ALL: [Self; 3] = [Self::Embedded, Self::Hybrid, Self::Supervised];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Embedded => "EMBEDDED",
            Self::Hybrid => "HYBRID",
            Self::Supervised => "SUPERVISED",
        }
    }
}

/// Source of an explicit integration-mode override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilityModeOverrideSource {
    User,
    Admin,
    Debug,
}

impl HostCapabilityModeOverrideSource {
    pub const ALL: [Self; 3] = [Self::User, Self::Admin, Self::Debug];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "USER",
            Self::Admin => "ADMIN",
            Self::Debug => "DEBUG",
        }
    }
}

/// Why the native host path is inactive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilityInactiveReason {
    None,
    PluginNotInstalled,
    HooksUnsupported,
    HooksNotConfigured,
    HooksUntrusted,
    HooksDisabled,
    HooksExcludedByAdminPolicy,
    InsufficientCoverage,
    ModeOverride,
    Unknown,
}

impl HostCapabilityInactiveReason {
    pub const ALL: [Self; 10] = [
        Self::None,
        Self::PluginNotInstalled,
        Self::HooksUnsupported,
        Self::HooksNotConfigured,
        Self::HooksUntrusted,
        Self::HooksDisabled,
        Self::HooksExcludedByAdminPolicy,
        Self::InsufficientCoverage,
        Self::ModeOverride,
        Self::Unknown,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::PluginNotInstalled => "PLUGIN_NOT_INSTALLED",
            Self::HooksUnsupported => "HOOKS_UNSUPPORTED",
            Self::HooksNotConfigured => "HOOKS_NOT_CONFIGURED",
            Self::HooksUntrusted => "HOOKS_UNTRUSTED",
            Self::HooksDisabled => "HOOKS_DISABLED",
            Self::HooksExcludedByAdminPolicy => "HOOKS_EXCLUDED_BY_ADMIN_POLICY",
            Self::InsufficientCoverage => "INSUFFICIENT_COVERAGE",
            Self::ModeOverride => "MODE_OVERRIDE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Provenance label for host capability evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilityEvidenceLabel {
    VerifiedCurrentSelfFetched,
    ReviewerSuppliedCurrentPrimarySource,
    VerifiedHistorical,
    IndependentVerified,
    UserDeclared,
    DesignDecision,
    Assumption,
    Unverified,
    PolicyNeedsReview,
}

impl HostCapabilityEvidenceLabel {
    pub const ALL: [Self; 9] = [
        Self::VerifiedCurrentSelfFetched,
        Self::ReviewerSuppliedCurrentPrimarySource,
        Self::VerifiedHistorical,
        Self::IndependentVerified,
        Self::UserDeclared,
        Self::DesignDecision,
        Self::Assumption,
        Self::Unverified,
        Self::PolicyNeedsReview,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::VerifiedCurrentSelfFetched => "VERIFIED_CURRENT_SELF_FETCHED",
            Self::ReviewerSuppliedCurrentPrimarySource => {
                "REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE"
            }
            Self::VerifiedHistorical => "VERIFIED_HISTORICAL",
            Self::IndependentVerified => "INDEPENDENT_VERIFIED",
            Self::UserDeclared => "USER_DECLARED",
            Self::DesignDecision => "DESIGN_DECISION",
            Self::Assumption => "ASSUMPTION",
            Self::Unverified => "UNVERIFIED",
            Self::PolicyNeedsReview => "POLICY_NEEDS_REVIEW",
        }
    }
}
