//! Focused tests for the non-temporal host capability report core.

use super::*;
use crate::{
    HostCapabilityConsistencyError, HostCapabilityEvidenceLabel, HostCapabilityHookCoverageClass,
    HostCapabilityInactiveReason, HostCapabilityModeOverrideSource, HostCapabilityProbeStatus,
    HostCapabilitySelectedMode, HostCapabilityStaleReason,
    host_capability_report_core::{
        HostCapabilityReportNonTemporalCore, HostCapabilityReportNonTemporalCoreError,
        HostCapabilityReportNonTemporalCoreInputs,
    },
};

fn valid_inputs() -> HostCapabilityReportNonTemporalCoreInputs {
    HostCapabilityReportNonTemporalCoreInputs {
        host_id: "test-host".to_owned(),
        host_version: Some("1.2.3".to_owned()),
        probe_status: HostCapabilityProbeStatus::Complete,
        validity_fingerprint: Some("fp-1".to_owned()),
        hook_definition_digest: Some("hook-digest".to_owned()),
        relevant_config_digest: Some("config-digest".to_owned()),
        stale_reason: HostCapabilityStaleReason::None,
        plugin_supported: Some(true),
        plugin_installed: Some(true),
        manifest_path: Some("/manifest".to_owned()),
        supports_skills: Some(true),
        supports_commands: Some(true),
        supports_subagents: Some(true),
        supports_mcp: Some(true),
        hooks_supported: Some(true),
        hooks_configured: Some(true),
        hook_trust_required: Some(true),
        hooks_trusted: Some(true),
        hooks_enabled: Some(true),
        hooks_allowed_by_admin_policy: Some(true),
        hook_events: Some(vec!["a".to_owned(), "b".to_owned()]),
        blocking_hook_events: Some(vec!["c".to_owned()]),
        hook_coverage_class: HostCapabilityHookCoverageClass::Full,
        required_hook_coverage_satisfied: Some(true),
        selected_mode: HostCapabilitySelectedMode::Embedded,
        mode_override: None,
        inactive_reason: Some(HostCapabilityInactiveReason::None),
        plugin_data_path: Some("/data".to_owned()),
        sandbox_modes: Some(vec!["sandbox".to_owned()]),
        evidence_label: Some(HostCapabilityEvidenceLabel::VerifiedCurrentSelfFetched),
        source_claim_id: Some("C-01".to_owned()),
    }
}

fn build(inputs: HostCapabilityReportNonTemporalCoreInputs) -> HostCapabilityReportNonTemporalCore {
    HostCapabilityReportNonTemporalCore::new(inputs).expect("valid inputs must construct")
}

// --------------------------------------------------
// FIELD SET
// --------------------------------------------------

#[test]
fn all_31_non_temporal_properties_are_represented() {
    let core = build(valid_inputs());
    assert_eq!(core.host_id(), "test-host");
    assert_eq!(core.host_version(), Some("1.2.3"));
    assert_eq!(core.probe_status(), HostCapabilityProbeStatus::Complete);
    assert_eq!(core.validity_fingerprint(), Some("fp-1"));
    assert_eq!(core.hook_definition_digest(), Some("hook-digest"));
    assert_eq!(core.relevant_config_digest(), Some("config-digest"));
    assert_eq!(core.stale_reason(), HostCapabilityStaleReason::None);
    assert_eq!(core.plugin_supported(), Some(true));
    assert_eq!(core.plugin_installed(), Some(true));
    assert_eq!(core.manifest_path(), Some("/manifest"));
    assert_eq!(core.supports_skills(), Some(true));
    assert_eq!(core.supports_commands(), Some(true));
    assert_eq!(core.supports_subagents(), Some(true));
    assert_eq!(core.supports_mcp(), Some(true));
    assert_eq!(core.hooks_supported(), Some(true));
    assert_eq!(core.hooks_configured(), Some(true));
    assert_eq!(core.hook_trust_required(), Some(true));
    assert_eq!(core.hooks_trusted(), Some(true));
    assert_eq!(core.hooks_enabled(), Some(true));
    assert_eq!(core.hooks_allowed_by_admin_policy(), Some(true));
    assert_eq!(
        core.hook_events(),
        Some(&["a".to_owned(), "b".to_owned()][..])
    );
    assert_eq!(core.blocking_hook_events(), Some(&["c".to_owned()][..]));
    assert_eq!(
        core.hook_coverage_class(),
        HostCapabilityHookCoverageClass::Full
    );
    assert_eq!(core.required_hook_coverage_satisfied(), Some(true));
    assert_eq!(core.selected_mode(), HostCapabilitySelectedMode::Embedded);
    assert_eq!(core.mode_override(), None);
    assert_eq!(
        core.inactive_reason(),
        Some(HostCapabilityInactiveReason::None)
    );
    assert_eq!(core.plugin_data_path(), Some("/data"));
    assert_eq!(core.sandbox_modes(), Some(&["sandbox".to_owned()][..]));
    assert_eq!(
        core.evidence_label(),
        Some(HostCapabilityEvidenceLabel::VerifiedCurrentSelfFetched)
    );
    assert_eq!(core.source_claim_id(), Some("C-01"));
}

