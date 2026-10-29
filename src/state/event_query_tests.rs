//! Tests for the read-only project/subject/type event query
//! (`SqliteStateRepository::list_events`).
//!
//! Every test appends through the public API into a real temporary SQLite
//! file and queries it, usually again after a reopen. Malformed rows that the
//! typed API and storage CHECKs would refuse are planted with CHECK
//! enforcement switched off inside the State layer only.

use crate::error::StateError;
use crate::event::{
    ActorKind, EventActor, EventEnvelope, EventPayloadReference, EventQuery, EventSubject,
    EventType, SubjectKind,
};
use crate::repository::SqliteStateRepository;
use crate::tests::{TempDir, state_epoch};

/// A distinct canonical ULID per index (same scheme as `event_tests`).
fn ulid_for(index: u64) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut suffix = [b'0'; 6];
    let mut value = index;
    for slot in suffix.iter_mut().rev() {
        *slot = ALPHABET[(value % 32) as usize];
        value /= 32;
    }
    let suffix = std::str::from_utf8(&suffix).expect("suffix is pure ASCII");
    format!("01ARZ3NDEKTSV4RRFFQ6{suffix}")
}

fn event(
    event_id: String,
    project: &str,
    event_type: EventType,
    subject_kind: SubjectKind,
    subject_id: &str,
) -> EventEnvelope {
    EventEnvelope {
        event_id,
        project_id: project.to_string(),
        goal_id: Some("goal-1".to_string()),
        event_type,
        actor: EventActor {
            kind: ActorKind::Role,
            id: Some("role-1".to_string()),
        },
        subject: EventSubject {
            kind: subject_kind,
            id: subject_id.to_string(),
        },
        occurred_at: "2026-08-17T10:00:00.000Z".to_string(),
        payload: EventPayloadReference {
            reference: "blob://events/1".to_string(),
            digest: "sha256:abc".to_string(),
        },
        correlation_id: "corr-1".to_string(),
        epoch: state_epoch(3),
    }
}

fn task_event(index: u64, project: &str) -> EventEnvelope {
    event(
        ulid_for(index),
        project,
        EventType::TaskCreated,
        SubjectKind::Task,
        "task-1",
    )
}

fn query(project: &str) -> EventQuery {
    EventQuery {
        project_id: project.to_string(),
        subject: None,
        event_types: None,
        limit: u32::MAX,
    }
}

fn ids(events: &[EventEnvelope]) -> Vec<String> {
    events.iter().map(|e| e.event_id.clone()).collect()
}

fn open(tmp: &TempDir) -> SqliteStateRepository {
    SqliteStateRepository::open(tmp.db_path()).expect("open")
}

fn append_all(repo: &mut SqliteStateRepository, events: impl IntoIterator<Item = EventEnvelope>) {
    for e in events {
        repo.append_event(e).expect("append");
    }
}

/// Inserts a row the typed API would refuse, with CHECK enforcement off.
fn plant_row(repo: &mut SqliteStateRepository, columns: [&str; 4], epoch: &str) {
    let [event_id, event_type, actor_kind, subject_kind] = columns;
    repo.run_transaction(|uow| {
        uow.execute_batch(&format!(
            "PRAGMA ignore_check_constraints = ON;
             INSERT INTO event (event_id, project_id, event_type, actor_kind, subject_kind,
                 subject_id, occurred_at, payload_reference, payload_digest, correlation_id, epoch)
             VALUES ('{event_id}', 'planted', '{event_type}', '{actor_kind}', '{subject_kind}',
                 'task-1', 't', 'blob://x', 'sha256:x', 'corr', '{epoch}');
             PRAGMA ignore_check_constraints = OFF;"
        ))
    })
    .expect("plant row");
}

fn table_counts(repo: &SqliteStateRepository) -> Vec<(String, i64)> {
    repo.list_tables()
        .expect("tables")
        .into_iter()
        .map(|table| {
            let count = repo.count_table_rows(&table).expect("count");
            (table, count)
        })
        .collect()
}

