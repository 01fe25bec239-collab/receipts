//! Bounded read-only observations. Source facts are not effective policy.
//!
//! Authority: BUILD-A1-ADR-HOST-A3-020-BOUNDED-STRUCTURED-PARSING-SUBSTRATE-001,
//! sources C1–C3 and X1–X3, and `HOST_CAPABILITY_DISCOVERY.md` (HI-W2-1).
//! File settings cannot establish higher managed tiers (MDM, server-managed
//! settings), project trust, or Receipts hook ownership, so they can only
//! REFUTE native hooks (`disableAllHooks`, `allowManagedHooksOnly`, a plugin
//! the host reports disabled), never prove them enabled or admin-allowed.
//! Plugin-list and plugin definitions establish observations only. Hook JSON,
//! environment markers and matching paths never prove effective permission.
//! Positive enablement/admin authority needs a later admitted trusted supplier.
//!
//! Claude Code has no hook trust model (frozen fixture 02):
//! `hook_trust_required` is known false and `hooks_trusted` stays unknown.

use crate::host_probe_port::{HostObservationFingerprintV1, HostProbeProcessPort, digest};
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
    /// The probe executable no longer holds the admitted bytes.
    ExecutableChanged,
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
    /// The installed Receipts plugin's `hooks/hooks.json`.
    ClaudePluginHooks,
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
    /// Legacy hook observation; it conveys no positive authority.
    pub hook_execution: Option<HostHookExecutionEvidenceV1>,
}

/// Legacy plugin-root observation. Even a matching root conveys no permission.
/// No production constructor exists; the retained request field is compatibility only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostHookExecutionEvidenceV1 {
    pub(crate) plugin_root: PathBuf,
}

/// Required registrations from the frozen Claude lifecycle mapping.
/// Core/worker mappings additionally require an admitted trusted supplier;
/// registration alone cannot establish that supplier or native admission.
pub const CLAUDE_REQUIRED_HOOK_EVENTS: [&str; 11] = [
    "SessionStart",
    "SessionEnd",
    "PostToolUse",
    "PreCompact",
    "PostCompact",
    "WorktreeCreate",
    "WorktreeRemove",
    "FileChanged",
    "SubagentStart",
    "SubagentStop",
    "TaskCompleted",
];

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
    /// First token of the version probe output, when well formed.
    pub host_version: Value<String>,
    /// Events the installed Receipts plugin manifest configures, sorted.
    pub hook_events: Value<Vec<String>>,
    /// Set only from a COMPLETE fingerprint of this same observation.
    pub validity_fingerprint: Option<String>,
    pub hook_definition_digest: Option<String>,
    pub relevant_config_digest: Option<String>,
}

impl HostCapabilityObservation {
    /// COMPLETE only with a complete fingerprint of this observation and every
    /// load-bearing native fact known; otherwise PARTIAL.
    fn probe_status(&self) -> HostCapabilityProbeStatus {
        let known = [
            &self.plugin_supported,
            &self.plugin_installed,
            &self.hooks_supported,
            &self.hooks_configured,
            &self.hook_trust_required,
            &self.hooks_enabled,
            &self.hooks_allowed_by_admin_policy,
            &self.required_hook_coverage_satisfied,
        ]
        .iter()
        .all(|v| matches!(v, Value::Known(_)));
        let trust = match self.hook_trust_required {
            Value::Known(true) => matches!(self.hooks_trusted, Value::Known(_)),
            _ => true,
        };
        if self.validity_fingerprint.is_some() && known && trust {
            HostCapabilityProbeStatus::Complete
        } else {
            HostCapabilityProbeStatus::Partial
        }
    }

