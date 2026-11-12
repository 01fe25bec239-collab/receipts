//! D12 records are evidence, never review acceptance or restart authority.
use crate::{
    GraphPhase, SqliteStateRepository, StateError, TrustedClockV1, graph_repository as graph,
    trusted_time,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemotePublicationIntentV1 {
    pub project_id: String,
    pub operation_id: String,
    /// Trusted canonical repository identity; never a worker-supplied path.
    pub repository: String,
    /// Canonical owned file-remote identity. Credential-bearing URLs are unsupported.
    pub remote: String,
    pub source_ref: String,
    pub destination_ref: String,
    pub commit: String,
    pub wire: String,
    pub mode: String,
    pub branch_class: String,
    /// Immutable acceptance artifact reference supplied by trusted composition.
    pub acceptance_ref: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemotePublicationOutcomeV1 {
    Published,
    RemoteFailed,
    Refused,
    Uncertain,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemotePublicationResultV1 {
    pub sequence: u64,
    pub outcome: RemotePublicationOutcomeV1,
    pub observed_commit: Option<String>,
    pub recorded_at: String,
    pub evidence_ref: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemotePublicationRecordV1 {
    pub intent: RemotePublicationIntentV1,
    pub results: Vec<RemotePublicationResultV1>,
}
fn invalid() -> StateError {
    StateError::SetupRefused {
        reason: crate::SetupRefusalV1::InvalidShape,
    }
}
fn conflict() -> StateError {
    StateError::SetupRefused {
        reason: crate::SetupRefusalV1::ConflictingReplay,
    }
}
fn sql(e: rusqlite::Error) -> StateError {
    graph::sqlite_error(GraphPhase::Write, e)
}
fn bounded(s: &str) -> bool {
    !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control)
}
fn reference(s: &str) -> bool {
    s.starts_with("refs/heads/")
        && bounded(s)
        && !s.contains([' ', '~', '^', ':', '?', '*', '[', '\\'])
        && !s.contains("..")
        && !s.contains("@{")
        && !s.ends_with(['.', '/'])
        && s.split('/')
            .all(|v| !v.is_empty() && !v.starts_with('.') && !v.ends_with(".lock"))
}
impl RemotePublicationIntentV1 {
    pub fn validate(&self) -> Result<(), StateError> {
        if ![
            &self.project_id,
            &self.operation_id,
            &self.repository,
            &self.remote,
        ]
        .iter()
        .all(|s| bounded(s))
            || !self.repository.starts_with('/')
            || !self.remote.starts_with('/')
            || !reference(&self.source_ref)
            || self.source_ref != self.destination_ref
            || !graph::hex(&self.commit, 40)
            || !matches!(
                self.wire.as_str(),
                "LOCAL_ONLY" | "PUSH_ON_ACCEPT" | "PUSH_ALWAYS"
            )
            || !matches!(
                self.mode.as_str(),
                "LOCAL_ONLY" | "PUSH_A2_BRANCHES" | "PUSH_ACCEPTED_A3" | "PUSH_ALL_CHECKPOINTS"
            )
            || !matches!(
                self.branch_class.as_str(),
                "WORKSTREAM" | "TASK" | "CHECKPOINT"
            )
            || self.acceptance_ref.as_deref().is_some_and(|s| !bounded(s))
            || (self.wire == "PUSH_ON_ACCEPT" && self.acceptance_ref.is_none())
        {
            return Err(invalid());
        }
        Ok(())
    }
}
impl SqliteStateRepository {
    pub fn read_remote_publication_v1(
        &self,
        project: &str,
        operation: &str,
    ) -> Result<Option<RemotePublicationRecordV1>, StateError> {
        graph::verify_current(self.connection(), GraphPhase::Read)?;
        let raw: Option<String> = self.connection().query_row("SELECT record_json FROM remote_publication_intent WHERE project_id=?1 AND operation_id=?2",params![project,operation],|r|r.get(0)).optional().map_err(sql)?;
        let Some(raw) = raw else { return Ok(None) };
        let intent: RemotePublicationIntentV1 =
            serde_json::from_str(&raw).map_err(|_| graph::corrupt())?;
        intent.validate().map_err(|_| graph::corrupt())?;
        if intent.project_id != project || intent.operation_id != operation {
            return Err(graph::corrupt());
        }
        let mut stmt=self.connection().prepare("SELECT sequence,record_json FROM remote_publication_result WHERE project_id=?1 AND operation_id=?2 ORDER BY sequence").map_err(sql)?;
        let rows = stmt
            .query_map(params![project, operation], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(sql)?;
        let mut results = Vec::new();
        for row in rows {
            let (sequence, raw) = row.map_err(sql)?;
            let result: RemotePublicationResultV1 =
                serde_json::from_str(&raw).map_err(|_| graph::corrupt())?;
            if result.sequence as i64 != sequence
                || sequence != results.len() as i64 + 1
                || !bounded(&result.evidence_ref)
                || result
                    .observed_commit
                    .as_deref()
                    .is_some_and(|s| !graph::hex(s, 40))
                || (result.outcome == RemotePublicationOutcomeV1::Published
                    && result.observed_commit.as_deref() != Some(&intent.commit))
            {
                return Err(graph::corrupt());
            }
            crate::canonical_timestamp::CanonicalTimestampV1::parse(&result.recorded_at)
                .map_err(|_| graph::corrupt())?;
            results.push(result);
        }
        Ok(Some(RemotePublicationRecordV1 { intent, results }))
    }
    pub(crate) fn reserve_remote_publication_v1(
        &mut self,
        intent: &RemotePublicationIntentV1,
    ) -> Result<crate::SetupWriteOutcomeV1, StateError> {
        intent.validate()?;
        let raw = serde_json::to_string(intent).map_err(|_| invalid())?;
        self.run_serialized_transaction(|unit| {
            let conn=unit.tx();
            graph::verify_current(conn,GraphPhase::Write)?;
            let old: Option<String>=conn.query_row("SELECT record_json FROM remote_publication_intent WHERE project_id=?1 AND operation_id=?2",params![intent.project_id,intent.operation_id],|r|r.get(0)).optional().map_err(sql)?;
            if let Some(old)=old { return if old==raw {Ok(crate::SetupWriteOutcomeV1::Replayed)} else {Err(conflict())}; }
            conn.execute("INSERT INTO remote_publication_intent VALUES (?1,?2,?3)",params![intent.project_id,intent.operation_id,raw]).map_err(sql)?;
            Ok(crate::SetupWriteOutcomeV1::Applied)
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn append_remote_publication_result_v1(
        &mut self,
        project: &str,
        operation: &str,
        sequence: u64,
        outcome: RemotePublicationOutcomeV1,
        observed_commit: Option<&str>,
        evidence_ref: &str,
        clock: &dyn TrustedClockV1,
    ) -> Result<crate::SetupWriteOutcomeV1, StateError> {
        if sequence == 0
            || sequence > i64::MAX as u64
            || !bounded(evidence_ref)
            || observed_commit.is_some_and(|s| !graph::hex(s, 40))
        {
            return Err(invalid());
        }
        let record = self
            .read_remote_publication_v1(project, operation)?
            .ok_or_else(invalid)?;
        if outcome == RemotePublicationOutcomeV1::Published
            && observed_commit != Some(record.intent.commit.as_str())
        {
            return Err(invalid());
        }
        if let Some(old) = record.results.iter().find(|r| r.sequence == sequence) {
            return if old.outcome == outcome
                && old.observed_commit.as_deref() == observed_commit
                && old.evidence_ref == evidence_ref
            {
                Ok(crate::SetupWriteOutcomeV1::Replayed)
            } else {
                Err(conflict())
            };
        }
        if sequence != record.results.len() as u64 + 1 {
            return Err(conflict());
        }
        let (sample, time) = trusted_time::validate_sample(clock.sample()?)?;
        let result = RemotePublicationResultV1 {
            sequence,
            outcome,
            observed_commit: observed_commit.map(str::to_owned),
            evidence_ref: evidence_ref.into(),
            recorded_at: sample.canonical_utc_timestamp.clone(),
        };
        let raw = serde_json::to_string(&result).map_err(|_| invalid())?;
        self.run_serialized_transaction(|unit| {
            let conn = unit.tx();
            graph::verify_current(conn, GraphPhase::Write)?;
            trusted_time::accept_sample(conn, project, &sample, &time)?;
            conn.execute(
                "INSERT INTO remote_publication_result VALUES (?1,?2,?3,?4)",
                params![project, operation, sequence as i64, raw],
            )
            .map_err(sql)?;
            Ok(crate::SetupWriteOutcomeV1::Applied)
        })
    }
}

impl RemotePublicationRecordV1 {
    /// A reservation without a response must be reconciled before any retry.
    pub fn outcome(&self) -> RemotePublicationOutcomeV1 {
        self.results
            .last()
            .map_or(RemotePublicationOutcomeV1::Uncertain, |r| r.outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        StateWriteOperationV1 as Op, StateWriteSubjectV1, TrustedCoreStore, TrustedTimeSampleV1,
        atomic_publication_tests::Dir,
    };
    fn auth(store: &TrustedCoreStore, project: &str) -> crate::StateWriteAdmission {
        store.admit(StateWriteSubjectV1 {
            operation: Op::RemotePublication,
            project_id: project.into(),
            graph_id: None,
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        })
    }
    struct Clock;
    impl TrustedClockV1 for Clock {
        fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
            Ok(TrustedTimeSampleV1 {
                canonical_utc_timestamp: "2026-11-12T00:00:00.000000000Z".into(),
                clock_source_id: "test".into(),
                clock_contract_version: "1".into(),
            })
        }
    }
    fn intent() -> RemotePublicationIntentV1 {
        RemotePublicationIntentV1 {
            project_id: "p".into(),
            operation_id: "op".into(),
            repository: "/owned/source".into(),
            remote: "/owned/remote".into(),
            source_ref: "refs/heads/checkpoint/a".into(),
            destination_ref: "refs/heads/checkpoint/a".into(),
            commit: "a".repeat(40),
            wire: "PUSH_ALWAYS".into(),
            mode: "PUSH_ALL_CHECKPOINTS".into(),
            branch_class: "CHECKPOINT".into(),
            acceptance_ref: None,
        }
    }
    #[test]
    fn admitted_immutable_replay_results_reopen_and_guards() {
        let dir = Dir::new("d12");
        let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
        let request = intent();
        let a = auth(&store, "foreign");
        assert!(store.reserve_remote_publication_v1(a, &request).is_err());
        let a = auth(&store, "p");
        assert_eq!(
            store.reserve_remote_publication_v1(a, &request).unwrap(),
            crate::SetupWriteOutcomeV1::Applied
        );
        let a = auth(&store, "p");
        assert_eq!(
            store.reserve_remote_publication_v1(a, &request).unwrap(),
            crate::SetupWriteOutcomeV1::Replayed
        );
        for field in 0..10 {
            let mut changed = request.clone();
            match field {
                0 => changed.repository.push('x'),
                1 => changed.remote.push('x'),
                2 => changed.source_ref.push('x'),
                3 => changed.destination_ref.push('x'),
                4 => changed.commit = "b".repeat(40),
                5 => changed.wire = "LOCAL_ONLY".into(),
                6 => changed.mode = "LOCAL_ONLY".into(),
                7 => changed.branch_class = "TASK".into(),
                8 => changed.acceptance_ref = Some("acceptance:changed".into()),
                _ => changed.project_id = "q".into(),
            }
            let a = auth(&store, "p");
            assert!(store.reserve_remote_publication_v1(a, &changed).is_err());
        }
        assert_eq!(
            store
                .reader()
                .read_remote_publication_v1("p", "op")
                .unwrap()
                .unwrap()
                .outcome(),
            RemotePublicationOutcomeV1::Uncertain
        );
        for (sequence, outcome, observed) in [
            (2, RemotePublicationOutcomeV1::Uncertain, None),
            (
                1,
                RemotePublicationOutcomeV1::Published,
                Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            ),
        ] {
            let a = auth(&store, "p");
            assert!(
                store
                    .append_remote_publication_result_v1(
                        a, "p", "op", sequence, outcome, observed, "d12:test", &Clock
                    )
                    .is_err()
            );
        }
        let a = auth(&store, "p");
        store
            .append_remote_publication_result_v1(
                a,
                "p",
                "op",
                1,
                RemotePublicationOutcomeV1::Uncertain,
                None,
                "d12:test",
                &Clock,
            )
            .unwrap();
        let a = auth(&store, "p");
        assert_eq!(
            store
                .append_remote_publication_result_v1(
                    a,
                    "p",
                    "op",
                    1,
                    RemotePublicationOutcomeV1::Uncertain,
                    None,
                    "d12:test",
                    &Clock
                )
                .unwrap(),
            crate::SetupWriteOutcomeV1::Replayed
        );
        let a = auth(&store, "p");
        assert!(
            store
                .append_remote_publication_result_v1(
                    a,
                    "p",
                    "op",
                    1,
                    RemotePublicationOutcomeV1::Refused,
                    None,
                    "d12:test",
                    &Clock
                )
                .is_err()
        );
        drop(store);
        let store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
        assert_eq!(
            store
                .reader()
                .read_remote_publication_v1("p", "op")
                .unwrap()
                .unwrap()
                .results
                .len(),
            1
        );
        assert!(
            store
                .reader()
                .read_remote_publication_v1("q", "op")
                .unwrap()
                .is_none()
        );
        let conn = rusqlite::Connection::open(dir.db()).unwrap();
        for table in ["remote_publication_intent", "remote_publication_result"] {
            for command in [
                format!("DELETE FROM {table}"),
                format!("UPDATE {table} SET record_json='changed'"),
                format!("INSERT OR REPLACE INTO {table} SELECT * FROM {table}"),
            ] {
                assert!(conn.execute_batch(&command).is_err());
            }
        }
    }
    #[test]
    fn migration_from_v24_is_atomic_and_preserves_ledger() {
        let dir = Dir::new("d12-migration");
        drop(TrustedCoreStore::open_exclusive(dir.db()).unwrap());
        let conn = rusqlite::Connection::open(dir.db()).unwrap();
        conn.execute_batch("DROP TABLE remote_publication_result; DROP TABLE remote_publication_intent; DELETE FROM state_schema_version WHERE version=25; CREATE TRIGGER fail25 BEFORE INSERT ON state_schema_version WHEN NEW.version=25 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
        assert!(TrustedCoreStore::migrate_exclusive(dir.db()).is_err());
        assert_eq!(
            conn.query_row("SELECT MAX(version) FROM state_schema_version", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            24
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name='remote_publication_intent'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        conn.execute_batch("DROP TRIGGER fail25").unwrap();
        drop(conn);
        let (store, head) = TrustedCoreStore::migrate_exclusive(dir.db()).unwrap();
        assert_eq!(head, 25);
        assert_eq!(store.reader().schema_version().unwrap(), 25);
    }
}
