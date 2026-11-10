//! RI-WIRE: strict JSON codecs for the seven Review-owned schemas.
//!
//! Each `decode_*` validates the complete frozen machine shape (unknown and
//! duplicate members, required members, types, enums, identifier lengths,
//! SHA patterns, `minItems`, integer domains) and then constructs the
//! existing structured type, so its validation also applies. Each
//! `encode_*` writes schema property order with exact integer digits;
//! `decode(encode(x)) == x`.
//!
//! Absent and `null` stay distinct where the schema allows `null`
//! (`A3Handoff.blocker`, `A4Review.reproduction.limitation`, the
//! IntegrationRequest/IntegrationDecision nullable SHAs); absent and `[]`
//! stay distinct for optional arrays.
//!
//! Carrier limits are reported as [`ReviewWireErrorKind::UnsupportedValue`],
//! never silently narrowed: `exit_code` beyond `i64` in A3Handoff, A4Review
//! and ReviewCapsule checks; `line` beyond `u64` in A4Review findings;
//! integers beyond [`json::MAX_INTEGER_DIGITS`] digits anywhere; date-time
//! strings the adopted `ReviewDateTimeV1` binding refuses.

mod fields;
mod integration;
pub(crate) mod json;
mod review;
mod verdict;

pub use integration::{
    decode_integration_decision, decode_integration_request, encode_integration_decision,
    encode_integration_request,
};
pub use review::{
    decode_a3_handoff, decode_review_capsule, decode_review_request, encode_a3_handoff,
    encode_review_capsule, encode_review_request,
};
pub use verdict::{
    decode_a4_review, decode_safety_interruption, encode_a4_review, encode_safety_interruption,
};

use std::fmt;

/// Why a document was refused. `path` is a JSON pointer to the member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewWireError {
    pub path: String,
    pub kind: ReviewWireErrorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewWireErrorKind {
    /// Not one well-formed JSON value (includes lone surrogates, trailing data).
    Syntax,
    DuplicateKey,
    UnknownField,
    MissingField,
    WrongType,
    /// Enum, pattern, length, `minItems` or range violation.
    InvalidValue,
    /// Schema-valid but beyond the in-process carrier.
    UnsupportedValue,
}

impl fmt::Display for ReviewWireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?} at {}", self.kind, self.path)
    }
}
impl std::error::Error for ReviewWireError {}

#[cfg(test)]
mod wire_tests;
