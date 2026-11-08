//! HI-WIRE: strict JSON codecs for the two Host-owned frozen schemas,
//! `HostCapabilityReport.schema.json` and `NormalizedHostEvent.schema.json`.
//!
//! Decoding is strict: one JSON object (duplicate keys refused by the shared
//! Orchestration parser), the schema's closed property set, exact types,
//! enum spellings, length bounds and date-time grammar. Values then pass the
//! existing in-process validators. A decoded report must additionally be the
//! report the frozen selection policy derives from its own facts: its
//! `selected_mode` and `inactive_reason` are recomputed and compared, so a
//! file cannot assert a mode its facts do not support. Errors carry a field
//! name only, never input text.
//!
//! Encoding is compact canonical JSON with deterministic key order. Required
//! nullable report fields are written as `null`; absent optional values are
//! omitted. Absent and `null` optional nullable report values decode to the
//! same `None`. An event's optional `payload` decodes to an empty object when
//! absent and is always written.

use std::collections::BTreeMap;

use receipts_orchestration::orchestration::{
    OrchestrationDateTimeV1, OrchestrationJsonObjectV1, OrchestrationJsonValueV1 as V,
};

use crate::{
    HostCapabilityEvidenceLabel, HostCapabilityHookCoverageClass, HostCapabilityInactiveReason,
    HostCapabilityModeOverride, HostCapabilityModeOverrideSource,
    HostCapabilityNativePrerequisiteInputs, HostCapabilityProbeStatus, HostCapabilityReport,
    HostCapabilityReportNonTemporalCore, HostCapabilityReportNonTemporalCoreInputs,
    HostCapabilityReportSelectionCompositionInputs,
    HostCapabilityReportSelectionCompositionOutcome, HostCapabilitySelectedMode,
    HostCapabilitySelectedModeConsistencyInputs, HostCapabilityStaleReason, NormalizedHostEvent,
    NormalizedHostEventConfidence, NormalizedHostEventHost, NormalizedHostEventId,
    NormalizedHostEventInputs, NormalizedHostEventRawRef, NormalizedHostEventRawRefType,
    NormalizedHostEventType, assess_native_path_prerequisites,
    compose_host_capability_report_selection, validate_selected_mode_consistency,
};

/// Largest accepted encoded document for either codec.
pub const HOST_WIRE_MAX_BYTES: usize = 1_048_576;

/// Why a document was refused. Only the offending field name is retained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostWireError {
    TooLarge,
    InvalidJson,
    NotAnObject,
    /// A property outside the schema's closed set (`additionalProperties: false`).
    UnknownField,
    MissingField(&'static str),
    InvalidField(&'static str),
    /// The value validators refused the decoded values.
    Invalid(&'static str),
    /// The declared mode/inactive reason is not what the frozen selection
    /// policy derives from the report's own facts.
    SelectionMismatch,
}

impl std::fmt::Display for HostWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge => f.write_str("document too large"),
            Self::InvalidJson => f.write_str("invalid JSON"),
            Self::NotAnObject => f.write_str("document is not a JSON object"),
            Self::UnknownField => f.write_str("unknown field"),
            Self::MissingField(n) => write!(f, "missing field {n}"),
            Self::InvalidField(n) => write!(f, "invalid field {n}"),
            Self::Invalid(n) => write!(f, "invalid {n}"),
            Self::SelectionMismatch => f.write_str("declared mode contradicts the report facts"),
        }
    }
}

impl std::error::Error for HostWireError {}

type Map = BTreeMap<String, V>;

fn parse_object(text: &str) -> Result<Map, HostWireError> {
    if text.len() > HOST_WIRE_MAX_BYTES {
        return Err(HostWireError::TooLarge);
    }
    match V::parse_json(text).map_err(|_| HostWireError::InvalidJson)? {
        V::Object(object) => Ok(object.as_map().clone()),
        _ => Err(HostWireError::NotAnObject),
    }
}

