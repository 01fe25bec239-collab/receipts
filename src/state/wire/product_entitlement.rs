//! `ProductEntitlement` wire codec.
use serde::{Deserialize, Serialize};

use super::{WireInteger, decode, encode, ensure, length_ok, present, unsupported};
use crate::entitlement::{EntitlementVerificationError as Error, ProductEntitlement};
use crate::error::StateError;

const SCHEMA: &str = "ProductEntitlement";

/// Schema-shaped entitlement data. It is NOT a verified entitlement: schema
/// spellings such as explicit `null` optionals, very large or exponent
/// versions and non-canonical dates are admitted here but never become a
/// physical or verified value except through the authoritative parser.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductEntitlementWireV1 {
    subject_id: String,
    tier_id: String,
    capabilities: Vec<String>,
    issued_at: String,
    expires_at: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    offline_grace_until: Option<Option<String>>,
    entitlement_version: WireInteger,
    key_id: String,
    signature: String,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    device_binding: Option<Option<String>>,
}

/// `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`
fn capability(value: &str) -> bool {
    let mut parts = 0;
    value.split('.').all(|part| {
        parts += 1;
        let b = part.as_bytes();
        b.first().is_some_and(u8::is_ascii_lowercase)
            && b[1..]
                .iter()
                .all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || *x == b'_')
    }) && parts >= 2
}

impl ProductEntitlementWireV1 {
    pub fn parse_json(text: &str) -> Result<Self, StateError> {
        let wire: Self = decode(SCHEMA, text)?;
        ensure(
            SCHEMA,
            length_ok(&wire.subject_id, 1, Some(200))
                && !wire.tier_id.is_empty()
                && wire.capabilities.iter().all(|c| capability(c))
                && wire.entitlement_version.at_least(1)
                && !wire.key_id.is_empty()
                && !wire.signature.is_empty(),
        )?;
        Ok(wire)
    }
    pub fn to_json(&self) -> Result<String, StateError> {
        encode(SCHEMA, self)
    }
    /// The exact version token as admitted.
    pub fn entitlement_version_token(&self) -> &str {
        self.entitlement_version.token()
    }
    /// Converts through the authoritative V1 entitlement parser. Anything
    /// that parser refuses is reported unsupported; this yields a physical
    /// value only, and signature verification remains separate.
    pub fn to_physical(&self) -> Result<ProductEntitlement, StateError> {
        let bytes = self.to_json()?;
        crate::entitlement::wire::parse(bytes.as_bytes()).map_err(|error| {
            unsupported(
                SCHEMA,
                match error {
                    Error::DuplicateField(field)
                    | Error::MissingRequiredField(field)
                    | Error::InvalidPhysicalField(field)
                    | Error::UnsupportedTimestamp(field) => field,
                    Error::UnsupportedVersion => "entitlement_version",
                    _ => "document",
                },
            )
        })
    }
}
