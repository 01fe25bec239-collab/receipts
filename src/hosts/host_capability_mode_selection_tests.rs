//! Semantic equivalents of all 8 positive and 13 negative frozen fixtures,
//! plus layered selection-boundary matrices. No fixture I/O or deserialization.

use super::*;
use HostCapabilityConsistencyError as CompleteError;
use HostCapabilityHookCoverageClass as Coverage;
use HostCapabilityInactiveReason as Reason;
use HostCapabilityModeSelectionError as Error;
use HostCapabilityModeSelectionIndeterminacy as Gap;
use HostCapabilityModeSelectionInputs as Inputs;
use HostCapabilityModeSelectionOutcome as Outcome;
use HostCapabilityNativePrerequisiteInputs as Facts;
use HostCapabilityNativePrerequisiteState as Native;
use HostCapabilityProbeStatus as Probe;
use HostCapabilitySelectedMode as Mode;
use HostCapabilityStaleReason as Stale;

const HEALTHY: Facts = Facts {
    plugin_supported: Some(true),
    plugin_installed: Some(true),
    hooks_supported: Some(true),
    hooks_configured: Some(true),
    hook_trust_required: Some(true),
    hooks_trusted: Some(true),
    hooks_enabled: Some(true),
    hooks_allowed_by_admin_policy: Some(true),
    required_hook_coverage_satisfied: Some(true),
};

const UNKNOWN: Facts = Facts {
    plugin_supported: None,
    plugin_installed: None,
    hooks_supported: None,
    hooks_configured: None,
    hook_trust_required: None,
    hooks_trusted: None,
    hooks_enabled: None,
    hooks_allowed_by_admin_policy: None,
    required_hook_coverage_satisfied: None,
};

fn healthy() -> Inputs {
    Inputs {
        report_validity_proven_current: true,
        probe_status: Probe::Complete,
        stale_reason: Stale::None,
        native_prerequisites: HEALTHY,
        hook_coverage_class: Coverage::Full,
        mode_override: None,
    }
}

fn selected(selected_mode: Mode, inactive_reason: Reason) -> Outcome {
    Outcome::Selected {
        selected_mode,
        inactive_reason,
    }
}

fn gap(cause: Gap, inactive_reason: Reason) -> Outcome {
    Outcome::Indeterminate {
        cause,
        inactive_reason,
    }
}

fn explicit_override() -> HostCapabilityModeOverride {
    HostCapabilityModeOverride::new(
        HostCapabilityModeOverrideSource::Debug,
        "fixture forces supervised".to_owned(),
    )
    .unwrap()
}

/// Every check also proves repeatability, structural immutability, and that
/// emitted modes validate against the ORIGINAL caller facts and derived state.
fn check(inputs: &Inputs, expected: Result<Outcome, Error>) {
    let before = inputs.clone();
    let actual = select_host_capability_mode(inputs);
    assert_eq!(actual, expected, "{inputs:?}");
    assert_eq!(select_host_capability_mode(inputs), actual);
    assert_eq!(*inputs, before);
    if let Ok(Outcome::Selected {
        selected_mode,
        inactive_reason,
    }) = actual
    {
        let facts = inputs.native_prerequisites;
        assert_eq!(
            validate_selected_mode_consistency(&HostCapabilitySelectedModeConsistencyInputs {
                report_validity_proven_current: inputs.report_validity_proven_current,
                probe_status: inputs.probe_status,
                stale_reason: inputs.stale_reason,
                native_prerequisite_state: assess_native_path_prerequisites(facts),
                selected_mode,
                mode_override: inputs.mode_override.clone(),
                inactive_reason: Some(inactive_reason),
            }),
            Ok(())
        );
        assert_eq!(
            inactive_reason,
            native_path_inactive_reason(HostCapabilityInactiveReasonInputs {
                plugin_installed: facts.plugin_installed,
                hooks_supported: facts.hooks_supported,
                hooks_configured: facts.hooks_configured,
                hook_trust_required: facts.hook_trust_required,
                hooks_trusted: facts.hooks_trusted,
                hooks_enabled: facts.hooks_enabled,
                hooks_allowed_by_admin_policy: facts.hooks_allowed_by_admin_policy,
                required_hook_coverage_satisfied: facts.required_hook_coverage_satisfied,
                mode_override_present: inputs.mode_override.is_some(),
            })
        );
    }
}

