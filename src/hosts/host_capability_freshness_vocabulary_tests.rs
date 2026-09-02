//! Contract tests for the host capability freshness authority vocabularies.

use std::collections::HashSet;
use std::{fmt::Debug, hash::Hash};

use super::*;

fn assert_closed<T: Copy + Debug + Eq + Hash, const N: usize>(
    all: [T; N],
    expected: [(T, &'static str); N],
    as_str: impl Fn(T) -> &'static str,
    contract: impl Fn(T) -> &'static str,
) {
    assert_eq!(all.len(), N);
    assert_eq!(all.iter().copied().collect::<HashSet<_>>().len(), N);
    assert_eq!(
        all.map(&as_str).into_iter().collect::<HashSet<_>>().len(),
        N
    );

    for (index, (value, canonical)) in expected.into_iter().enumerate() {
        assert_eq!(all[index], value);
        assert_eq!(as_str(value), canonical);
        assert_eq!(contract(value), canonical);
    }
}

#[test]
fn validity_inputs_are_exactly_authoritative() {
    let expected = [
        (
            HostCapabilityValidityInput::HostPluginIntegrationCapability,
            "host_plugin_integration_capability",
        ),
        (
            HostCapabilityValidityInput::PluginHookDefinitionIdentity,
            "plugin_hook_definition_identity",
        ),
        (
            HostCapabilityValidityInput::ExplicitHookTrustState,
            "explicit_hook_trust_state",
        ),
        (
            HostCapabilityValidityInput::HookEnablement,
            "hook_enablement",
        ),
        (HostCapabilityValidityInput::AdminPolicy, "admin_policy"),
        (
            HostCapabilityValidityInput::RequiredLifecycleEventCoverage,
            "required_lifecycle_event_coverage",
        ),
        (
            HostCapabilityValidityInput::RelevantConfiguration,
            "relevant_configuration",
        ),
    ];

    assert_closed(
        HostCapabilityValidityInput::ALL,
        expected,
        HostCapabilityValidityInput::as_str,
        |value| match value {
            HostCapabilityValidityInput::HostPluginIntegrationCapability => {
                "host_plugin_integration_capability"
            }
            HostCapabilityValidityInput::PluginHookDefinitionIdentity => {
                "plugin_hook_definition_identity"
            }
            HostCapabilityValidityInput::ExplicitHookTrustState => "explicit_hook_trust_state",
            HostCapabilityValidityInput::HookEnablement => "hook_enablement",
            HostCapabilityValidityInput::AdminPolicy => "admin_policy",
            HostCapabilityValidityInput::RequiredLifecycleEventCoverage => {
                "required_lifecycle_event_coverage"
            }
            HostCapabilityValidityInput::RelevantConfiguration => "relevant_configuration",
        },
    );
}

#[test]
fn reprobe_triggers_are_exactly_authoritative() {
    let expected = [
        (
            HostCapabilityReprobeTrigger::InitialInstall,
            "INITIAL_INSTALL",
        ),
        (
            HostCapabilityReprobeTrigger::PluginInstallOrRemoval,
            "PLUGIN_INSTALL_OR_REMOVAL",
        ),
        (HostCapabilityReprobeTrigger::PluginUpdate, "PLUGIN_UPDATE"),
        (
            HostCapabilityReprobeTrigger::HookDefinitionChange,
            "HOOK_DEFINITION_CHANGE",
        ),
        (
            HostCapabilityReprobeTrigger::HostVersionChange,
            "HOST_VERSION_CHANGE",
        ),
        (
            HostCapabilityReprobeTrigger::TrustStateChange,
            "TRUST_STATE_CHANGE",
        ),
        (
            HostCapabilityReprobeTrigger::HookEnablementOrConfigChange,
            "HOOK_ENABLEMENT_OR_CONFIG_CHANGE",
        ),
        (
            HostCapabilityReprobeTrigger::AdminPolicyChange,
            "ADMIN_POLICY_CHANGE",
        ),
        (
            HostCapabilityReprobeTrigger::SessionStartOrResume,
            "SESSION_START_OR_RESUME",
        ),
        (
            HostCapabilityReprobeTrigger::UnprovenValidityBeforeEmbedded,
            "UNPROVEN_VALIDITY_BEFORE_EMBEDDED",
        ),
    ];

    assert_closed(
        HostCapabilityReprobeTrigger::ALL,
        expected,
        HostCapabilityReprobeTrigger::as_str,
        |value| match value {
            HostCapabilityReprobeTrigger::InitialInstall => "INITIAL_INSTALL",
            HostCapabilityReprobeTrigger::PluginInstallOrRemoval => "PLUGIN_INSTALL_OR_REMOVAL",
            HostCapabilityReprobeTrigger::PluginUpdate => "PLUGIN_UPDATE",
            HostCapabilityReprobeTrigger::HookDefinitionChange => "HOOK_DEFINITION_CHANGE",
            HostCapabilityReprobeTrigger::HostVersionChange => "HOST_VERSION_CHANGE",
            HostCapabilityReprobeTrigger::TrustStateChange => "TRUST_STATE_CHANGE",
            HostCapabilityReprobeTrigger::HookEnablementOrConfigChange => {
                "HOOK_ENABLEMENT_OR_CONFIG_CHANGE"
            }
            HostCapabilityReprobeTrigger::AdminPolicyChange => "ADMIN_POLICY_CHANGE",
            HostCapabilityReprobeTrigger::SessionStartOrResume => "SESSION_START_OR_RESUME",
            HostCapabilityReprobeTrigger::UnprovenValidityBeforeEmbedded => {
                "UNPROVEN_VALIDITY_BEFORE_EMBEDDED"
            }
        },
    );
}

#[test]
fn total_new_authority_value_count_is_17() {
    assert_eq!(
        HostCapabilityValidityInput::ALL.len() + HostCapabilityReprobeTrigger::ALL.len(),
        17
    );
}
