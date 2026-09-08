use crate::CanonicalTimestampV1;

use super::{
    ActivationIdentityConsistencyError, ActivationIdentityFields, ActivationStateKind,
    ProductCapabilityId, ProductEntitlementKeyId, ProductEntitlementSignature,
    ProductEntitlementStringFields, ProductEntitlementSubjectId, ProductTierId,
    validate_activation_identity_fields,
};

/// Monotonically increasing entitlement version supported by the V1 State model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProductEntitlementVersion(i64);

impl ProductEntitlementVersion {
    /// Accepts the V1 in-process range `1..=i64::MAX`.
    pub fn new(value: i64) -> Option<Self> {
        (value >= 1).then_some(Self(value))
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

/// In-process physical representation of the frozen product entitlement fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductEntitlement {
    string_fields: ProductEntitlementStringFields,
    issued_at: CanonicalTimestampV1,
    expires_at: CanonicalTimestampV1,
    entitlement_version: ProductEntitlementVersion,
    offline_grace_until: Option<CanonicalTimestampV1>,
}

impl ProductEntitlement {
    pub fn new(
        string_fields: ProductEntitlementStringFields,
        issued_at: CanonicalTimestampV1,
        expires_at: CanonicalTimestampV1,
        entitlement_version: ProductEntitlementVersion,
        offline_grace_until: Option<CanonicalTimestampV1>,
    ) -> Self {
        Self {
            string_fields,
            issued_at,
            expires_at,
            entitlement_version,
            offline_grace_until,
        }
    }

    pub fn subject_id(&self) -> &ProductEntitlementSubjectId {
        self.string_fields.subject_id()
    }

    pub fn tier_id(&self) -> &ProductTierId {
        self.string_fields.tier_id()
    }

    pub fn capabilities(&self) -> &[ProductCapabilityId] {
        self.string_fields.capabilities()
    }

    pub fn issued_at(&self) -> &CanonicalTimestampV1 {
        &self.issued_at
    }

    pub fn expires_at(&self) -> &CanonicalTimestampV1 {
        &self.expires_at
    }

    pub fn entitlement_version(&self) -> ProductEntitlementVersion {
        self.entitlement_version
    }

    pub fn key_id(&self) -> &ProductEntitlementKeyId {
        self.string_fields.key_id()
    }

    pub fn signature(&self) -> &ProductEntitlementSignature {
        self.string_fields.signature()
    }

    pub fn offline_grace_until(&self) -> Option<&CanonicalTimestampV1> {
        self.offline_grace_until.as_ref()
    }

    pub fn device_binding(&self) -> Option<&str> {
        self.string_fields.device_binding()
    }
}

/// In-process physical representation of the frozen activation state fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivationState {
    identity_fields: ActivationIdentityFields,
    first_activated_at: Option<CanonicalTimestampV1>,
    last_entitlement_seen_at: Option<CanonicalTimestampV1>,
    last_observed_server_time: Option<CanonicalTimestampV1>,
    logged_out_at: Option<CanonicalTimestampV1>,
    recorded_at: CanonicalTimestampV1,
}

impl ActivationState {
    pub fn new(
        identity_fields: ActivationIdentityFields,
        first_activated_at: Option<CanonicalTimestampV1>,
        last_entitlement_seen_at: Option<CanonicalTimestampV1>,
        last_observed_server_time: Option<CanonicalTimestampV1>,
        logged_out_at: Option<CanonicalTimestampV1>,
        recorded_at: CanonicalTimestampV1,
    ) -> Result<Self, ActivationIdentityConsistencyError> {
        validate_activation_identity_fields(&identity_fields)?;
        Ok(Self {
            identity_fields,
            first_activated_at,
            last_entitlement_seen_at,
            last_observed_server_time,
            logged_out_at,
            recorded_at,
        })
    }

    pub fn activation_state(&self) -> ActivationStateKind {
        self.identity_fields.activation_state()
    }

    pub fn subject_id(&self) -> Option<&str> {
        self.identity_fields.subject_id()
    }

    pub fn first_activated_at(&self) -> Option<&CanonicalTimestampV1> {
        self.first_activated_at.as_ref()
    }

    pub fn last_known_tier_id(&self) -> Option<&str> {
        self.identity_fields.last_known_tier_id()
    }

    pub fn last_entitlement_seen_at(&self) -> Option<&CanonicalTimestampV1> {
        self.last_entitlement_seen_at.as_ref()
    }

    pub fn last_observed_server_time(&self) -> Option<&CanonicalTimestampV1> {
        self.last_observed_server_time.as_ref()
    }

    pub fn logged_out_at(&self) -> Option<&CanonicalTimestampV1> {
        self.logged_out_at.as_ref()
    }

    pub fn recorded_at(&self) -> &CanonicalTimestampV1 {
        &self.recorded_at
    }
}
