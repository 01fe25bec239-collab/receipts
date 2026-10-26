use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{AttemptFailureCode as Code, AttemptField as Field};
use crate::{
    AttemptFinish, AttemptId, AttemptRecord, AttemptRole, AttemptStart, GraphCompilerRefV1,
    GraphGenesisReasonV1, GraphSourceEvidenceRefV1, ReadOnlyGraphReader, SqliteStateRepository,
    StateError, StateGraphGenesisProvenanceV1, StateGraphGenesisV1, StateGraphNodeV1,
    TrustedClockV1, TrustedTimeSampleV1,
};

static NEXT_DB: AtomicU64 = AtomicU64::new(0);
const T0: &str = "2026-10-25T10:00:00.000000000Z";
const T1: &str = "2026-10-25T10:00:01.000000000Z";
const ID_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const ID_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
const ID_C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAX";
const NODE: &str = "n\0😀";
const GRAPH: &str = "g\0😀";

struct TempDb(PathBuf);
impl TempDb {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "receipts-a3-037-{}-{nonce}-{}.db",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for TempDb {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

struct Clock;
impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: T0.into(),
            clock_source_id: "synthetic-clock".into(),
            clock_contract_version: "1".into(),
        })
    }
}

fn aid(value: &str) -> AttemptId {
    AttemptId::parse(value).unwrap()
}
fn start_in(id: &str, project: &str, graph: &str, node: &str, number: &str) -> AttemptStart {
    AttemptStart::new(
        aid(id),
        project,
        graph,
        node,
        number,
        AttemptRole::RuntimeA3,
        "runtime:claude",
        "workspace-1",
        &"a".repeat(40),
        T0,
    )
    .unwrap()
}
fn start(id: &str, number: &str) -> AttemptStart {
    start_in(id, "project-a", GRAPH, NODE, number)
}
fn finish(id: &str) -> AttemptFinish {
    AttemptFinish::new(
        aid(id),
        T1,
        Some(&"b".repeat(40)),
        Some("outcome:result-1"),
        None,
    )
    .unwrap()
}
fn list(repo: &SqliteStateRepository) -> Vec<AttemptRecord> {
    repo.list_attempts_for_node("project-a", GRAPH, NODE)
        .unwrap()
}
fn rows(repo: &SqliteStateRepository) -> (i64, i64) {
    (
        repo.count_table_rows("attempt_started").unwrap(),
        repo.count_table_rows("attempt_finished").unwrap(),
    )
}
fn attempt_error(error: StateError) -> (Code, Option<Field>) {
    let StateError::AttemptPersistence { code, field } = error else {
        panic!("unexpected error kind");
    };
    (code, field)
}

fn attempt_row_image(repo: &SqliteStateRepository) -> Vec<Vec<rusqlite::types::Value>> {
    let mut image = Vec::new();
    for table in ["attempt_started", "attempt_finished"] {
        let mut statement = repo
            .connection()
            .prepare(&format!("SELECT * FROM {table} ORDER BY attempt_id"))
            .unwrap();
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                (0..columns).map(|column| row.get(column)).collect()
            })
            .unwrap();
        image.extend(rows.map(Result::unwrap));
    }
    image
}

