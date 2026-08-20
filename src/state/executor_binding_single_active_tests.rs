//! Deterministic tests for the single-active-binding persistence guard
//! (migration 0005 partial unique index plus the transactional create-time
//! pre-check).
//!
//! All tests use real temporary SQLite database files under the system
//! temporary directory (never inside the repository). Storage-backstop
//! probes that must bypass the typed create path run through crate-private
//! test helpers (`#[cfg(test)]` `UnitOfWork::execute` /
//! `execute_batch` and the `sqlite_master` inspection helper) and are never
//! exposed through the public repository API; no production raw-SQL surface
//! exists for them.
//!
//! The guard reasons only from the durable terminal pair
//! `released_at`/`release_reason`: a binding blocks rebind of its role until
//! an authorized explicit release records both fields. Nothing here parses,
//! compares, or evaluates `lease_expires_at` against any clock, and no test
//! requires a wall-clock verdict.
//!
//! T17 and T36–T49, T55–T56 (scope/absence invariants) are compile-time /
//! source-inspection invariants documented at the bottom of this module,
//! not fabricated runtime tests.

use crate::error::StateError;
use crate::executor_binding::{ExecutorBinding, ReleaseReason};
use crate::executor_binding_lease_expiry::ExecutorLeaseExpiryOutcomeV1;
use crate::executor_binding_lease_expiry_tests::{BINDING, DEADLINE, clock, request, seeded};
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::tests::{TempDir, trusted_clock};

/// The stable identifier of the migration-0005 partial unique index.
const GUARD_INDEX_NAME: &str = "idx_executor_binding_role_unreleased";

/// The LogicalRole columns frozen by migration 0002: any new role column —
/// in particular an active-binding pointer — would break this exact list.
const LOGICAL_ROLE_COLUMNS: [&str; 11] = [
    "role_id",
    "project_id",
    "role_type",
    "status",
    "current_context_epoch",
    "name",
    "workstream_id",
    "integration_branch",
    "context_manifest_id",
    "active_binding_id",
    "created_at",
];

