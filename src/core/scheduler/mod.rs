//! Owned scheduler namespace.
//!
//! This module currently establishes only the topology location for the four
//! closed dispatch-admission vocabularies. It contains no scheduler struct,
//! no scheduler loop, no admission composition, and no policy engine.

pub mod dispatch_admission_axis_references;
pub mod dispatch_admission_core;
pub mod dispatch_admission_vocabulary;

pub use dispatch_admission_axis_references::{
    DispatchAdmissionAxisReferenceError, DispatchAdmissionEntitlementAxisResult,
    DispatchAdmissionProviderAvailabilityAxisResult, DispatchAdmissionQualityFloorAxisResult,
    DispatchAdmissionSafetyAxisResult,
};
pub use dispatch_admission_core::{
    DispatchAdmissionDecisionCore, DispatchAdmissionDecisionCoreError,
};
pub use dispatch_admission_vocabulary::{
    DispatchAdmissionAxisResult, DispatchAdmissionDenialReason, DispatchAdmissionFailingAxis,
    DispatchAdmissionOutcome,
};

#[cfg(test)]
mod dispatch_admission_axis_references_tests;
#[cfg(test)]
mod dispatch_admission_core_tests;
#[cfg(test)]
mod dispatch_admission_vocabulary_tests;
