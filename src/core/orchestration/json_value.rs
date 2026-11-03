//! Physical binding under USER-ADR-ORCHESTRATION-JSON-VALUE-PHYSICAL-V1-001.

use std::collections::BTreeMap;

/// An in-process JSON value. Strings are preserved exactly; arrays retain order
/// and duplicates. Equality is structural, including lexical number identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OrchestrationJsonValueV1 {
    Null,
    Boolean(bool),
    Number(OrchestrationJsonNumberV1),
    String(String),
    Array(Vec<OrchestrationJsonValueV1>),
    Object(OrchestrationJsonObjectV1),
}

/// Exact string-keyed mappings, independent of insertion history.
/// Duplicate keys cannot coexist. Construction accepts an existing map, never
/// an entry sequence; iteration follows BTreeMap's deterministic key order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct OrchestrationJsonObjectV1(BTreeMap<String, OrchestrationJsonValueV1>);

impl OrchestrationJsonObjectV1 {
    pub fn new(properties: BTreeMap<String, OrchestrationJsonValueV1>) -> Self {
        Self(properties)
    }

    pub fn as_map(&self) -> &BTreeMap<String, OrchestrationJsonValueV1> {
        &self.0
    }

    /// Ordinary mapping insertion: returns the previous value when replacing.
    pub fn insert(
        &mut self,
        key: String,
        value: OrchestrationJsonValueV1,
    ) -> Option<OrchestrationJsonValueV1> {
        self.0.insert(key, value)
    }
}

/// The supplied lexical value does not satisfy the JSON number grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrchestrationJsonNumberError;

impl std::fmt::Display for OrchestrationJsonNumberError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid Orchestration V1 JSON number")
    }
}

impl std::error::Error for OrchestrationJsonNumberError {}

/// A validated lexical JSON number, preserved byte-for-byte.
/// Grammar: `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`.
/// Equality and hashing use exact lexical identity; no numeric ordering,
/// conversion, normalization, or precision/range limits are imposed.
///
/// ```
/// use receipts_orchestration::orchestration::OrchestrationJsonNumberV1;
/// let number = OrchestrationJsonNumberV1::try_new("-0.00E+999999999999999999999")?;
/// assert_eq!(number.as_str(), "-0.00E+999999999999999999999");
/// # Ok::<(), receipts_orchestration::orchestration::OrchestrationJsonNumberError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrchestrationJsonNumberV1(String);

impl OrchestrationJsonNumberV1 {
    pub fn try_new(value: impl Into<String>) -> Result<Self, OrchestrationJsonNumberError> {
        let value = value.into();
        validate_number(value.as_bytes())?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_number(bytes: &[u8]) -> Result<(), OrchestrationJsonNumberError> {
    let mut position = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(position) {
        Some(b'0') => position += 1,
        Some(b'1'..=b'9') => {
            while bytes.get(position).is_some_and(u8::is_ascii_digit) {
                position += 1;
            }
        }
        _ => return Err(OrchestrationJsonNumberError),
    }
    if bytes.get(position) == Some(&b'.') {
        position += 1;
        consume_digits(bytes, &mut position)?;
    }
    if matches!(bytes.get(position), Some(b'e' | b'E')) {
        position += 1;
        if matches!(bytes.get(position), Some(b'+' | b'-')) {
            position += 1;
        }
        consume_digits(bytes, &mut position)?;
    }
    if position == bytes.len() {
        Ok(())
    } else {
        Err(OrchestrationJsonNumberError)
    }
}

fn consume_digits(bytes: &[u8], position: &mut usize) -> Result<(), OrchestrationJsonNumberError> {
    let start = *position;
    while bytes.get(*position).is_some_and(u8::is_ascii_digit) {
        *position += 1;
    }
    if *position == start {
        Err(OrchestrationJsonNumberError)
    } else {
        Ok(())
    }
}

/// Text was not one strict JSON value: invalid syntax, a lone surrogate,
/// a duplicate object key, nesting deeper than 128, or trailing input. The
/// error never echoes input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrchestrationJsonParseError;

impl std::fmt::Display for OrchestrationJsonParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid Orchestration V1 JSON text")
    }
}

impl std::error::Error for OrchestrationJsonParseError {}

const MAX_JSON_DEPTH: usize = 128;

impl OrchestrationJsonValueV1 {
    /// Parses strict JSON with serde's parser, keeping every number's exact
    /// lexeme (no f64 or machine-integer intermediate) and refusing duplicate
    /// keys. Surrounding JSON whitespace is allowed.
    pub fn parse_json(text: &str) -> Result<Self, OrchestrationJsonParseError> {
        let raw: Box<RawValue> =
            serde_json::from_str(text).map_err(|_| OrchestrationJsonParseError)?;
        from_raw(&raw, 0)
    }

    /// Compact JSON text. Numbers are written as their exact lexemes; object
    /// keys follow the map's deterministic order.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        write_json(self, &mut out);
        out
    }
}

use serde_json::value::RawValue;

fn from_raw(
    raw: &RawValue,
    depth: usize,
) -> Result<OrchestrationJsonValueV1, OrchestrationJsonParseError> {
    use OrchestrationJsonValueV1 as V;
    if depth > MAX_JSON_DEPTH {
        return Err(OrchestrationJsonParseError);
    }
    let text = raw.get();
    let parse_err = |_| OrchestrationJsonParseError;
    Ok(match text.as_bytes().first() {
        Some(b'{') => {
            let RawEntries(entries) = serde_json::from_str(text).map_err(parse_err)?;
            let mut map = BTreeMap::new();
            for (key, value) in entries {
                let value = from_raw(&value, depth + 1)?;
                if map.insert(key, value).is_some() {
                    return Err(OrchestrationJsonParseError);
                }
            }
            V::Object(OrchestrationJsonObjectV1(map))
        }
        Some(b'[') => {
            let items: Vec<Box<RawValue>> = serde_json::from_str(text).map_err(parse_err)?;
            V::Array(
                items
                    .iter()
                    .map(|item| from_raw(item, depth + 1))
                    .collect::<Result<_, _>>()?,
            )
        }
        Some(b'"') => V::String(serde_json::from_str(text).map_err(parse_err)?),
        Some(b't' | b'f') => V::Boolean(serde_json::from_str(text).map_err(parse_err)?),
        Some(b'n') => V::Null,
        _ => V::Number(
            OrchestrationJsonNumberV1::try_new(text).map_err(|_| OrchestrationJsonParseError)?,
        ),
    })
}

/// Object entries in source order, so duplicates stay observable.
struct RawEntries(Vec<(String, Box<RawValue>)>);

impl<'de> serde::Deserialize<'de> for RawEntries {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries;
        impl<'de> serde::de::Visitor<'de> for Entries {
            type Value = RawEntries;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<RawEntries, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(RawEntries(entries))
            }
        }
        deserializer.deserialize_map(Entries)
    }
}

fn write_json(value: &OrchestrationJsonValueV1, out: &mut String) {
    use OrchestrationJsonValueV1 as V;
    match value {
        V::Null => out.push_str("null"),
        V::Boolean(b) => out.push_str(if *b { "true" } else { "false" }),
        V::Number(n) => out.push_str(n.as_str()),
        V::String(s) => write_string(s, out),
        V::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_json(item, out);
            }
            out.push(']');
        }
        V::Object(object) => {
            out.push('{');
            for (i, (key, item)) in object.0.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_json(item, out);
            }
            out.push('}');
        }
    }
}

fn write_string(s: &str, out: &mut String) {
    out.push_str(&serde_json::to_string(s).expect("string serialization cannot fail"));
}
