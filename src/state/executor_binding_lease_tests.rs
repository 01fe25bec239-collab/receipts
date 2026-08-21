//! Deterministic tests for bounded ExecutorBinding lease-renewal
//! persistence (T01–T48 of the A3-008 lease-persistence slice).
//!
//! All runtime tests use real temporary SQLite database files under the
//! system temporary directory (never inside the repository). Storage-level
//! checks that need SQL beyond the public repository API run through
//! crate-private helpers only and are `#[cfg(test)]`-gated.
//!
//! API-absence properties (T02's no-migration-file part and T34–T44, minus
//! the T43 compile-time signature assertion) are established by
//! compile-time signature assertions and source inspection of this crate's
//! public surface, not by fabricated runtime tests; the inspection
//! evidence is summarized at the bottom of this module and reported in the
//! task handoff's COMPILE_TIME_API_INVARIANTS section.

use crate::error::StateError;
use crate::event::{
    ActorKind, EventActor, EventEnvelope, EventPayloadReference, EventSubject, EventType,
    SubjectKind,
};
use crate::executor_binding::{ExecutorBinding, ReleaseReason, apply_lease_renewal};
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::tests::{TempDir, trusted_clock};

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

/// A minimal contract-valid unreleased binding: only required fields set,
/// every optional field absent, and contract date-time strings stored as
/// provided.
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

/// A fully populated unreleased binding: every optional field carries a
/// distinct value so that any unintended field mutation is detectable.
fn full_binding(binding_id: &str, role_id: &str) -> ExecutorBinding {
    ExecutorBinding {
        binding_id: binding_id.to_string(),
        role_id: role_id.to_string(),
        provider_id: "provider-omega".to_string(),
        model_id: "model-nine".to_string(),
        runtime_id: "runtime-a2-host".to_string(),
        session_ref: Some("sessions/0192-abc/0042".to_string()),
        routing_decision_id: Some("routing-decision-0177".to_string()),
        bound_at: "2026-08-16T09:30:00.000Z".to_string(),
        lease_expires_at: "2026-08-16T10:30:00.000000000Z".to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: Some(false),
    }
}

/// The first renewed lease value, supplied by the caller — never
/// generated, parsed, or compared by State.
const RENEWED_LEASE: &str = "2026-08-16T12:00:00.000000000Z";

/// A second, deliberately different renewal value for repeat-renewal
/// probing.
const SECOND_RENEWED_LEASE: &str = "2026-08-16T13:30:00.000000000Z";

/// The release timestamp used by the standard release path, supplied by
/// the caller — never generated, parsed, or compared by State.
const RELEASED_AT: &str = "2026-08-17T09:58:11.250Z";

/// Returns the binding as it must read back after a successful renewal.
fn with_renewed_lease(mut binding: ExecutorBinding, lease_expires_at: &str) -> ExecutorBinding {
    binding.lease_expires_at = lease_expires_at.to_string();
    binding
}

/// Returns the binding as it must read back after a successful release.
fn with_release(
    mut binding: ExecutorBinding,
    released_at: &str,
    release_reason: ReleaseReason,
) -> ExecutorBinding {
    binding.released_at = Some(released_at.to_string());
    binding.release_reason = Some(release_reason);
    binding
}

/// A minimal strict EventEnvelope for event-row non-mutation probes.
fn minimal_event(event_id: &str) -> EventEnvelope {
    EventEnvelope {
        event_id: event_id.to_string(),
        project_id: "project-1".to_string(),
        goal_id: None,
        event_type: EventType::TaskCreated,
        actor: EventActor {
            kind: ActorKind::System,
            id: None,
        },
        subject: EventSubject {
            kind: SubjectKind::Task,
            id: "task-001".to_string(),
        },
        occurred_at: "2026-08-17T10:00:00.000Z".to_string(),
        payload: EventPayloadReference {
            reference: "blob://events/0001".to_string(),
            digest: "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
                .to_string(),
        },
        correlation_id: "corr-0001".to_string(),
        epoch: 0,
    }
}

/// Creates a role and a fully populated binding, renews its lease once,
/// and returns the live store plus the pre-renewal original and the
/// post-renewal read of the same binding.
fn setup_renewed_full_binding(
    tag: &str,
) -> (
    TempDir,
    SqliteStateRepository,
    ExecutorBinding,
    ExecutorBinding,
) {
    let tmp = TempDir::new(tag);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-full-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let original = full_binding("binding-full-001", "role-full-001");
    repo.create_executor_binding(original.clone())
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-full-001", RENEWED_LEASE)
        .expect("renew");
    let found = repo
        .find_executor_binding("binding-full-001")
        .expect("find")
        .expect("present");
    (tmp, repo, original, found)
}

fn forced_failure() -> StateError {
    StateError::UnitOfWorkFailed {
        detail: "forced test failure".to_string(),
    }
}

// T01 — the schema version remains exactly 10 before and after lease
// renewals, including across close/reopen.
#[test]
fn t01_schema_version_remains_10() {
    let tmp = TempDir::new("ebl-t01");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(
        repo.schema_version().expect("version read"),
        10,
        "the supported schema version must be 10 before any renewal"
    );
    repo.create_logical_role(minimal_role("role-ver-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-ver-001", "role-ver-001"))
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-ver-001", RENEWED_LEASE)
        .expect("renew");
    assert_eq!(
        repo.schema_version().expect("version read"),
        10,
        "a lease renewal must not change the schema version"
    );
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(repo.schema_version().expect("version read"), 10);
}

