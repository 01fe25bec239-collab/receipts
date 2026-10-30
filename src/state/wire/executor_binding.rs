//! `ExecutorBinding` wire codec.
use serde::{Deserialize, Serialize};

use super::{decode, encode, ensure, length_ok, present};
use crate::error::StateError;
use crate::executor_binding::{ExecutorBinding, ReleaseReason};

const SCHEMA: &str = "ExecutorBinding";

/// Schema-shaped ExecutorBinding; keeps absent vs `null` for `session_ref`
/// and `released_at`, which the operational carrier collapses.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutorBindingWireV1 {
    binding_id: String,
    role_id: String,
    provider_id: String,
    model_id: String,
    runtime_id: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    session_ref: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    routing_decision_id: Option<String>,
    bound_at: String,
    lease_expires_at: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    released_at: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    release_reason: Option<String>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    rehydration_completed: Option<bool>,
}

impl ExecutorBindingWireV1 {
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
        let opaque = |s: &str| length_ok(s, 1, None);
        ensure(
            SCHEMA,
            id(&self.binding_id)
                && id(&self.role_id)
                && opaque(&self.provider_id)
                && opaque(&self.model_id)
                && opaque(&self.runtime_id)
                && self.routing_decision_id.as_deref().is_none_or(id)
                && self
                    .release_reason
                    .as_deref()
                    .is_none_or(|r| ReleaseReason::from_storage(r).is_ok()),
        )
    }
    /// Converts to the operational carrier (collapsing absent/`null`).
    pub fn to_executor_binding(&self) -> Result<ExecutorBinding, StateError> {
        Ok(ExecutorBinding {
            binding_id: self.binding_id.clone(),
            role_id: self.role_id.clone(),
            provider_id: self.provider_id.clone(),
            model_id: self.model_id.clone(),
            runtime_id: self.runtime_id.clone(),
            session_ref: self.session_ref.clone().flatten(),
            routing_decision_id: self.routing_decision_id.clone(),
            bound_at: self.bound_at.clone(),
            lease_expires_at: self.lease_expires_at.clone(),
            released_at: self.released_at.clone().flatten(),
            release_reason: self
                .release_reason
                .as_deref()
                .map(ReleaseReason::from_storage)
                .transpose()?,
            rehydration_completed: self.rehydration_completed,
        })
    }
    /// Exports an operational binding; absent optionals are omitted.
    pub fn from_executor_binding(binding: &ExecutorBinding) -> Result<Self, StateError> {
        let wire = Self {
            binding_id: binding.binding_id.clone(),
            role_id: binding.role_id.clone(),
            provider_id: binding.provider_id.clone(),
            model_id: binding.model_id.clone(),
            runtime_id: binding.runtime_id.clone(),
            session_ref: binding.session_ref.clone().map(Some),
            routing_decision_id: binding.routing_decision_id.clone(),
            bound_at: binding.bound_at.clone(),
            lease_expires_at: binding.lease_expires_at.clone(),
            released_at: binding.released_at.clone().map(Some),
            release_reason: binding.release_reason.map(|r| r.as_str().to_string()),
            rehydration_completed: binding.rehydration_completed,
        };
        wire.validate()?;
        Ok(wire)
    }
}
