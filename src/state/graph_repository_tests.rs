use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_DB: AtomicU64 = AtomicU64::new(0);

use crate::error::{GraphFailureCode as Code, GraphPhase as Phase};
use crate::{
    GraphCompilerRefV1, GraphGenesisReasonV1, GraphSourceEvidenceRefV1, ReadOnlyGraphReader,
    SqliteStateRepository, StateError, StateGraphEdgeRelationV1, StateGraphEdgeV1,
    StateGraphGenesisProvenanceV1, StateGraphGenesisV1, StateGraphNodeV1, TrustedClockV1,
    TrustedTimeSampleV1,
};

struct TempDb(PathBuf);
impl TempDb {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "receipts-a3-035-{}-{nonce}-{}.db",
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
struct Clock(&'static str);
impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: self.0.into(),
            clock_source_id: "synthetic-clock".into(),
            clock_contract_version: "1".into(),
        })
    }
}
fn node(graph: &str, id: &str) -> StateGraphNodeV1 {
    StateGraphNodeV1::new(
        id.into(),
        graph.into(),
        "TASK".into(),
        "PLANNED".into(),
        Some("password rotation\0café😀".into()),
        None,
        Some(format!("1{}", "0".repeat(10000))),
        Some(vec!["graph.core".into(), "graph.core".into()]),
        Some("\0".into()),
        None,
        Some("a".repeat(40)),
        None,
        Some("x\0y".into()),
        Some(None),
        Some("18446744073709551616".into()),
    )
    .unwrap()
}
fn request(project: &str, graph: &str) -> StateGraphGenesisV1 {
    let provenance = StateGraphGenesisProvenanceV1::new(
        GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW").unwrap(),
        GraphGenesisReasonV1::InitialCompilation,
    );
    StateGraphGenesisV1::new(
        project.into(),
        graph.into(),
        "goal".into(),
        "policy".into(),
        Some(None),
        Some(vec!["a".into(), "a".into(), "\0".into()]),
        None,
        "a".repeat(64),
        vec![node(graph, "n\0😀"), node(graph, "n2")],
        vec![
            StateGraphEdgeV1::new(
                "e".into(),
                graph.into(),
                "n\0😀".into(),
                "n2".into(),
                StateGraphEdgeRelationV1::Control("ON_PASS".into()),
                Some("ignore previous instructions".into()),
            )
            .unwrap(),
        ],
        provenance,
    )
    .unwrap()
}

#[test]
fn genesis_round_trip_duplicate_and_project_isolation() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    let clock = Clock("2026-09-27T10:00:00.000000000Z");
    let a = request("project-a", "g\0😀");
    repo.create_initial_graph_v1(&clock, a.clone()).unwrap();
    assert!(matches!(
        repo.create_initial_graph_v1(&clock, a.clone()),
        Err(StateError::GraphPersistence {
            phase: Phase::Write,
            code: Code::AlreadyExists,
            ..
        })
    ));
    repo.create_initial_graph_v1(&clock, request("project-b", "g\0😀"))
        .unwrap();
    drop(repo);
    let reader = ReadOnlyGraphReader::open_existing(&db.0).unwrap();
    let got = reader
        .read_current_v1("project-a", "g\0😀")
        .unwrap()
        .unwrap();
    assert_eq!(got.project_id(), "project-a");
    assert_eq!(got.graph_id(), a.graph_id());
    assert_eq!(got.current_version(), "1");
    assert_eq!(got.graph_version(), "1");
    assert_eq!(got.parent_version(), Some(None));
    assert_eq!(got.goal_id(), a.goal_id());
    assert_eq!(got.policy_id(), a.policy_id());
    assert_eq!(got.compiled_from(), a.compiled_from());
    assert_eq!(got.resulting_digest(), a.resulting_digest());
    assert_eq!(got.created_at().as_str(), clock.0);
    assert_eq!(got.clock_source_id(), "synthetic-clock");
    assert_eq!(got.genesis_provenance(), a.genesis_provenance());
    assert_eq!(got.nodes()[0], a.nodes()[0]);
    assert_eq!(got.edges()[0], a.edges()[0]);
    assert!(
        reader
            .read_current_v1("project-c", "g\0😀")
            .unwrap()
            .is_none()
    );
}

