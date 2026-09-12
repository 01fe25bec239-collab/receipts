use crate::RuntimeCapsuleFamily;
use receipts_orchestration::capsules::{RepairCapsule, TaskCapsule};
use receipts_review_integration::ReviewCapsuleNonTemporalCore;

#[test]
fn variants_accept_canonical_owner_types_directly() {
    let _: fn(TaskCapsule) -> RuntimeCapsuleFamily = RuntimeCapsuleFamily::Task;
    let _: fn(RepairCapsule) -> RuntimeCapsuleFamily = RuntimeCapsuleFamily::Repair;
    let _: fn(ReviewCapsuleNonTemporalCore) -> RuntimeCapsuleFamily = RuntimeCapsuleFamily::Review;
}

#[test]
fn exactly_three_branches_expose_canonical_owner_types() {
    // Rust checks exhaustiveness and payload identity without artificial capsules.
    let _: fn(RuntimeCapsuleFamily) = |family| match family {
        RuntimeCapsuleFamily::Task(value) => {
            let _: TaskCapsule = value;
        }
        RuntimeCapsuleFamily::Repair(value) => {
            let _: RepairCapsule = value;
        }
        RuntimeCapsuleFamily::Review(value) => {
            let _: ReviewCapsuleNonTemporalCore = value;
        }
    };
}
