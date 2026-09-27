use std::cmp::Ordering;

use super::ModelRoutingDateTimeV1;

/// The only comparison refusal: either operand has lexical second 60, whose
/// real occurrence and instant the lexical carrier does not establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelRoutingDateTimeComparisonError {
    UnsupportedLeapSecond,
}

/// Orders two accepted date-times by the UTC instant they denote: earlier is
/// `Less`. The local time minus its numeric offset gives UTC (RFC 3339 §4.2);
/// `Z`, `z`, `+00:00` and `-00:00` are zero offsets for this purpose only, so
/// the carriers' lexical `Eq`/`Hash` and text are untouched. Fractions compare
/// exactly at any length, trailing zeros ignored. Second 60 in either operand
/// is refused before any comparison, including of identical text. Pure: no
/// clock, freshness, latest-record or conflict semantics.
///
/// The lexical carrier deliberately has no ordering:
///
/// ```compile_fail
/// use receipts_model_routing::policy_eligibility::ModelRoutingDateTimeV1;
/// fn earlier(a: &ModelRoutingDateTimeV1, b: &ModelRoutingDateTimeV1) -> bool {
///     a < b
/// }
/// ```
///
/// ```
/// use receipts_model_routing::policy_eligibility::{
///     ModelRoutingDateTimeV1, compare_instants,
/// };
/// let local = ModelRoutingDateTimeV1::try_new("2026-09-08T18:50:00-04:00".into()).unwrap();
/// let utc = ModelRoutingDateTimeV1::try_new("2026-09-08T22:50:00Z".into()).unwrap();
/// assert_eq!(compare_instants(&local, &utc), Ok(std::cmp::Ordering::Equal));
/// assert_ne!(local, utc);
/// ```
pub fn compare_instants(
    left: &ModelRoutingDateTimeV1,
    right: &ModelRoutingDateTimeV1,
) -> Result<Ordering, ModelRoutingDateTimeComparisonError> {
    let (left, right) = (instant(left.as_str())?, instant(right.as_str())?);
    Ok(left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)))
}

/// Whole UTC seconds relative to 1970-01-01T00:00:00Z plus significant
/// fraction digits. Input is already validated by the carrier; the i64 range
/// covers every accepted value and offset crossing of years 0000/9999.
fn instant(text: &str) -> Result<(i64, &[u8]), ModelRoutingDateTimeComparisonError> {
    let bytes = text.as_bytes();
    let field = |start: usize, end: usize| {
        bytes[start..end]
            .iter()
            .fold(0_i64, |value, digit| value * 10 + i64::from(digit - b'0'))
    };
    let second = field(17, 19);
    if second == 60 {
        return Err(ModelRoutingDateTimeComparisonError::UnsupportedLeapSecond);
    }
    let (fraction_end, offset_seconds) = match bytes[bytes.len() - 1] {
        b'Z' | b'z' => (bytes.len() - 1, 0),
        _ => {
            let start = bytes.len() - 6;
            let magnitude = field(start + 1, start + 3) * 3600 + field(start + 4, start + 6) * 60;
            let signed = if bytes[start] == b'-' {
                -magnitude
            } else {
                magnitude
            };
            (start, signed)
        }
    };
    let mut fraction = bytes.get(20..fraction_end).unwrap_or_default();
    while let [significant @ .., b'0'] = fraction {
        fraction = significant;
    }
    let days = days_from_civil(field(0, 4), field(5, 7), field(8, 10));
    let local_seconds = days * 86_400 + field(11, 13) * 3600 + field(14, 16) * 60 + second;
    Ok((local_seconds - offset_seconds, fraction))
}

/// Proleptic Gregorian day number with 1970-01-01 as 0 (H. Hinnant's
/// `days_from_civil`); `div_euclid` keeps year 0000's January and February,
/// counted in era -1, exact.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}
