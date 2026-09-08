//! Bounded read-only observations. Source facts are not effective policy.
//!
//! Authority: BUILD-A1-ADR-HOST-A3-020-BOUNDED-STRUCTURED-PARSING-SUBSTRATE-001,
//! sources C1–C3 and X1–X3. No fingerprint representation is defined here.
//! File settings cannot establish higher managed tiers, project trust, or
//! Receipts hook ownership. Consequently they never authorize native hooks.

use crate::{
    HostCapabilityHookCoverageClass, HostCapabilityModeOverride, HostCapabilityProbeStatus,
    HostCapabilityReportSelectionCompositionInputs, HostCapabilityStaleReason, HostId,
};
use std::path::PathBuf;

#[path = "host_capability_observation_decode.rs"]
mod decode;
#[path = "host_capability_observation_io.rs"]
mod io;
#[cfg(test)]
#[path = "host_capability_observation_tests.rs"]
mod tests;

pub const HOST_STRUCTURED_OBSERVATION_MAX_BYTES: usize = 1_048_576;
pub const HOST_PROBE_STDOUT_MAX_BYTES: usize = 1_048_576;
pub const HOST_PROBE_STDERR_MAX_BYTES: usize = 1_048_576;
pub const HOST_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
pub const HOST_MANAGED_DROP_IN_MAX_FILES: usize = 32;

/// Absence of a source field is evidence about that source, never false.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum HostCapabilityObservationValue<T> {
    #[default]
    Unknown,
    Absent,
    Known(T),
}
use HostCapabilityObservationValue as Value;
impl<T: Clone> Value<T> {
    fn policy_value(&self) -> Option<T> {
        match self {
            Self::Known(value) => Some(value.clone()),
            _ => None,
        }
    }
}

/// Payload-free errors: vendor diagnostics and parser excerpts are never exposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapabilityObservationError {
    Missing,
    PermissionDenied,
    Unavailable,
    Unsupported,
    NotRegularFile,
    StructuredObservationTooLarge,
    TooManyFiles,
    InvalidUtf8,
    InvalidJson,
    InvalidToml,
    ProbeFailed,
    ProbeTimeout,
    ProbeStdoutTooLarge,
    ProbeStderrTooLarge,
    ProbeCleanupFailed,
}
use HostCapabilityObservationError as Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapabilityObservationSource {
    ClaudePluginList,
    ClaudeUserSettings,
    ClaudeProjectSettings,
    ClaudeLocalSettings,
    ClaudeManagedSettings,
    ClaudeManagedDropIn,
    CodexUserHooks,
    CodexProjectHooks,
    CodexUserConfig,
    CodexProjectConfig,
    CodexLocalRequirements,
}
use HostCapabilityObservationSource as Source;

/// Only the documented plugin-list fields needed for Receipts are retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostObservedPlugin {
    pub id: String,
    pub version: Value<String>,
    pub enabled: Value<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostObservedPluginSetting {
    pub id: String,
    pub enabled: Value<bool>,
}

/// Values in ONE configuration source, not merged/effective Host settings.
/// Hook names describe recognized configured events; no handler bodies survive.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HostObservedConfiguration {
    pub disable_all_hooks: Value<bool>,
    pub hooks_feature: Value<bool>,
    pub allow_managed_hooks_only: Value<bool>,
    pub receipts_enabled: Value<Vec<HostObservedPluginSetting>>,
    pub configured_recognized_events: Value<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConfigurationObservation {
    pub source: Source,
    pub result: Result<HostObservedConfiguration, Error>,
}

/// No executable, argv, arbitrary file suffix, or raw configuration input.
/// The root is context only and is never included in observation output.
pub struct HostCapabilityObservationRequest {
    pub host: HostId,
    pub project_root: PathBuf,
}

/// Seven validity families: plugin capability; definition identity; trust;
/// enablement; admin policy; lifecycle coverage; relevant source configuration.
/// Unknown effective facts remain unknown even when a source file is readable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilityObservation {
    pub host: HostId,
    pub plugin_supported: Value<bool>,
    pub plugin_installed: Value<bool>,
    pub plugins: Result<Vec<HostObservedPlugin>, Error>,
    pub hook_definition_identity: Value<String>,
    pub hooks_supported: Value<bool>,
    pub hooks_configured: Value<bool>,
    pub hook_trust_required: Value<bool>,
    pub hooks_trusted: Value<bool>,
    pub hooks_enabled: Value<bool>,
    pub hooks_allowed_by_admin_policy: Value<bool>,
    pub hook_coverage_class: HostCapabilityHookCoverageClass,
    pub required_hook_coverage_satisfied: Value<bool>,
    pub relevant_config_identity: Value<String>,
    pub configurations: Vec<HostConfigurationObservation>,
}

