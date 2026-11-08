//! Shared Review integer carrier and its construction errors.
//! Moved unchanged from receipts-review-integration review_request_structured_core.

use std::fmt;

/// Construction failures for the request and its bounded integer carriers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRequestConstructionError {
    InvalidIdentifier(&'static str),
    MalformedBaselineSha,
    MalformedImplementationSha,
    EmptyObjective,
    EmptyAcceptanceCriteria,
    EmptyAllowedWritePaths,
    InvalidDecimalInteger,
    NegativeInteger,
    ZeroAttemptNumber,
}

impl fmt::Display for ReviewRequestConstructionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidIdentifier(field) => write!(f, "{field} must contain 1..=200 characters"),
            Self::MalformedBaselineSha => {
                f.write_str("baseline_sha must be 40 lowercase ASCII hex characters")
            }
            Self::MalformedImplementationSha => {
                f.write_str("implementation_sha must be 40 lowercase ASCII hex characters")
            }
            Self::EmptyObjective => f.write_str("objective is empty"),
            Self::EmptyAcceptanceCriteria => f.write_str("acceptance_criteria is empty"),
            Self::EmptyAllowedWritePaths => f.write_str("allowed_write_paths is empty"),
            Self::InvalidDecimalInteger => {
                f.write_str("expected decimal integer digits with an optional sign")
            }
            Self::NegativeInteger => f.write_str("integer value must be non-negative"),
            Self::ZeroAttemptNumber => f.write_str("attempt_number must be at least one"),
        }
    }
}
impl std::error::Error for ReviewRequestConstructionError {}

/// Dependency-free, arbitrary-magnitude non-negative integer VALUE carrier.
///
/// Decimal digits are internal storage, not a schema string or a JSON number
/// representation. Every non-negative integer has a finite decimal expansion;
/// no fixed native integer or application maximum limits the domain. Equivalent
/// signed/zero-padded decimal inputs compare equal. No wire lexical rule is
/// implied: a future wire adapter must map integer values to this carrier.
///
/// Shared by ReviewRequest and ReviewCapsule for `context_epoch`, without
/// ownership of State's ContextEpoch aggregate, epoch advancement, or State lookup.
///
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_contracts::ReviewRequestNonNegativeInteger) {
/// value.digits.clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_contracts::ReviewRequestNonNegativeInteger) {
/// let _: &mut str = value.decimal_digits();
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewRequestNonNegativeInteger {
    digits: String,
}

impl ReviewRequestNonNegativeInteger {
    /// Constructs from a decimal-domain spelling, NOT from JSON syntax.
    /// Accepts optional `+`/`-` and leading zeroes; rejects negative values.
    /// `-0` denotes zero. Fractions, exponents and whitespace are not parsed by
    /// this convenience constructor; this does not exclude any integer VALUE.
    pub fn from_decimal(value: &str) -> Result<Self, ReviewRequestConstructionError> {
        let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
        if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(ReviewRequestConstructionError::InvalidDecimalInteger);
        }
        let magnitude = digits.trim_start_matches('0');
        if value.starts_with('-') && !magnitude.is_empty() {
            return Err(ReviewRequestConstructionError::NegativeInteger);
        }
        Ok(Self {
            digits: if magnitude.is_empty() { "0" } else { magnitude }.into(),
        })
    }

    /// Canonical decimal magnitude for in-process inspection, not serialization.
    pub fn decimal_digits(&self) -> &str {
        &self.digits
    }
}

impl TryFrom<i64> for ReviewRequestNonNegativeInteger {
    type Error = ReviewRequestConstructionError;

    /// Compatibility input for native-width callers; storage remains unbounded.
    fn try_from(value: i64) -> Result<Self, Self::Error> {
        Self::from_decimal(&value.to_string())
    }
}
