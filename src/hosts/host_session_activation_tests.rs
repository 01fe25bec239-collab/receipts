use super::*;
use crate::{
    HostCapabilityHookCoverageClass as Coverage, HostCapabilityInactiveReason as Reason,
    HostCapabilityModeOverrideSource, HostCapabilityModeSelectionError,
    HostCapabilityModeSelectionIndeterminacy as Gap, HostCapabilityProbeStatus as Probe,
    HostCapabilityReportNonTemporalCoreInputs, HostCapabilityReportSelectionCompositionError,
    HostCapabilityReportSelectionCompositionOutcome as Selection,
    HostCapabilitySelectedMode as Mode, HostCapabilityStaleReason as Stale,
    compose_host_capability_report_selection,
    host_capability_observation::{
        HostCapabilityObservation, HostCapabilityObservationValue as Value,
    },
};
use std::{cell::Cell, future::Ready};

// Placeholder contracts are test-only. Any operation other than id is a defect.
struct Adapter(HostId);
impl HostAdapter for Adapter {
    type DetectOutcome = ();
    type InstallPlan = ();
    type InstallOutcome = ();
    type CoreHandle = ();
    type NormalizedHostEvent = ();
    type EmitOutcome = ();
    type CoreView = ();
    type PresentOutcome = ();
    type UserPrompt = ();
    type UserResponse = ();
    type UserInputPending = Ready<()>;
    type HostCapabilityReport = ();
    type ShutdownReason = ();
    type ShutdownOutcome = ();
    fn id(&self) -> HostId {
        self.0
    }
    fn detect(&self) {
        panic!("unbound detect")
    }
    fn install(&self, _: &()) {
        panic!("unbound install")
    }
    fn start(&self) {
        panic!("unbound start")
    }
    fn emit(&self, _: &()) {
        panic!("unbound emit")
    }
    fn present(&self, _: &()) {
        panic!("unbound present")
    }
    fn request_user_input(&mut self, _: ()) -> Ready<()> {
        panic!("unbound input")
    }
    fn capabilities(&self) {
        panic!("unbound capabilities")
    }
    fn shutdown(self, _: ()) {
        panic!("unbound shutdown")
    }
}

fn request() -> HostSessionActivationRequest<'static> {
    HostSessionActivationRequest {
        detection_signals: HostDetectionSignals::NONE,
        explicit_host_override: None,
        project_root: "/unused-session-context".into(),
        mode_override: None,
        cached_report: None,
    }
}

fn unknown(host: HostId) -> HostCapabilityObservation {
    // Existing Headless observation has no physical I/O. Replace only identity
    // for synthetic tests; this does not claim any interactive-host evidence.
    HostCapabilityObservation {
        host,
        ..crate::host_capability_observation::observe_host_capabilities(
            &HostCapabilityObservationRequest {
                host: HostId::Headless,
                project_root: request().project_root,
            },
        )
    }
}

fn refreshed(
    observation: HostCapabilityObservation,
    mode_override: Option<HostCapabilityModeOverride>,
) -> HostCapabilityReportRefreshOutcome {
    let inputs = observation.report_selection_inputs(true, Stale::None, mode_override);
    HostCapabilityReportRefreshOutcome {
        report_selection: compose_host_capability_report_selection(&inputs),
        observation,
    }
}

fn report(outcome: &HostCapabilityReportRefreshOutcome) -> &HostCapabilityReportNonTemporalCore {
    let Ok(Selection::Selected { report }) = &outcome.report_selection else {
        panic!("expected selected report: {outcome:?}")
    };
    report
}

