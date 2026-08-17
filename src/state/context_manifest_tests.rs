//! Deterministic tests for durable ContextManifest create/read persistence
//! (migration 0006 slice).
//!
//! All tests use real temporary SQLite database files under the system
//! temporary directory (never inside the repository). Storage-backstop and
//! corruption checks that need SQL beyond the public repository API run
//! through crate-private test helpers only and are `#[cfg(test)]`-gated;
//! no production SQL surface exists for them.
//!
//! Compile-time/API-absence invariants (no runtime path exists to exercise
//! them, so they are established by the public surface of this crate and
//! recorded here for the task handoff, following the T35 convention of
//! `executor_binding_tests`):
//!
//! * no `update_context_manifest`, `replace_context_manifest`,
//!   `upsert_context_manifest`, `delete_context_manifest`, `add_source`,
//!   `remove_source`, `change_source`, `set_digest`, `set_last_read_at`,
//!   `set_last_rehydrated_at`, or `set_epoch` — create/read only;
//! * no `INSERT OR REPLACE` / `REPLACE` / `UPSERT` /
//!   `ON CONFLICT DO UPDATE` / `DELETE + INSERT` anywhere in this slice's
//!   SQL (duplicates are refused by pre-check + primary-key backstop);
//! * no digest comparison or computation API, and no hashing dependency
//!   (Cargo.toml/Cargo.lock unchanged);
//! * no source loading of any kind: the module imports no `std::fs`, no
//!   HTTP client, no process spawning, no artifact resolution, and never
//!   executes a `STATE_QUERY` target;
//! * no `rehydrate`, `compare_digests`, `load_required_sources`,
//!   `find_changed_sources`, `find_missing_sources`, `rebuild_context`,
//!   `resume_role`, or `record_rehydration`;
//! * no ContextEpoch entity/lifecycle, no compaction handling, no event
//!   emission, and no modification to `EventType` or the event schema;
//! * no `unsafe` Rust anywhere in this crate.

use rusqlite::ToSql;

use crate::context_manifest::{
    ContextManifest, ContextManifestSource, ContextSourceRef, ContextSourceRefType, RequiredFor,
    SourceClass,
};
use crate::error::StateError;
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

/// A minimal contract-valid LogicalRole for manifest ownership.
fn minimal_role(role_id: &str) -> LogicalRole {
    LogicalRole {
        role_id: role_id.to_string(),
        project_id: "project-1".to_string(),
        role_type: LogicalRoleType::RuntimeA1,
        status: LogicalRoleStatus::Active,
        current_context_epoch: 0,
        name: None,
        workstream_id: None,
        ownership_paths: Vec::new(),
        integration_branch: None,
        context_manifest_id: None,
        active_binding_id: None,
        created_at: None,
    }
}

/// A minimal contract-valid source: one REPO_PATH MANDATORY reference with
/// no optional fields and no required_for phases.
fn minimal_source() -> ContextManifestSource {
    ContextManifestSource {
        r#ref: ContextSourceRef {
            ref_type: ContextSourceRefType::RepoPath,
            target: "build-control/orchestrator-architecture/CONTEXT_MANIFEST_SPEC.md".to_string(),
        },
        source_class: SourceClass::Mandatory,
        digest: "digest-source-1".to_string(),
        last_read_at: None,
        required_for: Vec::new(),
    }
}

/// A minimal contract-valid manifest: only required fields set, one source,
/// every optional field absent.
fn minimal_manifest(manifest_id: &str, role_id: &str) -> ContextManifest {
    ContextManifest {
        manifest_id: manifest_id.to_string(),
        role_id: role_id.to_string(),
        project_id: "project-1".to_string(),
        epoch: 0,
        sources: vec![minimal_source()],
        created_at: "2026-08-17T10:00:00.000Z".to_string(),
        last_rehydrated_at: None,
    }
}

/// An opened repository with `role-1` already persisted.
fn seeded_repo(tag: &str) -> (TempDir, SqliteStateRepository) {
    let tmp = TempDir::new(tag);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-1"))
        .expect("role create");
    (tmp, repo)
}

/// Direct test-only SQL with bound parameters on the internal connection,
/// mirroring the single-active-binding test convention.
fn direct_exec(repo: &mut SqliteStateRepository, sql: &str, params: &[&dyn ToSql]) {
    repo.run_transaction(|uow| uow.execute(sql, params))
        .expect("test corruption statement");
}

// T01 — a fresh version-0 database bootstraps 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7,
// with exactly seven registered migrations ending at version 7 and exactly
// one metadata row per migration.
#[test]
fn t01_fresh_database_bootstraps_to_schema_version_7() {
    let registered = migrations::registered();
    assert_eq!(
        registered.len(),
        9,
        "exactly nine registered migrations (v0001–v0009) may exist"
    );
    assert_eq!(
        registered.last().expect("chain is non-empty").version,
        9,
        "the registered chain must end at version 9"
    );
    let tmp = TempDir::new("cm-t01");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("fresh database bootstraps");
    assert_eq!(repo.schema_version().expect("version read"), 9);
    assert_eq!(
        repo.count_table_rows("state_schema_version").expect("rows"),
        9,
        "one metadata row per applied migration"
    );
}

// T02 — a version-7 database reopens successfully and idempotently.
#[test]
fn t02_version_7_database_reopens_idempotently() {
    let tmp = TempDir::new("cm-t02");
    for _ in 0..3 {
        let repo = SqliteStateRepository::open(tmp.db_path()).expect("every reopen succeeds");
        assert_eq!(repo.schema_version().expect("version read"), 9);
        assert_eq!(
            repo.count_table_rows("state_schema_version").expect("rows"),
            9,
            "one metadata row per applied migration, never duplicated by reopen"
        );
    }
}

// T03 — ordinary open of an initialized schema-version-5 database fails
// closed with an explicit version mismatch and never silently migrates it.
#[test]
fn t03_ordinary_open_of_version_5_fails_closed() {
    let tmp = TempDir::new("cm-t03");
    let version_5_chain = &migrations::registered()[..5];
    drop(
        SqliteStateRepository::open_with_migrations(tmp.db_path(), version_5_chain)
            .expect("bootstrap at version 5"),
    );
    let error = SqliteStateRepository::open(tmp.db_path())
        .expect_err("ordinary open must not silently migrate a version-5 database");
    assert!(
        matches!(
            error,
            StateError::SchemaVersionMismatch {
                found: 5,
                supported: 9
            }
        ),
        "unexpected error: {error}"
    );
    // The refused open left the database untouched at version 5, with no
    // migration-6 tables materialized by the failed open.
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_5_chain)
        .expect("database still opens with its original chain");
    assert_eq!(repo.schema_version().expect("version read"), 5);
    assert!(
        !repo.table_exists("context_manifest").expect("table check"),
        "the failed ordinary open must not create migration-6 tables"
    );
}