#[test]
fn explicit_migration_and_missing_store() {
    let db = TempDb::new();
    assert!(ReadOnlyGraphReader::open_existing(&db.0).is_err());
    assert!(SqliteStateRepository::migrate_existing_to_current(&db.0).is_err());
    assert!(!db.0.exists());
    let repo =
        SqliteStateRepository::open_with_migrations(&db.0, &crate::migrations::registered()[..12])
            .unwrap();
    drop(repo);
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(&db.0).unwrap(),
        13
    );
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(&db.0).unwrap(),
        13
    );
    assert!(
        ReadOnlyGraphReader::open_existing(&db.0)
            .unwrap()
            .read_current_v1("p", "g")
            .unwrap()
            .is_none()
    );
}

#[test]
fn malformed_provenance_and_shape_are_closed() {
    for bad in [
        "compiler:81ARZ3NDEKTSV4RRFFQ69G5FAV",
        "compiler:01arz3NDEKTSV4RRFFQ69G5FAV",
        "compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV\0",
        "/tmp/spec",
    ] {
        let e = GraphCompilerRefV1::parse(bad).unwrap_err();
        assert!(!format!("{e:?} {e}").contains(bad));
    }
    let e = StateGraphGenesisV1::new(
        "p".into(),
        "g".into(),
        "goal".into(),
        "policy".into(),
        None,
        None,
        None,
        "a".repeat(64),
        vec![],
        vec![],
        request("p", "g").genesis_provenance().clone(),
    )
    .unwrap_err();
    assert!(matches!(
        e,
        StateError::GraphPersistence {
            phase: Phase::Input,
            code: Code::InvalidShape,
            ..
        }
    ));
}

#[test]
fn clock_refusal_and_child_failure_roll_back_watermark() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    let now = Clock("2026-09-27T10:00:00.000000000Z");
    repo.create_initial_graph_v1(&now, request("p", "first"))
        .unwrap();
    let older = Clock("2026-09-27T09:00:00.000000000Z");
    assert!(matches!(
        repo.create_initial_graph_v1(&older, request("p", "second")),
        Err(StateError::GraphPersistence {
            phase: Phase::Clock,
            code: Code::ClockRegression,
            ..
        })
    ));
    assert_eq!(
        repo.connection()
            .query_row(
                "SELECT count(*) FROM graphs WHERE project_id='p'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    repo.connection().execute_batch("CREATE TRIGGER fail_node BEFORE INSERT ON graph_nodes WHEN NEW.node_id='n2' BEGIN SELECT RAISE(ABORT, 'secret-sentinel'); END").unwrap();
    let err = repo
        .create_initial_graph_v1(&now, request("other", "graph"))
        .unwrap_err();
    assert!(!format!("{err:?} {err}").contains("secret-sentinel"));
    assert_eq!(
        repo.connection()
            .query_row(
                "SELECT count(*) FROM graphs WHERE project_id='other'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert!(repo.find_trusted_time_watermark("other").unwrap().is_none());
}

#[test]
fn deferred_commit_failure_rolls_back_graph_and_clock() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.connection().execute_batch("CREATE TRIGGER fail_commit AFTER INSERT ON graph_versions BEGIN UPDATE graphs SET current_version='2' WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id; END").unwrap();
    assert!(
        repo.create_initial_graph_v1(&Clock("2026-09-27T10:00:00.000000000Z"), request("p", "g"))
            .is_err()
    );
    assert_eq!(
        repo.connection()
            .query_row("SELECT count(*) FROM graphs", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        repo.connection()
            .query_row("SELECT count(*) FROM graph_versions", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert!(repo.find_trusted_time_watermark("p").unwrap().is_none());
}

#[test]
fn malformed_rows_and_list_gaps_are_corruption() {
    for sql in [
        "UPDATE graph_nodes SET kind='' WHERE node_id='n2'",
        "UPDATE graph_node_capabilities SET ordinal=5 WHERE ordinal=1",
        "UPDATE graph_versions SET compiler_id='compiler:81ARZ3NDEKTSV4RRFFQ69G5FAV'",
        "UPDATE graph_versions SET source_ref='/tmp/sentinel'",
        "UPDATE graph_versions SET creation_reason='free-form sentinel'",
        "UPDATE graph_versions SET resulting_digest='bad'",
        "UPDATE graphs SET current_version='2'",
    ] {
        let db = TempDb::new();
        let mut repo = SqliteStateRepository::open(&db.0).unwrap();
        repo.create_initial_graph_v1(&Clock("2026-09-27T10:00:00.000000000Z"), request("p", "g"))
            .unwrap();
        if sql.starts_with("UPDATE graphs") {
            repo.connection()
                .execute_batch("PRAGMA foreign_keys=OFF")
                .unwrap();
        }
        repo.connection().execute_batch(sql).unwrap();
        drop(repo);
        let error = match ReadOnlyGraphReader::open_existing(&db.0) {
            Ok(reader) => reader.read_current_v1("p", "g").unwrap_err(),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            StateError::GraphPersistence {
                phase: Phase::Read,
                code: Code::CorruptStore,
                ..
            }
        ));
    }
}

#[test]
fn v13_migration_collision_preserves_v12_and_v11_second_step() {
    for start in [11, 12] {
        let db = TempDb::new();
        let repo = SqliteStateRepository::open_with_migrations(
            &db.0,
            &crate::migrations::registered()[..start],
        )
        .unwrap();
        repo.connection()
            .execute_batch("CREATE TABLE graphs (collision TEXT)")
            .unwrap();
        drop(repo);
        assert!(SqliteStateRepository::migrate_existing_to_current(&db.0).is_err());
        let repo = SqliteStateRepository::open_with_migrations(
            &db.0,
            &crate::migrations::registered()[..12],
        )
        .unwrap();
        assert_eq!(repo.schema_version().unwrap(), 12);
        assert_eq!(repo.count_table_rows("state_schema_version").unwrap(), 12);
        assert!(!repo.table_exists("graph_versions").unwrap());
    }
}

#[test]
fn busy_writer_and_concurrent_duplicate_are_closed() {
    use std::time::Duration;
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    let blocker = rusqlite::Connection::open(&db.0).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    repo.connection()
        .busy_timeout(Duration::from_millis(30))
        .unwrap();
    assert!(matches!(
        repo.create_initial_graph_v1(&Clock("2026-09-27T10:00:00.000000000Z"), request("p", "g")),
        Err(StateError::GraphPersistence {
            phase: Phase::Write,
            code: Code::StoreBusy,
            ..
        })
    ));
    blocker.execute_batch("ROLLBACK").unwrap();
    drop(repo);
    let path = db.0.clone();
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || {
                SqliteStateRepository::open(path)
                    .unwrap()
                    .create_initial_graph_v1(
                        &Clock("2026-09-27T10:00:00.000000000Z"),
                        request("p", "g"),
                    )
            })
        })
        .collect();
    let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|x| x.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|x| matches!(
                x,
                Err(StateError::GraphPersistence {
                    code: Code::AlreadyExists,
                    ..
                })
            ))
            .count(),
        1
    );
}