// T02 — lease renewal itself introduces no migration beyond the authorized
// watermark migration: the registered chain ends at version 10, and the
// durable metadata carries exactly one row per applied migration after
// renewals.
#[test]
fn t02_no_migration_introduced_by_lease_renewal() {
    let registered = migrations::registered();
    assert_eq!(
        registered.len(),
        10,
        "exactly ten registered migrations (v0001–v0010) may exist"
    );
    assert_eq!(
        registered.last().expect("chain is non-empty").version,
        10,
        "the registered chain must end at version 10"
    );
    let tmp = TempDir::new("ebl-t02");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-v6-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-v6-001", "role-v6-001"))
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-v6-001", RENEWED_LEASE)
        .expect("renew");
    assert_eq!(
        repo.count_table_rows("state_schema_version").expect("rows"),
        10,
        "no extra migration metadata row may appear"
    );
}

// T03 — an existing unreleased ExecutorBinding renews its lease
// successfully, and the renewed record reads back as the original plus
// exactly the one replaced lease value.
#[test]
fn t03_renew_unreleased_binding_succeeds() {
    let tmp = TempDir::new("ebl-t03");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-ren-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let binding = minimal_binding("binding-ren-001", "role-ren-001");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-ren-001", RENEWED_LEASE)
        .expect("renew an existing unreleased binding");
    assert_eq!(
        repo.find_executor_binding("binding-ren-001")
            .expect("find")
            .expect("present"),
        with_renewed_lease(binding, RENEWED_LEASE),
        "renewal must change exactly lease_expires_at and nothing else"
    );
}

// T04 — find_executor_binding returns the renewed lease_expires_at value
// exactly as supplied.
#[test]
fn t04_find_returns_renewed_lease_exactly() {
    let tmp = TempDir::new("ebl-t04");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-ren-002", LogicalRoleType::RuntimeA2))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-ren-002", "role-ren-002"))
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-ren-002", RENEWED_LEASE)
        .expect("renew");
    assert_eq!(
        repo.find_executor_binding("binding-ren-002")
            .expect("find")
            .expect("present")
            .lease_expires_at,
        RENEWED_LEASE.to_string(),
        "the renewed lease must read back as the exact supplied contract string"
    );
}

// T05 — the renewed lease value survives database close and reopen
// exactly.
#[test]
fn t05_renewed_lease_survives_close_and_reopen() {
    let tmp = TempDir::new("ebl-t05");
    let binding = minimal_binding("binding-durable-001", "role-durable-001");
    {
        let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
        repo.create_logical_role(minimal_role("role-durable-001", LogicalRoleType::RuntimeA1))
            .expect("role create");
        repo.create_executor_binding(binding.clone())
            .expect("binding create");
        repo.renew_executor_binding_lease(&trusted_clock(), "binding-durable-001", RENEWED_LEASE)
            .expect("renew");
    }
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_executor_binding("binding-durable-001")
            .expect("find")
            .expect("present"),
        with_renewed_lease(binding, RENEWED_LEASE),
        "the renewed lease value must still be present exactly after reopen"
    );
}

// T06 — a second renewal of the same unreleased binding persists the
// second supplied lease_expires_at value.
#[test]
fn t06_second_renewal_persists_second_value() {
    let tmp = TempDir::new("ebl-t06");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-twice-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-twice-001", "role-twice-001"))
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-twice-001", RENEWED_LEASE)
        .expect("first renew");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-twice-001", SECOND_RENEWED_LEASE)
        .expect("second renew on the same unreleased binding");
    assert_eq!(
        repo.find_executor_binding("binding-twice-001")
            .expect("find")
            .expect("present")
            .lease_expires_at,
        SECOND_RENEWED_LEASE.to_string(),
        "the second supplied lease value must win"
    );
}

// T07 — the second renewal changes only lease_expires_at: the full record
// equals the original with only that one field replaced.
#[test]
fn t07_second_renewal_changes_only_lease_expires_at() {
    let tmp = TempDir::new("ebl-t07");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-only-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let binding = full_binding("binding-only-001", "role-only-001");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-only-001", RENEWED_LEASE)
        .expect("first renew");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-only-001", SECOND_RENEWED_LEASE)
        .expect("second renew");
    assert_eq!(
        repo.find_executor_binding("binding-only-001")
            .expect("find")
            .expect("present"),
        with_renewed_lease(binding, SECOND_RENEWED_LEASE),
        "two renewals must leave every field other than lease_expires_at untouched"
    );
}

// T08 — renewal preserves binding_id.
#[test]
fn t08_renewal_preserves_binding_id() {
    let (_tmp, _repo, original, found) = setup_renewed_full_binding("ebl-t08");
    assert_eq!(found.binding_id, original.binding_id);
    assert_eq!(found, with_renewed_lease(original, RENEWED_LEASE));
}

// T09 — renewal preserves role_id.
#[test]
fn t09_renewal_preserves_role_id() {
    let (_tmp, _repo, original, found) = setup_renewed_full_binding("ebl-t09");
    assert_eq!(found.role_id, original.role_id);
    assert_eq!(found, with_renewed_lease(original, RENEWED_LEASE));
}

