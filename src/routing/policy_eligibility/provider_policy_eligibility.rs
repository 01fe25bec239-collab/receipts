use super::ModelRoutingDateTimeV1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TechnicalStatus {
    Connected,
    AuthRequired,
    Expired,
    NotConfigured,
    Unknown,
}

impl TechnicalStatus {
    pub const ALL: [Self; 5] = [
        Self::Connected,
        Self::AuthRequired,
        Self::Expired,
        Self::NotConfigured,
        Self::Unknown,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Connected => "CONNECTED",
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::Expired => "EXPIRED",
            Self::NotConfigured => "NOT_CONFIGURED",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyStatus {
    VerifiedAllowed,
    VerifiedDisallowed,
    NeedsReview,
    Unknown,
}

impl PolicyStatus {
    pub const ALL: [Self; 4] = [
        Self::VerifiedAllowed,
        Self::VerifiedDisallowed,
        Self::NeedsReview,
        Self::Unknown,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VerifiedAllowed => "VERIFIED_ALLOWED",
            Self::VerifiedDisallowed => "VERIFIED_DISALLOWED",
            Self::NeedsReview => "NEEDS_REVIEW",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyEvidenceLabel {
    VerifiedCurrentSelfFetched,
    ReviewerSuppliedCurrentPrimarySource,
    VerifiedHistorical,
    IndependentVerified,
    UserDeclared,
    DesignDecision,
    Assumption,
    Unverified,
    PolicyNeedsReview,
}

impl PolicyEvidenceLabel {
    pub const ALL: [Self; 9] = [
        Self::VerifiedCurrentSelfFetched,
        Self::ReviewerSuppliedCurrentPrimarySource,
        Self::VerifiedHistorical,
        Self::IndependentVerified,
        Self::UserDeclared,
        Self::DesignDecision,
        Self::Assumption,
        Self::Unverified,
        Self::PolicyNeedsReview,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VerifiedCurrentSelfFetched => "VERIFIED_CURRENT_SELF_FETCHED",
            Self::ReviewerSuppliedCurrentPrimarySource => {
                "REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE"
            }
            Self::VerifiedHistorical => "VERIFIED_HISTORICAL",
            Self::IndependentVerified => "INDEPENDENT_VERIFIED",
            Self::UserDeclared => "USER_DECLARED",
            Self::DesignDecision => "DESIGN_DECISION",
            Self::Assumption => "ASSUMPTION",
            Self::Unverified => "UNVERIFIED",
            Self::PolicyNeedsReview => "POLICY_NEEDS_REVIEW",
        }
    }
}

impl PolicyStatus {
    /// Passes only the default policy-status gate. Technical availability,
    /// execution context, capability, quality, safety, and all other gates
    /// must be assessed independently by their owners.
    pub const fn passes_policy_gate_by_default(self) -> bool {
        matches!(self, Self::VerifiedAllowed)
    }
}

/// All frozen schema fields, stored independently without cross-field policy.
/// Optional nullable fields distinguish absence, explicit null, and a value.
/// `verified_at` requires a validated value at construction:
///
/// ```compile_fail
/// use receipts_model_routing::policy_eligibility::{
///     ProviderPolicyEligibility, TechnicalStatus, PolicyStatus, PolicyEvidenceLabel,
/// };
/// let missing_timestamp = ProviderPolicyEligibility::try_new(
///     "p".into(), "r".into(), "c".into(), TechnicalStatus::Unknown,
///     PolicyStatus::Unknown, None, None, None, PolicyEvidenceLabel::Unverified,
///     None, None, None, None,
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPolicyEligibility {
    provider_id: String,
    runtime_id: String,
    credential_mode: String,
    technical_status: TechnicalStatus,
    policy_status: PolicyStatus,
    allowed_execution_contexts: Option<Vec<String>>,
    verified_at: ModelRoutingDateTimeV1,
    evidence_source: Option<String>,
    evidence_label: PolicyEvidenceLabel,
    terms_version_or_digest: Option<Option<String>>,
    reason: Option<String>,
    reverification_deadline: Option<Option<ModelRoutingDateTimeV1>>,
    source_claim_id: Option<Option<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderPolicyEligibilityError {
    EmptyProviderId,
    EmptyRuntimeId,
    EmptyCredentialMode,
    EmptyExecutionContext { index: usize },
}

impl ProviderPolicyEligibility {
    /// Validates only physical schema constraints; preserves all accepted input.
    #[allow(clippy::too_many_arguments)] // Explicit schema fields; no hidden defaults.
    pub fn try_new(
        provider_id: String,
        runtime_id: String,
        credential_mode: String,
        technical_status: TechnicalStatus,
        policy_status: PolicyStatus,
        allowed_execution_contexts: Option<Vec<String>>,
        verified_at: ModelRoutingDateTimeV1,
        evidence_source: Option<String>,
        evidence_label: PolicyEvidenceLabel,
        terms_version_or_digest: Option<Option<String>>,
        reason: Option<String>,
        reverification_deadline: Option<Option<ModelRoutingDateTimeV1>>,
        source_claim_id: Option<Option<String>>,
    ) -> Result<Self, ProviderPolicyEligibilityError> {
        use ProviderPolicyEligibilityError::*;

        if provider_id.is_empty() {
            return Err(EmptyProviderId);
        }
        if runtime_id.is_empty() {
            return Err(EmptyRuntimeId);
        }
        if credential_mode.is_empty() {
            return Err(EmptyCredentialMode);
        }
        if let Some(contexts) = &allowed_execution_contexts
            && let Some(index) = contexts.iter().position(String::is_empty)
        {
            return Err(EmptyExecutionContext { index });
        }
        Ok(Self {
            provider_id,
            runtime_id,
            credential_mode,
            technical_status,
            policy_status,
            allowed_execution_contexts,
            verified_at,
            evidence_source,
            evidence_label,
            terms_version_or_digest,
            reason,
            reverification_deadline,
            source_claim_id,
        })
    }

    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }

    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    pub fn credential_mode(&self) -> &str {
        &self.credential_mode
    }

    pub fn technical_status(&self) -> TechnicalStatus {
        self.technical_status
    }

    pub fn policy_status(&self) -> PolicyStatus {
        self.policy_status
    }

    pub fn allowed_execution_contexts(&self) -> Option<&[String]> {
        self.allowed_execution_contexts.as_deref()
    }

    pub fn verified_at(&self) -> &ModelRoutingDateTimeV1 {
        &self.verified_at
    }

    pub fn evidence_source(&self) -> Option<&str> {
        self.evidence_source.as_deref()
    }

    pub fn evidence_label(&self) -> PolicyEvidenceLabel {
        self.evidence_label
    }

    pub fn terms_version_or_digest(&self) -> Option<Option<&str>> {
        self.terms_version_or_digest
            .as_ref()
            .map(|value| value.as_deref())
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    pub fn reverification_deadline(&self) -> Option<Option<&ModelRoutingDateTimeV1>> {
        self.reverification_deadline
            .as_ref()
            .map(|value| value.as_ref())
    }

    pub fn source_claim_id(&self) -> Option<Option<&str>> {
        self.source_claim_id.as_ref().map(|value| value.as_deref())
    }
}
