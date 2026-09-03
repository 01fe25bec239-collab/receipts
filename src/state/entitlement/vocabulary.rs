/// Product activation provenance state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationStateKind {
    NeverActivated,
    ActivatedKnown,
    LoggedOut,
}

impl ActivationStateKind {
    /// Every activation state in architecture order.
    pub const ALL: [Self; 3] = [Self::NeverActivated, Self::ActivatedKnown, Self::LoggedOut];

    /// The canonical architecture representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeverActivated => "NEVER_ACTIVATED",
            Self::ActivatedKnown => "ACTIVATED_KNOWN",
            Self::LoggedOut => "LOGGED_OUT",
        }
    }
}

/// Effective product entitlement state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductEntitlementState {
    Free,
    ProActive,
    ProGrace,
    ProExpired,
    EntitlementUnknown,
}

impl ProductEntitlementState {
    /// Every effective entitlement state in architecture order.
    pub const ALL: [Self; 5] = [
        Self::Free,
        Self::ProActive,
        Self::ProGrace,
        Self::ProExpired,
        Self::EntitlementUnknown,
    ];

    /// The canonical architecture representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "FREE",
            Self::ProActive => "PRO_ACTIVE",
            Self::ProGrace => "PRO_GRACE",
            Self::ProExpired => "PRO_EXPIRED",
            Self::EntitlementUnknown => "ENTITLEMENT_UNKNOWN",
        }
    }
}
