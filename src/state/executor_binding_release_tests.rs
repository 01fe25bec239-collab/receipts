//! Deterministic tests for write-once ExecutorBinding release persistence
//! (T01–T30 of the release slice).
//!
//! All tests use real temporary SQLite database files under the system
//! temporary directory (never inside the repository). Storage-level checks
//! that need SQL beyond the public repository API run through crate-private
//! helpers only and are `#[cfg(test)]`-gated.
//!
//! T25–T28 (no public binding delete / replacement / lease-renewal /
//! lease-expiry-evaluation API) are compile-time/API-absence invariants
//! established by inspecting the public surface of this crate, not runtime
//! tests; per the task rules no runtime tests are fabricated for them. They
//! are documented in the task handoff's COMPILE_TIME_API_INVARIANTS
//! section.

use crate::error::StateError;
use crate::executor_binding::{ExecutorBinding, ReleaseReason, apply_release};
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::migrations;
use crate::repository::SqliteStateRepository;
use crate::tests::TempDir;

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
        lease_expires_at: "2026-08-16T11:00:00.000Z".to_string(),
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
        lease_expires_at: "2026-08-16T10:30:00.000Z".to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: Some(false),
    }
}

/// The release timestamp used by the standard release path, supplied by the
/// caller — never generated, parsed, or compared by State.
const RELEASED_AT: &str = "2026-08-17T09:58:11.250Z";

/// A second, deliberately different timestamp for write-once probing.
const OTHER_RELEASED_AT: &str = "2027-12-31T23:59:59.999Z";

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

/// Creates a role and a fully populated binding, releases it, and returns
/// the live store plus the pre-release original and the post-release read
/// of the same binding.
fn setup_released_full_binding(
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
    repo.release_executor_binding("binding-full-001", RELEASED_AT, ReleaseReason::HostSwitch)
        .expect("release");
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

// T01 — an existing unreleased ExecutorBinding releases successfully, and
// the released record reads back as the original plus exactly the two
// terminal fields.
#[test]
fn t01_release_existing_unreleased_binding() {
    let tmp = TempDir::new("ebr-t01");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rel-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    let binding = minimal_binding("binding-rel-001", "role-rel-001");
    repo.create_executor_binding(binding.clone())
        .expect("binding create");
    repo.release_executor_binding("binding-rel-001", RELEASED_AT, ReleaseReason::HostSwitch)
        .expect("release an existing unreleased binding");
    assert_eq!(
        repo.find_executor_binding("binding-rel-001")
            .expect("find")
            .expect("present"),
        with_release(binding, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T02 — the released binding read returns exactly the supplied
// released_at string, stored as provided.
#[test]
fn t02_released_read_returns_supplied_released_at() {
    let tmp = TempDir::new("ebr-t02");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rel-002", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-rel-002", "role-rel-002"))
        .expect("binding create");
    repo.release_executor_binding(
        "binding-rel-002",
        "2026-08-16T09:58:11.250Z",
        ReleaseReason::Crash,
    )
    .expect("release");
    assert_eq!(
        repo.find_executor_binding("binding-rel-002")
            .expect("find")
            .expect("present")
            .released_at,
        Some("2026-08-16T09:58:11.250Z".to_string()),
        "released_at must round-trip as the exact supplied contract string"
    );
}

// T03 — the released binding read returns exactly the supplied
// release_reason enum value.
#[test]
fn t03_released_read_returns_supplied_release_reason() {
    let tmp = TempDir::new("ebr-t03");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-rel-003", LogicalRoleType::RuntimeA2))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-rel-003", "role-rel-003"))
        .expect("binding create");
    repo.release_executor_binding("binding-rel-003", RELEASED_AT, ReleaseReason::UserRequest)
        .expect("release");
    assert_eq!(
        repo.find_executor_binding("binding-rel-003")
            .expect("find")
            .expect("present")
            .release_reason,
        Some(ReleaseReason::UserRequest),
        "release_reason must round-trip as the exact supplied frozen value"
    );
}

