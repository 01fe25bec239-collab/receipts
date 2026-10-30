//! Exact integrality for a complete serde-admitted JSON number token.
use serde_json::value::RawValue;
use std::cmp::Ordering;

pub(crate) fn is_integer(value: &RawValue) -> bool {
    let text = value.get();
    if !matches!(text.as_bytes().first(), Some(b'-' | b'0'..=b'9')) {
        return false;
    }
    let (coefficient, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
    let coefficient = coefficient.strip_prefix('-').unwrap_or(coefficient);
    if coefficient.bytes().all(|b| matches!(b, b'0' | b'.')) {
        return true;
    }
    let fractional = coefficient.split_once('.').map_or(0, |(_, f)| f.len());
    let zeros = coefficient
        .bytes()
        .rev()
        .filter(|b| *b != b'.')
        .take_while(|b| *b == b'0')
        .count();
    let threshold_negative = zeros > fractional;
    let magnitude = fractional.abs_diff(zeros).to_string();
    let exponent_negative = exponent.starts_with('-');
    let digits = exponent
        .trim_start_matches(['+', '-'])
        .trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    let exponent_negative = exponent_negative && digits != "0";
    if exponent_negative != threshold_negative {
        return !exponent_negative;
    }
    let order = digits
        .len()
        .cmp(&magnitude.len())
        .then_with(|| digits.cmp(&magnitude));
    if exponent_negative {
        order != Ordering::Greater
    } else {
        order != Ordering::Less
    }
}

/// Coefficient of a number token (before any exponent).
fn coefficient(value: &RawValue) -> &str {
    value.get().split(['e', 'E']).next().unwrap_or("")
}

/// True when a number token's exact value is zero (`-0`, `0.0e9`, ...).
pub(crate) fn is_zero(value: &RawValue) -> bool {
    !coefficient(value)
        .bytes()
        .any(|b| (b'1'..=b'9').contains(&b))
}

/// True when a number token's exact value is strictly negative.
pub(crate) fn is_negative(value: &RawValue) -> bool {
    value.get().starts_with('-') && !is_zero(value)
}

/// The token itself when it is already canonical non-negative decimal
/// (`0` or digits without a leading zero); other integral spellings such
/// as `1.0`, `1e2` or `-0` have no direct operational spelling.
pub(crate) fn canonical_decimal(value: &RawValue) -> Option<&str> {
    let text = value.get();
    let b = text.as_bytes();
    (!b.is_empty() && b.iter().all(u8::is_ascii_digit) && (b[0] != b'0' || b.len() == 1))
        .then_some(text)
}