// T04 — migration 6 creates exactly the three authorized tables with
// exactly their conceptual columns: no source-body column, no derived_state
// column, and no extra columns of any kind.
#[test]
fn t04_migration_v6_creates_exactly_the_authorized_schema() {
    let tmp = TempDir::new("cm-t04");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    for table in [
        "context_manifest",
        "context_manifest_source",
        "context_manifest_source_required_for",
    ] {
        assert!(
            repo.table_exists(table).expect("table check"),
            "{table} must exist after migration 6"
        );
    }
    assert_eq!(
        repo.table_columns("context_manifest")
            .expect("columns")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "manifest_id",
            "role_id",
            "project_id",
            "epoch",
            "created_at",
            "last_rehydrated_at",
        ],
        "context_manifest must have exactly its conceptual columns"
    );
    assert_eq!(
        repo.table_columns("context_manifest_source")
            .expect("columns")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "manifest_id",
            "source_ordinal",
            "ref_type",
            "ref_target",
            "source_class",
            "digest",
            "last_read_at",
        ],
        "context_manifest_source must have exactly its conceptual columns"
    );
    assert_eq!(
        repo.table_columns("context_manifest_source_required_for")
            .expect("columns")
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec![
            "manifest_id",
            "source_ordinal",
            "required_for_ordinal",
            "required_for",
        ],
        "context_manifest_source_required_for must have exactly its conceptual columns"
    );
    // No trigger or view was added on any table by migration 6.
    for table in [
        "context_manifest",
        "context_manifest_source",
        "context_manifest_source_required_for",
    ] {
        assert!(
            repo.sqlite_master_entries("trigger", table)
                .expect("triggers")
                .is_empty(),
            "migration 6 must add no trigger on {table}"
        );
        assert!(
            repo.sqlite_master_entries("view", table)
                .expect("views")
                .is_empty(),
            "migration 6 must add no view on {table}"
        );
    }
}

// T05 — migration 6 adds no derived-state, source-content, digest-cache,
// URL-fetch, event, or binding storage: none of the forbidden future
// schema exists in a fully bootstrapped database. (`context_epoch` belongs
// to the later migration 7 and is not forbidden by this test.)
#[test]
fn t05_no_forbidden_future_schema_exists() {
    let tmp = TempDir::new("cm-t05");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    for forbidden in [
        "context_epoch_changed_source",
        "context_epoch_history",
        "derived_state",
        "context_manifest_derived_state",
        "context_manifest_source_content",
        "context_source_content",
        "digest_cache",
        "url_fetch",
        "rehydration",
        "rehydration_log",
        "task",
        "finding",
        "dependency",
        "provider",
        "model",
        "host",
    ] {
        assert!(
            !repo.table_exists(forbidden).expect("table check"),
            "no {forbidden} storage may exist after migration 6"
        );
    }
    // The pre-existing baseline tables are untouched.
    for expected in [
        "state_schema_version",
        "logical_role",
        "logical_role_ownership_path",
        "executor_binding",
        "event",
    ] {
        assert!(
            repo.table_exists(expected).expect("table check"),
            "{expected} must still exist"
        );
    }
}

// T06 — the smallest valid manifest (one source, no optional fields)
// persists atomically: one parent row, one source row, and zero
// required_for rows commit together.
#[test]
fn t06_create_minimum_valid_manifest() {
    let (tmp, mut repo) = seeded_repo("cm-t06");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("minimum valid manifest persists");
    assert_eq!(
        repo.count_table_rows("context_manifest")
            .expect("parent rows"),
        1
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source")
            .expect("source rows"),
        1
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source_required_for")
            .expect("phase rows"),
        0,
        "a source with an empty required_for list persists zero phase rows"
    );
    drop(repo);
    assert!(tmp.db_path().is_file());
}

// T07 — a created manifest reads back structurally exact, including every
// field of every source.
#[test]
fn t07_read_created_manifest_exact_round_trip() {
    let (_tmp, mut repo) = seeded_repo("cm-t07");
    let manifest = minimal_manifest("manifest-001", "role-1");
    repo.create_context_manifest(manifest.clone())
        .expect("manifest persists");
    assert_eq!(
        repo.find_context_manifest("manifest-001")
            .expect("read created manifest"),
        Some(manifest),
        "readback must reconstruct the exact persisted structure"
    );
}

// T08 — a manifest with multiple ordered sources round-trips with the
// exact persisted order (explicit ordinals 0..N-1).
#[test]
fn t08_multiple_ordered_sources_round_trip() {
    let (_tmp, mut repo) = seeded_repo("cm-t08");
    let manifest = ContextManifest {
        manifest_id: "manifest-001".to_string(),
        role_id: "role-1".to_string(),
        project_id: "project-1".to_string(),
        epoch: 2,
        sources: vec![
            ContextManifestSource {
                r#ref: ContextSourceRef {
                    ref_type: ContextSourceRefType::RepoPath,
                    target: "docs/first.md".to_string(),
                },
                source_class: SourceClass::Mandatory,
                digest: "d-0".to_string(),
                last_read_at: Some("2026-08-17T09:00:00.000Z".to_string()),
                required_for: vec![RequiredFor::Decomposition, RequiredFor::Dispatch],
            },
            ContextManifestSource {
                r#ref: ContextSourceRef {
                    ref_type: ContextSourceRefType::StateQuery,
                    target: "query://open-tasks".to_string(),
                },
                source_class: SourceClass::Consumed,
                digest: "d-1".to_string(),
                last_read_at: None,
                required_for: Vec::new(),
            },
            ContextManifestSource {
                r#ref: ContextSourceRef {
                    ref_type: ContextSourceRefType::ArtifactId,
                    target: "artifact://a3-010-evidence".to_string(),
                },
                source_class: SourceClass::Reference,
                digest: "d-2".to_string(),
                last_read_at: None,
                required_for: vec![
                    RequiredFor::Acceptance,
                    RequiredFor::Integration,
                    RequiredFor::Evaluation,
                ],
            },
        ],
        created_at: "2026-08-17T10:00:00.000Z".to_string(),
        last_rehydrated_at: Some("2026-08-17T10:05:00.000Z".to_string()),
    };
    repo.create_context_manifest(manifest.clone())
        .expect("manifest persists");
    assert_eq!(
        repo.find_context_manifest("manifest-001")
            .expect("read back")
            .expect("manifest exists"),
        manifest
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source")
            .expect("source rows"),
        3
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source_required_for")
            .expect("phase rows"),
        5
    );
}

// T09 — a persisted manifest survives close/reopen unchanged.
#[test]
fn t09_close_reopen_durability() {
    let (tmp, mut repo) = seeded_repo("cm-t09");
    let manifest = minimal_manifest("manifest-001", "role-1");
    repo.create_context_manifest(manifest.clone())
        .expect("manifest persists");
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_context_manifest("manifest-001")
            .expect("read after reopen"),
        Some(manifest),
        "the manifest graph must be durable across close/reopen"
    );
}