impl HostCapabilityObservation {
    /// Mechanical adaptation only. Freshness and override belong to the caller;
    /// selection/report construction remain with the accepted composition API.
    /// These sources cannot establish COMPLETE. No physical fingerprint is made.
    pub fn report_selection_inputs(
        &self,
        report_validity_proven_current: bool,
        stale_reason: HostCapabilityStaleReason,
        mode_override: Option<HostCapabilityModeOverride>,
    ) -> HostCapabilityReportSelectionCompositionInputs {
        HostCapabilityReportSelectionCompositionInputs {
            report_validity_proven_current,
            host_id: self.host.as_str().into(),
            host_version: None,
            probe_status: HostCapabilityProbeStatus::Partial,
            validity_fingerprint: None,
            // An identity is not necessarily a digest. No current source supplies either digest.
            hook_definition_digest: None,
            relevant_config_digest: None,
            stale_reason,
            plugin_supported: self.plugin_supported.policy_value(),
            plugin_installed: self.plugin_installed.policy_value(),
            manifest_path: None,
            supports_skills: None,
            supports_commands: None,
            supports_subagents: None,
            supports_mcp: None,
            hooks_supported: self.hooks_supported.policy_value(),
            hooks_configured: self.hooks_configured.policy_value(),
            hook_trust_required: self.hook_trust_required.policy_value(),
            hooks_trusted: self.hooks_trusted.policy_value(),
            hooks_enabled: self.hooks_enabled.policy_value(),
            hooks_allowed_by_admin_policy: self.hooks_allowed_by_admin_policy.policy_value(),
            hook_events: None,
            blocking_hook_events: None,
            hook_coverage_class: self.hook_coverage_class,
            required_hook_coverage_satisfied: self.required_hook_coverage_satisfied.policy_value(),
            mode_override,
            plugin_data_path: None,
            sandbox_modes: None,
            evidence_label: None,
            source_claim_id: None,
        }
    }
}

/// Performs only the internally enumerated, read-only Host observations.
pub fn observe_host_capabilities(
    request: &HostCapabilityObservationRequest,
) -> HostCapabilityObservation {
    observe(request, &io::LocalSources)
}

// Private injection boundary: a test double cannot become a public evidence API.
trait Sources {
    fn plugins(&self, project_root: &std::path::Path) -> Result<Vec<u8>, Error>;
    fn configurations(
        &self,
        request: &HostCapabilityObservationRequest,
    ) -> Vec<(Source, Result<Vec<u8>, Error>)>;
}

fn observe(
    request: &HostCapabilityObservationRequest,
    sources: &impl Sources,
) -> HostCapabilityObservation {
    let plugins = if request.host == HostId::ClaudeCode {
        sources
            .plugins(&request.project_root)
            .and_then(|bytes| decode::claude_plugins(&bytes))
    } else {
        Err(Error::Unsupported)
    };
    let (plugin_supported, plugin_installed) = match &plugins {
        Ok(entries) => (Value::Known(true), Value::Known(!entries.is_empty())),
        Err(_) => (Value::Unknown, Value::Unknown),
    };
    let configurations = sources
        .configurations(request)
        .into_iter()
        .map(|(source, bytes)| HostConfigurationObservation {
            source,
            result: bytes.and_then(|bytes| decode::configuration(source, &bytes)),
        })
        .collect();
    HostCapabilityObservation {
        host: request.host,
        plugin_supported,
        plugin_installed,
        plugins,
        hook_definition_identity: Value::Unknown,
        hooks_supported: Value::Unknown,
        hooks_configured: Value::Unknown,
        hook_trust_required: Value::Unknown,
        hooks_trusted: Value::Unknown,
        hooks_enabled: Value::Unknown,
        hooks_allowed_by_admin_policy: Value::Unknown,
        hook_coverage_class: HostCapabilityHookCoverageClass::Unknown,
        // No authoritative orchestrator required-event set is provided by C1–C3/X1–X3.
        required_hook_coverage_satisfied: Value::Unknown,
        relevant_config_identity: Value::Unknown,
        configurations,
    }
}
