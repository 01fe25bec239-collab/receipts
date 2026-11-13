use crate::tests::TempDir;
use crate::{GraphPhase, SqliteStateRepository, migrations};
use migrations::v0019_row_identity::TABLES;

#[test]
fn every_protected_table_has_no_hidden_insert_or_update_identity() {
    let tmp = TempDir::new("rowid-matrix");
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let conn = repo.connection();
    for recursive in [0, 1] {
        conn.execute_batch(&format!("PRAGMA recursive_triggers={recursive}"))
            .unwrap();
        for table in TABLES {
            assert_eq!(
                conn.query_row(
                    "SELECT wr FROM pragma_table_list WHERE schema='main' AND name=?1",
                    [table],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                1,
                "{table}"
            );
            let before = conn.total_changes();
            for alias in ["rowid", "_rowid_", "oid"] {
                for value in ["0", "-1", "-7", "1", "NULL"] {
                    for op in ["INSERT", "INSERT OR REPLACE", "REPLACE", "INSERT OR IGNORE"] {
                        let sql = format!("{op} INTO {table}({alias}) VALUES({value})");
                        let error = conn.execute_batch(&sql).unwrap_err();
                        assert!(
                            error.to_string().contains("no column named"),
                            "{table}: {error}"
                        );
                    }
                    assert!(
                        conn.execute_batch(&format!("UPDATE {table} SET {alias}={value}"))
                            .is_err()
                    );
                }
            }
            assert_eq!(conn.total_changes(), before, "{table}");
        }
    }
    drop(repo);
    SqliteStateRepository::open(tmp.db_path()).unwrap();
}

#[test]
fn migration19_failure_rolls_back_exact18_and_restores_foreign_keys() {
    let tmp = TempDir::new("rowid-rollback");
    let repo =
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &migrations::registered()[..18])
            .unwrap();
    let conn = repo.connection();
    seed_storage(conn);
    let before = image(conn);
    conn.execute_batch("CREATE TRIGGER fail19 BEFORE INSERT ON state_schema_version WHEN NEW.version=19 BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(migrations::v0019_row_identity::apply(conn).is_err());
    assert_eq!(
        conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    crate::graph_repository::verify_v18(conn, GraphPhase::Migration).unwrap();
    assert_eq!(image(conn), before);
    for table in TABLES {
        assert_eq!(
            conn.query_row(
                "SELECT wr FROM pragma_table_list WHERE schema='main' AND name=?1",
                [table],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
    }
    conn.execute_batch("DROP TRIGGER fail19").unwrap();
    migrations::v0019_row_identity::apply(conn).unwrap();
    crate::graph_repository::verify_v19(conn, GraphPhase::Migration).unwrap();
    assert!(crate::graph_repository::verify_v18(conn, GraphPhase::Migration).is_err());
}

#[test]
fn every_supported_head_including18_preserves_values_through19() {
    for head in 11..=18 {
        let tmp = TempDir::new("rowid-upgrade");
        let mut repo = SqliteStateRepository::open_with_migrations(
            tmp.db_path(),
            &migrations::registered()[..head],
        )
        .unwrap();
        let role=crate::wire::LogicalRoleWireV1::parse_json(r#"{"role_id":"r","project_id":"p","role_type":"RUNTIME_A1","status":"ACTIVE","current_context_epoch":0,"ownership_paths":["b","a","b"]}"#).unwrap().to_logical_role().unwrap();
        repo.create_logical_role(role.clone()).unwrap();
        drop(repo);
        assert!(SqliteStateRepository::open(tmp.db_path()).is_err());
        assert_eq!(
            SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
            25
        );
        assert_eq!(
            SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).unwrap(),
            25
        );
        let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        assert_eq!(repo.find_logical_role("r").unwrap(), Some(role));
    }
}

fn seed_storage(conn: &rusqlite::Connection) {
    conn.execute_batch(r#"
BEGIN;
INSERT INTO logical_role(role_id,project_id,role_type,status,current_context_epoch) VALUES('r','p','RUNTIME_A1','ACTIVE','0');
INSERT INTO logical_role_ownership_path VALUES('r',0,'original');
INSERT INTO event VALUES('01ARZ3NDEKTSV4RRFFQ69G5FAV','p',NULL,'TASK_CREATED','SYSTEM',NULL,'TASK','t','opaque','ref','digest','c','0');
INSERT INTO context_manifest VALUES('m','r','p','0','t',NULL);
INSERT INTO context_manifest_source VALUES('m',0,'REPO_PATH','original','MANDATORY','d',NULL);
INSERT INTO context_manifest_source_required_for VALUES('m',0,0,'DISPATCH');
INSERT INTO context_manifest_seal VALUES('m');
INSERT INTO logical_role_history VALUES('r','p',1,'STATUS','ACTIVE','SUSPENDED',NULL,NULL,'0','0',NULL,NULL,'2026-11-05T00:00:00.000000000Z',NULL);
UPDATE logical_role SET status='SUSPENDED' WHERE role_id='r';
INSERT INTO graphs VALUES('p','g','1');
INSERT INTO graph_versions VALUES('p','g','1',0,NULL,'goal','policy','t',1,NULL,'digest','clock','1','compiler','ref','reason');
INSERT INTO graph_nodes(project_id,graph_id,graph_version,node_id,kind,state,required_capabilities_present,locked_reason_present) VALUES('p','g','1','n','TASK','PLANNED',1,0);
INSERT INTO graph_edges VALUES('p','g','1','e','n','n','CONTROL',NULL,'ON_PASS',NULL);
INSERT INTO graph_node_capabilities VALUES('p','g','1','n',0,'graph.core');
INSERT INTO graph_compiled_sources VALUES('p','g','1',0,'source');
INSERT INTO graph_node_results VALUES('p','g','result','1','n','{}');
INSERT INTO graph_mutations VALUES('p','g','mutation','1','1','{}');
INSERT INTO graph_snapshot_seals VALUES(0,'p','g','1');
INSERT INTO attempt_started VALUES('01ARZ3NDEKTSV4RRFFQ69G5FAV','p','g','n','1','RUNTIME_A3','runtime','workspace','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','2026-11-05T00:00:00.000000000Z');
INSERT INTO attempt_finished VALUES('01ARZ3NDEKTSV4RRFFQ69G5FAV','2026-11-05T00:00:00.000000000Z',NULL,NULL,NULL);
INSERT INTO workspace_checkpoints VALUES('p','checkpoint','workspace','{}');
INSERT INTO output_refs VALUES('p','output','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','uri','0');
INSERT INTO decision_records VALUES('p','REGISTRY_CAPABILITY','record',NULL,NULL,NULL,'{}','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');
INSERT INTO provenance_links VALUES('p','t','START_SHA','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');
COMMIT;
"#).unwrap();
}
fn image(conn: &rusqlite::Connection) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
    TABLES
        .iter()
        .map(|table| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT * FROM {table} ORDER BY {}",
                    crate::tests::logical_column_order(conn, table)
                ))
                .unwrap();
            let n = stmt.column_count();
            stmt.query_map([], |r| {
                (0..n).map(|i| r.get(i)).collect::<Result<Vec<_>, _>>()
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
        })
        .collect()
}
#[test]
fn populated_tables_refuse_all_replacement_identities_without_losing_any_row() {
    let tmp = TempDir::new("rowid-populated");
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    let conn = repo.connection();
    seed_storage(conn);
    let before = image(conn);
    for recursive in [0, 1] {
        conn.execute_batch(&format!("PRAGMA recursive_triggers={recursive}"))
            .unwrap();
        assert_eq!(
            conn.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        for table in TABLES {
            for verb in ["REPLACE", "INSERT OR REPLACE", "INSERT OR IGNORE", "INSERT"] {
                let sql = format!("{verb} INTO {table} SELECT * FROM {table}");
                assert!(conn.execute_batch(&sql).is_err(), "{table}: {sql}");
                assert_eq!(image(conn), before, "{table}");
            }
            assert!(
                conn.execute_batch(&format!(
                    "INSERT INTO {table} SELECT * FROM {table} WHERE 1 ON CONFLICT DO NOTHING"
                ))
                .is_err(),
                "{table}"
            );
            for alias in ["rowid", "_rowid_", "oid"] {
                for value in ["0", "-1", "NULL"] {
                    assert!(
                        conn.execute_batch(&format!(
                            "INSERT OR REPLACE INTO {table}({alias}) SELECT {value} FROM {table}"
                        ))
                        .is_err()
                    );
                }
            }
            assert_eq!(image(conn), before, "{table}");
        }
        // The only additional unique key has a distinct application primary key.
        assert!(conn.execute_batch("INSERT OR REPLACE INTO graph_mutations SELECT project_id,graph_id,'different',parent_version,resulting_version,mutation_json FROM graph_mutations").is_err());
        assert_eq!(image(conn), before);
    }
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    assert_eq!(image(repo.connection()), before);
}
#[test]
fn migration19_preserves_every_populated_table_and_all_definitions() {
    let tmp = TempDir::new("rowid-all-upgrade");
    let repo =
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &migrations::registered()[..18])
            .unwrap();
    seed_storage(repo.connection());
    let before = image(repo.connection());
    migrations::v0019_row_identity::apply(repo.connection()).unwrap();
    assert_eq!(image(repo.connection()), before);
    assert!(migrations::v0019_row_identity::tables_match(repo.connection()).unwrap());
}

#[test]
fn every_table_refuses_missing_altered_and_unexpected_definitions() {
    for table in TABLES {
        for damage in ["table", "trigger", "extra"] {
            let tmp = TempDir::new("rowid-tamper");
            let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
            let conn = repo.connection();
            match damage {
                "table" => {
                    conn.execute_batch("PRAGMA writable_schema=ON").unwrap();
                    conn.execute("UPDATE sqlite_master SET sql=replace(sql,'WITHOUT ROWID','') WHERE type='table' AND name=?1",[table]).unwrap();
                    conn.execute_batch("PRAGMA writable_schema=RESET").unwrap();
                }
                "trigger" => {
                    let name:String=conn.query_row("SELECT name FROM sqlite_master WHERE type='trigger' AND tbl_name=?1 LIMIT 1",[table],|r|r.get(0)).unwrap();
                    conn.execute_batch(&format!("DROP TRIGGER {name}")).unwrap();
                }
                _ => conn
                    .execute_batch(&format!(
                        "CREATE TRIGGER unexpected BEFORE INSERT ON {table} BEGIN SELECT 1; END"
                    ))
                    .unwrap(),
            }
            drop(repo);
            assert!(
                SqliteStateRepository::open(tmp.db_path()).is_err(),
                "{table}: {damage}"
            );
            assert!(
                SqliteStateRepository::migrate_existing_to_current(tmp.db_path()).is_err(),
                "{table}: {damage}"
            );
        }
    }
}