#[test]
fn project_isolation_and_reopen() {
    let tmp = TempDir::new("evq-project");
    {
        let mut repo = open(&tmp);
        append_all(
            &mut repo,
            [
                task_event(1, "project-a"),
                task_event(2, "project-b"),
                task_event(3, "project-a"),
                task_event(4, "project-a "),
                task_event(5, "PROJECT-A"),
            ],
        );
    }
    let repo = open(&tmp);
    assert_eq!(
        ids(&repo.list_events(&query("project-a")).unwrap()),
        [ulid_for(1), ulid_for(3)]
    );
    assert_eq!(
        ids(&repo.list_events(&query("project-b")).unwrap()),
        [ulid_for(2)]
    );
    // Whitespace and case are data: neighbouring spellings are other projects.
    assert_eq!(
        ids(&repo.list_events(&query("project-a ")).unwrap()),
        [ulid_for(4)]
    );
    assert_eq!(
        ids(&repo.list_events(&query("PROJECT-A")).unwrap()),
        [ulid_for(5)]
    );
}

#[test]
fn missing_partition_is_empty_not_error() {
    let tmp = TempDir::new("evq-missing");
    let mut repo = open(&tmp);
    assert!(repo.list_events(&query("nobody")).unwrap().is_empty());
    append_all(&mut repo, [task_event(1, "project-a")]);
    assert!(repo.list_events(&query("nobody")).unwrap().is_empty());
}

#[test]
fn returned_events_equal_find_event() {
    let tmp = TempDir::new("evq-roundtrip");
    let mut repo = open(&tmp);
    let mut minimal = task_event(1, "p");
    minimal.goal_id = None;
    minimal.actor.id = None;
    minimal.epoch = state_epoch(0);
    let mut full = event(
        ulid_for(2),
        "p",
        EventType::RoutingDecided,
        SubjectKind::Workspace,
        "ws-1",
    );
    full.occurred_at = "not a timestamp \u{0}\n".to_string();
    append_all(&mut repo, [minimal.clone(), full.clone()]);
    let listed = repo.list_events(&query("p")).unwrap();
    assert_eq!(listed, [minimal, full]);
    for e in &listed {
        assert_eq!(repo.find_event(&e.event_id).unwrap().as_ref(), Some(e));
    }
}

#[test]
fn subject_matches_kind_and_id_exactly() {
    let tmp = TempDir::new("evq-subject");
    let mut repo = open(&tmp);
    let at = |index, kind, id: &str| event(ulid_for(index), "p", EventType::TaskCreated, kind, id);
    append_all(
        &mut repo,
        [
            at(1, SubjectKind::Task, "x-1"),
            at(2, SubjectKind::Role, "x-1"),
            at(3, SubjectKind::Task, "x-2"),
            at(4, SubjectKind::Task, "X-1"),
            at(5, SubjectKind::Task, "x-1 "),
            at(6, SubjectKind::Task, "x-1"),
        ],
    );
    // Same subject id under a different project must not leak in.
    repo.append_event(event(
        ulid_for(7),
        "other",
        EventType::TaskCreated,
        SubjectKind::Task,
        "x-1",
    ))
    .unwrap();

    let by = |kind, id: &str| {
        let mut q = query("p");
        q.subject = Some(EventSubject {
            kind,
            id: id.to_string(),
        });
        ids(&repo.list_events(&q).unwrap())
    };
    assert_eq!(by(SubjectKind::Task, "x-1"), [ulid_for(1), ulid_for(6)]);
    assert_eq!(by(SubjectKind::Role, "x-1"), [ulid_for(2)]);
    assert_eq!(by(SubjectKind::Task, "X-1"), [ulid_for(4)]);
    assert_eq!(by(SubjectKind::Task, "x-1 "), [ulid_for(5)]);
    assert!(by(SubjectKind::Goal, "x-1").is_empty());
    assert!(by(SubjectKind::Task, "x-").is_empty());
}

