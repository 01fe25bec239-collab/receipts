//! `ActivationState` wire codec.
use serde::{Deserialize, Serialize};

use super::{decode, encode, ensure, present, unsupported};
use crate::canonical_timestamp::CanonicalTimestampV1;
use crate::entitlement::{ActivationIdentityFields, ActivationState, ActivationStateKind};
use crate::error::StateError;

const SCHEMA: &str = "ActivationState";

/// Schema-shaped activation provenance; keeps absent vs `null`.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ActivationStateWireV1 {
    activation_state: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    subject_id: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    first_activated_at: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    last_known_tier_id: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    last_entitlement_seen_at: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    last_observed_server_time: Option<Option<String>>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    logged_out_at: Option<Option<String>>,
    recorded_at: String,
}

fn kind(value: &str) -> Option<ActivationStateKind> {
    ActivationStateKind::ALL
        .into_iter()
        .find(|k| k.as_str() == value)
}

impl ActivationStateWireV1 {
    pub fn parse_json(text: &str) -> Result<Self, StateError> {
        let wire: Self = decode(SCHEMA, text)?;
        ensure(SCHEMA, kind(&wire.activation_state).is_some())?;
        Ok(wire)
    }
    pub fn to_json(&self) -> Result<String, StateError> {
        encode(SCHEMA, self)
    }
    /// Converts to the physical carrier: timestamps must be
    /// [`CanonicalTimestampV1`] and identity fields must be consistent;
    /// otherwise the named field is reported unsupported.
    pub fn to_physical(&self) -> Result<ActivationState, StateError> {
        let time = |value: &Option<Option<String>>, field: &'static str| {
            value
                .clone()
                .flatten()
                .map(|v| CanonicalTimestampV1::parse(&v).map_err(|_| unsupported(SCHEMA, field)))
                .transpose()
        };
        let identity = ActivationIdentityFields::new(
            kind(&self.activation_state).ok_or_else(|| unsupported(SCHEMA, "activation_state"))?,
            self.subject_id.clone().flatten(),
            self.last_known_tier_id.clone().flatten(),
        );
        ActivationState::new(
            identity,
            time(&self.first_activated_at, "first_activated_at")?,
            time(&self.last_entitlement_seen_at, "last_entitlement_seen_at")?,
            time(&self.last_observed_server_time, "last_observed_server_time")?,
            time(&self.logged_out_at, "logged_out_at")?,
            CanonicalTimestampV1::parse(&self.recorded_at)
                .map_err(|_| unsupported(SCHEMA, "recorded_at"))?,
        )
        .map_err(|_| unsupported(SCHEMA, "activation_state"))
    }
}
