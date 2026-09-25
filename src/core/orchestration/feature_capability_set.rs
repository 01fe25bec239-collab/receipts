//! Passive, caller-supplied feature catalog; no admission or status inference.

use crate::graph::CapabilityName;

/// Exactly the frozen feature-catalog status vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FeatureCapabilityStatus {
    AvailableFree,
    AvailableEntitled,
    LockedRequiresPro,
    UnavailableProvider,
    UnavailablePolicy,
    UnavailableHost,
    UnavailableRuntime,
    BlockedSafety,
}

impl FeatureCapabilityStatus {
    pub const ALL: [Self; 8] = [
        Self::AvailableFree,
        Self::AvailableEntitled,
        Self::LockedRequiresPro,
        Self::UnavailableProvider,
        Self::UnavailablePolicy,
        Self::UnavailableHost,
        Self::UnavailableRuntime,
        Self::BlockedSafety,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AvailableFree => "AVAILABLE_FREE",
            Self::AvailableEntitled => "AVAILABLE_ENTITLED",
            Self::LockedRequiresPro => "LOCKED_REQUIRES_PRO",
            Self::UnavailableProvider => "UNAVAILABLE_PROVIDER",
            Self::UnavailablePolicy => "UNAVAILABLE_POLICY",
            Self::UnavailableHost => "UNAVAILABLE_HOST",
            Self::UnavailableRuntime => "UNAVAILABLE_RUNTIME",
            Self::BlockedSafety => "BLOCKED_SAFETY",
        }
    }
}

/// A required catalog value failed structural validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureCapabilitySetError {
    InvalidCatalogVersion,
    EmptyFeatures,
    EmptyTierRequired,
    EmptyDescription,
}

impl std::fmt::Display for FeatureCapabilitySetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidCatalogVersion => {
                "catalog_version must be a canonical positive ASCII decimal"
            }
            Self::EmptyFeatures => "features must be nonempty",
            Self::EmptyTierRequired => "tier_required must be nonempty",
            Self::EmptyDescription => "description must be nonempty",
        })
    }
}

impl std::error::Error for FeatureCapabilitySetError {}

/// One feature with a canonical capability ID and exact caller-supplied data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureCapability {
    capability_id: CapabilityName,
    name: String,
    tier_required: String,
    description: String,
    status: Option<FeatureCapabilityStatus>,
    upgrade_hint: Option<String>,
}

impl FeatureCapability {
    pub fn try_new(
        capability_id: CapabilityName,
        name: impl Into<String>,
        tier_required: impl Into<String>,
        description: impl Into<String>,
        status: Option<FeatureCapabilityStatus>,
        upgrade_hint: Option<String>,
    ) -> Result<Self, FeatureCapabilitySetError> {
        let tier_required = tier_required.into();
        let description = description.into();
        if tier_required.is_empty() {
            return Err(FeatureCapabilitySetError::EmptyTierRequired);
        }
        if description.is_empty() {
            return Err(FeatureCapabilitySetError::EmptyDescription);
        }
        Ok(Self {
            capability_id,
            name: name.into(),
            tier_required,
            description,
            status,
            upgrade_hint,
        })
    }

    pub fn capability_id(&self) -> &CapabilityName {
        &self.capability_id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn tier_required(&self) -> &str {
        &self.tier_required
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn status(&self) -> Option<FeatureCapabilityStatus> {
        self.status
    }

    pub fn upgrade_hint(&self) -> Option<&str> {
        self.upgrade_hint.as_deref()
    }
}

/// An ordered, nonempty catalog. Duplicate feature entries are retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeatureCapabilitySet {
    catalog_version: String,
    features: Vec<FeatureCapability>,
}

impl FeatureCapabilitySet {
    pub fn try_new(
        catalog_version: impl Into<String>,
        features: Vec<FeatureCapability>,
    ) -> Result<Self, FeatureCapabilitySetError> {
        let catalog_version = catalog_version.into();
        if !matches!(catalog_version.as_bytes().first(), Some(b'1'..=b'9'))
            || !catalog_version.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(FeatureCapabilitySetError::InvalidCatalogVersion);
        }
        if features.is_empty() {
            return Err(FeatureCapabilitySetError::EmptyFeatures);
        }
        Ok(Self {
            catalog_version,
            features,
        })
    }

    pub fn catalog_version(&self) -> &str {
        &self.catalog_version
    }

    pub fn features(&self) -> &[FeatureCapability] {
        &self.features
    }
}
