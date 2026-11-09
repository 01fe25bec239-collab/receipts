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
//!
//! # Seal
//!
//! Only the live claimant (the holder of the ticket) can, after its reviewer
//! returned and BEFORE it publishes anything, seal the exact result:
//! [`ReviewSealRequestV1`] binds the claim, the digest and settlement of the
//! one [`DecisionPublicationV1`](crate::DecisionPublicationV1) it will publish
//! and the exact recovery bytes ([`ReviewRecoveryDataV1`]). After a restart
//! [`SqliteStateRepository::find_review_seal`] returns the sealed result and
//! `DecisionPublicationV1::with_sealed_claim` finishes exactly that
//! publication, with no ticket, routing or reviewer run. A sealed claim is
//! completed only by its sealed package. An unsealed claim stays `CLAIMED`
//! and uncertain: a crash between the external execution and the seal commit
//! is never resolved automatically, because an external execution and a
//! SQLite commit are not one transaction.

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

    /// `true` for `completed`, `false` for `refused`.
    pub fn is_completed(&self) -> bool {
        self.resolution == ReviewClaimResolutionV1::Completed
    }
    pub fn outcome_kind(&self) -> &str {
        &self.outcome_kind
    }
    pub fn outcome_ref(&self) -> Option<&str> {
        self.outcome_ref.as_deref()
    }
    pub fn outcome_sha256(&self) -> Option<&str> {
        self.outcome_sha256.as_deref()
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

// ---- durable seal -------------------------------------------------------------

/// Upper bound of one seal's recovery data (the schema enforces the same).
pub const MAX_RECOVERY_BYTES: usize = 4 * 1024 * 1024;

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The exact observed result a claimant needs to finish its publication after
/// a restart, as opaque bytes plus a token naming their format (for example
/// `REVIEW_SEALED_V1`). State never parses it: the trusted core validates the
/// bytes before sealing and again before it rebuilds a package from them.
#[derive(Clone, PartialEq, Eq)]
pub struct ReviewRecoveryDataV1 {
    kind: String,
    bytes: Vec<u8>,
    sha256: String,
}

impl std::fmt::Debug for ReviewRecoveryDataV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReviewRecoveryDataV1")
    }
}

