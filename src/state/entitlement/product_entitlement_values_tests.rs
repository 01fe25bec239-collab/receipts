use super::{
    ProductCapabilityId, ProductEntitlementKeyId, ProductEntitlementSignature,
    ProductEntitlementSubjectId, ProductEntitlementValueError, ProductTierId,
};

// ---------------------------------------------------------------------------
// Subject id
// ---------------------------------------------------------------------------

#[test]
fn subject_01_empty_is_rejected() {
    let err = ProductEntitlementSubjectId::new(String::new()).expect_err("empty must fail");
    assert_eq!(err, ProductEntitlementValueError::SubjectIdEmpty);
}

#[test]
fn subject_02_single_char_is_accepted() {
    let id = ProductEntitlementSubjectId::new("a".to_string()).expect("one char must pass");
    assert_eq!(id.as_str(), "a");
    assert_eq!(id.into_inner(), "a");
}

#[test]
fn subject_03_exactly_200_ascii_chars_accepted() {
    let value = "a".repeat(200);
    assert_eq!(value.chars().count(), 200);
    let id = ProductEntitlementSubjectId::new(value.clone()).expect("200 chars must pass");
    assert_eq!(id.as_str(), value);
    assert_eq!(id.into_inner(), value);
}

#[test]
fn subject_04_201_ascii_chars_rejected() {
    let value = "a".repeat(201);
    let err = ProductEntitlementSubjectId::new(value).expect_err("201 chars must fail");
    assert_eq!(
        err,
        ProductEntitlementValueError::SubjectIdTooLong {
            observed_chars: 201
        }
    );
}

#[test]
fn subject_05_200_multibyte_chars_accepted_despite_byte_length() {
    // 'é' is 2 bytes in UTF-8, so 200 of them exceed 200 bytes.
    let value = "é".repeat(200);
    assert_eq!(value.chars().count(), 200);
    assert!(value.len() > 200);
    let id = ProductEntitlementSubjectId::new(value.clone()).expect("200 chars must pass");
    assert_eq!(id.as_str(), value);
    assert_eq!(id.into_inner(), value);
}

#[test]
fn subject_06_201_multibyte_chars_rejected() {
    let value = "é".repeat(201);
    assert_eq!(value.chars().count(), 201);
    let err = ProductEntitlementSubjectId::new(value).expect_err("201 chars must fail");
    assert_eq!(
        err,
        ProductEntitlementValueError::SubjectIdTooLong {
            observed_chars: 201
        }
    );
}

#[test]
fn subject_07_accepted_text_is_preserved_exactly() {
    let value = "Subject 123 _-./: mixed CASE".to_string();
    let id = ProductEntitlementSubjectId::new(value.clone()).expect("must pass");
    assert_eq!(id.as_str(), value);
    assert_eq!(id.into_inner(), value);
}

#[test]
fn subject_08_whitespace_is_preserved_not_trimmed() {
    let value = "  padded subject  ".to_string();
    let id = ProductEntitlementSubjectId::new(value.clone()).expect("must pass");
    assert_eq!(id.as_str(), value);
    assert_eq!(id.as_str().len(), value.len());
    assert!(id.as_str().starts_with("  "));
    assert!(id.as_str().ends_with("  "));
    assert_eq!(id.into_inner(), value);
}

// ---------------------------------------------------------------------------
// Tier id
// ---------------------------------------------------------------------------

#[test]
fn tier_01_empty_is_rejected() {
    let err = ProductTierId::new(String::new()).expect_err("empty must fail");
    assert_eq!(err, ProductEntitlementValueError::TierIdEmpty);
}

#[test]
fn tier_02_free_is_accepted() {
    let id = ProductTierId::new("free".to_string()).expect("free must pass");
    assert_eq!(id.as_str(), "free");
}

#[test]
fn tier_03_pro_is_accepted() {
    let id = ProductTierId::new("pro".to_string()).expect("pro must pass");
    assert_eq!(id.as_str(), "pro");
}

#[test]
fn tier_04_enterprise_is_accepted() {
    let id = ProductTierId::new("enterprise".to_string()).expect("enterprise must pass");
    assert_eq!(id.as_str(), "enterprise");
}

#[test]
fn tier_05_dotted_future_tier_is_accepted() {
    let id = ProductTierId::new("team.future".to_string()).expect("dotted tier must pass");
    assert_eq!(id.as_str(), "team.future");
}

#[test]
fn tier_06_whitespace_only_is_accepted() {
    let id = ProductTierId::new(" ".to_string()).expect("single space must pass");
    assert_eq!(id.as_str(), " ");
    assert_eq!(id.into_inner(), " ".to_string());
}

#[test]
fn tier_07_unknown_future_tier_is_accepted() {
    let id = ProductTierId::new("ultra.premium.tier.99_X".to_string()).expect("unknown must pass");
    assert_eq!(id.as_str(), "ultra.premium.tier.99_X");
    assert_eq!(id.into_inner(), "ultra.premium.tier.99_X".to_string());
}

