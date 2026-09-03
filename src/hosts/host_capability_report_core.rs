//! In-process non-temporal core for a host capability report.
//!
//! This module composes the already-integrated host capability structural
//! vocabulary and validation pieces into
//! [`HostCapabilityReportNonTemporalCore`].
//!
//! - Exactly 31 non-temporal schema properties are represented; the schema at
//!   `build-control/orchestrator-architecture/schemas/HostCapabilityReport.schema.json`
//!   defines 33 properties and this core deliberately omits the two date-time
//!   fields `probed_at` and `last_verified_at`.
//! - `probed_at` and `last_verified_at` are deliberately omitted; no temporal
//!   field, timestamp alias, or date/time import exists in this core.
//! - This is NOT `HostCapabilityReport`; no `HostCapabilityReport` struct,
//!   alias, builder, or constructor is introduced here.
//! - The caller supplies all values; this core performs no probing, observes
//!   no filesystem, environment, plugin, hook, trust, admin-policy, version,
//!   subprocess, network, or credential state.
//! - No probing occurs and no persistence occurs (no file/database/cache read,
//!   write, load, versioning, or global state).
//! - No mode selection occurs; [`HostCapabilityReportNonTemporalCore`] stores
//!   the caller-supplied `selected_mode` exactly without inferring it.
//! - COMPLETE consistency is delegated to the existing
//!   [`validate_complete_probe_consistency`](crate::validate_complete_probe_consistency)
//!   validator; its four rules are reused, never reimplemented.
//! - `selected_mode` is stored, not inferred, and grants no authority.
//! - Values grant no authority and no capability, trust, privilege, or
//!   authorization is conferred by construction or access.
//! - No serialization contract is created: no `serde`, `Serialize`,
//!   `Deserialize`, JSON parsing/generation, `FromStr`, `TryFrom<&str>`, or
//!   wire DTO exists here.

use crate::{
    HostCapabilityConsistencyError, HostCapabilityConsistencyInputs, HostCapabilityEvidenceLabel,
    HostCapabilityHookCoverageClass, HostCapabilityInactiveReason, HostCapabilityModeOverride,
    HostCapabilityProbeStatus, HostCapabilitySelectedMode, HostCapabilityStaleReason,
    validate_complete_probe_consistency,
};

/// Caller-supplied inputs for [`HostCapabilityReportNonTemporalCore`].
///
/// Semantically includes all 31 non-temporal schema properties and no other
/// property. There is no `probed_at` field and no `last_verified_at` field.
/// Every value is caller supplied; no probing, persistence, freshness
/// execution, mode selection, fingerprint calculation, or serialization is
/// performed. Values grant no authority.
///
/// Optional non-null schema properties such as arrays and enums preserve
/// absence with `Option`. Optional nullable scalar properties use a single
/// `Option` without distinguishing wire-level absence from JSON null; no
/// nested `Option<Option<T>>` is introduced because serialization is outside
/// this slice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilityReportNonTemporalCoreInputs {
    /// Open host identifier; must be non-empty (`minLength = 1`).
    pub host_id: String,
    /// Caller-supplied host version; empty strings remain valid.
    pub host_version: Option<String>,
    /// Caller-supplied probe outcome vocabulary.
    pub probe_status: HostCapabilityProbeStatus,
    /// Caller-supplied validity fingerprint; `Some("")` is rejected.
    pub validity_fingerprint: Option<String>,
    /// Caller-supplied hook definition digest; empty strings remain valid.
    pub hook_definition_digest: Option<String>,
    /// Caller-supplied relevant config digest; empty strings remain valid.
    pub relevant_config_digest: Option<String>,
    /// Caller-supplied staleness vocabulary.
    pub stale_reason: HostCapabilityStaleReason,
    /// Caller-supplied plugin support fact.
    pub plugin_supported: Option<bool>,
    /// Caller-supplied plugin installed fact.
    pub plugin_installed: Option<bool>,
    /// Caller-supplied manifest path; empty strings remain valid.
    pub manifest_path: Option<String>,
    /// Caller-supplied skills support fact.
    pub supports_skills: Option<bool>,
    /// Caller-supplied commands support fact.
    pub supports_commands: Option<bool>,
    /// Caller-supplied subagents support fact.
    pub supports_subagents: Option<bool>,
    /// Caller-supplied MCP support fact.
    pub supports_mcp: Option<bool>,
    /// Caller-supplied hooks supported fact.
    pub hooks_supported: Option<bool>,
    /// Caller-supplied hooks configured fact.
    pub hooks_configured: Option<bool>,
    /// Caller-supplied hook trust required fact.
    pub hook_trust_required: Option<bool>,
    /// Caller-supplied hooks trusted fact.
    pub hooks_trusted: Option<bool>,
    /// Caller-supplied hooks enabled fact.
    pub hooks_enabled: Option<bool>,
    /// Caller-supplied admin policy allowance fact.
    pub hooks_allowed_by_admin_policy: Option<bool>,
    /// Caller-supplied hook events; ordering, duplicates, and empty strings
    /// are preserved exactly.
    pub hook_events: Option<Vec<String>>,
    /// Caller-supplied blocking hook events; preserved exactly.
    pub blocking_hook_events: Option<Vec<String>>,
    /// Caller-supplied hook coverage vocabulary.
    pub hook_coverage_class: HostCapabilityHookCoverageClass,
    /// Caller-supplied required coverage fact.
    pub required_hook_coverage_satisfied: Option<bool>,
    /// Caller-supplied selected mode; stored exactly, never inferred.
    pub selected_mode: HostCapabilitySelectedMode,
    /// Caller-supplied mode override; stored exactly, never derived.
    pub mode_override: Option<HostCapabilityModeOverride>,
    /// Caller-supplied inactive reason; stored exactly, never recomputed.
    pub inactive_reason: Option<HostCapabilityInactiveReason>,
    /// Caller-supplied plugin data path; empty strings remain valid.
    pub plugin_data_path: Option<String>,
    /// Caller-supplied sandbox modes; preserved exactly.
    pub sandbox_modes: Option<Vec<String>>,
    /// Caller-supplied evidence label vocabulary.
    pub evidence_label: Option<HostCapabilityEvidenceLabel>,
    /// Caller-supplied source claim id; empty strings remain valid.
    pub source_claim_id: Option<String>,
}

