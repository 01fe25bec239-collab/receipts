//! Typed scope of one durable claim, and its binding to the opaque claim key.
//!
//! Every unit of Review work that reaches the trusted core is one claim. Its
//! identity is explicit and typed, never parsed back out of a hash:
//!
//! * [`ReviewClaimScope`] is project, task, optional subject SHA, request
//!   SHA-256 and a positive generation: exactly what the core needs to name the
//!   claim. The request SHA-256 covers the actual validated semantic request
//!   under a deterministic encoding and the [`ReviewOperation`], so a review, a
//!   repair decision, a disposition set and an acceptance decision for the same
//!   task and SHA are different claims.
//! * [`ReviewClaimBinding`] adds the operation and the opaque claim key, which
//!   is derived from the scope and operation only. A binding rebuilt from
//!   sealed bytes is accepted only if its key matches (`from_parts`).
//! * The generation comes from the trusted composition. Nothing here
//!   increments it; a new generation is the explicit way to run abandoned
//!   work again.

use crate::records::{derived_id, sha256_hex};
use crate::wire::{encode_review_request, encode_safety_interruption};
use crate::{ReviewCapsuleCheck, ReviewRequest, SafetyInterruption};
use serde::{Deserialize, Serialize};

/// What kind of Review work a claim covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewOperation {
    Review,
    Repair,
    Disposition,
    Acceptance,
}

impl ReviewOperation {
    pub const ALL: [Self; 4] = [
        Self::Review,
        Self::Repair,
        Self::Disposition,
        Self::Acceptance,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Review => "REVIEW",
            Self::Repair => "REPAIR",
            Self::Disposition => "DISPOSITION",
            Self::Acceptance => "ACCEPTANCE",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.as_str() == text)
    }

    /// The terminal outcome labels a sealed package of this operation may
    /// carry. Anything else is a sealed package that does not belong to the
    /// claim's operation.
    pub const fn outcomes(self) -> &'static [&'static str] {
        match self {
            Self::Review => &["REVIEWED", "INVALID_OUTPUT", "POLICY_BLOCKED", "REFUSED"],
            Self::Repair => &["REPAIR_ISSUED", "REPAIR_ESCALATED"],
            Self::Disposition => &["DISPOSITIONS"],
            Self::Acceptance => &["ACCEPTANCE_EVALUATED"],
        }
    }
}

