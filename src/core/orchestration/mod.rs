//! Shared Orchestration-owned physical value types.

mod date_time;
mod feature_admission_decision;
mod feature_capability_set;
mod json_value;
mod remote_policy;

pub use date_time::{OrchestrationDateTimeError, OrchestrationDateTimeV1};
pub use feature_admission_decision::{
    FeatureAdmissionDecision, FeatureAdmissionDecisionError, FeatureAdmissionOutcome,
    FeatureAdmissionUpgradeInfo,
};
pub use feature_capability_set::{
    FeatureCapability, FeatureCapabilitySet, FeatureCapabilitySetError, FeatureCapabilityStatus,
};
pub use json_value::{
    OrchestrationJsonNumberError, OrchestrationJsonNumberV1, OrchestrationJsonObjectV1,
    OrchestrationJsonValueV1,
};
pub use remote_policy::{ProjectRemotePolicy, WorkspaceCompositionLevel};

#[cfg(test)]
mod date_time_tests;

#[cfg(test)]
mod feature_admission_decision_tests;

#[cfg(test)]
mod feature_capability_set_tests;

#[cfg(test)]
mod json_value_tests;

#[cfg(test)]
mod remote_policy_tests;
