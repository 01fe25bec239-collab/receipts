//! In-process TaskCapsule physical contract. Construction validates only data
//! invariants; values authorize no dispatch, execution, or operational effects.

mod embedded;
mod error;
mod task_capsule;
mod task_capsule_types;
mod verification_plan;

pub use embedded::{CostPolicy, Criterion, Ref, ReviewPolicy};
pub use error::CapsuleError;
pub use task_capsule::TaskCapsule;
pub use task_capsule_types::*;
pub use verification_plan::VerificationPlanEntryV1;

use error::validate_identifier;

#[cfg(test)]
mod task_capsule_tests;
