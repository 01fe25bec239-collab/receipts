use super::{
    ActivationIdentityFields, ActivationStateKind,
    ActivationStateKind::{ActivatedKnown, LoggedOut, NeverActivated},
    LicensingServiceAvailability,
    LicensingServiceAvailability::{Available, Unavailable},
    LocalClockEvidence::{EarlierThanLastObservedServerTime, NoRollbackDetected},
    ObservedEntitlementEvidence,
    ObservedEntitlementEvidence::{Corrupt, Indeterminate, Missing, Verified},
    ProductEntitlementState,
    ProductEntitlementState::{EntitlementUnknown, Free, ProActive, ProExpired, ProGrace},
    VerifiedEntitlementTemporalClass,
    VerifiedEntitlementTemporalClass::{
        PastPermittedGrace, WithinActiveValidity, WithinApplicableSignedOfflineGrace,
    },
    VerifiedProductEntitlement, resolve_product_entitlement_state,
};

const SERVICES: [LicensingServiceAvailability; 2] = [Available, Unavailable];
const TEMPORAL_ROWS: [(VerifiedEntitlementTemporalClass, ProductEntitlementState); 3] = [
    (WithinActiveValidity, ProActive),
    (WithinApplicableSignedOfflineGrace, ProGrace),
    (PastPermittedGrace, ProExpired),
];

fn entitlement(tier: &str, capabilities: &[&str]) -> VerifiedProductEntitlement {
    use super::verification_vectors::*;
    let raw = match (tier, capabilities) {
        ("pro", []) => RESOLVER_0_0,
        ("pro", ["future.new_capability"]) => RESOLVER_0_1,
        ("enterprise.future", []) => RESOLVER_1_0,
        ("enterprise.future", ["future.new_capability"]) => RESOLVER_1_1,
        ("free", []) => RESOLVER_2_0,
        ("free", ["future.new_capability"]) => RESOLVER_2_1,
        _ => panic!("missing static verification vector"),
    };
    super::EntitlementVerifier::new([(
        super::ProductEntitlementKeyId::new("k".into()).unwrap(),
        PUBLIC_KEY,
    )])
    .unwrap()
    .reverify_cached(
        raw.as_bytes(),
        &super::ProductEntitlementSubjectId::new("account-001".into()).unwrap(),
    )
    .unwrap()
}

fn evidence_rows(fields: &VerifiedProductEntitlement) -> [ObservedEntitlementEvidence<'_>; 6] {
    [
        Missing,
        Corrupt,
        Indeterminate,
        Verified {
            entitlement: fields,
            temporal_class: WithinActiveValidity,
        },
        Verified {
            entitlement: fields,
            temporal_class: WithinApplicableSignedOfflineGrace,
        },
        Verified {
            entitlement: fields,
            temporal_class: PastPermittedGrace,
        },
    ]
}

#[test]
fn complete_72_row_truth_table_including_36_rollback_unknown_rows() {
    let fields = entitlement("pro", &[]);
    // Columns: Missing, Corrupt, Indeterminate, VerifiedActive, VerifiedGrace, VerifiedExpired.
    let rows = [
        (
            NeverActivated,
            [
                Free,
                Free,
                EntitlementUnknown,
                ProActive,
                ProGrace,
                ProExpired,
            ],
        ),
        (
            ActivatedKnown,
            [
                EntitlementUnknown,
                EntitlementUnknown,
                EntitlementUnknown,
                ProActive,
                ProGrace,
                ProExpired,
            ],
        ),
        (
            LoggedOut,
            [
                Free,
                Free,
                EntitlementUnknown,
                ProActive,
                ProGrace,
                ProExpired,
            ],
        ),
    ];
    assert_eq!(
        rows.map(|(activation, _)| activation),
        ActivationStateKind::ALL
    );
    let mut row_count = 0;
    let mut rollback_count = 0;
    for (state, expected_states) in rows {
        let activation = ActivationIdentityFields::new(state, None, None);
        for (evidence, expected) in evidence_rows(&fields).into_iter().zip(expected_states) {
            for service in SERVICES {
                for (clock, result) in [
                    (NoRollbackDetected, expected),
                    (EarlierThanLastObservedServerTime, EntitlementUnknown),
                ] {
                    assert_eq!(
                        resolve_product_entitlement_state(&activation, evidence, service, clock),
                        result,
                        "{state:?}, {evidence:?}, {service:?}, {clock:?}"
                    );
                    row_count += 1;
                    if clock == EarlierThanLastObservedServerTime {
                        rollback_count += 1;
                    }
                }
            }
        }
    }
    assert_eq!(row_count, 72);
    assert_eq!(rollback_count, 36);
}