/// Validated non-temporal core of a host capability report.
///
/// Represents exactly the 31 non-temporal schema properties. The two
/// date-time schema properties `probed_at` and `last_verified_at` are
/// deliberately omitted and no temporal alias exists. This is NOT
/// `HostCapabilityReport`.
///
/// Fields are private and immutable: there are no public mutable fields, no
/// public setters, no `&mut` accessors, and no method that can make `host_id`
/// empty or `validity_fingerprint` `Some("")` after construction. The caller
/// supplies all values; no probing, persistence, freshness execution, mode
/// selection, fingerprint calculation/comparison, or serialization occurs.
/// COMPLETE consistency is delegated to the existing validator. Stored values
/// grant no authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCapabilityReportNonTemporalCore {
    host_id: String,
    host_version: Option<String>,
    probe_status: HostCapabilityProbeStatus,
    validity_fingerprint: Option<String>,
    hook_definition_digest: Option<String>,
    relevant_config_digest: Option<String>,
    stale_reason: HostCapabilityStaleReason,
    plugin_supported: Option<bool>,
    plugin_installed: Option<bool>,
    manifest_path: Option<String>,
    supports_skills: Option<bool>,
    supports_commands: Option<bool>,
    supports_subagents: Option<bool>,
    supports_mcp: Option<bool>,
    hooks_supported: Option<bool>,
    hooks_configured: Option<bool>,
    hook_trust_required: Option<bool>,
    hooks_trusted: Option<bool>,
    hooks_enabled: Option<bool>,
    hooks_allowed_by_admin_policy: Option<bool>,
    hook_events: Option<Vec<String>>,
    blocking_hook_events: Option<Vec<String>>,
    hook_coverage_class: HostCapabilityHookCoverageClass,
    required_hook_coverage_satisfied: Option<bool>,
    selected_mode: HostCapabilitySelectedMode,
    mode_override: Option<HostCapabilityModeOverride>,
    inactive_reason: Option<HostCapabilityInactiveReason>,
    plugin_data_path: Option<String>,
    sandbox_modes: Option<Vec<String>>,
    evidence_label: Option<HostCapabilityEvidenceLabel>,
    source_claim_id: Option<String>,
}