/// A minimal contract-valid LogicalRole for binding targets.
fn minimal_role(role_id: &str, role_type: LogicalRoleType) -> LogicalRole {
    LogicalRole {
        role_id: role_id.to_string(),
        project_id: "project-1".to_string(),
        role_type,
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

/// A minimal contract-valid unreleased binding with a distinct binding_id.
fn minimal_binding(binding_id: &str, role_id: &str) -> ExecutorBinding {
    ExecutorBinding {
        binding_id: binding_id.to_string(),
        role_id: role_id.to_string(),
        provider_id: "provider-alpha".to_string(),
        model_id: "model-one".to_string(),
        runtime_id: "runtime-a1-host".to_string(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: "2026-08-16T10:00:00.000Z".to_string(),
        lease_expires_at: "2026-08-16T11:00:00.000000000Z".to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    }
}

/// Bootstraps a database at schema version 4 (the chain prefix before the
/// guard migration), so migration 5 behavior can be exercised directly.
fn open_version_4(tmp: &TempDir) -> SqliteStateRepository {
    let version_4_chain = &migrations::registered()[..4];
    SqliteStateRepository::open_with_migrations(tmp.db_path(), version_4_chain)
        .expect("bootstrap at version 4")
}

/// Test-only storage probe: inserts one binding row through bound-parameter
/// SQL inside a State transaction, deliberately bypassing the typed create
/// path so the database-level backstop can be observed on its own.
pub(crate) fn direct_insert_binding(
    repo: &mut SqliteStateRepository,
    binding: &ExecutorBinding,
) -> Result<(), StateError> {
    let values: &[&dyn rusqlite::ToSql] = &[
        &binding.binding_id,
        &binding.role_id,
        &binding.provider_id,
        &binding.model_id,
        &binding.runtime_id,
        &binding.session_ref,
        &binding.routing_decision_id,
        &binding.bound_at,
        &binding.lease_expires_at,
        &binding.released_at,
        &binding.release_reason.map(ReleaseReason::as_str),
        &binding.rehydration_completed.map(i64::from),
    ];
    repo.run_transaction(|uow| {
        uow.execute(
            "INSERT INTO executor_binding (
                binding_id, role_id, provider_id, model_id, runtime_id,
                session_ref, routing_decision_id, bound_at, lease_expires_at,
                released_at, release_reason, rehydration_completed
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            values,
        )
    })
    .map(|_| ())
}

/// Named (non-`sqlite_*`) index entries of the `executor_binding` table.
fn named_binding_indexes(repo: &SqliteStateRepository) -> Vec<(String, String)> {
    repo.sqlite_master_entries("index", "executor_binding")
        .expect("index listing")
        .into_iter()
        .filter(|(name, _)| !name.starts_with("sqlite_"))
        .collect()
}

// T01 — a fresh database bootstraps 0 → 1 → 2 → 3 → 4 → 5 → 6 → 7, and schema
// version 7 is durably recorded with exactly one metadata row per
// migration.
#[test]
fn t01_fresh_database_bootstraps_through_schema_7() {
    let tmp = TempDir::new("sab-t01");
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("fresh database bootstraps");
    assert_eq!(repo.schema_version().expect("version read"), 10);
    assert_eq!(
        repo.count_table_rows("state_schema_version").expect("rows"),
        10,
        "one metadata row per applied migration"
    );
    // The guard index is part of the fresh bootstrap.
    assert_eq!(
        named_binding_indexes(&repo)
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        vec![GUARD_INDEX_NAME],
        "fresh bootstrap must create exactly the guard index"
    );
}

// T02 — a schema-version-7 database reopens successfully and idempotently.
#[test]
fn t02_schema_version_7_reopens() {
    let tmp = TempDir::new("sab-t02");
    for _ in 0..3 {
        let repo = SqliteStateRepository::open(tmp.db_path()).expect("every reopen succeeds");
        assert_eq!(repo.schema_version().expect("version read"), 10);
        assert_eq!(
            repo.count_table_rows("state_schema_version").expect("rows"),
            10,
            "one metadata row per applied migration, never duplicated by reopen"
        );
    }
}

// T03 — ordinary open of an initialized schema-version-4 database fails
// closed with an explicit version mismatch and never auto-runs migration 5.
#[test]
fn t03_ordinary_open_of_version_4_fails_closed() {
    let tmp = TempDir::new("sab-t03");
    drop(open_version_4(&tmp));
    let error = SqliteStateRepository::open(tmp.db_path())
        .expect_err("ordinary open must not silently migrate a version-4 database");
    assert!(
        matches!(
            error,
            StateError::SchemaVersionMismatch {
                found: 4,
                supported: 10
            }
        ),
        "unexpected error: {error}"
    );
    // The refused open left the database untouched at version 4, with no
    // guard index materialized by the failed open.
    let repo = open_version_4(&tmp);
    assert_eq!(repo.schema_version().expect("version read"), 4);
    assert!(
        named_binding_indexes(&repo).is_empty(),
        "the failed ordinary open must not create the migration-5 index"
    );
}

// T04 — migration 5 adds exactly one partial unique index and nothing else:
// no table, no column, no trigger, on either the manual v4→v5 path or the
// fresh bootstrap path.
#[test]
fn t04_migration_v5_adds_exactly_one_partial_unique_index() {
    let tmp = TempDir::new("sab-t04");
    let (tables_before, columns_before, triggers_before) = {
        let repo = open_version_4(&tmp);
        assert!(
            named_binding_indexes(&repo).is_empty(),
            "version 4 has no named executor_binding index"
        );
        (
            repo.list_tables().expect("table listing"),
            repo.table_columns("executor_binding").expect("columns"),
            repo.sqlite_master_entries("trigger", "executor_binding")
                .expect("trigger listing"),
        )
    };

    // Apply migration 5 exactly as bootstrap would: atomically, in one
    // transaction, on the version-4 database.
    {
        let mut repo = open_version_4(&tmp);
        let migration = migrations::registered()[4];
        assert_eq!(migration.version, 5);
        repo.run_transaction(|uow| uow.execute_batch(migration.sql))
            .expect("apply migration 5");
    }

    // The database now records version 5, so it opens with the version-5
    // prefix of the registered chain (the ordinary chain ends at version 7
    // and refuses a version-5 database).
    let version_5_chain = &migrations::registered()[..5];
    let repo = SqliteStateRepository::open_with_migrations(tmp.db_path(), version_5_chain)
        .expect("open at version 5");
    assert_eq!(repo.schema_version().expect("version read"), 5);

    // Exactly one named index exists, it is the guard index, and its stored
    // SQL is exactly the partial unique index over role_id with the
    // fail-closed terminal-pair predicate.
    let indexes = named_binding_indexes(&repo);
    assert_eq!(
        indexes.len(),
        1,
        "migration 5 must add exactly one named index, found {indexes:?}"
    );
    assert_eq!(indexes[0].0, GUARD_INDEX_NAME);
    let normalized = indexes[0]
        .1
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    assert_eq!(
        normalized,
        "create unique index idx_executor_binding_role_unreleased \
         on executor_binding (role_id) \
         where released_at is null or release_reason is null",
        "the stored index SQL must be exactly the partial unique index"
    );

    // No table, column, or trigger beyond the version-4 shape.
    assert_eq!(
        repo.list_tables().expect("table listing"),
        tables_before,
        "migration 5 must not add or remove any table"
    );
    assert_eq!(
        repo.table_columns("executor_binding").expect("columns"),
        columns_before,
        "migration 5 must not add any executor_binding column"
    );
    assert_eq!(
        repo.sqlite_master_entries("trigger", "executor_binding")
            .expect("trigger listing"),
        triggers_before,
        "migration 5 must not add any trigger"
    );
}

// T05 — the first binding for a role with no bindings succeeds (CASE A).
#[test]
fn t05_first_binding_for_role_succeeds() {
    let tmp = TempDir::new("sab-t05");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-a-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let binding = minimal_binding("binding-a-001", "role-a-001");
    repo.create_executor_binding(binding.clone())
        .expect("the first binding for an unbound role must succeed");
    assert_eq!(
        repo.find_executor_binding("binding-a-001").expect("find"),
        Some(binding)
    );
}

// T06 — a second binding with a different binding_id for the same role
// while B1 is unreleased fails explicitly with the domain conflict
// (CASE D).
#[test]
fn t06_second_binding_same_role_unreleased_fails() {
    let tmp = TempDir::new("sab-t06");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-b-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-b1-001", "role-b-001"))
        .expect("binding create");
    let error = repo
        .create_executor_binding(minimal_binding("binding-b2-001", "role-b-001"))
        .expect_err("a second unreleased binding for one role must be refused");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingUnreleasedConflict {
                role_id,
                blocking_binding_id
            } if role_id == "role-b-001" && blocking_binding_id == "binding-b1-001"
        ),
        "unexpected error: {error}"
    );
}

// T07 — the refused same-role creation persists no B2 row.
#[test]
fn t07_conflict_creates_no_second_row() {
    let tmp = TempDir::new("sab-t07");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-c-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-c1-001", "role-c-001"))
        .expect("binding create");
    repo.create_executor_binding(minimal_binding("binding-c2-001", "role-c-001"))
        .expect_err("same-role conflict");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "a refused create must persist no row"
    );
    assert_eq!(
        repo.find_executor_binding("binding-c2-001").expect("find"),
        None
    );
    // Durable across close/reopen.
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "no refused-create row may appear after reopen"
    );
}

// T08 — the refused creation leaves B1 byte-for-byte unchanged.
#[test]
fn t08_conflict_leaves_b1_unchanged() {
    let tmp = TempDir::new("sab-t08");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-d-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let original = minimal_binding("binding-d1-001", "role-d-001");
    repo.create_executor_binding(original.clone())
        .expect("binding create");
    repo.create_executor_binding(minimal_binding("binding-d2-001", "role-d-001"))
        .expect_err("same-role conflict");
    assert_eq!(
        repo.find_executor_binding("binding-d1-001").expect("find"),
        Some(original),
        "the blocking binding must be unchanged by the refusal"
    );
}

// T09 — the guard is per role: different roles may each hold one unreleased
// binding.
#[test]
fn t09_different_roles_remain_independent() {
    let tmp = TempDir::new("sab-t09");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-e1-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_logical_role(minimal_role("role-e2-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-e1-001", "role-e1-001"))
        .expect("first role binds");
    repo.create_executor_binding(minimal_binding("binding-e2-001", "role-e2-001"))
        .expect("a different role must not be blocked by another role's binding");
    assert_eq!(repo.count_table_rows("executor_binding").expect("rows"), 2);
}

