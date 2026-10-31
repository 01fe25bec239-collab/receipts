//! Event immutability backstops in migration 17 and their current-head
//! verification. Direct-SQL guards are not a claim to resist a caller who can
//! drop schema objects; such damage is detected on open instead.

use crate::StateReader;
use crate::error::{GraphFailureCode, GraphPhase, StateError};
use crate::event::{
    ActorKind, EventActor, EventEnvelope, EventPayloadReference, EventQuery, EventSubject,
    EventType, SubjectKind,
};
use crate::graph_repository::ReadOnlyGraphReader;
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::tests::{TempDir, state_epoch};

const GUARDS: [&str; 3] = ["event_no_update", "event_no_delete", "event_no_replace"];

/// The exact stored definition of one migration-17 trigger.
fn guard_sql(name: &str) -> &'static str {
    migrations::v0017_decision_records::SQL
        .split(";\n")
        .map(str::trim)
        .find(|s| s.starts_with(&format!("CREATE TRIGGER {name} ")))
        .expect("guard defined by migration 17")
}

/// Corruption fixture: runs `damage` with one event guard deliberately
/// dropped, then restores the exact guard so the store is current again.
/// Production guards are never weakened; this only prepares damaged data.
pub(crate) fn with_guard_disabled<T>(
    repo: &SqliteStateRepository,
    name: &str,
    damage: impl FnOnce(&rusqlite::Connection) -> T,
) -> T {
    let conn = repo.connection();
    conn.execute_batch(&format!("DROP TRIGGER {name}")).unwrap();
    let result = damage(conn);
    conn.execute_batch(guard_sql(name)).unwrap();
    result
}

fn event(n: u8, project: &str) -> EventEnvelope {
    EventEnvelope {
        event_id: format!("01ARZ3NDEKTSV4RRFFQ69G5FA{}", (b'A' + n) as char),
        project_id: project.into(),
        goal_id: Some("goal".into()),
        event_type: EventType::TaskCreated,
        actor: EventActor {
            kind: ActorKind::Role,
            id: Some("r".into()),
        },
        subject: EventSubject {
            kind: SubjectKind::Task,
            id: format!("task-{n}"),
        },
        occurred_at: "2026-10-31T10:00:00.000000000Z".into(),
        payload: EventPayloadReference {
            reference: "ref".into(),
            digest: "d".into(),
        },
        correlation_id: "c".into(),
        epoch: state_epoch(3),
    }
}

