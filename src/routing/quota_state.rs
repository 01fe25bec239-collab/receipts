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
