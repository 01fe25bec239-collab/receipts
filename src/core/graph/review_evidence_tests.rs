//! The review-evidence verifier against a retained file/content-backed
//! source: valid evidence and every mismatch, tamper, stale and old-round
//! control. The source is a directory of records and content files, so the
//! verifier is exercised on bytes it did not make up.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::review_evidence::{
    ReviewEvidenceRecord, ReviewEvidenceRefusal as R, ReviewEvidenceSource, ReviewEvidenceVerifier,
    ReviewOutcome, ReviewProvenance, ReviewSourceUnavailable, ReviewedWork,
};

pub(crate) const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// One review as the test store records it. Empty `code_sha`, `round` and
/// `admitted` mean absent.
#[derive(Clone)]
pub(crate) struct Rec {
    pub project: String,
    pub graph: String,
    pub review_id: String,
    pub subject: String,
    pub attempt: String,
    pub code_sha: String,
    pub round: String,
    pub outcome: String,
    pub admitted: String,
    pub content: Vec<u8>,
}

impl Rec {
    pub fn new(review_id: &str, subject: &str, attempt: &str, code_sha: &str, round: &str) -> Self {
        Self {
            project: "P".into(),
            graph: "g".into(),
            review_id: review_id.into(),
            subject: subject.into(),
            attempt: attempt.into(),
            code_sha: code_sha.into(),
            round: round.into(),
            outcome: "PASS".into(),
            admitted: "adm-1".into(),
            content: format!("independent review {review_id}: no blocking findings\n").into_bytes(),
        }
    }
    pub fn rejected(mut self) -> Self {
        self.outcome = "REJECTED".into();
        self
    }
}

/// A directory source: records are `key=value` lines in
/// `<dir>/records/<project>/<graph>/<review_id>`, content in
/// `<dir>/content/<review_id>`.
pub(crate) struct ReviewDir(PathBuf);

impl ReviewDir {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("or-review-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("content")).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    /// Stores the record and its content; the recorded hash is the content's.
    pub fn put(&self, rec: &Rec) {
        let hash = sha256_hex(&rec.content);
        self.put_with_hash(rec, &hash);
    }

    pub fn put_with_hash(&self, rec: &Rec, content_sha256: &str) {
        self.put_at(rec, &rec.project, &rec.graph, content_sha256);
    }

    /// Files the record under `file_project`/`file_graph` whatever it says
    /// about itself, so a lookup can find a record that names another scope.
    pub fn put_at(&self, rec: &Rec, file_project: &str, file_graph: &str, content_sha256: &str) {
        let dir = self.0.join("records").join(file_project).join(file_graph);
        std::fs::create_dir_all(&dir).unwrap();
        let text = format!(
            "project={}\ngraph={}\nreview_id={}\nsubject={}\nattempt={}\ncode_sha={}\nround={}\noutcome={}\ncontent_sha256={content_sha256}\nadmitted={}\n",
            rec.project,
            rec.graph,
            rec.review_id,
            rec.subject,
            rec.attempt,
            rec.code_sha,
            rec.round,
            rec.outcome,
            rec.admitted,
        );
        std::fs::write(dir.join(&rec.review_id), text).unwrap();
        std::fs::write(self.0.join("content").join(&rec.review_id), &rec.content).unwrap();
    }

    /// Changes the stored content after the record was written.
    pub fn tamper(&self, review_id: &str, content: &[u8]) {
        std::fs::write(self.0.join("content").join(review_id), content).unwrap();
    }
}

impl Drop for ReviewDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl ReviewEvidenceSource for ReviewDir {
    fn review_record(
        &self,
        project_id: &str,
        graph_id: &str,
        review_id: &str,
    ) -> Result<Option<ReviewEvidenceRecord>, ReviewSourceUnavailable> {
        let path = self
            .0
            .join("records")
            .join(project_id)
            .join(graph_id)
            .join(review_id);
        let Ok(text) = std::fs::read_to_string(path) else {
            return Ok(None);
        };
        let get = |k: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("{k}=")))
                .unwrap_or("")
                .to_owned()
        };
        let optional = |k: &str| Some(get(k)).filter(|v| !v.is_empty());
        Ok(Some(ReviewEvidenceRecord {
            project_id: get("project"),
            graph_id: get("graph"),
            review_id: get("review_id"),
            subject_node_id: get("subject"),
            subject_attempt_id: get("attempt"),
            code_sha: optional("code_sha"),
            round: optional("round"),
            outcome: match get("outcome").as_str() {
                "PASS" => ReviewOutcome::Pass,
                _ => ReviewOutcome::Rejected,
            },
            content_sha256: get("content_sha256"),
            provenance: match optional("admitted") {
                Some(admission_id) => ReviewProvenance::Admitted { admission_id },
                None => ReviewProvenance::NotAdmitted,
            },
        }))
    }

    fn review_content(&self, review_id: &str) -> Result<Option<Vec<u8>>, ReviewSourceUnavailable> {
        Ok(std::fs::read(self.0.join("content").join(review_id)).ok())
    }
}

