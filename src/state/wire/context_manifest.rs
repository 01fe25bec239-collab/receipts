//! `ContextManifest` wire codec.
use serde::{Deserialize, Serialize};

use super::{WireInteger, decode, encode, ensure, length_ok, present, unsupported};
use crate::context_manifest::{
    ContextManifest, ContextManifestSource, ContextSourceRef, ContextSourceRefType, RequiredFor,
    SourceClass,
};
use crate::epoch_value::StateEpochValueV1;
use crate::error::StateError;
use crate::graph_node_result::RawObject;

const SCHEMA: &str = "ContextManifest";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceRefWire {
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

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SourceWire {
    r#ref: SourceRefWire,
    source_class: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    digest: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    last_read_at: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    required_for: Option<Vec<String>>,
}

/// Schema-shaped ContextManifest; keeps the open `derived_state` object's
/// exact bytes (duplicate decoded keys are refused), `URL` references and absent vs `null` optionals.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContextManifestWireV1 {
    manifest_id: String,
    role_id: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    project_id: Option<String>,
    epoch: WireInteger,
    sources: Vec<SourceWire>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    derived_state: Option<RawObject>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    last_rehydrated_at: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    created_at: Option<String>,
}

const REQUIRED_FOR: [&str; 5] = [
    "DECOMPOSITION",
    "DISPATCH",
    "ACCEPTANCE",
    "INTEGRATION",
    "EVALUATION",
];

impl ContextManifestWireV1 {
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
            id(&self.manifest_id)
                && id(&self.role_id)
                && self.project_id.as_deref().is_none_or(id)
                && self.epoch.at_least(0)
                // An open object with duplicate decoded keys has no single identity.
                && self
                    .derived_state
                    .as_ref()
                    .is_none_or(|open| crate::decision_records::unique_keys(&open.0).is_ok())
                && !self.sources.is_empty()
                && self.sources.iter().all(|s| {
                    matches!(
                        s.r#ref.ref_type.as_str(),
                        "REPO_PATH" | "STATE_QUERY" | "ARTIFACT_ID" | "URL"
                    ) && !s.r#ref.target.is_empty()
                        && matches!(
                            s.source_class.as_str(),
                            "MANDATORY" | "CONSUMED" | "REFERENCE"
                        )
                        && s.required_for
                            .iter()
                            .flatten()
                            .all(|r| REQUIRED_FOR.contains(&r.as_str()))
                }),
        )
    }
    /// Converts to the operational carrier. Unsupported there: absent
    /// `project_id`/`created_at`, a `URL` reference, a reference digest or
    /// section, an absent/`null` source digest, `derived_state`, and
    /// non-canonical-decimal epoch tokens.
    pub fn to_context_manifest(&self) -> Result<ContextManifest, StateError> {
        if self.derived_state.is_some() {
            return Err(unsupported(SCHEMA, "derived_state"));
        }
        let epoch = self
            .epoch
            .canonical_decimal()
            .and_then(|d| StateEpochValueV1::try_from(d).ok())
            .ok_or_else(|| unsupported(SCHEMA, "epoch"))?;
        let sources = self
            .sources
            .iter()
            .map(|s| {
                if s.r#ref.digest.is_some() || s.r#ref.section.is_some() {
                    return Err(unsupported(SCHEMA, "sources.ref"));
                }
                let ref_type = match s.r#ref.ref_type.as_str() {
                    "URL" => return Err(unsupported(SCHEMA, "sources.ref.ref_type")),
                    other => ContextSourceRefType::from_storage(other)?,
                };
                Ok(ContextManifestSource {
                    r#ref: ContextSourceRef {
                        ref_type,
                        target: s.r#ref.target.clone(),
                    },
                    source_class: SourceClass::from_storage(&s.source_class)?,
                    digest: s
                        .digest
                        .clone()
                        .flatten()
                        .ok_or_else(|| unsupported(SCHEMA, "sources.digest"))?,
                    last_read_at: s.last_read_at.clone().flatten(),
                    required_for: s
                        .required_for
                        .iter()
                        .flatten()
                        .map(|r| RequiredFor::from_storage(r))
                        .collect::<Result<_, _>>()?,
                })
            })
            .collect::<Result<Vec<_>, StateError>>()?;
        Ok(ContextManifest {
            manifest_id: self.manifest_id.clone(),
            role_id: self.role_id.clone(),
            project_id: self
                .project_id
                .clone()
                .ok_or_else(|| unsupported(SCHEMA, "project_id"))?,
            epoch,
            sources,
            created_at: self
                .created_at
                .clone()
                .ok_or_else(|| unsupported(SCHEMA, "created_at"))?,
            last_rehydrated_at: self.last_rehydrated_at.clone().flatten(),
        })
    }
    /// Exports an operational manifest; absent optionals are omitted.
    pub fn from_context_manifest(manifest: &ContextManifest) -> Result<Self, StateError> {
        let wire = Self {
            manifest_id: manifest.manifest_id.clone(),
            role_id: manifest.role_id.clone(),
            project_id: Some(manifest.project_id.clone()),
            epoch: WireInteger::from_decimal(manifest.epoch.as_str())?,
            sources: manifest
                .sources
                .iter()
                .map(|s| SourceWire {
                    r#ref: SourceRefWire {
                        ref_type: s.r#ref.ref_type.as_str().to_string(),
                        target: s.r#ref.target.clone(),
                        digest: None,
                        section: None,
                    },
                    source_class: s.source_class.as_str().to_string(),
                    digest: Some(Some(s.digest.clone())),
                    last_read_at: s.last_read_at.clone().map(Some),
                    required_for: Some(
                        s.required_for
                            .iter()
                            .map(|r| r.as_str().to_string())
                            .collect(),
                    ),
                })
                .collect(),
            derived_state: None,
            last_rehydrated_at: manifest.last_rehydrated_at.clone().map(Some),
            created_at: Some(manifest.created_at.clone()),
        };
        wire.validate()?;
        Ok(wire)
    }
}
