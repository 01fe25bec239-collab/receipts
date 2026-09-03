//! Pure native-path inactive-reason precedence policy.

use crate::HostCapabilityInactiveReason;

/// Caller-supplied facts used to explain why the native host path is inactive.
///
/// `None` means unknown or unproven for every nullable fact. This value is not
/// a host capability report: it performs no probing and its contents confer no
/// privilege or authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostCapabilityInactiveReasonInputs {
    pub plugin_installed: Option<bool>,
    pub hooks_supported: Option<bool>,
    pub hooks_configured: Option<bool>,
    pub hook_trust_required: Option<bool>,
    pub hooks_trusted: Option<bool>,
    pub hooks_enabled: Option<bool>,
    pub hooks_allowed_by_admin_policy: Option<bool>,
    pub required_hook_coverage_satisfied: Option<bool>,
    pub mode_override_present: bool,
}

/// Selects only the explanatory reason why the native host path is inactive.
///
/// Inputs are caller-supplied facts; `None` means unknown or unproven. This
/// pure policy performs no probing, selects no EMBEDDED, HYBRID, or SUPERVISED
/// integration mode, and treats supplied values as granting no privilege or
/// authority.
pub const fn native_path_inactive_reason(
    inputs: HostCapabilityInactiveReasonInputs,
) -> HostCapabilityInactiveReason {
    if matches!(inputs.plugin_installed, Some(false)) {
        HostCapabilityInactiveReason::PluginNotInstalled
    } else if matches!(inputs.hooks_supported, Some(false)) {
        HostCapabilityInactiveReason::HooksUnsupported
    } else if matches!(inputs.hooks_configured, Some(false)) {
        HostCapabilityInactiveReason::HooksNotConfigured
    } else if matches!(inputs.hook_trust_required, Some(true))
        && matches!(inputs.hooks_trusted, Some(false))
    {
        HostCapabilityInactiveReason::HooksUntrusted
    } else if matches!(inputs.hooks_enabled, Some(false)) {
        HostCapabilityInactiveReason::HooksDisabled
    } else if matches!(inputs.hooks_allowed_by_admin_policy, Some(false)) {
        HostCapabilityInactiveReason::HooksExcludedByAdminPolicy
    } else if matches!(inputs.required_hook_coverage_satisfied, Some(false)) {
        HostCapabilityInactiveReason::InsufficientCoverage
    } else if inputs.mode_override_present {
        HostCapabilityInactiveReason::ModeOverride
    } else if matches!(inputs.plugin_installed, Some(true))
        && matches!(inputs.hooks_supported, Some(true))
        && matches!(inputs.hooks_configured, Some(true))
        && (matches!(inputs.hook_trust_required, Some(false))
            || (matches!(inputs.hook_trust_required, Some(true))
                && matches!(inputs.hooks_trusted, Some(true))))
        && matches!(inputs.hooks_enabled, Some(true))
        && matches!(inputs.hooks_allowed_by_admin_policy, Some(true))
        && matches!(inputs.required_hook_coverage_satisfied, Some(true))
    {
        HostCapabilityInactiveReason::None
    } else {
        HostCapabilityInactiveReason::Unknown
    }
}
