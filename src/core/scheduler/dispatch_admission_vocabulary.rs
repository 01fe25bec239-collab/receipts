//! Closed vocabularies embedded in the frozen `DispatchAdmissionDecision`
//! contract.
//!
//! This slice defines **only** the four closed value vocabularies below, each
//! with its exact canonical string representation. It deliberately contains:
//!
//! * no `DispatchAdmissionDecision` aggregate struct, constructor, builder, or
//!   validation;
//! * no axis-result record/object model;
//! * no admission composition, precedence, or consistency logic;
//! * no dispatch, routing, entitlement, safety, or scheduler behavior;
//! * no parsers, aliases, fallbacks, or free-form string handling;
//! * no `ProviderPolicyEligibility` (or any shadow provider-policy status
//!   vocabulary), which is owned by `BUILD-A2-MODEL-ROUTING`.
//!
//! Every `as_str` uses exhaustive matching with no wildcard arm, so adding a
//! future variant forces compiler-visible handling.

/// The closed dispatch-admission outcome vocabulary.
///
/// Exactly the two frozen values, in frozen schema order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DispatchAdmissionOutcome {
    Allow,
    Deny,
}

impl DispatchAdmissionOutcome {
    /// Every outcome in frozen schema order.
    pub const ALL: [Self; 2] = [Self::Allow, Self::Deny];

    /// The exact canonical schema string for this outcome.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Allow => "ALLOW",
            Self::Deny => "DENY",
        }
    }
}

/// The closed dispatch-admission failing-axis vocabulary.
///
/// Exactly the seven frozen values, in frozen schema order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DispatchAdmissionFailingAxis {
    None,
    Entitlement,
    ProviderAuth,
    ProviderPolicy,
    ProviderAvailability,
    Safety,
    QualityFloor,
}

impl DispatchAdmissionFailingAxis {
    /// Every failing axis in frozen schema order.
    pub const ALL: [Self; 7] = [
        Self::None,
        Self::Entitlement,
        Self::ProviderAuth,
        Self::ProviderPolicy,
        Self::ProviderAvailability,
        Self::Safety,
        Self::QualityFloor,
    ];

    /// The exact canonical schema string for this failing axis.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Entitlement => "ENTITLEMENT",
            Self::ProviderAuth => "PROVIDER_AUTH",
            Self::ProviderPolicy => "PROVIDER_POLICY",
            Self::ProviderAvailability => "PROVIDER_AVAILABILITY",
            Self::Safety => "SAFETY",
            Self::QualityFloor => "QUALITY_FLOOR",
        }
    }
}

/// The closed dispatch-admission denial-reason vocabulary.
///
/// Exactly the fourteen frozen values, in frozen schema order. There is no
/// standalone `Unknown` variant: the only values containing `UNKNOWN` are the
/// exact frozen denial reasons `ENTITLEMENT_UNKNOWN` and
/// `PROVIDER_POLICY_UNKNOWN`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DispatchAdmissionDenialReason {
    None,
    LockedRequiresPro,
    EntitlementUnknown,
    EntitlementExpired,
    AuthRequired,
    ProviderPolicyDisallowed,
    ProviderPolicyUnknown,
    ProviderRateLimited,
    ProviderDown,
    NoEligibleRuntime,
    SafetyCheckPending,
    PolicyBlocked,
    HumanRequired,
    QualityFloorUnsatisfied,
}

impl DispatchAdmissionDenialReason {
    /// Every denial reason in frozen schema order.
    pub const ALL: [Self; 14] = [
        Self::None,
        Self::LockedRequiresPro,
        Self::EntitlementUnknown,
        Self::EntitlementExpired,
        Self::AuthRequired,
        Self::ProviderPolicyDisallowed,
        Self::ProviderPolicyUnknown,
        Self::ProviderRateLimited,
        Self::ProviderDown,
        Self::NoEligibleRuntime,
        Self::SafetyCheckPending,
        Self::PolicyBlocked,
        Self::HumanRequired,
        Self::QualityFloorUnsatisfied,
    ];

    /// The exact canonical schema string for this denial reason.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::LockedRequiresPro => "LOCKED_REQUIRES_PRO",
            Self::EntitlementUnknown => "ENTITLEMENT_UNKNOWN",
            Self::EntitlementExpired => "ENTITLEMENT_EXPIRED",
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::ProviderPolicyDisallowed => "PROVIDER_POLICY_DISALLOWED",
            Self::ProviderPolicyUnknown => "PROVIDER_POLICY_UNKNOWN",
            Self::ProviderRateLimited => "PROVIDER_RATE_LIMITED",
            Self::ProviderDown => "PROVIDER_DOWN",
            Self::NoEligibleRuntime => "NO_ELIGIBLE_RUNTIME",
            Self::SafetyCheckPending => "SAFETY_CHECK_PENDING",
            Self::PolicyBlocked => "POLICY_BLOCKED",
            Self::HumanRequired => "HUMAN_REQUIRED",
            Self::QualityFloorUnsatisfied => "QUALITY_FLOOR_UNSATISFIED",
        }
    }
}

/// The closed dispatch-admission per-axis result vocabulary.
///
/// Exactly the three frozen values, in frozen schema order. This is the bare
/// `PASS` / `FAIL` / `NOT_APPLICABLE` result name only — not the composed
/// per-axis record objects (`entitlement`, `provider_auth`, and so on), which
/// are outside this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DispatchAdmissionAxisResult {
    Pass,
    Fail,
    NotApplicable,
}

impl DispatchAdmissionAxisResult {
    /// Every axis result in frozen schema order.
    pub const ALL: [Self; 3] = [Self::Pass, Self::Fail, Self::NotApplicable];

    /// The exact canonical schema string for this axis result.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
            Self::NotApplicable => "NOT_APPLICABLE",
        }
    }
}