#[test]
fn debug_view_lists_31_fields_without_temporal_aliases() {
    let core = build(valid_inputs());
    let debug = format!("{core:?}");
    for field in [
        "host_id",
        "host_version",
        "probe_status",
        "validity_fingerprint",
        "hook_definition_digest",
        "relevant_config_digest",
        "stale_reason",
        "plugin_supported",
        "plugin_installed",
        "manifest_path",
        "supports_skills",
        "supports_commands",
        "supports_subagents",
        "supports_mcp",
        "hooks_supported",
        "hooks_configured",
        "hook_trust_required",
        "hooks_trusted",
        "hooks_enabled",
        "hooks_allowed_by_admin_policy",
        "hook_events",
        "blocking_hook_events",
        "hook_coverage_class",
        "required_hook_coverage_satisfied",
        "selected_mode",
        "mode_override",
        "inactive_reason",
        "plugin_data_path",
        "sandbox_modes",
        "evidence_label",
        "source_claim_id",
    ] {
        assert!(debug.contains(field), "debug must list {field}");
    }
}

#[test]
fn probed_at_is_absent() {
    let core = build(valid_inputs());
    let debug = format!("{core:?}");
    assert!(!debug.contains("probed_at"));
    assert!(!debug.contains("probed_timestamp"));
    assert!(!debug.contains("probe_time"));
}

#[test]
fn last_verified_at_is_absent() {
    let core = build(valid_inputs());
    let debug = format!("{core:?}");
    assert!(!debug.contains("last_verified"));
    assert!(!debug.contains("verified_at"));
    assert!(!debug.contains("verification_time"));
}

// --------------------------------------------------
// HOST ID
// --------------------------------------------------

#[test]
fn empty_host_id_is_rejected_with_typed_error() {
    let mut inputs = valid_inputs();
    inputs.host_id = String::new();
    assert_eq!(
        HostCapabilityReportNonTemporalCore::new(inputs),
        Err(HostCapabilityReportNonTemporalCoreError::EmptyHostId)
    );
}

#[test]
fn one_character_host_id_is_accepted() {
    let mut inputs = valid_inputs();
    inputs.host_id = "a".to_owned();
    assert_eq!(build(inputs).host_id(), "a");
}

#[test]
fn whitespace_only_host_id_is_accepted() {
    for id in [" ", "   ", "\t", "\n"] {
        let mut inputs = valid_inputs();
        inputs.host_id = id.to_owned();
        assert_eq!(build(inputs).host_id(), id);
    }
}

#[test]
fn accepted_host_id_is_preserved_byte_for_byte() {
    let id = "  Réason\t\r\n💾  ".to_owned();
    let expected = id.clone();
    let mut inputs = valid_inputs();
    inputs.host_id = id;
    let core = build(inputs);
    assert_eq!(core.host_id().as_bytes(), expected.as_bytes());
}

#[test]
fn host_id_is_not_restricted_to_closed_adapter_identities() {
    for id in [
        "arbitrary-host-xyz",
        "CLAUDE_CODE_EXTRA",
        "my-custom-host",
        "UPPER-lower-123",
    ] {
        let mut inputs = valid_inputs();
        inputs.host_id = id.to_owned();
        assert_eq!(build(inputs).host_id(), id);
    }
}

// --------------------------------------------------
// VALIDITY FINGERPRINT
// --------------------------------------------------

#[test]
fn none_fingerprint_is_accepted() {
    let mut inputs = valid_inputs();
    inputs.validity_fingerprint = None;
    assert_eq!(build(inputs).validity_fingerprint(), None);
}

