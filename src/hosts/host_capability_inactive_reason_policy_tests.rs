//! Contract tests for native-path inactive-reason precedence.

use std::collections::HashSet;

use super::*;

const HEALTHY: HostCapabilityInactiveReasonInputs = HostCapabilityInactiveReasonInputs {
    plugin_installed: Some(true),
    hooks_supported: Some(true),
    hooks_configured: Some(true),
    hook_trust_required: Some(true),
    hooks_trusted: Some(true),
    hooks_enabled: Some(true),
    hooks_allowed_by_admin_policy: Some(true),
    required_hook_coverage_satisfied: Some(true),
    mode_override_present: false,
};

#[derive(Clone, Copy)]
enum KnownCondition {
    PluginNotInstalled,
    HooksUnsupported,
    HooksNotConfigured,
    HooksUntrusted,
    HooksDisabled,
    HooksExcludedByAdminPolicy,
    InsufficientCoverage,
    ModeOverride,
}

const KNOWN_CONDITIONS: [(KnownCondition, HostCapabilityInactiveReason); 8] = [
    (
        KnownCondition::PluginNotInstalled,
        HostCapabilityInactiveReason::PluginNotInstalled,
    ),
    (
        KnownCondition::HooksUnsupported,
        HostCapabilityInactiveReason::HooksUnsupported,
    ),
    (
        KnownCondition::HooksNotConfigured,
        HostCapabilityInactiveReason::HooksNotConfigured,
    ),
    (
        KnownCondition::HooksUntrusted,
        HostCapabilityInactiveReason::HooksUntrusted,
    ),
    (
        KnownCondition::HooksDisabled,
        HostCapabilityInactiveReason::HooksDisabled,
    ),
    (
        KnownCondition::HooksExcludedByAdminPolicy,
        HostCapabilityInactiveReason::HooksExcludedByAdminPolicy,
    ),
    (
        KnownCondition::InsufficientCoverage,
        HostCapabilityInactiveReason::InsufficientCoverage,
    ),
    (
        KnownCondition::ModeOverride,
        HostCapabilityInactiveReason::ModeOverride,
    ),
];

fn with_condition(
    mut inputs: HostCapabilityInactiveReasonInputs,
    condition: KnownCondition,
) -> HostCapabilityInactiveReasonInputs {
    match condition {
        KnownCondition::PluginNotInstalled => inputs.plugin_installed = Some(false),
        KnownCondition::HooksUnsupported => inputs.hooks_supported = Some(false),
        KnownCondition::HooksNotConfigured => inputs.hooks_configured = Some(false),
        KnownCondition::HooksUntrusted => {
            inputs.hook_trust_required = Some(true);
            inputs.hooks_trusted = Some(false);
        }
        KnownCondition::HooksDisabled => inputs.hooks_enabled = Some(false),
        KnownCondition::HooksExcludedByAdminPolicy => {
            inputs.hooks_allowed_by_admin_policy = Some(false);
        }
        KnownCondition::InsufficientCoverage => {
            inputs.required_hook_coverage_satisfied = Some(false);
        }
        KnownCondition::ModeOverride => inputs.mode_override_present = true,
    }
    inputs
}

#[test]
fn all_ten_inactive_reasons_are_produced_directly() {
    let mut cases: Vec<_> = KNOWN_CONDITIONS
        .map(|(condition, expected)| (with_condition(HEALTHY, condition), expected))
        .into_iter()
        .collect();
    cases.push((HEALTHY, HostCapabilityInactiveReason::None));
    cases.push((
        HostCapabilityInactiveReasonInputs {
            plugin_installed: None,
            ..HEALTHY
        },
        HostCapabilityInactiveReason::Unknown,
    ));

    assert_eq!(cases.len(), 10);
    assert_eq!(
        cases
            .iter()
            .map(|(_, expected)| *expected)
            .collect::<HashSet<_>>(),
        HostCapabilityInactiveReason::ALL.into_iter().collect()
    );
    for (inputs, expected) in cases {
        assert_eq!(native_path_inactive_reason(inputs), expected);
    }
}

#[test]
fn all_28_pairwise_precedence_collisions_choose_the_earlier_reason() {
    let mut pair_count = 0;
    for (earlier, (earlier_condition, expected)) in KNOWN_CONDITIONS.iter().copied().enumerate() {
        for (later, (later_condition, _)) in KNOWN_CONDITIONS
            .iter()
            .copied()
            .enumerate()
            .skip(earlier + 1)
        {
            pair_count += 1;
            let inputs =
                with_condition(with_condition(HEALTHY, earlier_condition), later_condition);
            assert_eq!(
                native_path_inactive_reason(inputs),
                expected,
                "precedence pair {earlier} over {later}"
            );
        }
    }
    assert_eq!(pair_count, 28);
}