#[test]
fn tier_08_exact_input_is_preserved() {
    let id = ProductTierId::new(" Pro ".to_string()).expect("must pass");
    assert_eq!(id.as_str(), " Pro ");
    assert_eq!(id.into_inner(), " Pro ".to_string());
}

#[test]
fn tier_09_no_closed_vocabulary() {
    // Uppercase, mixed separators, and unfamiliar shapes all pass: there is
    // no recognized-tier registry.
    for candidate in ["FREE", "Pro", "PRO", "tier/v2:latest", "  x  ", "0"] {
        let id = ProductTierId::new(candidate.to_string())
            .unwrap_or_else(|_| panic!("{candidate:?} must pass"));
        assert_eq!(id.as_str(), candidate);
    }
}

// ---------------------------------------------------------------------------
// Capability id
// ---------------------------------------------------------------------------

fn assert_capability_accepted(value: &str) {
    let id = ProductCapabilityId::new(value.to_string())
        .unwrap_or_else(|_| panic!("{value:?} must pass"));
    assert_eq!(id.as_str(), value);
    assert_eq!(id.into_inner(), value.to_string());
}

fn assert_capability_rejected(value: &str) {
    let err =
        ProductCapabilityId::new(value.to_string()).expect_err(&format!("{value:?} must fail"));
    assert_eq!(err, ProductEntitlementValueError::CapabilityIdInvalid);
}

#[test]
fn capability_required_valid_matrix() {
    for value in [
        "graph.core",
        "orchestration.multi_runtime",
        "a.b",
        "a_1.b2",
        "a.b.c",
        "a1.b2.c_3",
        "a_.b_",
        "future.new_capability",
    ] {
        assert_capability_accepted(value);
    }
}

#[test]
fn capability_required_invalid_matrix() {
    for value in [
        "",
        "graph",
        "Graph.core",
        "1graph.core",
        "graph.",
        ".graph",
        "graph..core",
        "graph-core.x",
        "graph.Core",
        "a.1b",
        "a._b",
        "a.B",
        "a.b-",
        "a.b c",
        "a/b.c",
        "a::b.c",
        "é.a",
        "a.é",
    ] {
        assert_capability_rejected(value);
    }
}

#[test]
fn capability_needs_at_least_two_segments() {
    assert_capability_rejected("graph");
    assert_capability_rejected("");
    assert_capability_accepted("a.b");
    assert_capability_accepted("a.b.c.d");
}

#[test]
fn capability_ascii_exactness() {
    // Underscore and digits are fine after the first char; uppercase,
    // hyphens, spaces, and non-ASCII are not.
    assert_capability_accepted("abc.def_ghi012");
    assert_capability_rejected("Abc.def");
    assert_capability_rejected("abc.Def");
    assert_capability_rejected("abc.def-ghi");
    assert_capability_rejected("abc.def ghi");
    assert_capability_rejected("äbc.def");
}

#[test]
fn capability_is_extensible_without_code_change() {
    assert_capability_accepted("future.new_capability");
    assert_capability_accepted("someday.brand_new_thing_v2");
}

// ---------------------------------------------------------------------------
// Key id
// ---------------------------------------------------------------------------

#[test]
fn key_id_empty_is_rejected() {
    let err = ProductEntitlementKeyId::new(String::new()).expect_err("empty must fail");
    assert_eq!(err, ProductEntitlementValueError::KeyIdEmpty);
}

#[test]
fn key_id_opaque_non_empty_values_are_accepted() {
    for value in ["k", "future/key-format:v7", " "] {
        let id = ProductEntitlementKeyId::new(value.to_string())
            .unwrap_or_else(|_| panic!("{value:?} must pass"));
        assert_eq!(id.as_str(), value);
        assert_eq!(id.into_inner(), value.to_string());
    }
}

#[test]
fn key_id_preservation_is_exact() {
    let value = "  Key/ID:v7 with spaces  ".to_string();
    let id = ProductEntitlementKeyId::new(value.clone()).expect("must pass");
    assert_eq!(id.as_str(), value);
    assert_eq!(id.into_inner(), value);
}

// ---------------------------------------------------------------------------
// Signature
// ---------------------------------------------------------------------------

#[test]
fn signature_empty_is_rejected() {
    let err = ProductEntitlementSignature::new(String::new()).expect_err("empty must fail");
    assert_eq!(err, ProductEntitlementValueError::SignatureEmpty);
}

#[test]
fn signature_opaque_non_empty_values_are_accepted() {
    for value in ["x", "not-base64%%%", " ", "future.signature:opaque"] {
        let sig = ProductEntitlementSignature::new(value.to_string())
            .unwrap_or_else(|_| panic!("{value:?} must pass"));
        assert_eq!(sig.as_str(), value);
        assert_eq!(sig.into_inner(), value.to_string());
    }
}