#[test]
fn empty_fingerprint_is_rejected_with_typed_error() {
    let mut inputs = valid_inputs();
    inputs.validity_fingerprint = Some(String::new());
    assert_eq!(
        HostCapabilityReportNonTemporalCore::new(inputs),
        Err(HostCapabilityReportNonTemporalCoreError::EmptyValidityFingerprint)
    );
}

#[test]
fn single_char_fingerprint_is_accepted() {
    let mut inputs = valid_inputs();
    inputs.validity_fingerprint = Some("x".to_owned());
    assert_eq!(build(inputs).validity_fingerprint(), Some("x"));
}

#[test]
fn whitespace_fingerprint_is_accepted() {
    let mut inputs = valid_inputs();
    inputs.validity_fingerprint = Some(" ".to_owned());
    assert_eq!(build(inputs).validity_fingerprint(), Some(" "));
}

#[test]
fn accepted_fingerprint_is_preserved_byte_for_byte() {
    let fp = "  fp\t\r\n💾  ".to_owned();
    let expected = fp.clone();
    let mut inputs = valid_inputs();
    inputs.validity_fingerprint = Some(fp);
    let core = build(inputs);
    assert_eq!(
        core.validity_fingerprint().unwrap().as_bytes(),
        expected.as_bytes()
    );
}

// --------------------------------------------------
// OTHER NULLABLE STRINGS
// --------------------------------------------------

#[test]
fn empty_string_accepted_for_nullable_strings_without_min_length() {
    let mut inputs = valid_inputs();
    inputs.host_version = Some(String::new());
    inputs.hook_definition_digest = Some(String::new());
    inputs.relevant_config_digest = Some(String::new());
    inputs.manifest_path = Some(String::new());
    inputs.plugin_data_path = Some(String::new());
    inputs.source_claim_id = Some(String::new());
    let core = build(inputs);
    assert_eq!(core.host_version(), Some(""));
    assert_eq!(core.hook_definition_digest(), Some(""));
    assert_eq!(core.relevant_config_digest(), Some(""));
    assert_eq!(core.manifest_path(), Some(""));
    assert_eq!(core.plugin_data_path(), Some(""));
    assert_eq!(core.source_claim_id(), Some(""));
}

#[test]
fn nullable_string_values_are_preserved_exactly() {
    let mut inputs = valid_inputs();
    inputs.host_version = Some("  v\t💾".to_owned());
    inputs.hook_definition_digest = Some("  d\t💾".to_owned());
    inputs.relevant_config_digest = Some("  c\t💾".to_owned());
    inputs.manifest_path = Some("  m\t💾".to_owned());
    inputs.plugin_data_path = Some("  p\t💾".to_owned());
    inputs.source_claim_id = Some("  s\t💾".to_owned());
    let core = build(inputs);
    assert_eq!(core.host_version(), Some("  v\t💾"));
    assert_eq!(core.hook_definition_digest(), Some("  d\t💾"));
    assert_eq!(core.relevant_config_digest(), Some("  c\t💾"));
    assert_eq!(core.manifest_path(), Some("  m\t💾"));
    assert_eq!(core.plugin_data_path(), Some("  p\t💾"));
    assert_eq!(core.source_claim_id(), Some("  s\t💾"));
}

// --------------------------------------------------
// ARRAYS
// --------------------------------------------------

#[test]
fn hook_events_preserve_ordering() {
    let mut inputs = valid_inputs();
    inputs.hook_events = Some(vec!["c".to_owned(), "a".to_owned(), "b".to_owned()]);
    assert_eq!(
        build(inputs).hook_events(),
        Some(&["c".to_owned(), "a".to_owned(), "b".to_owned()][..])
    );
}

#[test]
fn hook_events_preserve_duplicates() {
    let mut inputs = valid_inputs();
    inputs.hook_events = Some(vec!["A".to_owned(), "A".to_owned(), "b".to_owned()]);
    assert_eq!(
        build(inputs).hook_events(),
        Some(&["A".to_owned(), "A".to_owned(), "b".to_owned()][..])
    );
}

#[test]
fn hook_events_accept_empty_string_entries() {
    let mut inputs = valid_inputs();
    inputs.hook_events = Some(vec!["".to_owned()]);
    assert_eq!(build(inputs).hook_events(), Some(&["".to_owned()][..]));

    let mut inputs = valid_inputs();
    inputs.hook_events = Some(vec!["A".to_owned(), "".to_owned(), "A".to_owned()]);
    assert_eq!(
        build(inputs).hook_events(),
        Some(&["A".to_owned(), "".to_owned(), "A".to_owned()][..])
    );
}

