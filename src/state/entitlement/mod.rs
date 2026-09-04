//! Closed product-entitlement vocabularies and validated string values.

mod activation_identity_fields;
mod product_entitlement_string_fields;
mod product_entitlement_values;
mod vocabulary;

pub use activation_identity_fields::ActivationIdentityFields;
pub use product_entitlement_string_fields::ProductEntitlementStringFields;

pub use product_entitlement_values::{
    ProductCapabilityId, ProductEntitlementKeyId, ProductEntitlementSignature,
    ProductEntitlementSubjectId, ProductEntitlementValueError, ProductTierId,
};

pub use vocabulary::{ActivationStateKind, ProductEntitlementState};

#[cfg(test)]
mod activation_identity_fields_tests;

#[cfg(test)]
mod product_entitlement_string_fields_tests;

#[cfg(test)]
mod product_entitlement_values_tests;

#[cfg(test)]
mod vocabulary_tests;
