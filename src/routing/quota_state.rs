use crate::policy_eligibility::ModelRoutingDateTimeV1;
use crate::registry::ProviderId;

/// Explicit caller-supplied scope; no implicit default.
///
/// ```compile_fail
/// use receipts_model_routing::QuotaScope;
/// let scope = QuotaScope::default();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaScope {
    Provider,
    Model,
    Session,
    Unknown,
}

impl QuotaScope {
    pub const ALL: [Self; 4] = [Self::Provider, Self::Model, Self::Session, Self::Unknown];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Provider => "PROVIDER",
            Self::Model => "MODEL",
            Self::Session => "SESSION",
            Self::Unknown => "UNKNOWN",
        }
    }
}

/// Required-core in-process storage only, not the complete QuotaState wire object.
/// Stores all three caller-supplied values unchanged.
///
/// Each constructor argument is required.
///
/// ```compile_fail
/// # use receipts_model_routing::{QuotaScope, QuotaStateRequiredCore};
/// # use receipts_model_routing::policy_eligibility::ModelRoutingDateTimeV1;
/// # fn forbidden(scope: QuotaScope, observed_at: ModelRoutingDateTimeV1) {
/// QuotaStateRequiredCore::new(scope, observed_at);
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::QuotaStateRequiredCore;
/// # use receipts_model_routing::registry::ProviderId;
/// # use receipts_model_routing::policy_eligibility::ModelRoutingDateTimeV1;
/// # fn forbidden(provider_id: ProviderId, observed_at: ModelRoutingDateTimeV1) {
/// QuotaStateRequiredCore::new(provider_id, observed_at);
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::{QuotaScope, QuotaStateRequiredCore};
/// # use receipts_model_routing::registry::ProviderId;
/// # fn forbidden(provider_id: ProviderId, scope: QuotaScope) {
/// QuotaStateRequiredCore::new(provider_id, scope);
/// # }
/// ```
/// ```compile_fail
/// use receipts_model_routing::QuotaStateRequiredCore;
/// let core = QuotaStateRequiredCore::default();
/// ```
/// Fields are private and borrowed values are shared only.
///
/// ```compile_fail
/// # use receipts_model_routing::{QuotaScope, QuotaStateRequiredCore};
/// # use receipts_model_routing::registry::ProviderId;
/// # use receipts_model_routing::policy_eligibility::ModelRoutingDateTimeV1;
/// # fn forbidden(provider_id: ProviderId, quota_scope: QuotaScope, observed_at: ModelRoutingDateTimeV1) {
/// let _ = QuotaStateRequiredCore { provider_id, quota_scope, observed_at };
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::QuotaStateRequiredCore;
/// # use receipts_model_routing::registry::ProviderId;
/// # fn forbidden(core: &mut QuotaStateRequiredCore) {
/// let _: &mut ProviderId = core.provider_id();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::QuotaStateRequiredCore;
/// # use receipts_model_routing::policy_eligibility::ModelRoutingDateTimeV1;
/// # fn forbidden(core: &mut QuotaStateRequiredCore) {
/// let _: &mut ModelRoutingDateTimeV1 = core.observed_at();
/// # }
/// ```
/// Deferred fields have no accessor in this bounded core.
///
/// ```compile_fail
/// # use receipts_model_routing::QuotaStateRequiredCore;
/// # fn forbidden(core: &QuotaStateRequiredCore) {
/// core.model_id();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::QuotaStateRequiredCore;
/// # fn forbidden(core: &QuotaStateRequiredCore) {
/// core.limit();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::QuotaStateRequiredCore;
/// # fn forbidden(core: &QuotaStateRequiredCore) {
/// core.remaining();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::QuotaStateRequiredCore;
/// # fn forbidden(core: &QuotaStateRequiredCore) {
/// core.resets_at();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::QuotaStateRequiredCore;
/// # fn forbidden(core: &QuotaStateRequiredCore) {
/// core.retry_after_seconds();
/// # }
/// ```
/// ```compile_fail
/// # use receipts_model_routing::QuotaStateRequiredCore;
/// # fn forbidden(core: &QuotaStateRequiredCore) {
/// core.visibility();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotaStateRequiredCore {
    provider_id: ProviderId,
    quota_scope: QuotaScope,
    observed_at: ModelRoutingDateTimeV1,
}

