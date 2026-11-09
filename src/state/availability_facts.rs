//! Durable, monotonically ordered provider availability lifecycle facts (D5).
//!
//! Each `(project, provider, runtime, model)` partition has a State-assigned
//! sequence starting at 1 with no gaps (enforced by a trigger as well). A
//! demotion is retained history. No authenticated post-demotion producer
//! exists yet (adopted post-W3 deferral), so nothing recorded here can clear
//! the last demotion or yield `Available`: an unauthenticated promotion claim
//! or an elapsed retry-after is kept as separate evidence and the derived
//! status stays `Demoted` (or `Unknown` with no demotion). This module mints
//! no producer credential and no Runtime health.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{GraphFailureCode as Code, GraphPhase as Phase, StateError};
use crate::graph_repository as graph;
use crate::repository::SqliteStateRepository;
use crate::trusted_time::{self, TrustedClockV1};

/// What one fact records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityFactKindV1 {
    /// An observed demotion (rate limit, outage, auth loss, ...).
    Demotion,
    /// A claim that the provider recovered, from a source that is not an
    /// authenticated post-demotion producer. Recorded, never promoting.
    PromotionUnauthenticated,
    /// A demotion's retry-after window elapsed. Not evidence of recovery.
    RetryAfterElapsed,
}

impl AvailabilityFactKindV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Demotion => "DEMOTION",
            Self::PromotionUnauthenticated => "PROMOTION_UNAUTHENTICATED",
            Self::RetryAfterElapsed => "RETRY_AFTER_ELAPSED",
        }
    }
}

/// Where the fact came from. There is deliberately no authenticated-producer
/// variant until a real producer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityFactSourceV1 {
    RuntimeFailureClass,
    ProbeReport,
    RetryAfterExpiry,
    UnauthenticatedClaim,
}

impl AvailabilityFactSourceV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RuntimeFailureClass => "RUNTIME_FAILURE_CLASS",
            Self::ProbeReport => "PROBE_REPORT",
            Self::RetryAfterExpiry => "RETRY_AFTER_EXPIRY",
            Self::UnauthenticatedClaim => "UNAUTHENTICATED_CLAIM",
        }
    }
}

/// One proposed fact. Non-demotion facts must name the exact sequence they
/// follow; a stale or forged `follows_sequence` is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailabilityFactV1 {
    project_id: String,
    provider_id: String,
    runtime_id: String,
    model_id: String,
    kind: AvailabilityFactKindV1,
    reason: String,
    source: AvailabilityFactSourceV1,
    source_ref: String,
    follows_sequence: Option<u64>,
}

fn bounded(value: &str, min: usize) -> bool {
    (min..=200).contains(&value.chars().count())
}

impl AvailabilityFactV1 {
    /// Validates shape. `model_id` may be empty (provider/runtime-wide fact).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        project_id: String,
        provider_id: String,
        runtime_id: String,
        model_id: String,
        kind: AvailabilityFactKindV1,
        reason: String,
        source: AvailabilityFactSourceV1,
        source_ref: String,
        follows_sequence: Option<u64>,
    ) -> Result<Self, StateError> {
        let shape_ok = bounded(&project_id, 1)
            && bounded(&provider_id, 1)
            && bounded(&runtime_id, 1)
            && bounded(&model_id, 0)
            && bounded(&reason, 1)
            && bounded(&source_ref, 1)
            && follows_sequence.is_none_or(|s| s >= 1 && s <= i64::MAX as u64)
            && match kind {
                AvailabilityFactKindV1::Demotion => follows_sequence.is_none(),
                AvailabilityFactKindV1::PromotionUnauthenticated => {
                    source == AvailabilityFactSourceV1::UnauthenticatedClaim
                        || source == AvailabilityFactSourceV1::ProbeReport
                }
                AvailabilityFactKindV1::RetryAfterElapsed => {
                    source == AvailabilityFactSourceV1::RetryAfterExpiry
                }
            }
            && (kind == AvailabilityFactKindV1::Demotion || follows_sequence.is_some());
        if !shape_ok {
            return Err(graph::failure(Phase::Input, Code::InvalidShape, None));
        }
        Ok(Self {
            project_id,
            provider_id,
            runtime_id,
            model_id,
            kind,
            reason,
            source,
            source_ref,
            follows_sequence,
        })
    }

    pub fn project_id(&self) -> &str {
        &self.project_id
    }
}

/// Derived lifecycle. There is no `Available` until an authenticated
/// producer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvailabilityLifecycleStatusV1 {
    Unknown,
    Demoted,
}

/// What appending a fact did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailabilityFactReceiptV1 {
    pub sequence: u64,
    pub recorded_at: String,
    pub status: AvailabilityLifecycleStatusV1,
}

/// Current lifecycle with retained demotion and separate unverified evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailabilityLifecycleV1 {
    pub status: AvailabilityLifecycleStatusV1,
    pub latest_sequence: u64,
    pub last_demotion_sequence: Option<u64>,
    pub last_demotion_reason: Option<String>,
    /// Latest unauthenticated promotion claim or retry-after expiry after the
    /// last demotion, kept as evidence only.
    pub unverified_recovery_sequence: Option<u64>,
}