fn no_trust_model() -> Inputs {
    Inputs {
        native_prerequisites: Facts {
            hook_trust_required: Some(false),
            hooks_trusted: None,
            ..HEALTHY
        },
        ..healthy()
    }
}

fn untrusted() -> Inputs {
    Inputs {
        native_prerequisites: Facts {
            hooks_trusted: Some(false),
            required_hook_coverage_satisfied: Some(false),
            ..HEALTHY
        },
        hook_coverage_class: Coverage::None,
        ..healthy()
    }
}

fn partial_coverage() -> Inputs {
    Inputs {
        native_prerequisites: Facts {
            required_hook_coverage_satisfied: Some(false),
            ..HEALTHY
        },
        hook_coverage_class: Coverage::Partial,
        ..healthy()
    }
}

fn unknown_partial(plugin_installed: Option<bool>) -> Inputs {
    Inputs {
        report_validity_proven_current: false,
        probe_status: Probe::Partial,
        stale_reason: Stale::ValidityFingerprintUnproven,
        native_prerequisites: Facts {
            plugin_supported: Some(true),
            plugin_installed,
            ..UNKNOWN
        },
        hook_coverage_class: Coverage::Unknown,
        ..healthy()
    }
}

#[test]
fn all_eight_positive_fixture_semantics() {
    let cases = [
        (
            "01_codex_embedded",
            healthy(),
            selected(Mode::Embedded, Reason::None),
        ),
        (
            "02_claude_embedded_no_trust_model",
            no_trust_model(),
            selected(Mode::Embedded, Reason::None),
        ),
        (
            "03_codex_supervised_untrusted",
            untrusted(),
            selected(Mode::Supervised, Reason::HooksUntrusted),
        ),
        (
            "04_codex_hybrid_partial_coverage",
            partial_coverage(),
            selected(Mode::Hybrid, Reason::InsufficientCoverage),
        ),
        (
            "05_host_no_trust_model_embedded_valid",
            no_trust_model(),
            selected(Mode::Embedded, Reason::None),
        ),
        // These old reports carry a caller-selected conservative mode. This API
        // must surface REPROBE_THEN_SELECT when validity is unproven instead.
        (
            "06_partial_unknown_supervised",
            unknown_partial(Some(true)),
            Outcome::ReprobeRequired,
        ),
        // Neither the provenance source nor the words "forces supervised"
        // create a target-mode contract for this selector.
        (
            "07_healthy_explicit_override",
            Inputs {
                mode_override: Some(explicit_override()),
                ..no_trust_model()
            },
            gap(Gap::OverrideHasNoTargetMode, Reason::ModeOverride),
        ),
        (
            "08_partial_plugin_support_known_install_unknown",
            unknown_partial(None),
            Outcome::ReprobeRequired,
        ),
    ];
    assert_eq!(cases.len(), 8);
    for (name, inputs, expected) in cases {
        assert_eq!(select_host_capability_mode(&inputs), Ok(expected), "{name}");
        check(&inputs, Ok(expected));
    }
    for plugin_installed in [None, Some(true)] {
        check(
            &Inputs {
                report_validity_proven_current: true,
                stale_reason: Stale::None,
                ..unknown_partial(plugin_installed)
            },
            Ok(selected(Mode::Supervised, Reason::Unknown)),
        );
    }
}