// T10 — an explicit full release of B1 unlocks creation of B2 for the same
// role (CASE B).
#[test]
fn t10_release_then_rebind_succeeds() {
    let tmp = TempDir::new("sab-t10");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-f-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-f1-001", "role-f-001"))
        .expect("binding create");
    repo.create_executor_binding(minimal_binding("binding-f2-001", "role-f-001"))
        .expect_err("blocked while unreleased");
    repo.release_executor_binding(
        "binding-f1-001",
        "2026-08-16T10:30:00.000Z",
        ReleaseReason::UserRequest,
    )
    .expect("release");
    let b2 = minimal_binding("binding-f2-001", "role-f-001");
    repo.create_executor_binding(b2.clone())
        .expect("an explicitly released role may be rebound");
    assert_eq!(
        repo.find_executor_binding("binding-f2-001").expect("find"),
        Some(b2)
    );
}

// T11 — the released B1 remains readable as durable history after B2 is
// created for the same role.
#[test]
fn t11_released_b1_remains_readable_after_b2() {
    let tmp = TempDir::new("sab-t11");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-g-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let mut b1 = minimal_binding("binding-g1-001", "role-g-001");
    repo.create_executor_binding(b1.clone())
        .expect("binding create");
    repo.release_executor_binding(
        "binding-g1-001",
        "2026-08-16T10:30:00.000Z",
        ReleaseReason::RateLimited,
    )
    .expect("release");
    b1.released_at = Some("2026-08-16T10:30:00.000Z".to_string());
    b1.release_reason = Some(ReleaseReason::RateLimited);
    repo.create_executor_binding(minimal_binding("binding-g2-001", "role-g-001"))
        .expect("rebind");
    assert_eq!(
        repo.find_executor_binding("binding-g1-001").expect("find"),
        Some(b1),
        "released history must survive rebinding"
    );
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        2,
        "no released row may be deleted, replaced, or compacted by rebinding"
    );
}

// T12 — multiple fully released bindings for one role remain allowed, and a
// further binding may still be created afterwards (CASE C).
#[test]
fn t12_multiple_released_history_allowed() {
    let tmp = TempDir::new("sab-t12");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-h-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-h1-001", "role-h-001"))
        .expect("binding create");
    repo.release_executor_binding(
        "binding-h1-001",
        "2026-08-16T10:30:00.000Z",
        ReleaseReason::Completed,
    )
    .expect("release");
    repo.create_executor_binding(minimal_binding("binding-h2-001", "role-h-001"))
        .expect("rebind");
    repo.release_executor_binding(
        "binding-h2-001",
        "2026-08-16T11:30:00.000Z",
        ReleaseReason::HostSwitch,
    )
    .expect("release");
    repo.create_executor_binding(minimal_binding("binding-h3-001", "role-h-001"))
        .expect("a role with only fully released history may bind again");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        3,
        "all prior released bindings remain persisted"
    );
    assert!(
        repo.find_executor_binding("binding-h1-001")
            .expect("find")
            .is_some()
    );
    assert!(
        repo.find_executor_binding("binding-h2-001")
            .expect("find")
            .is_some()
    );
}

// T13 — after rebind, the new B2 blocks B3 until B2 itself is released.
#[test]
fn t13_b2_blocks_b3_until_released() {
    let tmp = TempDir::new("sab-t13");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-i-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-i1-001", "role-i-001"))
        .expect("binding create");
    repo.release_executor_binding(
        "binding-i1-001",
        "2026-08-16T10:30:00.000Z",
        ReleaseReason::Crash,
    )
    .expect("release");
    repo.create_executor_binding(minimal_binding("binding-i2-001", "role-i-001"))
        .expect("rebind");
    repo.create_executor_binding(minimal_binding("binding-i3-001", "role-i-001"))
        .expect_err("the new unreleased binding must block the next one");
    repo.release_executor_binding(
        "binding-i2-001",
        "2026-08-16T11:30:00.000Z",
        ReleaseReason::SessionExhausted,
    )
    .expect("release");
    repo.create_executor_binding(minimal_binding("binding-i3-001", "role-i-001"))
        .expect("releasing the current binding re-opens the role");
}

// T14 — a renewed-but-unreleased binding still blocks a second binding
// (CASE E).
#[test]
fn t14_renew_then_second_binding_refused() {
    let tmp = TempDir::new("sab-t14");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-j-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-j1-001", "role-j-001"))
        .expect("binding create");
    repo.renew_executor_binding_lease(
        &trusted_clock(),
        "binding-j1-001",
        "2026-08-16T12:00:00.000000000Z",
    )
    .expect("renew");
    let error = repo
        .create_executor_binding(minimal_binding("binding-j2-001", "role-j-001"))
        .expect_err("renewal must not create another binding slot");
    assert!(
        matches!(error, StateError::ExecutorBindingUnreleasedConflict { .. }),
        "unexpected error: {error}"
    );
    // After releasing the renewed binding, the rebind succeeds.
    repo.release_executor_binding(
        "binding-j1-001",
        "2026-08-16T12:30:00.000Z",
        ReleaseReason::UserRequest,
    )
    .expect("release");
    repo.create_executor_binding(minimal_binding("binding-j2-001", "role-j-001"))
        .expect("release still unlocks rebind after renewal");
}

// T15 — the renewed lease value is unchanged by the refused second-binding
// creation.
#[test]
fn t15_renewed_lease_unchanged_by_refusal() {
    let tmp = TempDir::new("sab-t15");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-k-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut renewed = minimal_binding("binding-k1-001", "role-k-001");
    repo.create_executor_binding(renewed.clone())
        .expect("binding create");
    repo.renew_executor_binding_lease(
        &trusted_clock(),
        "binding-k1-001",
        "2026-08-16T23:00:00.000000000Z",
    )
    .expect("renew");
    repo.create_executor_binding(minimal_binding("binding-k2-001", "role-k-001"))
        .expect_err("same-role conflict");
    renewed.lease_expires_at = "2026-08-16T23:00:00.000000000Z".to_string();
    assert_eq!(
        repo.find_executor_binding("binding-k1-001").expect("find"),
        Some(renewed),
        "conflict handling must not adjust the renewed lease value"
    );
}

// T16 — a past-looking lease_expires_at does not permit another binding
// without an explicit release (CASE F): the guard renders no wall-clock
// verdict, so an old-looking value blocks exactly like a fresh one.
#[test]
fn t16_past_looking_lease_still_blocks() {
    let tmp = TempDir::new("sab-t16");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-l-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut stale = minimal_binding("binding-l1-001", "role-l-001");
    stale.lease_expires_at = "1999-01-01T00:00:00.000Z".to_string();
    repo.create_executor_binding(stale)
        .expect("State never evaluates the lease value against a clock");
    let error = repo
        .create_executor_binding(minimal_binding("binding-l2-001", "role-l-001"))
        .expect_err("an old-looking but unreleased lease must still block");
    assert!(
        matches!(error, StateError::ExecutorBindingUnreleasedConflict { .. }),
        "unexpected error: {error}"
    );
    // Generic LEASE_EXPIRED cannot unlock the role.
    assert!(matches!(
        repo.release_executor_binding(
            "binding-l1-001",
            "2026-08-16T10:30:00.000Z",
            ReleaseReason::LeaseExpired,
        ),
        Err(StateError::ExecutorBindingValidation { .. })
    ));
    repo.create_executor_binding(minimal_binding("binding-l2-001", "role-l-001"))
        .expect_err("rejected generic expiry leaves the original binding active");
}