pub(crate) struct Offline;

impl ReviewEvidenceSource for Offline {
    fn review_record(
        &self,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<Option<ReviewEvidenceRecord>, ReviewSourceUnavailable> {
        Err(ReviewSourceUnavailable)
    }
    fn review_content(&self, _: &str) -> Result<Option<Vec<u8>>, ReviewSourceUnavailable> {
        Err(ReviewSourceUnavailable)
    }
}

fn work<'a>(review_id: &'a str, expected: ReviewOutcome) -> ReviewedWork<'a> {
    ReviewedWork {
        project_id: "P",
        graph_id: "g",
        review_id,
        node_id: "t",
        attempt_id: "at2",
        code_sha: Some(SHA),
        round: Some("2"),
        expected,
    }
}

#[test]
fn valid_file_backed_evidence_is_admitted_with_its_bindings() {
    let dir = ReviewDir::new("valid");
    dir.put(&Rec::new("rv-2", "t", "at2", SHA, "2"));
    let admitted = ReviewEvidenceVerifier::new(&dir)
        .verify(&work("rv-2", ReviewOutcome::Pass))
        .unwrap();
    assert_eq!(admitted.review_id(), "rv-2");
    assert_eq!(admitted.outcome(), ReviewOutcome::Pass);
    assert_eq!(admitted.code_sha(), Some(SHA));
    assert_eq!(admitted.round(), Some("2"));
    assert_eq!(admitted.admission_id(), "adm-1");
    assert_eq!(
        admitted.content_sha256(),
        sha256_hex(b"independent review rv-2: no blocking findings\n")
    );
    // A rejected review is equally valid evidence for a REJECTED verdict.
    dir.put(&Rec::new("rv-3", "t", "at2", SHA, "2").rejected());
    assert!(
        ReviewEvidenceVerifier::new(&dir)
            .verify(&work("rv-3", ReviewOutcome::Rejected))
            .is_ok()
    );
    // A non-code subject carries no SHA and no round.
    dir.put(&Rec::new("rv-4", "t", "at2", "", ""));
    let bare = ReviewedWork {
        code_sha: None,
        round: None,
        ..work("rv-4", ReviewOutcome::Pass)
    };
    assert!(ReviewEvidenceVerifier::new(&dir).verify(&bare).is_ok());
}

