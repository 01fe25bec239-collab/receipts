//! Bounded ModelCapability evidence, not a complete arbitrary-JSON wire codec.
use crate::{EvidenceConfidence, EvidenceSourceRef, policy_eligibility::ModelRoutingDateTimeV1};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmptyIdentifier;

macro_rules! exact_identifier {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(String);
        impl $name {
            pub fn try_new(value: String) -> Result<Self, EmptyIdentifier> {
                if value.is_empty() {
                    return Err(EmptyIdentifier);
                }
                Ok(Self(value))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl std::borrow::Borrow<str> for $name {
            fn borrow(&self) -> &str {
                self.as_str()
            }
        }
    };
}
exact_identifier!(ProviderId);
exact_identifier!(ModelId);
exact_identifier!(RuntimeId);
exact_identifier!(CapabilityId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectKind {
    Model,
    Runtime,
    ModelRuntimePair,
}
impl SubjectKind {
    pub const ALL: [Self; 3] = [Self::Model, Self::Runtime, Self::ModelRuntimePair];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Model => "MODEL",
            Self::Runtime => "RUNTIME",
            Self::ModelRuntimePair => "MODEL_RUNTIME_PAIR",
        }
    }
}

/// Registry-bound subjects require their exact identities. This is a bounded
/// in-process association, not a replacement for the schema's optional fields.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum CapabilitySubject {
    Model {
        provider_id: ProviderId,
        model_id: ModelId,
    },
    Runtime {
        provider_id: ProviderId,
        runtime_id: RuntimeId,
    },
    ModelRuntimePair {
        provider_id: ProviderId,
        model_id: ModelId,
        runtime_id: RuntimeId,
    },
}
impl CapabilitySubject {
    pub fn kind(&self) -> SubjectKind {
        match self {
            Self::Model { .. } => SubjectKind::Model,
            Self::Runtime { .. } => SubjectKind::Runtime,
            Self::ModelRuntimePair { .. } => SubjectKind::ModelRuntimePair,
        }
    }
}

/// Only boolean capability assertions and explicit null/UNKNOWN are needed by
/// this foundation. An omitted schema value is represented by Option::None.
/// Scalars, arrays, objects and arbitrary JSON numbers are not implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityValue {
    Unknown,
    Boolean(bool),
}

/// Caller-supplied provenance. References are stored without being resolved;
/// observed_at preserves validated text and implies no freshness conclusion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub confidence: EvidenceConfidence,
    pub source_ref: Option<EvidenceSourceRef>,
    pub observed_at: ModelRoutingDateTimeV1,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityEvidence {
    pub subject: CapabilitySubject,
    pub capability: CapabilityId,
    pub value: Option<CapabilityValue>,
    pub observation: Observation,
    /// Absent / explicit null / nonnegative bounded u64; no threshold policy.
    pub sample_size: Option<Option<u64>>,
}

/// The complete ModelCapability record (every schema field). Subject
/// identifiers are optional exactly as in the schema; `value` is the open
/// schema value (`None` absent, JSON null = UNKNOWN). Conversion to registry
/// [`CapabilityEvidence`] is a separate, fallible step.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelCapabilityRecord {
    subject_kind: SubjectKind,
    provider_id: Option<String>,
    model_id: Option<String>,
    runtime_id: Option<String>,
    capability: String,
    value: Option<serde_json::Value>,
    confidence: EvidenceConfidence,
    source_ref: Option<EvidenceSourceRef>,
    observed_at: ModelRoutingDateTimeV1,
    sample_size: Option<Option<u64>>,
    lifecycle_state: Option<crate::intelligence::LifecycleState>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCapabilityRecordError {
    EmptyIdentifier,
    EmptyCapability,
    /// The subject identifiers do not name exactly the subject kind's ids.
    SubjectIdentityMismatch,
    /// A value other than boolean/null/"UNKNOWN" has no registry carrier.
    UnsupportedValue,
    /// Lifecycle rows are registry history, not capability evidence.
    LifecycleRow,
}

impl ModelCapabilityRecord {
    #[allow(clippy::too_many_arguments)] // Explicit schema fields; no hidden defaults.
    pub fn try_new(
        subject_kind: SubjectKind,
        provider_id: Option<String>,
        model_id: Option<String>,
        runtime_id: Option<String>,
        capability: String,
        value: Option<serde_json::Value>,
        confidence: EvidenceConfidence,
        source_ref: Option<EvidenceSourceRef>,
        observed_at: ModelRoutingDateTimeV1,
        sample_size: Option<Option<u64>>,
        lifecycle_state: Option<crate::intelligence::LifecycleState>,
    ) -> Result<Self, ModelCapabilityRecordError> {
        if [&provider_id, &model_id, &runtime_id]
            .into_iter()
            .any(|id| id.as_deref() == Some(""))
        {
            return Err(ModelCapabilityRecordError::EmptyIdentifier);
        }
        if capability.is_empty() {
            return Err(ModelCapabilityRecordError::EmptyCapability);
        }
        Ok(Self {
            subject_kind,
            provider_id,
            model_id,
            runtime_id,
            capability,
            value,
            confidence,
            source_ref,
            observed_at,
            sample_size,
            lifecycle_state,
        })
    }