/// Closed-set field reader: every key must be consumed.
struct Fields(Map);

impl Fields {
    /// Every schema property was consumed; anything left is not in the schema.
    fn finish(self) -> Result<(), HostWireError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(HostWireError::UnknownField)
        }
    }
    fn take(&mut self, name: &'static str) -> Option<V> {
        self.0.remove(name)
    }
    fn required(&mut self, name: &'static str) -> Result<V, HostWireError> {
        self.take(name).ok_or(HostWireError::MissingField(name))
    }
    fn string(&mut self, name: &'static str) -> Result<String, HostWireError> {
        as_string(name, self.required(name)?)
    }
    fn opt_string(&mut self, name: &'static str) -> Result<Option<String>, HostWireError> {
        self.take(name).map(|v| as_string(name, v)).transpose()
    }
    /// Nullable string: absent or null is `None`.
    fn nullable_string(&mut self, name: &'static str) -> Result<Option<String>, HostWireError> {
        match self.take(name) {
            None | Some(V::Null) => Ok(None),
            Some(v) => as_string(name, v).map(Some),
        }
    }
    fn nullable_bool(
        &mut self,
        name: &'static str,
        required: bool,
    ) -> Result<Option<bool>, HostWireError> {
        match self.take(name) {
            None if required => Err(HostWireError::MissingField(name)),
            None | Some(V::Null) => Ok(None),
            Some(V::Boolean(b)) => Ok(Some(b)),
            Some(_) => Err(HostWireError::InvalidField(name)),
        }
    }
    fn opt_strings(&mut self, name: &'static str) -> Result<Option<Vec<String>>, HostWireError> {
        match self.take(name) {
            None => Ok(None),
            Some(V::Array(items)) => items
                .into_iter()
                .map(|v| as_string(name, v))
                .collect::<Result<_, _>>()
                .map(Some),
            Some(_) => Err(HostWireError::InvalidField(name)),
        }
    }
    fn date_time(&mut self, name: &'static str) -> Result<OrchestrationDateTimeV1, HostWireError> {
        OrchestrationDateTimeV1::try_new(self.string(name)?)
            .map_err(|_| HostWireError::InvalidField(name))
    }
}

fn as_string(name: &'static str, value: V) -> Result<String, HostWireError> {
    match value {
        V::String(s) => Ok(s),
        _ => Err(HostWireError::InvalidField(name)),
    }
}

fn vocabulary<T: Copy>(
    name: &'static str,
    all: &[T],
    spell: impl Fn(T) -> &'static str,
    text: &str,
) -> Result<T, HostWireError> {
    all.iter()
        .copied()
        .find(|item| spell(*item) == text)
        .ok_or(HostWireError::InvalidField(name))
}

fn non_empty(name: &'static str, value: &str) -> Result<(), HostWireError> {
    if value.is_empty() {
        Err(HostWireError::InvalidField(name))
    } else {
        Ok(())
    }
}

