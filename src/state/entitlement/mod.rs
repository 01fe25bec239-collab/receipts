//! Closed product-entitlement vocabularies and validated string values.

mod activation_identity_consistency;
mod activation_identity_fields;
mod entitlement_state_resolution;
mod install_store;
mod install_store_migrations;
mod physical_contracts;
mod product_entitlement_string_fields;
mod product_entitlement_values;
mod verification;
mod vocabulary;
mod wire;

pub use install_store::{
    InstallCachedEntitlement, InstallCachedEntitlementFailure, InstallEntitlementIngestOutcome,
    InstallEntitlementRepository, InstallEntitlementSnapshot, InstallEntitlementStoreError,
};

pub use verification::{
    EntitlementCacheDecision, EntitlementVerificationError, EntitlementVerifier,
    VerifiedProductEntitlement,
};
pub use wire::MAX_ENTITLEMENT_WIRE_BYTES;

pub use activation_identity_consistency::{
    ActivationIdentityConsistencyError, validate_activation_identity_fields,
};
pub use activation_identity_fields::ActivationIdentityFields;
pub use entitlement_state_resolution::{
    LicensingServiceAvailability, LocalClockEvidence, ObservedEntitlementEvidence,
    VerifiedEntitlementTemporalClass, resolve_product_entitlement_state,
};
pub use physical_contracts::{ActivationState, ProductEntitlement, ProductEntitlementVersion};
pub use product_entitlement_string_fields::ProductEntitlementStringFields;

pub use product_entitlement_values::{
    ProductCapabilityId, ProductEntitlementKeyId, ProductEntitlementSignature,
    ProductEntitlementSubjectId, ProductEntitlementValueError, ProductTierId,
};

pub use vocabulary::{ActivationStateKind, ProductEntitlementState};

#[cfg(test)]
mod activation_identity_consistency_tests;

#[cfg(test)]
mod activation_identity_fields_tests;

#[cfg(test)]
mod entitlement_state_resolution_tests;

#[cfg(test)]
mod physical_contracts_tests;

#[cfg(test)]
mod product_entitlement_string_fields_tests;

#[cfg(test)]
mod product_entitlement_values_tests;

#[cfg(test)]
mod vocabulary_tests;

#[cfg(test)]
mod verification_vectors;

#[cfg(test)]
mod verification_tests;

#[cfg(test)]
mod install_store_tests;