#[test]
fn start_finish_find_list_survive_reopen() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    assert_eq!(repo.schema_version().unwrap(), 14);
    let huge = format!("1{}", "0".repeat(10000));
    // Appended out of numeric order; listing is numeric, not lexical.
    for (id, number) in [(ID_C, huge.as_str()), (ID_B, "10"), (ID_A, "9")] {
        repo.append_attempt_started(start(id, number)).unwrap();
    }
    repo.append_attempt_finished(finish(ID_A)).unwrap();
    drop(repo);

    let repo = SqliteStateRepository::open(&db.0).unwrap();
    let found = repo.find_attempt(&aid(ID_A)).unwrap().unwrap();
    assert_eq!(found.attempt_id().as_str(), ID_A);
    assert_eq!(found.project_id(), "project-a");
    assert_eq!(found.graph_id(), GRAPH);
    assert_eq!(found.node_id(), NODE);
    assert_eq!(found.attempt_number(), "9");
    assert_eq!(found.role(), AttemptRole::RuntimeA3);
    assert_eq!(found.runtime_ref(), "runtime:claude");
    assert_eq!(found.workspace_id(), "workspace-1");
    assert_eq!(found.start_sha(), "a".repeat(40));
    assert_eq!(found.started_at().as_str(), T0);
    assert!(found.is_finished());
    assert_eq!(found.ended_at().unwrap().as_str(), T1);
    assert_eq!(found.end_sha(), Some("b".repeat(40).as_str()));
    assert_eq!(found.outcome_ref(), Some("outcome:result-1"));
    assert_eq!(found.failure_class(), None);

    let unfinished = repo.find_attempt(&aid(ID_B)).unwrap().unwrap();
    assert!(!unfinished.is_finished());
    assert_eq!(unfinished.ended_at(), None);
    assert!(
        repo.find_attempt(&aid("01ARZ3NDEKTSV4RRFFQ69G5FAY"))
            .unwrap()
            .is_none()
    );

    let listed = list(&repo);
    let numbers: Vec<_> = listed.iter().map(|a| a.attempt_number()).collect();
    assert_eq!(numbers, ["9", "10", huge.as_str()]);
    assert_eq!(listed[0], found);
}

#[test]
fn opaque_strings_round_trip_exactly_and_preserve_optional_absence() {
    for value in [
        "".to_owned(),
        "two words".into(),
        "运行 😀".into(),
        "a\0b".into(),
        "r".repeat(10001),
    ] {
        let db = TempDb::new();
        let mut repo = SqliteStateRepository::open(&db.0).unwrap();
        let started = AttemptStart::new(
            aid(ID_A),
            "project-a",
            GRAPH,
            NODE,
            "1",
            AttemptRole::RuntimeA3,
            &value,
            "workspace-1",
            &"a".repeat(40),
            T0,
        )
        .unwrap();
        let finished = AttemptFinish::new(aid(ID_A), T1, None, Some(&value), Some(&value)).unwrap();
        repo.append_attempt_started(started).unwrap();
        repo.append_attempt_finished(finished.clone()).unwrap();
        repo.append_attempt_started(start(ID_B, "1")).unwrap();
        repo.append_attempt_finished(AttemptFinish::new(aid(ID_B), T1, None, None, None).unwrap())
            .unwrap();
        let before = list(&repo);
        drop(repo);

        let mut repo = SqliteStateRepository::open(&db.0).unwrap();
        let found = repo.find_attempt(&aid(ID_A)).unwrap().unwrap();
        assert_eq!(found.runtime_ref(), value);
        assert_eq!(found.outcome_ref(), Some(value.as_str()));
        assert_eq!(found.failure_class(), Some(value.as_str()));
        let absent = repo.find_attempt(&aid(ID_B)).unwrap().unwrap();
        assert_eq!(absent.outcome_ref(), None);
        assert_eq!(absent.failure_class(), None);
        assert_eq!(list(&repo), before);
        assert_eq!(list(&repo), vec![found, absent]);
        assert_eq!(format!("{finished:?}"), "AttemptFinish");
        assert_eq!(
            attempt_error(repo.append_attempt_finished(finished).unwrap_err()),
            (Code::DuplicateFinish, None)
        );
        for (outcome, class) in [(None, Some(value.as_str())), (Some(value.as_str()), None)] {
            let conflict = AttemptFinish::new(aid(ID_A), T1, None, outcome, class).unwrap();
            assert_eq!(
                attempt_error(repo.append_attempt_finished(conflict).unwrap_err()),
                (Code::ConflictingFinish, None)
            );
        }
        assert_eq!(list(&repo), before);
        assert_eq!(rows(&repo), (2, 2));
    }
}