#[test]
fn all_thirteen_negative_fixture_semantics() {
    let trust_error = Err(Error::CompleteProbeConsistency(
        CompleteError::TrustRequiredRequiresKnownTrustedState,
    ));
    let cases = [
        (
            "01_embedded_with_untrusted_hooks",
            Inputs {
                native_prerequisites: Facts {
                    hooks_trusted: Some(false),
                    ..HEALTHY
                },
                ..healthy()
            },
            Ok(selected(Mode::Supervised, Reason::HooksUntrusted)),
        ),
        (
            "02_plugin_installed_without_support",
            Inputs {
                native_prerequisites: Facts {
                    plugin_supported: Some(false),
                    hooks_supported: Some(false),
                    hooks_configured: Some(false),
                    hooks_enabled: Some(false),
                    hooks_allowed_by_admin_policy: Some(false),
                    required_hook_coverage_satisfied: Some(false),
                    ..no_trust_model().native_prerequisites
                },
                hook_coverage_class: Coverage::None,
                ..healthy()
            },
            Err(Error::CompleteProbeConsistency(
                CompleteError::PluginInstalledRequiresPluginSupported,
            )),
        ),
        // Caller-selected mode and inactive reason are deliberately absent from
        // selector inputs: these two false claims cannot be passed through.
        (
            "03_embedded_with_stale_inactive_reason",
            healthy(),
            Ok(selected(Mode::Embedded, Reason::None)),
        ),
        (
            "04_supervised_falsely_claims_no_inactive_reason",
            Inputs {
                native_prerequisites: Facts {
                    hooks_configured: Some(false),
                    hooks_trusted: None,
                    required_hook_coverage_satisfied: Some(false),
                    ..HEALTHY
                },
                hook_coverage_class: Coverage::None,
                ..healthy()
            },
            trust_error,
        ),
        (
            "05_trust_model_ambiguity_codex_null",
            Inputs {
                native_prerequisites: Facts {
                    hooks_trusted: None,
                    ..HEALTHY
                },
                ..healthy()
            },
            trust_error,
        ),
        (
            "06_inactive_reason_hooks_disabled_mismatch",
            untrusted(),
            Ok(selected(Mode::Supervised, Reason::HooksUntrusted)),
        ),
        (
            "07_insufficient_coverage_embedded",
            partial_coverage(),
            Ok(selected(Mode::Hybrid, Reason::InsufficientCoverage)),
        ),
        (
            "08_stale_embedded",
            Inputs {
                report_validity_proven_current: false,
                stale_reason: Stale::ValidityFingerprintChanged,
                ..healthy()
            },
            Ok(Outcome::ReprobeRequired),
        ),
        (
            "09_known_plugin_missing_unknown_reason",
            Inputs {
                native_prerequisites: Facts {
                    plugin_installed: Some(false),
                    hooks_supported: Some(false),
                    hooks_configured: Some(false),
                    hooks_trusted: Some(false),
                    hooks_enabled: Some(false),
                    hooks_allowed_by_admin_policy: Some(false),
                    required_hook_coverage_satisfied: Some(false),
                    ..HEALTHY
                },
                hook_coverage_class: Coverage::None,
                ..healthy()
            },
            Ok(selected(Mode::Supervised, Reason::PluginNotInstalled)),
        ),
        (
            "10_healthy_supervised_without_override",
            no_trust_model(),
            Ok(selected(Mode::Embedded, Reason::None)),
        ),
        (
            "11_hooks_configured_without_support",
            Inputs {
                native_prerequisites: Facts {
                    hooks_supported: Some(false),
                    hooks_enabled: Some(false),
                    hooks_allowed_by_admin_policy: Some(false),
                    required_hook_coverage_satisfied: Some(false),
                    ..no_trust_model().native_prerequisites
                },
                hook_coverage_class: Coverage::None,
                ..healthy()
            },
            Err(Error::CompleteProbeConsistency(
                CompleteError::HooksConfiguredRequiresHooksSupported,
            )),
        ),
        (
            "12_hooks_enabled_without_support",
            Inputs {
                native_prerequisites: Facts {
                    hooks_supported: Some(false),
                    ..no_trust_model().native_prerequisites
                },
                ..healthy()
            },
            Err(Error::CompleteProbeConsistency(
                CompleteError::HooksConfiguredRequiresHooksSupported,
            )),
        ),
        (
            "13_hook_trust_required_unknown",
            Inputs {
                native_prerequisites: Facts {
                    hooks_configured: Some(false),
                    hooks_trusted: None,
                    hooks_enabled: Some(false),
                    hooks_allowed_by_admin_policy: Some(false),
                    required_hook_coverage_satisfied: Some(false),
                    ..HEALTHY
                },
                hook_coverage_class: Coverage::None,
                ..healthy()
            },
            trust_error,
        ),
    ];
    assert_eq!(cases.len(), 13);
    for (name, inputs, expected) in cases {
        assert_eq!(select_host_capability_mode(&inputs), expected, "{name}");
        check(&inputs, expected);
    }
    // Isolate fixture 12's enabled rule after its earlier configured error.
    check(
        &Inputs {
            native_prerequisites: Facts {
                hooks_supported: Some(false),
                hooks_configured: Some(false),
                ..HEALTHY
            },
            ..healthy()
        },
        Err(Error::CompleteProbeConsistency(
            CompleteError::HooksEnabledRequiresHooksSupported,
        )),
    );
    // Once fixture 04 has a known trust state, its known configuration failure
    // is reported; it cannot falsely claim NONE.
    check(
        &Inputs {
            native_prerequisites: Facts {
                hooks_configured: Some(false),
                ..HEALTHY
            },
            ..healthy()
        },
        Ok(selected(Mode::Supervised, Reason::HooksNotConfigured)),
    );
}

