use crate::authority::{
    StatePublisherV1, StateReader, authoritative_event_publisher, context_epoch_advance_publisher,
};
use crate::error::{GraphFailureCode, StateError};
use crate::event::{
    ActorKind, EventActor, EventEnvelope, EventPayloadReference, EventQuery, EventSubject,
    EventType, SubjectKind,
};
use crate::repository::SqliteStateRepository;
use crate::tests::{TempDir, state_epoch};

fn ulid(i: usize) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    format!(
        "01ARZ3NDEKTSV4RRFFQ69G{}{}{}{}",
        'A',
        ALPHABET[i / 32 % 32] as char,
        ALPHABET[i % 32] as char,
        'A'
    )
}

fn event(i: usize, event_type: EventType, actor: ActorKind) -> EventEnvelope {
    EventEnvelope {
        event_id: ulid(i),
        project_id: "p".into(),
        goal_id: None,
        event_type,
        actor: EventActor {
            kind: actor,
            id: Some("claimed-writer".into()),
        },
        subject: EventSubject {
            kind: SubjectKind::Task,
            id: "t".into(),
        },
        occurred_at: "2026-10-29T10:00:00.000000000Z".into(),
        payload: EventPayloadReference {
            reference: "ref".into(),
            digest: "d".into(),
        },
        correlation_id: "c".into(),
        epoch: state_epoch(0),
    }
}

#[test]
fn every_event_type_and_epoch_advance_has_one_trusted_core_publisher() {
    let all = EventType::all();
    assert_eq!(all.len(), 53);
    let mut names: Vec<_> = all.iter().map(|t| t.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 53);
    assert!(
        all.iter()
            .all(|t| authoritative_event_publisher(*t) == StatePublisherV1::TrustedCore)
    );
    assert_eq!(
        context_epoch_advance_publisher(),
        StatePublisherV1::TrustedCore
    );
}

#[test]
fn actor_is_recorded_causal_data_not_a_writer_credential() {
    let tmp = TempDir::new("authority-actor");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let mut i = 0;
    for event_type in EventType::all() {
        for actor in ActorKind::ALL {
            let e = event(i, event_type, actor);
            repo.append_event(e.clone()).unwrap();
            assert_eq!(repo.find_event(&e.event_id).unwrap(), Some(e));
            i += 1;
        }
    }
}

#[test]
fn state_reader_reads_current_store_and_cannot_write() {
    let tmp = TempDir::new("authority-reader");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let e = event(0, EventType::TaskCreated, ActorKind::Host);
    repo.append_event(e.clone()).unwrap();
    let reader = StateReader::open_existing(tmp.db_path()).unwrap();
    assert_eq!(reader.find_event(&e.event_id).unwrap(), Some(e.clone()));
    let query = EventQuery {
        project_id: "p".into(),
        subject: None,
        event_types: None,
        limit: 9,
    };
    assert_eq!(
        reader.list_events(&query).unwrap(),
        repo.list_events(&query).unwrap()
    );
    assert!(reader.find_logical_role("absent").unwrap().is_none());
    assert!(reader.provenance_chain("p", "t").unwrap().missing.len() == 7);
    assert!(reader.find_trusted_time_watermark("p").unwrap().is_none());
    // The underlying SQLite handle itself refuses writes.
    let error = reader
        .inner_connection_for_test()
        .execute_batch("INSERT INTO trusted_time_watermark VALUES ('p','s','v','2026-10-29T10:00:00.000000000Z')")
        .unwrap_err();
    assert!(error.to_string().contains("readonly"), "{error}");
    drop(reader);
    let reread = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert!(reread.find_trusted_time_watermark("p").unwrap().is_none());
}

#[test]
fn state_reader_refuses_missing_older_and_tampered_stores() {
    let tmp = TempDir::new("authority-reader-refusals");
    assert!(StateReader::open_existing(tmp.db_path()).is_err());
    assert!(!tmp.db_path().exists(), "reader never creates a store");
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    crate::decision_records_tests::remove_v17(repo.connection());
    drop(repo);
    assert!(matches!(
        StateReader::open_existing(tmp.db_path()),
        Err(StateError::GraphPersistence {
            code: GraphFailureCode::UnsupportedSchema,
            ..
        })
    ));
    let other = TempDir::new("authority-reader-tamper");
    let repo = SqliteStateRepository::open(other.db_path()).unwrap();
    repo.connection()
        .execute_batch("DROP TRIGGER decision_records_no_delete")
        .unwrap();
    drop(repo);
    assert!(matches!(
        StateReader::open_existing(other.db_path()),
        Err(StateError::GraphPersistence {
            code: GraphFailureCode::CorruptStore,
            ..
        })
    ));
}