#[test]
fn blocking_hook_events_preserve_ordering_and_duplicates() {
    let mut inputs = valid_inputs();
    inputs.blocking_hook_events = Some(vec!["z".to_owned(), "a".to_owned(), "z".to_owned()]);
    assert_eq!(
        build(inputs).blocking_hook_events(),
        Some(&["z".to_owned(), "a".to_owned(), "z".to_owned()][..])
    );
}

#[test]
fn sandbox_modes_preserve_ordering_and_duplicates() {
    let mut inputs = valid_inputs();
    inputs.sandbox_modes = Some(vec!["b".to_owned(), "a".to_owned(), "b".to_owned()]);
    assert_eq!(
        build(inputs).sandbox_modes(),
        Some(&["b".to_owned(), "a".to_owned(), "b".to_owned()][..])
    );
}

#[test]
fn empty_string_array_items_remain_intact_everywhere() {
    let mut inputs = valid_inputs();
    inputs.hook_events = Some(vec!["".to_owned(), "x".to_owned()]);
    inputs.blocking_hook_events = Some(vec!["".to_owned()]);
    inputs.sandbox_modes = Some(vec!["".to_owned(), "".to_owned()]);
    let core = build(inputs);
    assert_eq!(
        core.hook_events(),
        Some(&["".to_owned(), "x".to_owned()][..])
    );
    assert_eq!(core.blocking_hook_events(), Some(&["".to_owned()][..]));
    assert_eq!(
        core.sandbox_modes(),
        Some(&["".to_owned(), "".to_owned()][..])
    );
}

#[test]
fn no_array_sorting_dedup_or_filtering_occurs() {
    let mut inputs = valid_inputs();
    let events = vec![
        "b".to_owned(),
        "".to_owned(),
        "a".to_owned(),
        "b".to_owned(),
    ];
    let expected = events.clone();
    inputs.hook_events = Some(events);
    let core = build(inputs);
    assert_eq!(core.hook_events(), Some(&expected[..]));
}

#[test]
fn absent_vs_present_arrays_are_preserved() {
    let mut absent = valid_inputs();
    absent.hook_events = None;
    absent.blocking_hook_events = None;
    absent.sandbox_modes = None;
    let core = build(absent);
    assert_eq!(core.hook_events(), None);
    assert_eq!(core.blocking_hook_events(), None);
    assert_eq!(core.sandbox_modes(), None);

    let mut present_empty = valid_inputs();
    present_empty.hook_events = Some(Vec::new());
    present_empty.blocking_hook_events = Some(Vec::new());
    present_empty.sandbox_modes = Some(Vec::new());
    let core = build(present_empty);
    assert_eq!(core.hook_events(), Some(&[][..]));
    assert_eq!(core.blocking_hook_events(), Some(&[][..]));
    assert_eq!(core.sandbox_modes(), Some(&[][..]));
}

// --------------------------------------------------
// EXISTING VOCABULARY REUSE
// --------------------------------------------------

#[test]
fn existing_vocabularies_are_reused_not_duplicated() {
    let mut inputs = valid_inputs();
    inputs.probe_status = HostCapabilityProbeStatus::Partial;
    inputs.stale_reason = HostCapabilityStaleReason::ValidityFingerprintChanged;
    inputs.hook_coverage_class = HostCapabilityHookCoverageClass::Unknown;
    inputs.selected_mode = HostCapabilitySelectedMode::Supervised;
    inputs.inactive_reason = Some(HostCapabilityInactiveReason::HooksDisabled);
    inputs.evidence_label = Some(HostCapabilityEvidenceLabel::Assumption);
    // PARTIAL avoids COMPLETE consistency for the healthy-true facts.
    let core = build(inputs);
    assert_eq!(core.probe_status(), HostCapabilityProbeStatus::Partial);
    assert_eq!(
        core.stale_reason(),
        HostCapabilityStaleReason::ValidityFingerprintChanged
    );
    assert_eq!(
        core.hook_coverage_class(),
        HostCapabilityHookCoverageClass::Unknown
    );
    assert_eq!(core.selected_mode(), HostCapabilitySelectedMode::Supervised);
    assert_eq!(
        core.inactive_reason(),
        Some(HostCapabilityInactiveReason::HooksDisabled)
    );
    assert_eq!(
        core.evidence_label(),
        Some(HostCapabilityEvidenceLabel::Assumption)
    );
}