impl QuotaStateRequiredCore {
    pub fn new(
        provider_id: ProviderId,
        quota_scope: QuotaScope,
        observed_at: ModelRoutingDateTimeV1,
    ) -> Self {
        Self {
            provider_id,
            quota_scope,
            observed_at,
        }
    }

    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub fn quota_scope(&self) -> QuotaScope {
        self.quota_scope
    }

    pub fn observed_at(&self) -> &ModelRoutingDateTimeV1 {
        &self.observed_at
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaVisibility {
    Headers,
    LocalApprox,
    None,
    Unknown,
}

impl QuotaVisibility {
    pub const ALL: [Self; 4] = [Self::Headers, Self::LocalApprox, Self::None, Self::Unknown];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Headers => "HEADERS",
            Self::LocalApprox => "LOCAL_APPROX",
            Self::None => "NONE",
            Self::Unknown => "UNKNOWN",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuotaStateError {
    EmptyModelId,
    NonFiniteLimit,
    NonFiniteRemaining,
}

/// The complete in-process QuotaState: the required core plus every optional
/// schema field, each distinguishing absent (`None`) from explicit null
/// (`Some(None)`) where the schema allows null. No number is fabricated:
/// UNKNOWN scope and absent/null limits stay exactly as supplied.
#[derive(Debug, Clone, PartialEq)]
pub struct QuotaState {
    core: QuotaStateRequiredCore,
    model_id: Option<String>,
    limit: Option<Option<f64>>,
    remaining: Option<Option<f64>>,
    resets_at: Option<Option<ModelRoutingDateTimeV1>>,
    retry_after_seconds: Option<Option<u64>>,
    visibility: Option<QuotaVisibility>,
}

impl QuotaState {
    pub fn try_new(
        core: QuotaStateRequiredCore,
        model_id: Option<String>,
        limit: Option<Option<f64>>,
        remaining: Option<Option<f64>>,
        resets_at: Option<Option<ModelRoutingDateTimeV1>>,
        retry_after_seconds: Option<Option<u64>>,
        visibility: Option<QuotaVisibility>,
    ) -> Result<Self, QuotaStateError> {
        if model_id.as_deref() == Some("") {
            return Err(QuotaStateError::EmptyModelId);
        }
        if limit.flatten().is_some_and(|v| !v.is_finite()) {
            return Err(QuotaStateError::NonFiniteLimit);
        }
        if remaining.flatten().is_some_and(|v| !v.is_finite()) {
            return Err(QuotaStateError::NonFiniteRemaining);
        }
        Ok(Self {
            core,
            model_id,
            limit,
            remaining,
            resets_at,
            retry_after_seconds,
            visibility,
        })
    }

    pub fn core(&self) -> &QuotaStateRequiredCore {
        &self.core
    }
    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }
    pub fn limit(&self) -> Option<Option<f64>> {
        self.limit
    }
    pub fn remaining(&self) -> Option<Option<f64>> {
        self.remaining
    }
    pub fn resets_at(&self) -> Option<Option<&ModelRoutingDateTimeV1>> {
        self.resets_at.as_ref().map(Option::as_ref)
    }
    pub fn retry_after_seconds(&self) -> Option<Option<u64>> {
        self.retry_after_seconds
    }
    pub fn visibility(&self) -> Option<QuotaVisibility> {
        self.visibility
    }

    /// The scope routing may rely on. A scope other than UNKNOWN is accepted
    /// only when the provider itself reported it (`HEADERS` visibility);
    /// local approximations, `NONE`/`UNKNOWN` visibility or an absent field
    /// keep UNKNOWN — scope is never assumed (RATE_LIMIT_AND_AVAILABILITY §53).
    pub fn verified_scope(&self) -> QuotaScope {
        match self.visibility {
            Some(QuotaVisibility::Headers) => self.core.quota_scope(),
            _ => QuotaScope::Unknown,
        }
    }
}