#[test]
fn signature_preservation_is_exact() {
    let value = "  opaque:::with spaces  ".to_string();
    let sig = ProductEntitlementSignature::new(value.clone()).expect("must pass");
    assert_eq!(sig.as_str(), value);
    assert_eq!(sig.into_inner(), value);
}

// ---------------------------------------------------------------------------
// Error distinction and payload safety
// ---------------------------------------------------------------------------

#[test]
fn errors_distinguish_every_field_shape() {
    assert_eq!(
        ProductEntitlementSubjectId::new(String::new()).expect_err("must fail"),
        ProductEntitlementValueError::SubjectIdEmpty
    );
    assert_eq!(
        ProductEntitlementSubjectId::new("a".repeat(201)).expect_err("must fail"),
        ProductEntitlementValueError::SubjectIdTooLong {
            observed_chars: 201
        }
    );
    assert_eq!(
        ProductTierId::new(String::new()).expect_err("must fail"),
        ProductEntitlementValueError::TierIdEmpty
    );
    assert_eq!(
        ProductCapabilityId::new("graph".to_string()).expect_err("must fail"),
        ProductEntitlementValueError::CapabilityIdInvalid
    );
    assert_eq!(
        ProductEntitlementKeyId::new(String::new()).expect_err("must fail"),
        ProductEntitlementValueError::KeyIdEmpty
    );
    assert_eq!(
        ProductEntitlementSignature::new(String::new()).expect_err("must fail"),
        ProductEntitlementValueError::SignatureEmpty
    );
}

#[test]
fn error_display_is_static_and_implements_std_error() {
    fn assert_std_error<T: std::error::Error>() {}
    assert_std_error::<ProductEntitlementValueError>();

    let cases = [
        ProductEntitlementValueError::SubjectIdEmpty,
        ProductEntitlementValueError::SubjectIdTooLong {
            observed_chars: 201,
        },
        ProductEntitlementValueError::TierIdEmpty,
        ProductEntitlementValueError::CapabilityIdInvalid,
        ProductEntitlementValueError::KeyIdEmpty,
        ProductEntitlementValueError::SignatureEmpty,
    ];
    for err in cases {
        let text = format!("{err}");
        assert!(!text.is_empty());
    }
    assert_eq!(
        format!("{}", ProductEntitlementValueError::SubjectIdEmpty),
        "product entitlement subject id is empty"
    );
    assert_eq!(
        format!("{}", ProductEntitlementValueError::TierIdEmpty),
        "product tier id is empty"
    );
    assert_eq!(
        format!("{}", ProductEntitlementValueError::CapabilityIdInvalid),
        "product capability id has an invalid shape"
    );
    assert_eq!(
        format!("{}", ProductEntitlementValueError::KeyIdEmpty),
        "product entitlement key id is empty"
    );
    assert_eq!(
        format!("{}", ProductEntitlementValueError::SignatureEmpty),
        "product entitlement signature is empty"
    );
}

#[test]
fn error_for_too_long_subject_reports_count_not_content() {
    let value = "q".repeat(201);
    let err = ProductEntitlementSubjectId::new(value.clone()).expect_err("must fail");
    let display = format!("{err}");
    let debug = format!("{err:?}");
    assert!(display.contains("201"));
    assert!(!display.contains(&value));
    assert!(!debug.contains(&value));
}

#[test]
fn error_display_does_not_leak_rejected_text() {
    let secret = "not-base64%%%";
    // A valid opaque signature is stored untouched; failures carry only
    // static field/violation text.
    let sig = ProductEntitlementSignature::new(secret.to_string()).expect("must pass");
    assert_eq!(sig.as_str(), secret);
    let empty_display = format!("{}", ProductEntitlementValueError::SignatureEmpty);
    let empty_debug = format!("{:?}", ProductEntitlementValueError::SignatureEmpty);
    assert!(!empty_display.contains(secret));
    assert!(!empty_debug.contains(secret));

    let subject_secret = "sensitive-subject-value";
    let subject_display = format!("{}", ProductEntitlementValueError::SubjectIdEmpty);
    let subject_debug = format!("{:?}", ProductEntitlementValueError::SubjectIdEmpty);
    assert!(!subject_display.contains(subject_secret));
    assert!(!subject_debug.contains(subject_secret));
}

// ---------------------------------------------------------------------------
// Value semantics
// ---------------------------------------------------------------------------

#[test]
fn value_objects_compare_clone_and_consume() {
    let first = ProductTierId::new("pro".to_string()).expect("must pass");
    let second = first.clone();
    assert_eq!(first, second);
    assert_eq!(second.as_str(), "pro");
    assert_eq!(first.into_inner(), "pro".to_string());

    let cap_first = ProductCapabilityId::new("graph.core".to_string()).expect("must pass");
    let cap_second = cap_first.clone();
    assert_eq!(cap_first, cap_second);
    assert_ne!(
        cap_first,
        ProductCapabilityId::new("graph.other".to_string()).expect("must pass")
    );
}
