//! Exact JSON numbers (F5/F6): wire admission is separate from the fallible
//! conversion into an operational carrier.
//!
//! A numeric field arrives as its raw token. Admission judges the token's
//! exact mathematical value against the schema (a number; an integer is
//! integral; `minimum`/`maximum`): a violation is `Shape`. Only an admitted
//! value is converted to the carrier (`u64`, `i64`, finite `f64`), and only
//! when the carrier holds *that exact value*; otherwise the value is
//! schema-valid but unsupported and the conversion is `Refused`. A value is
//! never rounded, saturated or flushed to zero.
//!
//! * integers: the canonical decimal spelling of a value in range; another
//!   integral spelling (`1.0`, `1e2`, `-0`) or an out-of-range value is
//!   `Refused` (no direct operational spelling);
//! * `f64`: the token is accepted when the carrier's shortest round-trip
//!   decimal is the same decimal value, so `0.1` and `12.50` are kept while
//!   `9007199254740993`, `1e400` and `1e-400` are refused.
//!
//! Exponents beyond `i128` saturate, which keeps their sign and dwarfs any
//! digit count a token can carry.

use serde_json::Value;
use serde_json::value::RawValue;

use super::{R, RoutingWireError};

/// The exact value of a JSON number token: `(-1)^negative × digits × 10^exponent`
/// with no leading or trailing zero digit; zero has no digits and no sign.
#[derive(Debug, PartialEq, Eq)]
struct Exact {
    negative: bool,
    digits: String,
    exponent: i128,
}

impl Exact {
    fn of(token: &str) -> Self {
        let (negative, rest) = match token.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, token),
        };
        let (mantissa, exponent) = rest.split_once(['e', 'E']).unwrap_or((rest, "0"));
        let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let all = format!("{int}{frac}");
        let significant = all.trim_start_matches('0');
        let digits = significant.trim_end_matches('0');
        if digits.is_empty() {
            return Self {
                negative: false,
                digits: String::new(),
                exponent: 0,
            };
        }
        let magnitude = exponent
            .trim_start_matches(['+', '-'])
            .trim_start_matches('0');
        let magnitude: i128 = if magnitude.len() > 30 {
            i128::MAX / 4
        } else {
            magnitude.parse().unwrap_or(0)
        };
        let exponent = if exponent.starts_with('-') {
            -magnitude
        } else {
            magnitude
        };
        Self {
            negative,
            digits: digits.to_owned(),
            exponent: exponent - frac.len() as i128 + (significant.len() - digits.len()) as i128,
        }
    }

    fn is_zero(&self) -> bool {
        self.digits.is_empty()
    }

    fn is_integral(&self) -> bool {
        self.is_zero() || self.exponent >= 0
    }

    /// Strictly greater than one.
    fn exceeds_one(&self) -> bool {
        if self.negative || self.is_zero() {
            return false;
        }
        // value = digits × 10^exponent; its decimal point sits `magnitude`
        // digits from the left of the first digit.
        let magnitude = self.digits.len() as i128 + self.exponent;
        match magnitude {
            ..=0 => false,
            1 => self.digits != "1",
            _ => true,
        }
    }
}

/// The token of a numeric field; any other JSON type is a shape error.
fn numeric<'a>(raw: &'a RawValue, schema: &'static str) -> R<&'a str> {
    let token = raw.get();
    matches!(token.as_bytes().first(), Some(b'-' | b'0'..=b'9'))
        .then_some(token)
        .ok_or(RoutingWireError::Shape(schema))
}

fn canonical_digits(text: &str) -> bool {
    let b = text.as_bytes();
    !b.is_empty() && b.iter().all(u8::is_ascii_digit) && (b[0] != b'0' || b.len() == 1)
}

fn canonical_u64(token: &str) -> Option<u64> {
    canonical_digits(token).then(|| token.parse().ok())?
}

fn canonical_i64(token: &str) -> Option<i64> {
    let digits = token.strip_prefix('-').unwrap_or(token);
    (canonical_digits(digits) && token != "-0").then(|| token.parse().ok())?
}

/// The finite `f64` that is the same decimal value as `token`, if any.
fn exact_f64(token: &str) -> Option<f64> {
    let value: f64 = token.parse().ok()?;
    (value.is_finite() && Exact::of(&format!("{value:e}")) == Exact::of(token)).then_some(value)
}

/// `minimum: 0` integer.
pub(super) fn uint(raw: &RawValue, schema: &'static str, field: &'static str) -> R<u64> {
    let token = numeric(raw, schema)?;
    let exact = Exact::of(token);
    if !exact.is_integral() || exact.negative {
        return Err(RoutingWireError::Shape(schema));
    }
    canonical_u64(token).ok_or(RoutingWireError::Refused(field))
}

/// Integer with no minimum.
pub(super) fn int(raw: &RawValue, schema: &'static str, field: &'static str) -> R<i64> {
    let token = numeric(raw, schema)?;
    if !Exact::of(token).is_integral() {
        return Err(RoutingWireError::Shape(schema));
    }
    canonical_i64(token).ok_or(RoutingWireError::Refused(field))
}

/// A JSON number held as a finite `f64`.
pub(super) fn float(raw: &RawValue, schema: &'static str, field: &'static str) -> R<f64> {
    exact_f64(numeric(raw, schema)?).ok_or(RoutingWireError::Refused(field))
}

/// A number in `[0, 1]`, range-checked on the exact value first.
pub(super) fn unit_float(raw: &RawValue, schema: &'static str, field: &'static str) -> R<f64> {
    let exact = Exact::of(numeric(raw, schema)?);
    if exact.negative || exact.exceeds_one() {
        return Err(RoutingWireError::Shape(schema));
    }
    float(raw, schema, field)
}

/// Absent / null / present, with `f` converting the present value.
pub(super) fn lift<T>(
    value: Option<Option<Box<RawValue>>>,
    f: impl FnOnce(&RawValue) -> R<T>,
) -> R<Option<Option<T>>> {
    match value {
        None => Ok(None),
        Some(None) => Ok(Some(None)),
        Some(Some(raw)) => f(&raw).map(|v| Some(Some(v))),
    }
}

/// An open (any-JSON) value: every number inside must be exactly held.
pub(super) fn open(raw: &RawValue, field: &'static str) -> R<Value> {
    let text = raw.get();
    let held = |t: &str| {
        canonical_u64(t).is_some() || canonical_i64(t).is_some() || exact_f64(t).is_some()
    };
    if !tokens(text).all(held) {
        return Err(RoutingWireError::Refused(field));
    }
    serde_json::from_str(text).map_err(|_| RoutingWireError::Refused(field))
}

/// The number tokens of a well-formed JSON document, outside strings.
fn tokens(json: &str) -> impl Iterator<Item = &str> {
    let b = json.as_bytes();
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < b.len() {
            match b[i] {
                b'"' => {
                    i += 1;
                    while i < b.len() && b[i] != b'"' {
                        i += if b[i] == b'\\' { 2 } else { 1 };
                    }
                    i += 1;
                }
                b'-' | b'0'..=b'9' => {
                    let start = i;
                    while i < b.len()
                        && matches!(b[i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                    {
                        i += 1;
                    }
                    return Some(&json[start..i]);
                }
                _ => i += 1,
            }
        }
        None
    })
}