/// Decodes and validates one `HostCapabilityReport` document.
pub fn host_capability_report_from_json(text: &str) -> Result<HostCapabilityReport, HostWireError> {
    let mut f = Fields(parse_object(text)?);
    let host_id = f.string("host_id")?;
    non_empty("host_id", &host_id)?;
    let probed_at = f.date_time("probed_at")?;
    let last_verified_at = f.date_time("last_verified_at")?;
    let probe_status = vocabulary(
        "probe_status",
        &HostCapabilityProbeStatus::ALL,
        HostCapabilityProbeStatus::as_str,
        &f.string("probe_status")?,
    )?;
    let validity_fingerprint = match f.required("validity_fingerprint")? {
        V::Null => None,
        v => {
            let s = as_string("validity_fingerprint", v)?;
            non_empty("validity_fingerprint", &s)?;
            Some(s)
        }
    };
    let stale_reason = vocabulary(
        "stale_reason",
        &HostCapabilityStaleReason::ALL,
        HostCapabilityStaleReason::as_str,
        &f.string("stale_reason")?,
    )?;
    let hook_coverage_class = vocabulary(
        "hook_coverage_class",
        &HostCapabilityHookCoverageClass::ALL,
        HostCapabilityHookCoverageClass::as_str,
        &f.string("hook_coverage_class")?,
    )?;
    let selected_mode = vocabulary(
        "selected_mode",
        &HostCapabilitySelectedMode::ALL,
        HostCapabilitySelectedMode::as_str,
        &f.string("selected_mode")?,
    )?;
    let mode_override = match f.take("mode_override") {
        None | Some(V::Null) => None,
        Some(V::Object(object)) => {
            let mut o = Fields(object.as_map().clone());
            let source = vocabulary(
                "mode_override.source",
                &HostCapabilityModeOverrideSource::ALL,
                HostCapabilityModeOverrideSource::as_str,
                &o.string("source")?,
            )?;
            let reason = o.string("reason")?;
            o.finish()?;
            Some(
                HostCapabilityModeOverride::new(source, reason)
                    .map_err(|_| HostWireError::InvalidField("mode_override.reason"))?,
            )
        }
        Some(_) => return Err(HostWireError::InvalidField("mode_override")),
    };
    let inactive_reason = f
        .opt_string("inactive_reason")?
        .map(|s| {
            vocabulary(
                "inactive_reason",
                &HostCapabilityInactiveReason::ALL,
                HostCapabilityInactiveReason::as_str,
                &s,
            )
        })
        .transpose()?;
    let evidence_label = f
        .opt_string("evidence_label")?
        .map(|s| {
            vocabulary(
                "evidence_label",
                &HostCapabilityEvidenceLabel::ALL,
                HostCapabilityEvidenceLabel::as_str,
                &s,
            )
        })
        .transpose()?;
    let inputs = HostCapabilityReportNonTemporalCoreInputs {
        host_id,
        host_version: f.nullable_string("host_version")?,
        probe_status,
        validity_fingerprint,
        hook_definition_digest: f.nullable_string("hook_definition_digest")?,
        relevant_config_digest: f.nullable_string("relevant_config_digest")?,
        stale_reason,
        plugin_supported: f.nullable_bool("plugin_supported", true)?,
        plugin_installed: f.nullable_bool("plugin_installed", true)?,
        manifest_path: f.nullable_string("manifest_path")?,
        supports_skills: f.nullable_bool("supports_skills", false)?,
        supports_commands: f.nullable_bool("supports_commands", false)?,
        supports_subagents: f.nullable_bool("supports_subagents", false)?,
        supports_mcp: f.nullable_bool("supports_mcp", false)?,
        hooks_supported: f.nullable_bool("hooks_supported", true)?,
        hooks_configured: f.nullable_bool("hooks_configured", true)?,
        hook_trust_required: f.nullable_bool("hook_trust_required", true)?,
        hooks_trusted: f.nullable_bool("hooks_trusted", true)?,
        hooks_enabled: f.nullable_bool("hooks_enabled", true)?,
        hooks_allowed_by_admin_policy: f.nullable_bool("hooks_allowed_by_admin_policy", true)?,
        hook_events: f.opt_strings("hook_events")?,
        blocking_hook_events: f.opt_strings("blocking_hook_events")?,
        hook_coverage_class,
        required_hook_coverage_satisfied: f
            .nullable_bool("required_hook_coverage_satisfied", true)?,
        selected_mode,
        mode_override,
        inactive_reason,
        plugin_data_path: f.nullable_string("plugin_data_path")?,
        sandbox_modes: f.opt_strings("sandbox_modes")?,
        evidence_label,
        source_claim_id: f.nullable_string("source_claim_id")?,
    };
    f.finish()?;
    let core = HostCapabilityReportNonTemporalCore::new(inputs.clone())
        .map_err(|_| HostWireError::Invalid("report"))?;
    require_derived_selection(&inputs)?;
    Ok(HostCapabilityReport::new(core, probed_at, last_verified_at))
}