    /// The record of one registry evidence observation (UNKNOWN → null).
    pub fn from_evidence(evidence: &CapabilityEvidence) -> Self {
        let (provider, model, runtime) = match &evidence.subject {
            CapabilitySubject::Model {
                provider_id,
                model_id,
            } => (provider_id, Some(model_id.as_str()), None),
            CapabilitySubject::Runtime {
                provider_id,
                runtime_id,
            } => (provider_id, None, Some(runtime_id.as_str())),
            CapabilitySubject::ModelRuntimePair {
                provider_id,
                model_id,
                runtime_id,
            } => (
                provider_id,
                Some(model_id.as_str()),
                Some(runtime_id.as_str()),
            ),
        };
        Self {
            subject_kind: evidence.subject.kind(),
            provider_id: Some(provider.as_str().to_owned()),
            model_id: model.map(str::to_owned),
            runtime_id: runtime.map(str::to_owned),
            capability: evidence.capability.as_str().to_owned(),
            value: evidence.value.map(|v| match v {
                CapabilityValue::Unknown => serde_json::Value::Null,
                CapabilityValue::Boolean(b) => serde_json::Value::Bool(b),
            }),
            confidence: evidence.observation.confidence,
            source_ref: evidence.observation.source_ref.clone(),
            observed_at: evidence.observation.observed_at.clone(),
            sample_size: evidence.sample_size,
            lifecycle_state: None,
        }
    }

    /// The registry evidence this record asserts. Every identifier the
    /// subject kind needs must be present and no other may be.
    pub fn to_evidence(&self) -> Result<CapabilityEvidence, ModelCapabilityRecordError> {
        use ModelCapabilityRecordError::*;
        if self.lifecycle_state.is_some() {
            return Err(LifecycleRow);
        }
        let id = |v: &Option<String>| v.clone();
        let p = id(&self.provider_id).ok_or(SubjectIdentityMismatch)?;
        let p = ProviderId::try_new(p).map_err(|_| EmptyIdentifier)?;
        let subject = match (self.subject_kind, id(&self.model_id), id(&self.runtime_id)) {
            (SubjectKind::Model, Some(m), None) => CapabilitySubject::Model {
                provider_id: p,
                model_id: ModelId::try_new(m).map_err(|_| EmptyIdentifier)?,
            },
            (SubjectKind::Runtime, None, Some(r)) => CapabilitySubject::Runtime {
                provider_id: p,
                runtime_id: RuntimeId::try_new(r).map_err(|_| EmptyIdentifier)?,
            },
            (SubjectKind::ModelRuntimePair, Some(m), Some(r)) => {
                CapabilitySubject::ModelRuntimePair {
                    provider_id: p,
                    model_id: ModelId::try_new(m).map_err(|_| EmptyIdentifier)?,
                    runtime_id: RuntimeId::try_new(r).map_err(|_| EmptyIdentifier)?,
                }
            }
            _ => return Err(SubjectIdentityMismatch),
        };
        let value = match &self.value {
            None => None,
            Some(serde_json::Value::Null) => Some(CapabilityValue::Unknown),
            Some(serde_json::Value::Bool(b)) => Some(CapabilityValue::Boolean(*b)),
            Some(serde_json::Value::String(s)) if s == "UNKNOWN" => Some(CapabilityValue::Unknown),
            Some(_) => return Err(UnsupportedValue),
        };
        Ok(CapabilityEvidence {
            subject,
            capability: CapabilityId::try_new(self.capability.clone())
                .map_err(|_| EmptyCapability)?,
            value,
            observation: Observation {
                confidence: self.confidence,
                source_ref: self.source_ref.clone(),
                observed_at: self.observed_at.clone(),
            },
            sample_size: self.sample_size,
        })
    }

    pub fn subject_kind(&self) -> SubjectKind {
        self.subject_kind
    }
    pub fn provider_id(&self) -> Option<&str> {
        self.provider_id.as_deref()
    }
    pub fn model_id(&self) -> Option<&str> {
        self.model_id.as_deref()
    }
    pub fn runtime_id(&self) -> Option<&str> {
        self.runtime_id.as_deref()
    }
    pub fn capability(&self) -> &str {
        &self.capability
    }
    pub fn value(&self) -> Option<&serde_json::Value> {
        self.value.as_ref()
    }
    pub fn confidence(&self) -> EvidenceConfidence {
        self.confidence
    }
    pub fn source_ref(&self) -> Option<&EvidenceSourceRef> {
        self.source_ref.as_ref()
    }
    pub fn observed_at(&self) -> &ModelRoutingDateTimeV1 {
        &self.observed_at
    }
    pub fn sample_size(&self) -> Option<Option<u64>> {
        self.sample_size
    }
    pub fn lifecycle_state(&self) -> Option<crate::intelligence::LifecycleState> {
        self.lifecycle_state
    }
}
