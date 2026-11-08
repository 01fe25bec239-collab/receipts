//! In-process task and repair capsule physical contract, plus the builders
//! that derive capsules from the graph and trusted context. Values
//! authorize no dispatch, execution, or operational effects.

mod builder;
mod credential_scan;
mod embedded;
mod error;
mod task_capsule;
mod task_capsule_types;
mod verification_plan;

pub use builder::{
    CapsuleBuildError, CapsuleContextV1, RepairCapsuleRequestV1, TaskCapsuleRequestV1,
    build_repair_capsule_v1, build_task_capsule_v1,
};
pub use embedded::{CostPolicy, Criterion, Ref, ReviewPolicy};
pub use error::CapsuleError;
pub use task_capsule::TaskCapsule;
pub use task_capsule_types::*;
pub use verification_plan::VerificationPlanEntryV1;

use error::validate_identifier;

#[cfg(test)]
mod builder_tests;
#[cfg(test)]
mod task_capsule_tests;

mod repair_capsule;
mod repair_capsule_snapshots;
mod repair_capsule_types;
pub use repair_capsule::RepairCapsule;
pub use repair_capsule_snapshots::{RepairCapsuleFailedCheckV1, RepairCapsuleFindingV1};
pub use repair_capsule_types::*;

#[cfg(test)]
mod repair_capsule_tests;