// T10 — renewal preserves provider_id.
#[test]
fn t10_renewal_preserves_provider_id() {
    let (_tmp, _repo, original, found) = setup_renewed_full_binding("ebl-t10");
    assert_eq!(found.provider_id, original.provider_id);
    assert_eq!(found, with_renewed_lease(original, RENEWED_LEASE));
}

// T11 — renewal preserves model_id.
#[test]
fn t11_renewal_preserves_model_id() {
    let (_tmp, _repo, original, found) = setup_renewed_full_binding("ebl-t11");
    assert_eq!(found.model_id, original.model_id);
    assert_eq!(found, with_renewed_lease(original, RENEWED_LEASE));
}

// T12 — renewal preserves runtime_id.
#[test]
fn t12_renewal_preserves_runtime_id() {
    let (_tmp, _repo, original, found) = setup_renewed_full_binding("ebl-t12");
    assert_eq!(found.runtime_id, original.runtime_id);
    assert_eq!(found, with_renewed_lease(original, RENEWED_LEASE));
}

// T13 — renewal preserves session_ref.
#[test]
fn t13_renewal_preserves_session_ref() {
    let (_tmp, _repo, original, found) = setup_renewed_full_binding("ebl-t13");
    assert_eq!(found.session_ref, original.session_ref);
    assert_eq!(found, with_renewed_lease(original, RENEWED_LEASE));
}

// T14 — renewal preserves routing_decision_id.
#[test]
fn t14_renewal_preserves_routing_decision_id() {
    let (_tmp, _repo, original, found) = setup_renewed_full_binding("ebl-t14");
    assert_eq!(found.routing_decision_id, original.routing_decision_id);
    assert_eq!(found, with_renewed_lease(original, RENEWED_LEASE));
}

// T15 — renewal preserves bound_at.
#[test]
fn t15_renewal_preserves_bound_at() {
    let (_tmp, _repo, original, found) = setup_renewed_full_binding("ebl-t15");
    assert_eq!(found.bound_at, original.bound_at);
    assert_eq!(found, with_renewed_lease(original, RENEWED_LEASE));
}

// T16 — renewal preserves rehydration_completed.
#[test]
fn t16_renewal_preserves_rehydration_completed() {
    let (_tmp, _repo, original, found) = setup_renewed_full_binding("ebl-t16");
    assert_eq!(found.rehydration_completed, original.rehydration_completed);
    assert_eq!(found, with_renewed_lease(original, RENEWED_LEASE));
}

// T17 — renewal of a nonexistent binding fails deterministically with the
// unknown-identity error and creates no row, durably.
#[test]
fn t17_renewal_of_nonexistent_binding_fails_and_creates_no_row() {
    let tmp = TempDir::new("ebl-t17");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-unknown-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-unknown-001", "role-unknown-001"))
        .expect("binding create");
    let error = repo
        .renew_executor_binding_lease(&trusted_clock(), "never-created", RENEWED_LEASE)
        .expect_err("renewing an unknown binding must fail explicitly");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingNotFound { binding_id } if binding_id == "never-created"
        ),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.find_executor_binding("never-created").expect("find"),
        None,
        "a failed renewal must not fabricate or implicitly create a binding"
    );
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "only the one real binding row may exist"
    );
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_executor_binding("never-created").expect("find"),
        None,
        "no fabricated binding may appear after reopen"
    );
}

// T18 — an empty binding_id is rejected before any storage access.
#[test]
fn t18_empty_binding_id_rejected() {
    let tmp = TempDir::new("ebl-t18");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-input-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-input-001", "role-input-001"))
        .expect("binding create");
    let error = repo
        .renew_executor_binding_lease(&trusted_clock(), "", RENEWED_LEASE)
        .expect_err("an empty binding_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );
}

// T19 — an over-length binding_id is rejected by the accepted identifier
// validation (at most 200 scalar values).
#[test]
fn t19_overlong_binding_id_rejected() {
    let tmp = TempDir::new("ebl-t19");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-input-002", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-input-002", "role-input-002"))
        .expect("binding create");
    let too_long = "b".repeat(201);
    let error = repo
        .renew_executor_binding_lease(&trusted_clock(), &too_long, RENEWED_LEASE)
        .expect_err("an over-length binding_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );
}

// T20 — an empty lease_expires_at is rejected before any storage access.
#[test]
fn t20_empty_lease_expires_at_rejected() {
    let tmp = TempDir::new("ebl-t20");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-input-003", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-input-003", "role-input-003"))
        .expect("binding create");
    let error = repo
        .renew_executor_binding_lease(&trusted_clock(), "binding-input-003", "")
        .expect_err("an empty lease_expires_at must be rejected");
    assert!(
        matches!(error, StateError::CanonicalTimestampInvalid { .. }),
        "unexpected error: {error}"
    );
}

