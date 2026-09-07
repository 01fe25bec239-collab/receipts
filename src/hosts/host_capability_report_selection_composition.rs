//! Pure composition of accepted mode selection and non-temporal report construction.
//!
//! Only caller-supplied facts are consumed. No observation, freshness proof
//! acquisition, reprobe, persistence, serialization, or authority is introduced.

use crate::{
    HostCapabilityEvidenceLabel, HostCapabilityHookCoverageClass, HostCapabilityInactiveReason,
    HostCapabilityModeOverride, HostCapabilityModeSelectionError,
    HostCapabilityModeSelectionIndeterminacy, HostCapabilityModeSelectionInputs,
    HostCapabilityModeSelectionOutcome, HostCapabilityNativePrerequisiteInputs,
    HostCapabilityProbeStatus, HostCapabilityReportNonTemporalCore,
    HostCapabilityReportNonTemporalCoreError, HostCapabilityReportNonTemporalCoreInputs,
    HostCapabilityStaleReason, select_host_capability_mode,
};

/// All non-derived report facts, plus caller-supplied freshness proof state.
/// Optional values, opaque strings, and arrays retain their exact meaning and contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilityReportSelectionCompositionInputs {
    pub report_validity_proven_current: bool,
    pub host_id: String,
    pub host_version: Option<String>,
    pub probe_status: HostCapabilityProbeStatus,
    pub validity_fingerprint: Option<String>,
    pub hook_definition_digest: Option<String>,
    pub relevant_config_digest: Option<String>,
    pub stale_reason: HostCapabilityStaleReason,
    pub plugin_supported: Option<bool>,
    pub plugin_installed: Option<bool>,
    pub manifest_path: Option<String>,
    pub supports_skills: Option<bool>,
    pub supports_commands: Option<bool>,
    pub supports_subagents: Option<bool>,
    pub supports_mcp: Option<bool>,
    pub hooks_supported: Option<bool>,
    pub hooks_configured: Option<bool>,
    pub hook_trust_required: Option<bool>,
    pub hooks_trusted: Option<bool>,
    pub hooks_enabled: Option<bool>,
    pub hooks_allowed_by_admin_policy: Option<bool>,
    pub hook_events: Option<Vec<String>>,
    pub blocking_hook_events: Option<Vec<String>>,
    pub hook_coverage_class: HostCapabilityHookCoverageClass,
    pub required_hook_coverage_satisfied: Option<bool>,
    pub mode_override: Option<HostCapabilityModeOverride>,
    pub plugin_data_path: Option<String>,
    pub sandbox_modes: Option<Vec<String>>,
    pub evidence_label: Option<HostCapabilityEvidenceLabel>,
    pub source_claim_id: Option<String>,
}

/// Only completed selection can carry a report; unresolved outcomes carry no mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCapabilityReportSelectionCompositionOutcome {
    Selected {
        report: Box<HostCapabilityReportNonTemporalCore>,
    },
    ReprobeRequired,
    Indeterminate {
        cause: HostCapabilityModeSelectionIndeterminacy,
        inactive_reason: HostCapabilityInactiveReason,
    },
}

/// Existing errors, preserved without adding policy or consistency rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapabilityReportSelectionCompositionError {
    ModeSelection(HostCapabilityModeSelectionError),
    ReportConstruction(HostCapabilityReportNonTemporalCoreError),
}

/// Delegates policy to the accepted selector, then validates and stores its result.
/// Inputs are borrowed unchanged; unresolved selection never constructs a report.
pub fn compose_host_capability_report_selection(
    inputs: &HostCapabilityReportSelectionCompositionInputs,
) -> Result<
    HostCapabilityReportSelectionCompositionOutcome,
    HostCapabilityReportSelectionCompositionError,
> {
    use HostCapabilityReportSelectionCompositionError as Error;
    use HostCapabilityReportSelectionCompositionOutcome as Outcome;

    let selection = select_host_capability_mode(&HostCapabilityModeSelectionInputs {
        report_validity_proven_current: inputs.report_validity_proven_current,
        probe_status: inputs.probe_status,
        stale_reason: inputs.stale_reason,
        native_prerequisites: HostCapabilityNativePrerequisiteInputs {
            plugin_supported: inputs.plugin_supported,
            plugin_installed: inputs.plugin_installed,
            hooks_supported: inputs.hooks_supported,
            hooks_configured: inputs.hooks_configured,
            hook_trust_required: inputs.hook_trust_required,
            hooks_trusted: inputs.hooks_trusted,
            hooks_enabled: inputs.hooks_enabled,
            hooks_allowed_by_admin_policy: inputs.hooks_allowed_by_admin_policy,
            required_hook_coverage_satisfied: inputs.required_hook_coverage_satisfied,
        },
        hook_coverage_class: inputs.hook_coverage_class,
        mode_override: inputs.mode_override.clone(),
    })
    .map_err(Error::ModeSelection)?;

    match selection {
        HostCapabilityModeSelectionOutcome::Selected {
            selected_mode,
            inactive_reason,
        } => {
            let report = HostCapabilityReportNonTemporalCore::new(
                HostCapabilityReportNonTemporalCoreInputs {
                    host_id: inputs.host_id.clone(),
                    host_version: inputs.host_version.clone(),
                    probe_status: inputs.probe_status,
                    validity_fingerprint: inputs.validity_fingerprint.clone(),
                    hook_definition_digest: inputs.hook_definition_digest.clone(),
                    relevant_config_digest: inputs.relevant_config_digest.clone(),
                    stale_reason: inputs.stale_reason,
                    plugin_supported: inputs.plugin_supported,
                    plugin_installed: inputs.plugin_installed,
                    manifest_path: inputs.manifest_path.clone(),
                    supports_skills: inputs.supports_skills,
                    supports_commands: inputs.supports_commands,
                    supports_subagents: inputs.supports_subagents,
                    supports_mcp: inputs.supports_mcp,
                    hooks_supported: inputs.hooks_supported,
                    hooks_configured: inputs.hooks_configured,
                    hook_trust_required: inputs.hook_trust_required,
                    hooks_trusted: inputs.hooks_trusted,
                    hooks_enabled: inputs.hooks_enabled,
                    hooks_allowed_by_admin_policy: inputs.hooks_allowed_by_admin_policy,
                    hook_events: inputs.hook_events.clone(),
                    blocking_hook_events: inputs.blocking_hook_events.clone(),
                    hook_coverage_class: inputs.hook_coverage_class,
                    required_hook_coverage_satisfied: inputs.required_hook_coverage_satisfied,
                    mode_override: inputs.mode_override.clone(),
                    plugin_data_path: inputs.plugin_data_path.clone(),
                    sandbox_modes: inputs.sandbox_modes.clone(),
                    evidence_label: inputs.evidence_label,
                    source_claim_id: inputs.source_claim_id.clone(),
                    selected_mode,
                    inactive_reason: Some(inactive_reason),
                },
            )
            .map_err(Error::ReportConstruction)?;
            Ok(Outcome::Selected {
                report: Box::new(report),
            })
        }
        HostCapabilityModeSelectionOutcome::ReprobeRequired => Ok(Outcome::ReprobeRequired),
        HostCapabilityModeSelectionOutcome::Indeterminate {
            cause,
            inactive_reason,
        } => Ok(Outcome::Indeterminate {
            cause,
            inactive_reason,
        }),
    }
}
