use super::{
    ActivationIdentityFields, ActivationStateKind, ProductEntitlementState,
    VerifiedProductEntitlement,
};

/// Resolver evidence requires a cryptographic proof for the verified case.
/// Temporal classification remains separately supplied authority.
///
/// ```compile_fail
/// use receipts_state::entitlement::{ObservedEntitlementEvidence, ProductEntitlementStringFields,
///     VerifiedEntitlementTemporalClass};
/// fn forge(raw: &ProductEntitlementStringFields) -> ObservedEntitlementEvidence<'_> {
///     ObservedEntitlementEvidence::Verified {
///         entitlement: raw,
///         temporal_class: VerifiedEntitlementTemporalClass::WithinActiveValidity,
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedEntitlementEvidence<'a> {
    /// No cached artifact is available.
    Missing,
    /// An existing artifact has already been classified unusable.
    Corrupt,
    /// Entitlement authority has already been classified as undetermined.
    Indeterminate,
    /// Upstream has verified this as the relevant paid authority, regardless of tier text.
    Verified {
        entitlement: &'a VerifiedProductEntitlement,
        temporal_class: VerifiedEntitlementTemporalClass,
    },
}

/// Temporal authority classified upstream under the signed entitlement policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedEntitlementTemporalClass {
    /// Still within the signed active validity.
    WithinActiveValidity,
    /// Grace is legitimately applicable and still within the signed offline grace bound.
    WithinApplicableSignedOfflineGrace,
    /// Beyond permitted active and grace authority.
    PastPermittedGrace,
}

/// Already-observed reachability, which grants no entitlement authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LicensingServiceAvailability {
    Available,
    Unavailable,
}

/// Already-classified clock evidence; rollback implies established prior server time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalClockEvidence {
    NoRollbackDetected,
    EarlierThanLastObservedServerTime,
}

/// Resolves classified evidence without I/O or inspecting entitlement string fields.
///
/// The caller supplies any required activation consistency gate, verification, and
/// temporal classification. Service reachability alone cannot change authority.
pub fn resolve_product_entitlement_state(
    activation: &ActivationIdentityFields,
    entitlement: ObservedEntitlementEvidence<'_>,
    service: LicensingServiceAvailability,
    clock: LocalClockEvidence,
) -> ProductEntitlementState {
    match clock {
        LocalClockEvidence::EarlierThanLastObservedServerTime => {
            return ProductEntitlementState::EntitlementUnknown;
        }
        LocalClockEvidence::NoRollbackDetected => {}
    }
    match service {
        LicensingServiceAvailability::Available | LicensingServiceAvailability::Unavailable => {}
    }
    match entitlement {
        ObservedEntitlementEvidence::Indeterminate => ProductEntitlementState::EntitlementUnknown,
        ObservedEntitlementEvidence::Verified {
            entitlement: _,
            temporal_class,
        } => match temporal_class {
            VerifiedEntitlementTemporalClass::WithinActiveValidity => {
                ProductEntitlementState::ProActive
            }
            VerifiedEntitlementTemporalClass::WithinApplicableSignedOfflineGrace => {
                ProductEntitlementState::ProGrace
            }
            VerifiedEntitlementTemporalClass::PastPermittedGrace => {
                ProductEntitlementState::ProExpired
            }
        },
        ObservedEntitlementEvidence::Missing | ObservedEntitlementEvidence::Corrupt => {
            match activation.activation_state() {
                ActivationStateKind::NeverActivated | ActivationStateKind::LoggedOut => {
                    ProductEntitlementState::Free
                }
                ActivationStateKind::ActivatedKnown => ProductEntitlementState::EntitlementUnknown,
            }
        }
    }
}