#[test]
fn shared_ordinals_across_and_within_roles_sort_by_attempt_id() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    for (id, role) in [
        (ID_C, AttemptRole::RuntimeA3),
        (ID_B, AttemptRole::RuntimeA4),
        (ID_A, AttemptRole::RuntimeA3),
    ] {
        let started = AttemptStart::new(
            aid(id),
            "project-a",
            GRAPH,
            NODE,
            "10",
            role,
            "runtime",
            "workspace-1",
            &"a".repeat(40),
            T0,
        )
        .unwrap();
        repo.append_attempt_started(started).unwrap();
    }
    drop(repo);
    let repo = SqliteStateRepository::open(&db.0).unwrap();
    let listed = list(&repo);
    assert_eq!(
        listed
            .iter()
            .map(|a| a.attempt_id().as_str())
            .collect::<Vec<_>>(),
        [ID_A, ID_B, ID_C]
    );
    for record in listed {
        assert_eq!(record.attempt_number(), "10");
        assert_eq!(
            repo.find_attempt(record.attempt_id()).unwrap(),
            Some(record)
        );
    }
    assert_eq!(rows(&repo), (3, 0));
}

#[test]
fn attempts_are_isolated_by_project_graph_and_node() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.append_attempt_started(start(ID_A, "1")).unwrap();
    // Same graph, node and number in other scopes does not collide.
    repo.append_attempt_started(start_in(ID_B, "project-b", GRAPH, NODE, "1"))
        .unwrap();
    repo.append_attempt_started(start_in(ID_C, "project-a", "other", NODE, "1"))
        .unwrap();
    assert_eq!(list(&repo).len(), 1);
    assert_eq!(
        repo.list_attempts_for_node("project-b", GRAPH, NODE)
            .unwrap()
            .len(),
        1
    );
    assert!(
        repo.list_attempts_for_node("project-a", GRAPH, "other-node")
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        attempt_error(repo.list_attempts_for_node("", GRAPH, NODE).unwrap_err()),
        (Code::InvalidShape, Some(Field::ProjectId))
    );
}

#[test]
fn attempt_finish_without_start_is_refused() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.append_attempt_started(start(ID_A, "1")).unwrap();
    let before = list(&repo);
    assert_eq!(
        attempt_error(repo.append_attempt_finished(finish(ID_B)).unwrap_err()),
        (Code::UnknownAttempt, Some(Field::AttemptId))
    );
    assert_eq!(rows(&repo), (1, 0));
    assert_eq!(list(&repo), before);
}

#[test]
fn duplicate_start_is_refused_and_preserves_prior_rows() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.append_attempt_started(start(ID_A, "1")).unwrap();
    repo.append_attempt_finished(finish(ID_A)).unwrap();
    let before = list(&repo);
    // Repeated identity is refused even when its ordinal changes.
    for duplicate in [start(ID_A, "1"), start(ID_A, "2")] {
        assert_eq!(
            attempt_error(repo.append_attempt_started(duplicate).unwrap_err()),
            (Code::DuplicateStart, None)
        );
    }
    assert_eq!(rows(&repo), (1, 1));
    assert_eq!(list(&repo), before);
}

#[test]
fn duplicate_and_conflicting_finishes_are_refused_and_preserve_prior_rows() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.append_attempt_started(start(ID_A, "1")).unwrap();
    repo.append_attempt_finished(finish(ID_A)).unwrap();
    let before = list(&repo);
    assert_eq!(
        attempt_error(repo.append_attempt_finished(finish(ID_A)).unwrap_err()),
        (Code::DuplicateFinish, None)
    );
    let different = AttemptFinish::new(aid(ID_A), T1, None, None, Some("TIMEOUT")).unwrap();
    assert_eq!(
        attempt_error(repo.append_attempt_finished(different).unwrap_err()),
        (Code::ConflictingFinish, None)
    );
    assert_eq!(rows(&repo), (1, 1));
    assert_eq!(list(&repo), before);
}