// T17 — no wall-clock comparison is used anywhere in the blocking decision
// of T16. Compile-time/source-inspection invariant: see the
// COMPILE_TIME_API_INVARIANTS block at the bottom of this module.

// T18 — a LEASE_EXPIRED release permits subsequent rebinding, proven
// through the only path authorized to create that state: the explicit
// trusted lease-expiry transaction.
//
// This also carries CREATE-BYPASS-12 and CREATE-BYPASS-13: a past-deadline
// but unreleased binding still blocks its successor, and only a successful
// explicit expiry lifts that block — for a later, separately requested
// create. Nothing rebinds automatically.
#[test]
fn t18_lease_expired_release_permits_rebind() {
    let (_tmp, mut repo, original) = seeded("sab-t18", DEADLINE);

    // CREATE-BYPASS-12 — trusted time is already at the deadline, yet while
    // the binding is unreleased it still blocks a successor: State never
    // infers a release from a lease value.
    let successor = minimal_binding("binding-m2-001", &original.role_id);
    let blocked = repo
        .create_executor_binding(successor.clone())
        .expect_err("a past-deadline but unreleased binding must still block");
    assert!(
        matches!(
            blocked,
            StateError::ExecutorBindingUnreleasedConflict { .. }
        ),
        "unexpected error: {blocked}"
    );

    // The legitimate explicit trusted expiry releases the old binding.
    assert_eq!(
        repo.expire_executor_binding_lease(
            &clock(DEADLINE),
            request(&original, DEADLINE, "01ARZ3NDEKTSV4RRFFQ69G5FB1")
        )
        .expect("trusted expiry"),
        ExecutorLeaseExpiryOutcomeV1::Released
    );
    let released = repo
        .find_executor_binding(BINDING)
        .expect("find")
        .expect("present");
    assert_eq!(released.released_at.as_deref(), Some(DEADLINE));
    assert_eq!(released.release_reason, Some(ReleaseReason::LeaseExpired));

    // CREATE-BYPASS-13 — and only now does a separately requested successor
    // create succeed.
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "expiry itself never creates a successor"
    );
    repo.create_executor_binding(successor.clone())
        .expect("a fully released binding no longer blocks its role");
    assert_eq!(
        repo.find_executor_binding("binding-m2-001").expect("find"),
        Some(successor)
    );
}

// CREATE-BYPASS-14 — pre-existing LEASE_EXPIRED rows stay fully readable
// and non-blocking. Readability is not writability: such a row is set up
// below the typed boundary, because the public create path no longer
// manufactures new LEASE_EXPIRED state and no read path was removed.
#[test]
fn t18b_historical_lease_expired_row_remains_readable_and_non_blocking() {
    let tmp = TempDir::new("sab-t18b");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-m-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut historical = minimal_binding("binding-m1-001", "role-m-001");
    historical.released_at = Some("2026-08-16T10:30:00.000Z".to_string());
    historical.release_reason = Some(ReleaseReason::LeaseExpired);
    assert!(
        matches!(
            repo.create_executor_binding(historical.clone()),
            Err(StateError::ExecutorBindingValidation { .. })
        ),
        "the public create path must not manufacture historical-looking state"
    );
    direct_insert_binding(&mut repo, &historical).expect("historical row reaches storage");
    assert_eq!(
        repo.find_executor_binding("binding-m1-001").expect("find"),
        Some(historical),
        "a historical LEASE_EXPIRED row decodes exactly as recorded"
    );
    repo.create_executor_binding(minimal_binding("binding-m2-001", "role-m-001"))
        .expect("a historical LEASE_EXPIRED-released binding does not block");
}

// T19 — a partial terminal row carrying released_at only makes creation for
// that role fail closed (CASE G, first corrupt shape).
#[test]
fn t19_partial_terminal_released_at_only_fails_closed() {
    let tmp = TempDir::new("sab-t19");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-n-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    // Construct the corrupt row below the typed boundary: this repository
    // never produces a partial terminal pair itself.
    let mut partial = minimal_binding("binding-n1-001", "role-n-001");
    partial.released_at = Some("2026-08-16T10:30:00.000Z".to_string());
    partial.release_reason = None;
    direct_insert_binding(&mut repo, &partial).expect("corrupt probe row reaches storage");
    let error = repo
        .create_executor_binding(minimal_binding("binding-n2-001", "role-n-001"))
        .expect_err("corrupt terminal history must fail closed");
    assert!(
        matches!(error, StateError::ExecutorBindingUnreleasedConflict { .. }),
        "a partial terminal row must not look like a released role: {error}"
    );
}

// T20 — a partial terminal row carrying release_reason only makes creation
// for that role fail closed (CASE G, second corrupt shape).
#[test]
fn t20_partial_terminal_release_reason_only_fails_closed() {
    let tmp = TempDir::new("sab-t20");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-o-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut partial = minimal_binding("binding-o1-001", "role-o-001");
    partial.released_at = None;
    partial.release_reason = Some(ReleaseReason::Crash);
    direct_insert_binding(&mut repo, &partial).expect("corrupt probe row reaches storage");
    let error = repo
        .create_executor_binding(minimal_binding("binding-o2-001", "role-o-001"))
        .expect_err("corrupt terminal history must fail closed");
    assert!(
        matches!(error, StateError::ExecutorBindingUnreleasedConflict { .. }),
        "a partial terminal row must not look like a released role: {error}"
    );
}

// T21 — the failed creation does not repair, complete, or clear the corrupt
// partial terminal pair: the row remains evidence.
#[test]
fn t21_partial_corrupt_row_not_repaired() {
    let tmp = TempDir::new("sab-t21");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-p-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut partial = minimal_binding("binding-p1-001", "role-p-001");
    partial.released_at = Some("2026-08-16T10:30:00.000Z".to_string());
    direct_insert_binding(&mut repo, &partial).expect("corrupt probe row reaches storage");
    repo.create_executor_binding(minimal_binding("binding-p2-001", "role-p-001"))
        .expect_err("same-role conflict");
    assert_eq!(
        repo.find_executor_binding("binding-p1-001").expect("find"),
        Some(partial),
        "the corrupt terminal pair must be preserved exactly as evidence"
    );
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "no repair row or replacement may appear"
    );
}

