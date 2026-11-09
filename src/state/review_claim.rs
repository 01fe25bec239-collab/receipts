//! Durable review claims, written BEFORE a review is routed or executed.
//!
//! A review callback may be repeated, concurrent or restarted. Each unit of
//! review work has one caller-derived `claim_id` (the Review owner derives it
//! deterministically from the attempt / review request, never at random per
//! callback). [`TrustedCoreStore::claim_review_v1`](crate::TrustedCoreStore::claim_review_v1)
//! atomically inserts the claim or reports the one that already exists:
//!
//! * [`ReviewClaimOutcomeV1::Claimed`] is the **only** result that permits
//!   execution, and it carries a non-`Clone` [`ReviewClaimTicketV1`] bound to
//!   the live store instance. After a restart no ticket exists for an
//!   earlier claim, so nothing can silently resume it.
//! * [`ReviewClaimOutcomeV1::Existing`] never permits execution. The record
//!   says why: still `CLAIMED` (unfinished or uncertain: a crash may have
//!   lost a provider response), `COMPLETED` (the recorded disposition),
//!   `REFUSED` (a retained terminal policy / safety refusal) or `ABANDONED`.
//!
//! A claim leaves `CLAIMED` exactly once (a schema trigger enforces it) and
//! there is no TTL, expiry or automatic reopening: an unfinished claim stays
//! unfinished until the claimant settles it or the trusted core records an
//! explicit [`ReviewClaimAbandonV1`] naming the human / policy authority. An
//! abandoned claim id stays terminal; deliberately running the work again
//! needs a new `claim_id` (for example a higher generation), so every
//! repeated provider call is a separately recorded, explicit decision.
//!
//! State stores what the Review owner defines. `outcome_kind` is an opaque
//! bounded token (for example `PASS`, `REJECT`, `POLICY_BLOCKED`); State
//! never interprets it.

use rusqlite::{Connection, OptionalExtension, params};

use crate::canonical_timestamp::CanonicalTimestampV1;
use crate::error::{GraphPhase as Phase, PublicationFailureCode as Code, StateError};
use crate::graph_repository as graph;
use crate::repository::SqliteStateRepository;
use crate::trusted_time::{self, TrustedClockV1};

const KIND: &str = "review_claim";

fn failure(code: Code) -> StateError {
    StateError::Publication { code, kind: KIND }
}

pub(crate) fn bounded_id(value: &str) -> bool {
    (1..=200).contains(&value.chars().count()) && !value.chars().any(char::is_control)
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn token(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 64
        && bytes.next().is_some_and(|b| b.is_ascii_uppercase())
        && bytes.all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

/// What State needs to claim one unit of review work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewClaimRequestV1 {
    project_id: String,
    claim_id: String,
    task_id: String,
    subject_sha: Option<String>,
    request_sha256: String,
}

impl ReviewClaimRequestV1 {
    /// `subject_sha` (when present) is the lowercase 40-hex SHA under review;
    /// `request_sha256` is the lowercase SHA-256 of the Review owner's
    /// request bytes. Reusing a `claim_id` with a different task, SHA or
    /// request digest is refused as a conflicting retry.
    pub fn new(
        project_id: &str,
        claim_id: &str,
        task_id: &str,
        subject_sha: Option<&str>,
        request_sha256: &str,
    ) -> Result<Self, StateError> {
        if !bounded_id(project_id)
            || !bounded_id(claim_id)
            || !bounded_id(task_id)
            || subject_sha.is_some_and(|s| !lower_hex(s, 40))
            || !lower_hex(request_sha256, 64)
        {
            return Err(failure(Code::InvalidShape));
        }
        Ok(Self {
            project_id: project_id.to_owned(),
            claim_id: claim_id.to_owned(),
            task_id: task_id.to_owned(),
            subject_sha: subject_sha.map(str::to_owned),
            request_sha256: request_sha256.to_owned(),
        })
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn claim_id(&self) -> &str {
        &self.claim_id
    }
}

/// Where a claim stands. Only `Claimed` is non-terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewClaimStateV1 {
    /// Claimed and not settled: unfinished / uncertain. Never re-executed
    /// automatically.
    Claimed,
    /// Finished with a recorded disposition.
    Completed,
    /// Finished with a retained terminal policy / safety refusal.
    Refused,
    /// Explicitly abandoned by the trusted core under a named authority.
    Abandoned,
}

impl ReviewClaimStateV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Claimed => "CLAIMED",
            Self::Completed => "COMPLETED",
            Self::Refused => "REFUSED",
            Self::Abandoned => "ABANDONED",
        }
    }
    fn parse(value: &str) -> Result<Self, StateError> {
        [
            Self::Claimed,
            Self::Completed,
            Self::Refused,
            Self::Abandoned,
        ]
        .into_iter()
        .find(|s| s.as_str() == value)
        .ok_or_else(|| failure(Code::CorruptStore))
    }
}