#[test]
fn finish_before_start_time_is_refused() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.append_attempt_started(start(ID_A, "1")).unwrap();
    let early = AttemptFinish::new(
        aid(ID_A),
        "2026-10-25T09:59:59.999999999Z",
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(
        attempt_error(repo.append_attempt_finished(early).unwrap_err()),
        (Code::InvalidShape, Some(Field::EndedAt))
    );
    assert_eq!(rows(&repo), (1, 0));
    // A failed attempt records a class label and no end SHA.
    let failed = AttemptFinish::new(aid(ID_A), T0, None, None, Some("RUNTIME_TIMEOUT")).unwrap();
    repo.append_attempt_finished(failed).unwrap();
    let record = repo.find_attempt(&aid(ID_A)).unwrap().unwrap();
    assert_eq!(record.failure_class(), Some("RUNTIME_TIMEOUT"));
    assert_eq!(record.end_sha(), None);
}

#[test]
fn persisted_finish_times_are_validated_by_both_read_apis() {
    for ended_at in ["2026-10-25T09:59:59.999999999Z", T0, T1] {
        let db = TempDb::new();
        let mut repo = SqliteStateRepository::open(&db.0).unwrap();
        repo.append_attempt_started(start(ID_A, "1")).unwrap();
        repo.connection()
            .execute(
                "INSERT INTO attempt_finished(attempt_id,ended_at) VALUES (?1,?2)",
                rusqlite::params![ID_A, ended_at],
            )
            .unwrap();
        drop(repo);
        let repo = SqliteStateRepository::open(&db.0).unwrap();
        let found = repo.find_attempt(&aid(ID_A));
        let listed = repo.list_attempts_for_node("project-a", GRAPH, NODE);
        if ended_at < T0 {
            assert_eq!(
                attempt_error(found.unwrap_err()),
                (Code::CorruptStore, None)
            );
            assert_eq!(
                attempt_error(listed.unwrap_err()),
                (Code::CorruptStore, None)
            );
        } else {
            let found = found.unwrap().unwrap();
            assert_eq!(found.ended_at().unwrap().as_str(), ended_at);
            assert_eq!(listed.unwrap(), vec![found]);
        }
        assert_eq!(rows(&repo), (1, 1));
    }
}

#[test]
fn ordinary_open_refuses_missing_or_altered_attempt_immutability_triggers() {
    use crate::error::{GraphFailureCode, GraphPhase};
    for table in ["attempt_started", "attempt_finished"] {
        for operation in ["update", "delete", "replace"] {
            for altered in [false, true] {
                let db = TempDb::new();
                let mut repo = SqliteStateRepository::open(&db.0).unwrap();
                repo.append_attempt_started(start(ID_A, "1")).unwrap();
                repo.append_attempt_finished(finish(ID_A)).unwrap();
                let before = list(&repo);
                let trigger = format!("{table}_no_{operation}");
                repo.connection()
                    .execute_batch(&format!("DROP TRIGGER {trigger}"))
                    .unwrap();
                if altered {
                    repo.connection()
                        .execute_batch(&format!(
                            "CREATE TRIGGER {trigger} BEFORE INSERT ON {table} BEGIN SELECT 1; END;"
                        ))
                        .unwrap();
                }
                drop(repo);
                assert!(matches!(
                    SqliteStateRepository::open(&db.0).unwrap_err(),
                    StateError::GraphPersistence {
                        phase: GraphPhase::Read,
                        code: GraphFailureCode::CorruptStore,
                        ..
                    }
                ));
                // The failed ordinary open must not repair or rewrite the store.
                let repo = SqliteStateRepository::open_with_migrations(
                    &db.0,
                    crate::migrations::registered(),
                )
                .unwrap();
                assert_eq!(rows(&repo), (1, 1));
                assert_eq!(repo.schema_version().unwrap(), 14);
                repo.connection()
                    .execute_batch(&format!("DROP TRIGGER IF EXISTS {trigger}"))
                    .unwrap();
                let original = crate::migrations::v0014_execution_records::SQL
                    .split(";\n")
                    .find(|sql| {
                        sql.trim_start()
                            .starts_with(&format!("CREATE TRIGGER {trigger} "))
                    })
                    .unwrap();
                repo.connection().execute_batch(original).unwrap();
                assert_eq!(list(&repo), before);
            }
        }
    }
}

