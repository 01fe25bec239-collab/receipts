//! Version 1 static evidence import, limited to the existing registry domain.
//! Source references are preserved, never fetched or authenticated. Loading
//! discovers models only; it cannot promote lifecycle or authorize dispatch.
use super::*;
use crate::{EvidenceSourceRef, EvidenceSourceRefType, policy_eligibility::ModelRoutingDateTimeV1};
use serde::{Deserialize, Deserializer};
use std::io::Read;

#[derive(Debug)]
pub enum SeedLoadError {
    Json(serde_json::Error),
    UnsupportedVersion(u64),
    InvalidField(&'static str),
    Registry(RegistryError),
}
impl From<RegistryError> for SeedLoadError {
    fn from(value: RegistryError) -> Self {
        Self::Registry(value)
    }
}

/// Load a complete seed from a reader (including `std::fs::File`). No existing
/// service is mutated: any decoding or validation failure drops the private
/// import. All collections and per-record provenance are required in version 1.
pub fn load_registry_seed(reader: impl Read) -> Result<ModelIntelligenceService, SeedLoadError> {
    let seed: Seed = serde_json::from_reader(reader).map_err(SeedLoadError::Json)?;
    if seed.version != 1 {
        return Err(SeedLoadError::UnsupportedVersion(seed.version));
    }
    let mut service = ModelIntelligenceService::new();
    for p in seed.providers {
        service.insert_provider(provider(p.provider_id)?, p.observation.convert()?)?;
    }
    for m in seed.models {
        service.discover_model(
            provider(m.provider_id)?,
            model(m.model_id)?,
            m.observation.convert()?,
        )?;
    }
    for r in seed.runtimes {
        service.insert_runtime(
            provider(r.provider_id)?,
            runtime(r.runtime_id)?,
            r.observation.convert()?,
        )?;
    }
    for a in seed.associations {
        service.associate_runtime(
            provider(a.provider_id)?,
            model(a.model_id)?,
            runtime(a.runtime_id)?,
            a.observation.convert()?,
        )?;
    }
    for e in seed.capabilities {
        service.record_capability(CapabilityEvidence {
            subject: e.subject.convert()?,
            capability: CapabilityId::try_new(e.capability)
                .map_err(|_| SeedLoadError::InvalidField("capability"))?,
            value: match e.value {
                None => None,
                Some(Some(SeedValue::Boolean(b))) => Some(CapabilityValue::Boolean(b)),
                Some(None) => Some(CapabilityValue::Unknown),
                Some(Some(SeedValue::Unknown(s))) if s == "UNKNOWN" => {
                    Some(CapabilityValue::Unknown)
                }
                Some(Some(SeedValue::Unknown(_))) => {
                    return Err(SeedLoadError::InvalidField("value"));
                }
            },
            observation: e.observation.convert()?,
            sample_size: e.sample_size,
        })?;
    }
    Ok(service)
}

fn provider(s: String) -> Result<ProviderId, SeedLoadError> {
    ProviderId::try_new(s).map_err(|_| SeedLoadError::InvalidField("provider_id"))
}
fn model(s: String) -> Result<ModelId, SeedLoadError> {
    ModelId::try_new(s).map_err(|_| SeedLoadError::InvalidField("model_id"))
}
fn runtime(s: String) -> Result<RuntimeId, SeedLoadError> {
    RuntimeId::try_new(s).map_err(|_| SeedLoadError::InvalidField("runtime_id"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Seed {
    version: u64,
    providers: Vec<SeedProvider>,
    models: Vec<SeedModel>,
    runtimes: Vec<SeedRuntime>,
    associations: Vec<SeedAssociation>,
    capabilities: Vec<SeedCapability>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedProvider {
    provider_id: String,
    observation: SeedObservation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedModel {
    provider_id: String,
    model_id: String,
    observation: SeedObservation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedRuntime {
    provider_id: String,
    runtime_id: String,
    observation: SeedObservation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedAssociation {
    provider_id: String,
    model_id: String,
    runtime_id: String,
    observation: SeedObservation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedObservation {
    confidence: String,
    source_ref: SeedSource,
    observed_at: String,
}
impl SeedObservation {
    fn convert(self) -> Result<Observation, SeedLoadError> {
        let confidence = EvidenceConfidence::ALL
            .into_iter()
            .find(|c| c.as_str() == self.confidence)
            .ok_or(SeedLoadError::InvalidField("confidence"))?;
        let ref_type = EvidenceSourceRefType::ALL
            .into_iter()
            .find(|r| r.as_str() == self.source_ref.ref_type)
            .ok_or(SeedLoadError::InvalidField("ref_type"))?;
        Ok(Observation {
            confidence,
            source_ref: Some(
                EvidenceSourceRef::try_new(
                    ref_type,
                    self.source_ref.target,
                    self.source_ref.digest,
                    self.source_ref.section,
                )
                .map_err(|_| SeedLoadError::InvalidField("source_ref"))?,
            ),
            observed_at: ModelRoutingDateTimeV1::try_new(self.observed_at)
                .map_err(|_| SeedLoadError::InvalidField("observed_at"))?,
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedSource {
    ref_type: String,
    target: String,
    digest: Option<String>,
    section: Option<String>,
}
#[derive(Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum SeedSubject {
    #[serde(rename = "MODEL")]
    Model {
        provider_id: String,
        model_id: String,
    },
    #[serde(rename = "RUNTIME")]
    Runtime {
        provider_id: String,
        runtime_id: String,
    },
    #[serde(rename = "MODEL_RUNTIME_PAIR")]
    Pair {
        provider_id: String,
        model_id: String,
        runtime_id: String,
    },
}
impl SeedSubject {
    fn convert(self) -> Result<CapabilitySubject, SeedLoadError> {
        Ok(match self {
            Self::Model {
                provider_id,
                model_id,
            } => CapabilitySubject::Model {
                provider_id: provider(provider_id)?,
                model_id: model(model_id)?,
            },
            Self::Runtime {
                provider_id,
                runtime_id,
            } => CapabilitySubject::Runtime {
                provider_id: provider(provider_id)?,
                runtime_id: runtime(runtime_id)?,
            },
            Self::Pair {
                provider_id,
                model_id,
                runtime_id,
            } => CapabilitySubject::ModelRuntimePair {
                provider_id: provider(provider_id)?,
                model_id: model(model_id)?,
                runtime_id: runtime(runtime_id)?,
            },
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedCapability {
    subject: SeedSubject,
    capability: String,
    #[serde(default, deserialize_with = "present")]
    value: Option<Option<SeedValue>>,
    observation: SeedObservation,
    #[serde(default, deserialize_with = "present")]
    sample_size: Option<Option<u64>>,
}
// Deserialize a present nullable field separately from serde's missing default.
fn present<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}
#[derive(Deserialize)]
#[serde(untagged)]
enum SeedValue {
    Boolean(bool),
    Unknown(String),
}
