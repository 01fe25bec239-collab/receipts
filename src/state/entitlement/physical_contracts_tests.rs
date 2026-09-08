use crate::CanonicalTimestampV1;

use super::{
    ActivationIdentityConsistencyError, ActivationIdentityFields, ActivationState,
    ActivationStateKind, ProductCapabilityId, ProductEntitlement, ProductEntitlementKeyId,
    ProductEntitlementSignature, ProductEntitlementStringFields, ProductEntitlementSubjectId,
    ProductEntitlementVersion, ProductTierId,
};

fn timestamp(value: &str) -> CanonicalTimestampV1 {
    CanonicalTimestampV1::parse(value).expect("test timestamp must be canonical")
}

fn string_fields(capabilities: &[&str]) -> ProductEntitlementStringFields {
    ProductEntitlementStringFields::new(
        ProductEntitlementSubjectId::new("subject".into()).unwrap(),
        ProductTierId::new("future-tier".into()).unwrap(),
        capabilities
            .iter()
            .map(|value| ProductCapabilityId::new((*value).into()).unwrap())
            .collect(),
        ProductEntitlementKeyId::new("key".into()).unwrap(),
        ProductEntitlementSignature::new("opaque-signature".into()).unwrap(),
        None,
    )
}

#[test]
fn canonical_timestamp_contract_is_reused_without_normalization() {
    let canonical = "2028-02-29T23:59:59.123456789Z";
    assert_eq!(timestamp(canonical).as_str(), canonical);

    for invalid in [
        "2028-02-29T23:59:59.123456789+00:00",
        "2028-02-29T23:59:59Z",
        "2028-02-29T23:59:59.12345678Z",
        "2028-02-30T23:59:59.123456789Z",
        "2028-02-29T23:59:60.123456789Z",
    ] {
        assert!(CanonicalTimestampV1::parse(invalid).is_err(), "{invalid}");
    }
}

#[test]
fn entitlement_version_accepts_exact_i64_positive_range() {
    assert_eq!(ProductEntitlementVersion::new(1).unwrap().get(), 1);
    assert_eq!(
        ProductEntitlementVersion::new(i64::MAX).unwrap().get(),
        i64::MAX
    );
    assert_eq!(ProductEntitlementVersion::new(0), None);
    assert_eq!(ProductEntitlementVersion::new(-1), None);
    assert_eq!(ProductEntitlementVersion::new(i64::MIN), None);
}

#[test]
fn product_entitlement_preserves_every_frozen_field() {
    let fields = ProductEntitlementStringFields::new(
        ProductEntitlementSubjectId::new("account-α".into()).unwrap(),
        ProductTierId::new("future.tier-X".into()).unwrap(),
        ["future.capability", "graph.core", "future.capability"]
            .map(|value| ProductCapabilityId::new(value.into()).unwrap())
            .into(),
        ProductEntitlementKeyId::new("key-1".into()).unwrap(),
        ProductEntitlementSignature::new("physical-data-only".into()).unwrap(),
        Some("opaque binding".into()),
    );
    let entitlement = ProductEntitlement::new(
        fields,
        timestamp("2026-01-01T00:00:00.000000000Z"),
        timestamp("2025-01-01T00:00:00.000000000Z"),
        ProductEntitlementVersion::new(1).unwrap(),
        Some(timestamp("2024-01-01T00:00:00.000000000Z")),
    );

    assert_eq!(entitlement.subject_id().as_str(), "account-α");
    assert_eq!(entitlement.tier_id().as_str(), "future.tier-X");
    assert_eq!(
        entitlement
            .capabilities()
            .iter()
            .map(ProductCapabilityId::as_str)
            .collect::<Vec<_>>(),
        ["future.capability", "graph.core", "future.capability"]
    );
    assert_eq!(
        entitlement.issued_at().as_str(),
        "2026-01-01T00:00:00.000000000Z"
    );
    assert_eq!(
        entitlement.expires_at().as_str(),
        "2025-01-01T00:00:00.000000000Z"
    );
    assert_eq!(entitlement.entitlement_version().get(), 1);
    assert_eq!(entitlement.key_id().as_str(), "key-1");
    assert_eq!(entitlement.signature().as_str(), "physical-data-only");
    assert_eq!(
        entitlement.offline_grace_until().unwrap().as_str(),
        "2024-01-01T00:00:00.000000000Z"
    );
    assert_eq!(entitlement.device_binding(), Some("opaque binding"));

    let absent = ProductEntitlement::new(
        string_fields(&[]),
        timestamp("2026-01-01T00:00:00.000000000Z"),
        timestamp("2026-01-01T00:00:00.000000000Z"),
        ProductEntitlementVersion::new(1).unwrap(),
        None,
    );
    assert!(absent.capabilities().is_empty());
    assert_eq!(absent.offline_grace_until(), None);
    assert_eq!(absent.device_binding(), None);
}