#[test]
fn event_type_filter_none_empty_duplicate_and_multi() {
    let tmp = TempDir::new("evq-types");
    let mut repo = open(&tmp);
    let kinds = [
        EventType::TaskCreated,
        EventType::TaskFailed,
        EventType::GoalCreated,
        EventType::TaskCreated,
    ];
    append_all(
        &mut repo,
        kinds
            .iter()
            .enumerate()
            .map(|(i, t)| event(ulid_for(i as u64), "p", *t, SubjectKind::Task, "task-1")),
    );
    let with = |types: Option<Vec<EventType>>| {
        let mut q = query("p");
        q.event_types = types;
        ids(&repo.list_events(&q).unwrap())
    };
    let all = [ulid_for(0), ulid_for(1), ulid_for(2), ulid_for(3)];
    assert_eq!(with(None), all);
    assert!(with(Some(vec![])).is_empty());
    assert_eq!(
        with(Some(vec![EventType::TaskCreated])),
        [ulid_for(0), ulid_for(3)]
    );
    // Duplicate entries do not duplicate events.
    assert_eq!(
        with(Some(vec![
            EventType::TaskCreated,
            EventType::TaskCreated,
            EventType::TaskCreated
        ])),
        [ulid_for(0), ulid_for(3)]
    );
    // Multiple types, any request order.
    assert_eq!(
        with(Some(vec![EventType::GoalCreated, EventType::TaskFailed])),
        [ulid_for(1), ulid_for(2)]
    );
    assert_eq!(
        with(Some(EventType::ALL.to_vec())),
        all,
        "all 53 types in the set is the same as no filter"
    );
    assert!(with(Some(vec![EventType::HumanRequired])).is_empty());
}

#[test]
fn combined_subject_and_type_filters() {
    let tmp = TempDir::new("evq-combined");
    let mut repo = open(&tmp);
    append_all(
        &mut repo,
        [
            event(
                ulid_for(1),
                "p",
                EventType::TaskCreated,
                SubjectKind::Task,
                "a",
            ),
            event(
                ulid_for(2),
                "p",
                EventType::TaskFailed,
                SubjectKind::Task,
                "a",
            ),
            event(
                ulid_for(3),
                "p",
                EventType::TaskFailed,
                SubjectKind::Task,
                "b",
            ),
            event(
                ulid_for(4),
                "q",
                EventType::TaskFailed,
                SubjectKind::Task,
                "a",
            ),
        ],
    );
    let mut q = query("p");
    q.subject = Some(EventSubject {
        kind: SubjectKind::Task,
        id: "a".to_string(),
    });
    q.event_types = Some(vec![EventType::TaskFailed]);
    assert_eq!(ids(&repo.list_events(&q).unwrap()), [ulid_for(2)]);
}

#[test]
fn order_is_event_id_bytes_regardless_of_insertion_or_timestamp_text() {
    let tmp = TempDir::new("evq-order");
    let mut repo = open(&tmp);
    // Digits sort before letters in byte order. Insert in a scrambled order
    // with timestamp spellings that sort opposite to, or nothing like, the ids.
    let raw_ids = [
        "7ZZZZZZZZZZZZZZZZZZZZZZZZZ",
        "00000000000000000000000000",
        "01ARZ3NDEKTSV4RRFFQ69G5FAK",
        "01ARZ3NDEKTSV4RRFFQ69G5FAJ",
        "ZZZZZZZZZZZZZZZZZZZZZZZZZZ",
        "10000000000000000000000000",
        "A0000000000000000000000000",
    ];
    let stamps = [
        "1999-01-01T00:00:00Z",
        "zzzz",
        "2999-12-31T23:59:59.999Z",
        " ",
        "yesterday",
        "0000-00-00T00:00:60Z",
        "2026-08-17T10:00:00.000Z",
    ];
    for (raw, stamp) in raw_ids.iter().zip(stamps) {
        let mut e = task_event(0, "p");
        e.event_id = (*raw).to_string();
        e.occurred_at = stamp.to_string();
        repo.append_event(e).unwrap();
    }
    let mut expected: Vec<String> = raw_ids.iter().map(|s| s.to_string()).collect();
    expected.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(ids(&repo.list_events(&query("p")).unwrap()), expected);

    // Reverse insertion order into a fresh store gives the same result.
    let tmp2 = TempDir::new("evq-order-rev");
    let mut repo2 = open(&tmp2);
    for (raw, stamp) in raw_ids.iter().zip(stamps).rev() {
        let mut e = task_event(0, "p");
        e.event_id = (*raw).to_string();
        e.occurred_at = stamp.to_string();
        repo2.append_event(e).unwrap();
    }
    assert_eq!(ids(&repo2.list_events(&query("p")).unwrap()), expected);

    // Repeating the query is stable.
    assert_eq!(
        repo.list_events(&query("p")).unwrap(),
        repo.list_events(&query("p")).unwrap()
    );
}

