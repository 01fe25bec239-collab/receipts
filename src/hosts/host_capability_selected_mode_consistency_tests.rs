//! Equivalent facts from directly read frozen fixtures; no JSON loading.

use super::*;
use HostCapabilityInactiveReason as Reason;
use HostCapabilityNativePrerequisiteState as Native;
use HostCapabilityProbeStatus as Probe;
use HostCapabilitySelectedMode as Mode;
use HostCapabilitySelectedModeConsistencyError as Error;
use HostCapabilitySelectedModeConsistencyInputs as Inputs;
use HostCapabilityStaleReason as Stale;

fn healthy() -> Inputs {
    Inputs {
        report_validity_proven_current: true,
        probe_status: Probe::Complete,
        stale_reason: Stale::None,
        native_prerequisite_state: Native::Satisfied,
        selected_mode: Mode::Embedded,
        mode_override: None,
        inactive_reason: Some(Reason::None),
    }
}

fn explicit_override(source: HostCapabilityModeOverrideSource) -> HostCapabilityModeOverride {
    HostCapabilityModeOverride::new(source, "fixture forces supervised".to_owned()).unwrap()
}

#[test]
fn frozen_positive_fixtures() {
    let cases = [
        ("01_codex_embedded", healthy()),
        (
            "03_codex_supervised_untrusted",
            Inputs {
                native_prerequisite_state: Native::Unsatisfied, // Required trust is false.
                selected_mode: Mode::Supervised,
                inactive_reason: Some(Reason::HooksUntrusted),
                ..healthy()
            },
        ),
        (
            "04_codex_hybrid_partial_coverage",
            Inputs {
                native_prerequisite_state: Native::Unsatisfied, // Required coverage is false.
                selected_mode: Mode::Hybrid,
                inactive_reason: Some(Reason::InsufficientCoverage),
                ..healthy()
            },
        ),
        (
            "07_healthy_explicit_override",
            Inputs {
                selected_mode: Mode::Supervised,
                mode_override: Some(explicit_override(HostCapabilityModeOverrideSource::Debug)),
                inactive_reason: Some(Reason::ModeOverride),
                ..healthy()
            },
        ),
    ];
    for (fixture, inputs) in cases {
        assert_eq!(
            validate_selected_mode_consistency(&inputs),
            Ok(()),
            "{fixture}"
        );
    }
}

#[test]
fn frozen_negative_fixtures() {
    let cases = [
        (
            "01_embedded_with_untrusted_hooks",
            Inputs {
                native_prerequisite_state: Native::Unsatisfied,
                ..healthy()
            },
            Error::EmbeddedNotEligible,
        ),
        (
            "03_embedded_with_stale_inactive_reason",
            Inputs {
                inactive_reason: Some(Reason::HooksDisabled),
                ..healthy()
            },
            Error::EmbeddedHasContradictoryInactiveReason,
        ),
        (
            "07_insufficient_coverage_embedded",
            Inputs {
                native_prerequisite_state: Native::Unsatisfied,
                ..healthy()
            },
            Error::EmbeddedNotEligible,
        ),
        (
            "08_stale_embedded",
            Inputs {
                report_validity_proven_current: false,
                stale_reason: Stale::ValidityFingerprintChanged,
                ..healthy()
            },
            Error::EmbeddedNotEligible,
        ),
        (
            "10_healthy_supervised_without_override",
            Inputs {
                selected_mode: Mode::Supervised,
                inactive_reason: Some(Reason::Unknown),
                ..healthy()
            },
            Error::HealthyNativePathDepartureRequiresModeOverride,
        ),
    ];
    for (fixture, inputs, error) in cases {
        assert_eq!(
            validate_selected_mode_consistency(&inputs),
            Err(error),
            "{fixture}"
        );
    }
}

#[test]
fn each_eligibility_dimension_independently_rejects_embedded() {
    let cases = [
        Inputs {
            report_validity_proven_current: false,
            ..healthy()
        },
        Inputs {
            probe_status: Probe::Partial,
            ..healthy()
        },
        Inputs {
            probe_status: Probe::Failed,
            ..healthy()
        },
        Inputs {
            stale_reason: Stale::ValidityFingerprintChanged,
            ..healthy()
        },
        Inputs {
            stale_reason: Stale::ValidityFingerprintUnproven,
            ..healthy()
        },
        Inputs {
            stale_reason: Stale::ProbeFailed,
            ..healthy()
        },
        Inputs {
            native_prerequisite_state: Native::Unsatisfied,
            ..healthy()
        },
        Inputs {
            native_prerequisite_state: Native::Unknown,
            ..healthy()
        },
    ];
    for inputs in cases {
        assert_eq!(
            validate_selected_mode_consistency(&inputs),
            Err(Error::EmbeddedNotEligible),
            "{inputs:?}"
        );
    }
}