#[test]
fn every_mismatched_binding_is_refused_for_its_own_reason() {
    let dir = ReviewDir::new("mismatch");
    let base = Rec::new("rv", "t", "at2", SHA, "2");
    for (label, rec, expect) in [
        (
            "record names another project",
            Rec {
                project: "other".into(),
                ..base.clone()
            },
            R::WrongProject,
        ),
        (
            "record names another graph",
            Rec {
                graph: "h".into(),
                ..base.clone()
            },
            R::WrongGraph,
        ),
        (
            "record is for another subject",
            Rec {
                subject: "u".into(),
                ..base.clone()
            },
            R::WrongSubject,
        ),
        (
            "old round: an earlier attempt of the same node",
            Rec {
                attempt: "at1".into(),
                ..base.clone()
            },
            R::StaleAttempt,
        ),
        (
            "another commit",
            Rec {
                code_sha: "f".repeat(40),
                ..base.clone()
            },
            R::CodeMismatch,
        ),
        (
            "no commit although the work is code-bearing",
            Rec {
                code_sha: String::new(),
                ..base.clone()
            },
            R::CodeMismatch,
        ),
        (
            "old round number",
            Rec {
                round: "1".into(),
                ..base.clone()
            },
            R::RoundMismatch,
        ),
        (
            "no round although the subject has one",
            Rec {
                round: String::new(),
                ..base.clone()
            },
            R::RoundMismatch,
        ),
        (
            "rejected review used for a PASSED verdict",
            base.clone().rejected(),
            R::OutcomeMismatch,
        ),
        (
            "not admitted by the source",
            Rec {
                admitted: String::new(),
                ..base.clone()
            },
            R::NotAdmitted,
        ),
    ] {
        // Filed where the lookup finds it, whatever scope it names.
        dir.put_at(&rec, "P", "g", &sha256_hex(&rec.content));
        assert_eq!(
            ReviewEvidenceVerifier::new(&dir).verify(&work("rv", ReviewOutcome::Pass)),
            Err(expect),
            "{label}"
        );
    }
}

#[test]
fn a_review_identity_cannot_be_the_work_it_judges() {
    let dir = ReviewDir::new("self");
    for review_id in ["t", "at2"] {
        dir.put(&Rec::new(review_id, "t", "at2", SHA, "2"));
        assert_eq!(
            ReviewEvidenceVerifier::new(&dir).verify(&work(review_id, ReviewOutcome::Pass)),
            Err(R::SelfReview),
            "{review_id}"
        );
    }
}

#[test]
fn absent_tampered_and_offline_evidence_is_never_admitted() {
    let dir = ReviewDir::new("tamper");
    let verifier = ReviewEvidenceVerifier::new(&dir);
    // No record: unavailable.
    let refusal = verifier
        .verify(&work("missing", ReviewOutcome::Pass))
        .unwrap_err();
    assert_eq!(refusal, R::MissingRecord);
    assert!(refusal.is_unavailable());
    // Record without its content: unavailable.
    let rec = Rec::new("rv", "t", "at2", SHA, "2");
    dir.put(&rec);
    std::fs::remove_file(dir.path().join("content/rv")).unwrap();
    let refusal = verifier
        .verify(&work("rv", ReviewOutcome::Pass))
        .unwrap_err();
    assert_eq!(refusal, R::MissingContent);
    assert!(refusal.is_unavailable());
    // Content changed after the record: contradicted, not merely unavailable.
    dir.put(&rec);
    dir.tamper("rv", b"independent review rv: approved by the author\n");
    let refusal = verifier
        .verify(&work("rv", ReviewOutcome::Pass))
        .unwrap_err();
    assert_eq!(refusal, R::ContentMismatch);
    assert!(!refusal.is_unavailable());
    // A recorded hash that does not match intact content.
    dir.put_with_hash(&rec, &"0".repeat(64));
    assert_eq!(
        verifier.verify(&work("rv", ReviewOutcome::Pass)),
        Err(R::ContentMismatch)
    );
    // An unreachable source is unavailable.
    let refusal = ReviewEvidenceVerifier::new(&Offline)
        .verify(&work("rv", ReviewOutcome::Pass))
        .unwrap_err();
    assert_eq!(refusal, R::SourceUnavailable);
    assert!(refusal.is_unavailable());
    assert!(R::NoSource.is_unavailable());
}
