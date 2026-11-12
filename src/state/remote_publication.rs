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
    /// Canonical file identity or credential-free HTTPS/SSH endpoint.
    pub remote: String,
    pub source_ref: String,
    pub destination_ref: String,
    pub commit: String,
    pub wire: String,
    pub mode: String,
    pub branch_class: String,
    /// Immutable acceptance artifact reference supplied by trusted composition.
    pub acceptance_ref: Option<String>,
    /// Explicit user direction, issued only at the trusted workflow boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletion_authorization: Option<String>,
}
/// Non-cloneable single-use capability. Only the trusted store holder may issue it
/// after verifying an explicit user instruction; worker claims are insufficient.
pub struct RemoteDeletionAuthorizationV1 {
    pub(crate) intent: RemotePublicationIntentV1,
}
impl RemoteDeletionAuthorizationV1 {
    pub fn intent(&self) -> &RemotePublicationIntentV1 {
        &self.intent
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RemotePublicationOutcomeV1 {
    Published,
    Deleted,
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
/// Conservative URL grammar: no passwords, tokens, escaping, query or fragment.
/// SSH login names are part of the immutable endpoint identity.
pub fn remote_endpoint_allowed(endpoint: &str) -> bool {
    if endpoint.starts_with('/') {
        return bounded(endpoint);
    }
    let (ssh, rest) = if let Some(rest) = endpoint.strip_prefix("https://") {
        (false, rest)
    } else if let Some(rest) = endpoint.strip_prefix("ssh://") {
        (true, rest)
    } else {
        return false;
    };
    if !bounded(endpoint) || !endpoint.is_ascii() {
        return false;
    }
    let Some((authority, path)) = rest.split_once('/') else {
        return false;
    };
    let host = if let Some((login, host)) = authority.split_once('@') {
        if !ssh
            || login.is_empty()
            || !login
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return false;
        }
        host
    } else {
        authority
    };
    let mut parts = host.split(':');
    let name = parts.next().unwrap_or_default();
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
    {
        return false;
    }
    if let Some(port) = parts.next()
        && port.parse::<u16>().ok().is_none_or(|p| p == 0)
    {
        return false;
    }
    parts.next().is_none()
        && !path.is_empty()
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._-~".contains(&b))
        && !path.split('/').any(|part| part == ".." || part == ".")
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
            || !remote_endpoint_allowed(&self.remote)
            || self
                .deletion_authorization
                .as_deref()
                .is_some_and(|id| id != self.operation_id || !bounded(id))
            || (self.deletion_authorization.is_some()
                && matches!(
                    self.source_ref.as_str(),
                    "refs/heads/main" | "refs/heads/master"
                ))
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
    fn check_remote_ref_history(
        &self,
        intent: &RemotePublicationIntentV1,
    ) -> Result<(), StateError> {
        let mut stmt = self
            .connection()
            .prepare("SELECT operation_id FROM remote_publication_intent WHERE project_id=?1")
            .map_err(sql)?;
        let ids = stmt
            .query_map([&intent.project_id], |r| r.get::<_, String>(0))
            .map_err(sql)?;
        for id in ids {
            let record = self
                .read_remote_publication_v1(&intent.project_id, &id.map_err(sql)?)?
                .ok_or_else(invalid)?;
            if record.intent.operation_id == intent.operation_id {
                continue;
            }
            let old = &record.intent;
            if old.repository != intent.repository
                || old.remote != intent.remote
                || old.destination_ref != intent.destination_ref
            {
                continue;
            }
            if old.deletion_authorization.is_some() {
                if record.outcome() == RemotePublicationOutcomeV1::Uncertain {
                    return Err(conflict());
                }
            } else if intent.deletion_authorization.is_some()
                && (old.wire == "PUSH_ALWAYS"
                    || record.outcome() != RemotePublicationOutcomeV1::Published)
            {
                // Without a durable acceptance artifact, checkpoint evidence is retained.
                return Err(conflict());
            }
        }
        Ok(())
    }
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
            if (result.outcome == RemotePublicationOutcomeV1::Deleted
                && (intent.deletion_authorization.is_none() || result.observed_commit.is_some()))
                || (result.outcome == RemotePublicationOutcomeV1::Published
                    && intent.deletion_authorization.is_some())
                || result.sequence as i64 != sequence
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
        // The exclusive TrustedCoreStore writer is held across remote execution.
        // Ref history is read from State, never supplied by the caller.
        self.check_remote_ref_history(intent)?;
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
        if (outcome == RemotePublicationOutcomeV1::Deleted
            && (record.intent.deletion_authorization.is_none() || observed_commit.is_some()))
            || (outcome == RemotePublicationOutcomeV1::Published
                && record.intent.deletion_authorization.is_some())
        {
            return Err(invalid());
        }
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
            deletion_authorization: None,
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
        assert!(
            !serde_json::to_string(&request)
                .unwrap()
                .contains("deletion_authorization")
        );
        let a = auth(&store, "p");
        assert!(store.authorize_remote_deletion_v1(a, &request).is_err());
        for field in 0..11 {
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
                9 => changed.project_id = "q".into(),
                _ => changed.deletion_authorization = Some(changed.operation_id.clone()),
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
        conn.execute_batch("DROP TABLE remote_publication_result; DROP TABLE remote_publication_intent; DELETE FROM state_schema_version WHERE version>=25; CREATE TRIGGER fail25 BEFORE INSERT ON state_schema_version WHEN NEW.version=25 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
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
        assert_eq!(head, 26);
        assert_eq!(store.reader().schema_version().unwrap(), 26);
    }
}

#[cfg(test)]
mod endpoint_tests {
    use super::*;
    #[test]
    fn endpoint_identity_preserves_login_and_refuses_credentials_and_overrides() {
        for value in [
            "/owned/remote",
            "https://git.example:443/repo.git",
            "ssh://alice@git.example:22/repo.git",
            "ssh://bob@git.example/repo.git",
        ] {
            assert!(remote_endpoint_allowed(value), "{value}");
        }
        for value in [
            "https://user:password@git.example/repo",
            "https://token@git.example/repo",
            "ssh://user:secret@git.example/repo",
            "https://git.example/repo?token=secret",
            "ssh://git.example/repo#secret",
            "ext::command",
            "file:///tmp/remote",
            "origin",
            "https://git.example/%2e%2e/repo",
            "ssh://git.example/../repo",
            "https://git.example:0/repo",
            "https://git.example:443:22/repo",
        ] {
            assert!(!remote_endpoint_allowed(value), "{value}");
        }
    }
}

#[cfg(test)]
mod workflow_migration_tests {
    use super::*;
    use crate::{TrustedCoreStore, atomic_publication_tests::Dir};
    #[test]
    fn v26_barrier_is_atomic_and_preserves_file_intent_replay() {
        let dir = Dir::new("remote-workflow-version");
        let mut store = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
        let intent = RemotePublicationIntentV1 {
            project_id: "p".into(),
            operation_id: "legacy-file".into(),
            repository: "/owned/repo".into(),
            remote: "/owned/remote".into(),
            source_ref: "refs/heads/task/a".into(),
            destination_ref: "refs/heads/task/a".into(),
            commit: "a".repeat(40),
            wire: "PUSH_ON_ACCEPT".into(),
            mode: "PUSH_ACCEPTED_A3".into(),
            branch_class: "TASK".into(),
            acceptance_ref: Some("acceptance:exact".into()),
            deletion_authorization: None,
        };
        let admission = store.admit(crate::StateWriteSubjectV1 {
            operation: crate::StateWriteOperationV1::RemotePublication,
            project_id: "p".into(),
            graph_id: None,
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        });
        store
            .reserve_remote_publication_v1(admission, &intent)
            .unwrap();
        drop(store);
        let conn = rusqlite::Connection::open(dir.db()).unwrap();
        let before: String = conn
            .query_row(
                "SELECT record_json FROM remote_publication_intent",
                [],
                |r| r.get(0),
            )
            .unwrap();
        conn.execute_batch("DELETE FROM state_schema_version WHERE version=26; CREATE TRIGGER fail26 BEFORE INSERT ON state_schema_version WHEN NEW.version=26 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
        assert!(TrustedCoreStore::open_exclusive(dir.db()).is_err());
        assert!(TrustedCoreStore::migrate_exclusive(dir.db()).is_err());
        assert_eq!(
            conn.query_row("SELECT MAX(version) FROM state_schema_version", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            25
        );
        conn.execute_batch("DROP TRIGGER fail26").unwrap();
        let (mut store, version) = TrustedCoreStore::migrate_exclusive(dir.db()).unwrap();
        assert_eq!(version, 26);
        assert_eq!(
            before,
            conn.query_row(
                "SELECT record_json FROM remote_publication_intent",
                [],
                |r| r.get::<_, String>(0)
            )
            .unwrap()
        );
        assert!(
            graph::verify_v25(&conn, GraphPhase::Read).is_err(),
            "old writer must refuse the new ledger"
        );
        let admission = store.admit(crate::StateWriteSubjectV1 {
            operation: crate::StateWriteOperationV1::RemotePublication,
            project_id: "p".into(),
            graph_id: None,
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        });
        assert_eq!(
            store
                .reserve_remote_publication_v1(admission, &intent)
                .unwrap(),
            crate::SetupWriteOutcomeV1::Replayed
        );
    }
}