// T10 — reading a manifest that does not exist is a deterministic absence.
#[test]
fn t10_missing_manifest_is_none() {
    let (_tmp, repo) = seeded_repo("cm-t10");
    assert_eq!(
        repo.find_context_manifest("no-such-manifest")
            .expect("missing manifest read"),
        None
    );
}

// T11 — creating a manifest whose manifest_id already exists fails
// explicitly with the duplicate-identity error, regardless of role.
#[test]
fn t11_duplicate_manifest_id_rejected() {
    let (_tmp, mut repo) = seeded_repo("cm-t11");
    repo.create_logical_role(minimal_role("role-2"))
        .expect("second role create");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("first manifest persists");
    let error = repo
        .create_context_manifest(minimal_manifest("manifest-001", "role-2"))
        .expect_err("duplicate manifest_id must fail");
    assert!(
        matches!(
            error,
            StateError::ContextManifestAlreadyExists {
                ref manifest_id
            } if manifest_id == "manifest-001"
        ),
        "unexpected error: {error}"
    );
}

// T12 — a refused duplicate create leaves the original manifest completely
// unchanged (never overwritten).
#[test]
fn t12_duplicate_leaves_original_unchanged() {
    let (_tmp, mut repo) = seeded_repo("cm-t12");
    let original = minimal_manifest("manifest-001", "role-1");
    repo.create_context_manifest(original.clone())
        .expect("original persists");
    let mut duplicate = minimal_manifest("manifest-001", "role-1");
    duplicate.project_id = "other-project".to_string();
    duplicate.epoch = 99;
    duplicate.sources[0].digest = "replaced-digest".to_string();
    let error = repo
        .create_context_manifest(duplicate)
        .expect_err("duplicate manifest_id must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestAlreadyExists { .. }
    ));
    assert_eq!(
        repo.find_context_manifest("manifest-001").expect("read"),
        Some(original),
        "the original manifest must remain byte-for-byte untouched"
    );
}

// T13 — creating a different manifest for a role that already owns one
// fails with the distinct role-manifest conflict, naming the existing
// manifest.
#[test]
fn t13_same_role_different_manifest_rejected() {
    let (_tmp, mut repo) = seeded_repo("cm-t13");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("first manifest persists");
    let error = repo
        .create_context_manifest(minimal_manifest("manifest-002", "role-1"))
        .expect_err("a role may own at most one manifest");
    assert!(
        matches!(
            error,
            StateError::ContextManifestRoleAlreadyHasManifest {
                ref role_id,
                ref existing_manifest_id,
            } if role_id == "role-1" && existing_manifest_id == "manifest-001"
        ),
        "unexpected error: {error}"
    );
}

// T14 — the role-manifest conflict leaves the existing manifest untouched:
// neither replaced, deleted, updated, nor silently chosen over.
#[test]
fn t14_role_conflict_leaves_original_unchanged() {
    let (_tmp, mut repo) = seeded_repo("cm-t14");
    let original = minimal_manifest("manifest-001", "role-1");
    repo.create_context_manifest(original.clone())
        .expect("original persists");
    let mut challenger = minimal_manifest("manifest-002", "role-1");
    challenger.epoch = 42;
    let error = repo
        .create_context_manifest(challenger)
        .expect_err("role conflict must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestRoleAlreadyHasManifest { .. }
    ));
    assert_eq!(
        repo.find_context_manifest("manifest-001").expect("read"),
        Some(original),
        "the existing manifest must remain untouched"
    );
    assert_eq!(
        repo.find_context_manifest("manifest-002")
            .expect("read challenger"),
        None,
        "the challenger manifest must not persist"
    );
}

// T15 — duplicate identity keeps precedence over the role-manifest
// conflict: a manifest_id that already exists reports the duplicate error
// even though the role also already owns a manifest.
#[test]
fn t15_duplicate_id_takes_precedence_over_role_conflict() {
    let (_tmp, mut repo) = seeded_repo("cm-t15");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("first manifest persists");
    let error = repo
        .create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect_err("duplicate must win over role conflict");
    assert!(
        matches!(error, StateError::ContextManifestAlreadyExists { .. }),
        "duplicate manifest_id must take error precedence, got: {error}"
    );
}

// T16 — different roles may each own their own manifest.
#[test]
fn t16_different_roles_may_own_manifests() {
    let (_tmp, mut repo) = seeded_repo("cm-t16");
    repo.create_logical_role(minimal_role("role-2"))
        .expect("second role create");
    repo.create_logical_role(minimal_role("role-3"))
        .expect("third role create");
    repo.create_context_manifest(minimal_manifest("manifest-a", "role-1"))
        .expect("role-1 manifest persists");
    repo.create_context_manifest(minimal_manifest("manifest-b", "role-2"))
        .expect("role-2 manifest persists");
    repo.create_context_manifest(minimal_manifest("manifest-c", "role-3"))
        .expect("role-3 manifest persists");
    assert_eq!(
        repo.count_table_rows("context_manifest")
            .expect("parent rows"),
        3
    );
}

// T17 — creating a manifest for a nonexistent role fails with the explicit
// role-not-found error and persists no parent or child rows.
#[test]
fn t17_nonexistent_role_rejected() {
    let (_tmp, mut repo) = seeded_repo("cm-t17");
    let error = repo
        .create_context_manifest(minimal_manifest("manifest-001", "no-such-role"))
        .expect_err("nonexistent role must fail");
    assert!(
        matches!(
            error,
            StateError::ContextManifestRoleNotFound {
                ref role_id
            } if role_id == "no-such-role"
        ),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.count_table_rows("context_manifest")
            .expect("parent rows"),
        0,
        "no parent rows may remain"
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source")
            .expect("source rows"),
        0,
        "no child rows may remain"
    );
}

// T18 — the schema's foreign key is the durable backstop: a direct SQL
// insert of a manifest row for a nonexistent role is refused by the
// database itself, and nothing persists.
#[test]
fn t18_foreign_key_backstop() {
    let (_tmp, mut repo) = seeded_repo("cm-t18");
    let result = repo.run_transaction(|uow| {
        uow.execute(
            "INSERT INTO context_manifest (manifest_id, role_id, project_id, epoch, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                &"manifest-fk-001",
                &"ghost-role",
                &"project-1",
                &0i64,
                &"2026-08-17T10:00:00.000Z",
            ],
        )
    });
    assert!(
        result.is_err(),
        "the database foreign key must refuse an orphan manifest"
    );
    assert_eq!(
        repo.count_table_rows("context_manifest")
            .expect("parent rows"),
        0
    );
}

