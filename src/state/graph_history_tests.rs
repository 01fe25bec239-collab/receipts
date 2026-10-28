use crate::error::{GraphFailureCode as Code, StateError};
use crate::*;
struct Clock;
impl TrustedClockV1 for Clock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        Ok(TrustedTimeSampleV1 {
            canonical_utc_timestamp: "2026-10-28T03:00:00.000000000Z".into(),
            clock_source_id: "test".into(),
            clock_contract_version: "1".into(),
        })
    }
}
fn content(project: &str, reference: Option<&str>) -> StateGraphGenesisV1 {
    StateGraphGenesisV1::new(
        project.into(),
        "g".into(),
        "goal".into(),
        "policy".into(),
        None,
        Some(vec!["source".into()]),
        None,
        "a".repeat(64),
        vec![
            StateGraphNodeV1::new(
                "n".into(),
                "g".into(),
                "TASK".into(),
                "PLANNED".into(),
                None,
                None,
                None,
                Some(vec!["graph.core".into()]),
                None,
                None,
                None,
                None,
                reference.map(str::to_owned),
                Some(None),
                None,
            )
            .unwrap(),
        ],
        vec![
            StateGraphEdgeV1::new(
                "e".into(),
                "g".into(),
                "n".into(),
                "n".into(),
                StateGraphEdgeRelationV1::Control("ON_PASS".into()),
                None,
            )
            .unwrap(),
        ],
        StateGraphGenesisProvenanceV1::new(
            GraphCompilerRefV1::parse("compiler:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
            GraphSourceEvidenceRefV1::parse("evidence:01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
            GraphGenesisReasonV1::InitialCompilation,
        ),
    )
    .unwrap()
}
fn mutation(parent: &str, version: &str) -> StateGraphMutationV1 {
    StateGraphMutationV1::parse_json(&format!(r#"{{"mutation_id":"m{version}","graph_id":"g","parent_version":{parent},"resulting_version":{version},"actor":{{"role":"USER"}},"reason":"change {version}","created_at":"2026-10-28T03:00:00Z","operations":[{{"op":"SET_NODE_STATE","detail":{{"$serde_json::private::Number":"1","x":1e999999999999999999999}}}}]}}"#)).unwrap()
}
fn result() -> StateGraphNodeResultV1 {
    StateGraphNodeResultV1::parse_json(r#"{"node_id":"n","graph_id":"g","outcome":"PASS","produced_at":"2026-10-28T03:00:00Z","checks":[{"command":["check"],"exit_code":1e999999999999999999999}]}"#).unwrap()
}
fn db() -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "st-history-{}-{}-{sequence}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).expect("exclusive isolated test directory");
    dir.join("state.db")
}
fn cleanup(path: std::path::PathBuf) {
    std::fs::remove_dir_all(path.parent().unwrap()).expect("test directory cleanup");
}

#[test]
fn results_history_binding_roundtrip_and_refusals() {
    let path = db();
    let mut repo = SqliteStateRepository::open(&path).unwrap();
    repo.create_initial_graph_v1(&Clock, content("p", None))
        .unwrap();
    for r in ["z", "A", "a"] {
        repo.append_node_result_v1("p", "1", r, result()).unwrap();
    }
    assert!(matches!(
        repo.append_node_result_v1("p", "1", "a", result()),
        Err(StateError::GraphPersistence {
            code: Code::AlreadyExists,
            ..
        })
    ));
    for (p, v) in [("other", "1"), ("p", "2")] {
        assert!(repo.append_node_result_v1(p, v, "new", result()).is_err());
    }
    let refs = repo.list_node_results_v1("p", "g", "1", "n").unwrap();
    assert_eq!(
        refs.iter().map(|x| x.result_ref()).collect::<Vec<_>>(),
        ["A", "a", "z"]
    );
    repo.append_graph_mutation_v1(
        &Clock,
        mutation("1", "2"),
        StateGraphSnapshotV1::new("2".into(), "1".into(), content("p", Some("a"))).unwrap(),
    )
    .unwrap();
    repo.append_graph_mutation_v1(
        &Clock,
        mutation("2", "999999999999999999999"),
        StateGraphSnapshotV1::new(
            "999999999999999999999".into(),
            "2".into(),
            content("p", Some("a")),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(matches!(
        repo.append_graph_mutation_v1(
            &Clock,
            mutation("1", "3"),
            StateGraphSnapshotV1::new("3".into(), "1".into(), content("p", None)).unwrap()
        ),
        Err(StateError::GraphPersistence {
            code: Code::ParentConflict,
            ..
        })
    ));
    let history = repo.list_graph_mutations_v1("p", "g").unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].actor_role(), "USER");
    let current = repo.load_current_graph_v1("p", "g").unwrap().unwrap();
    let genesis = repo.load_graph_version_v1("p", "g", "1").unwrap().unwrap();
    assert_eq!(current.genesis_provenance(), genesis.genesis_provenance());
    assert_eq!(
        repo.find_node_result_v1("p", "g", "a")
            .unwrap()
            .unwrap()
            .graph_version(),
        "1"
    );
    drop(repo);
    let reader = ReadOnlyGraphReader::open_existing(&path).unwrap();
    assert_eq!(reader.list_graph_mutations_v1("p", "g").unwrap().len(), 2);
    assert!(
        reader
            .find_node_result_v1("other", "g", "a")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        reader
            .load_graph_version_v1("p", "g", "2")
            .unwrap()
            .unwrap()
            .graph_version(),
        "2"
    );
    drop(reader);
    cleanup(path);
}
#[test]
fn immutable_sql_and_failed_transaction_preserve_head() {
    let path = db();
    let mut repo = SqliteStateRepository::open(&path).unwrap();
    repo.create_initial_graph_v1(&Clock, content("p", None))
        .unwrap();
    repo.append_node_result_v1("p", "1", "r", result()).unwrap();
    for table in ["graph_versions", "graph_nodes", "graph_node_results"] {
        for sql in [
            format!("DELETE FROM {table}"),
            format!("UPDATE {table} SET project_id=project_id"),
            format!("INSERT OR REPLACE INTO {table} SELECT * FROM {table}"),
        ] {
            assert!(repo.connection().execute_batch(&sql).is_err(), "{sql}");
        }
    }
    repo.connection().execute_batch("CREATE TRIGGER fail_mutation BEFORE INSERT ON graph_mutations BEGIN SELECT RAISE(ABORT,'test'); END;").unwrap();
    assert!(
        repo.append_graph_mutation_v1(
            &Clock,
            mutation("1", "2"),
            StateGraphSnapshotV1::new("2".into(), "1".into(), content("p", None)).unwrap()
        )
        .is_err()
    );
    assert_eq!(
        repo.load_current_graph_v1("p", "g")
            .unwrap()
            .unwrap()
            .graph_version(),
        "1"
    );
    assert!(repo.load_graph_version_v1("p", "g", "2").unwrap().is_none());
    assert!(repo.list_graph_mutations_v1("p", "g").unwrap().is_empty());
    drop(repo);
    cleanup(path);
}

/// Test-only controlled disk corruption with the original protections restored.
pub(crate) fn corrupt_graph(conn: &rusqlite::Connection, sql: &str) {
    let triggers:Vec<(String,String)>=conn.prepare("SELECT name,sql FROM sqlite_master WHERE type='trigger' AND name GLOB 'graph*_immutable_*'").unwrap().query_map([],|r|Ok((r.get(0)?,r.get(1)?))).unwrap().collect::<Result<_,_>>().unwrap();
    for (name, _) in &triggers {
        conn.execute_batch(&format!("DROP TRIGGER {name}")).unwrap();
    }
    conn.execute_batch(sql).unwrap();
    for (_, ddl) in &triggers {
        conn.execute_batch(ddl).unwrap();
    }
}
pub(crate) fn remove_v15(conn: &rusqlite::Connection) {
    let names:Vec<String>=conn.prepare("SELECT name FROM sqlite_master WHERE type='trigger' AND name GLOB 'graph*_immutable_*'").unwrap().query_map([],|r|r.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
    for n in names {
        conn.execute_batch(&format!("DROP TRIGGER {n}")).unwrap();
    }
    conn.execute_batch("DROP TABLE graph_mutations; DROP TABLE graph_node_results; DROP TABLE graph_snapshot_seals; DELETE FROM state_schema_version WHERE version=15;").unwrap();
}

#[test]
fn concurrent_cas_exactly_one_winner() {
    let path = db();
    let mut repo = SqliteStateRepository::open(&path).unwrap();
    repo.create_initial_graph_v1(&Clock, content("p", None))
        .unwrap();
    drop(repo);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut jobs = vec![];
    for version in ["2", "3"] {
        let path = path.clone();
        let b = barrier.clone();
        jobs.push(std::thread::spawn(move || {
            let mut repo = SqliteStateRepository::open(path).unwrap();
            b.wait();
            repo.append_graph_mutation_v1(
                &Clock,
                mutation("1", version),
                StateGraphSnapshotV1::new(version.into(), "1".into(), content("p", None)).unwrap(),
            )
        }));
    }
    let results: Vec<_> = jobs.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(
                r,
                Err(StateError::GraphPersistence {
                    code: Code::ParentConflict,
                    ..
                })
            ))
            .count(),
        1
    );
    assert_eq!(
        SqliteStateRepository::open(&path)
            .unwrap()
            .list_graph_mutations_v1("p", "g")
            .unwrap()
            .len(),
        1
    );
    cleanup(path);
}
#[test]
fn late_children_and_all_history_mutation_matrix() {
    for migrated in [false, true] {
        let path = db();
        let mut repo = SqliteStateRepository::open(&path).unwrap();
        repo.create_initial_graph_v1(&Clock, content("p", None))
            .unwrap();
        if migrated {
            remove_v15(repo.connection());
            drop(repo);
            assert!(SqliteStateRepository::open(&path).is_err());
            assert_eq!(
                SqliteStateRepository::migrate_existing_to_current(&path).unwrap(),
                15
            );
            repo = SqliteStateRepository::open(&path).unwrap();
            let legacy: i64 = repo
                .connection()
                .query_row("SELECT legacy FROM graph_snapshot_seals", [], |r| r.get(0))
                .unwrap();
            assert_eq!(legacy, 1);
        }
        repo.append_node_result_v1("p", "1", "r", result()).unwrap();
        repo.append_graph_mutation_v1(
            &Clock,
            mutation("1", "2"),
            StateGraphSnapshotV1::new("2".into(), "1".into(), content("p", Some("r"))).unwrap(),
        )
        .unwrap();
        for table in [
            "graph_versions",
            "graph_nodes",
            "graph_edges",
            "graph_node_capabilities",
            "graph_compiled_sources",
            "graph_node_results",
            "graph_mutations",
            "graph_snapshot_seals",
        ] {
            for action in [
                format!("DELETE FROM {table}"),
                format!("UPDATE {table} SET project_id=project_id"),
                format!("INSERT OR REPLACE INTO {table} SELECT * FROM {table}"),
            ] {
                let count: i64 = repo
                    .connection()
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                    .unwrap();
                assert!(count > 0, "matrix must reach a populated {table}");
                {
                    assert!(
                        repo.connection().execute_batch(&action).is_err(),
                        "{action}"
                    );
                }
            }
        }
        for version in ["1", "2"] {
            for sql in [
                format!(
                    "INSERT INTO graph_nodes(project_id,graph_id,graph_version,node_id,kind,state,required_capabilities_present,locked_reason_present) VALUES ('p','g','{version}','late','TASK','PLANNED',0,0)"
                ),
                format!(
                    "INSERT INTO graph_edges VALUES ('p','g','{version}','late','n','n','CONTROL',NULL,'ON_PASS',NULL)"
                ),
                format!(
                    "INSERT INTO graph_node_capabilities VALUES ('p','g','{version}','n',1,'graph.core')"
                ),
                format!("INSERT INTO graph_compiled_sources VALUES ('p','g','{version}',1,'late')"),
            ] {
                assert!(repo.connection().execute_batch(&sql).is_err(), "{sql}");
            }
        }
        drop(repo);
        cleanup(path);
    }
}
#[test]
fn corrupt_associations_and_seals_fail_current_and_historical_reads() {
    for sql in [
        "DELETE FROM graph_snapshot_seals WHERE graph_version='1'",
        "UPDATE graph_snapshot_seals SET graph_id='other' WHERE graph_version='2'",
        "DELETE FROM graph_mutations",
        "UPDATE graph_snapshot_seals SET legacy=1 WHERE graph_version='2'",
        "UPDATE graph_mutations SET mutation_json='{}'",
        "UPDATE graph_node_results SET node_id='wrong'",
        "UPDATE graph_node_results SET result_json='{}'",
        "UPDATE graph_nodes SET result_ref='missing' WHERE graph_version='2'",
    ] {
        let path = db();
        let mut repo = SqliteStateRepository::open(&path).unwrap();
        repo.create_initial_graph_v1(&Clock, content("p", None))
            .unwrap();
        repo.append_node_result_v1("p", "1", "r", result()).unwrap();
        repo.append_graph_mutation_v1(
            &Clock,
            mutation("1", "2"),
            StateGraphSnapshotV1::new("2".into(), "1".into(), content("p", Some("r"))).unwrap(),
        )
        .unwrap();
        repo.connection()
            .execute_batch("PRAGMA foreign_keys=OFF")
            .unwrap();
        corrupt_graph(repo.connection(), sql);
        assert!(repo.load_current_graph_v1("p", "g").is_err(), "{sql}");
        assert!(repo.load_graph_version_v1("p", "g", "1").is_err(), "{sql}");
        assert!(repo.find_node_result_v1("p", "g", "r").is_err(), "{sql}");
        drop(repo);
        cleanup(path);
    }
}
struct LaterClock;
impl TrustedClockV1 for LaterClock {
    fn sample(&self) -> Result<TrustedTimeSampleV1, StateError> {
        let mut s = Clock.sample()?;
        s.canonical_utc_timestamp = "2026-10-28T04:00:00.000000000Z".into();
        Ok(s)
    }
}
#[test]
fn later_clock_child_and_commit_failures_rollback_everything() {
    for trigger in [
        "CREATE TRIGGER fail_child BEFORE INSERT ON graph_nodes WHEN NEW.graph_version='2' BEGIN SELECT RAISE(ABORT,'child failure'); END;",
        "CREATE TRIGGER fail_commit AFTER INSERT ON graph_mutations BEGIN UPDATE graphs SET current_version='missing'; END;",
    ] {
        let path = db();
        let mut repo = SqliteStateRepository::open(&path).unwrap();
        repo.create_initial_graph_v1(&Clock, content("p", None))
            .unwrap();
        let before:String=repo.connection().query_row("SELECT last_accepted_trusted_time FROM trusted_time_watermark WHERE project_id='p'",[],|r|r.get(0)).unwrap();
        repo.connection().execute_batch(trigger).unwrap();
        assert!(
            repo.append_graph_mutation_v1(
                &LaterClock,
                mutation("1", "2"),
                StateGraphSnapshotV1::new("2".into(), "1".into(), content("p", None)).unwrap()
            )
            .is_err()
        );
        let after:String=repo.connection().query_row("SELECT last_accepted_trusted_time FROM trusted_time_watermark WHERE project_id='p'",[],|r|r.get(0)).unwrap();
        assert_eq!(before, after);
        assert_eq!(
            repo.load_current_graph_v1("p", "g")
                .unwrap()
                .unwrap()
                .graph_version(),
            "1"
        );
        assert!(repo.load_graph_version_v1("p", "g", "2").unwrap().is_none());
        assert!(repo.list_graph_mutations_v1("p", "g").unwrap().is_empty());
        let seals: i64 = repo
            .connection()
            .query_row("SELECT COUNT(*) FROM graph_snapshot_seals", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(seals, 1);
        drop(repo);
        cleanup(path);
    }
}

#[test]
fn migration_preserves_attempts_and_failure_leaves_exact_v14() {
    let path = db();
    let mut repo = SqliteStateRepository::open(&path).unwrap();
    repo.create_initial_graph_v1(&Clock, content("p", Some("legacy-ref")))
        .unwrap();
    let aid = AttemptId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    let start = AttemptStart::new(
        aid.clone(),
        "p",
        "g",
        "n",
        "1",
        AttemptRole::RuntimeA3,
        "runtime",
        "workspace",
        &"a".repeat(40),
        "2026-10-28T03:00:00.000000000Z",
    )
    .unwrap();
    repo.append_attempt_started(start).unwrap();
    remove_v15(repo.connection());
    drop(repo);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_v15 BEFORE INSERT ON state_schema_version WHEN NEW.version=15 BEGIN SELECT RAISE(ABORT,'v15 failure'); END;").unwrap();
    drop(conn);
    assert!(SqliteStateRepository::migrate_existing_to_current(&path).is_err());
    let conn = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        conn.query_row("SELECT MAX(version) FROM state_schema_version", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        14
    );
    crate::graph_repository::verify_v14(&conn, crate::error::GraphPhase::Read).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE name='graph_snapshot_seals'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
    conn.execute_batch("DROP TRIGGER fail_v15").unwrap();
    drop(conn);
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(&path).unwrap(),
        15
    );
    let repo = SqliteStateRepository::open(&path).unwrap();
    assert!(repo.find_attempt(&aid).unwrap().is_some());
    let g = repo.load_current_graph_v1("p", "g").unwrap().unwrap();
    assert_eq!(g.nodes()[0].result_ref(), Some("legacy-ref"));
    assert_eq!(
        g.genesis_provenance(),
        content("p", None).genesis_provenance()
    );
    drop(repo);
    cleanup(path);
}
#[test]
fn result_refs_binary_nul_unicode_and_exact_historical_node_tuple() {
    let path = db();
    let mut repo = SqliteStateRepository::open(&path).unwrap();
    for p in ["p", "q"] {
        repo.create_initial_graph_v1(&Clock, content(p, None))
            .unwrap();
    }
    let refs = ["a\0z", "a\0a", "é", "😀", "A"];
    for r in refs {
        repo.append_node_result_v1("p", "1", r, result()).unwrap();
    }
    repo.append_node_result_v1("q", "1", "A", result()).unwrap();
    let list = repo.list_node_results_v1("p", "g", "1", "n").unwrap();
    let mut expected = refs.to_vec();
    expected.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(
        list.iter().map(|x| x.result_ref()).collect::<Vec<_>>(),
        expected
    );
    let wrong=StateGraphNodeResultV1::parse_json(r#"{"node_id":"wrong","graph_id":"g","outcome":"PASS","produced_at":"2026-10-28T03:00:00Z"}"#).unwrap();
    assert!(
        repo.append_node_result_v1("p", "1", "wrong", wrong)
            .is_err()
    );
    assert!(
        repo.append_graph_mutation_v1(
            &Clock,
            mutation("1", "2"),
            StateGraphSnapshotV1::new("2".into(), "1".into(), content("p", Some("q-only")))
                .unwrap()
        )
        .is_err()
    );
    assert_eq!(
        repo.load_current_graph_v1("p", "g")
            .unwrap()
            .unwrap()
            .graph_version(),
        "1"
    );
    drop(repo);
    cleanup(path);
}

#[test]
fn migrated_historical_version_refuses_late_mutation() {
    let path = db();
    let mut repo = SqliteStateRepository::open(&path).unwrap();
    repo.create_initial_graph_v1(&Clock, content("p", None))
        .unwrap();
    repo.append_graph_mutation_v1(
        &Clock,
        mutation("1", "2"),
        StateGraphSnapshotV1::new("2".into(), "1".into(), content("p", None)).unwrap(),
    )
    .unwrap();
    remove_v15(repo.connection());
    drop(repo);
    SqliteStateRepository::migrate_existing_to_current(&path).unwrap();
    let repo = SqliteStateRepository::open(&path).unwrap();
    assert!(
        repo.connection()
            .execute(
                "INSERT INTO graph_mutations VALUES ('p','g','late','1','2',?1)",
                [mutation("1", "2").to_json().unwrap()]
            )
            .is_err()
    );
    assert!(repo.list_graph_mutations_v1("p", "g").unwrap().is_empty());
    assert_eq!(
        repo.load_graph_version_v1("p", "g", "2")
            .unwrap()
            .unwrap()
            .graph_version(),
        "2"
    );
    drop(repo);
    cleanup(path);
}

#[test]
fn empty_result_lists_validate_partition_for_both_readers() {
    let path = db();
    let mut repo = SqliteStateRepository::open(&path).unwrap();
    repo.create_initial_graph_v1(&Clock, content("p", None))
        .unwrap();
    let reader = ReadOnlyGraphReader::open_existing(&path).unwrap();
    assert!(
        repo.list_node_results_v1("p", "g", "1", "n")
            .unwrap()
            .is_empty()
    );
    assert!(
        reader
            .list_node_results_v1("p", "g", "1", "n")
            .unwrap()
            .is_empty()
    );
    corrupt_graph(
        repo.connection(),
        "DELETE FROM graph_snapshot_seals WHERE project_id='p' AND graph_id='g' AND graph_version='1'",
    );
    let repository_result = repo.list_node_results_v1("p", "g", "1", "n");
    let reader_result = reader.list_node_results_v1("p", "g", "1", "n");
    let control = reader.read_current_v1("p", "g");
    assert!(matches!(
        control,
        Err(StateError::GraphPersistence {
            code: Code::CorruptStore,
            ..
        })
    ));
    assert!(
        matches!(
            repository_result,
            Err(StateError::GraphPersistence {
                code: Code::CorruptStore,
                ..
            })
        ),
        "repository list bypassed corruption: {repository_result:?}"
    );
    assert!(
        matches!(
            reader_result,
            Err(StateError::GraphPersistence {
                code: Code::CorruptStore,
                ..
            })
        ),
        "read-only list bypassed corruption: {reader_result:?}"
    );
    drop(reader);
    drop(repo);
    cleanup(path);
}
