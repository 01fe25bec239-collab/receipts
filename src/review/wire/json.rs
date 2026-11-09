//! Minimal strict JSON reader/writer for the Review codecs.
//!
//! serde_json without `arbitrary_precision`/`raw_value` (not enabled on this
//! crate's own manifest) loses integer tokens beyond `f64` and keeps the last
//! of duplicate keys. This reader keeps every number as its exact token,
//! refuses duplicate keys at any depth, refuses lone surrogates and trailing
//! data, and bounds nesting depth. It is a decoder for Review wire data only.

use std::fmt::Write;

/// Maximum nesting depth; Review records are shallow.
pub(crate) const MAX_DEPTH: usize = 64;
/// Maximum canonical integer length a codec will materialise.
pub(crate) const MAX_INTEGER_DIGITS: usize = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Json {
    Null,
    Bool(bool),
    /// Exact number token as written.
    Number(String),
    String(String),
    Array(Vec<Json>),
    /// Members in document order; keys are unique.
    Object(Vec<(String, Json)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SyntaxError {
    Invalid,
    DuplicateKey,
    TooDeep,
}

pub(crate) fn parse(text: &str) -> Result<Json, SyntaxError> {
    let mut p = Parser {
        b: text.as_bytes(),
        i: 0,
    };
    p.ws();
    let v = p.value(0)?;
    p.ws();
    if p.i != p.b.len() {
        return Err(SyntaxError::Invalid);
    }
    Ok(v)
}

struct Parser<'a> {
    b: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while matches!(self.b.get(self.i), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }
    fn eat(&mut self, c: u8) -> Result<(), SyntaxError> {
        if self.b.get(self.i) == Some(&c) {
            self.i += 1;
            Ok(())
        } else {
            Err(SyntaxError::Invalid)
        }
    }
    fn lit(&mut self, s: &[u8], v: Json) -> Result<Json, SyntaxError> {
        if self.b[self.i..].starts_with(s) {
            self.i += s.len();
            Ok(v)
        } else {
            Err(SyntaxError::Invalid)
        }
    }
    fn value(&mut self, depth: usize) -> Result<Json, SyntaxError> {
        if depth > MAX_DEPTH {
            return Err(SyntaxError::TooDeep);
        }
        match self.b.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut members: Vec<(String, Json)> = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Object(members));
                }
                loop {
                    self.ws();
                    let key = self.string()?;
                    if members.iter().any(|(k, _)| *k == key) {
                        return Err(SyntaxError::DuplicateKey);
                    }
                    self.ws();
                    self.eat(b':')?;
                    self.ws();
                    let v = self.value(depth + 1)?;
                    members.push((key, v));
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Object(members));
                        }
                        _ => return Err(SyntaxError::Invalid),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.b.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Array(items));
                }
                loop {
                    self.ws();
                    items.push(self.value(depth + 1)?);
                    self.ws();
                    match self.b.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Array(items));
                        }
                        _ => return Err(SyntaxError::Invalid),
                    }
                }
            }
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.lit(b"true", Json::Bool(true)),
            Some(b'f') => self.lit(b"false", Json::Bool(false)),
            Some(b'n') => self.lit(b"null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(SyntaxError::Invalid),
        }
    }
    fn digits(&mut self) -> usize {
        let start = self.i;
        while matches!(self.b.get(self.i), Some(b'0'..=b'9')) {
            self.i += 1;
        }
        self.i - start
    }
    fn number(&mut self) -> Result<Json, SyntaxError> {
        let start = self.i;
        if self.b.get(self.i) == Some(&b'-') {
            self.i += 1;
        }
        match self.b.get(self.i) {
            Some(b'0') => self.i += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(SyntaxError::Invalid),
        }
        if self.b.get(self.i) == Some(&b'.') {
            self.i += 1;
            if self.digits() == 0 {
                return Err(SyntaxError::Invalid);
            }
        }
        if matches!(self.b.get(self.i), Some(b'e' | b'E')) {
            self.i += 1;
            if matches!(self.b.get(self.i), Some(b'+' | b'-')) {
                self.i += 1;
            }
            if self.digits() == 0 {
                return Err(SyntaxError::Invalid);
            }
        }
        // Token bytes are ASCII by construction.
        Ok(Json::Number(
            String::from_utf8_lossy(&self.b[start..self.i]).into_owned(),
        ))
    }
    fn hex4(&mut self) -> Result<u32, SyntaxError> {
        // JSON requires exactly four hexadecimal digits; `from_str_radix`
        // would also accept a leading `+`.
        let h = self.b.get(self.i..self.i + 4).ok_or(SyntaxError::Invalid)?;
        let mut v = 0u32;
        for b in h {
            v = (v << 4) | (*b as char).to_digit(16).ok_or(SyntaxError::Invalid)?;
        }
        self.i += 4;
        Ok(v)
    }
    fn string(&mut self) -> Result<String, SyntaxError> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let c = *self.b.get(self.i).ok_or(SyntaxError::Invalid)?;
            self.i += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|_| SyntaxError::Invalid),
                b'\\' => {
                    let e = *self.b.get(self.i).ok_or(SyntaxError::Invalid)?;
                    self.i += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                if self.b.get(self.i..self.i + 2) != Some(b"\\u") {
                                    return Err(SyntaxError::Invalid);
                                }
                                self.i += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(SyntaxError::Invalid);
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            char::from_u32(code).ok_or(SyntaxError::Invalid)?
                        }
                        _ => return Err(SyntaxError::Invalid),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                0x00..=0x1f => return Err(SyntaxError::Invalid),
                _ => out.push(c),
            }
        }
    }
}