    /// Mechanical adaptation only. Freshness and override belong to the caller;
    /// selection/report construction remain with the accepted composition API.
    pub fn report_selection_inputs(
        &self,
        report_validity_proven_current: bool,
        stale_reason: HostCapabilityStaleReason,
        mode_override: Option<HostCapabilityModeOverride>,
    ) -> HostCapabilityReportSelectionCompositionInputs {
        HostCapabilityReportSelectionCompositionInputs {
            report_validity_proven_current,
            host_id: crate::NormalizedHostEventHost::from(self.host)
                .as_str()
                .into(),
            host_version: self.host_version.policy_value(),
            probe_status: self.probe_status(),
            validity_fingerprint: self.validity_fingerprint.clone(),
            // Identities stay opaque; only fingerprint-path digests are digests.
            hook_definition_digest: self.hook_definition_digest.clone(),
            relevant_config_digest: self.relevant_config_digest.clone(),
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
            hook_events: self.hook_events.policy_value(),
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
/// Observes through the admitted composition's probe port.
pub fn observe_host_capabilities(
    request: &HostCapabilityObservationRequest,
    port: &dyn HostProbeProcessPort,
) -> HostCapabilityObservation {
    observe_host_capabilities_with_fingerprint(request, port).0
}

/// Observes and returns the exact fingerprint of the same observation.
pub fn observe_host_capabilities_with_fingerprint(
    request: &HostCapabilityObservationRequest,
    port: &dyn HostProbeProcessPort,
) -> (HostCapabilityObservation, HostObservationFingerprintV1) {
    let probes = request.host == HostId::ClaudeCode;
    let project_safe = crate::private_fs::HostDirectory::open(&request.project_root)
        .map_err(|_| Error::NotRegularFile);
    let executable = probes.then(|| {
        project_safe
            .as_ref()
            .map_err(|e| *e)
            .and_then(|_| port.executable())
    });
    let version = probes.then(|| {
        project_safe
            .as_ref()
            .map_err(|e| *e)
            .and_then(|_| port.run(&io::probe_command(&request.project_root, &["--version"])))
    });
    let (mut observation, plugins, sources) = observe_material(request, &io::LocalSources(port));
    // Every probe must describe the same admitted executable: re-check after
    // the last probe so a port that reports a different identity (or none)
    // leaves this fingerprint incomplete.
    let executable = executable.map(|before| {
        match (
            before,
            project_safe
                .as_ref()
                .map_err(|e| *e)
                .and_then(|_| port.executable()),
        ) {
            (Ok(before), Ok(after)) if before == after => Ok(before),
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
            (Ok(_), Ok(_)) => Err(Error::ExecutableChanged),
        }
    });
    let fingerprint = HostObservationFingerprintV1 {
        host: request.host,
        project_root: request.project_root.clone(),
        executable,
        version: version.as_ref().map(digest),
        plugins: plugins.as_ref().map(digest),
        sources: sources.iter().map(|(s, m)| (*s, digest(m))).collect(),
    };
    if let Some(Ok(bytes)) = &version {
        observation.host_version = decode::version(bytes);
    }
    if fingerprint.is_complete() {
        let digests = fingerprint.digests();
        observation.validity_fingerprint = Some(digests.validity_fingerprint);
        observation.hook_definition_digest = digests.hook_definition;
        observation.relevant_config_digest = Some(digests.relevant_config);
    }
    (observation, fingerprint)
}

// Private injection boundary: a test double cannot become a public evidence API.
trait Sources {
    fn plugins(&self, project_root: &std::path::Path) -> Result<Vec<u8>, Error>;
    fn configurations(
        &self,
        request: &HostCapabilityObservationRequest,
    ) -> Vec<(Source, Result<Vec<u8>, Error>)>;
    /// `<install_path>/hooks/hooks.json` of the installed Receipts plugin.
    fn plugin_hooks(&self, _install_path: &std::path::Path) -> Result<Vec<u8>, Error> {
        Err(Error::Unavailable)
    }
}

#[cfg(test)]
fn observe(
    request: &HostCapabilityObservationRequest,
    sources: &impl Sources,
) -> HostCapabilityObservation {
    observe_material(request, sources).0
}

type SourceMaterial = Vec<(Source, Result<Vec<u8>, Error>)>;

fn observe_material(
    request: &HostCapabilityObservationRequest,
    sources: &impl Sources,
) -> (
    HostCapabilityObservation,
    Option<Result<Vec<u8>, Error>>,
    SourceMaterial,
) {
    let plugin_bytes =
        (request.host == HostId::ClaudeCode).then(|| sources.plugins(&request.project_root));
    let plugins = match &plugin_bytes {
        Some(bytes) => bytes
            .clone()
            .and_then(|bytes| decode::claude_plugins(&bytes)),
        None => Err(Error::Unsupported),
    };
    let (plugin_supported, plugin_installed) = match &plugins {
        Ok(entries) => (Value::Known(true), Value::Known(!entries.is_empty())),
        Err(_) => (Value::Unknown, Value::Unknown),
    };
    let mut material = sources.configurations(request);
    let configurations: Vec<HostConfigurationObservation> = material
        .iter()
        .map(|(source, bytes)| HostConfigurationObservation {
            source: *source,
            result: bytes
                .clone()
                .and_then(|bytes| decode::configuration(*source, &bytes)),
        })
        .collect();
    let mut observation = HostCapabilityObservation {
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
        required_hook_coverage_satisfied: Value::Unknown,
        relevant_config_identity: Value::Unknown,
        configurations,
        host_version: Value::Unknown,
        hook_events: Value::Unknown,
        validity_fingerprint: None,
        hook_definition_digest: None,
        relevant_config_digest: None,
    };
    if request.host == HostId::ClaudeCode {
        let install_path = plugin_bytes
            .as_ref()
            .and_then(|b| b.as_ref().ok())
            .and_then(|b| decode::claude_receipts_install_path(b));
        let hooks = install_path.as_ref().map(|p| sources.plugin_hooks(p));
        claude_facts(&mut observation, hooks.as_ref());
        if let Some(hooks) = hooks {
            material.push((Source::ClaudePluginHooks, hooks));
        }
    }
    if request.host == HostId::Codex {
        codex_facts(&mut observation);
    }
    (observation, plugin_bytes, material)
}

/// Claude Code fact derivation; see the module documentation. Files only
/// refute; positive enablement/admin facts require admitted effective authority.
fn claude_facts(o: &mut HostCapabilityObservation, hooks: Option<&Result<Vec<u8>, Error>>) {
    use HostCapabilityHookCoverageClass as Coverage;
    let listed = o.plugins.as_ref().ok();
    if listed.is_some() {
        o.hooks_supported = Value::Known(true);
        o.hook_trust_required = Value::Known(false);
    }
    let events = match (&o.plugin_installed, hooks) {
        (Value::Known(false), _) => Value::Known(Vec::new()),
        (Value::Known(true), Some(Ok(bytes))) => decode::plugin_hook_events(bytes),
        (Value::Known(true), Some(Err(Error::Missing))) => Value::Known(Vec::new()),
        _ => Value::Unknown,
    };
    if let Value::Known(list) = &events {
        let required = CLAUDE_REQUIRED_HOOK_EVENTS
            .iter()
            .filter(|e| list.iter().any(|l| l == *e))
            .count();
        o.hooks_configured = Value::Known(!list.is_empty());
        o.required_hook_coverage_satisfied =
            Value::Known(required == CLAUDE_REQUIRED_HOOK_EVENTS.len());
        // The recognized set is not an exhaustive, version-bound vendor surface.
        // Registration can establish partial coverage, never vendor-wide FULL.
        o.hook_coverage_class = if list.is_empty() {
            Coverage::None
        } else {
            Coverage::Partial
        };
        let mut sorted = list.clone();
        sorted.sort();
        o.hook_events = Value::Known(sorted);
    }
    let managed = |s: Source| {
        matches!(
            s,
            Source::ClaudeManagedSettings | Source::ClaudeManagedDropIn
        )
    };
    let flag = |pick: fn(&HostObservedConfiguration) -> &Value<bool>, only_managed: bool| {
        o.configurations.iter().any(|c| {
            (!only_managed || managed(c.source))
                && c.result
                    .as_ref()
                    .is_ok_and(|r| pick(r) == &Value::Known(true))
        })
    };
    let disabled_by_file = flag(|r| &r.disable_all_hooks, false);
    let managed_only = flag(|r| &r.allow_managed_hooks_only, true);
    let plugin_disabled = listed.is_some_and(|plugins| plugins.iter().any(|p| p.enabled == Value::Known(false)))
        || o.configurations.iter().any(|c| c.result.as_ref().is_ok_and(|r| {
            matches!(&r.receipts_enabled, Value::Known(settings) if settings.iter().any(|p| p.enabled == Value::Known(false)))
        }));
    // Observations can refute permission; neither a path match nor a hook claim admits it.
    o.hooks_enabled = if disabled_by_file || plugin_disabled {
        Value::Known(false)
    } else {
        Value::Unknown
    };
    o.hooks_allowed_by_admin_policy = if managed_only {
        Value::Known(false)
    } else {
        Value::Unknown
    };
}

// Codex files can refute permission; missing/failed sources and positive flags
// never establish effective enablement, trust or managed authority.
fn codex_facts(o: &mut HostCapabilityObservation) {
    if o.configurations.iter().any(|c| {
        c.result
            .as_ref()
            .is_ok_and(|r| r.hooks_feature == Value::Known(false))
    }) {
        o.hooks_enabled = Value::Known(false);
    }
    if o.configurations.iter().any(|c| {
        c.source == Source::CodexLocalRequirements
            && c.result
                .as_ref()
                .is_ok_and(|r| r.allow_managed_hooks_only == Value::Known(true))
    }) {
        o.hooks_allowed_by_admin_policy = Value::Known(false);
    }
}