/// Deterministic bounded construction error for
/// [`HostCapabilityReportNonTemporalCore`].
///
/// Semantically exactly these error classes exist: an empty `host_id`, an
/// empty `validity_fingerprint`, or a wrapped
/// [`HostCapabilityConsistencyError`] from the reused COMPLETE validator. No
/// string, boxed, I/O, generic schema, policy, or authorization error exists
/// here, and the four consistency variants are wrapped rather than
/// duplicated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostCapabilityReportNonTemporalCoreError {
    /// The supplied `host_id` was empty; schema requires `minLength = 1`.
    EmptyHostId,
    /// The supplied `validity_fingerprint` was `Some("")`.
    EmptyValidityFingerprint,
    /// The reused COMPLETE validator rejected the eight supplied facts.
    CompleteProbeConsistency(HostCapabilityConsistencyError),
}

impl core::fmt::Display for HostCapabilityReportNonTemporalCoreError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyHostId => write!(f, "host_id must not be empty"),
            Self::EmptyValidityFingerprint => {
                write!(f, "validity_fingerprint must not be empty")
            }
            Self::CompleteProbeConsistency(inner) => {
                write!(f, "complete probe consistency violated: {inner:?}")
            }
        }
    }
}

impl std::error::Error for HostCapabilityReportNonTemporalCoreError {}