// T04 — all nine frozen release reasons can be recorded and read back
// exactly, each with its exact durable representation.
#[test]
fn t04_all_nine_release_reasons_recorded_and_read_exactly() {
    let all_nine: [(ReleaseReason, &str); 9] = [
        (ReleaseReason::RateLimited, "RATE_LIMITED"),
        (ReleaseReason::SessionExhausted, "SESSION_EXHAUSTED"),
        (ReleaseReason::AuthRequired, "AUTH_REQUIRED"),
        (ReleaseReason::ProviderDown, "PROVIDER_DOWN"),
        (ReleaseReason::Crash, "CRASH"),
        (ReleaseReason::HostSwitch, "HOST_SWITCH"),
        (ReleaseReason::UserRequest, "USER_REQUEST"),
        (ReleaseReason::Completed, "COMPLETED"),
        (ReleaseReason::LeaseExpired, "LEASE_EXPIRED"),
    ];
    let tmp = TempDir::new("ebr-t04");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-nine-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    for (index, (reason, durable)) in all_nine.iter().enumerate() {
        let binding_id = format!("binding-nine-{index:02}");
        let binding = minimal_binding(&binding_id, "role-nine-001");
        repo.create_executor_binding(binding.clone())
            .expect("binding create");
        repo.release_executor_binding(&binding_id, RELEASED_AT, *reason)
            .expect("each frozen release reason must be recordable");
        let found = repo
            .find_executor_binding(&binding_id)
            .expect("find")
            .expect("present");
        assert_eq!(found.release_reason, Some(*reason));
        assert_eq!(
            found.release_reason.map(ReleaseReason::as_str),
            Some(*durable)
        );
        assert_eq!(found, with_release(binding, RELEASED_AT, *reason));
    }
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        9,
        "nine released bindings, one per frozen reason"
    );
}

// T05 — a recorded release survives database close and reopen exactly.
#[test]
fn t05_release_survives_close_and_reopen() {
    let tmp = TempDir::new("ebr-t05");
    let binding = minimal_binding("binding-durable-001", "role-durable-001");
    {
        let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
        repo.create_logical_role(minimal_role("role-durable-001", LogicalRoleType::RuntimeA1))
            .expect("role create");
        repo.create_executor_binding(binding.clone())
            .expect("binding create");
        repo.release_executor_binding(
            "binding-durable-001",
            RELEASED_AT,
            ReleaseReason::SessionExhausted,
        )
        .expect("release");
    }
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_executor_binding("binding-durable-001")
            .expect("find")
            .expect("present"),
        with_release(binding, RELEASED_AT, ReleaseReason::SessionExhausted),
        "the recorded release metadata must still be present exactly after reopen"
    );
}

// T06 — releasing an unknown binding fails deterministically with the
// unknown-identity error.
#[test]
fn t06_unknown_binding_release_fails_deterministically() {
    let tmp = TempDir::new("ebr-t06");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-unknown-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-unknown-001", "role-unknown-001"))
        .expect("binding create");
    let error = repo
        .release_executor_binding("never-created", RELEASED_AT, ReleaseReason::Crash)
        .expect_err("releasing an unknown binding must fail explicitly");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingNotFound { binding_id } if binding_id == "never-created"
        ),
        "unexpected error: {error}"
    );
}

// T07 — the failed unknown-binding release creates no binding and leaves
// no trace, durably.
#[test]
fn t07_unknown_binding_release_creates_no_binding() {
    let tmp = TempDir::new("ebr-t07");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-unknown-002", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-unknown-002", "role-unknown-002"))
        .expect("binding create");
    repo.release_executor_binding("binding-fabricated-001", RELEASED_AT, ReleaseReason::Crash)
        .expect_err("unknown binding release must fail");
    assert_eq!(
        repo.find_executor_binding("binding-fabricated-001")
            .expect("find"),
        None,
        "a failed release must not fabricate or implicitly create a binding"
    );
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "only the one real binding row may exist"
    );
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(
        repo.find_executor_binding("binding-fabricated-001")
            .expect("find"),
        None,
        "no fabricated binding may appear after reopen"
    );
}

