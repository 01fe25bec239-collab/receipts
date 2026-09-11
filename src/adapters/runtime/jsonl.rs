//! Provider-neutral framing only. Lazy iteration preserves provider validation
//! order: an earlier shape error must precede a later malformed record.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JsonlErrorKind {
    InvalidUtf8,
    InvalidJson,
    IncompleteJson,
}

pub(crate) struct JsonlError {
    pub line: usize,
    pub kind: JsonlErrorKind,
}

pub(crate) struct JsonlRecord<'a> {
    pub line: usize,
    pub raw: &'a [u8],
    pub value: Value,
}

pub(crate) fn records(bytes: &[u8]) -> impl Iterator<Item = Result<JsonlRecord<'_>, JsonlError>> {
    bytes
        .split_inclusive(|b| *b == b'\n')
        .enumerate()
        .map(|(index, raw)| {
            let line = index + 1;
            let error = |kind| JsonlError { line, kind };
            let text = std::str::from_utf8(raw).map_err(|_| error(JsonlErrorKind::InvalidUtf8))?;
            let value = serde_json::from_str(text).map_err(|e| {
                error(if e.is_eof() {
                    JsonlErrorKind::IncompleteJson
                } else {
                    JsonlErrorKind::InvalidJson
                })
            })?;
            Ok(JsonlRecord { line, raw, value })
        })
}