fn cached(stale_reason: Stale, fingerprint: Option<&str>) -> HostCapabilityReportNonTemporalCore {
    HostCapabilityReportNonTemporalCore::new(HostCapabilityReportNonTemporalCoreInputs {
        host_id: HostId::Headless.as_str().into(),
        host_version: Some("apparently-current-version".into()),
        probe_status: Probe::Complete,
        validity_fingerprint: fingerprint.map(str::to_owned),
        hook_definition_digest: Some("old-hook-digest".into()),
        relevant_config_digest: Some("old-config-digest".into()),
        stale_reason,
        plugin_supported: Some(true),
        plugin_installed: Some(true),
        manifest_path: None,
        supports_skills: None,
        supports_commands: None,
        supports_subagents: None,
        supports_mcp: None,
        hooks_supported: Some(true),
        hooks_configured: Some(true),
        hook_trust_required: Some(true),
        hooks_trusted: Some(true),
        hooks_enabled: Some(true),
        hooks_allowed_by_admin_policy: Some(true),
        hook_events: None,
        blocking_hook_events: None,
        hook_coverage_class: Coverage::Full,
        required_hook_coverage_satisfied: Some(true),
        selected_mode: Mode::Embedded,
        mode_override: None,
        inactive_reason: Some(Reason::None),
        plugin_data_path: None,
        sandbox_modes: None,
        evidence_label: None,
        source_claim_id: None,
    })
    .unwrap()
}

#[test]
fn complete_detection_override_adapter_matrix_fails_before_refresh_and_is_deterministic() {
    let hosts = [HostId::ClaudeCode, HostId::Codex, HostId::Headless];
    let signals = [
        HostDetectionSignals::CLAUDE_ONLY,
        HostDetectionSignals::CODEX_ONLY,
        HostDetectionSignals::NONE,
        HostDetectionSignals::BOTH,
    ];
    let mut count = 0;
    for signals in signals {
        for explicit_host_override in [None, Some(hosts[0]), Some(hosts[1]), Some(hosts[2])] {
            for adapter_host in hosts {
                let request = HostSessionActivationRequest {
                    detection_signals: signals,
                    explicit_host_override,
                    ..request()
                };
                let expected = resolve_host(signals, explicit_host_override);
                let calls = Cell::new(0);
                let provider = |received: &HostCapabilityReportRefreshRequest| {
                    calls.set(calls.get() + 1);
                    assert_eq!(received.observation.host, expected.unwrap());
                    assert_eq!(received.observation.project_root, request.project_root);
                    assert_eq!(received.mode_override, None);
                    refreshed(unknown(received.observation.host), None)
                };
                let first = prepare_with_refresh(&Adapter(adapter_host), &request, provider);
                assert_eq!(
                    first,
                    prepare_with_refresh(&Adapter(adapter_host), &request, provider)
                );
                match expected {
                    Err(error) => {
                        assert_eq!(first, Err(HostSessionActivationError::Detection(error)));
                        assert_eq!(calls.get(), 0);
                        // Exercise the actual public gate too, with no I/O reachable.
                        assert_eq!(
                            prepare_host_session(&Adapter(adapter_host), &request),
                            first
                        );
                    }
                    Ok(resolved_host) if resolved_host != adapter_host => {
                        assert_eq!(
                            first,
                            Err(HostSessionActivationError::AdapterHostMismatch {
                                resolved_host,
                                adapter_host
                            })
                        );
                        assert_eq!(calls.get(), 0);
                        assert_eq!(
                            prepare_host_session(&Adapter(adapter_host), &request),
                            first
                        );
                    }
                    Ok(host) => {
                        let result = first.unwrap();
                        assert_eq!(result.resolved_host, host);
                        assert_eq!(result.freshness_disposition, freshness_disposition(false));
                        assert_eq!(calls.get(), 2);
                    }
                }
                count += 1;
            }
        }
    }
    assert_eq!(count, 48);
}

#[test]
fn identity_errors_are_typed_and_have_bounded_diagnostics() {
    let error = prepare_host_session(
        &Adapter(HostId::Headless),
        &HostSessionActivationRequest {
            detection_signals: HostDetectionSignals::BOTH,
            ..request()
        },
    )
    .unwrap_err();
    assert!(std::error::Error::source(&error).is_some());
    assert!(error.to_string().contains("ambiguous"));
    let mismatch = prepare_host_session(&Adapter(HostId::Codex), &request()).unwrap_err();
    assert!(std::error::Error::source(&mismatch).is_none());
    assert_eq!(
        mismatch.to_string(),
        "resolved host HEADLESS does not match adapter CODEX"
    );
}