fn assert_both_services(
    state: ActivationStateKind,
    evidence: ObservedEntitlementEvidence<'_>,
    expected: ProductEntitlementState,
) {
    let activation = ActivationIdentityFields::new(state, None, None);
    for service in SERVICES {
        assert_eq!(
            resolve_product_entitlement_state(&activation, evidence, service, NoRollbackDetected),
            expected,
            "{state:?}, {evidence:?}, {service:?}"
        );
    }
}

#[test]
fn indeterminate_never_activated_preserves_unknown() {
    assert_both_services(NeverActivated, Indeterminate, EntitlementUnknown);
}

#[test]
fn indeterminate_logged_out_preserves_unknown() {
    assert_both_services(LoggedOut, Indeterminate, EntitlementUnknown);
}

#[test]
fn indeterminate_activated_known_preserves_unknown() {
    assert_both_services(ActivatedKnown, Indeterminate, EntitlementUnknown);
}

#[test]
fn never_activated_missing_is_free_online_and_offline() {
    assert_both_services(NeverActivated, Missing, Free);
}

#[test]
fn logged_out_missing_is_free_online_and_offline() {
    assert_both_services(LoggedOut, Missing, Free);
}

#[test]
fn activated_known_missing_is_unknown_offline_and_online() {
    assert_both_services(ActivatedKnown, Missing, EntitlementUnknown);
}

#[test]
fn activated_known_corrupt_is_unknown_offline_and_online() {
    assert_both_services(ActivatedKnown, Corrupt, EntitlementUnknown);
}

#[test]
fn outage_does_not_create_grace() {
    let fields = entitlement("pro", &[]);
    for state in ActivationStateKind::ALL {
        let activation = ActivationIdentityFields::new(state, None, None);
        for evidence in [Missing, Corrupt] {
            assert_ne!(
                resolve_product_entitlement_state(
                    &activation,
                    evidence,
                    Unavailable,
                    NoRollbackDetected
                ),
                ProGrace
            );
        }
        for (temporal_class, expected) in [
            (WithinActiveValidity, ProActive),
            (PastPermittedGrace, ProExpired),
        ] {
            assert_eq!(
                resolve_product_entitlement_state(
                    &activation,
                    Verified {
                        entitlement: &fields,
                        temporal_class
                    },
                    Unavailable,
                    NoRollbackDetected
                ),
                expected
            );
        }
    }
}

#[test]
fn verified_future_tiers_and_capabilities_preserve_temporal_authority() {
    for tier in ["pro", "enterprise.future", "free"] {
        for capabilities in [&[][..], &["future.new_capability"][..]] {
            let fields = entitlement(tier, capabilities);
            for state in ActivationStateKind::ALL {
                for (temporal_class, expected) in TEMPORAL_ROWS {
                    assert_both_services(
                        state,
                        Verified {
                            entitlement: &fields,
                            temporal_class,
                        },
                        expected,
                    );
                }
            }
        }
    }
}

#[test]
fn activation_subject_and_last_known_tier_do_not_influence_resolution() {
    let fields = entitlement("enterprise.future", &["future.new_capability"]);
    for state in ActivationStateKind::ALL {
        let baseline = ActivationIdentityFields::new(state, None, None);
        for subject in [
            None,
            Some("account-001"),
            Some("different-account"),
            Some(""),
        ] {
            for tier in [
                None,
                Some("free"),
                Some("pro"),
                Some("enterprise.future"),
                Some(""),
            ] {
                // Consistency validation is separate, including NeverActivated with a subject.
                let varied = ActivationIdentityFields::new(
                    state,
                    subject.map(str::to_owned),
                    tier.map(str::to_owned),
                );
                for evidence in evidence_rows(&fields) {
                    for service in SERVICES {
                        for clock in [NoRollbackDetected, EarlierThanLastObservedServerTime] {
                            assert_eq!(
                                resolve_product_entitlement_state(
                                    &varied, evidence, service, clock
                                ),
                                resolve_product_entitlement_state(
                                    &baseline, evidence, service, clock
                                ),
                                "{varied:?}, {evidence:?}, {service:?}, {clock:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