#[test]
fn each_unknown_fact_fails_closed_only_after_known_reasons_and_override() {
    let unknown_cases = [
        HostCapabilityInactiveReasonInputs {
            plugin_installed: None,
            ..HEALTHY
        },
        HostCapabilityInactiveReasonInputs {
            hooks_supported: None,
            ..HEALTHY
        },
        HostCapabilityInactiveReasonInputs {
            hooks_configured: None,
            ..HEALTHY
        },
        HostCapabilityInactiveReasonInputs {
            hooks_trusted: None,
            ..HEALTHY
        },
        HostCapabilityInactiveReasonInputs {
            hook_trust_required: None,
            ..HEALTHY
        },
        HostCapabilityInactiveReasonInputs {
            hooks_enabled: None,
            ..HEALTHY
        },
        HostCapabilityInactiveReasonInputs {
            hooks_allowed_by_admin_policy: None,
            ..HEALTHY
        },
        HostCapabilityInactiveReasonInputs {
            required_hook_coverage_satisfied: None,
            ..HEALTHY
        },
    ];

    for inputs in unknown_cases {
        assert_eq!(
            native_path_inactive_reason(inputs),
            HostCapabilityInactiveReason::Unknown
        );
    }

    for (condition, expected) in KNOWN_CONDITIONS {
        let inputs = with_condition(
            HostCapabilityInactiveReasonInputs {
                plugin_installed: None,
                hooks_supported: None,
                hooks_configured: None,
                hook_trust_required: None,
                hooks_trusted: None,
                hooks_enabled: None,
                hooks_allowed_by_admin_policy: None,
                required_hook_coverage_satisfied: None,
                mode_override_present: false,
            },
            condition,
        );
        assert_eq!(native_path_inactive_reason(inputs), expected);
    }
}

#[test]
fn trust_is_required_only_when_the_caller_says_it_is() {
    assert_eq!(
        native_path_inactive_reason(HEALTHY),
        HostCapabilityInactiveReason::None
    );
    for hooks_trusted in [None, Some(false), Some(true)] {
        assert_eq!(
            native_path_inactive_reason(HostCapabilityInactiveReasonInputs {
                hook_trust_required: Some(false),
                hooks_trusted,
                ..HEALTHY
            }),
            HostCapabilityInactiveReason::None
        );
    }
    assert_eq!(
        native_path_inactive_reason(HostCapabilityInactiveReasonInputs {
            hooks_trusted: Some(false),
            ..HEALTHY
        }),
        HostCapabilityInactiveReason::HooksUntrusted
    );
    assert_eq!(
        native_path_inactive_reason(HostCapabilityInactiveReasonInputs {
            hooks_trusted: None,
            ..HEALTHY
        }),
        HostCapabilityInactiveReason::Unknown
    );
    assert_eq!(
        native_path_inactive_reason(HostCapabilityInactiveReasonInputs {
            hook_trust_required: None,
            ..HEALTHY
        }),
        HostCapabilityInactiveReason::Unknown
    );
}

#[test]
fn unknown_higher_facts_do_not_mask_known_later_failures() {
    let cases = [
        (
            HostCapabilityInactiveReasonInputs {
                plugin_installed: None,
                hooks_supported: Some(false),
                ..HEALTHY
            },
            HostCapabilityInactiveReason::HooksUnsupported,
        ),
        (
            HostCapabilityInactiveReasonInputs {
                hooks_supported: None,
                hooks_configured: Some(false),
                ..HEALTHY
            },
            HostCapabilityInactiveReason::HooksNotConfigured,
        ),
        (
            HostCapabilityInactiveReasonInputs {
                hook_trust_required: None,
                hooks_enabled: Some(false),
                ..HEALTHY
            },
            HostCapabilityInactiveReason::HooksDisabled,
        ),
        (
            HostCapabilityInactiveReasonInputs {
                required_hook_coverage_satisfied: None,
                mode_override_present: true,
                ..HEALTHY
            },
            HostCapabilityInactiveReason::ModeOverride,
        ),
    ];

    for (inputs, expected) in cases {
        assert_eq!(native_path_inactive_reason(inputs), expected);
    }
}