fn requires_refresh(candidate: Option<&HostCapabilityReportNonTemporalCore>) {
    let request = HostSessionActivationRequest {
        cached_report: candidate,
        ..request()
    };
    let calls = Cell::new(0);
    for _ in 0..2 {
        // start and resume both reevaluate; no retained authority
        let result = prepare_with_refresh(&Adapter(HostId::Headless), &request, |received| {
            calls.set(calls.get() + 1);
            refreshed(unknown(received.observation.host), None)
        })
        .unwrap();
        assert_eq!(
            result.freshness_disposition,
            HostCapabilityFreshnessDisposition::ReprobeThenSelect
        );
        let selected = report(&result.capability_refresh);
        assert_eq!(selected.selected_mode(), Mode::Supervised);
        assert_eq!(selected.probe_status(), Probe::Partial);
        assert_eq!(selected.validity_fingerprint(), None);
        assert_eq!(selected.host_version(), None);
        assert_eq!(selected.hooks_trusted(), None);
        assert_eq!(selected.stale_reason(), Stale::None);
    }
    assert_eq!(calls.get(), 2);
}

#[test]
fn no_cache_refreshes() {
    requires_refresh(None);
}
#[test]
fn none_fingerprint_is_unproven_and_refreshes() {
    requires_refresh(Some(&cached(Stale::None, None)));
}
#[test]
fn every_stale_reason_refreshes() {
    for stale in Stale::ALL.into_iter().filter(|s| *s != Stale::None) {
        requires_refresh(Some(&cached(stale, Some("old"))));
    }
}
#[test]
fn nonempty_uncomparable_fingerprint_refreshes() {
    requires_refresh(Some(&cached(Stale::None, Some("abc"))));
}
#[test]
fn stale_embedded_never_survives_real_session_start_or_resume() {
    let candidate = cached(Stale::ValidityFingerprintChanged, Some("looks-valid"));
    assert_eq!(candidate.selected_mode(), Mode::Embedded);
    let request = HostSessionActivationRequest {
        cached_report: Some(&candidate),
        ..request()
    };
    for _ in 0..2 {
        let result = prepare_host_session(&Adapter(HostId::Headless), &request).unwrap();
        let expected = refresh_host_capability_report(&HostCapabilityReportRefreshRequest {
            observation: HostCapabilityObservationRequest {
                host: HostId::Headless,
                project_root: request.project_root.clone(),
            },
            mode_override: None,
        });
        assert_eq!(result.capability_refresh, expected);
        assert_eq!(
            report(&result.capability_refresh).selected_mode(),
            Mode::Supervised
        );
        assert_eq!(
            report(&result.capability_refresh).validity_fingerprint(),
            None
        );
    }
}

#[test]
fn changed_plugin_install_and_other_failures_use_existing_policy() {
    type Change = fn(&mut HostCapabilityObservation);
    let cases: [(Change, Reason); 4] = [
        (
            |o| o.plugin_installed = Value::Known(false),
            Reason::PluginNotInstalled,
        ),
        (
            |o| o.hooks_enabled = Value::Known(false),
            Reason::HooksDisabled,
        ),
        (
            |o| {
                o.hook_trust_required = Value::Known(true);
                o.hooks_trusted = Value::Known(false);
            },
            Reason::HooksUntrusted,
        ),
        (
            |o| o.hooks_allowed_by_admin_policy = Value::Known(false),
            Reason::HooksExcludedByAdminPolicy,
        ),
    ];
    let candidate = cached(Stale::None, Some("old"));
    assert_eq!(candidate.plugin_installed(), Some(true));
    for host in [HostId::ClaudeCode, HostId::Codex, HostId::Headless] {
        for (change, reason) in cases {
            let request = HostSessionActivationRequest {
                explicit_host_override: Some(host),
                cached_report: Some(&candidate),
                ..request()
            };
            let mut observation = unknown(host);
            change(&mut observation);
            let expected = refreshed(observation, None);
            let result =
                prepare_with_refresh(&Adapter(host), &request, |_| expected.clone()).unwrap();
            assert_eq!(result.capability_refresh, expected);
            assert_eq!(
                report(&result.capability_refresh).inactive_reason(),
                Some(reason)
            );
            assert_eq!(
                report(&result.capability_refresh).selected_mode(),
                Mode::Supervised
            );
            if reason == Reason::PluginNotInstalled {
                assert_eq!(
                    report(&result.capability_refresh).plugin_installed(),
                    Some(false)
                );
            }
        }
    }
}