// T22 — the database index itself rejects a second not-fully-released row
// for the same role even when the application pre-check is bypassed by
// direct SQL inside State: the backstop is real integrity, not a name.
#[test]
fn t22_index_rejects_second_unreleased_row_bypassing_precheck() {
    let tmp = TempDir::new("sab-t22");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-q-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    direct_insert_binding(&mut repo, &minimal_binding("binding-q1-001", "role-q-001"))
        .expect("first direct row");
    let error = direct_insert_binding(&mut repo, &minimal_binding("binding-q2-001", "role-q-001"))
        .expect_err("the partial unique index must reject the second unreleased row");
    let StateError::InternalQueryFailed { detail } = &error else {
        panic!("unexpected error: {error}");
    };
    assert!(
        detail.contains("UNIQUE constraint failed") && detail.contains("role_id"),
        "expected a UNIQUE constraint failure naming role_id, observed: {detail}"
    );
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "the rejected statement must leave only the first row"
    );
}

// T23 — the partial index includes valid unreleased rows: a binding created
// through the typed path participates in the uniqueness set, so even a
// direct-SQL writer cannot add a second unreleased row for that role.
#[test]
fn t23_index_includes_valid_unreleased_row() {
    let tmp = TempDir::new("sab-t23");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-r-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-r1-001", "role-r-001"))
        .expect("typed binding create");
    let error = direct_insert_binding(&mut repo, &minimal_binding("binding-r2-001", "role-r-001"))
        .expect_err("a typed-created unreleased row must be uniqueness-blocking");
    assert!(
        matches!(error, StateError::InternalQueryFailed { ref detail } if detail.contains("UNIQUE constraint failed")),
        "unexpected error: {error}"
    );
}

// T24 — the partial index excludes fully released rows: once the terminal
// pair is complete, a direct-SQL unreleased insert for the same role
// succeeds.
#[test]
fn t24_index_excludes_fully_released_row() {
    let tmp = TempDir::new("sab-t24");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-s-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-s1-001", "role-s-001"))
        .expect("binding create");
    repo.release_executor_binding(
        "binding-s1-001",
        "2026-08-16T10:30:00.000Z",
        ReleaseReason::Completed,
    )
    .expect("release");
    direct_insert_binding(&mut repo, &minimal_binding("binding-s2-001", "role-s-001"))
        .expect("a fully released row must leave the uniqueness set");
    assert_eq!(repo.count_table_rows("executor_binding").expect("rows"), 2);
}

// T25 — the fail-closed predicate covers both corrupt partial terminal
// shapes: each is inside the uniqueness set, so neither an unreleased row
// nor the other partial shape can be added beside it.
#[test]
fn t25_index_predicate_covers_partial_terminal_state() {
    let tmp = TempDir::new("sab-t25");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-t-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    // released_at only.
    let mut partial_a = minimal_binding("binding-t1a-001", "role-t-001");
    partial_a.released_at = Some("2026-08-16T10:30:00.000Z".to_string());
    direct_insert_binding(&mut repo, &partial_a).expect("released_at-only probe row");
    let error = direct_insert_binding(&mut repo, &minimal_binding("binding-t2a-001", "role-t-001"))
        .expect_err("a released_at-only row must remain uniqueness-blocking");
    assert!(
        matches!(error, StateError::InternalQueryFailed { ref detail } if detail.contains("UNIQUE constraint failed")),
        "unexpected error: {error}"
    );

    // release_reason only, on a fresh role.
    repo.create_logical_role(minimal_role("role-t-002", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let mut partial_b = minimal_binding("binding-t1b-001", "role-t-002");
    partial_b.release_reason = Some(ReleaseReason::Crash);
    direct_insert_binding(&mut repo, &partial_b).expect("release_reason-only probe row");
    let error = direct_insert_binding(&mut repo, &minimal_binding("binding-t2b-001", "role-t-002"))
        .expect_err("a release_reason-only row must remain uniqueness-blocking");
    assert!(
        matches!(error, StateError::InternalQueryFailed { ref detail } if detail.contains("UNIQUE constraint failed")),
        "unexpected error: {error}"
    );
}

// T26 — duplicate binding_id retains its existing distinct behavior and
// precedence, even though the existing row also occupies the role.
#[test]
fn t26_duplicate_binding_id_behavior_retained() {
    let tmp = TempDir::new("sab-t26");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-u-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-u1-001", "role-u-001"))
        .expect("binding create");
    let error = repo
        .create_executor_binding(minimal_binding("binding-u1-001", "role-u-001"))
        .expect_err("duplicate binding_id must keep its own error");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingAlreadyExists { binding_id } if binding_id == "binding-u1-001"
        ),
        "duplicate-ID semantics must not be reclassified as a role conflict: {error}"
    );
}

// T27 — a different binding_id blocked by the same role produces the new,
// distinct domain conflict behavior, never the duplicate-ID error.
#[test]
fn t27_same_role_conflict_is_distinct_behavior() {
    let tmp = TempDir::new("sab-t27");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-v-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-v1-001", "role-v-001"))
        .expect("binding create");
    let error = repo
        .create_executor_binding(minimal_binding("binding-v2-001", "role-v-001"))
        .expect_err("same-role conflict");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingUnreleasedConflict {
                role_id,
                blocking_binding_id
            } if role_id == "role-v-001" && blocking_binding_id == "binding-v1-001"
        ),
        "unexpected error: {error}"
    );
    assert!(
        !matches!(error, StateError::ExecutorBindingAlreadyExists { .. }),
        "the two failure conditions must remain independently observable"
    );
}

// T28 — the foreign-key requirement to LogicalRole remains enforced, both
// at the typed boundary and as a storage backstop.
#[test]
fn t28_foreign_key_to_logical_role_remains_enforced() {
    let tmp = TempDir::new("sab-t28");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let error = repo
        .create_executor_binding(minimal_binding("binding-w1-001", "role-w-missing"))
        .expect_err("a binding for a nonexistent role must fail explicitly");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingRoleNotFound { role_id } if role_id == "role-w-missing"
        ),
        "unexpected error: {error}"
    );
    // The storage-level FK backstop also still holds below the typed path.
    let error = direct_insert_binding(
        &mut repo,
        &minimal_binding("binding-w2-001", "role-w-missing"),
    )
    .expect_err("the storage FK constraint must refuse an orphan binding");
    assert!(
        matches!(error, StateError::InternalQueryFailed { ref detail } if detail.contains("FOREIGN KEY constraint failed")),
        "unexpected error: {error}"
    );
    assert_eq!(repo.count_table_rows("executor_binding").expect("rows"), 0);
}

