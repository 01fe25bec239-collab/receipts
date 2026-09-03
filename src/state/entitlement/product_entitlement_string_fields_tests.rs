use super::{
    ProductCapabilityId, ProductEntitlementKeyId, ProductEntitlementSignature,
    ProductEntitlementStringFields, ProductEntitlementSubjectId, ProductTierId,
};

fn subject(value: &str) -> ProductEntitlementSubjectId {
    ProductEntitlementSubjectId::new(value.to_string()).expect("valid subject must pass")
}

fn tier(value: &str) -> ProductTierId {
    ProductTierId::new(value.to_string()).expect("valid tier must pass")
}

fn capability(value: &str) -> ProductCapabilityId {
    ProductCapabilityId::new(value.to_string()).expect("valid capability must pass")
}

fn key(value: &str) -> ProductEntitlementKeyId {
    ProductEntitlementKeyId::new(value.to_string()).expect("valid key id must pass")
}

fn signature(value: &str) -> ProductEntitlementSignature {
    ProductEntitlementSignature::new(value.to_string()).expect("valid signature must pass")
}

fn capability_strs(caps: &[ProductCapabilityId]) -> Vec<&str> {
    caps.iter().map(ProductCapabilityId::as_str).collect()
}

#[test]
fn string_fields_01_basic_composition_round_trips_every_accessor() {
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![capability("graph.core"), capability("future.first")],
        key("key-001"),
        signature("sig-001"),
        Some("device-001".to_string()),
    );

    assert_eq!(fields.subject_id().as_str(), "acct-001");
    assert_eq!(fields.tier_id().as_str(), "pro");
    assert_eq!(
        capability_strs(fields.capabilities()),
        vec!["graph.core", "future.first"]
    );
    assert_eq!(fields.key_id().as_str(), "key-001");
    assert_eq!(fields.signature().as_str(), "sig-001");
    assert_eq!(fields.device_binding(), Some("device-001"));
}

#[test]
fn string_fields_02_validated_subject_is_reused_unchanged() {
    let id = ProductEntitlementSubjectId::new("  padded subject  ".to_string())
        .expect("valid subject must pass");
    let expected = id.as_str().to_string();
    let fields = ProductEntitlementStringFields::new(
        id,
        tier("pro"),
        vec![capability("graph.core")],
        key("key-001"),
        signature("sig-001"),
        None,
    );

    assert_eq!(fields.subject_id().as_str(), expected);
    assert_eq!(fields.subject_id().as_str(), "  padded subject  ");
}

#[test]
fn string_fields_03_validated_tier_is_reused_including_future_tier() {
    let id = ProductTierId::new("enterprise.future".to_string()).expect("future tier must pass");
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        id,
        vec![capability("graph.core")],
        key("key-001"),
        signature("sig-001"),
        None,
    );

    assert_eq!(fields.tier_id().as_str(), "enterprise.future");
}

#[test]
fn string_fields_04_validated_capability_is_reused_including_future() {
    let cap = ProductCapabilityId::new("future.new_capability".to_string()).expect("must pass");
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![cap],
        key("key-001"),
        signature("sig-001"),
        None,
    );

    assert_eq!(
        capability_strs(fields.capabilities()),
        vec!["future.new_capability"]
    );
}

#[test]
fn string_fields_05_validated_key_and_signature_are_reused_unchanged() {
    let key_id =
        ProductEntitlementKeyId::new("  Key/ID:v7 with spaces  ".to_string()).expect("must pass");
    let sig = ProductEntitlementSignature::new("  opaque:::with spaces  ".to_string())
        .expect("must pass");
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![capability("graph.core")],
        key_id,
        sig,
        None,
    );

    assert_eq!(fields.key_id().as_str(), "  Key/ID:v7 with spaces  ");
    assert_eq!(fields.signature().as_str(), "  opaque:::with spaces  ");
}

#[test]
fn string_fields_06_empty_capability_array_is_accepted() {
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        Vec::new(),
        key("key-001"),
        signature("sig-001"),
        None,
    );

    assert!(fields.capabilities().is_empty());
    assert_eq!(capability_strs(fields.capabilities()), Vec::<&str>::new());
}

#[test]
fn string_fields_07_duplicate_capabilities_are_preserved() {
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![capability("graph.core"), capability("graph.core")],
        key("key-001"),
        signature("sig-001"),
        None,
    );

    assert_eq!(fields.capabilities().len(), 2);
    assert_eq!(
        capability_strs(fields.capabilities()),
        vec!["graph.core", "graph.core"]
    );
}

#[test]
fn string_fields_08_capability_order_is_preserved_exactly() {
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![
            capability("future.first"),
            capability("graph.core"),
            capability("future.second"),
            capability("graph.core"),
        ],
        key("key-001"),
        signature("sig-001"),
        None,
    );

    assert_eq!(
        capability_strs(fields.capabilities()),
        vec!["future.first", "graph.core", "future.second", "graph.core"]
    );
}

#[test]
fn string_fields_09_device_binding_none_is_accepted() {
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![capability("graph.core")],
        key("key-001"),
        signature("sig-001"),
        None,
    );

    assert_eq!(fields.device_binding(), None);
}

#[test]
fn string_fields_10_device_binding_empty_string_stays_some_empty() {
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![capability("graph.core")],
        key("key-001"),
        signature("sig-001"),
        Some(String::new()),
    );

    assert_eq!(fields.device_binding(), Some(""));
}

#[test]
fn string_fields_11_device_binding_whitespace_is_preserved() {
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![capability("graph.core")],
        key("key-001"),
        signature("sig-001"),
        Some(" ".to_owned()),
    );

    assert_eq!(fields.device_binding(), Some(" "));
}

#[test]
fn string_fields_12_device_binding_arbitrary_value_is_preserved_exactly() {
    let raw = " future/device:value ??? ".to_string();
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![capability("graph.core")],
        key("key-001"),
        signature("sig-001"),
        Some(raw.clone()),
    );

    assert_eq!(fields.device_binding(), Some(raw.as_str()));
    assert_eq!(fields.device_binding(), Some(" future/device:value ??? "));
}

#[test]
fn string_fields_13_extensible_tier_and_capability_are_accepted() {
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("enterprise.future"),
        vec![capability("future.new_capability")],
        key("key-001"),
        signature("sig-001"),
        None,
    );

    assert_eq!(fields.tier_id().as_str(), "enterprise.future");
    assert_eq!(
        capability_strs(fields.capabilities()),
        vec!["future.new_capability"]
    );
}

#[test]
fn string_fields_14_duplicate_and_order_combined_are_preserved() {
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![
            capability("future.alpha"),
            capability("graph.core"),
            capability("future.alpha"),
            capability("a.b"),
        ],
        key("key-001"),
        signature("sig-001"),
        None,
    );

    assert_eq!(
        capability_strs(fields.capabilities()),
        vec!["future.alpha", "graph.core", "future.alpha", "a.b"]
    );
    assert_eq!(fields.capabilities().len(), 4);
}

#[test]
fn string_fields_15_clone_and_equality_preserve_composition() {
    let fields = ProductEntitlementStringFields::new(
        subject("acct-001"),
        tier("pro"),
        vec![capability("graph.core")],
        key("key-001"),
        signature("sig-001"),
        Some("device-001".to_string()),
    );
    let cloned = fields.clone();

    assert_eq!(fields, cloned);
    assert_eq!(cloned.subject_id().as_str(), "acct-001");
    assert_eq!(cloned.device_binding(), Some("device-001"));
}