// T08 — a second release of the same binding fails explicitly.
#[test]
fn t08_second_release_fails() {
    let tmp = TempDir::new("ebr-t08");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-twice-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-twice-001", "role-twice-001"))
        .expect("binding create");
    repo.release_executor_binding("binding-twice-001", RELEASED_AT, ReleaseReason::Crash)
        .expect("first release");
    let error = repo
        .release_executor_binding(
            "binding-twice-001",
            OTHER_RELEASED_AT,
            ReleaseReason::Completed,
        )
        .expect_err("a second release must fail explicitly, not act idempotently");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingAlreadyReleased { binding_id }
                if binding_id == "binding-twice-001"
        ),
        "unexpected error: {error}"
    );
}

// T09 — the failed second release cannot overwrite the recorded
// released_at.
#[test]
fn t09_second_release_cannot_overwrite_released_at() {
    let tmp = TempDir::new("ebr-t09");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-twice-002", LogicalRoleType::RuntimeA2))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-twice-002", "role-twice-002"))
        .expect("binding create");
    repo.release_executor_binding("binding-twice-002", RELEASED_AT, ReleaseReason::Crash)
        .expect("first release");
    repo.release_executor_binding("binding-twice-002", OTHER_RELEASED_AT, ReleaseReason::Crash)
        .expect_err("second release must fail");
    assert_eq!(
        repo.find_executor_binding("binding-twice-002")
            .expect("find")
            .expect("present")
            .released_at,
        Some(RELEASED_AT.to_string()),
        "the originally recorded released_at must remain unchanged"
    );
}

// T10 — the failed second release cannot overwrite the recorded
// release_reason.
#[test]
fn t10_second_release_cannot_overwrite_release_reason() {
    let tmp = TempDir::new("ebr-t10");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-twice-003", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-twice-003", "role-twice-003"))
        .expect("binding create");
    repo.release_executor_binding("binding-twice-003", RELEASED_AT, ReleaseReason::Crash)
        .expect("first release");
    repo.release_executor_binding("binding-twice-003", RELEASED_AT, ReleaseReason::HostSwitch)
        .expect_err("second release must fail");
    assert_eq!(
        repo.find_executor_binding("binding-twice-003")
            .expect("find")
            .expect("present")
            .release_reason,
        Some(ReleaseReason::Crash),
        "the originally recorded release_reason must remain unchanged"
    );
}

// T11 — after reopen, a failed second release still leaves the first
// release evidence exactly as originally recorded.
#[test]
fn t11_failed_second_release_preserves_first_evidence_after_reopen() {
    let tmp = TempDir::new("ebr-t11");
    let binding = minimal_binding("binding-evidence-001", "role-evidence-001");
    {
        let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
        repo.create_logical_role(minimal_role(
            "role-evidence-001",
            LogicalRoleType::RuntimeA1,
        ))
        .expect("role create");
        repo.create_executor_binding(binding.clone())
            .expect("binding create");
        repo.release_executor_binding(
            "binding-evidence-001",
            RELEASED_AT,
            ReleaseReason::ProviderDown,
        )
        .expect("first release");
    }
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    repo.release_executor_binding(
        "binding-evidence-001",
        OTHER_RELEASED_AT,
        ReleaseReason::UserRequest,
    )
    .expect_err("second release must fail after reopen");
    assert_eq!(
        repo.find_executor_binding("binding-evidence-001")
            .expect("find")
            .expect("present"),
        with_release(binding, RELEASED_AT, ReleaseReason::ProviderDown),
        "the first release evidence must be unchanged after the failed second attempt"
    );
}