// T29 — all nine ReleaseReason values remain exactly the frozen set, with
// their exact durable representations.
#[test]
fn t29_release_reason_values_remain_exact() {
    let expected = [
        ("RATE_LIMITED", ReleaseReason::RateLimited),
        ("SESSION_EXHAUSTED", ReleaseReason::SessionExhausted),
        ("AUTH_REQUIRED", ReleaseReason::AuthRequired),
        ("PROVIDER_DOWN", ReleaseReason::ProviderDown),
        ("CRASH", ReleaseReason::Crash),
        ("HOST_SWITCH", ReleaseReason::HostSwitch),
        ("USER_REQUEST", ReleaseReason::UserRequest),
        ("COMPLETED", ReleaseReason::Completed),
        ("LEASE_EXPIRED", ReleaseReason::LeaseExpired),
    ];
    for (text, reason) in expected {
        assert_eq!(reason.as_str(), text);
    }
    for invalid in ["UNKNOWN", "OTHER", "ACTIVE", ""] {
        assert!(
            matches!(
                ReleaseReason::from_storage(invalid),
                Err(StateError::ExecutorBindingDecodeFailed { .. })
            ),
            "{invalid:?} must not decode"
        );
    }
}

// T30 — ExecutorBinding create/read round-trip remains unchanged for a
// fully populated row.
#[test]
fn t30_create_read_round_trip_unchanged() {
    let tmp = TempDir::new("sab-t30");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-x-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let binding = ExecutorBinding {
        binding_id: "binding-x1-001".to_string(),
        role_id: "role-x-001".to_string(),
        provider_id: "provider-omega".to_string(),
        model_id: "model-nine".to_string(),
        runtime_id: "runtime-a2-host".to_string(),
        session_ref: Some("sessions/0192-abc/0042".to_string()),
        routing_decision_id: Some("routing-decision-0177".to_string()),
        bound_at: "2026-08-16T09:30:00.000Z".to_string(),
        lease_expires_at: "2026-08-16T10:30:00.000Z".to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: Some(false),
    };
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    assert_eq!(
        repo.find_executor_binding("binding-x1-001").expect("find"),
        Some(binding)
    );
}

// T31 — release remains write-once: a second release of the same binding
// is refused.
#[test]
fn t31_release_remains_write_once() {
    let tmp = TempDir::new("sab-t31");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-y-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-y1-001", "role-y-001"))
        .expect("binding create");
    repo.release_executor_binding(
        "binding-y1-001",
        "2026-08-16T10:30:00.000Z",
        ReleaseReason::UserRequest,
    )
    .expect("first release");
    let error = repo
        .release_executor_binding(
            "binding-y1-001",
            "2026-08-16T11:00:00.000Z",
            ReleaseReason::Crash,
        )
        .expect_err("a second release must be refused");
    assert!(
        matches!(error, StateError::ExecutorBindingAlreadyReleased { .. }),
        "unexpected error: {error}"
    );
}

// T32 — lease renewal remains guarded to an unreleased binding: it works
// before release and is refused afterwards.
#[test]
fn t32_renewal_guarded_to_unreleased_binding() {
    let tmp = TempDir::new("sab-t32");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-z-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-z1-001", "role-z-001"))
        .expect("binding create");
    repo.renew_executor_binding_lease(
        &trusted_clock(),
        "binding-z1-001",
        "2026-08-16T12:00:00.000000000Z",
    )
    .expect("renewal while unreleased");
    repo.release_executor_binding(
        "binding-z1-001",
        "2026-08-16T12:30:00.000Z",
        ReleaseReason::Completed,
    )
    .expect("release");
    let error = repo
        .renew_executor_binding_lease(
            &trusted_clock(),
            "binding-z1-001",
            "2026-08-16T13:00:00.000Z",
        )
        .expect_err("renewal after release must be refused");
    assert!(
        matches!(error, StateError::ExecutorBindingAlreadyReleased { .. }),
        "unexpected error: {error}"
    );
}

// T33 — lease renewal after a release is refused and leaves the terminal
// pair and every other field untouched.
#[test]
fn t33_renewal_after_release_refused() {
    let tmp = TempDir::new("sab-t33");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-aa-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let mut released = minimal_binding("binding-aa1-001", "role-aa-001");
    repo.create_executor_binding(released.clone())
        .expect("binding create");
    repo.release_executor_binding(
        "binding-aa1-001",
        "2026-08-16T10:30:00.000Z",
        ReleaseReason::AuthRequired,
    )
    .expect("release");
    repo.renew_executor_binding_lease(
        &trusted_clock(),
        "binding-aa1-001",
        "2099-01-01T00:00:00.000Z",
    )
    .expect_err("no renewal after terminal release");
    released.released_at = Some("2026-08-16T10:30:00.000Z".to_string());
    released.release_reason = Some(ReleaseReason::AuthRequired);
    assert_eq!(
        repo.find_executor_binding("binding-aa1-001").expect("find"),
        Some(released),
        "a refused renewal must not reopen or mutate the released binding"
    );
}

// T34 — renewal creates no new binding row: only the one existing row
// remains, with its lease value replaced.
#[test]
fn t34_renewal_creates_no_new_row() {
    let tmp = TempDir::new("sab-t34");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-ab-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-ab1-001", "role-ab-001"))
        .expect("binding create");
    repo.renew_executor_binding_lease(
        &trusted_clock(),
        "binding-ab1-001",
        "2026-08-16T12:00:00.000000000Z",
    )
    .expect("renew");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "renewal must not create a binding slot"
    );
}

