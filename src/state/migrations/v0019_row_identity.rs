//! Remove hidden replacement identities from protected State storage.
use super::Migration;
use crate::{GraphPhase, StateError, graph_repository as graph};
use rusqlite::{Connection, OptionalExtension};
pub(crate) const MIGRATION: Migration = Migration {
    version: 19,
    name: "row_identity",
    sql: "INSERT INTO state_schema_version(version,migration_name) VALUES (19,'row_identity');",
};
pub(crate) const TABLES: &[&str] = &[
    "attempt_started",
    "attempt_finished",
    "graphs",
    "graph_versions",
    "graph_nodes",
    "graph_edges",
    "graph_node_capabilities",
    "graph_compiled_sources",
    "graph_node_results",
    "graph_mutations",
    "graph_snapshot_seals",
    "workspace_checkpoints",
    "output_refs",
    "decision_records",
    "provenance_links",
    "logical_role_history",
    "logical_role",
    "logical_role_ownership_path",
    "event",
    "context_manifest",
    "context_manifest_source",
    "context_manifest_source_required_for",
    "context_manifest_seal",
];

// Exact legacy definitions remain the source of truth; only the storage suffix changes.
pub(crate) fn definition_matches(
    conn: &Connection,
    name: &str,
    stored: Option<&str>,
    expected: &str,
) -> Result<bool, rusqlite::Error> {
    let head: i64 = conn.query_row("SELECT MAX(version) FROM state_schema_version", [], |r| {
        r.get(0)
    })?;
    let expected = if head >= 19 && TABLES.contains(&name) {
        format!(
            "CREATE TABLE \"{name}\" {}{} WITHOUT ROWID",
            &expected[expected.find('(').unwrap()..],
            if expected.ends_with("STRICT") {
                ","
            } else {
                ""
            }
        )
    } else {
        expected.to_owned()
    };
    Ok(stored == Some(expected.as_str()))
}
pub(crate) fn apply(conn: &Connection) -> Result<(), StateError> {
    let phase = GraphPhase::Migration;
    graph::verify_v18(conn, phase)?;
    let enabled: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .map_err(|e| graph::sqlite_error(phase, e))?;
    if enabled != 1 {
        return Err(graph::corrupt());
    }
    conn.execute_batch("PRAGMA foreign_keys=OFF")
        .map_err(|e| graph::sqlite_error(phase, e))?;
    let result = graph::migrate_step(conn, |conn| {
        graph::verify_v18(conn, phase)?;
        let mut definitions = Vec::new();
        let mut dependents = Vec::new();
        for table in TABLES {
            let sql: String = conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                    [table],
                    |r| r.get(0),
                )
                .map_err(|e| graph::sqlite_error(phase, e))?;
            definitions.push((*table, sql));
            let mut stmt=conn.prepare("SELECT type,name,sql FROM sqlite_master WHERE tbl_name=?1 AND type IN ('trigger','index') AND sql IS NOT NULL ORDER BY type,name").map_err(|e|graph::sqlite_error(phase,e))?;
            let rows = stmt
                .query_map([table], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| graph::sqlite_error(phase, e))?;
            for row in rows {
                dependents.push(row.map_err(|e| graph::sqlite_error(phase, e))?);
            }
        }
        // Drop triggers before any parent disappears, including cross-table trigger references.
        for (kind, name, _) in &dependents {
            conn.execute_batch(&format!("DROP {kind} \"{}\";", name.replace('"', "\"\"")))
                .map_err(|e| graph::sqlite_error(phase, e))?;
        }
        for (table, sql) in definitions {
            let tail = &sql[sql.find('(').ok_or_else(graph::corrupt)?..];
            let separator = if tail.ends_with("STRICT") { "," } else { "" };
            conn.execute_batch(&format!("CREATE TABLE row_identity_copy {tail}{separator} WITHOUT ROWID; INSERT INTO row_identity_copy SELECT * FROM \"{table}\"; DROP TABLE \"{table}\"; ALTER TABLE row_identity_copy RENAME TO \"{table}\";")).map_err(|e|graph::sqlite_error(phase,e))?;
        }
        for (_, _, sql) in dependents {
            conn.execute_batch(&sql)
                .map_err(|e| graph::sqlite_error(phase, e))?;
        }
        conn.execute_batch(MIGRATION.sql)
            .map_err(|e| graph::sqlite_error(phase, e))?;
        graph::verify_v19(conn, phase)
    });
    let restored = conn
        .execute_batch("PRAGMA foreign_keys=ON")
        .map_err(|e| graph::sqlite_error(phase, e))
        .and_then(|()| {
            let enabled: i64 = conn
                .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
                .map_err(|e| graph::sqlite_error(phase, e))?;
            if enabled != 1 {
                return Err(graph::corrupt());
            }
            let violation: Option<i64> = conn
                .query_row("SELECT 1 FROM pragma_foreign_key_check LIMIT 1", [], |r| {
                    r.get(0)
                })
                .optional()
                .map_err(|e| graph::sqlite_error(phase, e))?;
            if violation.is_some() {
                return Err(graph::corrupt());
            }
            Ok(())
        });
    result.and(restored)
}

