//! Trusted-clock consumption and durable per-project monotonic watermarking.

use rusqlite::{Connection, OptionalExtension, params};

use crate::canonical_timestamp::CanonicalTimestampV1;
use crate::error::StateError;
use crate::repository::SqliteStateRepository;

/// One sample supplied by the external trusted-clock contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedTimeSampleV1 {
    pub canonical_utc_timestamp: String,
    pub clock_source_id: String,
    pub clock_contract_version: String,
}

/// State's clock-consumer port. State provides no production implementation.
pub trait TrustedClockV1 {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError>;
}

/// The durable monotonic trusted-time fence for one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedTimeWatermarkV1 {
    pub project_id: String,
    pub clock_source_id: String,
    pub clock_contract_version: String,
    pub last_accepted_trusted_time: String,
}

impl SqliteStateRepository {
    /// Reads the durable watermark without offering a mutation or reset API.
    pub fn find_trusted_time_watermark(
        &self,
        project_id: &str,
    ) -> Result<Option<TrustedTimeWatermarkV1>, StateError> {
        read_watermark(self.connection(), project_id)
    }
}

pub(crate) fn validate_sample(
    sample: TrustedTimeSampleV1,
) -> Result<(TrustedTimeSampleV1, CanonicalTimestampV1), StateError> {
    if sample.clock_source_id.is_empty() || sample.clock_contract_version.is_empty() {
        return Err(StateError::TrustedClockSampleInvalid {
            detail: "clock_source_id and clock_contract_version must be non-empty".to_string(),
        });
    }
    let timestamp = CanonicalTimestampV1::parse(&sample.canonical_utc_timestamp)?;
    Ok((sample, timestamp))
}

pub(crate) fn accept_sample(
    conn: &Connection,
    project_id: &str,
    sample: &TrustedTimeSampleV1,
    timestamp: &CanonicalTimestampV1,
) -> Result<(), StateError> {
    let Some(current) = read_watermark(conn, project_id)? else {
        conn.execute(
            "INSERT INTO trusted_time_watermark (project_id, clock_source_id, clock_contract_version, last_accepted_trusted_time) VALUES (?1, ?2, ?3, ?4)",
            params![project_id, sample.clock_source_id, sample.clock_contract_version, sample.canonical_utc_timestamp],
        )
        .map_err(watermark_write_failure)?;
        return Ok(());
    };

    let current_time =
        CanonicalTimestampV1::parse(&current.last_accepted_trusted_time).map_err(|error| {
            StateError::TrustedTimeWatermarkDecodeFailed {
                detail: error.to_string(),
            }
        })?;
    if current.clock_source_id != sample.clock_source_id
        || current.clock_contract_version != sample.clock_contract_version
    {
        return Err(StateError::TrustedClockContinuityUnbound {
            project_id: project_id.to_string(),
        });
    }
    if timestamp < &current_time {
        return Err(StateError::TrustedClockRegression {
            project_id: project_id.to_string(),
            sample: sample.canonical_utc_timestamp.clone(),
            watermark: current.last_accepted_trusted_time,
        });
    }
    if timestamp > &current_time {
        conn.execute(
            "UPDATE trusted_time_watermark SET last_accepted_trusted_time = ?1 WHERE project_id = ?2 AND last_accepted_trusted_time = ?3",
            params![sample.canonical_utc_timestamp, project_id, current.last_accepted_trusted_time],
        )
        .map_err(watermark_write_failure)?;
    }
    Ok(())
}

pub(crate) fn require_current_sample(
    conn: &Connection,
    project_id: &str,
    sample: &TrustedTimeSampleV1,
    timestamp: &CanonicalTimestampV1,
) -> Result<(), StateError> {
    let current = read_watermark(conn, project_id)?.ok_or_else(|| {
        StateError::TrustedTimeWatermarkDecodeFailed {
            detail: format!("missing watermark for fenced project {project_id:?}"),
        }
    })?;
    if current.clock_source_id != sample.clock_source_id
        || current.clock_contract_version != sample.clock_contract_version
    {
        return Err(StateError::TrustedClockContinuityUnbound {
            project_id: project_id.to_string(),
        });
    }
    let current_time =
        CanonicalTimestampV1::parse(&current.last_accepted_trusted_time).map_err(|error| {
            StateError::TrustedTimeWatermarkDecodeFailed {
                detail: error.to_string(),
            }
        })?;
    if timestamp < &current_time {
        return Err(StateError::TrustedTimeSampleStale {
            project_id: project_id.to_string(),
            sample: sample.canonical_utc_timestamp.clone(),
            watermark: current.last_accepted_trusted_time,
        });
    }
    Ok(())
}

fn read_watermark(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<TrustedTimeWatermarkV1>, StateError> {
    let watermark = conn.query_row(
        "SELECT project_id, clock_source_id, clock_contract_version, last_accepted_trusted_time FROM trusted_time_watermark WHERE project_id = ?1",
        [project_id],
        |row| {
            Ok(TrustedTimeWatermarkV1 {
                project_id: row.get(0)?,
                clock_source_id: row.get(1)?,
                clock_contract_version: row.get(2)?,
                last_accepted_trusted_time: row.get(3)?,
            })
        },
    )
    .optional()
    .map_err(|error| StateError::TrustedTimeWatermarkDecodeFailed {
        detail: error.to_string(),
    })?;
    if let Some(value) = &watermark {
        if value.project_id.is_empty()
            || value.clock_source_id.is_empty()
            || value.clock_contract_version.is_empty()
        {
            return Err(StateError::TrustedTimeWatermarkDecodeFailed {
                detail: "persisted watermark identifiers must be non-empty".to_string(),
            });
        }
        CanonicalTimestampV1::parse(&value.last_accepted_trusted_time).map_err(|error| {
            StateError::TrustedTimeWatermarkDecodeFailed {
                detail: error.to_string(),
            }
        })?;
    }
    Ok(watermark)
}

fn watermark_write_failure(error: rusqlite::Error) -> StateError {
    StateError::TrustedTimeWatermarkWriteFailed {
        detail: error.to_string(),
    }
}