#[test]
fn mode_override_type_is_reused() {
    let value = HostCapabilityModeOverride::new(
        HostCapabilityModeOverrideSource::Admin,
        "admin reason".to_owned(),
    )
    .unwrap();
    let mut inputs = valid_inputs();
    inputs.mode_override = Some(value);
    let core = build(inputs);
    let stored = core.mode_override().expect("override stored");
    assert_eq!(stored.source(), HostCapabilityModeOverrideSource::Admin);
    assert_eq!(stored.reason(), "admin reason");
}

// --------------------------------------------------
// COMPLETE CONSISTENCY
// --------------------------------------------------

#[test]
fn constructor_reuses_complete_probe_validator_rule1() {
    let mut inputs = valid_inputs();
    inputs.plugin_supported = Some(false);
    inputs.plugin_installed = Some(true);
    assert_eq!(
        HostCapabilityReportNonTemporalCore::new(inputs),
        Err(
            HostCapabilityReportNonTemporalCoreError::CompleteProbeConsistency(
                HostCapabilityConsistencyError::PluginInstalledRequiresPluginSupported
            )
        )
    );
}

#[test]
fn constructor_reuses_complete_probe_validator_rule2() {
    let mut inputs = valid_inputs();
    inputs.hooks_supported = Some(false);
    inputs.hooks_configured = Some(true);
    // Keep rule 1 satisfied and isolate rule 2.
    inputs.plugin_installed = Some(false);
    assert_eq!(
        HostCapabilityReportNonTemporalCore::new(inputs),
        Err(
            HostCapabilityReportNonTemporalCoreError::CompleteProbeConsistency(
                HostCapabilityConsistencyError::HooksConfiguredRequiresHooksSupported
            )
        )
    );
}

#[test]
fn constructor_reuses_complete_probe_validator_rule3() {
    let mut inputs = valid_inputs();
    inputs.hooks_supported = Some(false);
    inputs.hooks_configured = Some(false);
    inputs.hooks_enabled = Some(true);
    inputs.plugin_installed = Some(false);
    assert_eq!(
        HostCapabilityReportNonTemporalCore::new(inputs),
        Err(
            HostCapabilityReportNonTemporalCoreError::CompleteProbeConsistency(
                HostCapabilityConsistencyError::HooksEnabledRequiresHooksSupported
            )
        )
    );
}

#[test]
fn constructor_reuses_complete_probe_validator_rule4() {
    let mut inputs = valid_inputs();
    inputs.plugin_installed = Some(false);
    inputs.hooks_configured = Some(false);
    inputs.hooks_enabled = Some(false);
    inputs.hook_trust_required = Some(true);
    inputs.hooks_trusted = None;
    assert_eq!(
        HostCapabilityReportNonTemporalCore::new(inputs),
        Err(
            HostCapabilityReportNonTemporalCoreError::CompleteProbeConsistency(
                HostCapabilityConsistencyError::TrustRequiredRequiresKnownTrustedState
            )
        )
    );
}

#[test]
fn trust_required_with_trusted_false_is_consistent() {
    let mut inputs = valid_inputs();
    inputs.hooks_trusted = Some(false);
    let core = build(inputs);
    assert_eq!(core.hooks_trusted(), Some(false));
}

#[test]
fn underlying_consistency_error_is_deterministically_wrapped() {
    let mut inputs = valid_inputs();
    inputs.plugin_supported = None;
    inputs.plugin_installed = Some(true);
    let first = HostCapabilityReportNonTemporalCore::new(inputs.clone());
    let second = HostCapabilityReportNonTemporalCore::new(inputs);
    assert_eq!(first, second);
    assert_eq!(
        first,
        Err(
            HostCapabilityReportNonTemporalCoreError::CompleteProbeConsistency(
                HostCapabilityConsistencyError::PluginInstalledRequiresPluginSupported
            )
        )
    );
}

#[test]
fn healthy_complete_probe_is_accepted() {
    let core = build(valid_inputs());
    assert_eq!(core.probe_status(), HostCapabilityProbeStatus::Complete);
}

// --------------------------------------------------
// PARTIAL / FAILED
// --------------------------------------------------