#[test]
fn freshness_probe_native_override_matrix_144_cells() {
    let mut count = 0;
    for current in [false, true] {
        for probe_status in Probe::ALL {
            for stale_reason in Stale::ALL {
                for (facts, state, reason) in [
                    (HEALTHY, Native::Satisfied, Reason::None),
                    (
                        Facts {
                            plugin_installed: Some(false),
                            ..HEALTHY
                        },
                        Native::Unsatisfied,
                        Reason::PluginNotInstalled,
                    ),
                    (
                        Facts {
                            hooks_allowed_by_admin_policy: None,
                            ..HEALTHY
                        },
                        Native::Unknown,
                        Reason::Unknown,
                    ),
                ] {
                    assert_eq!(assess_native_path_prerequisites(facts), state);
                    for with_override in [false, true] {
                        count += 1;
                        let inputs = Inputs {
                            report_validity_proven_current: current,
                            probe_status,
                            stale_reason,
                            native_prerequisites: facts,
                            mode_override: with_override.then(explicit_override),
                            ..healthy()
                        };
                        let derived_reason =
                            if with_override && reason != Reason::PluginNotInstalled {
                                Reason::ModeOverride
                            } else {
                                reason
                            };
                        let expected = if probe_status == Probe::Failed
                            && stale_reason != Stale::ProbeFailed
                        {
                            Err(Error::FailedProbeRequiresProbeFailedStaleReason)
                        } else if !current
                            || (stale_reason != Stale::None && probe_status != Probe::Failed)
                        {
                            Ok(Outcome::ReprobeRequired)
                        } else {
                            match (probe_status, state, with_override) {
                                (Probe::Complete, Native::Satisfied, false) => {
                                    Ok(selected(Mode::Embedded, Reason::None))
                                }
                                (Probe::Complete, Native::Satisfied, true) => {
                                    Ok(gap(Gap::OverrideHasNoTargetMode, Reason::ModeOverride))
                                }
                                (Probe::Partial, Native::Satisfied, _) => {
                                    Ok(gap(Gap::NoUniqueFallbackRule, derived_reason))
                                }
                                (Probe::Failed, Native::Satisfied, false) => {
                                    Err(Error::ConservativeFallbackHasNoInactiveReason)
                                }
                                _ => Ok(selected(Mode::Supervised, derived_reason)),
                            }
                        };
                        check(&inputs, expected);
                    }
                }
            }
        }
    }
    assert_eq!(count, 144);
}