// T19 — manifest_id validation: empty and overlong identifiers are
// rejected before any storage access.
#[test]
fn t19_manifest_id_validated() {
    let (_tmp, mut repo) = seeded_repo("cm-t19");
    let mut empty = minimal_manifest("manifest-001", "role-1");
    empty.manifest_id = String::new();
    let error = repo
        .create_context_manifest(empty)
        .expect_err("empty manifest_id must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("manifest_id")
    ));
    let mut overlong = minimal_manifest("manifest-001", "role-1");
    overlong.manifest_id = "x".repeat(201);
    let error = repo
        .create_context_manifest(overlong)
        .expect_err("overlong manifest_id must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("exceeds")
    ));
}

// T20 — role_id validation: empty and overlong identifiers are rejected.
#[test]
fn t20_role_id_validated() {
    let (_tmp, mut repo) = seeded_repo("cm-t20");
    let mut empty_role = minimal_manifest("manifest-001", "role-1");
    empty_role.role_id = String::new();
    let error = repo
        .create_context_manifest(empty_role)
        .expect_err("empty role_id must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("role_id")
    ));
    let mut overlong = minimal_manifest("manifest-001", "role-1");
    overlong.role_id = "x".repeat(201);
    let error = repo
        .create_context_manifest(overlong)
        .expect_err("overlong role_id must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("exceeds")
    ));
}

// T21 — project_id validation: empty and overlong identifiers are
// rejected.
#[test]
fn t21_project_id_validated() {
    let (_tmp, mut repo) = seeded_repo("cm-t21");
    let mut empty_project = minimal_manifest("manifest-001", "role-1");
    empty_project.project_id = String::new();
    let error = repo
        .create_context_manifest(empty_project)
        .expect_err("empty project_id must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("project_id")
    ));
    let mut overlong = minimal_manifest("manifest-001", "role-1");
    overlong.project_id = "x".repeat(201);
    let error = repo
        .create_context_manifest(overlong)
        .expect_err("overlong project_id must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("exceeds")
    ));
}

// T22 — the epoch snapshot: zero and positive epochs persist exactly; a
// negative epoch is rejected before storage (and by the CHECK backstop).
#[test]
fn t22_epoch_rules() {
    let (_tmp, mut repo) = seeded_repo("cm-t22");
    repo.create_logical_role(minimal_role("role-2"))
        .expect("second role create");

    let zero = minimal_manifest("manifest-zero", "role-1");
    repo.create_context_manifest(zero.clone()).expect("epoch 0");
    assert_eq!(
        repo.find_context_manifest("manifest-zero")
            .expect("read")
            .expect("exists")
            .epoch,
        0
    );

    let mut positive = minimal_manifest("manifest-pos", "role-2");
    positive.epoch = 4;
    repo.create_context_manifest(positive.clone())
        .expect("epoch 4");
    assert_eq!(
        repo.find_context_manifest("manifest-pos")
            .expect("read")
            .expect("exists")
            .epoch,
        4
    );

    let mut negative = minimal_manifest("manifest-neg", "role-1");
    negative.epoch = -1;
    let error = repo
        .create_context_manifest(negative)
        .expect_err("negative epoch must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("epoch")
    ));
}

// T23 — an empty source list is rejected; a supplied source list is never
// sorted: the persisted order is exactly the supplied order.
#[test]
fn t23_source_list_not_sorted_or_emptied() {
    let (_tmp, mut repo) = seeded_repo("cm-t23");
    let mut empty_sources = minimal_manifest("manifest-001", "role-1");
    empty_sources.sources = Vec::new();
    let error = repo
        .create_context_manifest(empty_sources)
        .expect_err("empty source list must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("at least one source")
    ));

    // Non-sorted multi-source list: "z-source", "a-source", "m-source" must
    // round-trip in exactly that (non-alphabetical) order.
    let manifest = ContextManifest {
        manifest_id: "manifest-002".to_string(),
        role_id: "role-1".to_string(),
        project_id: "project-1".to_string(),
        epoch: 0,
        sources: ["z-source", "a-source", "m-source"]
            .into_iter()
            .map(|target| ContextManifestSource {
                r#ref: ContextSourceRef {
                    ref_type: ContextSourceRefType::RepoPath,
                    target: target.to_string(),
                },
                source_class: SourceClass::Mandatory,
                digest: format!("d-{target}"),
                last_read_at: None,
                required_for: Vec::new(),
            })
            .collect(),
        created_at: "2026-08-17T10:00:00.000Z".to_string(),
        last_rehydrated_at: None,
    };
    repo.create_context_manifest(manifest.clone())
        .expect("manifest persists");
    let found = repo
        .find_context_manifest("manifest-002")
        .expect("read")
        .expect("exists");
    assert_eq!(found, manifest, "order must round-trip unsorted");
    let targets: Vec<&str> = found
        .sources
        .iter()
        .map(|source| source.r#ref.target.as_str())
        .collect();
    assert_eq!(targets, vec!["z-source", "a-source", "m-source"]);
}

// T24 — each of the three frozen reference kinds round-trips with its
// exact durable representation.
#[test]
fn t24_all_ref_kinds_round_trip() {
    let (_tmp, mut repo) = seeded_repo("cm-t24");
    let kinds = [
        (ContextSourceRefType::RepoPath, "REPO_PATH"),
        (ContextSourceRefType::StateQuery, "STATE_QUERY"),
        (ContextSourceRefType::ArtifactId, "ARTIFACT_ID"),
    ];
    for (index, (ref_type, stored)) in kinds.into_iter().enumerate() {
        assert_eq!(ref_type.as_str(), stored);
        let role_id = format!("role-kind-{index}");
        repo.create_logical_role(minimal_role(&role_id))
            .expect("role create");
        let manifest_id = format!("manifest-kind-{index}");
        let mut manifest = minimal_manifest(&manifest_id, &role_id);
        manifest.sources[0].r#ref.ref_type = ref_type;
        repo.create_context_manifest(manifest.clone())
            .expect("manifest persists");
        assert_eq!(
            repo.find_context_manifest(&manifest_id)
                .expect("read")
                .expect("exists")
                .sources[0]
                .r#ref
                .ref_type,
            ref_type
        );
    }
}