#[test]
fn partial_does_not_impose_complete_rules() {
    let mut inputs = valid_inputs();
    inputs.probe_status = HostCapabilityProbeStatus::Partial;
    inputs.plugin_supported = Some(false);
    inputs.plugin_installed = Some(true);
    inputs.hooks_supported = Some(false);
    inputs.hooks_configured = Some(true);
    inputs.hooks_enabled = Some(true);
    inputs.hook_trust_required = Some(true);
    inputs.hooks_trusted = None;
    let core = build(inputs);
    assert_eq!(core.probe_status(), HostCapabilityProbeStatus::Partial);
}

#[test]
fn failed_does_not_impose_complete_rules() {
    let mut inputs = valid_inputs();
    inputs.probe_status = HostCapabilityProbeStatus::Failed;
    inputs.plugin_supported = Some(false);
    inputs.plugin_installed = Some(true);
    inputs.hooks_supported = Some(false);
    inputs.hooks_configured = Some(true);
    inputs.hooks_enabled = Some(true);
    inputs.hook_trust_required = Some(true);
    inputs.hooks_trusted = None;
    let core = build(inputs);
    assert_eq!(core.probe_status(), HostCapabilityProbeStatus::Failed);
}

// --------------------------------------------------
// SELECTED MODE STORAGE
// --------------------------------------------------

#[test]
fn selected_mode_is_stored_exactly_without_inference() {
    for mode in [
        HostCapabilitySelectedMode::Embedded,
        HostCapabilitySelectedMode::Hybrid,
        HostCapabilitySelectedMode::Supervised,
    ] {
        let mut inputs = valid_inputs();
        inputs.probe_status = HostCapabilityProbeStatus::Partial;
        inputs.selected_mode = mode;
        assert_eq!(build(inputs).selected_mode(), mode);
    }
}

#[test]
fn selected_mode_survives_unhealthy_complete_facts() {
    // COMPLETE + unknown trust would fail, so use PARTIAL to prove storage
    // is independent of health.
    let mut inputs = valid_inputs();
    inputs.probe_status = HostCapabilityProbeStatus::Partial;
    inputs.selected_mode = HostCapabilitySelectedMode::Supervised;
    inputs.hooks_supported = Some(false);
    assert_eq!(
        build(inputs).selected_mode(),
        HostCapabilitySelectedMode::Supervised
    );
}

// --------------------------------------------------
// MODE OVERRIDE / OTHER STORAGE
// --------------------------------------------------

#[test]
fn valid_mode_override_is_preserved_exactly() {
    for source in HostCapabilityModeOverrideSource::ALL {
        let value = HostCapabilityModeOverride::new(source, "  reason 💾 ".to_owned()).unwrap();
        let mut inputs = valid_inputs();
        inputs.mode_override = Some(value);
        let core = build(inputs);
        let stored = core.mode_override().unwrap();
        assert_eq!(stored.source(), source);
        assert_eq!(stored.reason(), "  reason 💾 ");
    }
}

#[test]
fn inactive_reason_is_preserved_as_supplied() {
    for reason in [
        HostCapabilityInactiveReason::None,
        HostCapabilityInactiveReason::PluginNotInstalled,
        HostCapabilityInactiveReason::HooksUnsupported,
        HostCapabilityInactiveReason::HooksUntrusted,
        HostCapabilityInactiveReason::Unknown,
        HostCapabilityInactiveReason::ModeOverride,
    ] {
        let mut inputs = valid_inputs();
        inputs.probe_status = HostCapabilityProbeStatus::Partial;
        inputs.inactive_reason = Some(reason);
        assert_eq!(build(inputs).inactive_reason(), Some(reason));
    }
    let mut inputs = valid_inputs();
    inputs.inactive_reason = None;
    assert_eq!(build(inputs).inactive_reason(), None);
}

#[test]
fn evidence_label_is_preserved_as_supplied() {
    for label in HostCapabilityEvidenceLabel::ALL {
        let mut inputs = valid_inputs();
        inputs.evidence_label = Some(label);
        assert_eq!(build(inputs).evidence_label(), Some(label));
    }
    let mut inputs = valid_inputs();
    inputs.evidence_label = None;
    assert_eq!(build(inputs).evidence_label(), None);
}

// --------------------------------------------------
// BOUNDARY: purity and predecessor preservation
// --------------------------------------------------