fn bounded(value: &str) -> bool {
    (1..=200).contains(&value.chars().count()) && !value.chars().any(char::is_control)
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The explicit identity of one unit of work (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewClaimScope {
    project_id: String,
    task_id: String,
    subject_sha: Option<String>,
    request_sha256: String,
    generation: u32,
}

impl ReviewClaimScope {
    /// `subject_sha` is a lowercase 40-hex SHA when present, `request_sha256`
    /// a lowercase 64-hex digest (see [`request_sha256`]) and `generation` is
    /// at least 1. Ids are 1..=200 characters without control characters.
    pub fn new(
        project_id: &str,
        task_id: &str,
        subject_sha: Option<&str>,
        request_sha256: &str,
        generation: u32,
    ) -> Result<Self, String> {
        if !bounded(project_id) || !bounded(task_id) {
            return Err(
                "claim scope: project and task ids must be 1..=200 plain characters".into(),
            );
        }
        if subject_sha.is_some_and(|s| !lower_hex(s, 40)) {
            return Err("claim scope: subject SHA must be 40 lowercase hex digits".into());
        }
        if !lower_hex(request_sha256, 64) {
            return Err("claim scope: request digest must be 64 lowercase hex digits".into());
        }
        if generation == 0 {
            return Err("claim scope: generation starts at 1".into());
        }
        Ok(Self {
            project_id: project_id.to_owned(),
            task_id: task_id.to_owned(),
            subject_sha: subject_sha.map(str::to_owned),
            request_sha256: request_sha256.to_owned(),
            generation,
        })
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn subject_sha(&self) -> Option<&str> {
        self.subject_sha.as_deref()
    }
    pub fn request_sha256(&self) -> &str {
        &self.request_sha256
    }
    pub fn generation(&self) -> u32 {
        self.generation
    }
}

/// A scope, the operation it was digested for and the opaque claim key derived
/// from both. Fields are private: the key cannot drift from the scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewClaimBinding {
    operation: ReviewOperation,
    scope: ReviewClaimScope,
    key: String,
}

impl ReviewClaimBinding {
    pub fn new(operation: ReviewOperation, scope: ReviewClaimScope) -> Self {
        let key = derive_key(operation, &scope);
        Self {
            operation,
            scope,
            key,
        }
    }

    /// Rebuilds a binding from stored parts, refusing a key that is not the
    /// one the scope and operation derive.
    pub fn from_parts(
        operation: ReviewOperation,
        scope: ReviewClaimScope,
        key: &str,
    ) -> Result<Self, String> {
        let binding = Self::new(operation, scope);
        if binding.key != key {
            return Err("claim key does not match its scope and operation".into());
        }
        Ok(binding)
    }

    pub fn operation(&self) -> ReviewOperation {
        self.operation
    }
    pub fn scope(&self) -> &ReviewClaimScope {
        &self.scope
    }
    /// The opaque claim key. Identity, not data: never parse it.
    pub fn key(&self) -> &str {
        &self.key
    }
}

fn derive_key(operation: ReviewOperation, scope: &ReviewClaimScope) -> String {
    let subject = scope
        .subject_sha
        .as_deref()
        .map_or_else(|| "none".to_owned(), |s| format!("sha:{s}"));
    derived_id(
        "review-claim",
        &[
            operation.as_str(),
            &scope.project_id,
            &scope.task_id,
            &subject,
            &scope.request_sha256,
            &scope.generation.to_string(),
        ],
    )
}

/// SHA-256 (lowercase hex) of the operation and its deterministic request
/// parts, length-prefixed and domain-separated.
pub fn request_sha256(operation: ReviewOperation, parts: &[&str]) -> String {
    let mut bytes = Vec::new();
    for part in [
        "receipts.review.request.v1",
        operation.as_str(),
        &parts.len().to_string(),
    ]
    .into_iter()
    .chain(parts.iter().copied())
    {
        bytes.extend_from_slice(&(part.len() as u64).to_be_bytes());
        bytes.extend_from_slice(part.as_bytes());
    }
    sha256_hex(&bytes)
}

/// The request's canonical encoding without `requested_at`, which says when
/// the request was made, not what it asks: a redelivered callback must name
/// the same work.
pub(crate) fn request_identity_json(request: &ReviewRequest) -> String {
    encode_review_request(&ReviewRequest::new(request.core().clone(), None))
}

pub(crate) fn check_identity_json(check: &ReviewCapsuleCheck) -> String {
    serde_json::json!({
        "source": check.source().as_str(),
        "command": check.command(),
        "exit_code": check.exit_code(),
        "code_sha": check.code_sha().as_str(),
        "timed_out": check.timed_out(),
        "output_ref": check.output_ref().map(|r| serde_json::json!([
            format!("{:?}", r.ref_type()), r.target(), r.digest(), r.section()
        ])),
        "result": check.result().map(|r| r.as_str()),
        "started_at": check.started_at().map(|t| t.as_str()),
        "finished_at": check.finished_at().map(|t| t.as_str()),
    })
    .to_string()
}

pub(crate) fn interruption_identity_json(interruption: &SafetyInterruption) -> String {
    encode_safety_interruption(interruption)
}

/// Persist the semantic request with the exact capsule; timestamps do not identify work.
pub(crate) fn review_dispatch_json(
    request: &ReviewRequest,
    capsule: &crate::ReviewCapsuleNonTemporalCore,
) -> String {
    serde_json::json!({
        "request": request_identity_json(request),
        "capsule": crate::wire::encode_review_capsule(capsule),
    })
    .to_string()
}