// T25 — unknown stored ref_type values never decode: the closed
// enumeration has no UNKNOWN/OTHER/CUSTOM/URL fallback, and the storage
// CHECK backstop refuses non-frozen values outright.
#[test]
fn t25_unknown_ref_type_fails_closed() {
    for value in [
        "URL",
        "UNKNOWN",
        "OTHER",
        "CUSTOM",
        "REPO_PATH ",
        "repo_path",
    ] {
        assert!(
            ContextSourceRefType::from_storage(value).is_err(),
            "ref_type {value:?} must fail closed, never map to a fallback variant"
        );
    }
    let (_tmp, mut repo) = seeded_repo("cm-t25");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("manifest persists");
    let result = repo.run_transaction(|uow| {
        uow.execute(
            "INSERT INTO context_manifest_source (
                manifest_id, source_ordinal, ref_type, ref_target, source_class, digest
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            &[
                &"manifest-001",
                &1i64,
                &"URL",
                &"https://example.invalid/source",
                &"REFERENCE",
                &"d-url",
            ],
        )
    });
    assert!(
        result.is_err(),
        "the storage CHECK must refuse the non-frozen URL ref kind"
    );
    let result = repo.run_transaction(|uow| {
        uow.execute(
            "UPDATE context_manifest_source SET ref_type = ?1 WHERE manifest_id = ?2",
            &[&"URL", &"manifest-001"],
        )
    });
    assert!(
        result.is_err(),
        "the CHECK backstop refuses corruption-style UPDATEs too"
    );
}

// T26 — URL is not an accepted typed reference kind in this slice: the
// typed enumeration produces only the three frozen kinds, each of which
// decodes back exactly, while URL is refused everywhere.
#[test]
fn t26_url_is_not_a_typed_ref_kind() {
    for ref_type in [
        ContextSourceRefType::RepoPath,
        ContextSourceRefType::StateQuery,
        ContextSourceRefType::ArtifactId,
    ] {
        let stored = ref_type.as_str();
        assert!(
            ["REPO_PATH", "STATE_QUERY", "ARTIFACT_ID"].contains(&stored),
            "the typed enumeration may only produce the three frozen kinds"
        );
        assert_eq!(
            ContextSourceRefType::from_storage(stored).expect("round-trip"),
            ref_type
        );
    }
    assert!(
        ContextSourceRefType::from_storage("URL").is_err(),
        "URL has no typed representation in this slice"
    );
}

// T27 — an empty ref_target is rejected at validation.
#[test]
fn t27_empty_target_rejected() {
    let (_tmp, mut repo) = seeded_repo("cm-t27");
    let mut manifest = minimal_manifest("manifest-001", "role-1");
    manifest.sources[0].r#ref.target = String::new();
    let error = repo
        .create_context_manifest(manifest)
        .expect_err("empty ref_target must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("target")
    ));
}

// T28 — ref_target is opaque metadata persisted byte-for-byte, however
// path-like, query-like, or syntactically odd it is.
#[test]
fn t28_target_persisted_unchanged() {
    let (_tmp, mut repo) = seeded_repo("cm-t28");
    let odd_target = "../../weird path/with spaces & symbols/#fragment;$(not-executed)";
    let mut manifest = minimal_manifest("manifest-001", "role-1");
    manifest.sources[0].r#ref.target = odd_target.to_string();
    repo.create_context_manifest(manifest.clone())
        .expect("manifest persists");
    assert_eq!(
        repo.find_context_manifest("manifest-001")
            .expect("read")
            .expect("exists")
            .sources[0]
            .r#ref
            .target,
        odd_target
    );
}

// T29 — each of the three frozen source classes round-trips exactly.
#[test]
fn t29_all_source_classes_round_trip() {
    let (_tmp, mut repo) = seeded_repo("cm-t29");
    for (index, class) in [
        SourceClass::Mandatory,
        SourceClass::Consumed,
        SourceClass::Reference,
    ]
    .into_iter()
    .enumerate()
    {
        let role_id = format!("role-class-{index}");
        repo.create_logical_role(minimal_role(&role_id))
            .expect("role create");
        let manifest_id = format!("manifest-class-{index}");
        let mut manifest = minimal_manifest(&manifest_id, &role_id);
        manifest.sources[0].source_class = class;
        repo.create_context_manifest(manifest.clone())
            .expect("manifest persists");
        assert_eq!(
            repo.find_context_manifest(&manifest_id)
                .expect("read")
                .expect("exists")
                .sources[0]
                .source_class,
            class
        );
    }
}

// T30 — unknown stored source_class values never decode, and the storage
// CHECK backstop refuses non-frozen values outright.
#[test]
fn t30_unknown_source_class_fails_closed() {
    for value in ["UNKNOWN", "OPTIONAL", "OTHER", "CUSTOM", "mandatory", ""] {
        assert!(
            SourceClass::from_storage(value).is_err(),
            "source_class {value:?} must fail closed, never map to a fallback variant"
        );
    }
    let (_tmp, mut repo) = seeded_repo("cm-t30");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("manifest persists");
    let result = repo.run_transaction(|uow| {
        uow.execute(
            "UPDATE context_manifest_source SET source_class = ?1 WHERE manifest_id = ?2",
            &[&"UNKNOWN", &"manifest-001"],
        )
    });
    assert!(
        result.is_err(),
        "the storage CHECK must refuse a non-frozen source_class"
    );
}

// T31 — an empty digest is rejected at validation.
#[test]
fn t31_empty_digest_rejected() {
    let (_tmp, mut repo) = seeded_repo("cm-t31");
    let mut manifest = minimal_manifest("manifest-001", "role-1");
    manifest.sources[0].digest = String::new();
    let error = repo
        .create_context_manifest(manifest)
        .expect_err("empty digest must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("digest")
    ));
}

// T32 — the digest is opaque integrity metadata: it round-trips
// byte-for-byte, is never recomputed, and is never normalized into a
// hash-shaped value.
#[test]
fn t32_digest_round_trips_unchanged() {
    let (_tmp, mut repo) = seeded_repo("cm-t32");
    let not_a_hash = "definitely not a sha256: ;; 100%";
    let mut manifest = minimal_manifest("manifest-001", "role-1");
    manifest.sources[0].digest = not_a_hash.to_string();
    repo.create_context_manifest(manifest.clone())
        .expect("manifest persists");
    assert_eq!(
        repo.find_context_manifest("manifest-001")
            .expect("read")
            .expect("exists")
            .sources[0]
            .digest,
        not_a_hash,
        "the digest must persist exactly as supplied"
    );
}

// T33 — last_read_at: absent stays absent, a present value round-trips
// exactly, and a present-but-empty value is rejected.
#[test]
fn t33_last_read_at_semantics() {
    let (_tmp, mut repo) = seeded_repo("cm-t33");
    let absent = minimal_manifest("manifest-none", "role-1");
    repo.create_context_manifest(absent.clone())
        .expect("create");
    assert_eq!(
        repo.find_context_manifest("manifest-none")
            .expect("read")
            .expect("exists")
            .sources[0]
            .last_read_at,
        None
    );

    repo.create_logical_role(minimal_role("role-2"))
        .expect("second role create");
    let mut present = minimal_manifest("manifest-some", "role-2");
    present.sources[0].last_read_at = Some("2026-08-17T08:30:00.000Z".to_string());
    repo.create_context_manifest(present.clone())
        .expect("create");
    assert_eq!(
        repo.find_context_manifest("manifest-some")
            .expect("read")
            .expect("exists")
            .sources[0]
            .last_read_at,
        Some("2026-08-17T08:30:00.000Z".to_string())
    );

    repo.create_logical_role(minimal_role("role-3"))
        .expect("third role create");
    let mut empty = minimal_manifest("manifest-empty", "role-3");
    empty.sources[0].last_read_at = Some(String::new());
    let error = repo
        .create_context_manifest(empty)
        .expect_err("empty Some(last_read_at) must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("last_read_at")
    ));
}