#[test]
fn construction_is_pure_and_deterministic() {
    let first = build(valid_inputs());
    let second = build(valid_inputs());
    assert_eq!(first, second);
    assert_eq!(first.host_id(), second.host_id());
}

#[test]
fn construction_needs_no_filesystem_or_environment() {
    // If construction inspected the filesystem/environment it could not
    // succeed with purely synthetic caller-supplied values in a bare test.
    let core = build(valid_inputs());
    assert_eq!(core.host_id(), "test-host");
    assert_eq!(core.selected_mode(), HostCapabilitySelectedMode::Embedded);
}

#[test]
fn no_fingerprint_calculation_or_comparison_occurs() {
    let mut inputs = valid_inputs();
    inputs.validity_fingerprint = Some("fp-unrelated".to_owned());
    inputs.hook_definition_digest = Some("other".to_owned());
    inputs.relevant_config_digest = Some("other".to_owned());
    let core = build(inputs);
    assert_eq!(core.validity_fingerprint(), Some("fp-unrelated"));
    assert_eq!(core.hook_definition_digest(), Some("other"));
    assert_eq!(core.relevant_config_digest(), Some("other"));
}

#[test]
fn core_does_not_recompute_inactive_reason_or_derive_mode() {
    // Caller supplies a combination where the inactive-reason policy would
    // return HooksDisabled, but the core must preserve the supplied reason
    // and mode untouched.
    let mut inputs = valid_inputs();
    inputs.probe_status = HostCapabilityProbeStatus::Partial;
    inputs.hooks_enabled = Some(false);
    inputs.inactive_reason = Some(HostCapabilityInactiveReason::ModeOverride);
    inputs.selected_mode = HostCapabilitySelectedMode::Hybrid;
    let core = build(inputs);
    assert_eq!(
        core.inactive_reason(),
        Some(HostCapabilityInactiveReason::ModeOverride)
    );
    assert_eq!(core.selected_mode(), HostCapabilitySelectedMode::Hybrid);
}

#[test]
fn predecessors_remain_reusable_and_unchanged() {
    // A3-012 validator still enforces rule 1 through its own entry point.
    assert_eq!(
        crate::validate_complete_probe_consistency(crate::HostCapabilityConsistencyInputs {
            probe_status: HostCapabilityProbeStatus::Complete,
            plugin_supported: Some(false),
            plugin_installed: Some(true),
            hooks_supported: Some(true),
            hooks_configured: Some(true),
            hooks_enabled: Some(true),
            hook_trust_required: Some(false),
            hooks_trusted: None,
        }),
        Err(HostCapabilityConsistencyError::PluginInstalledRequiresPluginSupported)
    );
    // A3-010 freshness entry points remain independent.
    assert_eq!(
        crate::freshness_disposition(true),
        crate::HostCapabilityFreshnessDisposition::ReuseReport
    );
    assert!(crate::is_embedded_eligible(
        true,
        HostCapabilityProbeStatus::Complete,
        HostCapabilityStaleReason::None,
        true
    ));
    // A3-011 inactive-reason policy remains independent.
    assert_eq!(
        crate::native_path_inactive_reason(crate::HostCapabilityInactiveReasonInputs {
            plugin_installed: Some(false),
            hooks_supported: None,
            hooks_configured: None,
            hook_trust_required: None,
            hooks_trusted: None,
            hooks_enabled: None,
            hooks_allowed_by_admin_policy: None,
            required_hook_coverage_satisfied: None,
            mode_override_present: false,
        }),
        HostCapabilityInactiveReason::PluginNotInstalled
    );
    // A3-013 native prerequisite assessment remains independent.
    assert_eq!(
        crate::assess_native_path_prerequisites(crate::HostCapabilityNativePrerequisiteInputs {
            plugin_supported: Some(true),
            plugin_installed: Some(true),
            hooks_supported: Some(true),
            hooks_configured: Some(true),
            hook_trust_required: Some(false),
            hooks_trusted: None,
            hooks_enabled: Some(true),
            hooks_allowed_by_admin_policy: Some(true),
            required_hook_coverage_satisfied: Some(true),
        }),
        crate::HostCapabilityNativePrerequisiteState::Satisfied
    );
}

#[test]
fn core_clone_preserves_all_values() {
    let core = build(valid_inputs());
    let cloned = core.clone();
    assert_eq!(core, cloned);
    assert_eq!(cloned.host_id(), "test-host");
}