#[test]
fn attempt_rows_cannot_be_updated_or_deleted_sql_trigger() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.append_attempt_started(start(ID_A, "1")).unwrap();
    repo.append_attempt_finished(finish(ID_A)).unwrap();
    let before = list(&repo);
    let sha = "c".repeat(40);
    let assert_trigger = |error: rusqlite::Error| {
        let rusqlite::Error::SqliteFailure(code, message) = error else {
            panic!("expected a trigger constraint, got {error:?}");
        };
        assert_eq!(code.code, rusqlite::ErrorCode::ConstraintViolation);
        assert_eq!(code.extended_code, rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER);
        assert_eq!(message.as_deref(), Some("attempt rows are append-only"));
        assert_eq!(rows(&repo), (1, 1));
        assert_eq!(list(&repo), before);
    };
    for statement in [
        "UPDATE attempt_started SET workspace_id='x' WHERE attempt_id=?1",
        "DELETE FROM attempt_started WHERE attempt_id=?1",
        "UPDATE attempt_finished SET end_sha=NULL WHERE attempt_id=?1",
        "DELETE FROM attempt_finished WHERE attempt_id=?1",
    ] {
        assert_trigger(repo.connection().execute(statement, [ID_A]).unwrap_err());
    }
    for (project, graph, node) in [("p", "g", "n"), ("project-a", GRAPH, NODE)] {
        assert_trigger(repo.connection().execute(
            "INSERT OR REPLACE INTO attempt_started(attempt_id,project_id,graph_id,node_id,attempt_number,role,runtime_ref,workspace_id,start_sha,started_at) VALUES (?1,?2,?3,?4,'1','RUNTIME_A4','r','w',?5,?6)",
            rusqlite::params![ID_A, project, graph, node, sha, T0],
        ).unwrap_err());
    }
    assert_trigger(
        repo.connection()
            .execute(
                "INSERT OR REPLACE INTO attempt_finished(attempt_id,ended_at) VALUES (?1,?2)",
                rusqlite::params![ID_A, T1],
            )
            .unwrap_err(),
    );
    assert_eq!(rows(&repo), (1, 1));
    assert_eq!(list(&repo), before);
}