// T35 — no field of the existing binding is mutated by conflict handling.
#[test]
fn t35_no_existing_binding_field_mutated_by_conflict() {
    let tmp = TempDir::new("sab-t35");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-ac-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let original = ExecutorBinding {
        binding_id: "binding-ac1-001".to_string(),
        role_id: "role-ac-001".to_string(),
        provider_id: "provider-omega".to_string(),
        model_id: "model-nine".to_string(),
        runtime_id: "runtime-a2-host".to_string(),
        session_ref: Some("sessions/0192-abc/0042".to_string()),
        routing_decision_id: Some("routing-decision-0177".to_string()),
        bound_at: "2026-08-16T09:30:00.000Z".to_string(),
        lease_expires_at: "2026-08-16T10:30:00.000Z".to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: Some(true),
    };
    repo.create_executor_binding(original.clone())
        .expect("binding create");
    repo.create_executor_binding(minimal_binding("binding-ac2-001", "role-ac-001"))
        .expect_err("same-role conflict");
    assert_eq!(
        repo.find_executor_binding("binding-ac1-001").expect("find"),
        Some(original),
        "every field of the blocking binding must be preserved byte-for-byte"
    );
}

// T38 — a refused creation does not mutate the referenced LogicalRole in
// any way.
#[test]
fn t38_conflict_does_not_mutate_logical_role() {
    let tmp = TempDir::new("sab-t38");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut role = minimal_role("role-ae-001", LogicalRoleType::RuntimeA2);
    role.current_context_epoch = 7;
    role.name = Some("Guarded role".to_string());
    role.ownership_paths = vec!["receipts/one".to_string(), "receipts/two".to_string()];
    repo.create_logical_role(role.clone()).expect("role create");
    repo.create_executor_binding(minimal_binding("binding-ae1-001", "role-ae-001"))
        .expect("binding create");
    repo.create_executor_binding(minimal_binding("binding-ae2-001", "role-ae-001"))
        .expect_err("same-role conflict");
    assert_eq!(
        repo.find_logical_role("role-ae-001").expect("find"),
        Some(role),
        "conflict handling must leave every LogicalRole field untouched"
    );
}

// T39 — no LogicalRole active-binding pointer is introduced: the frozen
// migration-0002 column set is unchanged and active_binding_id stays NULL
// through conflict and rebind.
#[test]
fn t39_no_logical_role_active_binding_pointer_introduced() {
    let tmp = TempDir::new("sab-t39");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(
        repo.table_columns("logical_role").expect("columns"),
        LOGICAL_ROLE_COLUMNS.to_vec(),
        "the LogicalRole column set must remain exactly the frozen set"
    );
    repo.create_logical_role(minimal_role("role-af-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-af1-001", "role-af-001"))
        .expect("binding create");
    repo.create_executor_binding(minimal_binding("binding-af2-001", "role-af-001"))
        .expect_err("same-role conflict");
    repo.release_executor_binding(
        "binding-af1-001",
        "2026-08-16T10:30:00.000Z",
        ReleaseReason::UserRequest,
    )
    .expect("release");
    repo.create_executor_binding(minimal_binding("binding-af2-001", "role-af-001"))
        .expect("rebind");
    let found = repo
        .find_logical_role("role-af-001")
        .expect("find")
        .expect("present");
    assert_eq!(
        found.active_binding_id, None,
        "no binding may ever be attached to the role as an active pointer"
    );
}

// T50 — applying migration 5 to a version-4 database preserves existing
// binding history exactly: released and unreleased rows alike survive the
// index creation byte-for-byte.
#[test]
fn t50_migration_preserves_binding_history() {
    let tmp = TempDir::new("sab-t50");
    let (released, unreleased) = {
        let mut repo = open_version_4(&tmp);
        repo.create_logical_role(minimal_role("role-ag-001", LogicalRoleType::RuntimeA1))
            .expect("role create");
        repo.create_logical_role(minimal_role("role-ag-002", LogicalRoleType::RuntimeA2))
            .expect("role create");
        let mut released = minimal_binding("binding-ag1-001", "role-ag-001");
        repo.create_executor_binding(released.clone())
            .expect("binding create");
        repo.release_executor_binding(
            "binding-ag1-001",
            "2026-08-16T10:30:00.000Z",
            ReleaseReason::Completed,
        )
        .expect("release");
        released.released_at = Some("2026-08-16T10:30:00.000Z".to_string());
        released.release_reason = Some(ReleaseReason::Completed);
        let unreleased = minimal_binding("binding-ag2-001", "role-ag-002");
        repo.create_executor_binding(unreleased.clone())
            .expect("binding create");
        (released, unreleased)
    };
    {
        let mut repo = open_version_4(&tmp);
        repo.run_transaction(|uow| uow.execute_batch(migrations::registered()[4].sql))
            .expect("apply migration 5 over conforming version-4 history");
    }
    // The database now records version 5, so it opens with the version-5
    // prefix of the registered chain (the ordinary chain ends at version 7
    // and refuses a version-5 database).
    let repo =
        SqliteStateRepository::open_with_migrations(tmp.db_path(), &migrations::registered()[..5])
            .expect("open at version 5");
    assert_eq!(
        repo.find_executor_binding("binding-ag1-001").expect("find"),
        Some(released),
        "migration 5 must not rewrite released history"
    );
    assert_eq!(
        repo.find_executor_binding("binding-ag2-001").expect("find"),
        Some(unreleased),
        "migration 5 must not rewrite unreleased history"
    );
}

// T51 — if preexisting version-4 data violates the new constraint (two
// not-fully-released bindings for one role), migration 5 fails rather than
// silently repairing history: no row is deleted, released, re-pointed, or
// lease-adjusted, the version stays 4, and no index is left behind.
#[test]
fn t51_migration_conflicting_history_fails_without_repair() {
    let tmp = TempDir::new("sab-t51");
    let (first, second) = {
        let mut repo = open_version_4(&tmp);
        repo.create_logical_role(minimal_role("role-ah-001", LogicalRoleType::RuntimeA1))
            .expect("role create");
        let first = minimal_binding("binding-ah1-001", "role-ah-001");
        let mut second = minimal_binding("binding-ah2-001", "role-ah-001");
        second.lease_expires_at = "2027-01-01T00:00:00.000Z".to_string();
        second.provider_id = "provider-beta".to_string();
        // The typed create path already enforces the guard at every version,
        // so the conflicting version-4 rows are constructed through the
        // authorized test-only direct-SQL probe — exactly the pre-migration
        // state a version-4 database could be carrying.
        direct_insert_binding(&mut repo, &first).expect("create B1 at v4");
        direct_insert_binding(&mut repo, &second).expect("create B2 at v4");
        (first, second)
    };
    let mut repo = open_version_4(&tmp);
    let error = repo
        .run_transaction(|uow| uow.execute_batch(migrations::registered()[4].sql))
        .expect_err("migration 5 must fail on conflicting history");
    assert!(
        matches!(
            error,
            StateError::InternalQueryFailed { ref detail } if detail.contains("UNIQUE constraint failed")
        ),
        "unexpected error: {error}"
    );
    // Nothing was repaired, chosen, deleted, or rewritten.
    assert_eq!(
        repo.find_executor_binding("binding-ah1-001").expect("find"),
        Some(first),
        "the first conflicting row must be untouched"
    );
    assert_eq!(
        repo.find_executor_binding("binding-ah2-001").expect("find"),
        Some(second),
        "the second conflicting row must be untouched"
    );
    assert_eq!(
        repo.schema_version().expect("version read"),
        4,
        "the failed migration must not record version 5"
    );
    assert!(
        named_binding_indexes(&repo).is_empty(),
        "the failed migration must leave no index behind"
    );
}

