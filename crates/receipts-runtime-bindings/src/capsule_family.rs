/// Physical composition of canonical owner types, without translation or validation.
pub enum RuntimeCapsuleFamily {
    Task(receipts_orchestration::capsules::TaskCapsule),
    Repair(receipts_orchestration::capsules::RepairCapsule),
    /// Current canonical in-process non-temporal Review representation only.
    /// This does not close `started_at`, `finished_at`, `test_results`, full
    /// temporal semantics, or the complete wire/serialization representation.
    Review(receipts_review_integration::ReviewCapsuleNonTemporalCore),
}
