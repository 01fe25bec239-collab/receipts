use super::*;
use crate::{
    HostCapabilityHookCoverageClass as Coverage, HostCapabilityInactiveReason as Reason,
    HostCapabilityModeOverrideSource, HostCapabilityModeSelectionIndeterminacy as Gap,
    HostCapabilityProbeStatus as Probe, HostCapabilityReportNonTemporalCore,
    HostCapabilityReportSelectionCompositionOutcome as Selection,
    HostCapabilitySelectedMode as Mode, HostId,
    host_capability_observation::{
        HostCapabilityObservationError as Error, HostCapabilityObservationSource as Source,
        HostCapabilityObservationValue as Value, HostConfigurationObservation,
    },
};
use std::cell::Cell;

fn request(host: HostId) -> HostCapabilityReportRefreshRequest {
    HostCapabilityReportRefreshRequest {
        observation: HostCapabilityObservationRequest {
            host,
            project_root: "/unused-refresh-context".into(),
        },
        mode_override: None,
    }
}

fn unknown() -> HostCapabilityObservation {
    HostCapabilityObservation {
        host: HostId::Codex,
        plugin_supported: Value::Unknown,
        plugin_installed: Value::Unknown,
        plugins: Err(Error::Unsupported),
        hook_definition_identity: Value::Unknown,
        hooks_supported: Value::Unknown,
        hooks_configured: Value::Unknown,
        hook_trust_required: Value::Unknown,
        hooks_trusted: Value::Unknown,
        hooks_enabled: Value::Unknown,
        hooks_allowed_by_admin_policy: Value::Unknown,
        hook_coverage_class: Coverage::Unknown,
        required_hook_coverage_satisfied: Value::Unknown,
        relevant_config_identity: Value::Unknown,
        configurations: vec![HostConfigurationObservation {
            source: Source::CodexProjectConfig,
            result: Err(Error::InvalidToml),
        }],
    }
}

fn healthy_facts() -> HostCapabilityObservation {
    HostCapabilityObservation {
        plugin_supported: Value::Known(true),
        plugin_installed: Value::Known(true),
        hooks_supported: Value::Known(true),
        hooks_configured: Value::Known(true),
        hook_trust_required: Value::Known(true),
        hooks_trusted: Value::Known(true),
        hooks_enabled: Value::Known(true),
        hooks_allowed_by_admin_policy: Value::Known(true),
        required_hook_coverage_satisfied: Value::Known(true),
        hook_coverage_class: Coverage::Full,
        ..unknown()
    }
}

fn selected(outcome: &HostCapabilityReportRefreshOutcome) -> &HostCapabilityReportNonTemporalCore {
    let Ok(Selection::Selected { report }) = &outcome.report_selection else {
        panic!("expected selected report: {outcome:?}");
    };
    report
}

fn injected(
    observation: &HostCapabilityObservation,
    mode_override: Option<HostCapabilityModeOverride>,
) -> HostCapabilityReportRefreshOutcome {
    let request = HostCapabilityReportRefreshRequest {
        mode_override,
        ..request(observation.host)
    };
    let result = refresh_with_observer(&request, |_| observation.clone());
    assert_eq!(result.observation, *observation);
    let inputs = observation.report_selection_inputs(
        true,
        HostCapabilityStaleReason::None,
        request.mode_override.clone(),
    );
    assert!(inputs.report_validity_proven_current);
    assert_eq!(inputs.stale_reason, HostCapabilityStaleReason::None);
    assert_eq!(inputs.probe_status, Probe::Partial);
    assert_eq!(inputs.validity_fingerprint, None);
    assert_eq!(
        result.report_selection,
        compose_host_capability_report_selection(&inputs)
    );
    assert_eq!(
        result,
        refresh_with_observer(&request, |_| observation.clone())
    );
    if let Ok(Selection::Selected { report }) = &result.report_selection {
        assert_eq!(report.probe_status(), Probe::Partial);
        assert_eq!(report.stale_reason(), HostCapabilityStaleReason::None);
        assert_eq!(report.validity_fingerprint(), None);
        assert_ne!(report.selected_mode(), Mode::Embedded);
    }
    result
}

