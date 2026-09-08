/// Exact lexical JSON-Schema date-time V1 value, owned by Model-Routing.
/// Validates Gregorian dates, ASCII syntax, and lexical second 60 without
/// checking leap-second occurrences. Accepted text is never rewritten.
/// Equality is lexical identity; there is no temporal ordering or clock access.
///
/// ```compile_fail
/// use receipts_model_routing::policy_eligibility::ModelRoutingDateTimeV1;
/// let unchecked = ModelRoutingDateTimeV1(String::new());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelRoutingDateTimeV1(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelRoutingDateTimeV1Error;

impl ModelRoutingDateTimeV1 {
    pub fn try_new(value: String) -> Result<Self, ModelRoutingDateTimeV1Error> {
        if !valid_date_time(value.as_bytes()) {
            return Err(ModelRoutingDateTimeV1Error);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn digits(bytes: &[u8]) -> Option<u16> {
    bytes.iter().try_fold(0, |value, byte| {
        byte.is_ascii_digit()
            .then(|| value * 10 + u16::from(byte - b'0'))
    })
}

fn valid_date_time(bytes: &[u8]) -> bool {
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || !matches!(bytes[10], b'T' | b't')
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return false;
    }
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        digits(&bytes[..4]),
        digits(&bytes[5..7]),
        digits(&bytes[8..10]),
        digits(&bytes[11..13]),
        digits(&bytes[14..16]),
        digits(&bytes[17..19]),
    ) else {
        return false;
    };
    let days = match month {
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => return false,
    };
    if day == 0 || day > days || hour > 23 || minute > 59 || second > 60 {
        return false;
    }
    let mut offset = 19;
    if bytes[offset] == b'.' {
        offset += 1;
        let start = offset;
        while bytes.get(offset).is_some_and(u8::is_ascii_digit) {
            offset += 1;
        }
        if offset == start {
            return false;
        }
    }
    match &bytes[offset..] {
        [b'Z' | b'z'] => true,
        [b'+' | b'-', h1, h2, b':', m1, m2] => {
            digits(&[*h1, *h2]).is_some_and(|hour| hour <= 23)
                && digits(&[*m1, *m2]).is_some_and(|minute| minute <= 59)
        }
        _ => false,
    }
}
