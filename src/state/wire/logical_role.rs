//! `LogicalRole` wire codec.
use serde::{Deserialize, Serialize};

use super::{WireInteger, decode, encode, ensure, length_ok, present, unsupported};
use crate::epoch_value::StateEpochValueV1;
use crate::error::StateError;
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};

const SCHEMA: &str = "LogicalRole";

/// Schema-shaped LogicalRole; keeps `ownership_paths` absent vs `[]` and
/// `active_binding_id` absent vs `null`, which the operational carrier collapses.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogicalRoleWireV1 {
    role_id: String,
    project_id: String,
    role_type: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    name: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    workstream_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    ownership_paths: Option<Vec<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    integration_branch: Option<String>,
    status: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    context_manifest_id: Option<String>,
    current_context_epoch: WireInteger,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    active_binding_id: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    created_at: Option<String>,
}

impl LogicalRoleWireV1 {
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
            id(&self.role_id)
                && id(&self.project_id)
                && matches!(self.role_type.as_str(), "RUNTIME_A1" | "RUNTIME_A2")
                && matches!(self.status.as_str(), "ACTIVE" | "SUSPENDED" | "RETIRED")
                && self.workstream_id.as_deref().is_none_or(id)
                && self.context_manifest_id.as_deref().is_none_or(id)
                && self.current_context_epoch.at_least(0),
        )
    }
    /// Converts to the operational carrier (collapsing absent/`[]` paths and
    /// absent/`null` binding). Epoch tokens other than canonical decimal
    /// (e.g. `1.0`, `1e2`, `-0`) are reported unsupported.
    pub fn to_logical_role(&self) -> Result<LogicalRole, StateError> {
        let epoch = self
            .current_context_epoch
            .canonical_decimal()
            .and_then(|d| StateEpochValueV1::try_from(d).ok())
            .ok_or_else(|| unsupported(SCHEMA, "current_context_epoch"))?;
        Ok(LogicalRole {
            role_id: self.role_id.clone(),
            project_id: self.project_id.clone(),
            role_type: LogicalRoleType::from_storage(&self.role_type)?,
            status: LogicalRoleStatus::from_storage(&self.status)?,
            current_context_epoch: epoch,
            name: self.name.clone(),
            workstream_id: self.workstream_id.clone(),
            ownership_paths: self.ownership_paths.clone().unwrap_or_default(),
            integration_branch: self.integration_branch.clone(),
            context_manifest_id: self.context_manifest_id.clone(),
            active_binding_id: self.active_binding_id.clone().flatten(),
            created_at: self.created_at.clone(),
        })
    }
    /// Exports an operational role; paths are always present and an absent
    /// binding is omitted. The result is re-validated against the schema.
    pub fn from_logical_role(role: &LogicalRole) -> Result<Self, StateError> {
        let wire = Self {
            role_id: role.role_id.clone(),
            project_id: role.project_id.clone(),
            role_type: role.role_type.as_str().to_string(),
            name: role.name.clone(),
            workstream_id: role.workstream_id.clone(),
            ownership_paths: Some(role.ownership_paths.clone()),
            integration_branch: role.integration_branch.clone(),
            status: role.status.as_str().to_string(),
            context_manifest_id: role.context_manifest_id.clone(),
            current_context_epoch: WireInteger::from_decimal(role.current_context_epoch.as_str())?,
            active_binding_id: role.active_binding_id.clone().map(Some),
            created_at: role.created_at.clone(),
        };
        wire.validate()?;
        Ok(wire)
    }
}