#[test]
fn production_delegates_to_real_observation_engine() {
    // Headless exercises the actual public call without machine-dependent I/O.
    let request = request(HostId::Headless);
    let result = refresh_host_capability_report(&request);
    assert_eq!(
        result.observation,
        observe_host_capabilities(&request.observation)
    );
    assert_eq!(result.observation.host, HostId::Headless);
    assert!(result.observation.configurations.is_empty());
    assert_eq!(result.observation.plugins, Err(Error::Unsupported));
    assert_eq!(selected(&result).selected_mode(), Mode::Supervised);
    assert_eq!(
        selected(&result).stale_reason(),
        HostCapabilityStaleReason::None
    );
    assert_eq!(selected(&result).probe_status(), Probe::Partial);
    assert_eq!(selected(&result).validity_fingerprint(), None);
}

#[test]
fn repeated_refresh_observes_again_and_uses_new_facts_without_fingerprint() {
    let calls = Cell::new(0);
    let request = request(HostId::Codex);
    let provider = |received: &HostCapabilityObservationRequest| {
        assert_eq!(received.host, request.observation.host);
        assert_eq!(received.project_root, request.observation.project_root);
        calls.set(calls.get() + 1);
        let mut observation = unknown();
        if calls.get() == 2 {
            observation.plugin_installed = Value::Known(false);
        }
        observation
    };
    let first = refresh_with_observer(&request, provider);
    let second = refresh_with_observer(&request, provider);
    assert_eq!(calls.get(), 2);
    assert_eq!(selected(&first).inactive_reason(), Some(Reason::Unknown));
    assert_eq!(
        selected(&second).inactive_reason(),
        Some(Reason::PluginNotInstalled)
    );
    assert_eq!(selected(&first).validity_fingerprint(), None);
    assert_eq!(selected(&second).validity_fingerprint(), None);
    assert_ne!(first, second);
}

#[test]
fn fresh_unknowns_and_observation_errors_are_preserved_without_override() {
    let result = injected(&unknown(), None);
    let report = selected(&result);
    assert_eq!(report.selected_mode(), Mode::Supervised);
    assert_eq!(report.inactive_reason(), Some(Reason::Unknown));
    assert_eq!(report.plugin_supported(), None);
    assert_eq!(report.plugin_installed(), None);
    assert_eq!(report.hooks_supported(), None);
    assert_eq!(report.hooks_configured(), None);
    assert_eq!(report.hook_trust_required(), None);
    assert_eq!(report.hooks_trusted(), None);
    assert_eq!(report.hooks_enabled(), None);
    assert_eq!(report.hooks_allowed_by_admin_policy(), None);
    assert_eq!(report.required_hook_coverage_satisfied(), None);
    assert_eq!(report.mode_override(), None);
    assert_eq!(result.observation.plugins, Err(Error::Unsupported));
    assert_eq!(
        result.observation.configurations[0].result,
        Err(Error::InvalidToml)
    );
}

#[test]
fn known_failures_follow_existing_policy_with_unknowns_retained() {
    for (field, reason) in [
        (0, Reason::PluginNotInstalled),
        (1, Reason::HooksDisabled),
        (2, Reason::HooksUntrusted),
    ] {
        let mut observation = unknown();
        match field {
            0 => observation.plugin_installed = Value::Known(false),
            1 => observation.hooks_enabled = Value::Known(false),
            _ => {
                observation.hook_trust_required = Value::Known(true);
                observation.hooks_trusted = Value::Known(false);
            }
        }
        let result = injected(&observation, None);
        assert_eq!(selected(&result).selected_mode(), Mode::Supervised);
        assert_eq!(selected(&result).inactive_reason(), Some(reason));
        assert_eq!(selected(&result).hooks_allowed_by_admin_policy(), None);
    }
}

#[test]
fn fresh_partial_even_with_healthy_facts_remains_indeterminate() {
    assert_eq!(
        injected(&healthy_facts(), None).report_selection,
        Ok(Selection::Indeterminate {
            cause: Gap::NoUniqueFallbackRule,
            inactive_reason: Reason::None,
        })
    );
    let observation = HostCapabilityObservation {
        required_hook_coverage_satisfied: Value::Known(false),
        hook_coverage_class: Coverage::Partial,
        ..healthy_facts()
    };
    assert_eq!(
        injected(&observation, None).report_selection,
        Ok(Selection::Indeterminate {
            cause: Gap::CoverageDoesNotDetermineFallback,
            inactive_reason: Reason::InsufficientCoverage,
        })
    );
}

