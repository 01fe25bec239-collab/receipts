//! Read-only admission of independent review evidence for apply-v1 verdicts.
//!
//! An aggregate verdict (`AWAITING_REVIEW → PASSED | REJECTED`) is not proven
//! by the worker's own result or by an opaque `review_id` it names. Trusted
//! core supplies a [`ReviewEvidenceSource`]; [`ReviewEvidenceVerifier`] checks
//! that the record the source holds for that review identity is bound to the
//! exact project, graph, subject node, admitted attempt, code SHA, round and
//! outcome being judged, was admitted by the source, and has the content the
//! source stores. Anything missing is *unavailable*, never assumed.
//!
//! No production source exists in Wave1: with none supplied every
//! evidence-backed verdict is refused as unavailable. This module is a pure
//! consumer/verifier. It dispatches no review, calls no service and issues no
//! authority; a file- or content-backed source proves only that the verifier
//! checks what it is given.

use sha2::{Digest, Sha256};

/// The trusted source could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewSourceUnavailable;

/// The verdict a review reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewOutcome {
    Pass,
    Rejected,
}

/// Admission as recorded by the trusted source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewProvenance {
    Admitted { admission_id: String },
    NotAdmitted,
}

/// What the trusted source records for one review identity. Its claims are
/// checked against the work being judged, not believed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewEvidenceRecord {
    pub project_id: String,
    pub graph_id: String,
    /// The fresh review identity.
    pub review_id: String,
    pub subject_node_id: String,
    /// The subject attempt that was reviewed.
    pub subject_attempt_id: String,
    /// The reviewed commit; absent for a non-code subject.
    pub code_sha: Option<String>,
    /// The reviewed round (the subject's `attempt_number` when it has one).
    pub round: Option<String>,
    pub outcome: ReviewOutcome,
    pub content_sha256: String,
    pub provenance: ReviewProvenance,
}

/// Read-only port to the trusted review-evidence store.
pub trait ReviewEvidenceSource {
    fn review_record(
        &self,
        project_id: &str,
        graph_id: &str,
        review_id: &str,
    ) -> Result<Option<ReviewEvidenceRecord>, ReviewSourceUnavailable>;
    fn review_content(&self, review_id: &str) -> Result<Option<Vec<u8>>, ReviewSourceUnavailable>;
}

/// Why review evidence was not admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewEvidenceRefusal {
    /// No trusted source was supplied: unavailable, never true.
    NoSource,
    SourceUnavailable,
    MissingRecord,
    WrongProject,
    WrongGraph,
    WrongReview,
    WrongSubject,
    /// The record reviewed another attempt (an earlier round, or another run).
    StaleAttempt,
    CodeMismatch,
    RoundMismatch,
    OutcomeMismatch,
    NotAdmitted,
    /// The review identity is the subject's own node or attempt identity.
    SelfReview,
    MissingContent,
    ContentMismatch,
}

impl ReviewEvidenceRefusal {
    /// Whether the evidence is merely unavailable rather than contradicted.
    pub fn is_unavailable(self) -> bool {
        matches!(
            self,
            Self::NoSource | Self::SourceUnavailable | Self::MissingRecord | Self::MissingContent
        )
    }
}

/// The exact work a review must have judged.
#[derive(Debug, Clone, Copy)]
pub struct ReviewedWork<'a> {
    pub project_id: &'a str,
    pub graph_id: &'a str,
    pub review_id: &'a str,
    pub node_id: &'a str,
    pub attempt_id: &'a str,
    pub code_sha: Option<&'a str>,
    pub round: Option<&'a str>,
    pub expected: ReviewOutcome,
}

/// Admitted evidence; only [`ReviewEvidenceVerifier::verify`] creates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedReviewEvidence {
    review_id: String,
    outcome: ReviewOutcome,
    code_sha: Option<String>,
    round: Option<String>,
    content_sha256: String,
    admission_id: String,
}

impl AdmittedReviewEvidence {
    pub fn review_id(&self) -> &str {
        &self.review_id
    }
    pub fn outcome(&self) -> ReviewOutcome {
        self.outcome
    }
    pub fn code_sha(&self) -> Option<&str> {
        self.code_sha.as_deref()
    }
    pub fn round(&self) -> Option<&str> {
        self.round.as_deref()
    }
    pub fn content_sha256(&self) -> &str {
        &self.content_sha256
    }
    pub fn admission_id(&self) -> &str {
        &self.admission_id
    }
}

/// Admits review evidence from a trusted read-only source. Trusted core
/// builds it; a worker's result, role or `review_id` cannot.
#[derive(Clone, Copy)]
pub struct ReviewEvidenceVerifier<'s> {
    source: &'s dyn ReviewEvidenceSource,
}

impl<'s> ReviewEvidenceVerifier<'s> {
    pub fn new(source: &'s dyn ReviewEvidenceSource) -> Self {
        Self { source }
    }

    /// Checks every binding of the source record against `work`, then
    /// recomputes the content SHA-256 from the source bytes.
    pub fn verify(
        &self,
        work: &ReviewedWork<'_>,
    ) -> Result<AdmittedReviewEvidence, ReviewEvidenceRefusal> {
        use ReviewEvidenceRefusal as R;
        let record = self
            .source
            .review_record(work.project_id, work.graph_id, work.review_id)
            .map_err(|_| R::SourceUnavailable)?
            .ok_or(R::MissingRecord)?;
        let checks = [
            (record.project_id == work.project_id, R::WrongProject),
            (record.graph_id == work.graph_id, R::WrongGraph),
            (record.review_id == work.review_id, R::WrongReview),
            (
                work.review_id != work.node_id && work.review_id != work.attempt_id,
                R::SelfReview,
            ),
            (record.subject_node_id == work.node_id, R::WrongSubject),
            (
                record.subject_attempt_id == work.attempt_id,
                R::StaleAttempt,
            ),
            (record.code_sha.as_deref() == work.code_sha, R::CodeMismatch),
            (record.round.as_deref() == work.round, R::RoundMismatch),
            (record.outcome == work.expected, R::OutcomeMismatch),
        ];
        if let Some((_, refusal)) = checks.into_iter().find(|(ok, _)| !ok) {
            return Err(refusal);
        }
        let ReviewProvenance::Admitted { admission_id } = &record.provenance else {
            return Err(R::NotAdmitted);
        };
        if admission_id.is_empty() {
            return Err(R::NotAdmitted);
        }
        let content = self
            .source
            .review_content(&record.review_id)
            .map_err(|_| R::SourceUnavailable)?
            .ok_or(R::MissingContent)?;
        let actual: String = Sha256::digest(&content)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != record.content_sha256 {
            return Err(R::ContentMismatch);
        }
        Ok(AdmittedReviewEvidence {
            review_id: record.review_id,
            outcome: record.outcome,
            code_sha: record.code_sha,
            round: record.round,
            content_sha256: actual,
            admission_id: admission_id.clone(),
        })
    }
}
