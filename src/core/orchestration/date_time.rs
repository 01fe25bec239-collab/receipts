//! Physical binding under BUILD-A1-ADR-ORCHESTRATION-DATETIME-PHYSICAL-V1-001.

/// An input does not satisfy the Orchestration V1 date-time grammar/calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrchestrationDateTimeError;

impl std::fmt::Display for OrchestrationDateTimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid Orchestration V1 date-time")
    }
}

impl std::error::Error for OrchestrationDateTimeError {}

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
/// use receipts_orchestration::orchestration::OrchestrationDateTimeV1;
/// let value = OrchestrationDateTimeV1::try_new("2026-09-08t00:00:00.100z")?;
/// assert_eq!(value.as_str(), "2026-09-08t00:00:00.100z");
/// # Ok::<(), receipts_orchestration::orchestration::OrchestrationDateTimeError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrchestrationDateTimeV1(String);

impl OrchestrationDateTimeV1 {
    /// Validates the caller-supplied representation before constructing a value.
    pub fn try_new(value: impl Into<String>) -> Result<Self, OrchestrationDateTimeError> {
        let value = value.into();
        validate(value.as_bytes())?;
        Ok(Self(value))
    }

    /// The exact accepted representation, byte-for-byte.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate(bytes: &[u8]) -> Result<(), OrchestrationDateTimeError> {
    // Guard every fixed byte offset before indexing. No UTF-8 string slicing.
    if bytes.len() < 20
        || !bytes.is_ascii()
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return Err(OrchestrationDateTimeError);
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
        _ => return Err(OrchestrationDateTimeError),
    };
    if !(1..=days).contains(&day)
        || decimal(&bytes[11..13])? > 23
        || decimal(&bytes[14..16])? > 59
        || decimal(&bytes[17..19])? > 60
    {
        return Err(OrchestrationDateTimeError);
    }

    let mut offset = 19;
    if bytes[offset] == b'.' {
        offset += 1;
        let start = offset;
        while bytes.get(offset).is_some_and(u8::is_ascii_digit) {
            offset += 1;
        }
        if offset == start {
            return Err(OrchestrationDateTimeError);
        }
    }
    match &bytes[offset..] {
        [b'Z' | b'z'] => Ok(()),
        [b'+' | b'-', h1, h2, b':', m1, m2]
            if decimal(&[*h1, *h2])? <= 23 && decimal(&[*m1, *m2])? <= 59 =>
        {
            Ok(())
        }
        _ => Err(OrchestrationDateTimeError),
    }
}

// Called only for fixed two- or four-digit components, never the fraction.
fn decimal(bytes: &[u8]) -> Result<u16, OrchestrationDateTimeError> {
    let mut value = 0;
    for byte in bytes {
        if !byte.is_ascii_digit() {
            return Err(OrchestrationDateTimeError);
        }
        value = value * 10 + u16::from(byte - b'0');
    }
    Ok(value)
}