// T34 — required_for: an empty list persists as zero rows, and all five
// frozen phase values round-trip with their exact durable representations.
#[test]
fn t34_required_for_values() {
    let (_tmp, mut repo) = seeded_repo("cm-t34");
    let all_phases = vec![
        RequiredFor::Decomposition,
        RequiredFor::Dispatch,
        RequiredFor::Acceptance,
        RequiredFor::Integration,
        RequiredFor::Evaluation,
    ];
    let mut manifest = minimal_manifest("manifest-phases", "role-1");
    manifest.sources[0].required_for = all_phases.clone();
    repo.create_context_manifest(manifest.clone())
        .expect("manifest persists");
    assert_eq!(
        repo.find_context_manifest("manifest-phases")
            .expect("read")
            .expect("exists")
            .sources[0]
            .required_for,
        all_phases
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source_required_for")
            .expect("phase rows"),
        5
    );
    for phase in [
        RequiredFor::Decomposition,
        RequiredFor::Dispatch,
        RequiredFor::Acceptance,
        RequiredFor::Integration,
        RequiredFor::Evaluation,
    ] {
        assert_eq!(
            RequiredFor::from_storage(phase.as_str()).expect("decode"),
            phase
        );
    }
}

// T35 — required_for order round-trips exactly: never sorted, never
// deduplicated, no defaults inserted.
#[test]
fn t35_required_for_order_preserved() {
    let (_tmp, mut repo) = seeded_repo("cm-t35");
    let ordered_with_duplicates = vec![
        RequiredFor::Evaluation,
        RequiredFor::Decomposition,
        RequiredFor::Evaluation,
        RequiredFor::Dispatch,
        RequiredFor::Evaluation,
    ];
    let mut manifest = minimal_manifest("manifest-order", "role-1");
    manifest.sources[0].required_for = ordered_with_duplicates.clone();
    repo.create_context_manifest(manifest.clone())
        .expect("manifest persists");
    let found = repo
        .find_context_manifest("manifest-order")
        .expect("read")
        .expect("exists");
    assert_eq!(
        found.sources[0].required_for, ordered_with_duplicates,
        "order and duplicates must round-trip exactly"
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source_required_for")
            .expect("phase rows"),
        5,
        "duplicates must not be deduplicated"
    );
}

// T36 — unknown stored required_for values never decode, and the storage
// CHECK backstop refuses non-frozen values outright.
#[test]
fn t36_unknown_required_for_fails_closed() {
    for value in [
        "UNKNOWN",
        "OTHER",
        "CUSTOM",
        "PLANNING",
        "decomposition",
        "",
    ] {
        assert!(
            RequiredFor::from_storage(value).is_err(),
            "required_for {value:?} must fail closed, never map to a fallback variant"
        );
    }
    let (_tmp, mut repo) = seeded_repo("cm-t36");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("manifest persists");
    let result = repo.run_transaction(|uow| {
        uow.execute(
            "INSERT INTO context_manifest_source_required_for (
                manifest_id, source_ordinal, required_for_ordinal, required_for
             ) VALUES (?1, ?2, ?3, ?4)",
            &[&"manifest-001", &0i64, &0i64, &"PLANNING"],
        )
    });
    assert!(
        result.is_err(),
        "the storage CHECK must refuse a non-frozen required_for value"
    );
}

// T37 — created_at is opaque metadata: it round-trips exactly as supplied
// and an empty value is rejected.
#[test]
fn t37_created_at_semantics() {
    let (_tmp, mut repo) = seeded_repo("cm-t37");
    let stamp = "not-a-real-timestamp-but-opaque";
    let mut manifest = minimal_manifest("manifest-001", "role-1");
    manifest.created_at = stamp.to_string();
    repo.create_context_manifest(manifest.clone())
        .expect("manifest persists");
    assert_eq!(
        repo.find_context_manifest("manifest-001")
            .expect("read")
            .expect("exists")
            .created_at,
        stamp
    );
    let mut empty = minimal_manifest("manifest-002", "role-1");
    empty.created_at = String::new();
    let error = repo
        .create_context_manifest(empty)
        .expect_err("empty created_at must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("created_at")
    ));
}

// T38 — last_rehydrated_at: NULL round-trips as None, a present value
// round-trips as Some, and a present-but-empty value is rejected. No
// update method for this field exists in this slice.
#[test]
fn t38_last_rehydrated_at_semantics() {
    let (_tmp, mut repo) = seeded_repo("cm-t38");
    let absent = minimal_manifest("manifest-none", "role-1");
    repo.create_context_manifest(absent.clone())
        .expect("create");
    assert_eq!(
        repo.find_context_manifest("manifest-none")
            .expect("read")
            .expect("exists")
            .last_rehydrated_at,
        None
    );

    repo.create_logical_role(minimal_role("role-2"))
        .expect("second role create");
    let mut present = minimal_manifest("manifest-some", "role-2");
    present.last_rehydrated_at = Some("2026-08-17T11:00:00.000Z".to_string());
    repo.create_context_manifest(present.clone())
        .expect("create");
    assert_eq!(
        repo.find_context_manifest("manifest-some")
            .expect("read")
            .expect("exists")
            .last_rehydrated_at,
        Some("2026-08-17T11:00:00.000Z".to_string())
    );

    repo.create_logical_role(minimal_role("role-3"))
        .expect("third role create");
    let mut empty = minimal_manifest("manifest-empty", "role-3");
    empty.last_rehydrated_at = Some(String::new());
    let error = repo
        .create_context_manifest(empty)
        .expect_err("empty Some(last_rehydrated_at) must fail");
    assert!(matches!(
        error,
        StateError::ContextManifestValidation {
            ref detail
        } if detail.contains("last_rehydrated_at")
    ));
}