/// Every event column of every row, in identity order.
fn image(conn: &rusqlite::Connection) -> Vec<String> {
    let mut statement = conn
        .prepare("SELECT event_id||'|'||project_id||'|'||ifnull(goal_id,'~')||'|'||event_type||'|'||actor_kind||'|'||ifnull(actor_id,'~')||'|'||subject_kind||'|'||subject_id||'|'||occurred_at||'|'||payload_reference||'|'||payload_digest||'|'||correlation_id||'|'||epoch||'|'||typeof(epoch) FROM event ORDER BY event_id")
        .unwrap();
    statement
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn seeded(tag: &str) -> (TempDir, SqliteStateRepository) {
    let tmp = TempDir::new(tag);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    for n in 0..3 {
        repo.append_event(event(n, "p")).unwrap();
    }
    (tmp, repo)
}

const EXISTING: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAA";
const NEW_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAZ";
const COLUMNS: &str = "event_id,project_id,event_type,actor_kind,subject_kind,subject_id,occurred_at,payload_reference,payload_digest,correlation_id,epoch";

fn insert(verb: &str, id: &str, suffix: &str) -> String {
    format!(
        "{verb} INTO event({COLUMNS}) VALUES ('{id}','rewritten','TASK_FAILED','USER','TASK','x','t','r','d','c','9') {suffix}"
    )
}

#[test]
fn sql_cannot_update_delete_or_replace_existing_events() {
    let (tmp, mut repo) = seeded("event-guard-sql");
    let before = image(repo.connection());
    assert_eq!(before.len(), 3);
    let refused = [
        "UPDATE event SET project_id='x'".to_string(),
        format!("UPDATE event SET epoch='5' WHERE event_id='{EXISTING}'"),
        "UPDATE event SET payload_reference='forged'".to_string(),
        format!(
            "UPDATE OR REPLACE event SET event_id='{EXISTING}' WHERE event_id='01ARZ3NDEKTSV4RRFFQ69G5FAB'"
        ),
        "DELETE FROM event".to_string(),
        format!("DELETE FROM event WHERE event_id='{EXISTING}'"),
        insert("INSERT", EXISTING, ""),
        insert("INSERT OR REPLACE", EXISTING, ""),
        insert("REPLACE", EXISTING, ""),
        insert("INSERT OR IGNORE", EXISTING, ""),
        insert("INSERT OR ABORT", EXISTING, ""),
        insert(
            "INSERT",
            EXISTING,
            "ON CONFLICT(event_id) DO UPDATE SET project_id='rewritten'",
        ),
        insert("INSERT", EXISTING, "ON CONFLICT(event_id) DO NOTHING"),
    ];
    for sql in &refused {
        let error = repo.connection().execute_batch(sql).unwrap_err();
        assert!(
            error.to_string().contains("immutable event")
                || error.to_string().contains("duplicate event"),
            "{sql}: {error}"
        );
        assert_eq!(image(repo.connection()), before, "unchanged after: {sql}");
    }
    // The typed duplicate refusal and reads are unchanged.
    assert!(matches!(
        repo.append_event(event(0, "other")),
        Err(StateError::EventAlreadyExists { .. })
    ));
    assert_eq!(repo.find_event(EXISTING).unwrap().unwrap(), event(0, "p"));
    assert_eq!(image(repo.connection()), before);
    // Appending a new identity still works by every insert spelling.
    repo.connection()
        .execute_batch(&insert("INSERT", NEW_ID, ""))
        .unwrap();
    let appended = image(repo.connection());
    assert_eq!(appended.len(), 4);
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert_eq!(image(repo.connection()), appended, "reopen preserves rows");
    assert_eq!(
        repo.list_events(&EventQuery {
            project_id: "p".into(),
            subject: None,
            event_types: None,
            limit: 10
        })
        .unwrap()
        .len(),
        3
    );
}

#[test]
fn replacing_spellings_of_a_new_identity_are_plain_appends() {
    let (_tmp, repo) = seeded("event-guard-new-id");
    for (n, sql) in [
        insert("INSERT OR REPLACE", "01ARZ3NDEKTSV4RRFFQ69G5FB1", ""),
        insert("REPLACE", "01ARZ3NDEKTSV4RRFFQ69G5FB2", ""),
        insert(
            "INSERT",
            "01ARZ3NDEKTSV4RRFFQ69G5FB3",
            "ON CONFLICT(event_id) DO UPDATE SET project_id='x'",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        repo.connection().execute_batch(&sql).unwrap();
        assert_eq!(image(repo.connection()).len(), 4 + n);
    }
}

#[test]
fn missing_or_altered_event_guards_are_detected_by_every_current_head_check() {
    for (index, name) in GUARDS.iter().enumerate() {
        for altered in [false, true] {
            let (tmp, repo) = seeded(&format!("event-guard-damage-{index}-{altered}"));
            repo.connection()
                .execute_batch(&format!("DROP TRIGGER {name}"))
                .unwrap();
            if altered {
                let event = if *name == "event_no_update" {
                    "UPDATE"
                } else if *name == "event_no_delete" {
                    "DELETE"
                } else {
                    "INSERT"
                };
                repo.connection()
                    .execute_batch(&format!(
                        "CREATE TRIGGER {name} BEFORE {event} ON event BEGIN SELECT 1; END"
                    ))
                    .unwrap();
            }
            drop(repo);
            let corrupt = |error: StateError| {
                matches!(
                    error,
                    StateError::GraphPersistence {
                        code: GraphFailureCode::CorruptStore,
                        ..
                    }
                )
            };
            assert!(
                corrupt(SqliteStateRepository::open(tmp.db_path()).unwrap_err()),
                "{name} open"
            );
            assert!(
                corrupt(StateReader::open_existing(tmp.db_path()).unwrap_err()),
                "{name} reader"
            );
            assert!(
                corrupt(ReadOnlyGraphReader::open_existing(tmp.db_path()).unwrap_err()),
                "{name} graph reader"
            );
            assert!(
                corrupt(
                    SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap_err()
                ),
                "{name} migrate"
            );
        }
    }
    // An extra trigger on the event table is also not the exact current head.
    let (tmp, repo) = seeded("event-guard-extra");
    repo.connection()
        .execute_batch("CREATE TRIGGER event_extra BEFORE INSERT ON event BEGIN SELECT 1; END")
        .unwrap();
    drop(repo);
    assert!(SqliteStateRepository::open(tmp.db_path()).is_err());
}

#[test]
fn v16_store_with_events_migrates_to_guarded_v17_preserving_rows() {
    let (tmp, repo) = seeded("event-guard-v16");
    let before = image(repo.connection());
    crate::decision_records_tests::remove_v17(repo.connection());
    // The exact v16 image has no event guard: the intermediate postcondition.
    crate::graph_repository::verify_v16(repo.connection(), GraphPhase::Read).unwrap();
    repo.connection()
        .execute_batch("UPDATE event SET project_id=project_id")
        .unwrap();
    drop(repo);
    assert!(SqliteStateRepository::open(tmp.db_path()).is_err());
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
        17
    );
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert_eq!(image(repo.connection()), before);
    assert!(
        repo.connection()
            .execute_batch("UPDATE event SET project_id='x'")
            .is_err()
    );
    assert!(
        repo.connection()
            .execute_batch("DELETE FROM event")
            .is_err()
    );
    assert_eq!(image(repo.connection()), before);
    // Idempotent on an already current store.
    drop(repo);
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
        17
    );
}

