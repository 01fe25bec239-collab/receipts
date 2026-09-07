use super::*;
use HostCapabilityHookCoverageClass as Coverage;
use HostCapabilityInactiveReason as Reason;
use HostCapabilityModeSelectionIndeterminacy as Gap;
use HostCapabilityProbeStatus as Probe;
use HostCapabilityReportSelectionCompositionError as Error;
use HostCapabilityReportSelectionCompositionInputs as Inputs;
use HostCapabilityReportSelectionCompositionOutcome as Outcome;
use HostCapabilitySelectedMode as Mode;
use HostCapabilityStaleReason as Stale;

fn healthy() -> Inputs {
    Inputs {
        report_validity_proven_current: true,
        host_id: " synthetic host\t💾 ".into(),
        host_version: Some(" version\t ".into()),
        probe_status: Probe::Complete,
        validity_fingerprint: Some(" opaque fingerprint ".into()),
        hook_definition_digest: Some(" hook digest ".into()),
        relevant_config_digest: Some(" config digest ".into()),
        stale_reason: Stale::None,
        plugin_supported: Some(true),
        plugin_installed: Some(true),
        manifest_path: Some(" ./manifest\t ".into()),
        supports_skills: Some(false),
        supports_commands: None,
        supports_subagents: Some(true),
        supports_mcp: Some(false),
        hooks_supported: Some(true),
        hooks_configured: Some(true),
        hook_trust_required: Some(true),
        hooks_trusted: Some(true),
        hooks_enabled: Some(true),
        hooks_allowed_by_admin_policy: Some(true),
        hook_events: Some(vec!["z".into(), "".into(), "a".into(), "z".into()]),
        blocking_hook_events: Some(vec!["b".into(), "b".into(), "".into(), "a".into()]),
        hook_coverage_class: Coverage::Full,
        required_hook_coverage_satisfied: Some(true),
        mode_override: None,
        plugin_data_path: Some(" ./data\t ".into()),
        sandbox_modes: Some(vec!["y".into(), "".into(), "x".into(), "y".into()]),
        evidence_label: Some(HostCapabilityEvidenceLabel::Unverified),
        source_claim_id: Some(" claim\t ".into()),
    }
}

// Every path checks repeatability and input immutability.
fn run(inputs: &Inputs) -> Result<Outcome, Error> {
    let before = inputs.clone();
    let result = compose_host_capability_report_selection(inputs);
    assert_eq!(compose_host_capability_report_selection(inputs), result);
    assert_eq!(*inputs, before);
    result
}

fn selected(inputs: &Inputs, mode: Mode, reason: Reason) {
    let Outcome::Selected { report } = run(inputs).unwrap() else {
        panic!("expected a completed report");
    };
    assert_eq!(report.selected_mode(), mode);
    assert_eq!(report.inactive_reason(), Some(reason));
    assert_preserved(inputs, &report);
}

#[test]
fn healthy_current_complete_produces_embedded() {
    selected(&healthy(), Mode::Embedded, Reason::None);
}

#[test]
fn stale_and_unproven_require_reprobe_without_report_construction() {
    for stale_reason in Stale::ALL {
        for current in [false, true] {
            if current && stale_reason == Stale::None {
                continue;
            }
            let inputs = Inputs {
                stale_reason,
                report_validity_proven_current: current,
                // These would fail construction if a report were fabricated.
                host_id: String::new(),
                validity_fingerprint: Some(String::new()),
                ..healthy()
            };
            assert_eq!(run(&inputs), Ok(Outcome::ReprobeRequired));
        }
    }
}

#[test]
fn known_untrusted_hooks_and_missing_plugin_are_conservative() {
    selected(
        &Inputs {
            hooks_trusted: Some(false),
            ..healthy()
        },
        Mode::Supervised,
        Reason::HooksUntrusted,
    );
    selected(
        &Inputs {
            plugin_installed: Some(false),
            ..healthy()
        },
        Mode::Supervised,
        Reason::PluginNotInstalled,
    );
}

