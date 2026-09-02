//! Closed vocabularies from the single
//! `HOST_CAPABILITY_FRESHNESS_AUTHORITY.json` authority.
//!
//! These values are descriptive only. They perform no probing or mode
//! selection and grant no capability, trust, privilege, or authority.

/// Inputs whose current values determine host capability report validity.
///
/// Canonical strings mirror `HOST_CAPABILITY_FRESHNESS_AUTHORITY.json`.
/// This type performs no probing or mode selection; its variants describe
/// inputs and are not authorization decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilityValidityInput {
    HostPluginIntegrationCapability,
    PluginHookDefinitionIdentity,
    ExplicitHookTrustState,
    HookEnablement,
    AdminPolicy,
    RequiredLifecycleEventCoverage,
    RelevantConfiguration,
}

impl HostCapabilityValidityInput {
    /// Every validity input in machine-authority order.
    pub const ALL: [Self; 7] = [
        Self::HostPluginIntegrationCapability,
        Self::PluginHookDefinitionIdentity,
        Self::ExplicitHookTrustState,
        Self::HookEnablement,
        Self::AdminPolicy,
        Self::RequiredLifecycleEventCoverage,
        Self::RelevantConfiguration,
    ];

    /// Returns the exact machine-authority string for this input.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HostPluginIntegrationCapability => "host_plugin_integration_capability",
            Self::PluginHookDefinitionIdentity => "plugin_hook_definition_identity",
            Self::ExplicitHookTrustState => "explicit_hook_trust_state",
            Self::HookEnablement => "hook_enablement",
            Self::AdminPolicy => "admin_policy",
            Self::RequiredLifecycleEventCoverage => "required_lifecycle_event_coverage",
            Self::RelevantConfiguration => "relevant_configuration",
        }
    }
}

/// Events that require host capability reprobe consideration.
///
/// Canonical strings mirror `HOST_CAPABILITY_FRESHNESS_AUTHORITY.json`.
/// This type performs no probing or mode selection; its variants describe
/// events and are not authorization decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilityReprobeTrigger {
    InitialInstall,
    PluginInstallOrRemoval,
    PluginUpdate,
    HookDefinitionChange,
    HostVersionChange,
    TrustStateChange,
    HookEnablementOrConfigChange,
    AdminPolicyChange,
    SessionStartOrResume,
    UnprovenValidityBeforeEmbedded,
}

impl HostCapabilityReprobeTrigger {
    /// Every reprobe trigger in machine-authority order.
    pub const ALL: [Self; 10] = [
        Self::InitialInstall,
        Self::PluginInstallOrRemoval,
        Self::PluginUpdate,
        Self::HookDefinitionChange,
        Self::HostVersionChange,
        Self::TrustStateChange,
        Self::HookEnablementOrConfigChange,
        Self::AdminPolicyChange,
        Self::SessionStartOrResume,
        Self::UnprovenValidityBeforeEmbedded,
    ];

    /// Returns the exact machine-authority string for this trigger.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InitialInstall => "INITIAL_INSTALL",
            Self::PluginInstallOrRemoval => "PLUGIN_INSTALL_OR_REMOVAL",
            Self::PluginUpdate => "PLUGIN_UPDATE",
            Self::HookDefinitionChange => "HOOK_DEFINITION_CHANGE",
            Self::HostVersionChange => "HOST_VERSION_CHANGE",
            Self::TrustStateChange => "TRUST_STATE_CHANGE",
            Self::HookEnablementOrConfigChange => "HOOK_ENABLEMENT_OR_CONFIG_CHANGE",
            Self::AdminPolicyChange => "ADMIN_POLICY_CHANGE",
            Self::SessionStartOrResume => "SESSION_START_OR_RESUME",
            Self::UnprovenValidityBeforeEmbedded => "UNPROVEN_VALIDITY_BEFORE_EMBEDDED",
        }
    }
}
