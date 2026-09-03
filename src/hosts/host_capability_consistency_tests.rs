//! Contract tests for COMPLETE-probe capability consistency.

use super::*;

const HEALTHY: HostCapabilityConsistencyInputs = HostCapabilityConsistencyInputs {
    probe_status: HostCapabilityProbeStatus::Complete,
    plugin_supported: Some(true),
    plugin_installed: Some(true),
    hooks_supported: Some(true),
    hooks_configured: Some(true),
    hooks_enabled: Some(true),
    hook_trust_required: Some(true),
    hooks_trusted: Some(true),
};

#[test]
fn plugin_installed_requires_plugin_supported_only_for_true_antecedent() {
    assert_eq!(validate_complete_probe_consistency(HEALTHY), Ok(()));

    for plugin_supported in [Some(false), None] {
        assert_eq!(
            validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
                plugin_supported,
                ..HEALTHY
            }),
            Err(HostCapabilityConsistencyError::PluginInstalledRequiresPluginSupported)
        );
    }

    for plugin_installed in [Some(false), None] {
        assert_eq!(
            validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
                plugin_installed,
                ..HEALTHY
            }),
            Ok(())
        );
    }
}

#[test]
fn hooks_configured_requires_hooks_supported_only_for_true_antecedent() {
    assert_eq!(validate_complete_probe_consistency(HEALTHY), Ok(()));

    for hooks_supported in [Some(false), None] {
        assert_eq!(
            validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
                hooks_supported,
                ..HEALTHY
            }),
            Err(HostCapabilityConsistencyError::HooksConfiguredRequiresHooksSupported)
        );
    }

    for hooks_configured in [Some(false), None] {
        assert_eq!(
            validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
                hooks_configured,
                ..HEALTHY
            }),
            Ok(())
        );
    }
}

#[test]
fn hooks_enabled_requires_hooks_supported_only_for_true_antecedent() {
    assert_eq!(validate_complete_probe_consistency(HEALTHY), Ok(()));

    for hooks_supported in [Some(false), None] {
        assert_eq!(
            validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
                hooks_supported,
                hooks_configured: Some(false),
                ..HEALTHY
            }),
            Err(HostCapabilityConsistencyError::HooksEnabledRequiresHooksSupported)
        );
    }

    for hooks_enabled in [Some(false), None] {
        assert_eq!(
            validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
                hooks_enabled,
                hooks_configured: Some(false),
                hooks_supported: Some(false),
                ..HEALTHY
            }),
            Ok(())
        );
    }
}

#[test]
fn trust_required_requires_a_known_value_but_accepts_false() {
    for hooks_trusted in [Some(true), Some(false)] {
        assert_eq!(
            validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
                hooks_trusted,
                ..HEALTHY
            }),
            Ok(())
        );
    }

    assert_eq!(
        validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
            hooks_trusted: None,
            ..HEALTHY
        }),
        Err(HostCapabilityConsistencyError::TrustRequiredRequiresKnownTrustedState)
    );

    for hook_trust_required in [Some(false), None] {
        assert_eq!(
            validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
                hook_trust_required,
                hooks_trusted: None,
                ..HEALTHY
            }),
            Ok(())
        );
    }
}

#[test]
fn partial_and_failed_do_not_impose_complete_rules() {
    let violations = [
        HostCapabilityConsistencyInputs {
            plugin_supported: Some(false),
            ..HEALTHY
        },
        HostCapabilityConsistencyInputs {
            hooks_supported: Some(false),
            ..HEALTHY
        },
        HostCapabilityConsistencyInputs {
            hooks_supported: Some(false),
            hooks_configured: Some(false),
            ..HEALTHY
        },
        HostCapabilityConsistencyInputs {
            hooks_trusted: None,
            ..HEALTHY
        },
    ];

    for probe_status in [
        HostCapabilityProbeStatus::Partial,
        HostCapabilityProbeStatus::Failed,
    ] {
        for inputs in violations {
            assert_eq!(
                validate_complete_probe_consistency(HostCapabilityConsistencyInputs {
                    probe_status,
                    ..inputs
                }),
                Ok(())
            );
        }
    }
}

#[test]
fn simultaneous_violations_return_the_first_rule_in_schema_order() {
    let cases = [
        (
            HostCapabilityConsistencyInputs {
                plugin_supported: Some(false),
                hooks_supported: Some(false),
                ..HEALTHY
            },
            HostCapabilityConsistencyError::PluginInstalledRequiresPluginSupported,
        ),
        (
            HostCapabilityConsistencyInputs {
                plugin_installed: Some(false),
                hooks_supported: Some(false),
                ..HEALTHY
            },
            HostCapabilityConsistencyError::HooksConfiguredRequiresHooksSupported,
        ),
        (
            HostCapabilityConsistencyInputs {
                plugin_installed: Some(false),
                hooks_supported: Some(false),
                hooks_configured: Some(false),
                hooks_trusted: None,
                ..HEALTHY
            },
            HostCapabilityConsistencyError::HooksEnabledRequiresHooksSupported,
        ),
    ];

    for (inputs, expected) in cases {
        assert_eq!(validate_complete_probe_consistency(inputs), Err(expected));
    }
}

#[test]
fn schema_consistent_untrusted_state_remains_an_inactive_reason() {
    let consistency_inputs = HostCapabilityConsistencyInputs {
        hooks_trusted: Some(false),
        ..HEALTHY
    };
    assert_eq!(
        validate_complete_probe_consistency(consistency_inputs),
        Ok(())
    );

    assert_eq!(
        native_path_inactive_reason(HostCapabilityInactiveReasonInputs {
            plugin_installed: Some(true),
            hooks_supported: Some(true),
            hooks_configured: Some(true),
            hook_trust_required: Some(true),
            hooks_trusted: Some(false),
            hooks_enabled: Some(true),
            hooks_allowed_by_admin_policy: Some(true),
            required_hook_coverage_satisfied: Some(true),
            mode_override_present: false,
        }),
        HostCapabilityInactiveReason::HooksUntrusted
    );
}
