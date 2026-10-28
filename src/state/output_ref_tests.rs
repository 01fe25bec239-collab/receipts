use crate::error::GraphFailureCode as Code;
use crate::tests::TempDir;
use crate::{OutputRef, SqliteStateRepository, StateError};

#[test]
fn output_metadata_exact_unbounded_opaque_and_project_isolated_reopen() {
    let tmp = TempDir::new("output-roundtrip");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let long = "😀\0".repeat(300);
    let size = "9".repeat(1000);
    for p in ["p", "q"] {
        for reference in ["\0", "é", long.as_str()] {
            let value = OutputRef::new(
                p,
                reference,
                &"a".repeat(64),
                "\0opaque://not-fetched",
                &size,
            )
            .unwrap();
            repo.append_output_ref(value.clone()).unwrap();
            assert!(repo.append_output_ref(value.clone()).is_err());
            assert_eq!(repo.find_output_ref(p, reference).unwrap().unwrap(), value);
        }
    }
    assert!(repo.find_output_ref("wrong", &long).unwrap().is_none());
    assert!(repo.find_output_ref("p", "absent").unwrap().is_none());
    assert_eq!(repo.count_table_rows("trusted_time_watermark").unwrap(), 0);
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let value = repo.find_output_ref("p", &long).unwrap().unwrap();
    assert_eq!(value.byte_length(), size);
    assert_eq!(value.storage_uri(), "\0opaque://not-fetched");
    assert_eq!(value.sha256(), "a".repeat(64));
    assert_eq!(value.project_id(), "p");
    assert_eq!(value.output_ref(), long);
}
#[test]
fn output_invalid_metadata_refuses_without_echo() {
    for (p, r, d, u, n) in [
        ("", "r", "a".repeat(64), "u", "0"),
        ("p", "", "a".repeat(64), "u", "0"),
        ("p", "SECRET", "A".repeat(64), "u", "0"),
        ("p", "r", "a".repeat(63), "u", "0"),
        ("p", "r", "a".repeat(64), "", "0"),
    ] {
        let e = OutputRef::new(p, r, &d, u, n).unwrap_err();
        assert!(!format!("{e:?} {e}").contains("SECRET"));
    }
    for n in [
        "", "00", "01", "-0", "-1", "+1", "1.0", "1e3", " 1", "1\0", "１",
    ] {
        assert!(OutputRef::new("p", "r", &"a".repeat(64), "u", n).is_err());
    }
    for n in ["0", "1", "99999999999999999999999999999999"] {
        assert!(OutputRef::new("p", "r", &"a".repeat(64), "u", n).is_ok());
    }
}
#[test]
fn output_corrupt_rows_fail_closed_after_trigger_restoration() {
    for (column, value) in [
        ("project_id", ""),
        ("sha256", "bad"),
        ("storage_uri", ""),
        ("byte_length", "01"),
    ] {
        let tmp = TempDir::new("output-corrupt");
        let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        repo.append_output_ref(OutputRef::new("p", "r", &"a".repeat(64), "uri", "0").unwrap())
            .unwrap();
        let ddl: String = repo
            .connection()
            .query_row(
                "SELECT sql FROM sqlite_master WHERE name='output_refs_no_update'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        repo.connection()
            .execute_batch(
                "DROP TRIGGER output_refs_no_update; PRAGMA ignore_check_constraints=ON;",
            )
            .unwrap();
        repo.connection()
            .execute(&format!("UPDATE output_refs SET {column}=?1"), [value])
            .unwrap();
        repo.connection().execute_batch(&ddl).unwrap();
        repo.connection()
            .execute_batch("PRAGMA ignore_check_constraints=OFF")
            .unwrap();
        let p = if column == "project_id" { "" } else { "p" };
        if p.is_empty() {
            assert!(repo.find_output_ref(p, "r").is_err());
        } else {
            assert!(matches!(
                repo.find_output_ref(p, "r"),
                Err(StateError::GraphPersistence {
                    code: Code::CorruptStore,
                    ..
                })
            ));
        }
    }
}
#[test]
fn reference_injected_insert_failure_rolls_back_everything() {
    let tmp = TempDir::new("reference-rollback");
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    // Exercise the same append transaction with a failure after both row inserts.
    let error = crate::graph_repository::write_transaction(repo.connection(), |conn| {
        conn.execute(
            "INSERT INTO output_refs VALUES ('p','r',?1,'uri','0')",
            ["a".repeat(64)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workspace_checkpoints VALUES ('p','c','w','{}')",
            [],
        )
        .unwrap();
        Err::<(), _>(crate::graph_repository::failure(
            crate::GraphPhase::Write,
            Code::StorageFailure,
            None,
        ))
    })
    .unwrap_err();
    assert!(matches!(
        error,
        StateError::GraphPersistence {
            code: Code::StorageFailure,
            ..
        }
    ));
    assert_eq!(repo.count_table_rows("workspace_checkpoints").unwrap(), 0);
    assert_eq!(repo.count_table_rows("output_refs").unwrap(), 0);
    assert_eq!(repo.count_table_rows("state_schema_version").unwrap(), 16);
    assert_eq!(repo.count_table_rows("trusted_time_watermark").unwrap(), 0);
}
#[test]
fn reference_reads_and_writes_preserve_typed_store_busy() {
    let tmp = TempDir::new("reference-busy");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    repo.connection()
        .busy_timeout(std::time::Duration::ZERO)
        .unwrap();
    let lock = rusqlite::Connection::open(tmp.db_path()).unwrap();
    lock.execute_batch("BEGIN IMMEDIATE").unwrap();
    let e = repo
        .append_output_ref(OutputRef::new("p", "r", &"a".repeat(64), "uri", "0").unwrap())
        .unwrap_err();
    assert!(matches!(
        e,
        StateError::GraphPersistence {
            code: Code::StoreBusy,
            ..
        }
    ));
    lock.execute_batch("ROLLBACK").unwrap();
    drop(repo);
    drop(lock);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    // Both connections must use DELETE before acquiring an exclusive reader lock.
    repo.connection()
        .execute_batch("PRAGMA journal_mode=DELETE")
        .unwrap();
    repo.connection()
        .busy_timeout(std::time::Duration::ZERO)
        .unwrap();
    let lock = rusqlite::Connection::open(tmp.db_path()).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE").unwrap();
    for e in [
        repo.find_output_ref("p", "r").unwrap_err(),
        repo.find_checkpoint("p", "c").unwrap_err(),
        repo.list_checkpoints("p", "w").unwrap_err(),
        repo.latest_checkpoint("p", "w").unwrap_err(),
    ] {
        assert!(matches!(
            e,
            StateError::GraphPersistence {
                code: Code::StoreBusy,
                ..
            }
        ));
    }
    lock.execute_batch("ROLLBACK").unwrap();
    repo.append_output_ref(OutputRef::new("p", "r", &"a".repeat(64), "uri", "0").unwrap())
        .unwrap();
}
