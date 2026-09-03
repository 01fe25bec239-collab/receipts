//! Partial composition of product entitlement string fields.
//!
//! This module composes the already accepted string value objects and the
//! opaque device binding storage value into one small struct. All fields
//! stay private. Callers receive only read-only views. The struct performs
//! no further checks and defines no additional behavior.

use super::product_entitlement_values::{
    ProductCapabilityId, ProductEntitlementKeyId, ProductEntitlementSignature,
    ProductEntitlementSubjectId, ProductTierId,
};

/// String-field slice of a product entitlement.
///
/// This is only the string/capability portion. It is not a complete
/// entitlement token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductEntitlementStringFields {
    subject_id: ProductEntitlementSubjectId,
    tier_id: ProductTierId,
    capabilities: Vec<ProductCapabilityId>,
    key_id: ProductEntitlementKeyId,
    signature: ProductEntitlementSignature,
    device_binding: Option<String>,
}

impl ProductEntitlementStringFields {
    /// Builds the composition from already accepted values.
    pub fn new(
        subject_id: ProductEntitlementSubjectId,
        tier_id: ProductTierId,
        capabilities: Vec<ProductCapabilityId>,
        key_id: ProductEntitlementKeyId,
        signature: ProductEntitlementSignature,
        device_binding: Option<String>,
    ) -> Self {
        Self {
            subject_id,
            tier_id,
            capabilities,
            key_id,
            signature,
            device_binding,
        }
    }

    /// Returns the stored subject value.
    pub fn subject_id(&self) -> &ProductEntitlementSubjectId {
        &self.subject_id
    }

    /// Returns the stored tier value.
    pub fn tier_id(&self) -> &ProductTierId {
        &self.tier_id
    }

    /// Returns the stored capability list in stored order.
    pub fn capabilities(&self) -> &[ProductCapabilityId] {
        &self.capabilities
    }

    /// Returns the stored key value.
    pub fn key_id(&self) -> &ProductEntitlementKeyId {
        &self.key_id
    }

    /// Returns the stored signature value.
    pub fn signature(&self) -> &ProductEntitlementSignature {
        &self.signature
    }

    /// Returns the stored device binding value.
    pub fn device_binding(&self) -> Option<&str> {
        self.device_binding.as_deref()
    }
}
