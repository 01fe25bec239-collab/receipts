//! Raw identities observed from existing Registry model/runtime associations.

use crate::registry::{ModelId, ProviderId, Registry, RuntimeId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryCandidateIdentity {
    provider_id: ProviderId,
    model_id: ModelId,
    runtime_id: RuntimeId,
}

impl RegistryCandidateIdentity {
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    pub fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    pub fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }
}

/// Returns one identity per existing association, without determining eligibility.
/// Preserves Registry's exact lexical provider, model, then runtime order.
pub fn enumerate_registry_candidates(registry: &Registry) -> Vec<RegistryCandidateIdentity> {
    registry
        .models()
        .flat_map(|model| model.runtime_associations())
        .map(|association| RegistryCandidateIdentity {
            provider_id: association.provider_id().clone(),
            model_id: association.model_id().clone(),
            runtime_id: association.runtime_id().clone(),
        })
        .collect()
}
