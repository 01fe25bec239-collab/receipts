//! Strict canonical UTC timestamps used by trusted-time lease decisions.

use std::cmp::Ordering;

use crate::error::StateError;

/// A validated `YYYY-MM-DDTHH:MM:SS.nnnnnnnnnZ` timestamp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalTimestampV1 {
    value: String,
    components: [u32; 7],
}

impl CanonicalTimestampV1 {
    /// Parses only the one authorized UTC representation.
    pub fn parse(value: &str) -> Result<Self, StateError> {
        let bytes = value.as_bytes();
        if bytes.len() != 30
            || bytes[4] != b'-'
            || bytes[7] != b'-'
            || bytes[10] != b'T'
            || bytes[13] != b':'
            || bytes[16] != b':'
            || bytes[19] != b'.'
            || bytes[29] != b'Z'
        {
            return Err(invalid(value));
        }
        for range in [0..4, 5..7, 8..10, 11..13, 14..16, 17..19, 20..29] {
            if !bytes[range].iter().all(u8::is_ascii_digit) {
                return Err(invalid(value));
            }
        }

        let year = digits(bytes, 0, 4);
        let month = digits(bytes, 5, 7);
        let day = digits(bytes, 8, 10);
        let hour = digits(bytes, 11, 13);
        let minute = digits(bytes, 14, 16);
        let second = digits(bytes, 17, 19);
        let nanos = digits(bytes, 20, 29);
        let max_day = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if is_leap_year(year) => 29,
            2 => 28,
            _ => return Err(invalid(value)),
        };
        if day == 0 || day > max_day || hour > 23 || minute > 59 || second > 59 {
            return Err(invalid(value));
        }

        Ok(Self {
            value: value.to_string(),
            components: [year, month, day, hour, minute, second, nanos],
        })
    }

    /// Returns the exact validated representation.
    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl PartialOrd for CanonicalTimestampV1 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CanonicalTimestampV1 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.components.cmp(&other.components)
    }
}

fn digits(bytes: &[u8], start: usize, end: usize) -> u32 {
    bytes[start..end]
        .iter()
        .fold(0, |value, byte| value * 10 + u32::from(byte - b'0'))
}

fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

fn invalid(value: &str) -> StateError {
    StateError::CanonicalTimestampInvalid {
        value: value.to_string(),
    }
}