#[test]
fn limit_zero_one_n_and_applies_after_project_filter() {
    let tmp = TempDir::new("evq-limit");
    let mut repo = open(&tmp);
    // Another project owns the lowest ids; it must not consume the limit.
    append_all(&mut repo, (0..3).map(|i| task_event(i, "other")));
    append_all(&mut repo, (10..15).map(|i| task_event(i, "p")));
    let with = |limit| {
        let mut q = query("p");
        q.limit = limit;
        ids(&repo.list_events(&q).unwrap())
    };
    let all: Vec<String> = (10..15).map(ulid_for).collect();
    assert!(with(0).is_empty());
    assert_eq!(with(1), all[..1]);
    assert_eq!(with(3), all[..3]);
    assert_eq!(with(5), all);
    assert_eq!(with(6), all);
    assert_eq!(with(u32::MAX), all);

    // Limit applies to the filtered set, not before the type filter.
    let mut q = query("p");
    q.limit = 1;
    q.event_types = Some(vec![EventType::TaskCreated]);
    assert_eq!(ids(&repo.list_events(&q).unwrap()), all[..1]);
}

#[test]
fn hostile_and_unicode_identifiers_are_data() {
    let tmp = TempDir::new("evq-hostile");
    let mut repo = open(&tmp);
    let names = [
        "p' OR '1'='1",
        "p'; DROP TABLE event; --",
        "p\" OR \"1\"=\"1",
        "p%",
        "p_",
        "  ",
        "\n\t\r",
        "p\u{7f}\u{1b}[31m",
        "pro\u{0}ject",
        "проект/项目/🧾",
        "e\u{301}",
        "\u{e9}",
        "?",
        "?1",
    ];
    for (i, name) in names.iter().enumerate() {
        let mut e = event(
            ulid_for(i as u64),
            name,
            EventType::TaskCreated,
            SubjectKind::Task,
            name,
        );
        e.correlation_id = name.to_string();
        repo.append_event(e).unwrap();
    }
    let before = table_counts(&repo);
    for (i, name) in names.iter().enumerate() {
        let mut q = query(name);
        let found = repo.list_events(&q).unwrap();
        assert_eq!(ids(&found), [ulid_for(i as u64)], "project {name:?}");
        assert_eq!(found[0].project_id, *name, "byte-preserved");

        q.subject = Some(EventSubject {
            kind: SubjectKind::Task,
            id: name.to_string(),
        });
        assert_eq!(ids(&repo.list_events(&q).unwrap()), [ulid_for(i as u64)]);

        // A subject id never matches another row's spelling.
        q.subject = Some(EventSubject {
            kind: SubjectKind::Task,
            id: format!("{name}x"),
        });
        assert!(repo.list_events(&q).unwrap().is_empty());
    }
    assert_eq!(table_counts(&repo), before, "no injected statement ran");
    assert!(repo.table_exists("event").unwrap());
}

#[test]
fn empty_ids_refused_without_echo_even_when_nothing_would_be_read() {
    let tmp = TempDir::new("evq-invalid");
    let repo = open(&tmp);
    let check = |q: EventQuery, needle: &str| {
        let error = repo.list_events(&q).expect_err("must refuse");
        assert!(
            matches!(&error, StateError::EventValidation { .. }),
            "unexpected: {error}"
        );
        assert!(error.to_string().contains(needle), "{error}");
    };
    check(query(""), "project_id must not be empty");

    let mut q = query("p");
    q.subject = Some(EventSubject {
        kind: SubjectKind::Task,
        id: String::new(),
    });
    check(q.clone(), "subject.id must not be empty");

    // Validation happens before the empty-result shortcuts.
    q.limit = 0;
    check(q.clone(), "subject.id must not be empty");
    q.subject = None;
    q.limit = 5;
    q.project_id = String::new();
    q.event_types = Some(vec![]);
    check(q, "project_id must not be empty");
}