// T21 — every rejected renewal leaves the original lease_expires_at
// unchanged and persists nothing.
#[test]
fn t21_rejected_renewal_leaves_original_lease_unchanged() {
    let tmp = TempDir::new("ebl-t21");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-reject-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let binding = minimal_binding("binding-reject-001", "role-reject-001");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");

    repo.renew_executor_binding_lease(&trusted_clock(), "", RENEWED_LEASE)
        .expect_err("empty binding_id rejected");
    let too_long = "b".repeat(201);
    repo.renew_executor_binding_lease(&trusted_clock(), &too_long, RENEWED_LEASE)
        .expect_err("over-length binding_id rejected");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-reject-001", "")
        .expect_err("empty lease_expires_at rejected");
    repo.renew_executor_binding_lease(&trusted_clock(), "never-created", RENEWED_LEASE)
        .expect_err("unknown binding rejected");

    assert_eq!(
        repo.find_executor_binding("binding-reject-001")
            .expect("find")
            .expect("present"),
        binding,
        "every rejected renewal must leave the original lease value and all other fields unchanged"
    );
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "rejected renewals must persist nothing"
    );
}

// T22 — a released binding cannot be renewed: the renewal fails
// explicitly with the already-released error.
#[test]
fn t22_renewal_after_release_fails() {
    let tmp = TempDir::new("ebl-t22");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rel-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-rel-001", "role-rel-001"))
        .expect("binding create");
    repo.release_executor_binding("binding-rel-001", RELEASED_AT, ReleaseReason::HostSwitch)
        .expect("release");
    let error = repo
        .renew_executor_binding_lease(&trusted_clock(), "binding-rel-001", RENEWED_LEASE)
        .expect_err("renewing a released binding must fail explicitly");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingAlreadyReleased { binding_id }
                if binding_id == "binding-rel-001"
        ),
        "unexpected error: {error}"
    );
}

// T23 — the failed renewal-after-release preserves released_at unchanged.
#[test]
fn t23_renewal_after_release_preserves_released_at() {
    let tmp = TempDir::new("ebl-t23");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rel-002", LogicalRoleType::RuntimeA2))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-rel-002", "role-rel-002"))
        .expect("binding create");
    repo.release_executor_binding("binding-rel-002", RELEASED_AT, ReleaseReason::Crash)
        .expect("release");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-rel-002", RENEWED_LEASE)
        .expect_err("renewal after release must fail");
    assert_eq!(
        repo.find_executor_binding("binding-rel-002")
            .expect("find")
            .expect("present")
            .released_at,
        Some(RELEASED_AT.to_string()),
        "the recorded released_at must remain unchanged by the failed renewal"
    );
}

// T24 — the failed renewal-after-release preserves release_reason
// unchanged.
#[test]
fn t24_renewal_after_release_preserves_release_reason() {
    let tmp = TempDir::new("ebl-t24");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rel-003", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-rel-003", "role-rel-003"))
        .expect("binding create");
    repo.release_executor_binding("binding-rel-003", RELEASED_AT, ReleaseReason::UserRequest)
        .expect("release");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-rel-003", RENEWED_LEASE)
        .expect_err("renewal after release must fail");
    assert_eq!(
        repo.find_executor_binding("binding-rel-003")
            .expect("find")
            .expect("present")
            .release_reason,
        Some(ReleaseReason::UserRequest),
        "the recorded release_reason must remain unchanged by the failed renewal"
    );
}

// T25 — a binding released with LEASE_EXPIRED is still terminally
// released: it cannot be renewed.
#[test]
fn t25_lease_expired_released_binding_cannot_be_renewed() {
    let tmp = TempDir::new("ebl-t25");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-exp-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let binding = with_release(
        minimal_binding("binding-exp-001", "role-exp-001"),
        RELEASED_AT,
        ReleaseReason::LeaseExpired,
    );
    // Pre-existing LEASE_EXPIRED state is set up below the typed boundary:
    // the public create path refuses to manufacture new LEASE_EXPIRED
    // bindings, and only the trusted expiry transaction may write that
    // state, so this row stands in for storage this API never produced.
    repo.run_transaction(|uow| {
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
        uow.execute(
            "INSERT INTO executor_binding (
                binding_id, role_id, provider_id, model_id, runtime_id,
                session_ref, routing_decision_id, bound_at, lease_expires_at,
                released_at, release_reason, rehydration_completed
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            values,
        )
    })
    .expect("historical LEASE_EXPIRED row reaches storage");
    let error = repo
        .renew_executor_binding_lease(&trusted_clock(), "binding-exp-001", RENEWED_LEASE)
        .expect_err("a LEASE_EXPIRED release is still terminal and must refuse renewal");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingAlreadyReleased { binding_id }
                if binding_id == "binding-exp-001"
        ),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.find_executor_binding("binding-exp-001")
            .expect("find")
            .expect("present"),
        binding,
        "the terminal LEASE_EXPIRED evidence must remain exactly as recorded"
    );
}

// T26 — renewal before release: the release afterwards succeeds and the
// final record preserves the last renewed lease_expires_at alongside the
// terminal pair.
#[test]
fn t26_renewal_then_release_preserves_last_renewed_lease() {
    let tmp = TempDir::new("ebl-t26");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-seq-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    let binding = full_binding("binding-seq-001", "role-seq-001");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-seq-001", RENEWED_LEASE)
        .expect("first renew");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-seq-001", SECOND_RENEWED_LEASE)
        .expect("second renew");
    repo.release_executor_binding("binding-seq-001", RELEASED_AT, ReleaseReason::Completed)
        .expect("release after renewals");
    assert_eq!(
        repo.find_executor_binding("binding-seq-001")
            .expect("find")
            .expect("present"),
        with_release(
            with_renewed_lease(binding, SECOND_RENEWED_LEASE),
            RELEASED_AT,
            ReleaseReason::Completed
        ),
        "the released record must carry the last renewed lease value and exactly the terminal pair"
    );
}