impl ReviewRecoveryDataV1 {
    /// `kind` matches `[A-Z][A-Z0-9_]{0,63}`; `bytes` is 1..=[`MAX_RECOVERY_BYTES`];
    /// `declared_sha256` must be the lowercase SHA-256 of `bytes` (State
    /// recomputes it and refuses a mismatch).
    pub fn new(kind: &str, bytes: Vec<u8>, declared_sha256: &str) -> Result<Self, StateError> {
        if !token(kind)
            || bytes.is_empty()
            || bytes.len() > MAX_RECOVERY_BYTES
            || !lower_hex(declared_sha256, 64)
            || sha256_hex(&bytes) != declared_sha256
        {
            return Err(failure(Code::InvalidShape));
        }
        Ok(Self {
            kind: kind.to_owned(),
            bytes,
            sha256: declared_sha256.to_owned(),
        })
    }
    pub fn kind(&self) -> &str {
        &self.kind
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

/// What the claimant seals: the claim it holds, the identity and settlement of
/// the one package it will publish, and the exact recovery data. The package
/// identity and settlement are derived from the [`DecisionPublicationV1`]
/// itself, never supplied separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSealRequestV1 {
    project_id: String,
    claim_id: String,
    task_id: String,
    subject_sha: Option<String>,
    request_sha256: String,
    publication_id: String,
    package_sha256: String,
    settlement: ReviewClaimSettlementV1,
    ticket_store_id: String,
    ticket_instance: u64,
    recovery: ReviewRecoveryDataV1,
}

impl ReviewSealRequestV1 {
    /// `claim` is the request the claimant claimed with (State verifies it
    /// equals the stored claim); `package` must settle that same claim through
    /// [`DecisionPublicationV1::with_claim_settlement`].
    pub fn new(
        claim: &ReviewClaimRequestV1,
        package: &crate::DecisionPublicationV1,
        recovery: ReviewRecoveryDataV1,
    ) -> Result<Self, StateError> {
        let (claim_id, settlement) = package
            .claim_identity()
            .ok_or_else(|| failure(Code::InvalidShape))?;
        let (ticket_store_id, ticket_instance) = package
            .claim_binding()
            .ok_or_else(|| failure(Code::InvalidShape))?;
        if claim_id != claim.claim_id || package.project_id() != claim.project_id {
            return Err(failure(Code::InvalidShape));
        }
        Ok(Self {
            project_id: claim.project_id.clone(),
            claim_id: claim.claim_id.clone(),
            task_id: claim.task_id.clone(),
            subject_sha: claim.subject_sha.clone(),
            request_sha256: claim.request_sha256.clone(),
            publication_id: package.publication_id().to_owned(),
            package_sha256: package.sha256(),
            settlement: settlement.clone(),
            ticket_store_id: ticket_store_id.to_owned(),
            ticket_instance,
            recovery,
        })
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn claim_id(&self) -> &str {
        &self.claim_id
    }
    pub(crate) fn ticket_binding(&self) -> (&str, u64) {
        (&self.ticket_store_id, self.ticket_instance)
    }

    fn seal_sha256(&self) -> String {
        seal_digest(&SealIdentity {
            project_id: &self.project_id,
            claim_id: &self.claim_id,
            task_id: &self.task_id,
            subject_sha: self.subject_sha.as_deref(),
            request_sha256: &self.request_sha256,
            publication_id: &self.publication_id,
            package_sha256: &self.package_sha256,
            settlement: &self.settlement,
            recovery_kind: &self.recovery.kind,
            recovery_sha256: &self.recovery.sha256,
        })
    }
}

struct SealIdentity<'a> {
    project_id: &'a str,
    claim_id: &'a str,
    task_id: &'a str,
    subject_sha: Option<&'a str>,
    request_sha256: &'a str,
    publication_id: &'a str,
    package_sha256: &'a str,
    settlement: &'a ReviewClaimSettlementV1,
    recovery_kind: &'a str,
    recovery_sha256: &'a str,
}

fn seal_digest(s: &SealIdentity<'_>) -> String {
    sha256_hex(
        serde_json::json!([
            "receipts.review-claim-seal.v1",
            s.project_id,
            s.claim_id,
            s.task_id,
            s.subject_sha,
            s.request_sha256,
            s.publication_id,
            s.package_sha256,
            s.settlement.canonical(),
            s.recovery_kind,
            s.recovery_sha256,
        ])
        .to_string()
        .as_bytes(),
    )
}

/// The durable, immutable seal read back from State. Reading verifies the
/// recovery bytes against their digest and the whole seal against its identity
/// digest; any drift is `CorruptStore`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSealRecordV1 {
    pub project_id: String,
    pub claim_id: String,
    pub task_id: String,
    pub subject_sha: Option<String>,
    pub request_sha256: String,
    /// The publication identity the sealed package must be published under.
    pub publication_id: String,
    /// [`DecisionPublicationV1::sha256`](crate::DecisionPublicationV1::sha256)
    /// of the sealed package (it includes the claim settlement below).
    pub package_sha256: String,
    /// The disposition the claim takes when the sealed package is published.
    pub settlement: ReviewClaimSettlementV1,
    pub recovery_kind: String,
    pub recovery_data: Vec<u8>,
    pub recovery_sha256: String,
    /// The identity digest of everything above except `sealed_at`.
    pub seal_sha256: String,
    pub sealed_at: String,
}

const SEAL_SELECT: &str = "SELECT task_id,subject_sha,request_sha256,publication_id,package_sha256,resolution,outcome_kind,outcome_ref,outcome_sha256,recovery_kind,recovery_data,recovery_sha256,seal_sha256,sealed_at FROM review_claim_seal WHERE project_id=?1 AND claim_id=?2";