#[test]
fn embedded_inactive_reason_exhaustion_and_override_non_rule() {
    let mut contradictory = 0;
    for inactive_reason in std::iter::once(None).chain(Reason::ALL.map(Some)) {
        let expected = match inactive_reason {
            None | Some(Reason::None) => Ok(()),
            _ => {
                contradictory += 1;
                Err(Error::EmbeddedHasContradictoryInactiveReason)
            }
        };
        for mode_override in std::iter::once(None).chain(
            HostCapabilityModeOverrideSource::ALL.map(|source| Some(explicit_override(source))),
        ) {
            let inputs = Inputs {
                inactive_reason,
                mode_override,
                ..healthy()
            };
            assert_eq!(
                validate_selected_mode_consistency(&inputs),
                expected,
                "{inputs:?}"
            );
        }
    }
    assert_eq!(contradictory, 9);
}

#[test]
fn healthy_departure_matrix_preserves_error_order_and_override_structure() {
    for selected_mode in [Mode::Hybrid, Mode::Supervised] {
        for inactive_reason in std::iter::once(None).chain(Reason::ALL.map(Some)) {
            let mut inputs = Inputs {
                selected_mode,
                inactive_reason,
                ..healthy()
            };
            assert_eq!(
                validate_selected_mode_consistency(&inputs),
                Err(Error::HealthyNativePathDepartureRequiresModeOverride),
                "{inputs:?}"
            );
            for source in HostCapabilityModeOverrideSource::ALL {
                for reason in ["fixture forces supervised", " ", "\t", "💾"] {
                    inputs.mode_override =
                        Some(HostCapabilityModeOverride::new(source, reason.to_owned()).unwrap());
                    let before = inputs.clone();
                    let expected = if inactive_reason == Some(Reason::ModeOverride) {
                        Ok(())
                    } else {
                        Err(Error::HealthyNativePathDepartureRequiresModeOverrideInactiveReason)
                    };
                    assert_eq!(
                        validate_selected_mode_consistency(&inputs),
                        expected,
                        "{inputs:?}"
                    );
                    assert_eq!(validate_selected_mode_consistency(&inputs), expected);
                    assert_eq!(inputs, before);
                }
            }
        }
    }
}

#[test]
fn all_72_base_combinations_delegate_eligibility_without_choosing_departure_mode() {
    let mut count = 0;
    let mut eligible_facts = Vec::new();
    for current in [false, true] {
        for probe_status in Probe::ALL {
            for stale_reason in Stale::ALL {
                for native in [Native::Satisfied, Native::Unsatisfied, Native::Unknown] {
                    count += 1;
                    let eligible = is_embedded_eligible(
                        current,
                        probe_status,
                        stale_reason,
                        native == Native::Satisfied,
                    );
                    if eligible {
                        eligible_facts.push((current, probe_status, stale_reason, native));
                    }
                    let mut inputs = Inputs {
                        report_validity_proven_current: current,
                        probe_status,
                        stale_reason,
                        native_prerequisite_state: native,
                        ..healthy()
                    };
                    assert_eq!(
                        validate_selected_mode_consistency(&inputs),
                        if eligible {
                            Ok(())
                        } else {
                            Err(Error::EmbeddedNotEligible)
                        },
                        "{inputs:?}"
                    );
                    if !eligible {
                        // No universal inactive-reason policy or override requirement
                        // applies to either caller-selected non-embedded mode.
                        for selected_mode in [Mode::Hybrid, Mode::Supervised] {
                            inputs.selected_mode = selected_mode;
                            for inactive_reason in
                                std::iter::once(None).chain(Reason::ALL.map(Some))
                            {
                                inputs.inactive_reason = inactive_reason;
                                for mode_override in [
                                    None,
                                    Some(explicit_override(HostCapabilityModeOverrideSource::User)),
                                ] {
                                    inputs.mode_override = mode_override;
                                    assert_eq!(
                                        validate_selected_mode_consistency(&inputs),
                                        Ok(()),
                                        "{inputs:?}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(count, 72);
    assert_eq!(
        eligible_facts,
        vec![(true, Probe::Complete, Stale::None, Native::Satisfied)]
    );
}