// T12 — release does not modify binding_id.
#[test]
fn t12_release_does_not_modify_binding_id() {
    let (_tmp, _repo, original, found) = setup_released_full_binding("ebr-t12");
    assert_eq!(found.binding_id, original.binding_id);
    assert_eq!(
        found,
        with_release(original, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T13 — release does not modify role_id.
#[test]
fn t13_release_does_not_modify_role_id() {
    let (_tmp, _repo, original, found) = setup_released_full_binding("ebr-t13");
    assert_eq!(found.role_id, original.role_id);
    assert_eq!(
        found,
        with_release(original, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T14 — release does not modify provider_id.
#[test]
fn t14_release_does_not_modify_provider_id() {
    let (_tmp, _repo, original, found) = setup_released_full_binding("ebr-t14");
    assert_eq!(found.provider_id, original.provider_id);
    assert_eq!(
        found,
        with_release(original, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T15 — release does not modify model_id.
#[test]
fn t15_release_does_not_modify_model_id() {
    let (_tmp, _repo, original, found) = setup_released_full_binding("ebr-t15");
    assert_eq!(found.model_id, original.model_id);
    assert_eq!(
        found,
        with_release(original, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T16 — release does not modify runtime_id.
#[test]
fn t16_release_does_not_modify_runtime_id() {
    let (_tmp, _repo, original, found) = setup_released_full_binding("ebr-t16");
    assert_eq!(found.runtime_id, original.runtime_id);
    assert_eq!(
        found,
        with_release(original, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T17 — release does not modify session_ref.
#[test]
fn t17_release_does_not_modify_session_ref() {
    let (_tmp, _repo, original, found) = setup_released_full_binding("ebr-t17");
    assert_eq!(found.session_ref, original.session_ref);
    assert_eq!(
        found,
        with_release(original, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T18 — release does not modify routing_decision_id.
#[test]
fn t18_release_does_not_modify_routing_decision_id() {
    let (_tmp, _repo, original, found) = setup_released_full_binding("ebr-t18");
    assert_eq!(found.routing_decision_id, original.routing_decision_id);
    assert_eq!(
        found,
        with_release(original, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T19 — release does not modify bound_at.
#[test]
fn t19_release_does_not_modify_bound_at() {
    let (_tmp, _repo, original, found) = setup_released_full_binding("ebr-t19");
    assert_eq!(found.bound_at, original.bound_at);
    assert_eq!(
        found,
        with_release(original, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T20 — release does not modify lease_expires_at, and no lease-renewal
// behavior sneaks in through release.
#[test]
fn t20_release_does_not_modify_lease_expires_at() {
    let (_tmp, _repo, original, found) = setup_released_full_binding("ebr-t20");
    assert_eq!(found.lease_expires_at, original.lease_expires_at);
    assert_eq!(
        found,
        with_release(original, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T21 — release does not modify rehydration_completed, and no rehydration
// gating sneaks in through release.
#[test]
fn t21_release_does_not_modify_rehydration_completed() {
    let (_tmp, _repo, original, found) = setup_released_full_binding("ebr-t21");
    assert_eq!(found.rehydration_completed, original.rehydration_completed);
    assert_eq!(
        found,
        with_release(original, RELEASED_AT, ReleaseReason::HostSwitch)
    );
}

// T22 — release does not modify any field of the referenced LogicalRole.
#[test]
fn t22_release_does_not_modify_referenced_logical_role() {
    let tmp = TempDir::new("ebr-t22");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut role = minimal_role("role-mut-001", LogicalRoleType::RuntimeA2);
    role.status = LogicalRoleStatus::Suspended;
    role.current_context_epoch = 7;
    role.name = Some("Role under release".to_string());
    role.workstream_id = Some("workstream-42".to_string());
    role.ownership_paths = vec!["receipts/one".to_string(), "receipts/two".to_string()];
    role.integration_branch = Some("integration/release-1".to_string());
    role.context_manifest_id = Some("manifest-0199".to_string());
    role.active_binding_id = Some("binding-mut-001".to_string());
    role.created_at = Some("2026-08-01T08:00:00.000Z".to_string());
    repo.create_logical_role(role.clone()).expect("role create");
    repo.create_executor_binding(minimal_binding("binding-mut-001", "role-mut-001"))
        .expect("binding create");
    repo.release_executor_binding("binding-mut-001", RELEASED_AT, ReleaseReason::HostSwitch)
        .expect("release");
    assert_eq!(
        repo.find_logical_role("role-mut-001").expect("find"),
        Some(role),
        "releasing a binding must leave every LogicalRole field untouched"
    );
}

// T23 — release does not modify LogicalRole.active_binding_id: recording a
// release neither attaches nor detaches the role's active binding.
#[test]
fn t23_release_does_not_modify_logical_role_active_binding_id() {
    let tmp = TempDir::new("ebr-t23");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    let mut role = minimal_role("role-active-001", LogicalRoleType::RuntimeA1);
    role.active_binding_id = Some("binding-active-001".to_string());
    repo.create_logical_role(role.clone()).expect("role create");
    repo.create_executor_binding(minimal_binding("binding-active-001", "role-active-001"))
        .expect("binding create");
    repo.release_executor_binding("binding-active-001", RELEASED_AT, ReleaseReason::Completed)
        .expect("release");
    let found = repo
        .find_logical_role("role-active-001")
        .expect("find")
        .expect("present");
    assert_eq!(
        found.active_binding_id,
        Some("binding-active-001".to_string()),
        "release must not change LogicalRole.active_binding_id"
    );
    assert_eq!(found, role);
}

// T24 — release is atomic: a forced transaction failure after the release
// write rolls the whole release back, leaving both terminal fields
// unchanged, durably; a committed transaction persists both fields
// together.
#[test]
fn t24_forced_transaction_failure_leaves_release_unchanged() {
    let tmp = TempDir::new("ebr-t24");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-atomic-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-atomic-001", "role-atomic-001"))
        .expect("binding create");

    // The same in-transaction release write the public API performs, then
    // a forced failure so the transaction rolls back instead of committing.
    let error = repo
        .run_transaction(|uow| {
            apply_release(
                uow.tx(),
                "binding-atomic-001",
                RELEASED_AT,
                ReleaseReason::Crash,
            )?;
            Err::<(), StateError>(forced_failure())
        })
        .expect_err("forced failure must surface");
    assert!(
        matches!(error, StateError::UnitOfWorkFailed { .. }),
        "unexpected error: {error}"
    );
    let found = repo
        .find_executor_binding("binding-atomic-001")
        .expect("find")
        .expect("present");
    assert_eq!(found.released_at, None, "rollback must undo released_at");
    assert_eq!(
        found.release_reason, None,
        "rollback must undo release_reason"
    );

    // The rollback is durable: neither terminal field appears after
    // close/reopen.
    drop(repo);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    let found = repo
        .find_executor_binding("binding-atomic-001")
        .expect("find")
        .expect("present");
    assert_eq!(found.released_at, None);
    assert_eq!(found.release_reason, None);

    // Control: the same in-transaction write that is allowed to commit
    // persists both terminal fields together — never just one of them.
    repo.run_transaction(|uow| {
        apply_release(
            uow.tx(),
            "binding-atomic-001",
            RELEASED_AT,
            ReleaseReason::Crash,
        )
    })
    .expect("committed release persists");
    let found = repo
        .find_executor_binding("binding-atomic-001")
        .expect("find")
        .expect("present");
    assert_eq!(found.released_at, Some(RELEASED_AT.to_string()));
    assert_eq!(found.release_reason, Some(ReleaseReason::Crash));
}

// T25 — no public binding delete operation is introduced.
//
// Compile-time/API-absence invariant; no runtime test is fabricated. See
// the module documentation and the task handoff.

// T26 — no public binding replacement operation is introduced.
//
// Compile-time/API-absence invariant; no runtime test is fabricated. See
// the module documentation and the task handoff.

// T27 — no public lease-renewal operation is introduced.
//
// Compile-time/API-absence invariant; no runtime test is fabricated. See
// the module documentation and the task handoff.

// T28 — no public lease-expiry evaluation operation is introduced.
//
// Compile-time/API-absence invariant; no runtime test is fabricated. See
// the module documentation and the task handoff.

// T29 — the schema version remains exactly 3 before and after releases,
// with exactly one metadata row per applied migration and no migration 4.
#[test]
fn t29_schema_version_remains_exactly_3() {
    let tmp = TempDir::new("ebr-t29");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    assert_eq!(
        migrations::registered()
            .last()
            .expect("registered chain is non-empty")
            .version,
        3,
        "the registered chain itself must end at version 3"
    );
    assert_eq!(repo.schema_version().expect("version read"), 3);
    repo.create_logical_role(minimal_role("role-ver-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-ver-001", "role-ver-001"))
        .expect("binding create");
    repo.release_executor_binding("binding-ver-001", RELEASED_AT, ReleaseReason::LeaseExpired)
        .expect("release");
    assert_eq!(
        repo.schema_version().expect("version read"),
        3,
        "a release must not change the schema version"
    );
    assert_eq!(
        repo.count_table_rows("state_schema_version").expect("rows"),
        3,
        "no migration 4 metadata row may appear"
    );
    drop(repo);
    let repo = SqliteStateRepository::open(tmp.db_path()).expect("reopen");
    assert_eq!(repo.schema_version().expect("version read"), 3);
}

// T30 — this task introduces no new schema objects: after releases the
// database contains exactly the version-1/2/3 tables and none of the
// forbidden future storage.
#[test]
fn t30_no_new_schema_objects_introduced() {
    let tmp = TempDir::new("ebr-t30");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-obj-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-obj-001", "role-obj-001"))
        .expect("binding create");
    repo.release_executor_binding("binding-obj-001", RELEASED_AT, ReleaseReason::Crash)
        .expect("release");

    for expected in [
        "state_schema_version",
        "logical_role",
        "logical_role_ownership_path",
        "executor_binding",
    ] {
        assert!(
            repo.table_exists(expected).expect("table check"),
            "{expected} must exist"
        );
    }
    for forbidden in [
        "event_log",
        "context_manifest",
        "context_epoch",
        "entitlement",
        "graph",
        "graph_node",
        "graph_edge",
        "task",
        "task_attempt",
        "review",
        "finding",
        "evidence",
        "routing_decision",
        "provider",
        "model",
        "host_session",
        "workspace",
        "recovery",
        "lease_scheduler_state",
        "executor_binding_release",
        "executor_binding_release_history",
    ] {
        assert!(
            !repo.table_exists(forbidden).expect("table check"),
            "no {forbidden} storage may exist after a release"
        );
    }
}

// Extra — a binding created already-released (accepted historical import
// behavior) occupies its terminal slot: release refuses to touch it.
#[test]
fn t31_create_time_released_binding_cannot_be_released_again() {
    let tmp = TempDir::new("ebr-t31");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role(
        "role-imported-001",
        LogicalRoleType::RuntimeA1,
    ))
    .expect("role create");
    let imported = with_release(
        minimal_binding("binding-imported-001", "role-imported-001"),
        "2026-08-15T08:00:00.000Z",
        ReleaseReason::UserRequest,
    );
    repo.create_executor_binding(imported.clone())
        .expect("create-time released bindings remain persistable history");
    let error = repo
        .release_executor_binding("binding-imported-001", RELEASED_AT, ReleaseReason::Crash)
        .expect_err("the terminal slot is already occupied at create time");
    assert!(
        matches!(
            &error,
            StateError::ExecutorBindingAlreadyReleased { binding_id }
                if binding_id == "binding-imported-001"
        ),
        "unexpected error: {error}"
    );
    assert_eq!(
        repo.find_executor_binding("binding-imported-001")
            .expect("find"),
        Some(imported),
        "the imported release evidence must remain untouched"
    );
}

// Extra — a persisted partial terminal shape (exactly one of the two
// terminal fields non-NULL, reachable only through non-typed writers)
// occupies the write-once slot: release refuses to complete or overwrite
// it, and the partial evidence remains unchanged.
#[test]
fn t32_partial_terminal_shape_fails_closed() {
    let tmp = TempDir::new("ebr-t32");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-partial-001", LogicalRoleType::RuntimeA1))
        .expect("role create");

    // Partial shape A: released_at recorded, release_reason NULL.
    repo.run_transaction(|uow| {
        let values: &[&dyn rusqlite::ToSql] = &[
            &"binding-partial-a",
            &"role-partial-001",
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
        .release_executor_binding("binding-partial-a", RELEASED_AT, ReleaseReason::Crash)
        .expect_err("a partially occupied terminal slot must not be completed");
    assert!(
        matches!(error, StateError::ExecutorBindingAlreadyReleased { .. }),
        "unexpected error: {error}"
    );
    let found = repo
        .find_executor_binding("binding-partial-a")
        .expect("find")
        .expect("present");
    assert_eq!(
        found.released_at,
        Some("2026-08-16T10:44:00.000Z".to_string())
    );
    assert_eq!(found.release_reason, None);

    // Partial shape B: release_reason recorded, released_at NULL.
    repo.run_transaction(|uow| {
        let values: &[&dyn rusqlite::ToSql] = &[
            &"binding-partial-b",
            &"role-partial-001",
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
        .release_executor_binding("binding-partial-b", RELEASED_AT, ReleaseReason::Crash)
        .expect_err("a partially occupied terminal slot must not be overwritten");
    assert!(
        matches!(error, StateError::ExecutorBindingAlreadyReleased { .. }),
        "unexpected error: {error}"
    );
    let found = repo
        .find_executor_binding("binding-partial-b")
        .expect("find")
        .expect("present");
    assert_eq!(found.released_at, None);
    assert_eq!(found.release_reason, Some(ReleaseReason::HostSwitch));
}

// Extra — invalid release inputs fail closed before any storage access and
// persist nothing: empty binding_id, over-length binding_id, and empty
// released_at.
#[test]
fn t33_invalid_release_inputs_fail_closed_without_persistence() {
    let tmp = TempDir::new("ebr-t33");
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("bootstrap");
    repo.create_logical_role(minimal_role("role-input-001", LogicalRoleType::RuntimeA1))
        .expect("role create");
    repo.create_executor_binding(minimal_binding("binding-input-001", "role-input-001"))
        .expect("binding create");

    let error = repo
        .release_executor_binding("", RELEASED_AT, ReleaseReason::Crash)
        .expect_err("an empty binding_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );

    let too_long = "b".repeat(201);
    let error = repo
        .release_executor_binding(&too_long, RELEASED_AT, ReleaseReason::Crash)
        .expect_err("an over-length binding_id must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );

    let error = repo
        .release_executor_binding("binding-input-001", "", ReleaseReason::Crash)
        .expect_err("an empty released_at must be rejected");
    assert!(
        matches!(error, StateError::ExecutorBindingValidation { .. }),
        "unexpected error: {error}"
    );

    let found = repo
        .find_executor_binding("binding-input-001")
        .expect("find")
        .expect("present");
    assert_eq!(found.released_at, None);
    assert_eq!(found.release_reason, None);
    assert_eq!(
        repo.count_table_rows("executor_binding").expect("rows"),
        1,
        "rejected releases must persist nothing"
    );
}