// T27 — release remains write-once after a lease renewal: a second
// release after a renewal-and-release sequence fails and cannot overwrite
// the first terminal evidence.
#[test]
fn t27_release_remains_write_once_after_lease_renewal() {
    let tmp = TempDir::new("ebl-t27");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-once-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-once-001", "role-once-001"))
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-once-001", RENEWED_LEASE)
        .expect("renew");
    repo.release_executor_binding("binding-once-001", RELEASED_AT, ReleaseReason::Crash)
        .expect("first release after renewal");
    let error = repo
        .release_executor_binding(
            "binding-once-001",
            "2027-12-31T23:59:59.999Z",
            ReleaseReason::HostSwitch,
        )
        .expect_err("a second release must still fail after a lease renewal");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingAlreadyReleased { binding_id }
                if binding_id == "binding-once-001"
        ),
        "unexpected error: {error}"
    );
    let found = repo
        .find_executor_binding("binding-once-001")
        .expect("find")
        .expect("present");
    assert_eq!(found.released_at, Some(RELEASED_AT.to_string()));
    assert_eq!(found.release_reason, Some(ReleaseReason::Crash));
    assert_eq!(
        found.lease_expires_at,
        RENEWED_LEASE.to_string(),
        "the renewed lease value must survive the failed second release"
    );
}

// T28 — a failed renewal never clears terminal evidence: the full
// released record is byte-for-byte unchanged by the attempt.
#[test]
fn t28_renewal_never_clears_terminal_evidence() {
    let tmp = TempDir::new("ebl-t28");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-term-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let binding = full_binding("binding-term-001", "role-term-001");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    repo.release_executor_binding("binding-term-001", RELEASED_AT, ReleaseReason::ProviderDown)
        .expect("release");
    let released = with_release(binding, RELEASED_AT, ReleaseReason::ProviderDown);
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-term-001", RENEWED_LEASE)
        .expect_err("renewal of a released binding must fail");
    assert_eq!(
        repo.find_executor_binding("binding-term-001")
            .expect("find")
            .expect("present"),
        released,
        "the failed renewal must leave released_at, release_reason, and every other field intact"
    );
}

// T29 — behavioral guard evidence: persisted partial terminal shapes
// (exactly one terminal field non-NULL, reachable only through non-typed
// writers) occupy the eligibility slot, so renewal fails and the stored
// lease value is unchanged. Together with the source inspection of
// RENEW_LEASE_SQL (see the module documentation) this establishes that the
// durable update is constrained by released_at IS NULL AND release_reason
// IS NULL.
#[test]
fn t29_guarded_update_refuses_occupied_terminal_slot() {
    let tmp = TempDir::new("ebl-t29");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-guard-001", LogicalRoleType::RuntimeA1))
        .expect("role create");

    // Partial shape A: released_at recorded, release_reason NULL.
    repo.run_transaction(|uow| {
        let values: &[&dyn rusqlite::ToSql] = &[
            &"binding-guard-a",
            &"role-guard-001",
            &"p",
            &"m",
            &"r",
            &"2026-08-16T10:00:00.000Z",
            &"2026-08-16T11:00:00.000Z",
            &"2026-08-16T10:44:00.000Z",
        ];
        uow.execute(
            "INSERT INTO executor_binding (
                binding_id, role_id, provider_id, model_id, runtime_id,
                bound_at, lease_expires_at, released_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            values,
        )
    })
    .expect("partial shape A reaches storage through direct SQL");
    let error = repo
        .renew_executor_binding_lease(&trusted_clock(), "binding-guard-a", RENEWED_LEASE)
        .expect_err("a partially occupied terminal slot must refuse renewal");
    assert!(
        matches!(error, StateError::ExecutorBindingAlreadyReleased { .. }),
        "unexpected error: {error}"
    );

    // Partial shape B: release_reason recorded, released_at NULL. Since the
    // single-active-binding guard (migration 0005) makes each partial shape
    // uniqueness-blocking for its role, shape B probes from its own role.
    repo.create_logical_role(minimal_role("role-guard-002", LogicalRoleType::RuntimeA2))
        .expect("role create");
    repo.run_transaction(|uow| {
        let values: &[&dyn rusqlite::ToSql] = &[
            &"binding-guard-b",
            &"role-guard-002",
            &"p",
            &"m",
            &"r",
            &"2026-08-16T10:00:00.000Z",
            &"2026-08-16T11:00:00.000Z",
            &"HOST_SWITCH",
        ];
        uow.execute(
            "INSERT INTO executor_binding (
                binding_id, role_id, provider_id, model_id, runtime_id,
                bound_at, lease_expires_at, release_reason
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            values,
        )
    })
    .expect("partial shape B reaches storage through direct SQL");
    let error = repo
        .renew_executor_binding_lease(&trusted_clock(), "binding-guard-b", RENEWED_LEASE)
        .expect_err("a partially occupied terminal slot must refuse renewal");
    assert!(
        matches!(error, StateError::ExecutorBindingAlreadyReleased { .. }),
        "unexpected error: {error}"
    );

    for unchanged in ["binding-guard-a", "binding-guard-b"] {
        let stored: String = repo
            .connection()
            .query_row(
                "SELECT lease_expires_at FROM executor_binding WHERE binding_id = ?1",
                [unchanged],
                |row| row.get(0),
            )
            .expect("raw lease read");
        assert_eq!(
            stored, "2026-08-16T11:00:00.000Z",
            "the refused renewal must leave the stored lease value unchanged ({unchanged})"
        );
    }
}

