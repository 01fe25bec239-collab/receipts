//! Shared Orchestration-owned physical value types.

mod date_time;
mod json_value;
mod remote_policy;

pub use date_time::{OrchestrationDateTimeError, OrchestrationDateTimeV1};
pub use json_value::{
    OrchestrationJsonNumberError, OrchestrationJsonNumberV1, OrchestrationJsonObjectV1,
    OrchestrationJsonValueV1,
};
pub use remote_policy::{ProjectRemotePolicy, WorkspaceCompositionLevel};

#[cfg(test)]
mod date_time_tests;

#[cfg(test)]
mod json_value_tests;

#[cfg(test)]
mod remote_policy_tests;