#[test]
fn required_coverage_is_independent_of_vendor_coverage_72_cells() {
    let mut count = 0;
    for probe_status in Probe::ALL {
        for hook_coverage_class in Coverage::ALL {
            for required_hook_coverage_satisfied in [None, Some(false), Some(true)] {
                for with_override in [false, true] {
                    count += 1;
                    let inputs = Inputs {
                        probe_status,
                        hook_coverage_class,
                        stale_reason: if probe_status == Probe::Failed {
                            Stale::ProbeFailed
                        } else {
                            Stale::None
                        },
                        native_prerequisites: Facts {
                            required_hook_coverage_satisfied,
                            ..HEALTHY
                        },
                        mode_override: with_override.then(explicit_override),
                        ..healthy()
                    };
                    let reason = match required_hook_coverage_satisfied {
                        Some(false) => Reason::InsufficientCoverage,
                        _ if with_override => Reason::ModeOverride,
                        Some(true) => Reason::None,
                        None => Reason::Unknown,
                    };
                    let expected = match (
                        probe_status,
                        required_hook_coverage_satisfied,
                        hook_coverage_class,
                    ) {
                        (Probe::Failed, Some(true), _) if !with_override => {
                            Err(Error::ConservativeFallbackHasNoInactiveReason)
                        }
                        (Probe::Failed, _, _) | (_, None, _) => {
                            Ok(selected(Mode::Supervised, reason))
                        }
                        (Probe::Complete, Some(true), _) if with_override => {
                            Ok(gap(Gap::OverrideHasNoTargetMode, reason))
                        }
                        (Probe::Complete, Some(true), _) => Ok(selected(Mode::Embedded, reason)),
                        (Probe::Partial, Some(true), _) => {
                            Ok(gap(Gap::NoUniqueFallbackRule, reason))
                        }
                        (_, Some(false), Coverage::None) => Ok(selected(Mode::Supervised, reason)),
                        (Probe::Complete, Some(false), Coverage::Partial) => {
                            Ok(selected(Mode::Hybrid, reason))
                        }
                        (_, Some(false), _) => {
                            Ok(gap(Gap::CoverageDoesNotDetermineFallback, reason))
                        }
                    };
                    check(&inputs, expected);
                }
            }
        }
    }
    assert_eq!(count, 72);
}

