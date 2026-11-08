//! The ReviewCapsule Runtime receives is the one canonical contracts type,
//! re-exported unchanged by Review and Runtime bindings (no duplicate DTOs).
use receipts_runtime_bindings::RuntimeCapsuleFamily;

#[test]
fn runtime_bindings_review_and_contracts_name_one_capsule_type() {
    let _: fn(receipts_review_integration::ReviewCapsuleNonTemporalCore) -> RuntimeCapsuleFamily =
        RuntimeCapsuleFamily::Review;
    let _: fn(
        receipts_review_contracts::ReviewCapsuleNonTemporalCore,
    ) -> receipts_runtime_bindings::ReviewCapsuleNonTemporalCore = |v| v;
    let _: fn(
        receipts_review_integration::ReviewCapsuleCriterion,
    ) -> receipts_review_contracts::ReviewCapsuleCriterion = |v| v;
}
