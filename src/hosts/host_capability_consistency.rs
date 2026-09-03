//! Pure validation of the four COMPLETE-probe schema consistency rules.

use crate::HostCapabilityProbeStatus;

/// Caller-supplied nullable facts needed by the COMPLETE-probe consistency rules.
///
/// This is not a capability report. Supplying these facts grants no capability or
/// authority, and validation performs no probing or mode selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostCapabilityConsistencyInputs {
    /// Status of the already-completed caller probe.
    pub probe_status: HostCapabilityProbeStatus,
    /// Whether the host supports plugins, or `None` when unknown.
    pub plugin_supported: Option<bool>,
    /// Whether the plugin is installed, or `None` when unknown.
    pub plugin_installed: Option<bool>,
    /// Whether the host supports hooks, or `None` when unknown.
    pub hooks_supported: Option<bool>,
    /// Whether hooks are configured, or `None` when unknown.
    pub hooks_configured: Option<bool>,
    /// Whether hooks are enabled, or `None` when unknown.
    pub hooks_enabled: Option<bool>,
    /// Whether hook trust is required, or `None` when unknown.
    pub hook_trust_required: Option<bool>,
    /// Whether hooks are trusted, or `None` when unknown.
    pub hooks_trusted: Option<bool>,
}

/// A violated COMPLETE-probe schema consistency rule.
///
/// These four errors describe schema inconsistencies only. They grant no
/// capability or authority and do not select an integration mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapabilityConsistencyError {
    /// `plugin_installed` is true but `plugin_supported` is not known true.
    PluginInstalledRequiresPluginSupported,
    /// `hooks_configured` is true but `hooks_supported` is not known true.
    HooksConfiguredRequiresHooksSupported,
    /// `hooks_enabled` is true but `hooks_supported` is not known true.
    HooksEnabledRequiresHooksSupported,
    /// Trust is required but `hooks_trusted` is unknown.
    ///
    /// Both known values, including `Some(false)`, satisfy this rule.
    TrustRequiredRequiresKnownTrustedState,
}

/// Validates exactly four schema constraints that apply to COMPLETE probes.
///
/// Inputs are caller-supplied. PARTIAL and FAILED probes are outside these four
/// rules and always pass. `Some(false)` for `hooks_trusted` is known and valid.
/// This pure validator performs no probing or mode selection and grants no
/// capability or authority.
pub const fn validate_complete_probe_consistency(
    inputs: HostCapabilityConsistencyInputs,
) -> Result<(), HostCapabilityConsistencyError> {
    if !matches!(inputs.probe_status, HostCapabilityProbeStatus::Complete) {
        return Ok(());
    }

    if matches!(inputs.plugin_installed, Some(true))
        && !matches!(inputs.plugin_supported, Some(true))
    {
        Err(HostCapabilityConsistencyError::PluginInstalledRequiresPluginSupported)
    } else if matches!(inputs.hooks_configured, Some(true))
        && !matches!(inputs.hooks_supported, Some(true))
    {
        Err(HostCapabilityConsistencyError::HooksConfiguredRequiresHooksSupported)
    } else if matches!(inputs.hooks_enabled, Some(true))
        && !matches!(inputs.hooks_supported, Some(true))
    {
        Err(HostCapabilityConsistencyError::HooksEnabledRequiresHooksSupported)
    } else if matches!(inputs.hook_trust_required, Some(true)) && inputs.hooks_trusted.is_none() {
        Err(HostCapabilityConsistencyError::TrustRequiredRequiresKnownTrustedState)
    } else {
        Ok(())
    }
}
