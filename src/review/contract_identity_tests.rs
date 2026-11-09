//! The extracted contracts crate owns the only implementation; Review's old
//! root and module paths name the very same Rust types (no duplicate DTOs).
use receipts_review_contracts as contracts;

#[test]
fn root_and_module_paths_are_the_canonical_contract_types() {
    let _: fn(contracts::ReviewCapsuleNonTemporalCore) -> crate::ReviewCapsuleNonTemporalCore =
        |v| v;
    let _: fn(
        contracts::ReviewCapsuleNonTemporalCore,
    ) -> crate::review_capsule_structured_core::ReviewCapsuleNonTemporalCore = |v| v;
    let _: fn(contracts::ReviewCapsuleCheck) -> crate::ReviewCapsuleCheck = |v| v;
    let _: fn(contracts::ReviewCapsuleCriterion) -> crate::ReviewCapsuleCriterion = |v| v;
    let _: fn(contracts::ReviewDateTimeV1) -> crate::date_time::ReviewDateTimeV1 = |v| v;
    let _: fn(contracts::ReviewDateTimeError) -> crate::ReviewDateTimeError = |v| v;
    let _: fn(
        contracts::ReviewRequestNonNegativeInteger,
    ) -> crate::review_request_structured_core::ReviewRequestNonNegativeInteger = |v| v;
    let _: fn(contracts::ReviewRequestConstructionError) -> crate::ReviewRequestConstructionError =
        |v| v;
}

#[test]
fn shared_integer_still_validates_once() {
    // PR141 domain: negative refused, legacy i64 accepted, unbounded preserved.
    assert_eq!(
        crate::ReviewRequestNonNegativeInteger::try_from(-1i64),
        Err(crate::ReviewRequestConstructionError::NegativeInteger)
    );
    let big = "1".repeat(64);
    let value = contracts::ReviewRequestNonNegativeInteger::from_decimal(&big).unwrap();
    assert_eq!(value.decimal_digits(), big);
}