#[test]
fn fixture_04_partial_coverage_only_produces_hybrid() {
    selected(
        &Inputs {
            hook_coverage_class: Coverage::Partial,
            required_hook_coverage_satisfied: Some(false),
            ..healthy()
        },
        Mode::Hybrid,
        Reason::InsufficientCoverage,
    );
}

#[test]
fn targetless_override_preserves_indeterminacy_and_never_constructs_report() {
    for source in HostCapabilityModeOverrideSource::ALL {
        let inputs = Inputs {
            mode_override: Some(
                HostCapabilityModeOverride::new(source, " force HYBRID or SUPERVISED ".into())
                    .unwrap(),
            ),
            host_id: String::new(),
            ..healthy()
        };
        assert_eq!(
            run(&inputs),
            Ok(Outcome::Indeterminate {
                cause: Gap::OverrideHasNoTargetMode,
                inactive_reason: Reason::ModeOverride,
            })
        );
    }
}

#[test]
fn all_other_indeterminacies_are_preserved() {
    assert_eq!(
        run(&Inputs {
            required_hook_coverage_satisfied: Some(false),
            ..healthy()
        }),
        Ok(Outcome::Indeterminate {
            cause: Gap::CoverageDoesNotDetermineFallback,
            inactive_reason: Reason::InsufficientCoverage,
        })
    );
    assert_eq!(
        run(&Inputs {
            probe_status: Probe::Partial,
            ..healthy()
        }),
        Ok(Outcome::Indeterminate {
            cause: Gap::NoUniqueFallbackRule,
            inactive_reason: Reason::None,
        })
    );
}

#[test]
fn unknown_and_failed_evidence_remain_conservative_with_known_failure_precedence() {
    for probe_status in [Probe::Complete, Probe::Partial, Probe::Failed] {
        let inputs = Inputs {
            probe_status,
            stale_reason: if probe_status == Probe::Failed {
                Stale::ProbeFailed
            } else {
                Stale::None
            },
            hooks_allowed_by_admin_policy: None,
            ..healthy()
        };
        selected(&inputs, Mode::Supervised, Reason::Unknown);
        selected(
            &Inputs {
                plugin_installed: Some(false),
                ..inputs
            },
            Mode::Supervised,
            Reason::PluginNotInstalled,
        );
    }
}

#[test]
fn complete_consistency_error_is_wrapped() {
    assert_eq!(
        run(&Inputs {
            plugin_supported: None,
            ..healthy()
        }),
        Err(Error::ModeSelection(
            HostCapabilityModeSelectionError::CompleteProbeConsistency(
                HostCapabilityConsistencyError::PluginInstalledRequiresPluginSupported
            )
        ))
    );
}

#[test]
fn other_selector_errors_are_wrapped() {
    assert_eq!(
        run(&Inputs {
            probe_status: Probe::Failed,
            ..healthy()
        }),
        Err(Error::ModeSelection(
            HostCapabilityModeSelectionError::FailedProbeRequiresProbeFailedStaleReason
        ))
    );
    assert_eq!(
        run(&Inputs {
            probe_status: Probe::Failed,
            stale_reason: Stale::ProbeFailed,
            ..healthy()
        }),
        Err(Error::ModeSelection(
            HostCapabilityModeSelectionError::ConservativeFallbackHasNoInactiveReason
        ))
    );
}

#[test]
fn report_storage_errors_are_wrapped() {
    assert_eq!(
        run(&Inputs {
            host_id: String::new(),
            ..healthy()
        }),
        Err(Error::ReportConstruction(
            HostCapabilityReportNonTemporalCoreError::EmptyHostId
        ))
    );
    assert_eq!(
        run(&Inputs {
            validity_fingerprint: Some(String::new()),
            ..healthy()
        }),
        Err(Error::ReportConstruction(
            HostCapabilityReportNonTemporalCoreError::EmptyValidityFingerprint
        ))
    );
}

