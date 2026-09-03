//! Closed product-entitlement vocabularies and validated string values.

mod product_entitlement_values;
mod vocabulary;

pub use product_entitlement_values::{
    ProductCapabilityId, ProductEntitlementKeyId, ProductEntitlementSignature,
    ProductEntitlementSubjectId, ProductEntitlementValueError, ProductTierId,
};

pub use vocabulary::{ActivationStateKind, ProductEntitlementState};

#[cfg(test)]
mod product_entitlement_values_tests;

#[cfg(test)]
mod vocabulary_tests;
