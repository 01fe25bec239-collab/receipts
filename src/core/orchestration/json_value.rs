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
