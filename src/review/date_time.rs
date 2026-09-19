//! Physical binding under BUILD-A1-ADR-REVIEW-DATETIME-PHYSICAL-V1-001.

/// An input does not satisfy the Review V1 date-time grammar/calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewDateTimeError;

impl std::fmt::Display for ReviewDateTimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid Review V1 date-time")
    }
}

impl std::error::Error for ReviewDateTimeError {}

/// Validated ASCII date-time evidence, preserving the supplied string exactly.
/// Equality and hashing identify lexical values, not equivalent instants.
/// No ordering, clock, timezone conversion, or business temporal policy is owned.
///
/// Years are exactly four digits (including 0000); dates use the Gregorian
/// calendar. Time is hh:mm:ss (00..23, 00..59, 00..60), optionally followed by
/// a decimal point and one or more digits of unlimited precision. The separator
/// is T/t; the required offset is Z/z or +/-HH:MM (00..23, 00..59).
/// Second 60 is lexical acceptance only, without an occurrence database.
/// Whitespace is rejected and no accepted component is normalized.
///
/// ```
/// use receipts_review_integration::ReviewDateTimeV1;
/// let value = ReviewDateTimeV1::try_new("2026-09-08t00:00:00.100z")?;
/// assert_eq!(value.as_str(), "2026-09-08t00:00:00.100z");
/// # Ok::<(), receipts_review_integration::ReviewDateTimeError>(())
/// ```
/// Validation cannot be bypassed or the backing string mutated externally.
///
/// ```compile_fail
/// use receipts_review_integration::ReviewDateTimeV1;
/// let _ = ReviewDateTimeV1(String::from("invalid"));
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewDateTimeV1) {
/// value.0.clear();
/// # }
/// ```
/// ```compile_fail
/// # fn forbidden(value: &mut receipts_review_integration::ReviewDateTimeV1) {
/// let _: &mut str = value.as_str();
/// # }
/// ```
/// Ordering is intentionally unavailable, including lexical ordering.
///
/// ```compile_fail
/// fn requires_ord<T: Ord>() {}
/// requires_ord::<receipts_review_integration::ReviewDateTimeV1>();
/// ```
/// ```compile_fail
/// fn requires_partial_ord<T: PartialOrd>() {}
/// requires_partial_ord::<receipts_review_integration::ReviewDateTimeV1>();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReviewDateTimeV1(String);

impl ReviewDateTimeV1 {
    /// Validates the caller-supplied representation before constructing a value.
    pub fn try_new(value: impl Into<String>) -> Result<Self, ReviewDateTimeError> {
        let value = value.into();
        validate(value.as_bytes())?;
        Ok(Self(value))
    }

    /// The exact accepted representation, byte-for-byte.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate(bytes: &[u8]) -> Result<(), ReviewDateTimeError> {
    // Guard every fixed byte offset before indexing. No UTF-8 string slicing.
    if bytes.len() < 20
        || !bytes.is_ascii()
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return Err(ReviewDateTimeError);
    }

    let year = decimal(&bytes[..4])?;
    let month = decimal(&bytes[5..7])?;
    let day = decimal(&bytes[8..10])?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(ReviewDateTimeError),
    };
    if !(1..=days).contains(&day)
        || decimal(&bytes[11..13])? > 23
        || decimal(&bytes[14..16])? > 59
        || decimal(&bytes[17..19])? > 60
    {
        return Err(ReviewDateTimeError);
    }

    let mut offset = 19;
    if bytes[offset] == b'.' {
        offset += 1;
        let start = offset;
        while bytes.get(offset).is_some_and(u8::is_ascii_digit) {
            offset += 1;
        }
        if offset == start {
            return Err(ReviewDateTimeError);
        }
    }
    match &bytes[offset..] {
        [b'Z' | b'z'] => Ok(()),
        [b'+' | b'-', h1, h2, b':', m1, m2]
            if decimal(&[*h1, *h2])? <= 23 && decimal(&[*m1, *m2])? <= 59 =>
        {
            Ok(())
        }
        _ => Err(ReviewDateTimeError),
    }
}

// Called only for fixed two- or four-digit components, never the fraction.
fn decimal(bytes: &[u8]) -> Result<u16, ReviewDateTimeError> {
    let mut value = 0;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err(ReviewDateTimeError);
        }
        value = value * 10 + u16::from(byte - b'0');
    }
    Ok(value)
}