// T39 — the whole graph (parent + sources + required_for) is one atomic
// transaction: when the unit of work reports failure after the graph
// insert, nothing persists — there is no success before commit and no
// partial manifest after a failed create.
#[test]
fn t39_graph_create_is_atomic_and_rolls_back() {
    let (_tmp, mut repo) = seeded_repo("cm-t39");
    let second_source = {
        let mut source = minimal_source();
        source.r#ref.target = "docs/second.md".to_string();
        source.required_for = vec![
            RequiredFor::Decomposition,
            RequiredFor::Dispatch,
            RequiredFor::Acceptance,
        ];
        source
    };
    let mut manifest = minimal_manifest("manifest-001", "role-1");
    manifest.sources.push(second_source);
    let result: Result<(), StateError> = repo.run_transaction(|uow| {
        uow.insert_context_manifest(&manifest)?;
        // The graph insert succeeded inside the transaction; force a
        // later failure so rollback of the entire graph is observable.
        Err(StateError::UnitOfWorkFailed {
            detail: "forced test failure after graph insert".to_string(),
        })
    });
    assert!(result.is_err(), "the forced failure must surface");
    assert_eq!(
        repo.count_table_rows("context_manifest")
            .expect("parent rows"),
        0,
        "no parent row may survive the rollback"
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source")
            .expect("source rows"),
        0,
        "no source rows may survive the rollback"
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source_required_for")
            .expect("phase rows"),
        0,
        "no required_for rows may survive the rollback"
    );
    assert_eq!(
        repo.find_context_manifest("manifest-001").expect("read"),
        None,
        "a rolled-back create must not be readable"
    );
    // The store holds no residue of the rolled-back attempt: the same
    // manifest can still be created afterwards.
    repo.create_context_manifest(manifest)
        .expect("clean create");
}

// T40 — every failure class leaves zero partial rows: duplicate id, role
// conflict, and missing role never persist a partial graph.
#[test]
fn t40_no_partial_manifest_after_failed_creates() {
    let (_tmp, mut repo) = seeded_repo("cm-t40");
    repo.create_logical_role(minimal_role("role-2"))
        .expect("second role create");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("first manifest persists");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-2"))
        .expect_err("duplicate rejected");
    repo.create_context_manifest(minimal_manifest("manifest-002", "role-1"))
        .expect_err("role conflict rejected");
    repo.create_context_manifest(minimal_manifest("manifest-003", "ghost"))
        .expect_err("missing role rejected");
    assert_eq!(
        repo.count_table_rows("context_manifest").expect("rows"),
        1,
        "only the original manifest may exist"
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source")
            .expect("source rows"),
        1
    );
}

// T41 — a corrupt source ordinal gap fails closed at decode instead of
// being silently renumbered.
#[test]
fn t41_corrupt_source_ordinal_gap_fails_closed() {
    let (_tmp, mut repo) = seeded_repo("cm-t41");
    let manifest = ContextManifest {
        manifest_id: "manifest-001".to_string(),
        role_id: "role-1".to_string(),
        project_id: "project-1".to_string(),
        epoch: 0,
        sources: vec![
            minimal_source(),
            {
                let mut source = minimal_source();
                source.r#ref.target = "docs/second.md".to_string();
                source
            },
            {
                let mut source = minimal_source();
                source.r#ref.target = "docs/third.md".to_string();
                source
            },
        ],
        created_at: "2026-08-17T10:00:00.000Z".to_string(),
        last_rehydrated_at: None,
    };
    repo.create_context_manifest(manifest).expect("create");
    // Move the last source's ordinal from 2 to 5 (it has no required_for
    // children, so the update is allowed), leaving 0, 1, 5.
    direct_exec(
        &mut repo,
        "UPDATE context_manifest_source
         SET source_ordinal = ?1
         WHERE manifest_id = ?2 AND source_ordinal = 2",
        &[&5i64, &"manifest-001"],
    );
    let error = repo
        .find_context_manifest("manifest-001")
        .expect_err("a source ordinal gap must fail closed");
    assert!(
        matches!(
            error,
            StateError::ContextManifestDecodeFailed {
                ref detail
            } if detail.contains("contiguous")
        ),
        "unexpected error: {error}"
    );
}

// T42 — a corrupt required_for ordinal gap fails closed at decode.
#[test]
fn t42_corrupt_required_for_ordinal_gap_fails_closed() {
    let (_tmp, mut repo) = seeded_repo("cm-t42");
    let mut manifest = minimal_manifest("manifest-001", "role-1");
    manifest.sources[0].required_for = vec![
        RequiredFor::Decomposition,
        RequiredFor::Dispatch,
        RequiredFor::Acceptance,
    ];
    repo.create_context_manifest(manifest).expect("create");
    // Move the middle phase ordinal from 1 to 9, leaving 0, 2, 9.
    direct_exec(
        &mut repo,
        "UPDATE context_manifest_source_required_for
         SET required_for_ordinal = ?1
         WHERE manifest_id = ?2 AND source_ordinal = 0 AND required_for_ordinal = 1",
        &[&9i64, &"manifest-001"],
    );
    let error = repo
        .find_context_manifest("manifest-001")
        .expect_err("a required_for ordinal gap must fail closed");
    assert!(
        matches!(
            error,
            StateError::ContextManifestDecodeFailed {
                ref detail
            } if detail.contains("contiguous")
        ),
        "unexpected error: {error}"
    );
}

// T43 — a corrupt empty digest fails closed at decode; the malformed
// source is never silently dropped and never receives a default.
#[test]
fn t43_corrupt_empty_digest_fails_closed() {
    let (_tmp, mut repo) = seeded_repo("cm-t43");
    let mut manifest = minimal_manifest("manifest-001", "role-1");
    manifest.sources = vec![minimal_source(), {
        let mut source = minimal_source();
        source.r#ref.target = "docs/second.md".to_string();
        source
    }];
    repo.create_context_manifest(manifest).expect("create");
    direct_exec(
        &mut repo,
        "UPDATE context_manifest_source SET digest = ?1
         WHERE manifest_id = ?2 AND source_ordinal = 1",
        &[&"", &"manifest-001"],
    );
    let error = repo
        .find_context_manifest("manifest-001")
        .expect_err("an empty persisted digest must fail closed");
    assert!(
        matches!(
            error,
            StateError::ContextManifestDecodeFailed {
                ref detail
            } if detail.contains("digest")
        ),
        "unexpected error: {error}"
    );
}

// T44 — a corrupt empty ref_target fails closed at decode.
#[test]
fn t44_corrupt_empty_target_fails_closed() {
    let (_tmp, mut repo) = seeded_repo("cm-t44");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("create");
    direct_exec(
        &mut repo,
        "UPDATE context_manifest_source SET ref_target = ?1 WHERE manifest_id = ?2",
        &[&"", &"manifest-001"],
    );
    let error = repo
        .find_context_manifest("manifest-001")
        .expect_err("an empty persisted ref_target must fail closed");
    assert!(
        matches!(
            error,
            StateError::ContextManifestDecodeFailed {
                ref detail
            } if detail.contains("ref_target")
        ),
        "unexpected error: {error}"
    );
}

