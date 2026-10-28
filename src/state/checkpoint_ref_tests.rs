use crate::error::{GraphFailureCode as Code, StateError};
use crate::tests::TempDir;
use crate::{CheckpointRecord, OutputRef, ReadOnlyGraphReader, SqliteStateRepository};
use rusqlite::Connection;

pub(crate) fn remove_v16(conn: &Connection) {
    conn.execute_batch("DROP TABLE workspace_checkpoints; DROP TABLE output_refs; DELETE FROM state_schema_version WHERE version=16;").unwrap();
}
fn checkpoint(p: &str, id: &str, date: &str) -> CheckpointRecord {
    CheckpointRecord::parse_json(p,&format!(r#"{{"checkpoint_id":{id},"workspace_id":"w","head_sha":"{}","kind":"PROGRESS","captured_at":"{date}"}}"#, "a".repeat(40))).unwrap()
}
fn full(exit: &str) -> String {
    format!(
        r#"{{"checkpoint_id":"c","workspace_id":"w","head_sha":"{sha}","captured_at":"2000-02-29t01:00:00.123456789123456789+01:00","kind":"RECOVERY_CAPTURE","task_id":"t","attempt_id":"a","base_sha":"{sha}","dirty_diff_ref":{{"ref_type":"URL","target":"\u0000opaque","digest":"","section":""}},"modified_files":["","a","a"],"untracked_files":["😀"],"executed_checks":[{{"source":"BROKER_EXECUTION","command":["","--","😀"],"exit_code":{exit},"code_sha":"{sha}","timed_out":false,"started_at":"2000-02-29T23:59:60Z","finished_at":"2000-03-01T00:59:60+01:00","output_ref":{{"ref_type":"ARTIFACT_ID","target":"ref","digest":"opaque\u0000digest","section":"s"}},"result":"UNKNOWN"}}],"crash_classification":"UNKNOWN","recovery_decision":"INSPECT_AND_SALVAGE","recovery_rationale":""}}"#,
        sha = "a".repeat(40)
    )
}
#[test]
fn checkpoint_complete_exact_roundtrip_first_use_partitions_and_reopen() {
    let tmp = TempDir::new("checkpoint-roundtrip");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    for p in ["p", "q"] {
        let value =
            CheckpointRecord::parse_json(p, &full("-184467440737095516170000000000001e1000001"))
                .unwrap();
        let expected = value.to_json().unwrap();
        repo.append_checkpoint(value).unwrap();
        assert_eq!(
            repo.find_checkpoint(p, "c")
                .unwrap()
                .unwrap()
                .to_json()
                .unwrap(),
            expected
        );
        assert!(
            repo.append_checkpoint(CheckpointRecord::parse_json(p, &expected).unwrap())
                .is_err()
        );
        assert!(
            repo.append_checkpoint(
                CheckpointRecord::parse_json(p, &expected.replace("\"w\"", "\"other\"")).unwrap()
            )
            .is_err()
        );
    }
    assert!(repo.find_checkpoint("wrong", "c").unwrap().is_none());
    assert!(repo.list_checkpoints("wrong", "w").unwrap().is_empty());
    assert!(repo.latest_checkpoint("p", "other").unwrap().is_none());
    assert_eq!(repo.count_table_rows("trusted_time_watermark").unwrap(), 0);
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let value = repo.latest_checkpoint("p", "w").unwrap().unwrap();
    assert_eq!(
        value.captured_at(),
        "2000-02-29t01:00:00.123456789123456789+01:00"
    );
    assert!(
        value
            .to_json()
            .unwrap()
            .contains("-184467440737095516170000000000001e1000001")
    );
    assert_eq!(repo.list_checkpoints("q", "w").unwrap().len(), 1);
}
#[test]
fn checkpoint_order_offsets_fractions_leaps_and_binary_ties() {
    let entries = [
        ("a", "1999-12-31T23:59:59.99999999999999999999Z"),
        ("b", "2000-01-01T00:59:60+01:00"),
        ("c", "1999-12-31T23:59:60.00000000000000000001Z"),
        ("d", "2000-01-01T00:00:00Z"),
        ("e", "2000-01-01T00:00:00.00000000000000000001Z"),
        ("f", "2000-01-01T00:00:00.1Z"),
        ("z", "1999-12-31T23:00:00.100000000000000000-01:00"),
        ("é", "2000-01-01t00:00:00.10z"),
        ("😀", "2000-01-01T23:59:00.1+23:59"),
    ];
    for reversed in [false, true] {
        let tmp = TempDir::new("checkpoint-order");
        let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        let mut items = entries.to_vec();
        if reversed {
            items.reverse();
        }
        for (id, date) in items {
            repo.append_checkpoint(checkpoint("p", &serde_json::to_string(id).unwrap(), date))
                .unwrap();
        }
        let list = repo.list_checkpoints("p", "w").unwrap();
        assert_eq!(
            list.iter().map(|c| c.checkpoint_id()).collect::<Vec<_>>(),
            entries.iter().map(|x| x.0).collect::<Vec<_>>()
        );
        for (value, (_, date)) in list.iter().zip(entries) {
            assert_eq!(value.captured_at(), date);
        }
        assert_eq!(
            repo.latest_checkpoint("p", "w")
                .unwrap()
                .unwrap()
                .checkpoint_id(),
            "😀"
        );
    }
}
#[test]
fn checkpoint_schema_refusals_are_content_free_and_exact_integer_only() {
    for exit in [
        "0",
        "1.5e1",
        "1e999999999999999999999999",
        "-0e-999999999999999999",
    ] {
        let v = CheckpointRecord::parse_json("p", &full(exit)).unwrap();
        assert!(v.to_json().unwrap().contains(exit));
    }
    for exit in [
        "1.1",
        "1e-9999999999999999",
        "null",
        "true",
        "[]",
        r#"{"$serde_json::private::Number":"1"}"#,
        r#"{"$serde_json::private::RawValue":"1"}"#,
    ] {
        assert!(CheckpointRecord::parse_json("p", &full(exit)).is_err());
    }
    let raw = full("0");
    for bad in [
        raw.replace("\"checkpoint_id\":\"c\"", "\"checkpoint_id\":\"\""),
        raw.replace("\"workspace_id\":\"w\"", "\"workspace_id\":null"),
        raw.replace("\"kind\":\"RECOVERY_CAPTURE\"", "\"kind\":\"SECRET\""),
        raw.replace("\"task_id\":\"t\"", "\"task_id\":null"),
        raw.replace("\"digest\":\"\"", "\"digest\":null"),
        raw.replace("\"target\":\"ref\"", "\"target\":\"\""),
        raw.replace("\"source\":\"BROKER_EXECUTION\"", "\"source\":\"bad\""),
        raw.replace("\"command\":[\"\",\"--\",\"😀\"]", "\"command\":[]"),
        raw.replace("\"result\":\"UNKNOWN\"", "\"result\":\"bad\""),
        raw.replace("2000-02-29t", "1900-02-29t"),
        raw.replace("\"timed_out\":false", "\"timed_out\":0"),
        raw.replace(
            "\"modified_files\":[\"\",\"a\",\"a\"]",
            "\"modified_files\":[0]",
        ),
        raw.replace("\"kind\":", "\"unknown\":\"SECRET\",\"kind\":"),
        raw.replace(
            "\"checkpoint_id\":",
            "\"checkpoint_id\":\"duplicate\",\"checkpoint_id\":",
        ),
        "{SECRET".into(),
    ] {
        let error = CheckpointRecord::parse_json("p", &bad).unwrap_err();
        assert!(!format!("{error} {error:?}").contains("SECRET"));
    }
    assert!(CheckpointRecord::parse_json("", &raw).is_err());
    assert!(CheckpointRecord::parse_json(&"x".repeat(201), &raw).is_err());
}
#[test]
fn checkpoint_and_output_sql_immutability_rollback_and_schema_tamper() {
    for table in ["workspace_checkpoints", "output_refs"] {
        let tmp = TempDir::new("reference-immutability");
        let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        repo.append_checkpoint(checkpoint("p", "\"c\"", "2000-01-01T00:00:00Z"))
            .unwrap();
        repo.append_output_ref(OutputRef::new("p", "r", &"a".repeat(64), "opaque", "0").unwrap())
            .unwrap();
        for sql in [
            format!("UPDATE {table} SET project_id=project_id"),
            format!("DELETE FROM {table}"),
            format!("INSERT OR REPLACE INTO {table} SELECT * FROM {table}"),
        ] {
            assert!(repo.connection().execute_batch(&sql).is_err());
            assert_eq!(repo.count_table_rows(table).unwrap(), 1);
        }
        let trigger = format!("{table}_no_update");
        let ddl: String = repo
            .connection()
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name=?1",
                [&trigger],
                |r| r.get(0),
            )
            .unwrap();
        repo.connection()
            .execute_batch(&format!("DROP TRIGGER {trigger}"))
            .unwrap();
        assert!(repo.find_checkpoint("p", "c").is_err());
        assert!(repo.find_output_ref("p", "r").is_err());
        assert!(SqliteStateRepository::open(tmp.db_path()).is_err());
        assert!(
            repo.append_checkpoint(checkpoint("p", "\"new\"", "2000-01-01T00:00:00Z"))
                .is_err()
        );
        assert_eq!(repo.count_table_rows("workspace_checkpoints").unwrap(), 1);
        assert_eq!(repo.count_table_rows("trusted_time_watermark").unwrap(), 0);
        repo.connection().execute_batch(&ddl).unwrap();
        repo.connection().execute_batch(&format!("DROP TRIGGER {trigger}; CREATE TRIGGER {trigger} BEFORE UPDATE ON {table} BEGIN SELECT 1; END;")).unwrap();
        assert!(repo.latest_checkpoint("p", "w").is_err());
    }
}
#[test]
fn checkpoint_corrupt_json_and_physical_identity_fail_closed() {
    for sql in [
        "UPDATE workspace_checkpoints SET checkpoint_json='{'",
        "UPDATE workspace_checkpoints SET checkpoint_id='mismatch'",
        "UPDATE workspace_checkpoints SET workspace_id='mismatch'",
    ] {
        let tmp = TempDir::new("checkpoint-corrupt");
        let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        repo.append_checkpoint(checkpoint("p", "\"c\"", "2000-01-01T00:00:00Z"))
            .unwrap();
        let ddl: String = repo
            .connection()
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='workspace_checkpoints_no_update'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        repo.connection()
            .execute_batch("DROP TRIGGER workspace_checkpoints_no_update")
            .unwrap();
        repo.connection().execute_batch(sql).unwrap();
        repo.connection().execute_batch(&ddl).unwrap();
        let id = if sql.contains("checkpoint_id=") {
            "mismatch"
        } else {
            "c"
        };
        assert!(matches!(
            repo.find_checkpoint("p", id),
            Err(StateError::GraphPersistence {
                code: Code::CorruptStore,
                ..
            })
        ));
        let workspace = if sql.contains("workspace_id=") {
            "mismatch"
        } else {
            "w"
        };
        assert!(repo.list_checkpoints("p", workspace).is_err());
    }
}
#[test]
fn v15_to_v16_failure_rolls_back_and_v15_postcondition_is_enforced() {
    let tmp = TempDir::new("v16-rollback");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let aid = crate::AttemptId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
    repo.append_attempt_started(
        crate::AttemptStart::new(
            aid.clone(),
            "p",
            "g",
            "n",
            "1",
            crate::AttemptRole::RuntimeA3,
            "r",
            "w",
            &"a".repeat(40),
            "2000-01-01T00:00:00.000000000Z",
        )
        .unwrap(),
    )
    .unwrap();
    remove_v16(repo.connection());
    repo.connection().execute_batch("CREATE TRIGGER fail_v16 BEFORE INSERT ON state_schema_version WHEN NEW.version=16 BEGIN SELECT RAISE(ABORT,'fail'); END;").unwrap();
    drop(repo);
    assert!(SqliteStateRepository::open(tmp.db_path()).is_err());
    assert!(ReadOnlyGraphReader::open_existing(tmp.db_path()).is_err());
    assert!(SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).is_err());
    let conn = Connection::open(tmp.db_path()).unwrap();
    crate::graph_repository::verify_v15(&conn, crate::GraphPhase::Read).unwrap();
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name IN ('workspace_checkpoints','output_refs')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    conn.execute_batch("DROP TRIGGER fail_v16").unwrap();
    assert_eq!(
        SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
        16
    );
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert!(repo.find_attempt(&aid).unwrap().is_some());
    ReadOnlyGraphReader::open_existing(tmp.db_path()).unwrap();
}