/// Why a number token has no supported integer value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotInteger {
    Fraction,
    Oversized,
}

/// Exponents beyond this magnitude are saturated: they decide only zero /
/// fraction / oversized, never a digit count.
const EXPONENT_SATURATION: i128 = 1_000_000_000_000_000_000_000_000_000_000;

/// Canonical decimal digits of an integral number token (`1.0`, `1e3`, `-0`
/// are integers). Total over every grammatical token: zero is `"0"` under
/// any exponent, a nonzero value that is not an integer is `Fraction`, and
/// one beyond the carrier is `Oversized`. Never panics.
pub(crate) fn integer_digits(token: &str) -> Result<String, NotInteger> {
    use NotInteger::{Fraction, Oversized};
    let (negative, rest) = match token.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, token),
    };
    let (mantissa, exponent) = match rest.find(['e', 'E']) {
        Some(at) => (&rest[..at], saturating_exponent(&rest[at + 1..])?),
        None => (rest, 0),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let mut digits: String = format!("{int}{frac}");
    if digits.bytes().all(|b| b == b'0') {
        return Ok("0".into());
    }
    let exponent = exponent - frac.len() as i128;
    if exponent >= 0 {
        let zeros = usize::try_from(exponent).map_err(|_| Oversized)?;
        if zeros > MAX_INTEGER_DIGITS {
            return Err(Oversized);
        }
        digits.extend(std::iter::repeat_n('0', zeros));
    } else {
        let keep = usize::try_from(-exponent)
            .ok()
            .and_then(|cut| digits.len().checked_sub(cut));
        let (head, tail) = match keep {
            Some(k) => digits.split_at(k),
            None => ("", digits.as_str()),
        };
        if !tail.bytes().all(|b| b == b'0') {
            return Err(Fraction);
        }
        digits = head.to_owned();
    }
    let trimmed = digits.trim_start_matches('0');
    let magnitude = if trimmed.is_empty() { "0" } else { trimmed };
    if magnitude.len() > MAX_INTEGER_DIGITS {
        return Err(Oversized);
    }
    Ok(if negative && magnitude != "0" {
        format!("-{magnitude}")
    } else {
        magnitude.to_owned()
    })
}

/// `[+-]?digits` as a saturated signed value (grammar already validated).
fn saturating_exponent(text: &str) -> Result<i128, NotInteger> {
    let (negative, digits) = match text.as_bytes().first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut value: i128 = 0;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            return Err(NotInteger::Oversized);
        }
        value = (value * 10 + i128::from(b - b'0')).min(EXPONENT_SATURATION);
    }
    Ok(if negative { -value } else { value })
}

/// Canonical JSON writer: members are written in the order supplied.
#[derive(Default)]
pub(crate) struct Writer {
    out: String,
    first: Vec<bool>,
}

impl Writer {
    pub(crate) fn finish(self) -> String {
        self.out
    }
    fn sep(&mut self) {
        if let Some(first) = self.first.last_mut() {
            if !*first {
                self.out.push(',');
            }
            *first = false;
        }
    }
    pub(crate) fn begin_object(&mut self) {
        self.sep();
        self.out.push('{');
        self.first.push(true);
    }
    pub(crate) fn end_object(&mut self) {
        self.first.pop();
        self.out.push('}');
    }
    pub(crate) fn begin_array(&mut self) {
        self.sep();
        self.out.push('[');
        self.first.push(true);
    }
    pub(crate) fn end_array(&mut self) {
        self.first.pop();
        self.out.push(']');
    }
    /// Writes a key; the next value call writes its value without a comma.
    pub(crate) fn key(&mut self, key: &str) {
        self.sep();
        quote(&mut self.out, key);
        self.out.push(':');
        if let Some(first) = self.first.last_mut() {
            *first = true;
        }
    }
    fn after_value(&mut self) {
        if let Some(first) = self.first.last_mut() {
            *first = false;
        }
    }
    pub(crate) fn str(&mut self, v: &str) {
        self.sep();
        quote(&mut self.out, v);
        self.after_value();
    }
    pub(crate) fn bool(&mut self, v: bool) {
        self.sep();
        self.out.push_str(if v { "true" } else { "false" });
        self.after_value();
    }
    pub(crate) fn null(&mut self) {
        self.sep();
        self.out.push_str("null");
        self.after_value();
    }
    /// Writes canonical integer digits verbatim.
    pub(crate) fn raw_integer(&mut self, digits: &str) {
        self.sep();
        self.out.push_str(digits);
        self.after_value();
    }
    pub(crate) fn strs(&mut self, values: &[String]) {
        self.begin_array();
        for v in values {
            self.str(v);
        }
        self.end_array();
        self.after_value();
    }
}

fn quote(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