// T30 — renewal is atomic: a forced transaction failure after the renewal
// write rolls the whole renewal back, leaving the prior lease value
// unchanged, durably; a committed transaction persists the new value.
#[test]
fn t30_forced_transaction_failure_leaves_prior_lease_unchanged() {
    let tmp = TempDir::new("ebl-t30");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-atomic-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-atomic-001", "role-atomic-001"))
        .expect("binding create");

    // The same in-transaction renewal write the public API performs, then
    // a forced failure so the transaction rolls back instead of committing.
    let error = repo
        .run_transaction(|uow| {
            apply_lease_renewal(uow.tx(), "binding-atomic-001", RENEWED_LEASE)?;
            Err::<(), StateError>(forced_failure())
        })
        .expect_err("forced failure must surface");
    assert!(
        matches!(error, StateError::UnitOfWorkFailed { .. }),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.find_executor_binding("binding-atomic-001")
            .expect("find")
            .expect("present")
            .lease_expires_at,
        "2026-08-16T11:00:00.000000000Z",
        "rollback must restore the prior lease value"
    );

    // The rollback is durable: the prior value survives close/reopen.
    drop(repo);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_executor_binding("binding-atomic-001")
            .expect("find")
            .expect("present")
            .lease_expires_at,
        "2026-08-16T11:00:00.000000000Z"
    );

    // Control: the same in-transaction write that is allowed to commit
    // persists the renewed value.
    repo.run_transaction(|uow| apply_lease_renewal(uow.tx(), "binding-atomic-001", RENEWED_LEASE))
        .expect("committed renewal persists");
    assert_eq!(
        repo.find_executor_binding("binding-atomic-001")
            .expect("find")
            .expect("present")
            .lease_expires_at,
        RENEWED_LEASE.to_string()
    );
}

// T31 — renewal cannot create another ExecutorBinding row: a successful
// renewal keeps exactly the one row, and a failed one adds nothing.
#[test]
fn t31_renewal_cannot_create_another_binding_row() {
    let tmp = TempDir::new("ebl-t31");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-count-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-count-001", "role-count-001"))
        .expect("binding create");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "exactly one binding row before renewal"
    );
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-count-001", RENEWED_LEASE)
        .expect("renew");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-count-001", SECOND_RENEWED_LEASE)
        .expect("renew again");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "a lease renewal operates on the existing row and must never add one"
    );
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-never-created", RENEWED_LEASE)
        .expect_err("unknown binding renewal fails");
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "a failed renewal must not create a row either"
    );
}

// T32 — renewal does not modify any field of the referenced LogicalRole,
// including active_binding_id.
#[test]
fn t32_renewal_does_not_mutate_logical_role() {
    let tmp = TempDir::new("ebl-t32");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut role = minimal_role("role-mut-001", LogicalRoleType::RuntimeA2);
    role.status = LogicalRoleStatus::Suspended;
    role.current_context_epoch = 7;
    role.name = Some("Role under renewal".to_string());
    role.workstream_id = Some("workstream-42".to_string());
    role.ownership_paths = vec!["receipts/one".to_string(), "receipts/two".to_string()];
    role.integration_branch = Some("integration/lease-1".to_string());
    role.context_manifest_id = Some("manifest-0199".to_string());
    role.active_binding_id = Some("binding-mut-001".to_string());
    role.created_at = Some("2026-08-01T08:00:00.000Z".to_string());
    repo.create_logical_role(role.clone()).expect("role create");
    repo.create_executor_binding(minimal_binding("binding-mut-001", "role-mut-001"))
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-mut-001", RENEWED_LEASE)
        .expect("renew");
    assert_eq!(
        repo.find_logical_role("role-mut-001").expect("find"),
        Some(role),
        "renewing a binding must leave every LogicalRole field untouched"
    );
}

// T33 — renewal does not mutate EventEnvelope/event rows.
#[test]
fn t33_renewal_does_not_mutate_event_rows() {
    let tmp = TempDir::new("ebl-t33");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-evt-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-evt-001", "role-evt-001"))
        .expect("binding create");
    let event = minimal_event("01ARZ3NDEKTSV4RRFFQ69G5FAK");
    repo.append_event(event.clone()).expect("append event");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-evt-001", RENEWED_LEASE)
        .expect("renew");
    assert_eq!(
        repo.find_event(&event.event_id).expect("find event"),
        Some(event),
        "renewing a binding must leave the appended event row untouched"
    );
    assert_eq!(
        repo.count_table_rows("event").expect("rows"),
        1,
        "renewal must neither append, alter, nor remove event rows"
    );
}

