//! Pure native-path prerequisite assessment.
//!
//! Answers only whether all prerequisites for the native embedded path are
//! currently proven satisfied. Inputs are caller-supplied facts; this module
//! performs no probing, observes nothing itself, selects no
//! EMBEDDED/HYBRID/SUPERVISED integration mode, and executes no freshness,
//! fingerprint, reprobe, serialization, or report logic.
//!
//! `None` means unknown or unproven for every nullable fact. A supplied value
//! records a fact only and grants no privilege, capability, or authority.
//!
//! Known failures dominate unrelated unknowns: any known mandatory `Some(false)`
//! yields [`HostCapabilityNativePrerequisiteState::Unsatisfied`] even when other
//! facts are `None`. [`HostCapabilityNativePrerequisiteState::Unknown`] is
//! returned only when no known failure exists and at least one required fact
//! remains unresolved. [`HostCapabilityNativePrerequisiteState::Satisfied`]
//! requires every required prerequisite to be proven healthy.
//!
//! Trust is conditional: when `hook_trust_required` is `Some(false)`,
//! `hooks_trusted` is irrelevant and any of its three states may still yield
//! `Satisfied`. When `hook_trust_required` is `Some(true)`, `hooks_trusted`
//! must be `Some(true)` for `Satisfied`. When `hook_trust_required` is `None`,
//! the trust model itself is unresolved, so the result is at best `Unknown`
//! absent another known failure.
//!
//! This result is an in-process behavioral-policy value only. It is not a
//! `HostCapabilityReport` field, has no wire representation, and carries no
//! serialized, canonical-string, or schema meaning.

/// Aggregate three-valued assessment of the native embedded path prerequisites.
///
/// `Satisfied` means every required native-path prerequisite is proven
/// healthy. It does not select any integration mode. `Unsatisfied` means at
/// least one mandatory prerequisite is known false. `Unknown` means no known
/// failure exists but at least one required fact remains unresolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostCapabilityNativePrerequisiteState {
    Satisfied,
    Unsatisfied,
    Unknown,
}

/// Caller-supplied nullable facts for the native-path prerequisite assessment.
///
/// `Some(true)` means proven true, `Some(false)` means proven false, and
/// `None` means unknown or unproven. This value is not a host capability
/// report: it performs no observation, probing, or freshness checking, and its
/// contents confer no privilege or authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostCapabilityNativePrerequisiteInputs {
    pub plugin_supported: Option<bool>,
    pub plugin_installed: Option<bool>,
    pub hooks_supported: Option<bool>,
    pub hooks_configured: Option<bool>,
    pub hook_trust_required: Option<bool>,
    pub hooks_trusted: Option<bool>,
    pub hooks_enabled: Option<bool>,
    pub hooks_allowed_by_admin_policy: Option<bool>,
    pub required_hook_coverage_satisfied: Option<bool>,
}

/// Purely assesses whether all native-path prerequisites are proven satisfied.
///
/// Inputs are caller-supplied facts; `None` means unknown or unproven. This
/// pure policy performs no probing, selects no EMBEDDED, HYBRID, or SUPERVISED
/// integration mode, touches no report, freshness, fingerprint, or
/// serialization state, and treats supplied values as granting no privilege or
/// authority.
///
/// A known mandatory `Some(false)` dominates all unrelated unknowns and yields
/// `Unsatisfied`. With no known failure, any unresolved required fact yields
/// `Unknown`. `Satisfied` requires all seven unconditional facts to be
/// `Some(true)` plus healthy trust: either trust explicitly not required, or
/// trust required and proven trusted.
pub const fn assess_native_path_prerequisites(
    inputs: HostCapabilityNativePrerequisiteInputs,
) -> HostCapabilityNativePrerequisiteState {
    if matches!(inputs.plugin_supported, Some(false))
        || matches!(inputs.plugin_installed, Some(false))
        || matches!(inputs.hooks_supported, Some(false))
        || matches!(inputs.hooks_configured, Some(false))
        || matches!(inputs.hooks_enabled, Some(false))
        || matches!(inputs.hooks_allowed_by_admin_policy, Some(false))
        || matches!(inputs.required_hook_coverage_satisfied, Some(false))
        || (matches!(inputs.hook_trust_required, Some(true))
            && matches!(inputs.hooks_trusted, Some(false)))
    {
        HostCapabilityNativePrerequisiteState::Unsatisfied
    } else if inputs.plugin_supported.is_none()
        || inputs.plugin_installed.is_none()
        || inputs.hooks_supported.is_none()
        || inputs.hooks_configured.is_none()
        || inputs.hooks_enabled.is_none()
        || inputs.hooks_allowed_by_admin_policy.is_none()
        || inputs.required_hook_coverage_satisfied.is_none()
        || inputs.hook_trust_required.is_none()
        || (matches!(inputs.hook_trust_required, Some(true)) && inputs.hooks_trusted.is_none())
    {
        HostCapabilityNativePrerequisiteState::Unknown
    } else {
        HostCapabilityNativePrerequisiteState::Satisfied
    }
}
