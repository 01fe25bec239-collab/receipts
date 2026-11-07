//! `ContextEpoch` wire codec.
use serde::{Deserialize, Serialize};

use super::{WireInteger, decode, encode, ensure, length_ok, present, unsupported};
use crate::context_epoch::{
    ChangedSource, ChangedSourceRefType, ChangedSources, ContextEpoch, ContextEpochTrigger,
};
use crate::epoch_value::StateEpochValueV1;
use crate::error::StateError;

const SCHEMA: &str = "ContextEpoch";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChangedSourceWire {
    ref_type: String,
    target: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    digest: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    section: Option<String>,
}

/// Schema-shaped ContextEpoch; keeps absent vs `[]` for both arrays.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextEpochWireV1 {
    epoch: WireInteger,
    project_id: String,
    advanced_at: String,
    trigger: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    changed_sources: Option<Vec<ChangedSourceWire>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    invalidated_role_ids: Option<Vec<String>>,
}

/// Operational parts of one epoch with array presence retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextEpochPartsV1 {
    pub epoch: ContextEpoch,
    pub changed_sources: ChangedSources,
    pub invalidated_role_ids: Option<Vec<String>>,
}

impl ContextEpochWireV1 {
    pub fn parse_json(text: &str) -> Result<Self, StateError> {
        let wire: Self = decode(SCHEMA, text)?;
        wire.validate()?;
        Ok(wire)
    }
    pub fn to_json(&self) -> Result<String, StateError> {
        encode(SCHEMA, self)
    }
    fn validate(&self) -> Result<(), StateError> {
        let id = |s: &str| length_ok(s, 1, Some(200));
        ensure(
            SCHEMA,
            self.epoch.at_least(0)
                && id(&self.project_id)
                && ContextEpochTrigger::from_storage(&self.trigger).is_ok()
                && self.changed_sources.iter().flatten().all(|c| {
                    ChangedSourceRefType::from_storage(&c.ref_type).is_ok() && !c.target.is_empty()
                })
                && self.invalidated_role_ids.iter().flatten().all(|r| id(r)),
        )
    }
    /// Converts to operational parts; non-canonical-decimal epoch tokens are
    /// reported unsupported.
    pub fn to_parts(&self) -> Result<ContextEpochPartsV1, StateError> {
        let epoch = self
            .epoch
            .canonical_decimal()
            .and_then(|d| StateEpochValueV1::try_from(d).ok())
            .ok_or_else(|| unsupported(SCHEMA, "epoch"))?;
        Ok(ContextEpochPartsV1 {
            epoch: ContextEpoch {
                project_id: self.project_id.clone(),
                epoch,
                advanced_at: self.advanced_at.clone(),
                trigger: ContextEpochTrigger::from_storage(&self.trigger)?,
            },
            changed_sources: match &self.changed_sources {
                None => ChangedSources::Omitted,
                Some(sources) => ChangedSources::Present(
                    sources
                        .iter()
                        .map(|c| {
                            Ok(ChangedSource {
                                ref_type: ChangedSourceRefType::from_storage(&c.ref_type)?,
                                target: c.target.clone(),
                                digest: c.digest.clone(),
                                section: c.section.clone(),
                            })
                        })
                        .collect::<Result<_, StateError>>()?,
                ),
            },
            invalidated_role_ids: self.invalidated_role_ids.clone(),
        })
    }
    /// Exports operational parts, retaining array presence.
    pub fn from_parts(parts: &ContextEpochPartsV1) -> Result<Self, StateError> {
        let wire = Self {
            epoch: WireInteger::from_decimal(parts.epoch.epoch.as_str())?,
            project_id: parts.epoch.project_id.clone(),
            advanced_at: parts.epoch.advanced_at.clone(),
            trigger: parts.epoch.trigger.as_str().to_string(),
            changed_sources: match &parts.changed_sources {
                ChangedSources::Omitted => None,
                ChangedSources::Present(sources) => Some(
                    sources
                        .iter()
                        .map(|c| ChangedSourceWire {
                            ref_type: c.ref_type.as_str().to_string(),
                            target: c.target.clone(),
                            digest: c.digest.clone(),
                            section: c.section.clone(),
                        })
                        .collect(),
                ),
            },
            invalidated_role_ids: parts.invalidated_role_ids.clone(),
        };
        wire.validate()?;
        Ok(wire)
    }
}
