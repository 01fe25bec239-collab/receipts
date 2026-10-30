//! Exact JSON wire codecs for State's six owned schemas.
//!
//! Each `*WireV1` value is schema-shaped data, distinct from the operational
//! carrier and from verified values. Decoding uses serde's JSON parser with
//! unknown, duplicate and trailing input refused; absence, `null`, strings,
//! arrays, open objects and exact number tokens are preserved (numbers stay
//! raw tokens and are never converted through `f64`, so no numeric ceiling
//! is invented). `date-time` is a schema annotation here, so wire strings
//! are not reparsed as time. Conversion into an operational carrier is
//! explicit and fallible: an admitted wire value with no operational
//! representation reports [`StateError::WireConversionUnsupported`] and the
//! wire value is left unchanged. Product entitlements convert only through
//! the authoritative narrower entitlement parser; signature verification
//! stays with `EntitlementVerifier`.

mod activation_state;
mod context_epoch;
mod context_manifest;
mod executor_binding;
mod logical_role;
mod product_entitlement;
#[cfg(test)]
mod wire_tests;

pub use activation_state::ActivationStateWireV1;
pub use context_epoch::{ContextEpochPartsV1, ContextEpochWireV1};
pub use context_manifest::ContextManifestWireV1;
pub use executor_binding::ExecutorBindingWireV1;
pub use logical_role::LogicalRoleWireV1;
pub use product_entitlement::ProductEntitlementWireV1;

use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::value::RawValue;

use crate::error::StateError;

/// Distinguishes a present value (including `null` for `Option<Option<_>>`)
/// from absence when used with `#[serde(default)]`.
pub(crate) fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// A JSON number token that is exactly integral; kept verbatim.
#[derive(Serialize)]
pub(crate) struct WireInteger(Box<RawValue>);
impl<'de> Deserialize<'de> for WireInteger {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(d)?;
        if !matches!(raw.get().as_bytes().first(), Some(b'-' | b'0'..=b'9'))
            || !crate::json_integer::is_integer(&raw)
        {
            return Err(de::Error::custom("invalid integer"));
        }
        Ok(Self(raw))
    }
}
impl WireInteger {
    pub(crate) fn token(&self) -> &str {
        self.0.get()
    }
    pub(crate) fn at_least(&self, minimum: u8) -> bool {
        !crate::json_integer::is_negative(&self.0)
            && (minimum == 0 || !crate::json_integer::is_zero(&self.0))
    }
    pub(crate) fn canonical_decimal(&self) -> Option<&str> {
        crate::json_integer::canonical_decimal(&self.0)
    }
    pub(crate) fn from_decimal(decimal: &str) -> Result<Self, StateError> {
        RawValue::from_string(decimal.to_string())
            .map(Self)
            .map_err(|_| StateError::WireDecode { schema: "integer" })
    }
}

/// `minLength`/`maxLength` count Unicode scalar values.
pub(crate) fn length_ok(value: &str, min: usize, max: Option<usize>) -> bool {
    let n = value.chars().count();
    n >= min && max.is_none_or(|max| n <= max)
}

pub(crate) fn decode<'a, T: Deserialize<'a>>(
    schema: &'static str,
    text: &'a str,
) -> Result<T, StateError> {
    serde_json::from_str(text).map_err(|_| StateError::WireDecode { schema })
}

pub(crate) fn encode<T: Serialize>(schema: &'static str, value: &T) -> Result<String, StateError> {
    serde_json::to_string(value).map_err(|_| StateError::WireDecode { schema })
}

pub(crate) fn unsupported(schema: &'static str, field: &'static str) -> StateError {
    StateError::WireConversionUnsupported { schema, field }
}

pub(crate) fn ensure(schema: &'static str, ok: bool) -> Result<(), StateError> {
    if ok {
        Ok(())
    } else {
        Err(StateError::WireDecode { schema })
    }
}
