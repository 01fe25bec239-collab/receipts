//! Validated string value objects for frozen product entitlement fields.
//!
//! Each type owns its accepted [`String`], keeps the inner string private,
//! checks the frozen field shape before construction, preserves accepted
//! text exactly, and exposes only read-only views of the stored text.

use std::fmt;

/// Longest accepted subject id, measured in Unicode scalar values.
const SUBJECT_ID_MAX_CHARS: usize = 200;

/// Narrow typed validation failure for product entitlement string fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProductEntitlementValueError {
    /// Subject id was empty.
    SubjectIdEmpty,
    /// Subject id held more than 200 Unicode scalar values.
    SubjectIdTooLong {
        /// Observed Unicode scalar value count.
        observed_chars: usize,
    },
    /// Tier id was empty.
    TierIdEmpty,
    /// Capability id did not match the frozen dotted shape.
    CapabilityIdInvalid,
    /// Key id was empty.
    KeyIdEmpty,
    /// Signature representation was empty.
    SignatureEmpty,
}

impl fmt::Display for ProductEntitlementValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectIdEmpty => write!(f, "product entitlement subject id is empty"),
            Self::SubjectIdTooLong { observed_chars } => write!(
                f,
                "product entitlement subject id exceeds 200 characters (observed: {observed_chars} characters)"
            ),
            Self::TierIdEmpty => write!(f, "product tier id is empty"),
            Self::CapabilityIdInvalid => {
                write!(f, "product capability id has an invalid shape")
            }
            Self::KeyIdEmpty => write!(f, "product entitlement key id is empty"),
            Self::SignatureEmpty => write!(f, "product entitlement signature is empty"),
        }
    }
}

impl std::error::Error for ProductEntitlementValueError {}

/// Opaque product account subject, 1..=200 Unicode scalar values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductEntitlementSubjectId(String);

impl ProductEntitlementSubjectId {
    /// Builds a subject id from a caller-supplied [`String`].
    ///
    /// The text is kept exactly as supplied; nothing is trimmed or
    /// rewritten.
    pub fn new(value: String) -> Result<Self, ProductEntitlementValueError> {
        if value.is_empty() {
            return Err(ProductEntitlementValueError::SubjectIdEmpty);
        }
        let observed_chars = value.chars().count();
        if observed_chars > SUBJECT_ID_MAX_CHARS {
            return Err(ProductEntitlementValueError::SubjectIdTooLong { observed_chars });
        }
        Ok(Self(value))
    }

    /// Returns the stored text exactly as accepted.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the wrapper and returns the stored text.
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// Extensible tier identifier; any non-empty string is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductTierId(String);

impl ProductTierId {
    /// Builds a tier id from a caller-supplied [`String`].
    ///
    /// The text is kept exactly as supplied; nothing is trimmed or
    /// rewritten.
    pub fn new(value: String) -> Result<Self, ProductEntitlementValueError> {
        if value.is_empty() {
            return Err(ProductEntitlementValueError::TierIdEmpty);
        }
        Ok(Self(value))
    }

    /// Returns the stored text exactly as accepted.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the wrapper and returns the stored text.
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// Dotted ASCII capability id, e.g. `graph.core`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductCapabilityId(String);

impl ProductCapabilityId {
    /// Builds a capability id from a caller-supplied [`String`].
    ///
    /// The text is kept exactly as supplied; nothing is trimmed or
    /// rewritten.
    pub fn new(value: String) -> Result<Self, ProductEntitlementValueError> {
        if !has_valid_capability_shape(&value) {
            return Err(ProductEntitlementValueError::CapabilityIdInvalid);
        }
        Ok(Self(value))
    }

    /// Returns the stored text exactly as accepted.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the wrapper and returns the stored text.
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// Opaque key identifier; any non-empty string is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductEntitlementKeyId(String);

impl ProductEntitlementKeyId {
    /// Builds a key id from a caller-supplied [`String`].
    ///
    /// The text is kept exactly as supplied; nothing is trimmed or
    /// rewritten.
    pub fn new(value: String) -> Result<Self, ProductEntitlementValueError> {
        if value.is_empty() {
            return Err(ProductEntitlementValueError::KeyIdEmpty);
        }
        Ok(Self(value))
    }

    /// Returns the stored text exactly as accepted.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the wrapper and returns the stored text.
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// Opaque signature storage representation; any non-empty string is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductEntitlementSignature(String);

impl ProductEntitlementSignature {
    /// Builds a signature holder from a caller-supplied [`String`].
    ///
    /// The text is kept exactly as supplied; nothing is trimmed or
    /// rewritten.
    pub fn new(value: String) -> Result<Self, ProductEntitlementValueError> {
        if value.is_empty() {
            return Err(ProductEntitlementValueError::SignatureEmpty);
        }
        Ok(Self(value))
    }

    /// Returns the stored text exactly as accepted.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the wrapper and returns the stored text.
    pub fn into_inner(self) -> String {
        self.0
    }
}

/// Checks the frozen dotted capability shape with ordinary string splits.
fn has_valid_capability_shape(value: &str) -> bool {
    let mut segments = value.split('.');
    let first = segments.next().unwrap_or("");
    let second = segments.next();
    let second = match second {
        Some(segment) => segment,
        None => return false,
    };
    if !is_valid_capability_segment(first) {
        return false;
    }
    if !is_valid_capability_segment(second) {
        return false;
    }
    for segment in segments {
        if !is_valid_capability_segment(segment) {
            return false;
        }
    }
    true
}

/// Checks one dot-separated segment: ASCII `a-z` first, then `a-z0-9_`.
fn is_valid_capability_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() => {}
        _ => return false,
    }
    for rest in chars {
        if !(rest.is_ascii_lowercase() || rest.is_ascii_digit() || rest == '_') {
            return false;
        }
    }
    true
}
