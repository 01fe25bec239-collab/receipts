use super::{
    FeatureAdmissionDecision, FeatureAdmissionDecisionError, FeatureAdmissionOutcome,
    FeatureAdmissionUpgradeInfo, OrchestrationDateTimeV1,
};
use crate::graph::CapabilityName;
use receipts_state::entitlement::{ActivationStateKind, ProductEntitlementState};

fn decision(id: &str) -> Result<FeatureAdmissionDecision, FeatureAdmissionDecisionError> {
    FeatureAdmissionDecision::try_new(
        id,
        CapabilityName::new("graph.core").unwrap(),
        FeatureAdmissionOutcome::Allow,
        ProductEntitlementState::Free,
        ActivationStateKind::NeverActivated,
        None,
        None,
        None,
        None,
        OrchestrationDateTimeV1::try_new("2026-09-20t00:00:00.100-00:00").unwrap(),
    )
}

#[test]
fn outcome_vocabulary_is_exactly_four_entitlement_only_values() {
    assert_eq!(
        FeatureAdmissionOutcome::ALL.map(|value| value.as_str()),
        [
            "ALLOW",
            "LOCKED_REQUIRES_PRO",
            "ENTITLEMENT_UNKNOWN",
            "ENTITLEMENT_EXPIRED"
        ]
    );
    // Exhaustive match also prevents an unlisted fifth variant from compiling.
    for outcome in FeatureAdmissionOutcome::ALL {
        let expected = match outcome {
            FeatureAdmissionOutcome::Allow => "ALLOW",
            FeatureAdmissionOutcome::LockedRequiresPro => "LOCKED_REQUIRES_PRO",
            FeatureAdmissionOutcome::EntitlementUnknown => "ENTITLEMENT_UNKNOWN",
            FeatureAdmissionOutcome::EntitlementExpired => "ENTITLEMENT_EXPIRED",
        };
        assert_eq!(outcome.as_str(), expected);
    }
}

#[test]
fn decision_id_rejects_empty_and_201_scalars() {
    for (id, character_count) in [(String::new(), 0), ("界".repeat(201), 201)] {
        assert_eq!(
            decision(&id),
            Err(FeatureAdmissionDecisionError::DecisionIdLengthOutOfRange { character_count })
        );
    }
}

#[test]
fn decision_id_counts_scalars_and_preserves_accepted_text() {
    for id in [
        "x".to_owned(),
        " ".to_owned(),
        "界".repeat(200),
        "e\u{301}".repeat(100),
        " \nMiXeD\0界 ".to_owned(),
    ] {
        assert_eq!(decision(&id).unwrap().decision_id(), id);
    }
}

#[test]
fn canonical_capability_is_stored_exactly() {
    for value in ["graph.core", "orchestration.multi_runtime", "a0_.b.c_2"] {
        let capability = CapabilityName::new(value).unwrap();
        let record = FeatureAdmissionDecision::try_new(
            "id",
            capability.clone(),
            FeatureAdmissionOutcome::Allow,
            ProductEntitlementState::Free,
            ActivationStateKind::NeverActivated,
            None,
            None,
            None,
            None,
            decision("id").unwrap().decided_at().clone(),
        )
        .unwrap();
        assert_eq!(record.capability_id(), &capability);
        assert_eq!(record.capability_id().as_str(), value);
    }
    for malformed in [
        "",
        "graph",
        "Graph.core",
        "graph..core",
        "graph.core ",
        "graph.1core",
        "graph.界",
    ] {
        assert!(CapabilityName::new(malformed).is_err());
    }
}

#[test]
fn canonical_state_values_and_outcomes_are_passive_for_every_combination() {
    for entitlement in ProductEntitlementState::ALL {
        for activation in ActivationStateKind::ALL {
            for outcome in FeatureAdmissionOutcome::ALL {
                for permitted in [None, Some(false), Some(true)] {
                    let record = FeatureAdmissionDecision::try_new(
                        "id",
                        CapabilityName::new("graph.core").unwrap(),
                        outcome,
                        entitlement,
                        activation,
                        None,
                        None,
                        None,
                        permitted,
                        decision("id").unwrap().decided_at().clone(),
                    )
                    .unwrap();
                    assert_eq!(record.entitlement_state(), entitlement);
                    assert_eq!(record.activation_state(), activation);
                    assert_eq!(record.outcome(), outcome);
                    assert_eq!(record.dispatch_permitted(), permitted);
                }
            }
        }
    }
}

#[test]
fn decided_at_preserves_caller_supplied_lexical_evidence() {
    for value in [
        "2026-09-20t00:00:00.100z",
        "2026-09-20T05:30:00.100+05:30",
        "0000-01-01T00:00:60-00:00",
    ] {
        let timestamp = OrchestrationDateTimeV1::try_new(value).unwrap();
        let record = FeatureAdmissionDecision::try_new(
            "id",
            CapabilityName::new("graph.core").unwrap(),
            FeatureAdmissionOutcome::Allow,
            ProductEntitlementState::Free,
            ActivationStateKind::NeverActivated,
            None,
            None,
            None,
            None,
            timestamp.clone(),
        )
        .unwrap();
        assert_eq!(record.decided_at(), &timestamp);
        assert_eq!(record.decided_at().as_str(), value);
    }
}

#[test]
fn optional_strings_preserve_absence_empty_and_arbitrary_contents() {
    for tier in [None, Some(""), Some("pro"), Some("  pro  ")] {
        for reason in [None, Some(""), Some(" \nArbitrary 界\0 reason\t ")] {
            let record = FeatureAdmissionDecision::try_new(
                "id",
                CapabilityName::new("graph.core").unwrap(),
                FeatureAdmissionOutcome::Allow,
                ProductEntitlementState::Free,
                ActivationStateKind::NeverActivated,
                tier.map(str::to_owned),
                reason.map(str::to_owned),
                None,
                None,
                decision("id").unwrap().decided_at().clone(),
            )
            .unwrap();
            assert_eq!(record.tier_id(), tier);
            assert_eq!(record.reason(), reason);
        }
    }
}

#[test]
fn upgrade_info_absence_null_and_strings_remain_distinct() {
    let values = [
        None,
        Some(FeatureAdmissionUpgradeInfo::Null),
        Some(FeatureAdmissionUpgradeInfo::String(String::new())),
        Some(FeatureAdmissionUpgradeInfo::String(
            "upgrade text".to_owned(),
        )),
        Some(FeatureAdmissionUpgradeInfo::String(
            " \n界\0 upgrade \t".to_owned(),
        )),
    ];
    let records = values.clone().map(|upgrade| {
        FeatureAdmissionDecision::try_new(
            "id",
            CapabilityName::new("graph.core").unwrap(),
            FeatureAdmissionOutcome::Allow,
            ProductEntitlementState::Free,
            ActivationStateKind::NeverActivated,
            None,
            None,
            upgrade,
            None,
            decision("id").unwrap().decided_at().clone(),
        )
        .unwrap()
    });
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record.upgrade_info(), values[index].as_ref());
        for other in &records[index + 1..] {
            assert_ne!(record, other);
        }
    }
}
