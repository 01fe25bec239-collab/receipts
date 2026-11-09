//! Explicit dispositions for nonblocking findings (A3_A4_REPAIR_LOOP:
//! PASS_WITH_NONBLOCKING_FINDINGS). The reviewer cannot disposition its own
//! findings and the controller cannot invent a disposition, so a passing
//! verdict with nonblocking findings stays pending until a decision maker
//! supplies exactly one disposition per finding: fix now (a new bounded
//! task), defer (issue ID and revisit point) or reject (reason). This module
//! validates that set and produces one finding-specific FINDING_DISPOSITIONED
//! event per finding. It records nothing silently and drops nothing.

use crate::records::{ProposalBuilder, derived_id};
use crate::{A4ReviewNonTemporalCore, A4ReviewVerdict, ReviewRequest, ports::ReviewRecordProposal};
use receipts_state::{EventType, SubjectKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindingDispositionKind {
    /// Fix now, as a new bounded task.
    FixNow { task_id: String },
    /// Defer with an issue and a point at which it is revisited.
    Defer {
        issue_id: String,
        revisit_point: String,
    },
    /// Reject the finding, with a reason.
    Reject { reason: String },
}

impl FindingDispositionKind {
    const fn label(&self) -> &'static str {
        match self {
            Self::FixNow { .. } => "FIX_NOW",
            Self::Defer { .. } => "DEFER",
            Self::Reject { .. } => "REJECT",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FindingDisposition {
    pub finding_id: String,
    pub kind: FindingDispositionKind,
    /// Who decided (a role or identity; recorded verbatim).
    pub decided_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispositionRefusal {
    /// Only a PASS_WITH_NONBLOCKING_FINDINGS review is dispositioned here.
    NotApplicable(A4ReviewVerdict),
    /// Nonblocking findings without a disposition.
    Missing(Vec<String>),
    /// A disposition for a finding the review does not list as nonblocking.
    UnknownFinding(String),
    DuplicateFinding(String),
    /// A required field is empty (task id, issue id, revisit point, reason,
    /// decider).
    EmptyField(&'static str),
    Records(String),
}

/// Ids of the review's nonblocking findings that have no disposition.
pub fn undispositioned(
    review: &A4ReviewNonTemporalCore,
    dispositions: &[FindingDisposition],
) -> Vec<String> {
    review
        .nonblocking_findings()
        .iter()
        .map(|f| f.finding_id().to_owned())
        .filter(|id| !dispositions.iter().any(|d| d.finding_id == *id))
        .collect()
}

/// Checks that `dispositions` covers every nonblocking finding exactly once
/// with complete fields and mentions nothing else.
pub fn validate_dispositions(
    review: &A4ReviewNonTemporalCore,
    dispositions: &[FindingDisposition],
) -> Result<(), DispositionRefusal> {
    use DispositionRefusal as R;
    if review.verdict() != A4ReviewVerdict::PassWithNonblockingFindings {
        return Err(R::NotApplicable(review.verdict()));
    }
    let nonblocking: Vec<&str> = review
        .nonblocking_findings()
        .iter()
        .map(|f| f.finding_id())
        .collect();
    let mut seen: Vec<&str> = Vec::new();
    for d in dispositions {
        if !nonblocking.contains(&d.finding_id.as_str()) {
            return Err(R::UnknownFinding(d.finding_id.clone()));
        }
        if seen.contains(&d.finding_id.as_str()) {
            return Err(R::DuplicateFinding(d.finding_id.clone()));
        }
        seen.push(&d.finding_id);
        let (field, text) = match &d.kind {
            FindingDispositionKind::FixNow { task_id } => ("task_id", task_id.as_str()),
            FindingDispositionKind::Defer { issue_id, .. } if issue_id.trim().is_empty() => {
                ("issue_id", "")
            }
            FindingDispositionKind::Defer { revisit_point, .. } => {
                ("revisit_point", revisit_point.as_str())
            }
            FindingDispositionKind::Reject { reason } => ("reason", reason.as_str()),
        };
        if text.trim().is_empty() {
            return Err(R::EmptyField(field));
        }
        if d.decided_by.trim().is_empty() {
            return Err(R::EmptyField("decided_by"));
        }
    }
    let missing = undispositioned(review, dispositions);
    if !missing.is_empty() {
        return Err(R::Missing(missing));
    }
    Ok(())
}

/// Durable identity of one review's disposition set.
pub fn disposition_claim_key(project_id: &str, review_id: &str) -> String {
    derived_id("finding-disposition", &[project_id, review_id])
}

/// One FINDING_DISPOSITIONED event per finding, each with its own payload.
pub fn disposition_proposal(
    project_id: &str,
    request: &ReviewRequest,
    review: &A4ReviewNonTemporalCore,
    dispositions: &[FindingDisposition],
) -> Result<ReviewRecordProposal, DispositionRefusal> {
    validate_dispositions(review, dispositions)?;
    let core = request.core();
    let records = |e: String| DispositionRefusal::Records(e);
    let mut b = ProposalBuilder::new(
        project_id,
        core.task_id(),
        core.context_epoch().decimal_digits(),
        &disposition_claim_key(project_id, review.review_id()),
    )
    .map_err(records)?;
    for finding in review.nonblocking_findings() {
        let Some(d) = dispositions
            .iter()
            .find(|d| d.finding_id == finding.finding_id())
        else {
            continue;
        };
        let detail = match &d.kind {
            FindingDispositionKind::FixNow { task_id } => serde_json::json!({"task_id": task_id}),
            FindingDispositionKind::Defer {
                issue_id,
                revisit_point,
            } => serde_json::json!({"issue_id": issue_id, "revisit_point": revisit_point}),
            FindingDispositionKind::Reject { reason } => serde_json::json!({"reason": reason}),
        };
        let body = serde_json::json!({
            "review_id": review.review_id(),
            "task_id": review.task_id(),
            "finding_id": finding.finding_id(),
            "category": finding.category().as_str(),
            "disposition": d.kind.label(),
            "detail": detail,
            "decided_by": d.decided_by,
        })
        .to_string();
        let payload = b.content(
            &format!(
                "review:disposition:{}:{}",
                review.review_id(),
                finding.finding_id()
            ),
            body.as_bytes(),
        );
        b.event(
            EventType::FindingDispositioned,
            (SubjectKind::Review, review.review_id()),
            payload,
        )
        .map_err(records)?;
    }
    Ok(b.finish())
}