// T45 — a persisted parent with zero source rows fails closed: a corrupt
// empty manifest never decodes as a valid empty manifest.
#[test]
fn t45_zero_source_rows_fail_closed() {
    let (_tmp, mut repo) = seeded_repo("cm-t45");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("create");
    repo.run_transaction(|uow| {
        uow.execute(
            "DELETE FROM context_manifest_source WHERE manifest_id = ?1",
            &[&"manifest-001"],
        )
    })
    .expect("test corruption delete");
    let error = repo
        .find_context_manifest("manifest-001")
        .expect_err("a zero-source manifest must fail closed");
    assert!(
        matches!(
            error,
            StateError::ContextManifestDecodeFailed {
                ref detail
            } if detail.contains("zero persisted sources")
        ),
        "unexpected error: {error}"
    );
}

// T46 — corrupt parent fields fail closed: an empty created_at and an
// empty Some(last_rehydrated_at) never decode.
#[test]
fn t46_corrupt_parent_fields_fail_closed() {
    let (_tmp, mut repo) = seeded_repo("cm-t46");
    repo.create_logical_role(minimal_role("role-2"))
        .expect("second role create");
    repo.create_context_manifest(minimal_manifest("manifest-a", "role-1"))
        .expect("create");
    direct_exec(
        &mut repo,
        "UPDATE context_manifest SET created_at = ?1 WHERE manifest_id = ?2",
        &[&"", &"manifest-a"],
    );
    let error = repo
        .find_context_manifest("manifest-a")
        .expect_err("an empty persisted created_at must fail closed");
    assert!(matches!(
        error,
        StateError::ContextManifestDecodeFailed {
            ref detail
        } if detail.contains("created_at")
    ));

    let mut manifest = minimal_manifest("manifest-b", "role-2");
    manifest.last_rehydrated_at = Some("2026-08-17T11:00:00.000Z".to_string());
    repo.create_context_manifest(manifest).expect("create");
    direct_exec(
        &mut repo,
        "UPDATE context_manifest SET last_rehydrated_at = ?1 WHERE manifest_id = ?2",
        &[&"", &"manifest-b"],
    );
    let error = repo
        .find_context_manifest("manifest-b")
        .expect_err("an empty persisted last_rehydrated_at must fail closed");
    assert!(matches!(
        error,
        StateError::ContextManifestDecodeFailed {
            ref detail
        } if detail.contains("last_rehydrated_at")
    ));
}

// T47 — creating a manifest emits no events and mutates no other entity:
// the event log stays empty, and the owning LogicalRole and any
// ExecutorBinding rows are untouched.
#[test]
fn t47_no_event_emission_and_no_entity_mutation() {
    use crate::executor_binding::ExecutorBinding;

    let (_tmp, mut repo) = seeded_repo("cm-t47");
    let binding = ExecutorBinding {
        binding_id: "binding-001".to_string(),
        role_id: "role-1".to_string(),
        provider_id: "provider-alpha".to_string(),
        model_id: "model-one".to_string(),
        runtime_id: "runtime-a1-host".to_string(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: "2026-08-17T10:00:00.000Z".to_string(),
        lease_expires_at: "2026-08-17T11:00:00.000Z".to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    };
    repo.create_executor_binding(binding.clone())
        .expect("binding");
    let role_before = repo.find_logical_role("role-1").expect("role read");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("manifest create");
    assert_eq!(
        repo.count_table_rows("event").expect("event rows"),
        0,
        "manifest persistence must not emit any event"
    );
    assert_eq!(
        repo.find_logical_role("role-1").expect("role read after"),
        role_before,
        "creating a manifest must not mutate the owning LogicalRole"
    );
    assert_eq!(
        repo.find_executor_binding("binding-001")
            .expect("binding read"),
        Some(binding),
        "creating a manifest must not mutate ExecutorBinding rows"
    );
}

// T48 — duplicate semantic source references are never silently removed:
// two identical sources both persist and both round-trip.
#[test]
fn t48_duplicate_sources_not_removed() {
    let (_tmp, mut repo) = seeded_repo("cm-t48");
    let mut manifest = minimal_manifest("manifest-001", "role-1");
    manifest.sources = vec![minimal_source(), minimal_source()];
    repo.create_context_manifest(manifest.clone())
        .expect("manifest persists");
    assert_eq!(
        repo.find_context_manifest("manifest-001")
            .expect("read")
            .expect("exists"),
        manifest,
        "both semantically duplicate sources must round-trip"
    );
    assert_eq!(
        repo.count_table_rows("context_manifest_source")
            .expect("source rows"),
        2
    );
}

// T49 — a failed migration 6 leaves no partial v6 schema or version state:
// applying the migration SQL onto a database whose context_manifest table
// already exists fails atomically, and version 5 remains recorded.
#[test]
fn t49_failed_migration_leaves_no_partial_state() {
    let tmp = TempDir::new("cm-t49");
    let version_5_chain = &migrations::registered()[..5];
    {
        let mut repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_5_chain)
            .expect("bootstrap at version 5");
        repo.run_transaction(|uow| {
            uow.execute_batch("CREATE TABLE context_manifest (probe INTEGER);")
        })
        .expect("pre-create a conflicting table");
    }
    {
        let mut repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_5_chain)
            .expect("reopen at version 5");
        let migration = migrations::registered()[5];
        assert_eq!(migration.version, 6);
        let result = repo.run_transaction(|uow| uow.execute_batch(migration.sql));
        assert!(
            result.is_err(),
            "migration 6 must fail against the conflicting table"
        );
    }
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_5_chain)
        .expect("database still opens with its original chain");
    assert_eq!(
        repo.schema_version().expect("version read"),
        5,
        "the failed migration must not record version 6"
    );
    assert!(
        !repo
            .table_exists("context_manifest_source")
            .expect("tables"),
        "the failed migration must leave no partial v6 schema behind"
    );
}

// T50 — the create-time one-manifest-per-role pre-check is backed by the
// storage-level UNIQUE constraint on role_id: a direct SQL insert of a
// second manifest for a role that already owns one is refused by the
// database itself.
#[test]
fn t50_role_uniqueness_backstop() {
    let (_tmp, mut repo) = seeded_repo("cm-t50");
    repo.create_context_manifest(minimal_manifest("manifest-001", "role-1"))
        .expect("first manifest persists");
    let result = repo.run_transaction(|uow| {
        uow.execute(
            "INSERT INTO context_manifest (manifest_id, role_id, project_id, epoch, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            &[
                &"manifest-backstop",
                &"role-1",
                &"project-1",
                &0i64,
                &"2026-08-17T10:00:00.000Z",
            ],
        )
    });
    assert!(
        result.is_err(),
        "the UNIQUE backstop must refuse a second manifest for one role"
    );
    assert_eq!(
        repo.count_table_rows("context_manifest").expect("rows"),
        1,
        "only the original manifest may exist"
    );
}