#[test]
fn malformed_inputs_are_refused_without_echoing_them() {
    let sha = "a".repeat(40);
    let sentinel = "SENTINEL secret prompt text";
    let start = |id: &str, project: &str, number: &str, runtime: &str, sha: &str, at: &str| {
        AttemptStart::new(
            aid(id),
            project,
            "g",
            "n",
            number,
            AttemptRole::RuntimeA4,
            runtime,
            "w",
            sha,
            at,
        )
    };
    let cases = [
        (start(ID_A, "", "1", "r", &sha, T0), Field::ProjectId),
        (
            start(ID_A, &"p".repeat(201), "1", "r", &sha, T0),
            Field::ProjectId,
        ),
        (start(ID_A, "p", "0", "r", &sha, T0), Field::AttemptNumber),
        (start(ID_A, "p", "01", "r", &sha, T0), Field::AttemptNumber),
        (start(ID_A, "p", "1x", "r", &sha, T0), Field::AttemptNumber),
        (
            start(ID_A, sentinel, "0", sentinel, &sha, T0),
            Field::AttemptNumber,
        ),
        (
            start(ID_A, "p", "1", "r", &sha.to_uppercase(), T0),
            Field::StartSha,
        ),
        (start(ID_A, "p", "1", "r", "abc", T0), Field::StartSha),
        (
            start(ID_A, "p", "1", "r", &sha, "2026-10-25"),
            Field::StartedAt,
        ),
        (
            start(ID_A, "p", "1", "r", &sha, "2026-13-25T10:00:00.000000000Z"),
            Field::StartedAt,
        ),
    ];
    for (result, field) in cases {
        let error = result.unwrap_err();
        let text = format!("{error} {error:?}");
        assert!(
            !text.contains("SENTINEL") && !text.contains("secret"),
            "{text}"
        );
        assert!(error.source().is_none());
        assert_eq!(attempt_error(error), (Code::InvalidShape, Some(field)));
    }
    for bad in [
        "",
        "01ARZ3NDEKTSV4RRFFQ69G5FA",
        "81ARZ3NDEKTSV4RRFFQ69G5FAV",
        "01arz3ndektsv4rrffq69g5fav",
        "01ARZ3NDEKTSV4RRFFQ69G5FAI",
        "01ARZ3NDEKTSV4RRFFQ69G5FA\0",
    ] {
        let error = AttemptId::parse(bad).unwrap_err();
        assert_eq!(
            attempt_error(error),
            (Code::InvalidShape, Some(Field::AttemptId))
        );
    }
    let finish_cases = [
        AttemptFinish::new(aid(ID_A), T1, Some("abc"), None, None),
        AttemptFinish::new(aid(ID_A), "bad", None, None, None),
    ];
    let fields = [Field::EndSha, Field::EndedAt];
    for (result, field) in finish_cases.into_iter().zip(fields) {
        assert_eq!(
            attempt_error(result.unwrap_err()),
            (Code::InvalidShape, Some(field))
        );
    }
    let record_debug = format!("{:?}", start(ID_A, "p", "1", sentinel, &sha, T0).unwrap());
    assert_eq!(record_debug, "AttemptStart");
    assert_eq!(format!("{:?}", aid(ID_A)), "AttemptId");
}

#[test]
fn tampered_store_fails_closed_without_writing() {
    for (tamper, expected) in [
        (
            "UPDATE state_schema_version SET migration_name='tampered' WHERE version=14",
            Code::CorruptStore,
        ),
        (
            "DROP TRIGGER attempt_finished_no_delete",
            Code::CorruptStore,
        ),
        (
            "DROP TRIGGER attempt_started_no_replace",
            Code::CorruptStore,
        ),
        (
            "INSERT INTO state_schema_version(version,migration_name) VALUES (15,'future')",
            Code::UnsupportedSchema,
        ),
    ] {
        let db = TempDb::new();
        let mut repo = SqliteStateRepository::open(&db.0).unwrap();
        repo.append_attempt_started(start(ID_A, "1")).unwrap();
        let before = attempt_row_image(&repo);
        repo.connection().execute_batch(tamper).unwrap();
        for result in [
            repo.append_attempt_started(start(ID_B, "2")),
            repo.append_attempt_finished(finish(ID_A)),
        ] {
            assert_eq!(attempt_error(result.unwrap_err()).0, expected);
        }
        assert_eq!(
            attempt_error(repo.find_attempt(&aid(ID_A)).unwrap_err()).0,
            expected
        );
        assert_eq!(
            attempt_error(
                repo.list_attempts_for_node("project-a", GRAPH, NODE)
                    .unwrap_err()
            ),
            (expected, None)
        );
        assert_eq!(rows(&repo), (1, 0));
        assert_eq!(attempt_row_image(&repo), before);
    }
}