#[test]
fn optional_storage_and_override_values_are_preserved_exactly() {
    for source in HostCapabilityModeOverrideSource::ALL {
        for label in HostCapabilityEvidenceLabel::ALL {
            let inputs = Inputs {
                plugin_installed: Some(false),
                mode_override: Some(
                    HostCapabilityModeOverride::new(source, " \t💾 ".into()).unwrap(),
                ),
                evidence_label: Some(label),
                ..healthy()
            };
            selected(&inputs, Mode::Supervised, Reason::PluginNotInstalled);
        }
    }
    for empty in [false, true] {
        let string = empty.then(String::new);
        let array = empty.then(Vec::new);
        selected(
            &Inputs {
                host_id: " ".into(),
                host_version: string.clone(),
                validity_fingerprint: None,
                hook_definition_digest: string.clone(),
                relevant_config_digest: string.clone(),
                manifest_path: string.clone(),
                plugin_data_path: string.clone(),
                source_claim_id: string,
                hook_events: array.clone(),
                blocking_hook_events: array.clone(),
                sandbox_modes: array,
                evidence_label: None,
                hook_trust_required: Some(false),
                hooks_trusted: None,
                ..healthy()
            },
            Mode::Embedded,
            Reason::None,
        );
    }
}

fn assert_preserved(inputs: &Inputs, report: &HostCapabilityReportNonTemporalCore) {
    assert_eq!(report.host_id(), inputs.host_id.as_str(), "host_id");
    assert_eq!(
        report.host_version(),
        inputs.host_version.as_deref(),
        "host_version"
    );
    assert_eq!(report.probe_status(), inputs.probe_status, "probe_status");
    assert_eq!(
        report.validity_fingerprint(),
        inputs.validity_fingerprint.as_deref(),
        "validity_fingerprint"
    );
    assert_eq!(
        report.hook_definition_digest(),
        inputs.hook_definition_digest.as_deref(),
        "hook_definition_digest"
    );
    assert_eq!(
        report.relevant_config_digest(),
        inputs.relevant_config_digest.as_deref(),
        "relevant_config_digest"
    );
    assert_eq!(report.stale_reason(), inputs.stale_reason, "stale_reason");
    assert_eq!(
        report.plugin_supported(),
        inputs.plugin_supported,
        "plugin_supported"
    );
    assert_eq!(
        report.plugin_installed(),
        inputs.plugin_installed,
        "plugin_installed"
    );
    assert_eq!(
        report.manifest_path(),
        inputs.manifest_path.as_deref(),
        "manifest_path"
    );
    assert_eq!(
        report.supports_skills(),
        inputs.supports_skills,
        "supports_skills"
    );
    assert_eq!(
        report.supports_commands(),
        inputs.supports_commands,
        "supports_commands"
    );
    assert_eq!(
        report.supports_subagents(),
        inputs.supports_subagents,
        "supports_subagents"
    );
    assert_eq!(report.supports_mcp(), inputs.supports_mcp, "supports_mcp");
    assert_eq!(
        report.hooks_supported(),
        inputs.hooks_supported,
        "hooks_supported"
    );
    assert_eq!(
        report.hooks_configured(),
        inputs.hooks_configured,
        "hooks_configured"
    );
    assert_eq!(
        report.hook_trust_required(),
        inputs.hook_trust_required,
        "hook_trust_required"
    );
    assert_eq!(
        report.hooks_trusted(),
        inputs.hooks_trusted,
        "hooks_trusted"
    );
    assert_eq!(
        report.hooks_enabled(),
        inputs.hooks_enabled,
        "hooks_enabled"
    );
    assert_eq!(
        report.hooks_allowed_by_admin_policy(),
        inputs.hooks_allowed_by_admin_policy,
        "hooks_allowed_by_admin_policy"
    );
    assert_eq!(
        report.hook_events(),
        inputs.hook_events.as_deref(),
        "hook_events"
    );
    assert_eq!(
        report.blocking_hook_events(),
        inputs.blocking_hook_events.as_deref(),
        "blocking_hook_events"
    );
    assert_eq!(
        report.hook_coverage_class(),
        inputs.hook_coverage_class,
        "hook_coverage_class"
    );
    assert_eq!(
        report.required_hook_coverage_satisfied(),
        inputs.required_hook_coverage_satisfied,
        "required_hook_coverage_satisfied"
    );
    assert_eq!(
        report.mode_override(),
        inputs.mode_override.as_ref(),
        "mode_override"
    );
    assert_eq!(
        report.plugin_data_path(),
        inputs.plugin_data_path.as_deref(),
        "plugin_data_path"
    );
    assert_eq!(
        report.sandbox_modes(),
        inputs.sandbox_modes.as_deref(),
        "sandbox_modes"
    );
    assert_eq!(
        report.evidence_label(),
        inputs.evidence_label,
        "evidence_label"
    );
    assert_eq!(
        report.source_claim_id(),
        inputs.source_claim_id.as_deref(),
        "source_claim_id"
    );
}