#[test]
fn v11_store_with_events_reaches_guarded_v17() {
    let tmp = TempDir::new("event-guard-v11");
    let repo =
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &migrations::registered()[..11])
            .unwrap();
    repo.connection()
        .execute_batch(&insert("INSERT", EXISTING, ""))
        .unwrap();
    drop(repo);
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
        17
    );
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let rows = image(repo.connection());
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].starts_with(EXISTING) && rows[0].ends_with("|9|text"),
        "{rows:?}"
    );
    assert!(
        repo.connection()
            .execute_batch("DELETE FROM event")
            .is_err()
    );
    assert_eq!(image(repo.connection()), rows);
}

#[test]
fn colliding_guard_object_fails_the_v17_step_atomically() {
    let (tmp, repo) = seeded("event-guard-collision");
    let before = image(repo.connection());
    crate::decision_records_tests::remove_v17(repo.connection());
    repo.connection()
        .execute_batch("CREATE TRIGGER event_no_delete BEFORE DELETE ON event BEGIN SELECT 1; END")
        .unwrap();
    drop(repo);
    assert!(SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).is_err());
    let conn = rusqlite::Connection::open(tmp.db_path()).unwrap();
    // Still the v16 head, with no other v17 object and the rows untouched.
    let version: i64 = conn
        .query_row("SELECT MAX(version) FROM state_schema_version", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(version, 16);
    for table in [
        "decision_records",
        "provenance_links",
        "logical_role_history",
    ] {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name=?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
    assert_eq!(image(&conn), before);
}

#[test]
fn corruption_fixtures_disable_a_guard_only_inside_the_fixture() {
    let (tmp, repo) = seeded("event-guard-fixture");
    with_guard_disabled(&repo, "event_no_update", |conn| {
        conn.execute_batch(
            "PRAGMA ignore_check_constraints=ON;
             UPDATE event SET event_type='NOT_A_TYPE' WHERE event_id='01ARZ3NDEKTSV4RRFFQ69G5FAA';
             PRAGMA ignore_check_constraints=OFF;",
        )
    })
    .unwrap();
    // The guard is back, the store is current, and the damage is detected on read.
    assert!(
        repo.connection()
            .execute_batch("UPDATE event SET project_id='x'")
            .is_err()
    );
    assert!(matches!(
        repo.find_event(EXISTING),
        Err(StateError::EventDecodeFailed { .. })
    ));
    drop(repo);
    // The planted CHECK violation is also caught by the open-time integrity check.
    assert!(matches!(
        SqliteStateRepository::open(tmp.db_path()),
        Err(StateError::GraphPersistence {
            code: GraphFailureCode::CorruptStore,
            ..
        })
    ));
}
