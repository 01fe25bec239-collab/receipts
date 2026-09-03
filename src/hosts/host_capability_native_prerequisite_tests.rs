//! Contract tests for the native-path prerequisite assessment.

use super::*;

const HEALTHY_TRUST_REQUIRED: HostCapabilityNativePrerequisiteInputs =
    HostCapabilityNativePrerequisiteInputs {
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

fn healthy_trust_not_required(
    hooks_trusted: Option<bool>,
) -> HostCapabilityNativePrerequisiteInputs {
    HostCapabilityNativePrerequisiteInputs {
        hook_trust_required: Some(false),
        hooks_trusted,
        ..HEALTHY_TRUST_REQUIRED
    }
}

#[test]
fn trust_not_required_with_unknown_trust_is_satisfied() {
    assert_eq!(
        assess_native_path_prerequisites(healthy_trust_not_required(None)),
        HostCapabilityNativePrerequisiteState::Satisfied
    );
}

#[test]
fn trust_not_required_with_distrusted_hooks_is_satisfied() {
    assert_eq!(
        assess_native_path_prerequisites(healthy_trust_not_required(Some(false))),
        HostCapabilityNativePrerequisiteState::Satisfied
    );
}

#[test]
fn trust_not_required_with_trusted_hooks_is_satisfied() {
    assert_eq!(
        assess_native_path_prerequisites(healthy_trust_not_required(Some(true))),
        HostCapabilityNativePrerequisiteState::Satisfied
    );
}

#[test]
fn trust_required_with_trusted_hooks_is_satisfied() {
    assert_eq!(
        assess_native_path_prerequisites(HEALTHY_TRUST_REQUIRED),
        HostCapabilityNativePrerequisiteState::Satisfied
    );
}

#[test]
fn trust_required_with_distrusted_hooks_is_unsatisfied() {
    assert_eq!(
        assess_native_path_prerequisites(HostCapabilityNativePrerequisiteInputs {
            hooks_trusted: Some(false),
            ..HEALTHY_TRUST_REQUIRED
        }),
        HostCapabilityNativePrerequisiteState::Unsatisfied
    );
}

#[test]
fn trust_required_with_unknown_trust_is_unknown() {
    assert_eq!(
        assess_native_path_prerequisites(HostCapabilityNativePrerequisiteInputs {
            hooks_trusted: None,
            ..HEALTHY_TRUST_REQUIRED
        }),
        HostCapabilityNativePrerequisiteState::Unknown
    );
}

#[test]
fn unresolved_trust_model_is_unknown_without_another_failure() {
    for hooks_trusted in [None, Some(false), Some(true)] {
        assert_eq!(
            assess_native_path_prerequisites(HostCapabilityNativePrerequisiteInputs {
                hook_trust_required: None,
                hooks_trusted,
                ..HEALTHY_TRUST_REQUIRED
            }),
            HostCapabilityNativePrerequisiteState::Unknown
        );
    }
}

#[test]
fn each_unconditional_fact_false_is_individually_unsatisfied() {
    let cases: [fn(
        HostCapabilityNativePrerequisiteInputs,
    ) -> HostCapabilityNativePrerequisiteInputs; 7] = [
        |inputs| HostCapabilityNativePrerequisiteInputs {
            plugin_supported: Some(false),
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            plugin_installed: Some(false),
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            hooks_supported: Some(false),
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            hooks_configured: Some(false),
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            hooks_enabled: Some(false),
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            hooks_allowed_by_admin_policy: Some(false),
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            required_hook_coverage_satisfied: Some(false),
            ..inputs
        },
    ];
    assert_eq!(cases.len(), 7);
    for apply in cases {
        assert_eq!(
            assess_native_path_prerequisites(apply(HEALTHY_TRUST_REQUIRED)),
            HostCapabilityNativePrerequisiteState::Unsatisfied
        );
    }
}

#[test]
fn each_unconditional_fact_unknown_is_individually_unknown() {
    let cases: [fn(
        HostCapabilityNativePrerequisiteInputs,
    ) -> HostCapabilityNativePrerequisiteInputs; 7] = [
        |inputs| HostCapabilityNativePrerequisiteInputs {
            plugin_supported: None,
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            plugin_installed: None,
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            hooks_supported: None,
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            hooks_configured: None,
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            hooks_enabled: None,
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            hooks_allowed_by_admin_policy: None,
            ..inputs
        },
        |inputs| HostCapabilityNativePrerequisiteInputs {
            required_hook_coverage_satisfied: None,
            ..inputs
        },
    ];
    assert_eq!(cases.len(), 7);
    for apply in cases {
        assert_eq!(
            assess_native_path_prerequisites(apply(HEALTHY_TRUST_REQUIRED)),
            HostCapabilityNativePrerequisiteState::Unknown
        );
    }
}

#[test]
fn known_false_dominates_unrelated_unknown() {
    assert_eq!(
        assess_native_path_prerequisites(HostCapabilityNativePrerequisiteInputs {
            plugin_installed: Some(false),
            hooks_enabled: None,
            ..HEALTHY_TRUST_REQUIRED
        }),
        HostCapabilityNativePrerequisiteState::Unsatisfied
    );
}

#[test]
fn unknown_plugin_support_does_not_hide_known_coverage_failure() {
    assert_eq!(
        assess_native_path_prerequisites(HostCapabilityNativePrerequisiteInputs {
            plugin_supported: None,
            required_hook_coverage_satisfied: Some(false),
            ..HEALTHY_TRUST_REQUIRED
        }),
        HostCapabilityNativePrerequisiteState::Unsatisfied
    );
}

#[test]
fn unknown_trust_model_does_not_hide_known_configuration_failure() {
    assert_eq!(
        assess_native_path_prerequisites(HostCapabilityNativePrerequisiteInputs {
            hook_trust_required: None,
            hooks_trusted: Some(true),
            hooks_configured: Some(false),
            ..HEALTHY_TRUST_REQUIRED
        }),
        HostCapabilityNativePrerequisiteState::Unsatisfied
    );
}

#[test]
fn unknown_trust_with_disabled_hooks_is_unsatisfied() {
    assert_eq!(
        assess_native_path_prerequisites(HostCapabilityNativePrerequisiteInputs {
            hooks_trusted: None,
            hooks_enabled: Some(false),
            ..HEALTHY_TRUST_REQUIRED
        }),
        HostCapabilityNativePrerequisiteState::Unsatisfied
    );
}

#[test]
fn only_hooks_trusted_may_be_missing_for_satisfied() {
    let single_missing: [HostCapabilityNativePrerequisiteInputs; 9] = [
        HostCapabilityNativePrerequisiteInputs {
            plugin_supported: None,
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            plugin_installed: None,
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            hooks_supported: None,
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            hooks_configured: None,
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            hook_trust_required: None,
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            hooks_trusted: None,
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            hooks_enabled: None,
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            hooks_allowed_by_admin_policy: None,
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            required_hook_coverage_satisfied: None,
            ..HEALTHY_TRUST_REQUIRED
        },
    ];
    for inputs in single_missing {
        assert_ne!(
            assess_native_path_prerequisites(inputs),
            HostCapabilityNativePrerequisiteState::Satisfied
        );
    }
    assert_eq!(
        assess_native_path_prerequisites(healthy_trust_not_required(None)),
        HostCapabilityNativePrerequisiteState::Satisfied
    );
}

#[test]
fn assessment_is_deterministic_for_repeated_inputs() {
    let cases = [
        healthy_trust_not_required(None),
        healthy_trust_not_required(Some(false)),
        healthy_trust_not_required(Some(true)),
        HEALTHY_TRUST_REQUIRED,
        HostCapabilityNativePrerequisiteInputs {
            hooks_trusted: Some(false),
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            hooks_trusted: None,
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            hook_trust_required: None,
            ..HEALTHY_TRUST_REQUIRED
        },
        HostCapabilityNativePrerequisiteInputs {
            plugin_installed: Some(false),
            hooks_enabled: None,
            ..HEALTHY_TRUST_REQUIRED
        },
    ];
    for inputs in cases {
        assert_eq!(
            assess_native_path_prerequisites(inputs),
            assess_native_path_prerequisites(inputs)
        );
    }
}

fn trit_to_opt(trit: usize) -> Option<bool> {
    match trit {
        0 => None,
        1 => Some(false),
        _ => Some(true),
    }
}

fn inputs_from_index(mut index: usize) -> HostCapabilityNativePrerequisiteInputs {
    let mut trits = [0; 9];
    for slot in trits.iter_mut() {
        *slot = index % 3;
        index /= 3;
    }
    HostCapabilityNativePrerequisiteInputs {
        plugin_supported: trit_to_opt(trits[0]),
        plugin_installed: trit_to_opt(trits[1]),
        hooks_supported: trit_to_opt(trits[2]),
        hooks_configured: trit_to_opt(trits[3]),
        hook_trust_required: trit_to_opt(trits[4]),
        hooks_trusted: trit_to_opt(trits[5]),
        hooks_enabled: trit_to_opt(trits[6]),
        hooks_allowed_by_admin_policy: trit_to_opt(trits[7]),
        required_hook_coverage_satisfied: trit_to_opt(trits[8]),
    }
}

fn is_proven_false(value: Option<bool>) -> bool {
    value == Some(false)
}

fn is_unresolved(value: Option<bool>) -> bool {
    value.is_none()
}

/// Independent oracle for the three-valued contract, written directly from the
/// task's decision structure rather than by reusing the implementation.
fn expected_state(
    inputs: HostCapabilityNativePrerequisiteInputs,
) -> HostCapabilityNativePrerequisiteState {
    let unconditional_failure = is_proven_false(inputs.plugin_supported)
        || is_proven_false(inputs.plugin_installed)
        || is_proven_false(inputs.hooks_supported)
        || is_proven_false(inputs.hooks_configured)
        || is_proven_false(inputs.hooks_enabled)
        || is_proven_false(inputs.hooks_allowed_by_admin_policy)
        || is_proven_false(inputs.required_hook_coverage_satisfied);
    let trust_failure =
        inputs.hook_trust_required == Some(true) && inputs.hooks_trusted == Some(false);
    if unconditional_failure || trust_failure {
        return HostCapabilityNativePrerequisiteState::Unsatisfied;
    }
    let unconditional_gap = is_unresolved(inputs.plugin_supported)
        || is_unresolved(inputs.plugin_installed)
        || is_unresolved(inputs.hooks_supported)
        || is_unresolved(inputs.hooks_configured)
        || is_unresolved(inputs.hooks_enabled)
        || is_unresolved(inputs.hooks_allowed_by_admin_policy)
        || is_unresolved(inputs.required_hook_coverage_satisfied);
    let trust_gap = is_unresolved(inputs.hook_trust_required)
        || (inputs.hook_trust_required == Some(true) && is_unresolved(inputs.hooks_trusted));
    if unconditional_gap || trust_gap {
        return HostCapabilityNativePrerequisiteState::Unknown;
    }
    HostCapabilityNativePrerequisiteState::Satisfied
}

#[test]
fn exhaustive_tri_state_proof_covers_all_19683_combinations() {
    let mut satisfied = 0u32;
    let mut unknown = 0u32;
    let mut unsatisfied = 0u32;
    let mut trust_not_required_healthy = 0u32;

    for index in 0..19683 {
        let inputs = inputs_from_index(index);
        let actual = assess_native_path_prerequisites(inputs);
        assert_eq!(actual, expected_state(inputs), "oracle mismatch at {index}");
        assert_eq!(
            actual,
            assess_native_path_prerequisites(inputs),
            "nondeterministic at {index}"
        );

        let unconditional_failure = inputs.plugin_supported == Some(false)
            || inputs.plugin_installed == Some(false)
            || inputs.hooks_supported == Some(false)
            || inputs.hooks_configured == Some(false)
            || inputs.hooks_enabled == Some(false)
            || inputs.hooks_allowed_by_admin_policy == Some(false)
            || inputs.required_hook_coverage_satisfied == Some(false);
        let trust_failure =
            inputs.hook_trust_required == Some(true) && inputs.hooks_trusted == Some(false);
        if unconditional_failure || trust_failure {
            assert_eq!(
                actual,
                HostCapabilityNativePrerequisiteState::Unsatisfied,
                "known failure must dominate at {index}"
            );
        }

        if actual == HostCapabilityNativePrerequisiteState::Unknown {
            assert!(
                !unconditional_failure && !trust_failure,
                "Unknown must never hide a known failure at {index}"
            );
            let gap = inputs.plugin_supported.is_none()
                || inputs.plugin_installed.is_none()
                || inputs.hooks_supported.is_none()
                || inputs.hooks_configured.is_none()
                || inputs.hooks_enabled.is_none()
                || inputs.hooks_allowed_by_admin_policy.is_none()
                || inputs.required_hook_coverage_satisfied.is_none()
                || inputs.hook_trust_required.is_none()
                || (inputs.hook_trust_required == Some(true) && inputs.hooks_trusted.is_none());
            assert!(gap, "Unknown requires an unresolved fact at {index}");
        }

        if actual == HostCapabilityNativePrerequisiteState::Satisfied {
            assert_eq!(inputs.plugin_supported, Some(true), "gap at {index}");
            assert_eq!(inputs.plugin_installed, Some(true), "gap at {index}");
            assert_eq!(inputs.hooks_supported, Some(true), "gap at {index}");
            assert_eq!(inputs.hooks_configured, Some(true), "gap at {index}");
            assert_eq!(inputs.hooks_enabled, Some(true), "gap at {index}");
            assert_eq!(
                inputs.hooks_allowed_by_admin_policy,
                Some(true),
                "gap at {index}"
            );
            assert_eq!(
                inputs.required_hook_coverage_satisfied,
                Some(true),
                "gap at {index}"
            );
            assert!(
                inputs.hook_trust_required == Some(false)
                    || (inputs.hook_trust_required == Some(true)
                        && inputs.hooks_trusted == Some(true)),
                "unhealthy trust must never satisfy at {index}"
            );
        }

        if inputs.hook_trust_required == Some(false)
            && inputs.plugin_supported == Some(true)
            && inputs.plugin_installed == Some(true)
            && inputs.hooks_supported == Some(true)
            && inputs.hooks_configured == Some(true)
            && inputs.hooks_enabled == Some(true)
            && inputs.hooks_allowed_by_admin_policy == Some(true)
            && inputs.required_hook_coverage_satisfied == Some(true)
        {
            trust_not_required_healthy += 1;
            assert_eq!(
                actual,
                HostCapabilityNativePrerequisiteState::Satisfied,
                "trust-not-required must ignore hooks_trusted at {index}"
            );
        }

        if inputs.hook_trust_required == Some(true) && inputs.hooks_trusted == Some(false) {
            assert_eq!(
                actual,
                HostCapabilityNativePrerequisiteState::Unsatisfied,
                "distrusted trust-required state at {index}"
            );
        }

        if inputs.hook_trust_required == Some(true)
            && inputs.hooks_trusted.is_none()
            && !unconditional_failure
        {
            assert_ne!(
                actual,
                HostCapabilityNativePrerequisiteState::Satisfied,
                "unresolved trust must never satisfy at {index}"
            );
            if !trust_failure {
                assert_eq!(
                    actual,
                    HostCapabilityNativePrerequisiteState::Unknown,
                    "unresolved trust without failure at {index}"
                );
            }
        }

        if inputs.hook_trust_required.is_none() && !unconditional_failure && !trust_failure {
            assert_eq!(
                actual,
                HostCapabilityNativePrerequisiteState::Unknown,
                "unresolved trust model without failure at {index}"
            );
        }

        match actual {
            HostCapabilityNativePrerequisiteState::Satisfied => satisfied += 1,
            HostCapabilityNativePrerequisiteState::Unknown => unknown += 1,
            HostCapabilityNativePrerequisiteState::Unsatisfied => unsatisfied += 1,
        }
    }

    assert_eq!(trust_not_required_healthy, 3);
    assert_eq!(satisfied, 4);
    assert_eq!(unknown, 1020);
    assert_eq!(unsatisfied, 18659);
    assert_eq!(satisfied + unknown + unsatisfied, 19683);
}
