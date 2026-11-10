//! Shared Orchestration-owned physical value types.

mod date_time;
mod decision_publication;
mod feature_admission_decision;
mod feature_capability_set;
mod json_value;
mod remote_policy;
mod review_claims;
mod state_write_authority;

pub use date_time::{OrchestrationDateTimeError, OrchestrationDateTimeV1};
pub use decision_publication::{CoreEventV1, CorePublicationInputV1, publication_id_v1};
pub use feature_admission_decision::{
    FeatureAdmissionDecision, FeatureAdmissionDecisionError, FeatureAdmissionOutcome,
    FeatureAdmissionUpgradeInfo,
};
pub use feature_capability_set::{
    FeatureCapability, FeatureCapabilitySet, FeatureCapabilitySetError, FeatureCapabilityStatus,
};
pub use json_value::{
    OrchestrationJsonNumberError, OrchestrationJsonNumberV1, OrchestrationJsonObjectV1,
    OrchestrationJsonParseError, OrchestrationJsonValueV1,
};
pub use remote_policy::{ProjectRemotePolicy, WorkspaceCompositionLevel};
pub use review_claims::{
    CoreRecoveredReviewV1, CoreRecoveryCodecV1, CoreReviewClaimV1, CoreReviewUnitV1,
    CoreSealedReviewV1,
};
pub use state_write_authority::{
    CoreAbandonmentGrantV1, CoreAdmissionIdentity, CoreContentResolverV1, CorePolicyRefusal,
    CoreStateWriteError, CoreStateWriter,
};

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

#[cfg(test)]
mod state_write_authority_tests;

#[cfg(test)]
mod integration_fixtures;

#[cfg(test)]
mod recovery_forwarder_tests;

#[cfg(test)]
mod decision_publication_tests;

#[cfg(test)]
mod review_claim_tests;
