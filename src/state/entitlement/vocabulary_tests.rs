use super::{ActivationStateKind, ProductEntitlementState};

#[test]
fn activation_vocabulary_is_exact_ordered_and_distinct() {
    use ActivationStateKind::{ActivatedKnown, LoggedOut, NeverActivated};

    assert_eq!(
        ActivationStateKind::ALL,
        [NeverActivated, ActivatedKnown, LoggedOut]
    );
    assert_eq!(ActivationStateKind::ALL.len(), 3);
    assert_eq!(NeverActivated.as_str(), "NEVER_ACTIVATED");
    assert_eq!(ActivatedKnown.as_str(), "ACTIVATED_KNOWN");
    assert_eq!(LoggedOut.as_str(), "LOGGED_OUT");
    assert_eq!(
        ActivationStateKind::ALL.map(ActivationStateKind::as_str),
        ["NEVER_ACTIVATED", "ACTIVATED_KNOWN", "LOGGED_OUT"]
    );

    for left in 0..ActivationStateKind::ALL.len() {
        for right in left + 1..ActivationStateKind::ALL.len() {
            assert_ne!(
                ActivationStateKind::ALL[left],
                ActivationStateKind::ALL[right]
            );
            assert_ne!(
                ActivationStateKind::ALL[left].as_str(),
                ActivationStateKind::ALL[right].as_str()
            );
        }
    }
}

#[test]
fn entitlement_vocabulary_is_exact_ordered_and_distinct() {
    use ProductEntitlementState::{EntitlementUnknown, Free, ProActive, ProExpired, ProGrace};

    assert_eq!(
        ProductEntitlementState::ALL,
        [Free, ProActive, ProGrace, ProExpired, EntitlementUnknown]
    );
    assert_eq!(ProductEntitlementState::ALL.len(), 5);
    assert_eq!(Free.as_str(), "FREE");
    assert_eq!(ProActive.as_str(), "PRO_ACTIVE");
    assert_eq!(ProGrace.as_str(), "PRO_GRACE");
    assert_eq!(ProExpired.as_str(), "PRO_EXPIRED");
    assert_eq!(EntitlementUnknown.as_str(), "ENTITLEMENT_UNKNOWN");
    assert_eq!(
        ProductEntitlementState::ALL.map(ProductEntitlementState::as_str),
        [
            "FREE",
            "PRO_ACTIVE",
            "PRO_GRACE",
            "PRO_EXPIRED",
            "ENTITLEMENT_UNKNOWN",
        ]
    );

    for left in 0..ProductEntitlementState::ALL.len() {
        for right in left + 1..ProductEntitlementState::ALL.len() {
            assert_ne!(
                ProductEntitlementState::ALL[left],
                ProductEntitlementState::ALL[right]
            );
            assert_ne!(
                ProductEntitlementState::ALL[left].as_str(),
                ProductEntitlementState::ALL[right].as_str()
            );
        }
    }

    assert_ne!(EntitlementUnknown, Free);
    assert_ne!(EntitlementUnknown.as_str(), Free.as_str());
}

#[test]
fn complete_closed_surface_has_eight_unique_canonical_strings() {
    let strings = [
        ActivationStateKind::ALL[0].as_str(),
        ActivationStateKind::ALL[1].as_str(),
        ActivationStateKind::ALL[2].as_str(),
        ProductEntitlementState::ALL[0].as_str(),
        ProductEntitlementState::ALL[1].as_str(),
        ProductEntitlementState::ALL[2].as_str(),
        ProductEntitlementState::ALL[3].as_str(),
        ProductEntitlementState::ALL[4].as_str(),
    ];

    assert_eq!(ActivationStateKind::ALL.len(), 3);
    assert_eq!(ProductEntitlementState::ALL.len(), 5);
    assert_eq!(strings.len(), 8);
    for left in 0..strings.len() {
        for right in left + 1..strings.len() {
            assert_ne!(strings[left], strings[right]);
        }
    }
}