fn latest(conn: &Connection, f: &AvailabilityFactV1) -> Result<u64, StateError> {
    let max: Option<i64> = conn
        .query_row(
            "SELECT MAX(sequence) FROM availability_fact WHERE project_id=?1 AND provider_id=?2 AND runtime_id=?3 AND model_id=?4",
            params![f.project_id, f.provider_id, f.runtime_id, f.model_id],
            |r| r.get(0),
        )
        .map_err(|e| graph::sqlite_error(Phase::Read, e))?;
    Ok(max.unwrap_or(0) as u64)
}

fn lifecycle(
    conn: &Connection,
    project: &str,
    provider: &str,
    runtime: &str,
    model: &str,
) -> Result<AvailabilityLifecycleV1, StateError> {
    let phase = Phase::Read;
    let key = params![project, provider, runtime, model];
    let latest: Option<i64> = conn
        .query_row(
            "SELECT MAX(sequence) FROM availability_fact WHERE project_id=?1 AND provider_id=?2 AND runtime_id=?3 AND model_id=?4",
            key,
            |r| r.get(0),
        )
        .map_err(|e| graph::sqlite_error(phase, e))?;
    let demotion: Option<(i64, String)> = conn
        .query_row(
            "SELECT sequence,reason FROM availability_fact WHERE project_id=?1 AND provider_id=?2 AND runtime_id=?3 AND model_id=?4 AND kind='DEMOTION' ORDER BY sequence DESC LIMIT 1",
            key,
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| graph::sqlite_error(phase, e))?;
    let since = demotion.as_ref().map_or(0, |(s, _)| *s);
    let unverified: Option<i64> = conn
        .query_row(
            "SELECT MAX(sequence) FROM availability_fact WHERE project_id=?1 AND provider_id=?2 AND runtime_id=?3 AND model_id=?4 AND kind<>'DEMOTION' AND sequence>?5",
            params![project, provider, runtime, model, since],
            |r| r.get(0),
        )
        .map_err(|e| graph::sqlite_error(phase, e))?;
    Ok(AvailabilityLifecycleV1 {
        status: if demotion.is_some() {
            AvailabilityLifecycleStatusV1::Demoted
        } else {
            AvailabilityLifecycleStatusV1::Unknown
        },
        latest_sequence: latest.unwrap_or(0) as u64,
        last_demotion_sequence: demotion.as_ref().map(|(s, _)| *s as u64),
        last_demotion_reason: demotion.map(|(_, r)| r),
        unverified_recovery_sequence: unverified.map(|s| s as u64),
    })
}

impl SqliteStateRepository {
    pub(crate) fn append_availability_fact(
        &mut self,
        clock: &dyn TrustedClockV1,
        fact: &AvailabilityFactV1,
    ) -> Result<AvailabilityFactReceiptV1, StateError> {
        let sample = clock
            .sample()
            .map_err(|e| graph::legacy_error(Phase::Clock, e))?;
        let (sample, timestamp) = trusted_time::validate_sample(sample)
            .map_err(|e| graph::legacy_error(Phase::Clock, e))?;
        graph::write_transaction(self.connection(), |conn| {
            let current = latest(conn, fact)?;
            // A non-demotion fact must follow the exact latest sequence:
            // anything older is stale, anything newer is forged.
            if fact.follows_sequence.is_some_and(|s| s != current)
                || (fact.kind != AvailabilityFactKindV1::Demotion && current == 0)
            {
                return Err(graph::failure(Phase::Write, Code::ParentConflict, None));
            }
            graph::accept_graph_sample(conn, &fact.project_id, &sample, &timestamp)?;
            let sequence = current + 1;
            conn.execute(
                "INSERT INTO availability_fact(project_id,provider_id,runtime_id,model_id,sequence,kind,reason,source_kind,source_ref,follows_sequence,recorded_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                params![
                    fact.project_id,
                    fact.provider_id,
                    fact.runtime_id,
                    fact.model_id,
                    sequence as i64,
                    fact.kind.as_str(),
                    fact.reason,
                    fact.source.as_str(),
                    fact.source_ref,
                    fact.follows_sequence.map(|s| s as i64),
                    sample.canonical_utc_timestamp,
                ],
            )
            .map_err(|e| graph::sqlite_error(Phase::Write, e))?;
            let status = lifecycle(
                conn,
                &fact.project_id,
                &fact.provider_id,
                &fact.runtime_id,
                &fact.model_id,
            )?
            .status;
            Ok(AvailabilityFactReceiptV1 {
                sequence,
                recorded_at: sample.canonical_utc_timestamp.clone(),
                status,
            })
        })
    }

    /// Current derived lifecycle for one partition (empty `model_id` for
    /// provider/runtime-wide facts).
    pub fn availability_lifecycle(
        &self,
        project_id: &str,
        provider_id: &str,
        runtime_id: &str,
        model_id: &str,
    ) -> Result<AvailabilityLifecycleV1, StateError> {
        lifecycle(
            self.connection(),
            project_id,
            provider_id,
            runtime_id,
            model_id,
        )
    }
}
