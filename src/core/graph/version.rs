//! Graph-owned physical binding: BUILD-A1-ADR-ORCHESTRATION-GRAPH-VERSION-PHYSICAL-V1-001.

/// A canonical positive integer with no numeric or digit-count ceiling.
/// The authoritative value is lexical: no conversion, normalization, ordering,
/// or arithmetic is performed.
///
/// Raw construction cannot bypass validation:
/// ```compile_fail
/// use receipts_orchestration::GraphVersionV1;
/// let invalid = GraphVersionV1(String::from("0"));
/// ```
/// Access cannot mutate the accepted representation:
/// ```compile_fail
/// use receipts_orchestration::GraphVersionV1;
/// let mut version = GraphVersionV1::try_new("1").unwrap();
/// version.as_str().clear();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphVersionV1(String);

/// Input was not canonical positive ASCII decimal (`[1-9][0-9]*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphVersionError;

impl std::fmt::Display for GraphVersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("expected a canonical positive ASCII decimal graph version")
    }
}

impl std::error::Error for GraphVersionError {}

impl GraphVersionV1 {
    pub fn try_new(value: impl Into<String>) -> Result<Self, GraphVersionError> {
        let value = value.into();
        if !matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
            || !value.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(GraphVersionError);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}