#[test]
fn busy_verification_preserves_both_reads_and_recovers_after_rollback() {
    use crate::error::{GraphFailureCode, GraphPhase};
    use std::time::Duration;

    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.append_attempt_started(start(ID_A, "1")).unwrap();
    repo.append_attempt_finished(finish(ID_A)).unwrap();
    let expected_rows = attempt_row_image(&repo);
    let expected_found = repo.find_attempt(&aid(ID_A)).unwrap();
    let expected_list = list(&repo);

    // Rollback-journal EXCLUSIVE locks block deferred readers, unlike WAL writers.
    assert!(repo.connection().is_autocommit());
    let mode: String = repo
        .connection()
        .query_row("PRAGMA journal_mode=DELETE", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "delete");
    repo.connection().busy_timeout(Duration::ZERO).unwrap();
    let blocker = rusqlite::Connection::open(&db.0).unwrap();
    blocker.busy_timeout(Duration::ZERO).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();

    for error in [
        crate::graph_repository::schema_head(repo.connection(), GraphPhase::Read).unwrap_err(),
        crate::graph_repository::verify_ledger(repo.connection(), 14, GraphPhase::Read)
            .unwrap_err(),
    ] {
        assert!(matches!(
            error,
            StateError::GraphPersistence {
                code: GraphFailureCode::StoreBusy,
                ..
            }
        ));
    }
    let errors = [
        attempt_error(repo.find_attempt(&aid(ID_A)).unwrap_err()),
        attempt_error(
            repo.list_attempts_for_node("project-a", GRAPH, NODE)
                .unwrap_err(),
        ),
    ];
    blocker.execute_batch("ROLLBACK").unwrap();
    assert_eq!(attempt_row_image(&repo), expected_rows);
    assert_eq!(repo.find_attempt(&aid(ID_A)).unwrap(), expected_found);
    assert_eq!(list(&repo), expected_list);
    assert_eq!(errors, [(Code::StoreBusy, None); 2]);
}

#[test]
fn busy_writer_is_closed_and_leaves_rows_unchanged() {
    use std::time::Duration;
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    let blocker = rusqlite::Connection::open(&db.0).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    repo.connection()
        .busy_timeout(Duration::from_millis(30))
        .unwrap();
    let error = repo.append_attempt_started(start(ID_A, "1")).unwrap_err();
    assert_eq!(attempt_error(error), (Code::StoreBusy, None));
    blocker.execute_batch("ROLLBACK").unwrap();
    assert_eq!(rows(&repo), (0, 0));
    repo.append_attempt_started(start(ID_A, "1")).unwrap();
}

fn graph_request(project: &str, graph: &str) -> StateGraphGenesisV1 {
    let provenance = StateGraphGenesisProvenanceV1::new(
        GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
        GraphGenesisReasonV1::InitialCompilation,
    );
    let node = StateGraphNodeV1::new(
        "n1".into(),
        graph.into(),
        "TASK".into(),
        "PLANNED".into(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    StateGraphGenesisV1::new(
        project.into(),
        graph.into(),
        "goal".into(),
        "policy".into(),
        None,
        None,
        None,
        "a".repeat(64),
        vec![node],
        vec![],
        provenance,
    )
    .unwrap()
}

/// Rewinds a v14 store to the exact v13 image (graph tables and data untouched).
fn rewind_to_v13(path: &std::path::Path) {
    rusqlite::Connection::open(path)
        .unwrap()
        .execute_batch(
            "DROP TABLE attempt_finished; DROP TABLE attempt_started;
             DELETE FROM state_schema_version WHERE version=14;",
        )
        .unwrap();
}

fn graph_image(path: &std::path::Path) -> Vec<String> {
    let conn = rusqlite::Connection::open(path).unwrap();
    let mut image = Vec::new();
    for table in [
        "graphs",
        "graph_versions",
        "graph_nodes",
        "graph_edges",
        "graph_node_capabilities",
        "graph_compiled_sources",
        "trusted_time_watermark",
    ] {
        let ddl: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name=?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        image.push(ddl);
        let mut statement = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns = statement.column_count();
        let dump = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|i| {
                        row.get::<_, rusqlite::types::Value>(i)
                            .map(|v| format!("{v:?}"))
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        image.push(format!("{dump:?}"));
    }
    image
}

#[test]
fn v13_store_migrates_to_v14_preserving_graph_genesis() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.create_initial_graph_v1(&Clock, graph_request("project-a", "g1"))
        .unwrap();
    drop(repo);
    rewind_to_v13(&db.0);
    let before = graph_image(&db.0);

    // Ordinary open never upgrades; the attempt API is not usable at v13.
    assert!(matches!(
        SqliteStateRepository::open(&db.0).unwrap_err(),
        StateError::SchemaVersionMismatch {
            found: 13,
            supported: 14
        }
    ));
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(&db.0).unwrap(),
        14
    );
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(&db.0).unwrap(),
        14
    );
    assert_eq!(graph_image(&db.0), before);

    let reader = ReadOnlyGraphReader::open_existing(&db.0).unwrap();
    let graph = reader.read_current_v1("project-a", "g1").unwrap().unwrap();
    assert_eq!(graph.current_version(), "1");
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.create_initial_graph_v1(&Clock, graph_request("project-a", "g2"))
        .unwrap();
    assert!(reader.read_current_v1("project-a", "g2").unwrap().is_some());
    repo.append_attempt_started(start(ID_A, "1")).unwrap();
    repo.append_attempt_finished(finish(ID_A)).unwrap();
    assert!(
        repo.find_attempt(&aid(ID_A))
            .unwrap()
            .unwrap()
            .is_finished()
    );
}