pub(crate) fn read_seal(
    conn: &Connection,
    project: &str,
    claim_id: &str,
) -> Result<Option<ReviewSealRecordV1>, StateError> {
    type Row = (
        String,
        Option<String>,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        String,
        Vec<u8>,
        String,
        String,
        String,
    );
    let row: Option<Row> = conn
        .query_row(SEAL_SELECT, params![project, claim_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
                r.get(12)?,
                r.get(13)?,
            ))
        })
        .optional()
        .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
    let Some(row) = row else { return Ok(None) };
    let corrupt = || failure(Code::CorruptStore);
    let settlement = match row.5.as_str() {
        "COMPLETED" => {
            ReviewClaimSettlementV1::completed(&row.6, row.7.as_deref(), row.8.as_deref())
        }
        "REFUSED" => ReviewClaimSettlementV1::refused(&row.6, row.7.as_deref(), row.8.as_deref()),
        _ => return Err(corrupt()),
    }
    .map_err(|_| corrupt())?;
    let record = ReviewSealRecordV1 {
        project_id: project.to_owned(),
        claim_id: claim_id.to_owned(),
        task_id: row.0,
        subject_sha: row.1,
        request_sha256: row.2,
        publication_id: row.3,
        package_sha256: row.4,
        settlement,
        recovery_kind: row.9,
        recovery_data: row.10,
        recovery_sha256: row.11,
        seal_sha256: row.12,
        sealed_at: row.13,
    };
    let identity = seal_digest(&SealIdentity {
        project_id: &record.project_id,
        claim_id: &record.claim_id,
        task_id: &record.task_id,
        subject_sha: record.subject_sha.as_deref(),
        request_sha256: &record.request_sha256,
        publication_id: &record.publication_id,
        package_sha256: &record.package_sha256,
        settlement: &record.settlement,
        recovery_kind: &record.recovery_kind,
        recovery_sha256: &record.recovery_sha256,
    });
    if sha256_hex(&record.recovery_data) != record.recovery_sha256 || identity != record.seal_sha256
    {
        return Err(corrupt());
    }
    Ok(Some(record))
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
            // A sealed claim is completed only by publishing its sealed package.
            if read_seal(conn, project, claim_id)?.is_some() {
                return Err(failure(Code::ClaimSealed));
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

    /// Seals the claimant's exact result (idempotent for identical content).
    pub(crate) fn seal_review_claim_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: &ReviewSealRequestV1,
    ) -> Result<ReviewSealRecordV1, StateError> {
        let (sample, timestamp) = sample(clock)?;
        let digest = request.seal_sha256();
        graph::write_transaction(self.connection(), |conn| {
            let claim = read_claim(conn, &request.project_id, &request.claim_id)?
                .ok_or_else(|| failure(Code::ClaimNotFound))?;
            if claim.task_id != request.task_id
                || claim.subject_sha != request.subject_sha
                || claim.request_sha256 != request.request_sha256
            {
                return Err(failure(Code::ConflictingRetry));
            }
            if let Some(existing) = read_seal(conn, &request.project_id, &request.claim_id)? {
                return if existing.seal_sha256 == digest {
                    Ok(existing)
                } else {
                    Err(failure(Code::ConflictingRetry))
                };
            }
            if claim.state != ReviewClaimStateV1::Claimed {
                return Err(failure(Code::ClaimNotUnfinished));
            }
            // The publication identity belongs to this one claim and package.
            let taken: Option<String> = conn
                .query_row(
                    "SELECT package_sha256 FROM atomic_publication WHERE project_id=?1 AND publication_id=?2",
                    params![request.project_id, request.publication_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
            let other_seal: bool = conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM review_claim_seal WHERE project_id=?1 AND publication_id=?2)",
                    params![request.project_id, request.publication_id],
                    |r| r.get(0),
                )
                .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
            if other_seal || taken.is_some_and(|stored| stored != request.package_sha256) {
                return Err(failure(Code::ConflictingRetry));
            }
            graph::accept_graph_sample(conn, &request.project_id, &sample, &timestamp)?;
            crate::write_fault::boundary()?;
            conn.execute(
                "INSERT INTO review_claim_seal(project_id,claim_id,task_id,subject_sha,request_sha256,publication_id,package_sha256,resolution,outcome_kind,outcome_ref,outcome_sha256,recovery_kind,recovery_data,recovery_sha256,seal_sha256,sealed_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                params![
                    request.project_id,
                    request.claim_id,
                    request.task_id,
                    request.subject_sha,
                    request.request_sha256,
                    request.publication_id,
                    request.package_sha256,
                    request.settlement.state().as_str(),
                    request.settlement.outcome_kind,
                    request.settlement.outcome_ref,
                    request.settlement.outcome_sha256,
                    request.recovery.kind,
                    request.recovery.bytes,
                    request.recovery.sha256,
                    digest,
                    sample.canonical_utc_timestamp
                ],
            )
            .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
            crate::write_fault::boundary()?;
            read_seal(conn, &request.project_id, &request.claim_id)?
                .ok_or_else(|| failure(Code::CorruptStore))
        })
    }

    /// Reads one claim's seal by exact project and id (reads never write).
    pub fn find_review_seal(
        &self,
        project_id: &str,
        claim_id: &str,
    ) -> Result<Option<ReviewSealRecordV1>, StateError> {
        if !bounded_id(project_id) || !bounded_id(claim_id) {
            return Err(failure(Code::InvalidShape));
        }
        read_seal(self.connection(), project_id, claim_id)
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