/// The durable claim row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewClaimRecordV1 {
    pub project_id: String,
    pub claim_id: String,
    pub task_id: String,
    pub subject_sha: Option<String>,
    pub request_sha256: String,
    pub state: ReviewClaimStateV1,
    /// The recorded disposition token (`None` while `Claimed`).
    pub outcome_kind: Option<String>,
    pub outcome_ref: Option<String>,
    pub outcome_sha256: Option<String>,
    /// The authority an `Abandoned` claim names.
    pub authority_ref: Option<String>,
    pub claimed_at: String,
    pub finished_at: Option<String>,
}

/// Proof that this live store instance just inserted the claim. Not `Clone`;
/// it neither survives a restart nor moves between stores.
#[derive(Debug)]
pub struct ReviewClaimTicketV1 {
    pub(crate) store_id: String,
    pub(crate) instance: u64,
    pub(crate) project_id: String,
    pub(crate) claim_id: String,
}

impl ReviewClaimTicketV1 {
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn claim_id(&self) -> &str {
        &self.claim_id
    }
}

/// The result of claiming. Only `Claimed` permits execution.
#[derive(Debug)]
pub enum ReviewClaimOutcomeV1 {
    Claimed(ReviewClaimTicketV1),
    Existing(ReviewClaimRecordV1),
}

/// How a claim finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewClaimResolutionV1 {
    Completed,
    Refused,
}

/// The disposition recorded when the claimant settles its claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewClaimSettlementV1 {
    resolution: ReviewClaimResolutionV1,
    outcome_kind: String,
    outcome_ref: Option<String>,
    outcome_sha256: Option<String>,
}

impl ReviewClaimSettlementV1 {
    fn new(
        resolution: ReviewClaimResolutionV1,
        outcome_kind: &str,
        outcome_ref: Option<&str>,
        outcome_sha256: Option<&str>,
    ) -> Result<Self, StateError> {
        if !token(outcome_kind)
            || outcome_ref.is_some_and(|r| !bounded_id(r))
            || outcome_sha256.is_some_and(|s| !lower_hex(s, 64))
        {
            return Err(failure(Code::InvalidShape));
        }
        Ok(Self {
            resolution,
            outcome_kind: outcome_kind.to_owned(),
            outcome_ref: outcome_ref.map(str::to_owned),
            outcome_sha256: outcome_sha256.map(str::to_owned),
        })
    }

    /// A finished review. `outcome_kind` matches `[A-Z][A-Z0-9_]{0,63}`;
    /// `outcome_ref` is typically the persisted decision record id.
    pub fn completed(
        outcome_kind: &str,
        outcome_ref: Option<&str>,
        outcome_sha256: Option<&str>,
    ) -> Result<Self, StateError> {
        Self::new(
            ReviewClaimResolutionV1::Completed,
            outcome_kind,
            outcome_ref,
            outcome_sha256,
        )
    }

    /// A retained terminal policy / safety refusal (for example
    /// `POLICY_BLOCKED`): later callbacks see it instead of re-routing.
    pub fn refused(
        outcome_kind: &str,
        outcome_ref: Option<&str>,
        outcome_sha256: Option<&str>,
    ) -> Result<Self, StateError> {
        Self::new(
            ReviewClaimResolutionV1::Refused,
            outcome_kind,
            outcome_ref,
            outcome_sha256,
        )
    }

    pub(crate) fn canonical(&self) -> serde_json::Value {
        serde_json::json!([
            match self.resolution {
                ReviewClaimResolutionV1::Completed => "COMPLETED",
                ReviewClaimResolutionV1::Refused => "REFUSED",
            },
            self.outcome_kind,
            self.outcome_ref,
            self.outcome_sha256,
        ])
    }

    fn state(&self) -> ReviewClaimStateV1 {
        match self.resolution {
            ReviewClaimResolutionV1::Completed => ReviewClaimStateV1::Completed,
            ReviewClaimResolutionV1::Refused => ReviewClaimStateV1::Refused,
        }
    }