#[test]
fn unknown_trust_partial_and_overrides_are_preserved_for_every_host() {
    for host in [HostId::ClaudeCode, HostId::Codex, HostId::Headless] {
        for mode_override in
            std::iter::once(None).chain(HostCapabilityModeOverrideSource::ALL.map(|s| {
                Some(HostCapabilityModeOverride::new(s, " EMBEDDED\t💾 ".into()).unwrap())
            }))
        {
            let request = HostSessionActivationRequest {
                explicit_host_override: Some(host),
                mode_override: mode_override.clone(),
                ..request()
            };
            let result = prepare_with_refresh(&Adapter(host), &request, |received| {
                assert_eq!(received.mode_override, mode_override);
                refreshed(unknown(host), received.mode_override.clone())
            })
            .unwrap();
            let selected = report(&result.capability_refresh);
            assert_eq!(selected.mode_override(), mode_override.as_ref());
            assert_eq!(selected.probe_status(), Probe::Partial);
            assert_eq!(selected.selected_mode(), Mode::Supervised);
            assert_eq!(selected.hooks_trusted(), None);
            assert_eq!(selected.plugin_supported(), None);
            assert_eq!(selected.hooks_supported(), None);
            assert_eq!(selected.validity_fingerprint(), None);
        }
    }
}

#[test]
fn all_selection_outcomes_and_composition_errors_are_preserved() {
    let observation = unknown(HostId::Headless);
    let mut inputs = observation.report_selection_inputs(true, Stale::None, None);
    let selected = compose_host_capability_report_selection(&inputs);
    inputs.report_validity_proven_current = false;
    let reprobe = compose_host_capability_report_selection(&inputs);
    assert_eq!(reprobe, Ok(Selection::ReprobeRequired));
    for report_selection in [
        selected,
        reprobe,
        Ok(Selection::Indeterminate {
            cause: Gap::NoUniqueFallbackRule,
            inactive_reason: Reason::Unknown,
        }),
        Err(
            HostCapabilityReportSelectionCompositionError::ModeSelection(
                HostCapabilityModeSelectionError::ConservativeFallbackHasNoInactiveReason,
            ),
        ),
        Err(
            HostCapabilityReportSelectionCompositionError::ReportConstruction(
                crate::HostCapabilityReportNonTemporalCoreError::EmptyHostId,
            ),
        ),
    ] {
        let expected = HostCapabilityReportRefreshOutcome {
            observation: observation.clone(),
            report_selection,
        };
        let outcome =
            prepare_with_refresh(&Adapter(HostId::Headless), &request(), |_| expected.clone())
                .unwrap();
        assert_eq!(outcome.capability_refresh, expected);
    }
}

#[test]
fn hybrid_is_only_preserved_when_existing_selector_establishes_it() {
    // COMPLETE evidence is synthetic; the current real observation is PARTIAL.
    let mut inputs = unknown(HostId::Headless).report_selection_inputs(true, Stale::None, None);
    inputs.probe_status = Probe::Complete;
    inputs.plugin_supported = Some(true);
    inputs.plugin_installed = Some(true);
    inputs.hooks_supported = Some(true);
    inputs.hooks_configured = Some(true);
    inputs.hook_trust_required = Some(false);
    inputs.hooks_enabled = Some(true);
    inputs.hooks_allowed_by_admin_policy = Some(true);
    inputs.required_hook_coverage_satisfied = Some(false);
    inputs.hook_coverage_class = Coverage::Partial;
    let expected = HostCapabilityReportRefreshOutcome {
        observation: unknown(HostId::Headless),
        report_selection: compose_host_capability_report_selection(&inputs),
    };
    assert_eq!(report(&expected).selected_mode(), Mode::Hybrid);
    let result =
        prepare_with_refresh(&Adapter(HostId::Headless), &request(), |_| expected.clone()).unwrap();
    assert_eq!(result.capability_refresh, expected);
}

