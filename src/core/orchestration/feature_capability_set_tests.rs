use super::{
    FeatureAdmissionDecision, FeatureAdmissionOutcome, FeatureCapability, FeatureCapabilitySet,
    FeatureCapabilitySetError as Error, FeatureCapabilityStatus as Status, OrchestrationDateTimeV1,
};
use crate::graph::CapabilityName;
use receipts_state::entitlement::{ActivationStateKind, ProductEntitlementState};

fn feature(id: &str) -> FeatureCapability {
    FeatureCapability::try_new(
        CapabilityName::new(id).unwrap(),
        "name",
        "free",
        "description",
        None,
        None,
    )
    .unwrap()
}

#[test]
fn catalog_version_is_canonical_positive_unbounded_decimal() {
    for value in ["1", "42", "18446744073709551616", &"9".repeat(10_000)] {
        let set = FeatureCapabilitySet::try_new(value, vec![feature("graph.core")]).unwrap();
        assert_eq!(set.catalog_version(), value);
    }
    for value in [
        "", "0", "00", "01", "001", "+1", "-1", "1.0", "1e0", " 1", "1 ", "1\n", "１", "١", "1é",
        "1🙂",
    ] {
        assert_eq!(
            FeatureCapabilitySet::try_new(value, vec![feature("graph.core")]),
            Err(Error::InvalidCatalogVersion),
            "{value:?}"
        );
    }
}

#[test]
fn required_collections_and_strings_reject_empty_values() {
    assert_eq!(
        FeatureCapabilitySet::try_new("1", vec![]),
        Err(Error::EmptyFeatures)
    );
    let id = || CapabilityName::new("graph.core").unwrap();
    assert_eq!(
        FeatureCapability::try_new(id(), "", "", "description", None, None),
        Err(Error::EmptyTierRequired)
    );
    assert_eq!(
        FeatureCapability::try_new(id(), "", "free", "", None, None),
        Err(Error::EmptyDescription)
    );
}

#[test]
fn feature_fields_and_optional_states_are_preserved_exactly() {
    let id = CapabilityName::new("orchestration.multi_runtime").unwrap();
    let absent = FeatureCapability::try_new(id.clone(), "", " pro ", "界", None, None).unwrap();
    assert_eq!(absent.capability_id(), &id);
    assert_eq!(absent.name(), "");
    assert_eq!(absent.tier_required(), " pro ");
    assert_eq!(absent.description(), "界");
    assert_eq!(absent.status(), None);
    assert_eq!(absent.upgrade_hint(), None);

    let present = FeatureCapability::try_new(
        id,
        "",
        "free",
        "description",
        Some(Status::AvailableFree),
        Some(String::new()),
    )
    .unwrap();
    assert_eq!(present.status(), Some(Status::AvailableFree));
    assert_eq!(present.upgrade_hint(), Some(""));
    assert_ne!(absent, present);
}

#[test]
fn status_vocabulary_has_exactly_the_eight_schema_values() {
    assert_eq!(
        Status::ALL.map(Status::as_str),
        [
            "AVAILABLE_FREE",
            "AVAILABLE_ENTITLED",
            "LOCKED_REQUIRES_PRO",
            "UNAVAILABLE_PROVIDER",
            "UNAVAILABLE_POLICY",
            "UNAVAILABLE_HOST",
            "UNAVAILABLE_RUNTIME",
            "BLOCKED_SAFETY",
        ]
    );
    for status in Status::ALL {
        let record = FeatureCapability::try_new(
            CapabilityName::new("graph.core").unwrap(),
            "",
            "free",
            "description",
            Some(status),
            Some("hint".to_owned()),
        )
        .unwrap();
        assert_eq!(record.status(), Some(status));
        assert_eq!(record.upgrade_hint(), Some("hint"));
    }
}

#[test]
fn canonical_capability_is_shared_with_admission_and_never_normalized() {
    for invalid in [
        "",
        "graph",
        "Graph.core",
        "graph..core",
        "graph.core ",
        "graph.界",
    ] {
        assert!(CapabilityName::new(invalid).is_err(), "{invalid:?}");
    }
    let capability = CapabilityName::new("a0_.b.c_2").unwrap();
    let catalog =
        FeatureCapability::try_new(capability.clone(), "", "free", "d", None, None).unwrap();
    let admission = FeatureAdmissionDecision::try_new(
        "decision",
        capability,
        FeatureAdmissionOutcome::Allow,
        ProductEntitlementState::Free,
        ActivationStateKind::NeverActivated,
        None,
        None,
        None,
        None,
        OrchestrationDateTimeV1::try_new("2026-09-20T00:00:00Z").unwrap(),
    )
    .unwrap();
    assert_eq!(catalog.capability_id(), admission.capability_id());
    assert_eq!(catalog.capability_id().as_str(), "a0_.b.c_2");
}

#[test]
fn feature_order_and_duplicates_are_retained() {
    let a = feature("graph.core");
    let b = feature("orchestration.multi_runtime");
    let set = FeatureCapabilitySet::try_new("7", vec![a.clone(), b.clone(), a.clone()]).unwrap();
    assert_eq!(set.features(), &[a.clone(), b, a]);
    assert_eq!(set.features().len(), 3);
}