#[test]
fn selected_malformed_row_fails_whole_result() {
    let tmp = TempDir::new("evq-malformed");
    let mut repo = open(&tmp);
    append_all(
        &mut repo,
        [task_event(1, "planted"), task_event(9, "planted")],
    );
    let good = ulid_for(5);
    for (columns, epoch) in [
        ([good.as_str(), "NOT_A_TYPE", "SYSTEM", "TASK"], "0"),
        ([good.as_str(), "TASK_CREATED", "AGENT", "TASK"], "0"),
        ([good.as_str(), "TASK_CREATED", "SYSTEM", "SESSION"], "0"),
        ([good.as_str(), "TASK_CREATED", "SYSTEM", "TASK"], "-1"),
        ([good.as_str(), "TASK_CREATED", "SYSTEM", "TASK"], "007"),
        ([good.as_str(), "TASK_CREATED", "SYSTEM", "TASK"], "1.5"),
    ] {
        plant_row(&mut repo, columns, epoch);
        // Valid rows sort before and after the bad one; none may be returned.
        let error = repo
            .list_events(&query("planted"))
            .expect_err("selected malformed row must fail the whole call");
        assert!(
            matches!(error, StateError::EventDecodeFailed { .. }),
            "unexpected: {error} for {columns:?} epoch {epoch}"
        );
        // Not selected: another project, or a filter that excludes the row.
        assert!(repo.list_events(&query("p")).unwrap().is_empty());
        let mut limited = query("planted");
        limited.limit = 1;
        assert_eq!(
            ids(&repo.list_events(&limited).unwrap()),
            [ulid_for(1)],
            "rows beyond the limit are not decoded"
        );
        let mut subject = query("planted");
        subject.subject = Some(EventSubject {
            kind: SubjectKind::Role,
            id: "task-1".to_string(),
        });
        assert!(repo.list_events(&subject).unwrap().is_empty());
        repo.run_transaction(|uow| uow.execute("DELETE FROM event WHERE event_id = ?1", &[&good]))
            .expect("remove planted row");
    }
    // With the planted rows gone the partition decodes again.
    assert_eq!(
        ids(&repo.list_events(&query("planted")).unwrap()),
        [ulid_for(1), ulid_for(9)]
    );
}

#[test]
fn type_filter_can_exclude_a_malformed_row() {
    let tmp = TempDir::new("evq-malformed-type");
    let mut repo = open(&tmp);
    append_all(&mut repo, [task_event(1, "planted")]);
    plant_row(
        &mut repo,
        [&ulid_for(5), "NOT_A_TYPE", "SYSTEM", "TASK"],
        "0",
    );
    let mut q = query("planted");
    q.event_types = Some(vec![EventType::TaskCreated]);
    assert_eq!(ids(&repo.list_events(&q).unwrap()), [ulid_for(1)]);
    q.event_types = None;
    assert!(matches!(
        repo.list_events(&q),
        Err(StateError::EventDecodeFailed { .. })
    ));
}

#[test]
fn reads_write_nothing() {
    let tmp = TempDir::new("evq-readonly");
    let mut repo = open(&tmp);
    append_all(
        &mut repo,
        (0..4).map(|i| task_event(i, if i % 2 == 0 { "a" } else { "b" })),
    );
    let counts = table_counts(&repo);
    let everything = |repo: &SqliteStateRepository| {
        ["a", "b"]
            .into_iter()
            .flat_map(|p| repo.list_events(&query(p)).unwrap())
            .collect::<Vec<_>>()
    };
    let rows = everything(&repo);

    let mut q = query("a");
    q.subject = Some(EventSubject {
        kind: SubjectKind::Task,
        id: "task-1".to_string(),
    });
    q.event_types = Some(vec![EventType::TaskCreated, EventType::TaskFailed]);
    q.limit = 1;
    for _ in 0..3 {
        repo.list_events(&q).unwrap();
        let _ = repo.list_events(&query(""));
        repo.list_events(&query("missing")).unwrap();
    }
    assert_eq!(table_counts(&repo), counts);
    assert_eq!(everything(&repo), rows);
    drop(repo);
    let repo = open(&tmp);
    assert_eq!(table_counts(&repo), counts);
    assert_eq!(everything(&repo), rows);
}

#[test]
fn append_and_find_still_work_alongside_queries() {
    let tmp = TempDir::new("evq-regression");
    let mut repo = open(&tmp);
    repo.append_event(task_event(1, "p")).unwrap();
    assert_eq!(repo.list_events(&query("p")).unwrap().len(), 1);
    repo.append_event(task_event(2, "p")).unwrap();
    assert!(matches!(
        repo.append_event(task_event(2, "p")),
        Err(StateError::EventAlreadyExists { .. })
    ));
    assert_eq!(repo.list_events(&query("p")).unwrap().len(), 2);
    assert_eq!(
        repo.find_event(&ulid_for(1)).unwrap(),
        Some(task_event(1, "p"))
    );
    assert_eq!(repo.find_event(&ulid_for(3)).unwrap(), None);
}