impl HostCapabilityReportNonTemporalCore {
    /// Validates and stores all 31 non-temporal values exactly.
    ///
    /// Responsibilities are only: reject an empty `host_id`; reject
    /// `validity_fingerprint == Some("")`; invoke the existing
    /// `validate_complete_probe_consistency` for the relevant eight supplied
    /// facts and map its error; otherwise store all 31 values byte-for-byte.
    /// No other policy applies: no trimming, normalization, fingerprint
    /// calculation, freshness, native-prerequisite assessment, inactive-reason
    /// recomputation, mode selection, probing, persistence, or serialization.
    pub fn new(
        inputs: HostCapabilityReportNonTemporalCoreInputs,
    ) -> Result<Self, HostCapabilityReportNonTemporalCoreError> {
        if inputs.host_id.is_empty() {
            return Err(HostCapabilityReportNonTemporalCoreError::EmptyHostId);
        }
        if matches!(inputs.validity_fingerprint.as_deref(), Some("")) {
            return Err(HostCapabilityReportNonTemporalCoreError::EmptyValidityFingerprint);
        }
        validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
            probe_status: inputs.probe_status,
            plugin_supported: inputs.plugin_supported,
            plugin_installed: inputs.plugin_installed,
            hooks_supported: inputs.hooks_supported,
            hooks_configured: inputs.hooks_configured,
            hooks_enabled: inputs.hooks_enabled,
            hook_trust_required: inputs.hook_trust_required,
            hooks_trusted: inputs.hooks_trusted,
        })
        .map_err(HostCapabilityReportNonTemporalCoreError::CompleteProbeConsistency)?;

        Ok(Self {
            host_id: inputs.host_id,
            host_version: inputs.host_version,
            probe_status: inputs.probe_status,
            validity_fingerprint: inputs.validity_fingerprint,
            hook_definition_digest: inputs.hook_definition_digest,
            relevant_config_digest: inputs.relevant_config_digest,
            stale_reason: inputs.stale_reason,
            plugin_supported: inputs.plugin_supported,
            plugin_installed: inputs.plugin_installed,
            manifest_path: inputs.manifest_path,
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
            hook_events: inputs.hook_events,
            blocking_hook_events: inputs.blocking_hook_events,
            hook_coverage_class: inputs.hook_coverage_class,
            required_hook_coverage_satisfied: inputs.required_hook_coverage_satisfied,
            selected_mode: inputs.selected_mode,
            mode_override: inputs.mode_override,
            inactive_reason: inputs.inactive_reason,
            plugin_data_path: inputs.plugin_data_path,
            sandbox_modes: inputs.sandbox_modes,
            evidence_label: inputs.evidence_label,
            source_claim_id: inputs.source_claim_id,
        })
    }

    /// Returns the open `host_id` exactly as supplied.
    pub fn host_id(&self) -> &str {
        &self.host_id
    }

    /// Returns the caller-supplied host version exactly as supplied.
    pub fn host_version(&self) -> Option<&str> {
        self.host_version.as_deref()
    }

    /// Returns the caller-supplied probe status.
    pub fn probe_status(&self) -> HostCapabilityProbeStatus {
        self.probe_status
    }

    /// Returns the caller-supplied validity fingerprint exactly as supplied.
    pub fn validity_fingerprint(&self) -> Option<&str> {
        self.validity_fingerprint.as_deref()
    }

    /// Returns the caller-supplied hook definition digest exactly as supplied.
    pub fn hook_definition_digest(&self) -> Option<&str> {
        self.hook_definition_digest.as_deref()
    }

    /// Returns the caller-supplied relevant config digest exactly as supplied.
    pub fn relevant_config_digest(&self) -> Option<&str> {
        self.relevant_config_digest.as_deref()
    }

    /// Returns the caller-supplied stale reason.
    pub fn stale_reason(&self) -> HostCapabilityStaleReason {
        self.stale_reason
    }

    /// Returns the caller-supplied plugin supported fact.
    pub fn plugin_supported(&self) -> Option<bool> {
        self.plugin_supported
    }

    /// Returns the caller-supplied plugin installed fact.
    pub fn plugin_installed(&self) -> Option<bool> {
        self.plugin_installed
    }

    /// Returns the caller-supplied manifest path exactly as supplied.
    pub fn manifest_path(&self) -> Option<&str> {
        self.manifest_path.as_deref()
    }

    /// Returns the caller-supplied skills support fact.
    pub fn supports_skills(&self) -> Option<bool> {
        self.supports_skills
    }

    /// Returns the caller-supplied commands support fact.
    pub fn supports_commands(&self) -> Option<bool> {
        self.supports_commands
    }

    /// Returns the caller-supplied subagents support fact.
    pub fn supports_subagents(&self) -> Option<bool> {
        self.supports_subagents
    }

    /// Returns the caller-supplied MCP support fact.
    pub fn supports_mcp(&self) -> Option<bool> {
        self.supports_mcp
    }

    /// Returns the caller-supplied hooks supported fact.
    pub fn hooks_supported(&self) -> Option<bool> {
        self.hooks_supported
    }

    /// Returns the caller-supplied hooks configured fact.
    pub fn hooks_configured(&self) -> Option<bool> {
        self.hooks_configured
    }

    /// Returns the caller-supplied hook trust required fact.
    pub fn hook_trust_required(&self) -> Option<bool> {
        self.hook_trust_required
    }

    /// Returns the caller-supplied hooks trusted fact.
    pub fn hooks_trusted(&self) -> Option<bool> {
        self.hooks_trusted
    }

    /// Returns the caller-supplied hooks enabled fact.
    pub fn hooks_enabled(&self) -> Option<bool> {
        self.hooks_enabled
    }

    /// Returns the caller-supplied admin policy allowance fact.
    pub fn hooks_allowed_by_admin_policy(&self) -> Option<bool> {
        self.hooks_allowed_by_admin_policy
    }

    /// Returns the caller-supplied hook events with ordering, duplicates, and
    /// empty strings preserved exactly.
    pub fn hook_events(&self) -> Option<&[String]> {
        self.hook_events.as_deref()
    }

    /// Returns the caller-supplied blocking hook events preserved exactly.
    pub fn blocking_hook_events(&self) -> Option<&[String]> {
        self.blocking_hook_events.as_deref()
    }

    /// Returns the caller-supplied hook coverage class.
    pub fn hook_coverage_class(&self) -> HostCapabilityHookCoverageClass {
        self.hook_coverage_class
    }

    /// Returns the caller-supplied required coverage fact.
    pub fn required_hook_coverage_satisfied(&self) -> Option<bool> {
        self.required_hook_coverage_satisfied
    }

    /// Returns the caller-supplied selected mode exactly as supplied.
    pub fn selected_mode(&self) -> HostCapabilitySelectedMode {
        self.selected_mode
    }

    /// Returns the caller-supplied mode override exactly as supplied.
    pub fn mode_override(&self) -> Option<&HostCapabilityModeOverride> {
        self.mode_override.as_ref()
    }

    /// Returns the caller-supplied inactive reason exactly as supplied.
    pub fn inactive_reason(&self) -> Option<HostCapabilityInactiveReason> {
        self.inactive_reason
    }

    /// Returns the caller-supplied plugin data path exactly as supplied.
    pub fn plugin_data_path(&self) -> Option<&str> {
        self.plugin_data_path.as_deref()
    }

    /// Returns the caller-supplied sandbox modes preserved exactly.
    pub fn sandbox_modes(&self) -> Option<&[String]> {
        self.sandbox_modes.as_deref()
    }

    /// Returns the caller-supplied evidence label exactly as supplied.
    pub fn evidence_label(&self) -> Option<HostCapabilityEvidenceLabel> {
        self.evidence_label
    }

    /// Returns the caller-supplied source claim id exactly as supplied.
    pub fn source_claim_id(&self) -> Option<&str> {
        self.source_claim_id.as_deref()
    }
}