#[test]
fn conditional_trust_and_coverage_matrix_108_cells() {
    let mut count = 0;
    for hook_trust_required in [None, Some(false), Some(true)] {
        for hooks_trusted in [None, Some(false), Some(true)] {
            for probe_status in Probe::ALL {
                for coverage_satisfied in [false, true] {
                    for with_override in [false, true] {
                        count += 1;
                        let inputs = Inputs {
                            probe_status,
                            stale_reason: if probe_status == Probe::Failed {
                                Stale::ProbeFailed
                            } else {
                                Stale::None
                            },
                            native_prerequisites: Facts {
                                hook_trust_required,
                                hooks_trusted,
                                required_hook_coverage_satisfied: Some(coverage_satisfied),
                                ..HEALTHY
                            },
                            hook_coverage_class: Coverage::Partial,
                            mode_override: with_override.then(explicit_override),
                            ..healthy()
                        };
                        let expected = if probe_status == Probe::Complete
                            && hook_trust_required == Some(true)
                            && hooks_trusted.is_none()
                        {
                            Err(Error::CompleteProbeConsistency(
                                CompleteError::TrustRequiredRequiresKnownTrustedState,
                            ))
                        } else if hook_trust_required == Some(true) && hooks_trusted == Some(false)
                        {
                            Ok(selected(Mode::Supervised, Reason::HooksUntrusted))
                        } else if !coverage_satisfied {
                            if probe_status == Probe::Failed {
                                Ok(selected(Mode::Supervised, Reason::InsufficientCoverage))
                            } else if probe_status == Probe::Complete
                                && (hook_trust_required == Some(false)
                                    || (hook_trust_required == Some(true)
                                        && hooks_trusted == Some(true)))
                            {
                                Ok(selected(Mode::Hybrid, Reason::InsufficientCoverage))
                            } else {
                                Ok(gap(
                                    Gap::CoverageDoesNotDetermineFallback,
                                    Reason::InsufficientCoverage,
                                ))
                            }
                        } else {
                            let trust_known = hook_trust_required == Some(false)
                                || (hook_trust_required == Some(true)
                                    && hooks_trusted == Some(true));
                            let reason = if with_override {
                                Reason::ModeOverride
                            } else if trust_known {
                                Reason::None
                            } else {
                                Reason::Unknown
                            };
                            match (probe_status, trust_known, with_override) {
                                (Probe::Complete, true, false) => {
                                    Ok(selected(Mode::Embedded, reason))
                                }
                                (Probe::Complete, true, true) => {
                                    Ok(gap(Gap::OverrideHasNoTargetMode, reason))
                                }
                                (Probe::Partial, true, _) => {
                                    Ok(gap(Gap::NoUniqueFallbackRule, reason))
                                }
                                (Probe::Failed, true, false) => {
                                    Err(Error::ConservativeFallbackHasNoInactiveReason)
                                }
                                _ => Ok(selected(Mode::Supervised, reason)),
                            }
                        };
                        check(&inputs, expected);
                    }
                }
            }
        }
    }
    assert_eq!(count, 108);
}

#[test]
fn every_native_negative_axis_and_complete_error_order() {
    // Reuse policy outputs, already independently exhaustively tested in the
    // predecessor, to prove selector boundary consumption rather than copy it.
    let setters: [fn(&mut Facts, Option<bool>); 9] = [
        |f, v| f.plugin_supported = v,
        |f, v| f.plugin_installed = v,
        |f, v| f.hooks_supported = v,
        |f, v| f.hooks_configured = v,
        |f, v| f.hook_trust_required = v,
        |f, v| f.hooks_trusted = v,
        |f, v| f.hooks_enabled = v,
        |f, v| f.hooks_allowed_by_admin_policy = v,
        |f, v| f.required_hook_coverage_satisfied = v,
    ];
    for (axis, set) in setters.into_iter().enumerate() {
        for value in [None, Some(false)] {
            for probe_status in Probe::ALL {
                let mut inputs = healthy();
                inputs.probe_status = probe_status;
                inputs.stale_reason = if probe_status == Probe::Failed {
                    Stale::ProbeFailed
                } else {
                    Stale::None
                };
                set(&mut inputs.native_prerequisites, value);
                let f = inputs.native_prerequisites;
                let consistency =
                    validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
                        probe_status,
                        plugin_supported: f.plugin_supported,
                        plugin_installed: f.plugin_installed,
                        hooks_supported: f.hooks_supported,
                        hooks_configured: f.hooks_configured,
                        hooks_enabled: f.hooks_enabled,
                        hook_trust_required: f.hook_trust_required,
                        hooks_trusted: f.hooks_trusted,
                    });
                let outcome = select_host_capability_mode(&inputs);
                if let Err(error) = consistency {
                    check(&inputs, Err(Error::CompleteProbeConsistency(error)));
                    inputs.report_validity_proven_current = false;
                    check(&inputs, Err(Error::CompleteProbeConsistency(error)));
                } else {
                    check(&inputs, outcome);
                    let eligible = is_embedded_eligible(
                        true,
                        probe_status,
                        inputs.stale_reason,
                        assess_native_path_prerequisites(f) == Native::Satisfied,
                    );
                    assert_eq!(
                        outcome == Ok(selected(Mode::Embedded, Reason::None)),
                        eligible,
                        "axis {axis}, {value:?}"
                    );
                    assert!(!matches!(outcome, Err(Error::CompleteProbeConsistency(_))));
                }
            }
        }
    }
}