pub(crate) fn tables_match(conn: &Connection) -> Result<bool, rusqlite::Error> {
    for table in TABLES {
        let mut expected = None;
        for migration in super::registered().iter().take(18) {
            for statement in migration.sql.split(";\n") {
                let statement = statement.trim();
                let mut words = statement.split_whitespace();
                if let (Some("CREATE"), Some("TABLE"), Some(name)) =
                    (words.next(), words.next(), words.next())
                    && (name == *table
                        || name == format!("{table}_v12")
                        || name == format!("{table}_v18"))
                {
                    expected = Some(statement.to_owned());
                }
            }
        }
        let Some(expected) = expected else {
            return Ok(false);
        };
        let stored: Option<String> = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |r| r.get(0),
            )
            .optional()?;
        if !definition_matches(conn, table, stored.as_deref(), &expected)? {
            return Ok(false);
        }
    }
    // No unrecognized uniqueness or guard may add an unchecked collision identity.
    let mut stmt=conn.prepare("SELECT type,tbl_name,sql FROM sqlite_master WHERE type IN ('index','trigger') AND sql IS NOT NULL")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (_, table, sql) = row?;
        if TABLES.contains(&table.as_str())
            && !super::registered()
                .iter()
                .take(18)
                .any(|m| m.sql.split(";\n").any(|s| s.trim() == sql))
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(crate) fn objects_match(conn: &Connection) -> Result<bool, rusqlite::Error> {
    use rusqlite::params;
    let mut triggers = 0;
    for sql in [
        super::v0017_decision_records::SQL,
        super::v0018_manifest_successors::SQL,
    ] {
        for statement in sql.split(";\n") {
            let statement = statement.trim();
            let mut words = statement.split_whitespace();
            let (Some("CREATE"), Some(kind), Some(name)) =
                (words.next(), words.next(), words.next())
            else {
                continue;
            };
            let (name, expected) = if name == "context_manifest_v18" {
                (
                    "context_manifest",
                    statement.replace("context_manifest_v18", "\"context_manifest\""),
                )
            } else {
                (name, statement.to_string())
            };
            if kind == "TRIGGER" {
                triggers += 1;
            }
            let stored: Option<String> = conn
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE type=?1 AND name=?2",
                    params![kind.to_ascii_lowercase(), name],
                    |r| r.get(0),
                )
                .optional()?;
            if !definition_matches(conn, name, stored.as_deref(), expected.as_str())? {
                return Ok(false);
            }
        }
    }
    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND tbl_name IN ('decision_records','provenance_links','logical_role_history','logical_role','logical_role_ownership_path','event','context_manifest','context_manifest_source','context_manifest_source_required_for','context_manifest_seal')", [], |r| r.get(0))?;
    Ok(count == triggers)
}