// T52 — successful creation returns only after its transaction commits: the
// row is durably visible to a fresh connection.
#[test]
fn t52_success_returns_only_after_commit() {
    let tmp = TempDir::new("sab-t52");
    {
        let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
        repo.create_logical_role(minimal_role("role-ai-001", LogicalRoleType::RuntimeA1))
            .expect("role create");
        repo.create_executor_binding(minimal_binding("binding-ai1-001", "role-ai-001"))
            .expect("binding create");
    }
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert!(
        repo.find_executor_binding("binding-ai1-001")
            .expect("find")
            .is_some(),
        "a successful create must be committed before success is returned"
    );
}

// T53 — a failed same-role creation rolls back cleanly and leaves the store
// usable.
#[test]
fn t53_failed_creation_rolls_back_cleanly() {
    let tmp = TempDir::new("sab-t53");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-aj-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let original = minimal_binding("binding-aj1-001", "role-aj-001");
    repo.create_executor_binding(original.clone())
        .expect("binding create");
    repo.create_executor_binding(minimal_binding("binding-aj2-001", "role-aj-001"))
        .expect_err("same-role conflict");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "the failed transaction must leave no partial row"
    );
    assert_eq!(
        repo.find_executor_binding("binding-aj1-001").expect("find"),
        Some(original)
    );
    // The store remains fully usable after the rollback.
    repo.release_executor_binding(
        "binding-aj1-001",
        "2026-08-16T10:30:00.000Z",
        ReleaseReason::UserRequest,
    )
    .expect("release");
    repo.create_executor_binding(minimal_binding("binding-aj2-001", "role-aj-001"))
        .expect("store usable after rolled-back conflict");
}

// T54 — a SQLite backstop/constraint failure can never produce a partial
// B2: the rejected direct insert leaves the original row as the only row,
// and the store stays consistent afterwards.
#[test]
fn t54_backstop_failure_produces_no_partial_row() {
    let tmp = TempDir::new("sab-t54");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-ak-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let original = minimal_binding("binding-ak1-001", "role-ak-001");
    repo.create_executor_binding(original.clone())
        .expect("binding create");
    direct_insert_binding(
        &mut repo,
        &minimal_binding("binding-ak2-001", "role-ak-001"),
    )
    .expect_err("the backstop must refuse the second unreleased row");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "a constraint failure must not produce a partial row"
    );
    assert_eq!(
        repo.find_executor_binding("binding-ak1-001").expect("find"),
        Some(original)
    );
    // Durably consistent across close/reopen.
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(repo.count_table_rows("executor_binding").expect("rows"), 1);
    assert_eq!(
        repo.find_executor_binding("binding-ak2-001").expect("find"),
        None
    );
}

// ============================================================================
// COMPILE_TIME_API_INVARIANTS (T17, T36–T49, T55–T56)
// ============================================================================
//
// The following scope/absence requirements are compile-time or
// source-inspection invariants of this slice. No runtime behavior exists to
// fabricate a test from, so — exactly as the accepted baseline handled its
// T35 API-absence invariant — they are established by inspecting the source
// and public surface of `src/state/**` and reported in the task handoff
// rather than faked as runtime tests:
//
// T17 — no wall-clock comparison participates in any blocking decision:
//      `src/state/**` contains no `SystemTime`, `Instant`, `std::time`
//      use beyond the repository's fixed `busy_timeout` `Duration`, no
//      chrono/time-crate dependency (Cargo.toml unchanged), and no
//      timestamp parsing, ordering, or duration arithmetic on
//      `lease_expires_at`. The T16 runtime test supplies the behavioral
//      half: an obviously past lease blocks identically to a fresh one.
// T36 — no binding DELETE path: no `DELETE FROM executor_binding`
//      statement exists in `src/state/**`.
// T37 — no `INSERT OR REPLACE`, `REPLACE INTO`, or destructive UPSERT for
//      bindings exists in `src/state/**`; the only binding INSERT is the
//      plain guarded insert in `executor_binding.rs`.
// T40 — no active-binding/authority public lookup API: the public surface
//      of the crate remains exactly `create_executor_binding`,
//      `find_executor_binding`, `release_executor_binding`,
//      `renew_executor_binding_lease` (plus the non-binding LogicalRole,
//      event, and repository-open surface); no method named or behaving
//      like `find_active_binding`, `get_current_binding`,
//      `find_authoritative_executor`, `is_binding_authoritative`,
//      `lease_is_valid_now`, or similar exists on any public type.
// T41 — no time parser / timestamp comparison is introduced anywhere in
//      `src/state/**` (see T17).
// T42 — no automatic lease-expiry evaluator exists: no scheduler, timer,
//      expiry loop, or background task appears in `src/state/**`.
// T43 — no automatic LEASE_EXPIRED release exists: new `LEASE_EXPIRED`
//      transitions are recorded only by the trusted explicit expiry
//      transaction.
// T44 — no heartbeat/liveness system is introduced.
// T45 — no automatic executor replacement, rebind, or failover
//      orchestration is introduced; this slice only refuses a second
//      binding until an explicit release makes the role rebindable.
// T46 — no provider/model/runtime selection or routing is introduced.
// T47 — no ContextManifest/ContextEpoch/rehydration/startup recovery is
//      implemented.
// T48 — no event-production semantics are introduced: the guarded create
//      emits no events and adds no event type.
// T49 — the A3-007 strict EventEnvelope boundary is unchanged: `event.rs`,
//      `v0004_event.rs`, and `event_tests.rs` are untouched by this slice
//      except for mechanical schema-version expectations, and the event
//      tests still pass unmodified in behavior.
// T55 — no arbitrary-SQL public API is introduced: the new
//      `sqlite_master_entries` helper is `#[cfg(test)]` crate-private, and
//      the `UnitOfWork` SQL helpers remain `#[cfg(test)]` crate-private.
// T56 — no `unsafe` Rust exists anywhere in `src/state/**`.
