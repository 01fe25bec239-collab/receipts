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