/// The frozen selector, applied to the report's own facts, must yield exactly
/// the declared mode and inactive reason. Validity is current only when the
/// report itself says it is not stale.
fn require_derived_selection(
    r: &HostCapabilityReportNonTemporalCoreInputs,
) -> Result<(), HostWireError> {
    let current = r.stale_reason == HostCapabilityStaleReason::None;
    let derived =
        compose_host_capability_report_selection(&HostCapabilityReportSelectionCompositionInputs {
            report_validity_proven_current: current,
            host_id: r.host_id.clone(),
            host_version: r.host_version.clone(),
            probe_status: r.probe_status,
            validity_fingerprint: r.validity_fingerprint.clone(),
            hook_definition_digest: r.hook_definition_digest.clone(),
            relevant_config_digest: r.relevant_config_digest.clone(),
            stale_reason: r.stale_reason,
            plugin_supported: r.plugin_supported,
            plugin_installed: r.plugin_installed,
            manifest_path: r.manifest_path.clone(),
            supports_skills: r.supports_skills,
            supports_commands: r.supports_commands,
            supports_subagents: r.supports_subagents,
            supports_mcp: r.supports_mcp,
            hooks_supported: r.hooks_supported,
            hooks_configured: r.hooks_configured,
            hook_trust_required: r.hook_trust_required,
            hooks_trusted: r.hooks_trusted,
            hooks_enabled: r.hooks_enabled,
            hooks_allowed_by_admin_policy: r.hooks_allowed_by_admin_policy,
            hook_events: r.hook_events.clone(),
            blocking_hook_events: r.blocking_hook_events.clone(),
            hook_coverage_class: r.hook_coverage_class,
            required_hook_coverage_satisfied: r.required_hook_coverage_satisfied,
            mode_override: r.mode_override.clone(),
            plugin_data_path: r.plugin_data_path.clone(),
            sandbox_modes: r.sandbox_modes.clone(),
            evidence_label: r.evidence_label,
            source_claim_id: r.source_claim_id.clone(),
        });
    match derived {
        Ok(HostCapabilityReportSelectionCompositionOutcome::Selected { report }) => {
            if report.selected_mode() == r.selected_mode
                && report.inactive_reason()
                    == Some(
                        r.inactive_reason
                            .unwrap_or(HostCapabilityInactiveReason::None),
                    )
            {
                Ok(())
            } else {
                Err(HostWireError::SelectionMismatch)
            }
        }
        // Stale reports and healthy-path overrides are not uniquely derived:
        // the declared mode must still satisfy the frozen consistency rules
        // (no stale EMBEDDED; a healthy departure needs an override).
        Ok(_) => validate_selected_mode_consistency(&HostCapabilitySelectedModeConsistencyInputs {
            report_validity_proven_current: current,
            probe_status: r.probe_status,
            stale_reason: r.stale_reason,
            native_prerequisite_state: assess_native_path_prerequisites(
                HostCapabilityNativePrerequisiteInputs {
                    plugin_supported: r.plugin_supported,
                    plugin_installed: r.plugin_installed,
                    hooks_supported: r.hooks_supported,
                    hooks_configured: r.hooks_configured,
                    hook_trust_required: r.hook_trust_required,
                    hooks_trusted: r.hooks_trusted,
                    hooks_enabled: r.hooks_enabled,
                    hooks_allowed_by_admin_policy: r.hooks_allowed_by_admin_policy,
                    required_hook_coverage_satisfied: r.required_hook_coverage_satisfied,
                },
            ),
            selected_mode: r.selected_mode,
            mode_override: r.mode_override.clone(),
            inactive_reason: r.inactive_reason,
        })
        .map_err(|_| HostWireError::SelectionMismatch),
        Err(_) => Err(HostWireError::SelectionMismatch),
    }
}