#[test]
fn selection_input_adaptation_matches_accepted_selector() {
    let setters: [fn(&mut Inputs, Option<bool>); 9] = [
        |inputs, value| inputs.plugin_supported = value,
        |inputs, value| inputs.plugin_installed = value,
        |inputs, value| inputs.hooks_supported = value,
        |inputs, value| inputs.hooks_configured = value,
        |inputs, value| inputs.hook_trust_required = value,
        |inputs, value| inputs.hooks_trusted = value,
        |inputs, value| inputs.hooks_enabled = value,
        |inputs, value| inputs.hooks_allowed_by_admin_policy = value,
        |inputs, value| inputs.required_hook_coverage_satisfied = value,
    ];
    for set in setters {
        for value in [None, Some(false), Some(true)] {
            for probe_status in Probe::ALL {
                for stale_reason in Stale::ALL {
                    for hook_coverage_class in Coverage::ALL {
                        for current in [false, true] {
                            for overridden in [false, true] {
                                let mut inputs = Inputs {
                                    probe_status,
                                    stale_reason,
                                    hook_coverage_class,
                                    report_validity_proven_current: current,
                                    mode_override: overridden.then(|| {
                                        HostCapabilityModeOverride::new(
                                            HostCapabilityModeOverrideSource::User,
                                            " opaque override ".into(),
                                        )
                                        .unwrap()
                                    }),
                                    ..healthy()
                                };
                                set(&mut inputs, value);
                                let expected = select_host_capability_mode(
                                    &HostCapabilityModeSelectionInputs {
                                        report_validity_proven_current: current,
                                        probe_status,
                                        stale_reason,
                                        hook_coverage_class,
                                        mode_override: inputs.mode_override.clone(),
                                        native_prerequisites:
                                            HostCapabilityNativePrerequisiteInputs {
                                                plugin_supported: inputs.plugin_supported,
                                                plugin_installed: inputs.plugin_installed,
                                                hooks_supported: inputs.hooks_supported,
                                                hooks_configured: inputs.hooks_configured,
                                                hook_trust_required: inputs.hook_trust_required,
                                                hooks_trusted: inputs.hooks_trusted,
                                                hooks_enabled: inputs.hooks_enabled,
                                                hooks_allowed_by_admin_policy: inputs
                                                    .hooks_allowed_by_admin_policy,
                                                required_hook_coverage_satisfied: inputs
                                                    .required_hook_coverage_satisfied,
                                            },
                                    },
                                );
                                match expected {
                                    Ok(HostCapabilityModeSelectionOutcome::Selected {
                                        selected_mode,
                                        inactive_reason,
                                    }) => selected(&inputs, selected_mode, inactive_reason),
                                    Ok(HostCapabilityModeSelectionOutcome::ReprobeRequired) => {
                                        assert_eq!(run(&inputs), Ok(Outcome::ReprobeRequired))
                                    }
                                    Ok(HostCapabilityModeSelectionOutcome::Indeterminate {
                                        cause,
                                        inactive_reason,
                                    }) => assert_eq!(
                                        run(&inputs),
                                        Ok(Outcome::Indeterminate {
                                            cause,
                                            inactive_reason
                                        })
                                    ),
                                    Err(error) => {
                                        assert_eq!(run(&inputs), Err(Error::ModeSelection(error)))
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
