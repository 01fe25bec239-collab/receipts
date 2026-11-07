//! Tests for the checked chronological event query.
use crate::error::StateError;
use crate::event::{
    ActorKind, EventActor, EventEnvelope, EventPayloadReference, EventQuery, EventSubject,
    EventTimeQueryV1, EventType, SubjectKind,
};
use crate::repository::SqliteStateRepository;
use crate::tests::{TempDir, state_epoch};

fn ev(id_suffix: &str, project: &str, subject: &str, at: &str) -> EventEnvelope {
    EventEnvelope {
        event_id: format!("01ARZ3NDEKTSV4RRFFQ69G{id_suffix}"),
        project_id: project.into(),
        goal_id: None,
        event_type: EventType::TaskCreated,
        actor: EventActor {
            kind: ActorKind::System,
            id: None,
        },
        subject: EventSubject {
            kind: SubjectKind::Task,
            id: subject.into(),
        },
        occurred_at: at.into(),
        payload: EventPayloadReference {
            reference: "ref".into(),
            digest: "d".into(),
        },
        correlation_id: "c".into(),
        epoch: state_epoch(0),
    }
}

fn q(project: &str, since: Option<&str>, until: Option<&str>, limit: u32) -> EventTimeQueryV1 {
    EventTimeQueryV1 {
        selection: EventQuery {
            project_id: project.into(),
            subject: None,
            event_types: None,
            limit,
        },
        since: since.map(Into::into),
        until: until.map(Into::into),
    }
}

fn ids(events: &[EventEnvelope]) -> Vec<&str> {
    events.iter().map(|e| &e.event_id[22..]).collect()
}

const T1: &str = "2026-10-29T10:00:00.000000000Z";
const T2: &str = "2026-10-29T10:00:00.000000001Z";
const T3: &str = "2026-10-29T11:00:00.000000000Z";

fn seeded(tag: &str) -> (TempDir, SqliteStateRepository) {
    let tmp = TempDir::new(tag);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    // Identity order deliberately disagrees with time order.
    for e in [
        ev("5AAA", "p", "t", T1),
        ev("1AAA", "p", "t", T3),
        ev("3AAA", "p", "t", T2),
        ev("2AAA", "p", "t", T1),
        // Non-canonical timestamps outside the selection must not poison it.
        ev("4AAA", "other", "t", "2026-10-29T10:00:00Z"),
        ev("6AAA", "p", "noncanonical", "yesterday"),
    ] {
        repo.append_event(e).unwrap();
    }
    (tmp, repo)
}

fn subject(mut query: EventTimeQueryV1, id: &str) -> EventTimeQueryV1 {
    query.selection.subject = Some(EventSubject {
        kind: SubjectKind::Task,
        id: id.into(),
    });
    query
}

#[test]
fn event_query_is_deterministically_ordered_by_semantic_time_then_id() {
    let (tmp, repo) = seeded("time-order");
    let all = repo
        .list_events_chronological(&subject(q("p", None, None, u32::MAX), "t"))
        .unwrap();
    assert_eq!(ids(&all), vec!["2AAA", "5AAA", "3AAA", "1AAA"]);
    // Limit applies after time ordering, not to identity order.
    let first = repo
        .list_events_chronological(&subject(q("p", None, None, 2), "t"))
        .unwrap();
    assert_eq!(ids(&first), vec!["2AAA", "5AAA"]);
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert_eq!(
        repo.list_events_chronological(&subject(q("p", None, None, u32::MAX), "t"))
            .unwrap(),
        all
    );
    // The legacy query keeps identity order and opaque bytes.
    let legacy = repo
        .list_events(&EventQuery {
            project_id: "p".into(),
            subject: None,
            event_types: None,
            limit: 10,
        })
        .unwrap();
    assert_eq!(ids(&legacy), vec!["1AAA", "2AAA", "3AAA", "5AAA", "6AAA"]);
}

#[test]
fn bounds_are_inclusive_since_and_exclusive_until() {
    let (_tmp, repo) = seeded("time-bounds");
    let run = |since, until| {
        ids(&repo
            .list_events_chronological(&subject(q("p", since, until, 10), "t"))
            .unwrap())
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
    };
    assert_eq!(run(Some(T2), None), vec!["3AAA", "1AAA"]);
    assert_eq!(run(None, Some(T2)), vec!["2AAA", "5AAA"]);
    assert_eq!(run(Some(T1), Some(T3)), vec!["2AAA", "5AAA", "3AAA"]);
    assert!(run(Some(T2), Some(T2)).is_empty());
}

#[test]
fn invalid_or_reversed_bounds_refuse_even_for_empty_shortcuts() {
    let (_tmp, repo) = seeded("time-invalid");
    for (since, until, limit) in [
        (Some("2026-10-29T10:00:00Z"), None, 10),
        (None, Some("not a time"), 10),
        (Some(T3), Some(T1), 10),
        (Some(T3), Some(T1), 0),
        (Some("bad"), None, 0),
    ] {
        assert!(matches!(
            repo.list_events_chronological(&subject(q("p", since, until, limit), "t")),
            Err(StateError::EventQueryInvalid { .. })
        ));
    }
    let mut empty_types = subject(q("p", Some("bad"), None, 10), "t");
    empty_types.selection.event_types = Some(Vec::new());
    assert!(repo.list_events_chronological(&empty_types).is_err());
    assert!(matches!(
        repo.list_events_chronological(&q("", None, None, 0)),
        Err(StateError::EventValidation { .. })
    ));
    let mut shortcut = subject(q("p", Some(T1), None, 10), "t");
    shortcut.selection.event_types = Some(Vec::new());
    assert!(
        repo.list_events_chronological(&shortcut)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn selected_timestamp_outside_domain_refuses_instead_of_dropping() {
    let (_tmp, repo) = seeded("time-domain");
    // Whole project selection includes the non-canonical row.
    assert!(matches!(
        repo.list_events_chronological(&q("p", Some(T1), None, 10)),
        Err(StateError::EventTimestampOutsideQueryDomain { event_id }) if event_id.ends_with("6AAA")
    ));
    // Even when the bad row would be outside the requested range.
    assert!(
        repo.list_events_chronological(&q("p", Some(T3), Some(T3), 10))
            .is_err()
    );
    // A type selection that excludes it is not poisoned.
    let mut typed = q("p", None, None, 10);
    typed.selection.event_types = Some(vec![EventType::TaskCreated]);
    assert!(repo.list_events_chronological(&typed).is_err());
    typed.selection.event_types = Some(vec![EventType::TaskReady]);
    assert!(repo.list_events_chronological(&typed).unwrap().is_empty());
    assert!(matches!(
        repo.list_events_chronological(&q("other", None, None, 10)),
        Err(StateError::EventTimestampOutsideQueryDomain { .. })
    ));
}

#[test]
fn chronological_read_writes_nothing() {
    let (_tmp, repo) = seeded("time-readonly");
    let version = |repo: &SqliteStateRepository| -> i64 {
        repo.connection()
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .unwrap()
    };
    let changes = |repo: &SqliteStateRepository| repo.connection().total_changes();
    let (v, c) = (version(&repo), changes(&repo));
    repo.list_events_chronological(&subject(q("p", None, None, 10), "t"))
        .unwrap();
    let _ = repo.list_events_chronological(&q("p", None, None, 10));
    assert_eq!((version(&repo), changes(&repo)), (v, c));
    assert!(repo.find_trusted_time_watermark("p").unwrap().is_none());
}