fn s(value: &str) -> V {
    V::String(value.to_owned())
}
fn nullable_s(value: Option<&str>) -> V {
    value.map_or(V::Null, s)
}
fn nullable_b(value: Option<bool>) -> V {
    value.map_or(V::Null, V::Boolean)
}
fn strings(values: &[String]) -> V {
    V::Array(values.iter().map(|v| s(v)).collect())
}

/// Compact canonical JSON for one report.
pub fn host_capability_report_to_json(report: &HostCapabilityReport) -> String {
    let c = report.core();
    let mut m = Map::new();
    let mut put = |k: &str, v: V| {
        m.insert(k.to_owned(), v);
    };
    put("host_id", s(c.host_id()));
    if let Some(v) = c.host_version() {
        put("host_version", s(v));
    }
    put("probed_at", s(report.probed_at().as_str()));
    put("last_verified_at", s(report.last_verified_at().as_str()));
    put("probe_status", s(c.probe_status().as_str()));
    put("validity_fingerprint", nullable_s(c.validity_fingerprint()));
    if let Some(v) = c.hook_definition_digest() {
        put("hook_definition_digest", s(v));
    }
    if let Some(v) = c.relevant_config_digest() {
        put("relevant_config_digest", s(v));
    }
    put("stale_reason", s(c.stale_reason().as_str()));
    put("plugin_supported", nullable_b(c.plugin_supported()));
    put("plugin_installed", nullable_b(c.plugin_installed()));
    if let Some(v) = c.manifest_path() {
        put("manifest_path", s(v));
    }
    for (k, v) in [
        ("supports_skills", c.supports_skills()),
        ("supports_commands", c.supports_commands()),
        ("supports_subagents", c.supports_subagents()),
        ("supports_mcp", c.supports_mcp()),
    ] {
        if let Some(v) = v {
            put(k, V::Boolean(v));
        }
    }
    put("hooks_supported", nullable_b(c.hooks_supported()));
    put("hooks_configured", nullable_b(c.hooks_configured()));
    put("hook_trust_required", nullable_b(c.hook_trust_required()));
    put("hooks_trusted", nullable_b(c.hooks_trusted()));
    put("hooks_enabled", nullable_b(c.hooks_enabled()));
    put(
        "hooks_allowed_by_admin_policy",
        nullable_b(c.hooks_allowed_by_admin_policy()),
    );
    if let Some(v) = c.hook_events() {
        put("hook_events", strings(v));
    }
    if let Some(v) = c.blocking_hook_events() {
        put("blocking_hook_events", strings(v));
    }
    put("hook_coverage_class", s(c.hook_coverage_class().as_str()));
    put(
        "required_hook_coverage_satisfied",
        nullable_b(c.required_hook_coverage_satisfied()),
    );
    put("selected_mode", s(c.selected_mode().as_str()));
    if let Some(o) = c.mode_override() {
        let mut om = Map::new();
        om.insert("source".into(), s(o.source().as_str()));
        om.insert("reason".into(), s(o.reason()));
        put(
            "mode_override",
            V::Object(OrchestrationJsonObjectV1::new(om)),
        );
    }
    if let Some(v) = c.inactive_reason() {
        put("inactive_reason", s(v.as_str()));
    }
    if let Some(v) = c.plugin_data_path() {
        put("plugin_data_path", s(v));
    }
    if let Some(v) = c.sandbox_modes() {
        put("sandbox_modes", strings(v));
    }
    if let Some(v) = c.evidence_label() {
        put("evidence_label", s(v.as_str()));
    }
    if let Some(v) = c.source_claim_id() {
        put("source_claim_id", s(v));
    }
    V::Object(OrchestrationJsonObjectV1::new(m)).to_json()
}

fn bounded(name: &'static str, value: &str) -> Result<(), HostWireError> {
    if (1..=200).contains(&value.chars().count()) {
        Ok(())
    } else {
        Err(HostWireError::InvalidField(name))
    }
}