#[test]
fn activation_state_preserves_identity_rules_optionality_and_opaque_text() {
    let recorded_at = || timestamp("2026-01-01T00:00:00.000000000Z");

    for kind in [
        ActivationStateKind::ActivatedKnown,
        ActivationStateKind::LoggedOut,
    ] {
        let state = ActivationState::new(
            ActivationIdentityFields::new(kind, None, None),
            None,
            None,
            None,
            None,
            recorded_at(),
        )
        .unwrap();
        assert_eq!(state.activation_state(), kind);
        assert_eq!(state.subject_id(), None);
        assert_eq!(state.first_activated_at(), None);
        assert_eq!(state.last_known_tier_id(), None);
        assert_eq!(state.last_entitlement_seen_at(), None);
        assert_eq!(state.last_observed_server_time(), None);
        assert_eq!(state.logged_out_at(), None);
    }

    for subject in ["", " ", "用户"] {
        let state = ActivationState::new(
            ActivationIdentityFields::new(
                ActivationStateKind::ActivatedKnown,
                Some(subject.into()),
                Some("unknown future tier ".into()),
            ),
            Some(timestamp("2026-01-05T00:00:00.000000000Z")),
            Some(timestamp("2026-01-04T00:00:00.000000000Z")),
            Some(timestamp("2026-01-03T00:00:00.000000000Z")),
            Some(timestamp("2026-01-02T00:00:00.000000000Z")),
            recorded_at(),
        )
        .unwrap();
        assert_eq!(state.subject_id(), Some(subject));
        assert_eq!(state.last_known_tier_id(), Some("unknown future tier "));
        assert_eq!(
            state.first_activated_at().unwrap().as_str(),
            "2026-01-05T00:00:00.000000000Z"
        );
        assert_eq!(
            state.last_entitlement_seen_at().unwrap().as_str(),
            "2026-01-04T00:00:00.000000000Z"
        );
        assert_eq!(
            state.last_observed_server_time().unwrap().as_str(),
            "2026-01-03T00:00:00.000000000Z"
        );
        assert_eq!(
            state.logged_out_at().unwrap().as_str(),
            "2026-01-02T00:00:00.000000000Z"
        );
        assert_eq!(
            state.recorded_at().as_str(),
            "2026-01-01T00:00:00.000000000Z"
        );
    }

    let never = ActivationState::new(
        ActivationIdentityFields::new(ActivationStateKind::NeverActivated, None, None),
        None,
        None,
        None,
        None,
        recorded_at(),
    )
    .unwrap();
    assert_eq!(
        never.activation_state(),
        ActivationStateKind::NeverActivated
    );

    assert_eq!(
        ActivationState::new(
            ActivationIdentityFields::new(
                ActivationStateKind::NeverActivated,
                Some(String::new()),
                None,
            ),
            None,
            None,
            None,
            None,
            recorded_at(),
        ),
        Err(ActivationIdentityConsistencyError::NeverActivatedSubjectPresent)
    );
}
