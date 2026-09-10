//! In-process registry foundation only. State persistence is not implemented.
//! Only the intelligence owner holds the mutable service; workers receive
//! &Registry. No mutable collections or record references are exposed.
mod capability;
pub use capability::*;

use crate::EvidenceConfidence;
use crate::intelligence::{
    LifecycleError, LifecycleEvidence, LifecycleState, LifecycleTransition,
    RoutablePromotionEvidence, TransitionRecord, VerificationPath, validate_transition_shape,
};
use std::collections::{BTreeMap, btree_map::Entry};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRecord {
    provider_id: ProviderId,
    discovery: Observation,
}
impl ProviderRecord {
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }
    pub fn discovery(&self) -> &Observation {
        &self.discovery
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeRecord {
    provider_id: ProviderId,
    runtime_id: RuntimeId,
    discovery: Observation,
}
impl RuntimeRecord {
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }
    pub fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }
    pub fn discovery(&self) -> &Observation {
        &self.discovery
    }
}

/// Association existence does not assert compatibility. Pair-subject boolean
/// evidence confirms each exact required capability on this runtime path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeAssociation {
    provider_id: ProviderId,
    model_id: ModelId,
    runtime_id: RuntimeId,
    observation: Observation,
}
impl RuntimeAssociation {
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }
    pub fn model_id(&self) -> &ModelId {
        &self.model_id
    }
    pub fn runtime_id(&self) -> &RuntimeId {
        &self.runtime_id
    }
    pub fn observation(&self) -> &Observation {
        &self.observation
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRecord {
    provider_id: ProviderId,
    model_id: ModelId,
    discovery: Observation,
    lifecycle_state: LifecycleState,
    verified_path: Option<VerificationPath>,
    transitions: Vec<TransitionRecord>,
    runtimes: BTreeMap<RuntimeId, RuntimeAssociation>,
}
impl ModelRecord {
    pub fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }
    pub fn model_id(&self) -> &ModelId {
        &self.model_id
    }
    pub fn discovery(&self) -> &Observation {
        &self.discovery
    }
    pub fn lifecycle_state(&self) -> LifecycleState {
        self.lifecycle_state
    }
    pub fn verified_path(&self) -> Option<&VerificationPath> {
        self.verified_path.as_ref()
    }
    /// Successful transitions in application order, never timestamp order.
    pub fn transitions(&self) -> &[TransitionRecord] {
        &self.transitions
    }
    pub fn runtime_associations(&self) -> impl Iterator<Item = &RuntimeAssociation> {
        self.runtimes.values()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    Unknown,
    Confirmed,
    Unsupported,
}

/// Read-only exact lookups. Enumeration is lexical identity order, not quality.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Registry {
    providers: BTreeMap<ProviderId, ProviderRecord>,
    models: BTreeMap<ProviderId, BTreeMap<ModelId, ModelRecord>>,
    runtimes: BTreeMap<ProviderId, BTreeMap<RuntimeId, RuntimeRecord>>,
    evidence: BTreeMap<CapabilitySubject, BTreeMap<CapabilityId, Vec<CapabilityEvidence>>>,
}
impl Registry {
    pub fn provider(&self, provider: &str) -> Option<&ProviderRecord> {
        self.providers.get(provider)
    }
    pub fn model(&self, provider: &str, model: &str) -> Option<&ModelRecord> {
        self.models.get(provider)?.get(model)
    }
    pub fn runtime(&self, provider: &str, runtime: &str) -> Option<&RuntimeRecord> {
        self.runtimes.get(provider)?.get(runtime)
    }
    pub fn association(
        &self,
        provider: &str,
        model: &str,
        runtime: &str,
    ) -> Option<&RuntimeAssociation> {
        self.model(provider, model)?.runtimes.get(runtime)
    }
    pub fn providers(&self) -> impl Iterator<Item = &ProviderRecord> {
        self.providers.values()
    }
    pub fn models(&self) -> impl Iterator<Item = &ModelRecord> {
        self.models.values().flat_map(|models| models.values())
    }
    pub fn runtimes(&self) -> impl Iterator<Item = &RuntimeRecord> {
        self.runtimes
            .values()
            .flat_map(|runtimes| runtimes.values())
    }
    /// Evidence observations preserve explicit append order. No timestamp sort,
    /// implicit replacement, or confidence aggregation occurs.
    pub fn capability_evidence(
        &self,
        subject: &CapabilitySubject,
        capability: &str,
    ) -> Option<&[CapabilityEvidence]> {
        Some(self.evidence.get(subject)?.get(capability)?.as_slice())
    }
    fn subject_exists(&self, subject: &CapabilitySubject) -> bool {
        match subject {
            CapabilitySubject::Model {
                provider_id,
                model_id,
            } => self
                .model(provider_id.as_str(), model_id.as_str())
                .is_some(),
            CapabilitySubject::Runtime {
                provider_id,
                runtime_id,
            } => self
                .runtime(provider_id.as_str(), runtime_id.as_str())
                .is_some(),
            CapabilitySubject::ModelRuntimePair {
                provider_id,
                model_id,
                runtime_id,
            } => self
                .association(provider_id.as_str(), model_id.as_str(), runtime_id.as_str())
                .is_some(),
        }
    }
    /// All assertions in the requested confidence class must be sourced and
    /// explicitly true. Conflicting or UNKNOWN assertions fail closed; evidence
    /// reconciliation and supersession require a later authorized policy.
    fn supports(
        &self,
        subject: &CapabilitySubject,
        capability: &CapabilityId,
        confidence: EvidenceConfidence,
    ) -> bool {
        let Some(evidence) = self.capability_evidence(subject, capability.as_str()) else {
            return false;
        };
        let mut matching = evidence
            .iter()
            .filter(|e| e.observation.confidence == confidence)
            .peekable();
        matching.peek().is_some()
            && matching.all(|e| {
                e.value == Some(CapabilityValue::Boolean(true))
                    && e.observation.source_ref.is_some()
            })
    }
    pub fn compatibility(
        &self,
        provider: &ProviderId,
        model: &ModelId,
        runtime: &RuntimeId,
        capability: &CapabilityId,
    ) -> Compatibility {
        let subject = CapabilitySubject::ModelRuntimePair {
            provider_id: provider.clone(),
            model_id: model.clone(),
            runtime_id: runtime.clone(),
        };
        if self.supports(&subject, capability, EvidenceConfidence::OfficialVerified) {
            return Compatibility::Confirmed;
        }
        if self
            .capability_evidence(&subject, capability.as_str())
            .is_some_and(|evidence| {
                evidence.iter().any(|e| {
                    e.observation.confidence == EvidenceConfidence::OfficialVerified
                        && e.observation.source_ref.is_some()
                        && e.value == Some(CapabilityValue::Boolean(false))
                })
            })
        {
            return Compatibility::Unsupported;
        }
        Compatibility::Unknown
    }
    fn verify_path(
        &self,
        model: &ModelRecord,
        path: &VerificationPath,
    ) -> Result<(), LifecycleError> {
        if path.required_capabilities.is_empty() {
            return Err(LifecycleError::EmptyRequiredCapabilities);
        }
        let subject = CapabilitySubject::Model {
            provider_id: model.provider_id.clone(),
            model_id: model.model_id.clone(),
        };
        for capability in &path.required_capabilities {
            if !self.supports(&subject, capability, EvidenceConfidence::OfficialVerified) {
                return Err(LifecycleError::MissingAuthoritativeCapability);
            }
            if self.compatibility(
                &model.provider_id,
                &model.model_id,
                &path.runtime_id,
                capability,
            ) != Compatibility::Confirmed
            {
                return Err(LifecycleError::MissingRuntimeCompatibility);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryError {
    DuplicateProvider,
    DuplicateModel,
    DuplicateRuntime,
    DuplicateAssociation,
    DuplicateEvidence,
    ProviderNotFound,
    ModelNotFound,
    RuntimeNotFound,
    SubjectNotFound,
    Lifecycle(LifecycleError),
}
impl From<LifecycleError> for RegistryError {
    fn from(value: LifecycleError) -> Self {
        Self::Lifecycle(value)
    }
}

/// Validated mutation boundary for the Model Intelligence owner. The host must
/// retain this owner and give workers only registry(). No persistence or I/O.
#[derive(Debug, Default)]
pub struct ModelIntelligenceService {
    registry: Registry,
}
impl ModelIntelligenceService {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
    pub fn insert_provider(
        &mut self,
        provider_id: ProviderId,
        discovery: Observation,
    ) -> Result<(), RegistryError> {
        match self.registry.providers.entry(provider_id.clone()) {
            Entry::Occupied(_) => Err(RegistryError::DuplicateProvider),
            Entry::Vacant(entry) => {
                entry.insert(ProviderRecord {
                    provider_id,
                    discovery,
                });
                Ok(())
            }
        }
    }
    pub fn discover_model(
        &mut self,
        provider_id: ProviderId,
        model_id: ModelId,
        discovery: Observation,
    ) -> Result<(), RegistryError> {
        if self.registry.provider(provider_id.as_str()).is_none() {
            return Err(RegistryError::ProviderNotFound);
        }
        match self
            .registry
            .models
            .entry(provider_id.clone())
            .or_default()
            .entry(model_id.clone())
        {
            Entry::Occupied(_) => Err(RegistryError::DuplicateModel),
            Entry::Vacant(entry) => {
                entry.insert(ModelRecord {
                    provider_id,
                    model_id,
                    discovery,
                    lifecycle_state: LifecycleState::Discovered,
                    verified_path: None,
                    transitions: Vec::new(),
                    runtimes: BTreeMap::new(),
                });
                Ok(())
            }
        }
    }
    pub fn insert_runtime(
        &mut self,
        provider_id: ProviderId,
        runtime_id: RuntimeId,
        discovery: Observation,
    ) -> Result<(), RegistryError> {
        if self.registry.provider(provider_id.as_str()).is_none() {
            return Err(RegistryError::ProviderNotFound);
        }
        match self
            .registry
            .runtimes
            .entry(provider_id.clone())
            .or_default()
            .entry(runtime_id.clone())
        {
            Entry::Occupied(_) => Err(RegistryError::DuplicateRuntime),
            Entry::Vacant(entry) => {
                entry.insert(RuntimeRecord {
                    provider_id,
                    runtime_id,
                    discovery,
                });
                Ok(())
            }
        }
    }
    pub fn associate_runtime(
        &mut self,
        provider_id: ProviderId,
        model_id: ModelId,
        runtime_id: RuntimeId,
        observation: Observation,
    ) -> Result<(), RegistryError> {
        if self
            .registry
            .model(provider_id.as_str(), model_id.as_str())
            .is_none()
        {
            return Err(RegistryError::ModelNotFound);
        }
        if self
            .registry
            .runtime(provider_id.as_str(), runtime_id.as_str())
            .is_none()
        {
            return Err(RegistryError::RuntimeNotFound);
        }
        let model = self
            .registry
            .models
            .get_mut(&provider_id)
            .unwrap()
            .get_mut(&model_id)
            .unwrap();
        match model.runtimes.entry(runtime_id.clone()) {
            Entry::Occupied(_) => Err(RegistryError::DuplicateAssociation),
            Entry::Vacant(entry) => {
                entry.insert(RuntimeAssociation {
                    provider_id,
                    model_id,
                    runtime_id,
                    observation,
                });
                Ok(())
            }
        }
    }
    /// Explicit append-only observation insertion; identical observations are
    /// rejected. Distinct provenance/confidence/value observations coexist.
    pub fn record_capability(&mut self, evidence: CapabilityEvidence) -> Result<(), RegistryError> {
        if !self.registry.subject_exists(&evidence.subject) {
            return Err(RegistryError::SubjectNotFound);
        }
        let observations = self
            .registry
            .evidence
            .entry(evidence.subject.clone())
            .or_default()
            .entry(evidence.capability.clone())
            .or_default();
        if observations.contains(&evidence) {
            return Err(RegistryError::DuplicateEvidence);
        }
        observations.push(evidence);
        Ok(())
    }
    pub fn try_transition(
        &mut self,
        provider: &str,
        model_id: &str,
        transition: LifecycleTransition,
    ) -> Result<(), RegistryError> {
        let model = self
            .registry
            .model(provider, model_id)
            .ok_or(RegistryError::ModelNotFound)?;
        validate_transition_shape(model.lifecycle_state, &transition)?;
        use LifecycleEvidence::*;
        match &transition.evidence {
            CapabilityVerification(path) => self.registry.verify_path(model, path)?,
            BoundedCalibrationAdmission { authorized } => {
                if !authorized {
                    return Err(LifecycleError::BoundedAdmissionRequired.into());
                }
                self.registry.verify_path(
                    model,
                    model
                        .verified_path
                        .as_ref()
                        .ok_or(LifecycleError::MissingAuthoritativeCapability)?,
                )?;
            }
            RoutablePromotion(evidence) => {
                let path = model
                    .verified_path
                    .as_ref()
                    .ok_or(LifecycleError::MissingAuthoritativeCapability)?;
                self.registry.verify_path(model, path)?;
                let subject = CapabilitySubject::ModelRuntimePair {
                    provider_id: model.provider_id.clone(),
                    model_id: model.model_id.clone(),
                    runtime_id: path.runtime_id.clone(),
                };
                let sufficient = match evidence {
                    RoutablePromotionEvidence::LocalCalibration {
                        sufficient_acceptable_observations,
                    } => {
                        *sufficient_acceptable_observations
                            && transition.observation.confidence
                                == EvidenceConfidence::LocalEmpirical
                            && path.required_capabilities.iter().all(|cap| {
                                self.registry.supports(
                                    &subject,
                                    cap,
                                    EvidenceConfidence::LocalEmpirical,
                                )
                            })
                    }
                    RoutablePromotionEvidence::OfficialAndIndependent {
                        ask_on_uncertainty_user_consent,
                    } => {
                        *ask_on_uncertainty_user_consent
                            && transition.observation.confidence == EvidenceConfidence::UserDeclared
                            && path.required_capabilities.iter().all(|cap| {
                                self.registry.supports(
                                    &subject,
                                    cap,
                                    EvidenceConfidence::IndependentVerified,
                                )
                            })
                    }
                };
                if !sufficient {
                    return Err(LifecycleError::SufficientEvidenceRequired.into());
                }
            }
            ReadyForAssessment | Demotion(_) => {}
        }
        let model = self
            .registry
            .models
            .get_mut(provider)
            .unwrap()
            .get_mut(model_id)
            .unwrap();
        if let CapabilityVerification(path) = &transition.evidence {
            model.verified_path = Some(path.clone());
        }
        let from = model.lifecycle_state;
        model.lifecycle_state = transition.target;
        model
            .transitions
            .push(TransitionRecord { from, transition });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