/// Decodes and validates one `NormalizedHostEvent` document.
pub fn normalized_host_event_from_json(text: &str) -> Result<NormalizedHostEvent, HostWireError> {
    let mut f = Fields(parse_object(text)?);
    let event_id = NormalizedHostEventId::try_new(f.string("event_id")?)
        .map_err(|_| HostWireError::InvalidField("event_id"))?;
    let event_type = vocabulary(
        "event_type",
        &NormalizedHostEventType::ALL,
        NormalizedHostEventType::as_str,
        &f.string("event_type")?,
    )?;
    let host = NormalizedHostEventHost::try_new(f.string("host")?)
        .map_err(|_| HostWireError::InvalidField("host"))?;
    let host_session_id = f.string("host_session_id")?;
    bounded("host_session_id", &host_session_id)?;
    let project_id = f.opt_string("project_id")?;
    if let Some(id) = &project_id {
        bounded("project_id", id)?;
    }
    let occurred_at = f.date_time("occurred_at")?;
    let payload = match f.take("payload") {
        None => OrchestrationJsonObjectV1::default(),
        Some(V::Object(o)) => o,
        Some(_) => return Err(HostWireError::InvalidField("payload")),
    };
    let raw_ref = match f.take("raw_ref") {
        None => None,
        Some(V::Object(o)) => {
            let mut r = Fields(o.as_map().clone());
            let ref_type = vocabulary(
                "raw_ref.ref_type",
                &[
                    NormalizedHostEventRawRefType::RepoPath,
                    NormalizedHostEventRawRefType::StateQuery,
                    NormalizedHostEventRawRefType::ArtifactId,
                    NormalizedHostEventRawRefType::Url,
                ],
                NormalizedHostEventRawRefType::as_str,
                &r.string("ref_type")?,
            )?;
            let target = r.string("target")?;
            let digest = r.opt_string("digest")?;
            let section = r.opt_string("section")?;
            r.finish()?;
            Some(
                NormalizedHostEventRawRef::try_new(ref_type, target, digest, section)
                    .map_err(|_| HostWireError::InvalidField("raw_ref.target"))?,
            )
        }
        Some(_) => return Err(HostWireError::InvalidField("raw_ref")),
    };
    let confidence = vocabulary(
        "confidence",
        &NormalizedHostEventConfidence::ALL,
        NormalizedHostEventConfidence::as_str,
        &f.string("confidence")?,
    )?;
    f.finish()?;
    NormalizedHostEvent::try_new(NormalizedHostEventInputs {
        event_id,
        event_type,
        host,
        host_session_id,
        project_id,
        occurred_at,
        payload,
        raw_ref,
        confidence,
    })
    .map_err(|_| HostWireError::Invalid("event"))
}

/// Compact canonical JSON for one event (one spool line without newline).
pub fn normalized_host_event_to_json(event: &NormalizedHostEvent) -> String {
    let mut m = Map::new();
    m.insert("event_id".into(), s(event.event_id().as_str()));
    m.insert("event_type".into(), s(event.event_type().as_str()));
    m.insert("host".into(), s(event.host().as_str()));
    m.insert("host_session_id".into(), s(event.host_session_id()));
    if let Some(id) = event.project_id() {
        m.insert("project_id".into(), s(id));
    }
    m.insert("occurred_at".into(), s(event.occurred_at().as_str()));
    m.insert("payload".into(), V::Object(event.payload().clone()));
    if let Some(r) = event.raw_ref() {
        let mut rm = Map::new();
        rm.insert("ref_type".into(), s(r.ref_type().as_str()));
        rm.insert("target".into(), s(r.target()));
        if let Some(d) = r.digest() {
            rm.insert("digest".into(), s(d));
        }
        if let Some(sec) = r.section() {
            rm.insert("section".into(), s(sec));
        }
        m.insert(
            "raw_ref".into(),
            V::Object(OrchestrationJsonObjectV1::new(rm)),
        );
    }
    m.insert("confidence".into(), s(event.confidence().as_str()));
    V::Object(OrchestrationJsonObjectV1::new(m)).to_json()
}

#[cfg(test)]
mod tests;