#[test]
fn all_known_failure_pairs_and_override_preserve_precedence() {
    type SetFailure = fn(&mut Facts);
    let failures: [(SetFailure, Reason); 7] = [
        (
            |f| f.plugin_installed = Some(false),
            Reason::PluginNotInstalled,
        ),
        (
            |f| f.hooks_supported = Some(false),
            Reason::HooksUnsupported,
        ),
        (
            |f| f.hooks_configured = Some(false),
            Reason::HooksNotConfigured,
        ),
        (|f| f.hooks_trusted = Some(false), Reason::HooksUntrusted),
        (|f| f.hooks_enabled = Some(false), Reason::HooksDisabled),
        (
            |f| f.hooks_allowed_by_admin_policy = Some(false),
            Reason::HooksExcludedByAdminPolicy,
        ),
        (
            |f| f.required_hook_coverage_satisfied = Some(false),
            Reason::InsufficientCoverage,
        ),
    ];
    let mut count = 0;
    for (i, (first, reason)) in failures.iter().enumerate() {
        for (second, _) in failures.iter().skip(i + 1) {
            for with_override in [false, true] {
                for probe_status in Probe::ALL {
                    count += 1;
                    let mut facts = HEALTHY;
                    first(&mut facts);
                    second(&mut facts);
                    // Keep COMPLETE fixtures schema-consistent. These lower
                    // failures do not alter the expected earlier reason.
                    if facts.hooks_supported == Some(false) {
                        facts.hooks_configured = Some(false);
                        facts.hooks_enabled = Some(false);
                    }
                    check(
                        &Inputs {
                            probe_status,
                            stale_reason: if probe_status == Probe::Failed {
                                Stale::ProbeFailed
                            } else {
                                Stale::None
                            },
                            native_prerequisites: facts,
                            mode_override: with_override.then(explicit_override),
                            ..healthy()
                        },
                        Ok(selected(Mode::Supervised, *reason)),
                    );
                }
            }
        }
    }
    assert_eq!(count, 126);
}

#[test]
fn missing_plugin_dominates_unknowns_lower_failures_and_every_override_source() {
    // Complete 3^4 lower-fact matrix; no duplication of 3^9 prerequisites.
    let mut count = 0;
    for hooks_supported in [None, Some(false), Some(true)] {
        for hooks_enabled in [None, Some(false), Some(true)] {
            for hooks_allowed_by_admin_policy in [None, Some(false), Some(true)] {
                for required_hook_coverage_satisfied in [None, Some(false), Some(true)] {
                    for mode_override in std::iter::once(None).chain(
                        HostCapabilityModeOverrideSource::ALL.map(|source| {
                            Some(HostCapabilityModeOverride::new(source, " ".to_owned()).unwrap())
                        }),
                    ) {
                        count += 1;
                        check(
                            &Inputs {
                                probe_status: Probe::Partial,
                                native_prerequisites: Facts {
                                    plugin_installed: Some(false),
                                    hooks_supported,
                                    hooks_enabled,
                                    hooks_allowed_by_admin_policy,
                                    required_hook_coverage_satisfied,
                                    ..UNKNOWN
                                },
                                mode_override,
                                ..healthy()
                            },
                            Ok(selected(Mode::Supervised, Reason::PluginNotInstalled)),
                        );
                    }
                }
            }
        }
    }
    assert_eq!(count, 324);
}

