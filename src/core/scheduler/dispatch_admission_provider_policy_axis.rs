//! Embedded policy evidence only; no policy evaluation or routing behavior.

use super::DispatchAdmissionAxisResult;

/// Exact policy-status vocabulary embedded in `DispatchAdmissionDecision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DispatchAdmissionProviderPolicyStatus {
    VerifiedAllowed,
    VerifiedDisallowed,
    NeedsReview,
    Unknown,
}

impl DispatchAdmissionProviderPolicyStatus {
    pub const ALL: [Self; 4] = [
        Self::VerifiedAllowed,
        Self::VerifiedDisallowed,
        Self::NeedsReview,
        Self::Unknown,
    ];

    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::VerifiedAllowed => "VERIFIED_ALLOWED",
            Self::VerifiedDisallowed => "VERIFIED_DISALLOWED",
            Self::NeedsReview => "NEEDS_REVIEW",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchAdmissionProviderPolicyAxisError {
    ProviderPolicyEligibilityIdLengthOutOfRange { character_count: usize },
}

/// Stores supplied evidence, without deriving a result from policy status.
/// Only the optional reference has a frozen length constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchAdmissionProviderPolicyAxisResult {
    result: DispatchAdmissionAxisResult,
    provider_policy_eligibility_id: Option<String>,
    policy_status: Option<DispatchAdmissionProviderPolicyStatus>,
    execution_context: Option<String>,
}

impl DispatchAdmissionProviderPolicyAxisResult {
    pub fn try_new(
        result: DispatchAdmissionAxisResult,
        provider_policy_eligibility_id: Option<String>,
        policy_status: Option<DispatchAdmissionProviderPolicyStatus>,
        execution_context: Option<String>,
    ) -> Result<Self, DispatchAdmissionProviderPolicyAxisError> {
        if let Some(reference) = &provider_policy_eligibility_id {
            let character_count = reference.chars().count();
            if !(1..=200).contains(&character_count) {
                return Err(DispatchAdmissionProviderPolicyAxisError::ProviderPolicyEligibilityIdLengthOutOfRange {
                    character_count,
                });
            }
        }
        Ok(Self {
            result,
            provider_policy_eligibility_id,
            policy_status,
            execution_context,
        })
    }

    pub fn result(&self) -> DispatchAdmissionAxisResult {
        self.result
    }

    pub fn provider_policy_eligibility_id(&self) -> Option<&str> {
        self.provider_policy_eligibility_id.as_deref()
    }

    pub fn policy_status(&self) -> Option<DispatchAdmissionProviderPolicyStatus> {
        self.policy_status
    }

    pub fn execution_context(&self) -> Option<&str> {
        self.execution_context.as_deref()
    }
}