// T43 — compile-time evidence: the renewal capability has exactly the
// bounded public signature
// `fn(&mut SqliteStateRepository, &dyn TrustedClockV1, &str, &str) -> Result<(), StateError>`;
// there is no SQL string, connection, transaction, redaction, or
// arbitrary-execution parameter anywhere on the path. The function-pointer
// coercion below compiles only for that exact signature.
#[test]
fn t43_renewal_signature_is_exactly_bounded() {
    let renew: fn(
        &mut SqliteStateRepository,
        &dyn crate::trusted_time::TrustedClockV1,
        &str,
        &str,
    ) -> Result<(), StateError> = SqliteStateRepository::renew_executor_binding_lease;
    let tmp = TempDir::new("ebl-t43");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-sig-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-sig-001", "role-sig-001"))
        .expect("binding create");
    renew(
        &mut repo,
        &trusted_clock(),
        "binding-sig-001",
        RENEWED_LEASE,
    )
    .expect("renew through the exact signature");
    assert_eq!(
        repo.find_executor_binding("binding-sig-001")
            .expect("find")
            .expect("present")
            .lease_expires_at,
        RENEWED_LEASE.to_string()
    );
}

// T46 — all eight generic release reasons still round-trip exactly after
// renewal; generic LEASE_EXPIRED is rejected without changing the renewal.
#[test]
fn t46_eight_generic_release_reasons_round_trip_after_renewal() {
    let generic_reasons: [(ReleaseReason, &str); 8] = [
        (ReleaseReason::RateLimited, "RATE_LIMITED"),
        (ReleaseReason::SessionExhausted, "SESSION_EXHAUSTED"),
        (ReleaseReason::AuthRequired, "AUTH_REQUIRED"),
        (ReleaseReason::ProviderDown, "PROVIDER_DOWN"),
        (ReleaseReason::Crash, "CRASH"),
        (ReleaseReason::HostSwitch, "HOST_SWITCH"),
        (ReleaseReason::UserRequest, "USER_REQUEST"),
        (ReleaseReason::Completed, "COMPLETED"),
    ];
    let tmp = TempDir::new("ebl-t46");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-nine-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    for (index, (reason, durable)) in generic_reasons.iter().enumerate() {
        let binding_id = format!("binding-nine-{index:02}");
        let binding = minimal_binding(&binding_id, "role-nine-001");
        repo.create_executor_binding(binding.clone())
            .expect("binding create");
        repo.renew_executor_binding_lease(&trusted_clock(), &binding_id, RENEWED_LEASE)
            .expect("renew before release");
        repo.release_executor_binding(&binding_id, RELEASED_AT, *reason)
            .expect("each generic release reason must remain recordable");
        let found = repo
            .find_executor_binding(&binding_id)
            .expect("find")
            .expect("present");
        assert_eq!(found.release_reason, Some(*reason));
        assert_eq!(
            found.release_reason.map(ReleaseReason::as_str),
            Some(*durable)
        );
        assert_eq!(
            found,
            with_release(
                with_renewed_lease(binding, RENEWED_LEASE),
                RELEASED_AT,
                *reason
            )
        );
    }
    let binding = minimal_binding("binding-nine-08", "role-nine-001");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-nine-08", RENEWED_LEASE)
        .expect("renew before rejected expiry");
    assert!(matches!(
        repo.release_executor_binding("binding-nine-08", RELEASED_AT, ReleaseReason::LeaseExpired,),
        Err(StateError::ExecutorBindingValidation { .. })
    ));
    assert_eq!(
        repo.find_executor_binding("binding-nine-08")
            .expect("find")
            .expect("present"),
        with_renewed_lease(binding, RENEWED_LEASE)
    );
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        9,
        "eight renewed-and-released bindings plus one unchanged renewal"
    );
}

// T47 — corrupt persisted ExecutorBinding data fails closed: a corrupt
// rehydration_completed value makes the renewal refuse with a decode error,
// the stored lease value stays unchanged, and the corruption is never
// "repaired" by the renewal attempt.
#[test]
fn t47_corrupt_persisted_binding_fails_closed() {
    let tmp = TempDir::new("ebl-t47");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-corrupt-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-corrupt-001", "role-corrupt-001"))
        .expect("binding create");
    repo.run_transaction(|uow| {
        let values: &[&dyn rusqlite::ToSql] = &[&7i64];
        uow.execute(
            "UPDATE executor_binding SET rehydration_completed = ?1 WHERE binding_id = 'binding-corrupt-001'",
            values,
        )
    })
    .expect("corruption reaches storage through direct SQL");
    let error = repo
        .renew_executor_binding_lease(&trusted_clock(), "binding-corrupt-001", RENEWED_LEASE)
        .expect_err("a corrupt row must fail closed instead of being renewed");
    assert!(
        matches!(error, StateError::ExecutorBindingDecodeFailed { .. }),
        "unexpected error: {error}"
    );
    let stored: String = repo
        .connection()
        .query_row(
            "SELECT lease_expires_at FROM executor_binding WHERE binding_id = ?1",
            ["binding-corrupt-001"],
            |row| row.get(0),
        )
        .expect("raw lease read");
    assert_eq!(
        stored, "2026-08-16T11:00:00.000000000Z",
        "the refused renewal must leave the stored lease value unchanged"
    );
    let still_corrupt: i64 = repo
        .connection()
        .query_row(
            "SELECT rehydration_completed FROM executor_binding WHERE binding_id = ?1",
            ["binding-corrupt-001"],
            |row| row.get(0),
        )
        .expect("raw corruption read");
    assert_eq!(
        still_corrupt, 7,
        "the renewal attempt must not repair or rewrite the corrupt value"
    );
    assert!(
        matches!(
            repo.find_executor_binding("binding-corrupt-001"),
            Err(StateError::ExecutorBindingDecodeFailed { .. })
        ),
        "the corrupt row must still fail closed on decode after the refused renewal"
    );
}

