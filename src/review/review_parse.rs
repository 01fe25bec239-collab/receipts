//! Strict reviewer-output parsing. Anything missing, truncated, ambiguous,
//! schema-invalid, bound to another subject or internally inconsistent is a
//! [`ReviewParseError`] — never a PASS.
//!
//! Accepted shapes: the whole trimmed text is one A4Review JSON object, or
//! the text contains exactly one ```` ```json ```` fenced block holding it.

use crate::ports::ReviewerRunOutput;
use crate::wire::{ReviewWireError, decode_a4_review};
use crate::{A4Review, A4ReviewDimension, A4ReviewVerdict, ReviewCapsuleNonTemporalCore};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewParseError {
    MissingOutput,
    TruncatedOutput,
    /// Zero or several candidate JSON documents.
    NotExactlyOneDocument,
    Wire(ReviewWireError),
    /// The document names another review, task or SHA.
    SubjectMismatch(&'static str),
    /// The reviewer itself reported independence as false (voids the review).
    IndependenceDenied,
    /// Verdict and findings contradict each other.
    VerdictInconsistent(&'static str),
    /// The frozen lifecycle requires every audit dimension to be assessed
    /// (NOT_APPLICABLE where appropriate); these were not.
    IncompleteDimensions(String),
}

fn document(text: &str) -> Result<&str, ReviewParseError> {
    let trimmed = text.trim();
    if trimmed.starts_with('{') {
        return Ok(trimmed);
    }
    let mut blocks = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("```json") {
        let body = &rest[start + "```json".len()..];
        let Some(end) = body.find("```") else {
            return Err(ReviewParseError::NotExactlyOneDocument);
        };
        blocks.push(body[..end].trim());
        rest = &body[end + 3..];
    }
    match blocks.as_slice() {
        [one] => Ok(one),
        _ => Err(ReviewParseError::NotExactlyOneDocument),
    }
}

/// Parses and binds the reviewer's A4Review to `capsule`.
pub fn parse_reviewer_output(
    output: &ReviewerRunOutput,
    capsule: &ReviewCapsuleNonTemporalCore,
) -> Result<A4Review, ReviewParseError> {
    use ReviewParseError as E;
    if output.truncated {
        return Err(E::TruncatedOutput);
    }
    let text = output.text.as_deref().ok_or(E::MissingOutput)?;
    let review = decode_a4_review(document(text)?).map_err(E::Wire)?;
    let core = review.core();
    if core.review_id() != capsule.review_id() {
        return Err(E::SubjectMismatch("review_id"));
    }
    if core.task_id() != capsule.task_id() {
        return Err(E::SubjectMismatch("task_id"));
    }
    if core.reviewed_sha() != capsule.implementation_sha().as_str() {
        return Err(E::SubjectMismatch("reviewed_sha"));
    }
    if !core.independence_attested() {
        return Err(E::IndependenceDenied);
    }
    if core.blocking_findings().iter().any(|f| !f.blocking()) {
        return Err(E::VerdictInconsistent(
            "blocking_findings entry with blocking=false",
        ));
    }
    if core.nonblocking_findings().iter().any(|f| f.blocking()) {
        return Err(E::VerdictInconsistent(
            "nonblocking_findings entry with blocking=true",
        ));
    }
    // Operational policy (not schema admission, which stays permissive in
    // `wire`): PASS carries no findings and no verdict other than REJECT
    // carries blocking ones, matching the exact-SHA foundation. A REJECT is
    // retained as a REJECT whatever its finding arrays hold; it can never
    // become acceptance because they are empty.
    match core.verdict() {
        A4ReviewVerdict::Pass if !core.nonblocking_findings().is_empty() => {
            return Err(E::VerdictInconsistent("PASS with findings"));
        }
        A4ReviewVerdict::Pass | A4ReviewVerdict::PassWithNonblockingFindings
            if !core.blocking_findings().is_empty() =>
        {
            return Err(E::VerdictInconsistent("PASS with blocking findings"));
        }
        _ => {}
    }
    let missing: Vec<&str> = A4ReviewDimension::ALL
        .into_iter()
        .filter(|d| !core.dimensions().iter().any(|r| r.dimension() == *d))
        .map(A4ReviewDimension::as_str)
        .collect();
    if !missing.is_empty() {
        return Err(E::IncompleteDimensions(missing.join(",")));
    }
    Ok(review)
}