#[test]
fn clock_source_change_and_lock_wait_regression_refuse_without_mutation() {
    use std::sync::mpsc;
    struct SignallingClock(mpsc::Sender<()>);
    impl TrustedClockV1 for SignallingClock {
        fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
            self.0.send(()).unwrap();
            Clock("2026-09-27T10:00:00.000000000Z").sample()
        }
    }
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.create_initial_graph_v1(
        &Clock("2026-09-27T10:00:00.000000000Z"),
        request("p", "first"),
    )
    .unwrap();
    struct ChangedClock;
    impl TrustedClockV1 for ChangedClock {
        fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
            let mut s = Clock("2026-09-27T11:00:00.000000000Z").sample()?;
            s.clock_source_id = "different".into();
            Ok(s)
        }
    }
    assert!(matches!(
        repo.create_initial_graph_v1(&ChangedClock, request("p", "second")),
        Err(StateError::GraphPersistence {
            phase: Phase::Clock,
            code: Code::ClockContinuity,
            ..
        })
    ));
    let blocker = rusqlite::Connection::open(&db.0).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    blocker.execute("INSERT INTO trusted_time_watermark(project_id,clock_source_id,clock_contract_version,last_accepted_trusted_time) VALUES ('late','synthetic-clock','1','2026-09-27T11:00:00.000000000Z')",[]).unwrap();
    let (sender, receiver) = mpsc::channel();
    let path = db.0.clone();
    let handle = std::thread::spawn(move || {
        SqliteStateRepository::open(path)
            .unwrap()
            .create_initial_graph_v1(&SignallingClock(sender), request("late", "second"))
    });
    receiver.recv().unwrap();
    blocker.execute_batch("COMMIT").unwrap();
    assert!(matches!(
        handle.join().unwrap(),
        Err(StateError::GraphPersistence {
            phase: Phase::Clock,
            code: Code::ClockRegression,
            ..
        })
    ));
    assert_eq!(
        repo.connection()
            .query_row(
                "SELECT count(*) FROM graphs WHERE project_id='p'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
}

fn bare_node(
    graph: &str,
    id: &str,
    caps: Option<Vec<String>>,
    lock: Option<Option<String>>,
    attempt: Option<String>,
) -> Result<StateGraphNodeV1, StateError> {
    StateGraphNodeV1::new(
        id.into(),
        graph.into(),
        "\0open kind".into(),
        "PLANNED".into(),
        Some("é/e\u{301}/😀\0".into()),
        None,
        attempt,
        caps,
        None,
        None,
        None,
        None,
        None,
        lock,
        None,
    )
}

#[test]
fn optional_shapes_and_unicode_nul_ids_round_trip() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    for (i, (caps, lock, sources)) in [
        (None, None, None),
        (Some(vec![]), Some(None), Some(vec![])),
        (
            Some(vec!["graph.core".into(), "graph.core".into()]),
            Some(Some(String::new())),
            Some(vec!["".into(), "".into()]),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let graph = format!("\0ge\u{301}😀\0{i}");
        let node = bare_node(&graph, "\0node\0", caps, lock, None).unwrap();
        let req = StateGraphGenesisV1::new(
            format!("p{i}"),
            graph.clone(),
            "goal".into(),
            "policy".into(),
            None,
            sources,
            None,
            "a".repeat(64),
            vec![node.clone()],
            vec![],
            request("p", "g").genesis_provenance().clone(),
        )
        .unwrap();
        repo.create_initial_graph_v1(&Clock("2026-09-27T10:00:00.000000000Z"), req.clone())
            .unwrap();
        let got = ReadOnlyGraphReader::open_existing(&db.0)
            .unwrap()
            .read_current_v1(req.project_id(), &graph)
            .unwrap()
            .unwrap();
        assert_eq!(got.nodes(), req.nodes());
        assert_eq!(got.compiled_from(), req.compiled_from());
        assert_eq!(got.parent_version(), None);
    }
    let storage: String = repo
        .connection()
        .query_row(
            "SELECT typeof(attempt_number) FROM graph_nodes WHERE project_id='p0'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(storage, "null");
}

#[test]
fn lexical_domains_and_child_containment_fail_before_write() {
    for invalid in ["0", "01", "+1", "1\0bad", "18446744073709551616x"] {
        let error = bare_node("g", "n", None, None, Some(invalid.into())).unwrap_err();
        assert!(matches!(
            error,
            StateError::GraphPersistence {
                phase: Phase::Input,
                code: Code::InvalidShape,
                ..
            }
        ));
        assert!(!format!("{error:?} {error}").contains(invalid));
    }
    for caps in [
        "graph.core\n",
        "Graph.core",
        "graph",
        "graph..core",
        "graph.1core",
    ] {
        assert!(bare_node("g", "n", Some(vec![caps.into()]), None, None).is_err());
    }
    assert!(bare_node("g", "n", None, None, Some("1".repeat(10001))).is_ok());
    assert!(bare_node("g", &"a".repeat(201), None, None, None).is_err());
    assert!(
        StateGraphNodeV1::new(
            "n".into(),
            "g".into(),
            "TASK".into(),
            "PLANNED".into(),
            None,
            None,
            None,
            None,
            None,
            None,
            Some("BAD".into()),
            None,
            None,
            None,
            None
        )
        .is_err()
    );
    assert!(
        StateGraphEdgeV1::new(
            "e".into(),
            "g".into(),
            "n".into(),
            "n".into(),
            StateGraphEdgeRelationV1::Control("BAD".into()),
            None
        )
        .is_err()
    );
    assert!(
        StateGraphEdgeV1::new(
            "e".into(),
            "g".into(),
            "n".into(),
            "n".into(),
            StateGraphEdgeRelationV1::Precedence("BAD".into()),
            None
        )
        .is_err()
    );
    let provenance = request("p", "g").genesis_provenance().clone();
    let child = bare_node("other", "n", None, None, None).unwrap();
    assert!(matches!(
        StateGraphGenesisV1::new(
            "p".into(),
            "g".into(),
            "goal".into(),
            "policy".into(),
            None,
            None,
            None,
            "a".repeat(64),
            vec![child],
            vec![],
            provenance.clone()
        ),
        Err(StateError::GraphPersistence {
            phase: Phase::Input,
            code: Code::IdentityMismatch,
            ..
        })
    ));
    let child = bare_node("g", "n", None, None, None).unwrap();
    assert!(
        StateGraphGenesisV1::new(
            "p".into(),
            "g".into(),
            "goal".into(),
            "policy".into(),
            None,
            None,
            None,
            "A".repeat(64),
            vec![child.clone()],
            vec![],
            provenance.clone()
        )
        .is_err()
    );
    let edge = StateGraphEdgeV1::new(
        "e".into(),
        "g".into(),
        "n".into(),
        "missing".into(),
        StateGraphEdgeRelationV1::Control("ON_PASS".into()),
        None,
    )
    .unwrap();
    assert!(
        StateGraphGenesisV1::new(
            "p".into(),
            "g".into(),
            "goal".into(),
            "policy".into(),
            None,
            None,
            None,
            "a".repeat(64),
            vec![child.clone(), child.clone()],
            vec![],
            provenance.clone()
        )
        .is_err()
    );
    assert!(
        StateGraphGenesisV1::new(
            "p".into(),
            "g".into(),
            "goal".into(),
            "policy".into(),
            None,
            None,
            None,
            "a".repeat(64),
            vec![child],
            vec![edge],
            provenance
        )
        .is_err()
    );
}

#[test]
fn numeric_storage_is_not_decoded_as_decimal_text() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    repo.create_initial_graph_v1(&Clock("2026-09-27T10:00:00.000000000Z"), request("p", "g"))
        .unwrap();
    let (storage, decimal): (String, String) = repo
        .connection()
        .query_row(
            "SELECT typeof(attempt_number),attempt_number FROM graph_nodes WHERE node_id='n2'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(storage, "text");
    assert_eq!(decimal, format!("1{}", "0".repeat(10000)));
    repo.connection().execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE graph_nodes SET attempt_number=1 WHERE node_id='n2';").unwrap();
    drop(repo);
    let error = ReadOnlyGraphReader::open_existing(&db.0)
        .unwrap()
        .read_current_v1("p", "g")
        .unwrap_err();
    assert!(matches!(
        error,
        StateError::GraphPersistence {
            phase: Phase::Read,
            code: Code::CorruptStore,
            ..
        }
    ));
}

#[test]
fn ledger_failure_rolls_back_all_v13_tables() {
    let db = TempDb::new();
    let repo =
        SqliteStateRepository::open_with_migrations(&db.0, &crate::migrations::registered()[..12])
            .unwrap();
    repo.connection().execute_batch("CREATE TRIGGER fail_ledger BEFORE INSERT ON state_schema_version WHEN NEW.version=13 BEGIN SELECT RAISE(ABORT,'ledger-sentinel'); END").unwrap();
    drop(repo);
    let error = SqliteStateRepository::migrate_existing_to_current(&db.0).unwrap_err();
    assert!(!format!("{error:?} {error}").contains("ledger-sentinel"));
    let repo =
        SqliteStateRepository::open_with_migrations(&db.0, &crate::migrations::registered()[..12])
            .unwrap();
    assert_eq!(repo.schema_version().unwrap(), 12);
    for table in [
        "graphs",
        "graph_versions",
        "graph_nodes",
        "graph_edges",
        "graph_node_capabilities",
        "graph_compiled_sources",
    ] {
        assert!(!repo.table_exists(table).unwrap());
    }
}

#[test]
fn full_tuple_keys_and_orphans_fail_closed() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    let now = Clock("2026-09-27T10:00:00.000000000Z");
    repo.create_initial_graph_v1(&now, request("a", "g"))
        .unwrap();
    repo.create_initial_graph_v1(&now, request("b", "g"))
        .unwrap();
    assert!(repo.connection().execute("INSERT INTO graph_node_capabilities(project_id,graph_id,graph_version,node_id,ordinal,capability) VALUES ('c','g','1','n2',0,'graph.core')",[]).is_err());
    assert!(repo.connection().execute("INSERT INTO graph_edges(project_id,graph_id,graph_version,edge_id,from_node,to_node,edge_class,precedence_kind,control_kind,note) VALUES ('c','g','1','e','n2','n2','CONTROL',NULL,'ON_PASS',NULL)",[]).is_err());
    repo.connection().execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO graph_node_capabilities(project_id,graph_id,graph_version,node_id,ordinal,capability) VALUES ('c','g','1','n2',0,'graph.core')").unwrap();
    drop(repo);
    let error = ReadOnlyGraphReader::open_existing(&db.0).unwrap_err();
    assert!(matches!(
        error,
        StateError::GraphPersistence {
            phase: Phase::Read,
            code: Code::CorruptStore,
            ..
        }
    ));
}

#[test]
fn source_reason_and_clock_diagnostics_never_echo_sentinels() {
    for bad in [
        "evidence:81ARZ3NDEKTSV4RRFFQ69G5FAW",
        "evidence:01arz3NDEKTSV4RRFFQ69G5FAW",
        "/tmp/sentinel",
        "https://sentinel.invalid",
        "evidence:01ARZ3NDEKTSV4RRFFQ69G5FAW\n",
    ] {
        let error = GraphSourceEvidenceRefV1::parse(bad).unwrap_err();
        assert!(!format!("{error:?} {error}").contains(bad));
    }
    let error = GraphGenesisReasonV1::parse("compiled from sentinel").unwrap_err();
    assert!(!format!("{error:?} {error}").contains("sentinel"));
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    let error = repo
        .create_initial_graph_v1(&Clock("sentinel-invalid-time"), request("p", "g"))
        .unwrap_err();
    assert!(matches!(
        error,
        StateError::GraphPersistence {
            phase: Phase::Clock,
            code: Code::ClockInvalid,
            ..
        }
    ));
    assert!(!format!("{error:?} {error}").contains("sentinel"));
    assert!(repo.find_trusted_time_watermark("p").unwrap().is_none());
    let path = std::env::temp_dir().join("missing-sentinel-graph-store/absent.db");
    let error = ReadOnlyGraphReader::open_existing(path).unwrap_err();
    assert!(!format!("{error:?} {error}").contains("sentinel"));
}

#[test]
fn accepted_minimal_vector_fields_and_digest_are_preserved() {
    // PROFILE v1 `minimal`: State stores the supplied accepted digest; it does not hash.
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    let node = StateGraphNodeV1::new(
        "n".into(),
        "g".into(),
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
    let digest = "5672b05de08463af659f705be8241857caa67177c9baed3b97aa0aec1bdc9e4f";
    let request = StateGraphGenesisV1::new(
        "p".into(),
        "g".into(),
        "goal".into(),
        "policy".into(),
        None,
        None,
        None,
        digest.into(),
        vec![node.clone()],
        vec![],
        request("p", "g").genesis_provenance().clone(),
    )
    .unwrap();
    repo.create_initial_graph_v1(&Clock("2026-09-27T10:00:00.000000000Z"), request)
        .unwrap();
    let got = ReadOnlyGraphReader::open_existing(&db.0)
        .unwrap()
        .read_current_v1("p", "g")
        .unwrap()
        .unwrap();
    assert_eq!(got.nodes(), &[node]);
    assert_eq!(got.resulting_digest(), digest);
}

#[test]
fn context_epoch_and_decimal_sql_constraints() {
    let db = TempDb::new();
    let mut repo = SqliteStateRepository::open(&db.0).unwrap();
    let epoch = crate::StateEpochValueV1::try_from(format!("1{}", "0".repeat(10000))).unwrap();
    let request = StateGraphGenesisV1::new(
        "p".into(),
        "g".into(),
        "goal".into(),
        "policy".into(),
        None,
        None,
        Some(epoch.clone()),
        "a".repeat(64),
        vec![bare_node("g", "n", None, None, None).unwrap()],
        vec![],
        request("p", "g").genesis_provenance().clone(),
    )
    .unwrap();
    repo.create_initial_graph_v1(&Clock("2026-09-27T10:00:00.000000000Z"), request)
        .unwrap();
    let got = ReadOnlyGraphReader::open_existing(&db.0)
        .unwrap()
        .read_current_v1("p", "g")
        .unwrap()
        .unwrap();
    assert_eq!(got.context_epoch(), Some(&epoch));
    for invalid in ["01", "1\0bad", "-1", "1e0", ""] {
        assert!(
            repo.connection()
                .execute(
                    "UPDATE graph_nodes SET attempt_number=?1 WHERE node_id='n'",
                    [invalid]
                )
                .is_err()
        );
    }
    assert!(
        repo.connection()
            .execute(
                "UPDATE graph_nodes SET attempt_number=?1 WHERE node_id='n'",
                [1_i64]
            )
            .is_err()
    );
    assert!(
        repo.connection()
            .execute(
                "UPDATE graph_versions SET context_epoch=?1 WHERE graph_id='g'",
                [1_i64]
            )
            .is_err()
    );
}

#[test]
fn nested_debug_and_source_chain_carry_no_graph_content() {
    use std::error::Error;
    let graph = request("secret-sentinel-project", "secret-sentinel-graph");
    assert!(!format!("{graph:?}").contains("secret-sentinel"));
    assert!(!format!("{:?}", graph.nodes()[0]).contains("password rotation"));
    assert!(!format!("{:?}", graph.genesis_provenance()).contains("compiler:"));
    let error = GraphCompilerRefV1::parse("secret-sentinel-compiler").unwrap_err();
    assert!(error.source().is_none());
    assert!(!format!("{error:?} {error}").contains("secret-sentinel"));
}

#[test]
fn reader_sees_absence_then_complete_genesis_after_writer_lock() {
    use std::sync::mpsc;
    struct SignalClock(mpsc::Sender<()>);
    impl TrustedClockV1 for SignalClock {
        fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
            self.0.send(()).unwrap();
            Clock("2026-09-27T10:00:00.000000000Z").sample()
        }
    }
    let db = TempDb::new();
    drop(SqliteStateRepository::open(&db.0).unwrap());
    let reader = ReadOnlyGraphReader::open_existing(&db.0).unwrap();
    let blocker = rusqlite::Connection::open(&db.0).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let (sender, receiver) = mpsc::channel();
    let path = db.0.clone();
    let writer = std::thread::spawn(move || {
        SqliteStateRepository::open(path)
            .unwrap()
            .create_initial_graph_v1(&SignalClock(sender), request("p", "g"))
    });
    receiver.recv().unwrap();
    assert!(reader.read_current_v1("p", "g").unwrap().is_none());
    blocker.execute_batch("COMMIT").unwrap();
    writer.join().unwrap().unwrap();
    let got = reader.read_current_v1("p", "g").unwrap().unwrap();
    assert_eq!(got.nodes().len(), 2);
    assert_eq!(got.edges().len(), 1);
    assert_eq!(got.current_version(), "1");
}

fn insert_valid_noncurrent_version(conn: &rusqlite::Connection, project: &str, graph: &str) {
    conn.execute(
        "INSERT INTO graph_versions SELECT project_id,graph_id,'2',1,'1',goal_id,policy_id,created_at,1,context_epoch,resulting_digest,clock_source_id,clock_contract_version,compiler_id,source_ref,creation_reason FROM graph_versions WHERE project_id=?1 AND graph_id=?2 AND graph_version='1'",
        rusqlite::params![project, graph],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO graph_nodes(project_id,graph_id,graph_version,node_id,kind,state,required_capabilities_present,locked_reason_present) VALUES (?1,?2,'2','v2-node','TASK','PLANNED',1,0)",
        rusqlite::params![project, graph],
    )
    .unwrap();
    for (ordinal, capability) in [(0, "graph.core"), (1, "graph.core")] {
        conn.execute(
            "INSERT INTO graph_node_capabilities(project_id,graph_id,graph_version,node_id,ordinal,capability) VALUES (?1,?2,'2','v2-node',?3,?4)",
            rusqlite::params![project, graph, ordinal, capability],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO graph_edges(project_id,graph_id,graph_version,edge_id,from_node,to_node,edge_class,control_kind) VALUES (?1,?2,'2','v2-edge','v2-node','v2-node','CONTROL','ON_PASS')",
        rusqlite::params![project, graph],
    )
    .unwrap();
    for (ordinal, source) in [(0, "source\0one"), (1, "source\0one")] {
        conn.execute(
            "INSERT INTO graph_compiled_sources(project_id,graph_id,graph_version,ordinal,source) VALUES (?1,?2,'2',?3,?4)",
            rusqlite::params![project, graph, ordinal, source],
        )
        .unwrap();
    }
}

#[test]
fn complete_partition_validation_includes_noncurrent_versions() {
    for sql in [
        "UPDATE graph_versions SET goal_id='' WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_versions SET policy_id='' WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_versions SET created_at='SENTINEL-bad-time' WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_versions SET resulting_digest='SENTINEL-bad-digest' WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_versions SET compiler_id='SENTINEL-compiler' WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_versions SET source_ref='/tmp/SENTINEL' WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_versions SET creation_reason='SENTINEL-reason' WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_nodes SET kind='' WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_nodes SET state='SENTINEL-state' WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_nodes SET required_capabilities_present=0 WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_edges SET control_kind='SENTINEL-kind' WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_versions SET compiled_from_present=0 WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
        "UPDATE graph_node_capabilities SET ordinal=3 WHERE project_id='p' AND graph_id='g' AND graph_version='2' AND ordinal=1",
        "UPDATE graph_compiled_sources SET ordinal=3 WHERE project_id='p' AND graph_id='g' AND graph_version='2' AND ordinal=1",
        "DELETE FROM graph_edges WHERE project_id='p' AND graph_id='g' AND graph_version='2'; DELETE FROM graph_node_capabilities WHERE project_id='p' AND graph_id='g' AND graph_version='2'; DELETE FROM graph_nodes WHERE project_id='p' AND graph_id='g' AND graph_version='2'",
    ] {
        let db = TempDb::new();
        let mut repo = SqliteStateRepository::open(&db.0).unwrap();
        let clock = Clock("2026-09-27T10:00:00.000000000Z");
        for (project, graph) in [("p", "g"), ("q", "g"), ("p", "other")] {
            repo.create_initial_graph_v1(&clock, request(project, graph))
                .unwrap();
        }
        insert_valid_noncurrent_version(repo.connection(), "p", "g");
        drop(repo);
        let reader = ReadOnlyGraphReader::open_existing(&db.0).unwrap();
        let observer = rusqlite::Connection::open(&db.0).unwrap();
        let before: i64 = observer
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .unwrap();
        let current = reader.read_current_v1("p", "g").unwrap().unwrap();
        assert_eq!(current.graph_version(), "1");
        assert_eq!(current.nodes().len(), 2);
        let after: i64 = observer
            .query_row("PRAGMA data_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(before, after);
        drop(observer);
        drop(reader);
        let conn = rusqlite::Connection::open(&db.0).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON").unwrap();
        conn.execute_batch(sql).unwrap();
        drop(conn);
        let reader = ReadOnlyGraphReader::open_existing(&db.0).unwrap();
        let error = reader.read_current_v1("p", "g").unwrap_err();
        assert!(matches!(
            error,
            StateError::GraphPersistence {
                phase: Phase::Read,
                code: Code::CorruptStore,
                ..
            }
        ));
        assert!(!format!("{error:?} {error}").contains("SENTINEL"));
        assert!(reader.read_current_v1("q", "g").unwrap().is_some());
        assert!(reader.read_current_v1("p", "other").unwrap().is_some());
    }
}

#[test]
fn utf16_store_returns_nodes_and_edges_in_utf8_id_order() {
    for encoding in ["UTF-8", "UTF-16le"] {
        let db = TempDb::new();
        let conn = rusqlite::Connection::open(&db.0).unwrap();
        conn.execute_batch(&format!("PRAGMA encoding='{encoding}'"))
            .unwrap();
        drop(conn);
        let mut repo = SqliteStateRepository::open(&db.0).unwrap();
        let graph = "g\0e\u{301}";
        let req = StateGraphGenesisV1::new(
            "p".into(),
            graph.into(),
            "goal".into(),
            "policy".into(),
            None,
            Some(vec!["source\0é".into(), "source\0é".into()]),
            None,
            "a".repeat(64),
            vec![node(graph, "\u{10000}"), node(graph, "\u{e000}")],
            vec![
                StateGraphEdgeV1::new(
                    "\u{10000}".into(),
                    graph.into(),
                    "\u{10000}".into(),
                    "\u{e000}".into(),
                    StateGraphEdgeRelationV1::Control("ON_PASS".into()),
                    Some("é\0e\u{301}".into()),
                )
                .unwrap(),
                StateGraphEdgeV1::new(
                    "\u{e000}".into(),
                    graph.into(),
                    "\u{e000}".into(),
                    "\u{10000}".into(),
                    StateGraphEdgeRelationV1::Control("ON_PASS".into()),
                    None,
                )
                .unwrap(),
            ],
            request("p", graph).genesis_provenance().clone(),
        )
        .unwrap();
        repo.create_initial_graph_v1(&Clock("2026-09-27T10:00:00.000000000Z"), req)
            .unwrap();
        drop(repo);
        let reader = ReadOnlyGraphReader::open_existing(&db.0).unwrap();
        let got = reader.read_current_v1("p", graph).unwrap().unwrap();
        assert_eq!(
            got.nodes()
                .iter()
                .map(StateGraphNodeV1::node_id)
                .collect::<Vec<_>>(),
            ["\u{e000}", "\u{10000}"]
        );
        assert_eq!(
            got.edges()
                .iter()
                .map(StateGraphEdgeV1::edge_id)
                .collect::<Vec<_>>(),
            ["\u{e000}", "\u{10000}"]
        );
        assert_eq!(got.graph_id(), graph);
        assert_eq!(
            got.compiled_from(),
            Some(["source\0é".to_string(), "source\0é".to_string()].as_slice())
        );
        assert_eq!(got.edges()[1].note(), Some("é\0e\u{301}"));
    }
}