    fn matches(&self, record: &ReviewClaimRecordV1) -> bool {
        record.state == self.state()
            && record.outcome_kind.as_deref() == Some(self.outcome_kind.as_str())
            && record.outcome_ref == self.outcome_ref
            && record.outcome_sha256 == self.outcome_sha256
    }
}

/// An explicit, non-automatic decision to stop waiting on an unfinished
/// claim. The trusted core must only issue it on human / policy authority,
/// which `authority_ref` names for the record. It never re-runs anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewClaimAbandonV1 {
    project_id: String,
    claim_id: String,
    authority_ref: String,
}

impl ReviewClaimAbandonV1 {
    pub fn new(project_id: &str, claim_id: &str, authority_ref: &str) -> Result<Self, StateError> {
        if !bounded_id(project_id) || !bounded_id(claim_id) || !bounded_id(authority_ref) {
            return Err(failure(Code::InvalidShape));
        }
        Ok(Self {
            project_id: project_id.to_owned(),
            claim_id: claim_id.to_owned(),
            authority_ref: authority_ref.to_owned(),
        })
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

const SELECT: &str = "SELECT project_id,claim_id,task_id,subject_sha,request_sha256,state,outcome_kind,outcome_ref,outcome_sha256,authority_ref,claimed_at,finished_at FROM review_claim WHERE project_id=?1 AND claim_id=?2";

fn decode(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReviewClaimRecordV1> {
    let state: String = r.get(5)?;
    Ok(ReviewClaimRecordV1 {
        project_id: r.get(0)?,
        claim_id: r.get(1)?,
        task_id: r.get(2)?,
        subject_sha: r.get(3)?,
        request_sha256: r.get(4)?,
        // Re-checked below; an unknown stored state is corruption.
        state: ReviewClaimStateV1::parse(&state).unwrap_or(ReviewClaimStateV1::Claimed),
        outcome_kind: r.get(6)?,
        outcome_ref: r.get(7)?,
        outcome_sha256: r.get(8)?,
        authority_ref: r.get(9)?,
        claimed_at: r.get(10)?,
        finished_at: r.get(11)?,
    })
}

pub(crate) fn read_claim(
    conn: &Connection,
    project: &str,
    claim_id: &str,
) -> Result<Option<ReviewClaimRecordV1>, StateError> {
    let row = conn
        .query_row(SELECT, params![project, claim_id], |r| {
            Ok((r.get::<_, String>(5)?, decode(r)?))
        })
        .optional()
        .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
    row.map(|(state, record)| {
        ReviewClaimStateV1::parse(&state)?;
        Ok(record)
    })
    .transpose()
}

/// Settles an unfinished claim inside the caller's open transaction. The
/// caller has accepted the trusted sample behind `finished_at`. Settling a
/// claim already settled with the identical disposition is accepted and
/// writes nothing; any other settled state refuses.
pub(crate) fn settle_in_tx(
    conn: &Connection,
    project: &str,
    claim_id: &str,
    settlement: &ReviewClaimSettlementV1,
    finished_at: &str,
) -> Result<ReviewClaimRecordV1, StateError> {
    let current =
        read_claim(conn, project, claim_id)?.ok_or_else(|| failure(Code::ClaimNotFound))?;
    if current.state != ReviewClaimStateV1::Claimed {
        return if settlement.matches(&current) {
            Ok(current)
        } else {
            Err(failure(Code::ClaimNotUnfinished))
        };
    }
    let changed = conn
        .execute(
            "UPDATE review_claim SET state=?1,outcome_kind=?2,outcome_ref=?3,outcome_sha256=?4,finished_at=?5 WHERE project_id=?6 AND claim_id=?7 AND state='CLAIMED'",
            params![
                settlement.state().as_str(),
                settlement.outcome_kind,
                settlement.outcome_ref,
                settlement.outcome_sha256,
                finished_at,
                project,
                claim_id
            ],
        )
        .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
    if changed != 1 {
        return Err(failure(Code::ClaimNotUnfinished));
    }
    crate::write_fault::boundary()?;
    read_claim(conn, project, claim_id)?.ok_or_else(|| failure(Code::CorruptStore))
}

fn sample(
    clock: &dyn TrustedClockV1,
) -> Result<(crate::TrustedTimeSampleV1, CanonicalTimestampV1), StateError> {
    let sample = clock
        .sample()
        .map_err(|e| graph::legacy_error(Phase::Clock, e))?;
    trusted_time::validate_sample(sample).map_err(|e| graph::legacy_error(Phase::Clock, e))
}

impl SqliteStateRepository {
    /// Inserts the claim, or returns the existing one. `true` means this call
    /// inserted it.
    pub(crate) fn claim_review_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: &ReviewClaimRequestV1,
    ) -> Result<(bool, ReviewClaimRecordV1), StateError> {
        let (sample, timestamp) = sample(clock)?;
        graph::write_transaction(self.connection(), |conn| {
            if let Some(existing) = read_claim(conn, &request.project_id, &request.claim_id)? {
                if existing.task_id != request.task_id
                    || existing.subject_sha != request.subject_sha
                    || existing.request_sha256 != request.request_sha256
                {
                    return Err(failure(Code::ConflictingRetry));
                }
                return Ok((false, existing));
            }
            graph::accept_graph_sample(conn, &request.project_id, &sample, &timestamp)?;
            crate::write_fault::boundary()?;
            conn.execute(
                "INSERT INTO review_claim(project_id,claim_id,task_id,subject_sha,request_sha256,state,claimed_at) VALUES (?1,?2,?3,?4,?5,'CLAIMED',?6)",
                params![
                    request.project_id,
                    request.claim_id,
                    request.task_id,
                    request.subject_sha,
                    request.request_sha256,
                    sample.canonical_utc_timestamp
                ],
            )
            .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
            crate::write_fault::boundary()?;
            let record = read_claim(conn, &request.project_id, &request.claim_id)?
                .ok_or_else(|| failure(Code::CorruptStore))?;
            Ok((true, record))
        })
    }

    /// Settles an unfinished claim (idempotent for the identical disposition).
    pub(crate) fn settle_review_claim_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        project: &str,
        claim_id: &str,
        settlement: &ReviewClaimSettlementV1,
    ) -> Result<ReviewClaimRecordV1, StateError> {
        let (sample, timestamp) = sample(clock)?;
        graph::write_transaction(self.connection(), |conn| {
            let current =
                read_claim(conn, project, claim_id)?.ok_or_else(|| failure(Code::ClaimNotFound))?;
            if current.state != ReviewClaimStateV1::Claimed {
                return settle_in_tx(conn, project, claim_id, settlement, "");
            }
            graph::accept_graph_sample(conn, project, &sample, &timestamp)?;
            crate::write_fault::boundary()?;
            settle_in_tx(
                conn,
                project,
                claim_id,
                settlement,
                &sample.canonical_utc_timestamp,
            )
        })
    }

    /// Records the explicit abandonment of an unfinished claim.
    pub(crate) fn abandon_review_claim_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: &ReviewClaimAbandonV1,
    ) -> Result<ReviewClaimRecordV1, StateError> {
        let (sample, timestamp) = sample(clock)?;
        graph::write_transaction(self.connection(), |conn| {
            let current = read_claim(conn, &request.project_id, &request.claim_id)?
                .ok_or_else(|| failure(Code::ClaimNotFound))?;
            match current.state {
                ReviewClaimStateV1::Abandoned
                    if current.authority_ref.as_deref() == Some(request.authority_ref.as_str()) =>
                {
                    return Ok(current);
                }
                ReviewClaimStateV1::Claimed => {}
                _ => return Err(failure(Code::ClaimNotUnfinished)),
            }
            graph::accept_graph_sample(conn, &request.project_id, &sample, &timestamp)?;
            crate::write_fault::boundary()?;
            let changed = conn
                .execute(
                    "UPDATE review_claim SET state='ABANDONED',outcome_kind='ABANDONED',authority_ref=?1,finished_at=?2 WHERE project_id=?3 AND claim_id=?4 AND state='CLAIMED'",
                    params![
                        request.authority_ref,
                        sample.canonical_utc_timestamp,
                        request.project_id,
                        request.claim_id
                    ],
                )
                .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
            if changed != 1 {
                return Err(failure(Code::ClaimNotUnfinished));
            }
            crate::write_fault::boundary()?;
            read_claim(conn, &request.project_id, &request.claim_id)?
                .ok_or_else(|| failure(Code::CorruptStore))
        })
    }

    /// Reads one claim by exact project and id (reads never write).
    pub fn find_review_claim(
        &self,
        project_id: &str,
        claim_id: &str,
    ) -> Result<Option<ReviewClaimRecordV1>, StateError> {
        if !bounded_id(project_id) || !bounded_id(claim_id) {
            return Err(failure(Code::InvalidShape));
        }
        read_claim(self.connection(), project_id, claim_id)
    }
}