#[test]
fn v11_through_v15_maintenance_preserves_rows_and_v15_intermediate_postcondition() {
    for version in 11..=15 {
        let tmp = TempDir::new("checkpoint-migration-matrix");
        let repo = SqliteStateRepository::open_with_migrations(
            tmp.db_path(),
            &crate::migrations::registered()[..version],
        )
        .unwrap();
        repo.connection().execute_batch("INSERT INTO logical_role(role_id,project_id,role_type,status,current_context_epoch) VALUES ('sentinel','p','RUNTIME_A1','ACTIVE',0)").unwrap();
        if version == 14 {
            crate::graph_repository::migrate_history_store(repo.connection()).unwrap();
            crate::graph_repository::verify_v15(repo.connection(), crate::GraphPhase::Read)
                .unwrap();
            assert!(!repo.table_exists("workspace_checkpoints").unwrap());
            assert!(!repo.table_exists("output_refs").unwrap());
        }
        drop(repo);
        assert!(SqliteStateRepository::open(tmp.db_path()).is_err());
        assert!(ReadOnlyGraphReader::open_existing(tmp.db_path()).is_err());
        assert_eq!(
            SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
            16
        );
        let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        assert_eq!(repo.count_table_rows("logical_role").unwrap(), 1);
        assert_eq!(
            repo.connection()
                .query_row(
                    "SELECT current_context_epoch FROM logical_role WHERE role_id='sentinel'",
                    [],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
            "0"
        );
        assert_eq!(repo.schema_version().unwrap(), 16);
        ReadOnlyGraphReader::open_existing(tmp.db_path()).unwrap();
    }
}
#[test]
fn checkpoint_nested_optional_digest_absence_and_opaque_identity() {
    let tmp = TempDir::new("checkpoint-optional");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let raw = full("0")
        .replace(",\"digest\":\"\"", "")
        .replace(",\"digest\":\"opaque\\u0000digest\"", "")
        .replace("RECOVERY_CAPTURE", "PRE_TERMINATION");
    let raw = raw
        .replace(
            "\"checkpoint_id\":\"c\"",
            "\"checkpoint_id\":\"c\\u0000😀\"",
        )
        .replace("\"workspace_id\":\"w\"", "\"workspace_id\":\"w\\u0000😀\"");
    let value = CheckpointRecord::parse_json("p\0😀", &raw).unwrap();
    let expected = value.to_json().unwrap();
    repo.append_checkpoint(value).unwrap();
    assert_eq!(
        repo.find_checkpoint("p\0😀", "c\0😀")
            .unwrap()
            .unwrap()
            .to_json()
            .unwrap(),
        expected
    );
    assert_eq!(
        repo.latest_checkpoint("p\0😀", "w\0😀")
            .unwrap()
            .unwrap()
            .to_json()
            .unwrap(),
        expected
    );
    assert!(repo.find_checkpoint("p", "c\0😀").unwrap().is_none());
}
#[test]
fn new_table_definition_tampering_refuses_even_absent_reads_and_appends() {
    for table in ["workspace_checkpoints", "output_refs"] {
        let tmp = TempDir::new("checkpoint-table-tamper");
        let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        repo.connection()
            .execute_batch(&format!("ALTER TABLE {table} ADD COLUMN unexpected TEXT"))
            .unwrap();
        assert!(repo.find_checkpoint("p", "absent").is_err());
        assert!(repo.list_checkpoints("p", "absent").is_err());
        assert!(repo.latest_checkpoint("p", "absent").is_err());
        assert!(repo.find_output_ref("p", "absent").is_err());
        assert!(
            repo.append_output_ref(OutputRef::new("p", "r", &"a".repeat(64), "uri", "0").unwrap())
                .is_err()
        );
        assert!(SqliteStateRepository::open(tmp.db_path()).is_err());
        assert!(ReadOnlyGraphReader::open_existing(tmp.db_path()).is_err());
        assert!(SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).is_err());
    }
}