#[test]
fn v14_migration_failures_and_corrupt_ledgers_fail_closed() {
    use crate::error::{GraphFailureCode, GraphPhase};
    let ledger = |path: &std::path::Path| -> i64 {
        rusqlite::Connection::open(path)
            .unwrap()
            .query_row("SELECT MAX(version) FROM state_schema_version", [], |r| {
                r.get(0)
            })
            .unwrap()
    };
    // A pre-existing conflicting object makes the v14 step fail; it rolls back.
    let db = TempDb::new();
    drop(SqliteStateRepository::open(&db.0).unwrap());
    rewind_to_v13(&db.0);
    rusqlite::Connection::open(&db.0)
        .unwrap()
        .execute_batch("CREATE TABLE attempt_finished(x)")
        .unwrap();
    let error = SqliteStateRepository::migrate_existing_to_current(&db.0).unwrap_err();
    assert!(matches!(
        error,
        StateError::GraphPersistence {
            phase: GraphPhase::Migration,
            ..
        }
    ));
    assert_eq!(ledger(&db.0), 13);
    assert!(
        rusqlite::Connection::open(&db.0)
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='attempt_started'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap()
            == 0
    );

    // A corrupt v13 ledger is refused and stays v13.
    let db = TempDb::new();
    drop(SqliteStateRepository::open(&db.0).unwrap());
    rewind_to_v13(&db.0);
    rusqlite::Connection::open(&db.0)
        .unwrap()
        .execute_batch("UPDATE state_schema_version SET migration_name='wrong' WHERE version=5")
        .unwrap();
    assert!(matches!(
        SqliteStateRepository::migrate_existing_to_current(&db.0).unwrap_err(),
        StateError::GraphPersistence {
            code: GraphFailureCode::CorruptStore,
            ..
        }
    ));
    assert_eq!(ledger(&db.0), 13);

    // Newer and corrupt v14 stores are refused by open and maintenance.
    let db = TempDb::new();
    drop(SqliteStateRepository::open(&db.0).unwrap());
    rusqlite::Connection::open(&db.0)
        .unwrap()
        .execute_batch(
            "INSERT INTO state_schema_version(version,migration_name) VALUES (15,'future')",
        )
        .unwrap();
    assert!(SqliteStateRepository::open(&db.0).is_err());
    assert!(SqliteStateRepository::migrate_existing_to_current(&db.0).is_err());
    let db = TempDb::new();
    drop(SqliteStateRepository::open(&db.0).unwrap());
    rusqlite::Connection::open(&db.0)
        .unwrap()
        .execute_batch("DROP TRIGGER attempt_started_no_update")
        .unwrap();
    assert!(matches!(
        SqliteStateRepository::migrate_existing_to_current(&db.0).unwrap_err(),
        StateError::GraphPersistence {
            code: GraphFailureCode::CorruptStore,
            ..
        }
    ));
    assert!(ReadOnlyGraphReader::open_existing(&db.0).is_err());
}