#[test]
fn unknown_codex_trust_or_admin_alone_prevents_embedded() {
    for trust_unknown in [true, false] {
        let mut observation = healthy_facts();
        if trust_unknown {
            observation.hooks_trusted = Value::Unknown;
        } else {
            observation.hooks_allowed_by_admin_policy = Value::Unknown;
        }
        let result = injected(&observation, None);
        assert_eq!(selected(&result).selected_mode(), Mode::Supervised);
        assert_eq!(selected(&result).inactive_reason(), Some(Reason::Unknown));
        if trust_unknown {
            assert_eq!(selected(&result).hooks_trusted(), None);
        } else {
            assert_eq!(selected(&result).hooks_allowed_by_admin_policy(), None);
        }
    }
}

#[test]
fn overrides_preserve_source_and_reason_without_interpreting_target_mode() {
    for source in HostCapabilityModeOverrideSource::ALL {
        let mode_override =
            HostCapabilityModeOverride::new(source, " EMBEDDED\t💾 ".into()).unwrap();
        let result = injected(&unknown(), Some(mode_override.clone()));
        assert_eq!(selected(&result).mode_override(), Some(&mode_override));
        assert_eq!(selected(&result).selected_mode(), Mode::Supervised);
        assert_eq!(
            selected(&result).inactive_reason(),
            Some(Reason::ModeOverride)
        );
        assert_eq!(
            injected(&healthy_facts(), Some(mode_override)).report_selection,
            Ok(Selection::Indeterminate {
                cause: Gap::NoUniqueFallbackRule,
                inactive_reason: Reason::ModeOverride,
            })
        );
    }
}

#[test]
fn reprobe_is_unreachable_only_because_this_invocation_establishes_freshness() {
    let observation = unknown();
    let mut inputs =
        observation.report_selection_inputs(false, HostCapabilityStaleReason::None, None);
    assert_eq!(inputs.validity_fingerprint, None);
    // None is NOT matching evidence: the accepted policy still demands reprobe.
    assert_eq!(
        compose_host_capability_report_selection(&inputs),
        Ok(Selection::ReprobeRequired)
    );
    let result = injected(&observation, None);
    assert!(matches!(
        result.report_selection,
        Ok(Selection::Selected { .. })
    ));
    inputs.report_validity_proven_current = true;
    assert_eq!(
        result.report_selection,
        compose_host_capability_report_selection(&inputs)
    );
}

#[test]
fn production_surface_only_delegates_and_has_no_io_parser_or_cache() {
    let source = include_str!("host_capability_report_refresh.rs");
    let body = source
        .split("pub fn refresh_host_capability_report(")
        .nth(1)
        .unwrap();
    assert!(body.contains("refresh_with_observer(request, observe_host_capabilities)"));
    assert!(body.contains("let observation = observe(&request.observation);"));
    assert!(body.contains("observation.report_selection_inputs("));
    assert!(body.contains("compose_host_capability_report_selection(&inputs)"));
    for forbidden in [
        "std::fs",
        "std::process",
        "std::env",
        "std::net",
        "serde",
        "toml::",
        "Command",
        "SystemTime",
        "Instant",
        "fingerprint",
        "digest",
        "cached",
        "unsafe",
        "NormalizedHostEvent",
        "raw_ref",
        "WorktreeRemove",
        "HostCapabilityReportNonTemporalCore::new",
        "select_host_capability_mode(",
    ] {
        assert!(
            !body.contains(forbidden),
            "refresh must delegate: {forbidden}"
        );
    }
}

#[test]
fn composition_error_and_observation_are_both_preserved() {
    let observation = HostCapabilityObservation {
        plugin_supported: Value::Unknown,
        ..healthy_facts()
    };
    let result = injected(&observation, None);
    assert_eq!(result.observation, observation);
    assert_eq!(
        result.report_selection,
        Err(
            HostCapabilityReportSelectionCompositionError::ModeSelection(
                crate::HostCapabilityModeSelectionError::ConservativeFallbackHasNoInactiveReason,
            )
        )
    );
}