#[test]
fn production_is_only_identity_and_refresh_composition() {
    let source = include_str!("host_session_activation.rs");
    let body = source.split("pub fn prepare_host_session<").nth(1).unwrap();
    assert!(
        body.contains("prepare_with_refresh(adapter, request, refresh_host_capability_report)")
    );
    assert!(body.contains("freshness_disposition(false)"));
    assert!(body.find("resolve_host(").unwrap() < body.find("adapter.id()").unwrap());
    for forbidden in [
        "HostId::",
        "selected_mode",
        "inactive_reason",
        "cached_report.",
        "report_selection_inputs(",
        "compose_host_capability_report_selection(",
        "select_host_capability_mode(",
        "native_path_inactive_reason(",
        "assess_native_path_prerequisites(",
        "HostCapabilityReportNonTemporalCore::new",
        "observe_host_capabilities(",
        "std::fs",
        "std::env",
        "std::process",
        "std::net",
        "serde",
        "toml::",
        "Command",
        "SystemTime",
        "Instant",
        "Hasher",
        "sha2",
        "blake",
        "adapter.start(",
        "adapter.detect(",
        "adapter.capabilities(",
        "raw_ref",
        "NormalizedHostEvent",
        "WorktreeRemove",
        "START_GOAL",
        "unsafe",
        "routing",
        "ProductEntitlement",
        "ActivationState",
        "RuntimeAdapter",
        "Workspace",
        "Graph",
        "DAG",
    ] {
        assert!(
            !body.contains(forbidden),
            "unexpected production behavior: {forbidden}"
        );
    }
    for forbidden in [
        "crate::core",
        "crate::routing",
        "crate::state",
        "crate::workspace",
        "crate::review",
        "crate::adapters",
    ] {
        assert!(!source.contains(forbidden));
    }
}

#[test]
fn fresh_support_and_healthy_partial_facts_never_grant_embedded() {
    for host in [HostId::ClaudeCode, HostId::Codex, HostId::Headless] {
        for scenario in 0..5 {
            let mut observation = unknown(host);
            match scenario {
                0 => observation.plugin_supported = Value::Known(true),
                1 => observation.hooks_supported = Value::Known(true),
                _ => {
                    observation.plugin_supported = Value::Known(true);
                    observation.plugin_installed = Value::Known(true);
                    observation.hooks_supported = Value::Known(true);
                    observation.hooks_configured = Value::Known(true);
                    observation.hook_trust_required = Value::Known(true);
                    observation.hooks_trusted = Value::Known(true);
                    observation.hooks_enabled = Value::Known(true);
                    observation.hooks_allowed_by_admin_policy = Value::Known(true);
                    observation.required_hook_coverage_satisfied = Value::Known(true);
                    observation.hook_coverage_class = Coverage::Full;
                    if scenario == 3 {
                        observation.hooks_trusted = Value::Unknown;
                    }
                    if scenario == 4 {
                        observation.plugin_supported = Value::Unknown;
                    }
                }
            }
            let expected = refreshed(observation, None);
            let result = prepare_with_refresh(
                &Adapter(host),
                &HostSessionActivationRequest {
                    explicit_host_override: Some(host),
                    ..request()
                },
                |_| expected.clone(),
            )
            .unwrap();
            assert_eq!(result.capability_refresh, expected);
            match scenario {
                2 => assert_eq!(
                    result.capability_refresh.report_selection,
                    Ok(Selection::Indeterminate {
                        cause: Gap::NoUniqueFallbackRule,
                        inactive_reason: Reason::None,
                    })
                ),
                4 => {
                    assert_eq!(result.capability_refresh.report_selection, Err(
                    HostCapabilityReportSelectionCompositionError::ModeSelection(
                        HostCapabilityModeSelectionError::ConservativeFallbackHasNoInactiveReason)))
                }
                _ => {
                    let selected = report(&result.capability_refresh);
                    assert_eq!(selected.probe_status(), Probe::Partial);
                    assert_eq!(selected.selected_mode(), Mode::Supervised);
                    assert_eq!(selected.hooks_trusted(), None);
                }
            }
        }
    }
}