#[test]
fn override_source_and_reason_never_supply_a_target_mode() {
    for source in HostCapabilityModeOverrideSource::ALL {
        for reason in [
            "SUPERVISED",
            "HYBRID",
            "EMBEDDED",
            " ",
            "\t",
            "💾",
            "fixture forces supervised",
        ] {
            let mode_override =
                Some(HostCapabilityModeOverride::new(source, reason.to_owned()).unwrap());
            check(
                &Inputs {
                    mode_override: mode_override.clone(),
                    ..healthy()
                },
                Ok(gap(Gap::OverrideHasNoTargetMode, Reason::ModeOverride)),
            );
            check(
                &Inputs {
                    mode_override,
                    ..partial_coverage()
                },
                Ok(selected(Mode::Hybrid, Reason::InsufficientCoverage)),
            );
        }
    }
}

#[test]
fn coverage_only_proof_never_promotes_an_unrelated_unknown_or_false() {
    for plugin_supported in [None, Some(false)] {
        for probe_status in [Probe::Complete, Probe::Partial] {
            let inputs = Inputs {
                probe_status,
                native_prerequisites: Facts {
                    plugin_supported,
                    plugin_installed: None,
                    required_hook_coverage_satisfied: Some(false),
                    ..HEALTHY
                },
                ..partial_coverage()
            };
            check(
                &inputs,
                Ok(gap(
                    Gap::CoverageDoesNotDetermineFallback,
                    Reason::InsufficientCoverage,
                )),
            );
        }
    }
    for hooks_allowed_by_admin_policy in [None, Some(false)] {
        let inputs = Inputs {
            native_prerequisites: Facts {
                hooks_allowed_by_admin_policy,
                ..partial_coverage().native_prerequisites
            },
            ..partial_coverage()
        };
        let expected = if hooks_allowed_by_admin_policy.is_none() {
            gap(
                Gap::CoverageDoesNotDetermineFallback,
                Reason::InsufficientCoverage,
            )
        } else {
            selected(Mode::Supervised, Reason::HooksExcludedByAdminPolicy)
        };
        check(&inputs, Ok(expected));
    }
}

#[test]
fn failed_unknown_evidence_and_contradictory_fallback_reason_are_not_rewritten() {
    let failed = Inputs {
        probe_status: Probe::Failed,
        stale_reason: Stale::ProbeFailed,
        native_prerequisites: UNKNOWN,
        hook_coverage_class: Coverage::Unknown,
        ..healthy()
    };
    check(&failed, Ok(selected(Mode::Supervised, Reason::Unknown)));
    check(
        &Inputs {
            report_validity_proven_current: false,
            ..failed.clone()
        },
        Ok(Outcome::ReprobeRequired),
    );
    check(
        &Inputs {
            native_prerequisites: Facts {
                plugin_installed: Some(false),
                ..UNKNOWN
            },
            ..failed.clone()
        },
        Ok(selected(Mode::Supervised, Reason::PluginNotInstalled)),
    );
    check(
        &Inputs {
            native_prerequisites: HEALTHY,
            ..failed
        },
        Err(Error::ConservativeFallbackHasNoInactiveReason),
    );
    // The prerequisite policy includes plugin support, the reason policy does
    // not. Preserve this mismatch as an error, never manufacture UNKNOWN.
    check(
        &Inputs {
            probe_status: Probe::Partial,
            native_prerequisites: Facts {
                plugin_supported: None,
                ..HEALTHY
            },
            ..healthy()
        },
        Err(Error::ConservativeFallbackHasNoInactiveReason),
    );
    check(
        &Inputs {
            probe_status: Probe::Partial,
            native_prerequisites: Facts {
                plugin_supported: Some(false),
                ..HEALTHY
            },
            ..healthy()
        },
        Ok(gap(Gap::NoUniqueFallbackRule, Reason::None)),
    );
}