// T48 — a canonical caller-provided deadline is persisted exactly; State
// validates but does not normalize or rewrite it.
#[test]
fn t48_caller_supplied_lease_value_persisted_exactly() {
    let tmp = TempDir::new("ebl-t48");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-opaque-001", LogicalRoleType::RuntimeA2))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-opaque-001", "role-opaque-001"))
        .expect("binding create");
    let canonical = "2026-08-16T14:00:00.000000001Z";
    repo.renew_executor_binding_lease(&trusted_clock(), "binding-opaque-001", canonical)
        .expect("renew with a canonical caller-supplied value");
    assert_eq!(
        repo.find_executor_binding("binding-opaque-001")
            .expect("find")
            .expect("present")
            .lease_expires_at,
        canonical.to_string(),
        "the renewed lease must round-trip byte-for-byte without normalization"
    );
    let stored: String = repo
        .connection()
        .query_row(
            "SELECT lease_expires_at FROM executor_binding WHERE binding_id = ?1",
            ["binding-opaque-001"],
            |row| row.get(0),
        )
        .expect("raw lease read");
    assert_eq!(
        stored, canonical,
        "the durable stored value must equal the supplied string exactly"
    );
}

// T34 — no wall-clock/time parsing/comparison dependency or helper is
// introduced.
//
// Source-inspection invariant: `src/state/**` contains no `chrono`, `time`,
// `Instant`, `SystemTime`, date parsing, UTC conversion, timezone logic,
// duration arithmetic, or timestamp comparison (the only `std::time` use
// is the pre-existing `Duration` busy_timeout constant in `repository.rs`,
// which is a SQLite milliseconds value, not a timestamp). The renewal
// stores the supplied string through one bound parameter and never reads
// the clock. No runtime test is fabricated for an absent dependency.

// T35 — no lease-expiry evaluation API is introduced.
//
// Compile-time/API-absence invariant: no `is_active`, `is_expired`,
// `lease_valid_now`, `lease_due`, `can_renew_now`, or similar semantic
// helper exists on any public type in this crate; `lease_expires_at`
// remains an opaque `String` on `ExecutorBinding` with no ordering or
// evaluation method.

// T36 — no automatic release or LEASE_EXPIRED transition is introduced.
//
// Source-inspection invariant: the renewal statement
// (`RENEW_LEASE_SQL` in `src/state/executor_binding.rs`) writes exactly
// one column (`lease_expires_at`); no code path in `src/state/**` writes
// `released_at`/`release_reason` other than the caller-driven
// `apply_release`, and nothing schedules or triggers it automatically.

// T37 — no heartbeat API/storage is introduced.
//
// Source-inspection invariant: no heartbeat field, column, table, method,
// or timer exists anywhere under `src/state/**` (see also T30 of the
// release slice: no `lease_scheduler_state`-style storage exists).

// T38 — no single-active-binding enforcement is introduced.
//
// Source-inspection invariant: no uniqueness index, active-binding lookup,
// or arbitration path exists; migration v0003 declares only the
// `binding_id` primary key and the `role_id` foreign key, and this slice
// adds no migration.

// T39 — no LogicalRole.active_binding_id mutation is introduced.
//
// Source-inspection invariant: the only LogicalRole write in this crate is
// `INSERT` on create (see `src/state/logical_role.rs`); the renewal path
// touches only the `executor_binding` table (T32 proves it behaviorally).

// T40 — no replacement/failover/rebind capability is introduced.
//
// Compile-time/API-absence invariant: no method named or behaving like
// replace/rebind/failover/substitute exists on any public type; the only
// binding mutations remain `release_executor_binding` and
// `renew_executor_binding_lease`, and renewal operates on the existing row
// (T31 proves it behaviorally).

// T41 — no routing/provider/model/runtime selection is introduced.
//
// Source-inspection invariant: `provider_id`, `model_id`, and `runtime_id`
// remain opaque persisted strings; no registry, eligibility, or selection
// code exists under `src/state/**`.

// T42 — no context rehydration/startup recovery behavior is introduced.
//
// Source-inspection invariant: `rehydration_completed` is read and written
// only as opaque persisted data; no rehydration, recovery, orphan
// detection, or startup sweep code exists under `src/state/**`.

// T44 — no unsafe Rust is introduced.
//
// Source-inspection invariant: `unsafe` appears nowhere under `src/state/**`.

// T45 — the strict A3-007 event payload/reference boundary remains
// unchanged.
//
// Source-inspection invariant: this slice does not touch
// `src/state/event.rs` or migration v0004; the event table remains the
// structural EventEnvelope shape with payload reference + digest only, and
// the event tests of the accepted baseline continue to pass unmodified.
