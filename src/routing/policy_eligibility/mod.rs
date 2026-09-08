//! Complete in-process ProviderPolicyEligibility physical record; no wire codec.
//! Field authority: ProviderPolicyEligibility.schema.json at
//! f49d621ee510705939394f7df4996223a73fdcb7. Timestamp authority:
//! BUILD-A1-ADR-MODEL-ROUTING-DATETIME-PHYSICAL-V1-001.
//! Q-V13-04 remains blocking before provider path enablement; this module
//! stores evidence and evaluates only the provider-policy gate.

mod date_time;
mod evaluator;
mod provider_policy_eligibility;

pub use date_time::{ModelRoutingDateTimeV1, ModelRoutingDateTimeV1Error};
pub use evaluator::PolicyEligibilityEvaluator;
pub use provider_policy_eligibility::{
    PolicyEvidenceLabel, PolicyStatus, ProviderPolicyEligibility, ProviderPolicyEligibilityError,
    TechnicalStatus,
};

#[cfg(test)]
mod evaluator_tests;
#[cfg(test)]
mod tests;
