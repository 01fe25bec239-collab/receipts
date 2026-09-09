//! Sequential V1 wire decoding. No decoded object map is used.
use std::fmt;

use serde::Deserializer;
use serde::de::{self, MapAccess, Visitor};

use super::{
    EntitlementVerificationError as Error, ProductCapabilityId, ProductEntitlement,
    ProductEntitlementKeyId, ProductEntitlementSignature, ProductEntitlementStringFields,
    ProductEntitlementSubjectId, ProductEntitlementVersion, ProductTierId,
};
use crate::CanonicalTimestampV1;

pub const MAX_ENTITLEMENT_WIRE_BYTES: usize = 65_536;

const FIELDS: [&str; 10] = [
    "subject_id",
    "tier_id",
    "capabilities",
    "issued_at",
    "expires_at",
    "entitlement_version",
    "key_id",
    "signature",
    "offline_grace_until",
    "device_binding",
];

#[derive(Default)]
struct Wire {
    subject_id: Option<String>,
    tier_id: Option<String>,
    capabilities: Option<Vec<String>>,
    issued_at: Option<String>,
    expires_at: Option<String>,
    entitlement_version: Option<i64>,
    key_id: Option<String>,
    signature: Option<String>,
    offline_grace_until: Option<String>,
    device_binding: Option<String>,
}

struct EntitlementVisitor<'a>(&'a mut Option<Error>);

impl<'de> Visitor<'de> for EntitlementVisitor<'_> {
    type Value = Wire;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("one entitlement object")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Wire, M::Error> {
        let mut wire = Wire::default();
        let mut seen = [false; 10];
        while let Some(key) = map.next_key::<String>()? {
            let Some(index) = FIELDS.iter().position(|field| *field == key) else {
                *self.0 = Some(Error::UnknownField);
                return Err(de::Error::custom("unknown entitlement field"));
            };
            if seen[index] {
                *self.0 = Some(Error::DuplicateField(FIELDS[index]));
                return Err(de::Error::custom("duplicate entitlement field"));
            }
            seen[index] = true;
            // Keep only static field metadata; never expose serde's input-bearing errors.
            macro_rules! value {
                () => {
                    map.next_value().map_err(|error| {
                        *self.0 = Some(if index == 5 {
                            Error::UnsupportedVersion
                        } else {
                            Error::InvalidPhysicalField(FIELDS[index])
                        });
                        error
                    })?
                };
            }
            match index {
                0 => wire.subject_id = Some(value!()),
                1 => wire.tier_id = Some(value!()),
                2 => wire.capabilities = Some(value!()),
                3 => wire.issued_at = Some(value!()),
                4 => wire.expires_at = Some(value!()),
                5 => wire.entitlement_version = Some(value!()),
                6 => wire.key_id = Some(value!()),
                7 => wire.signature = Some(value!()),
                // Schema null and omission both map to None. Seen bits remain independent.
                8 => wire.offline_grace_until = value!(),
                9 => wire.device_binding = value!(),
                _ => unreachable!(),
            }
        }
        for index in 0..8 {
            if !seen[index] {
                *self.0 = Some(Error::MissingRequiredField(FIELDS[index]));
                return Err(de::Error::custom("missing entitlement field"));
            }
        }
        Ok(wire)
    }
}

pub(super) fn parse(raw: &[u8]) -> Result<ProductEntitlement, Error> {
    if raw.len() > MAX_ENTITLEMENT_WIRE_BYTES {
        return Err(Error::DocumentTooLarge);
    }
    let text = std::str::from_utf8(raw).map_err(|_| Error::InvalidUtf8)?;
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let mut failure = None;
    let wire = deserializer
        .deserialize_map(EntitlementVisitor(&mut failure))
        .map_err(|error| {
            if error.is_eof() || (error.is_syntax() && failure != Some(Error::UnsupportedVersion)) {
                Error::MalformedJson
            } else {
                failure.unwrap_or(Error::MalformedJson)
            }
        })?;
    deserializer.end().map_err(|_| Error::MalformedJson)?;
    wire.validate()
}

impl Wire {
    fn validate(self) -> Result<ProductEntitlement, Error> {
        fn required<T>(value: Option<T>, field: &'static str) -> Result<T, Error> {
            value.ok_or(Error::MissingRequiredField(field))
        }
        fn timestamp(value: &str, field: &'static str) -> Result<CanonicalTimestampV1, Error> {
            CanonicalTimestampV1::parse(value).map_err(|_| Error::UnsupportedTimestamp(field))
        }
        let strings = ProductEntitlementStringFields::new(
            ProductEntitlementSubjectId::new(required(self.subject_id, "subject_id")?)
                .map_err(|_| Error::InvalidPhysicalField("subject_id"))?,
            ProductTierId::new(required(self.tier_id, "tier_id")?)
                .map_err(|_| Error::InvalidPhysicalField("tier_id"))?,
            required(self.capabilities, "capabilities")?
                .into_iter()
                .map(|value| {
                    ProductCapabilityId::new(value)
                        .map_err(|_| Error::InvalidPhysicalField("capabilities"))
                })
                .collect::<Result<_, _>>()?,
            ProductEntitlementKeyId::new(required(self.key_id, "key_id")?)
                .map_err(|_| Error::InvalidPhysicalField("key_id"))?,
            ProductEntitlementSignature::new(required(self.signature, "signature")?)
                .map_err(|_| Error::InvalidPhysicalField("signature"))?,
            self.device_binding,
        );
        Ok(ProductEntitlement::new(
            strings,
            timestamp(&required(self.issued_at, "issued_at")?, "issued_at")?,
            timestamp(&required(self.expires_at, "expires_at")?, "expires_at")?,
            ProductEntitlementVersion::new(required(
                self.entitlement_version,
                "entitlement_version",
            )?)
            .ok_or(Error::UnsupportedVersion)?,
            self.offline_grace_until
                .as_deref()
                .map(|value| timestamp(value, "offline_grace_until"))
                .transpose()?,
        ))
    }
}
