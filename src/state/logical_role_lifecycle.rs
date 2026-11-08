//! Atomic LogicalRole status, binding-pointer and context changes (migration 17).
//!
//! Each change is an explicit expected-current compare-and-set. The role's
//! current pointer columns and one immutable `logical_role_history` row
//! commit in the same serialized transaction or not at all; storage triggers
//! refuse a pointer update without that matching history row. Role identity,
//! project, type, ownership and executor-binding rows are never changed.
//! State does not run failover, rehydration or binding release here, and the
//! legacy create/release APIs gain no hidden pointer effects.

use rusqlite::{Connection, OptionalExtension, params};

use crate::canonical_timestamp::CanonicalTimestampV1;
use crate::epoch_value::StateEpochValueV1;
use crate::error::StateError;
use crate::executor_binding::read_executor_binding;
use crate::logical_role::{LogicalRole, LogicalRoleStatus, ensure_identifier, read_logical_role};
use crate::repository::SqliteStateRepository;

/// Closed, content-free reason a role change was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleChangeRefusal {
    /// The role does not exist.
    RoleNotFound,
    /// The supplied expected-current value differs from durable state.
    StaleExpectation,
    /// The requested value equals the current value.
    NoChange,
    /// The status pair is not ACTIVE↔SUSPENDED or ACTIVE/SUSPENDED→RETIRED.
    IllegalStatusTransition,
    /// RETIRED is terminal; no further change is accepted.
    RoleRetired,
    /// Retirement requested while a binding pointer or unreleased binding remains.
    RoleHasBinding,
    /// The newly selected binding does not exist.
    BindingNotFound,
    /// A referenced binding belongs to another role.
    BindingRoleMismatch,
    /// The newly selected binding is released or has a partial terminal pair.
    BindingNotUnreleased,
    /// The current pointer names a binding that is not conclusively released.
    CurrentBindingUnreleased,
    /// The current pointer names a binding that does not exist.
    CurrentBindingDangling,
    /// The context epoch would move backwards.
    EpochRegression,
    /// The new epoch has no project ContextEpoch history row.
    EpochUnknown,
    /// The supplied manifest does not exist.
    ManifestNotFound,
    /// The manifest's role, project or epoch differs from the change.
    ManifestMismatch,
    /// An existing manifest pointer cannot be cleared.
    ManifestClear,
}

impl RoleChangeRefusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RoleNotFound => "ROLE_NOT_FOUND",
            Self::StaleExpectation => "STALE_EXPECTATION",
            Self::NoChange => "NO_CHANGE",
            Self::IllegalStatusTransition => "ILLEGAL_STATUS_TRANSITION",
            Self::RoleRetired => "ROLE_RETIRED",
            Self::RoleHasBinding => "ROLE_HAS_BINDING",
            Self::BindingNotFound => "BINDING_NOT_FOUND",
            Self::BindingRoleMismatch => "BINDING_ROLE_MISMATCH",
            Self::BindingNotUnreleased => "BINDING_NOT_UNRELEASED",
            Self::CurrentBindingUnreleased => "CURRENT_BINDING_UNRELEASED",
            Self::CurrentBindingDangling => "CURRENT_BINDING_DANGLING",
            Self::EpochRegression => "EPOCH_REGRESSION",
            Self::EpochUnknown => "EPOCH_UNKNOWN",
            Self::ManifestNotFound => "MANIFEST_NOT_FOUND",
            Self::ManifestMismatch => "MANIFEST_MISMATCH",
            Self::ManifestClear => "MANIFEST_CLEAR",
        }
    }
}

/// Which pointer family one history row changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleChangeKind {
    Status,
    Binding,
    Context,
}

impl RoleChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Status => "STATUS",
            Self::Binding => "BINDING",
            Self::Context => "CONTEXT",
        }
    }
    fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "STATUS" => Ok(Self::Status),
            "BINDING" => Ok(Self::Binding),
            "CONTEXT" => Ok(Self::Context),
            _ => Err(decode_failure("unknown role change_kind")),
        }
    }
}

/// Common evidence for one change: canonical time and optional opaque reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleChangeEvidenceV1 {
    pub changed_at: CanonicalTimestampV1,
    /// Opaque, non-empty when present. Never a prompt, output or credential.
    pub reason: Option<String>,
}

/// One immutable before/after snapshot of the role's pointer columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogicalRoleHistoryEntryV1 {
    pub role_id: String,
    pub project_id: String,
    pub sequence: i64,
    pub change_kind: RoleChangeKind,
    pub from_status: LogicalRoleStatus,
    pub to_status: LogicalRoleStatus,
    pub from_binding_id: Option<String>,
    pub to_binding_id: Option<String>,
    pub from_epoch: StateEpochValueV1,
    pub to_epoch: StateEpochValueV1,
    pub from_manifest_id: Option<String>,
    pub to_manifest_id: Option<String>,
    pub changed_at: String,
    pub reason: Option<String>,
}

struct Pointers {
    status: LogicalRoleStatus,
    binding: Option<String>,
    epoch: StateEpochValueV1,
    manifest: Option<String>,
}

impl SqliteStateRepository {
    /// Moves status ACTIVE↔SUSPENDED or ACTIVE/SUSPENDED→RETIRED.
    pub(crate) fn transition_logical_role_status(
        &mut self,
        role_id: &str,
        expected: LogicalRoleStatus,
        new: LogicalRoleStatus,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), StateError> {
        validate_input(role_id, evidence)?;
        self.run_serialized_transaction(|uow| {
            let conn = uow.tx();
            let role = current_role(conn, role_id, expected)?;
            if new == expected {
                return Err(refused(role_id, RoleChangeRefusal::NoChange));
            }
            use LogicalRoleStatus::{Active, Retired, Suspended};
            if !matches!(
                (expected, new),
                (Active, Suspended) | (Suspended, Active) | (Active | Suspended, Retired)
            ) {
                return Err(refused(role_id, RoleChangeRefusal::IllegalStatusTransition));
            }
            if new == Retired
                && (role.active_binding_id.is_some() || has_unreleased_binding(conn, role_id)?)
            {
                return Err(refused(role_id, RoleChangeRefusal::RoleHasBinding));
            }
            let mut next = pointers(&role);
            next.status = new;
            apply(conn, &role, RoleChangeKind::Status, next, evidence)
        })
    }

    /// Replaces or clears the active-binding pointer by expected-current CAS.
    ///
    /// A new binding must exist, belong to this role and have an empty
    /// terminal pair. A replaced or cleared current binding must exist,
    /// belong to this role and be conclusively released (both terminal
    /// fields recorded). Nothing is released, created or repaired here.
    pub(crate) fn set_logical_role_active_binding(
        &mut self,
        role_id: &str,
        expected: Option<&str>,
        new: Option<&str>,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), StateError> {
        validate_input(role_id, evidence)?;
        for id in [expected, new].into_iter().flatten() {
            ensure_identifier("binding_id", id)?;
        }
        self.run_serialized_transaction(|uow| {
            let conn = uow.tx();
            let role = current_role_any(conn, role_id)?;
            if role.active_binding_id.as_deref() != expected {
                return Err(refused(role_id, RoleChangeRefusal::StaleExpectation));
            }
            if role.status == LogicalRoleStatus::Retired {
                return Err(refused(role_id, RoleChangeRefusal::RoleRetired));
            }
            if new == expected {
                return Err(refused(role_id, RoleChangeRefusal::NoChange));
            }
            if let Some(current) = expected {
                let binding = read_executor_binding(conn, current)?
                    .ok_or_else(|| refused(role_id, RoleChangeRefusal::CurrentBindingDangling))?;
                if binding.role_id != role_id {
                    return Err(refused(role_id, RoleChangeRefusal::BindingRoleMismatch));
                }
                if binding.released_at.is_none() || binding.release_reason.is_none() {
                    return Err(refused(
                        role_id,
                        RoleChangeRefusal::CurrentBindingUnreleased,
                    ));
                }
            }
            if let Some(selected) = new {
                let binding = read_executor_binding(conn, selected)?
                    .ok_or_else(|| refused(role_id, RoleChangeRefusal::BindingNotFound))?;
                if binding.role_id != role_id {
                    return Err(refused(role_id, RoleChangeRefusal::BindingRoleMismatch));
                }
                if binding.released_at.is_some() || binding.release_reason.is_some() {
                    return Err(refused(role_id, RoleChangeRefusal::BindingNotUnreleased));
                }
            }
            let mut next = pointers(&role);
            next.binding = new.map(str::to_string);
            apply(conn, &role, RoleChangeKind::Binding, next, evidence)
        })
    }

    /// Advances the role's context epoch and/or sets its manifest by CAS.
    ///
    /// The epoch never regresses; a changed epoch must exist in the role
    /// project's ContextEpoch history. A supplied manifest must exist and
    /// match this role, project and the new epoch. A set manifest pointer
    /// cannot be cleared.
    pub(crate) fn set_logical_role_context(
        &mut self,
        role_id: &str,
        expected_epoch: &StateEpochValueV1,
        expected_manifest_id: Option<&str>,
        new_epoch: &StateEpochValueV1,
        new_manifest_id: Option<&str>,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), StateError> {
        validate_input(role_id, evidence)?;
        for id in [expected_manifest_id, new_manifest_id]
            .into_iter()
            .flatten()
        {
            ensure_identifier("context_manifest_id", id)?;
        }
        self.run_serialized_transaction(|uow| {
            let conn = uow.tx();
            change_context(
                conn,
                role_id,
                expected_epoch,
                expected_manifest_id,
                new_epoch,
                new_manifest_id,
                evidence,
            )
        })
    }

    /// Creates an immutable successor and switches the attached manifest by CAS.
    /// The snapshot, ordered children, role pointer and history commit together.
    /// The predecessor must be attached; same-epoch replacement is permitted.
    pub(crate) fn supersede_context_manifest(
        &mut self,
        role_id: &str,
        expected_epoch: &StateEpochValueV1,
        expected_manifest_id: &str,
        manifest: crate::ContextManifest,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), StateError> {
        validate_input(role_id, evidence)?;
        ensure_identifier("context_manifest_id", expected_manifest_id)?;
        crate::context_manifest::validate_for_create(&manifest)?;
        self.run_serialized_transaction(|uow| {
            let conn = uow.tx();
            let role = current_role_any(conn, role_id)?;
            if &role.current_context_epoch != expected_epoch
                || role.context_manifest_id.as_deref() != Some(expected_manifest_id)
            {
                return Err(refused(role_id, RoleChangeRefusal::StaleExpectation));
            }
            let predecessor = manifest_identity(conn, expected_manifest_id)?
                .ok_or_else(|| refused(role_id, RoleChangeRefusal::ManifestNotFound))?;
            if predecessor
                != (
                    role.role_id.clone(),
                    role.project_id.clone(),
                    expected_epoch.clone(),
                )
                || manifest.role_id != role_id
                || manifest.project_id != role.project_id
            {
                return Err(refused(role_id, RoleChangeRefusal::ManifestMismatch));
            }
            crate::context_manifest::insert_context_manifest(conn, &manifest, false)?;
            change_context(
                conn,
                role_id,
                expected_epoch,
                Some(expected_manifest_id),
                &manifest.epoch,
                Some(&manifest.manifest_id),
                evidence,
            )
        })
    }

    /// Lists one role's immutable change history in sequence order.
    pub fn list_logical_role_history(
        &self,
        role_id: &str,
    ) -> Result<Vec<LogicalRoleHistoryEntryV1>, StateError> {
        ensure_identifier("role_id", role_id)?;
        let tx = self
            .connection()
            .unchecked_transaction()
            .map_err(query_failure)?;
        let entries = {
            let mut statement = tx
                .prepare(
                    "SELECT role_id,project_id,sequence,change_kind,from_status,to_status,from_binding_id,to_binding_id,from_epoch,to_epoch,from_manifest_id,to_manifest_id,changed_at,reason FROM logical_role_history WHERE role_id=?1 ORDER BY sequence",
                )
                .map_err(query_failure)?;
            let rows = statement
                .query_map([role_id], |r| {
                    Ok((
                        (
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, Option<String>>(6)?,
                        ),
                        (
                            r.get::<_, Option<String>>(7)?,
                            r.get::<_, String>(8)?,
                            r.get::<_, String>(9)?,
                            r.get::<_, Option<String>>(10)?,
                            r.get::<_, Option<String>>(11)?,
                            r.get::<_, String>(12)?,
                            r.get::<_, Option<String>>(13)?,
                        ),
                    ))
                })
                .map_err(query_failure)?;
            rows.map(|row| {
                let (
                    (role_id, project_id, sequence, kind, fs, ts, fb),
                    (tb, fe, te, fm, tm, at, reason),
                ) = row.map_err(query_failure)?;
                let epoch = |v: String| {
                    StateEpochValueV1::try_from(v).map_err(|_| decode_failure("history epoch"))
                };
                Ok(LogicalRoleHistoryEntryV1 {
                    role_id,
                    project_id,
                    sequence,
                    change_kind: RoleChangeKind::from_storage(&kind)?,
                    from_status: LogicalRoleStatus::from_storage(&fs)?,
                    to_status: LogicalRoleStatus::from_storage(&ts)?,
                    from_binding_id: fb,
                    to_binding_id: tb,
                    from_epoch: epoch(fe)?,
                    to_epoch: epoch(te)?,
                    from_manifest_id: fm,
                    to_manifest_id: tm,
                    changed_at: at,
                    reason,
                })
            })
            .collect::<Result<Vec<_>, StateError>>()?
        };
        tx.commit().map_err(query_failure)?;
        Ok(entries)
    }
}

fn validate_input(role_id: &str, evidence: &RoleChangeEvidenceV1) -> Result<(), StateError> {
    ensure_identifier("role_id", role_id)?;
    if evidence.reason.as_deref().is_some_and(str::is_empty) {
        return Err(StateError::LogicalRoleValidation {
            detail: "reason must not be empty when present".to_string(),
        });
    }
    Ok(())
}

fn current_role_any(conn: &Connection, role_id: &str) -> Result<LogicalRole, StateError> {
    read_logical_role(conn, role_id)?
        .ok_or_else(|| refused(role_id, RoleChangeRefusal::RoleNotFound))
}

fn current_role(
    conn: &Connection,
    role_id: &str,
    expected: LogicalRoleStatus,
) -> Result<LogicalRole, StateError> {
    let role = current_role_any(conn, role_id)?;
    if role.status != expected {
        return Err(refused(role_id, RoleChangeRefusal::StaleExpectation));
    }
    if role.status == LogicalRoleStatus::Retired {
        return Err(refused(role_id, RoleChangeRefusal::RoleRetired));
    }
    Ok(role)
}

fn pointers(role: &LogicalRole) -> Pointers {
    Pointers {
        status: role.status,
        binding: role.active_binding_id.clone(),
        epoch: role.current_context_epoch.clone(),
        manifest: role.context_manifest_id.clone(),
    }
}

fn has_unreleased_binding(conn: &Connection, role_id: &str) -> Result<bool, StateError> {
    conn.query_row(
        "SELECT 1 FROM executor_binding WHERE role_id=?1 AND (released_at IS NULL OR release_reason IS NULL) LIMIT 1",
        [role_id],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
    .map_err(query_failure)
}

fn epoch_exists(
    conn: &Connection,
    project_id: &str,
    epoch: &StateEpochValueV1,
) -> Result<bool, StateError> {
    conn.query_row(
        "SELECT 1 FROM context_epoch WHERE project_id=?1 AND epoch=?2",
        params![project_id, epoch.as_str()],
        |_| Ok(()),
    )
    .optional()
    .map(|found| found.is_some())
    .map_err(query_failure)
}

fn manifest_identity(
    conn: &Connection,
    manifest_id: &str,
) -> Result<Option<(String, String, StateEpochValueV1)>, StateError> {
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT role_id, project_id, epoch FROM context_manifest WHERE manifest_id=?1",
            [manifest_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(query_failure)?;
    row.map(|(role, project, epoch)| {
        StateEpochValueV1::try_from(epoch)
            .map(|epoch| (role, project, epoch))
            .map_err(|_| decode_failure("manifest epoch"))
    })
    .transpose()
}

/// Appends the history row first (its trigger checks it starts from the
/// current row), then CAS-updates the pointers (its trigger checks the
/// matching fresh history row). Any failure rolls back both.
fn apply(
    conn: &Connection,
    role: &LogicalRole,
    kind: RoleChangeKind,
    next: Pointers,
    evidence: &RoleChangeEvidenceV1,
) -> Result<(), StateError> {
    conn.execute(
        "INSERT INTO logical_role_history(role_id,project_id,sequence,change_kind,from_status,to_status,from_binding_id,to_binding_id,from_epoch,to_epoch,from_manifest_id,to_manifest_id,changed_at,reason) SELECT ?1,?2,COALESCE(MAX(sequence),0)+1,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13 FROM logical_role_history WHERE role_id=?1",
        params![
            role.role_id,
            role.project_id,
            kind.as_str(),
            role.status.as_str(),
            next.status.as_str(),
            role.active_binding_id,
            next.binding,
            role.current_context_epoch.as_str(),
            next.epoch.as_str(),
            role.context_manifest_id,
            next.manifest,
            evidence.changed_at.as_str(),
            evidence.reason,
        ],
    )
    .map_err(write_failure)?;
    let changed = conn
        .execute(
            "UPDATE logical_role SET status=?1, active_binding_id=?2, current_context_epoch=?3, context_manifest_id=?4 WHERE role_id=?5 AND status=?6 AND active_binding_id IS ?7 AND current_context_epoch=?8 AND context_manifest_id IS ?9",
            params![
                next.status.as_str(),
                next.binding,
                next.epoch.as_str(),
                next.manifest,
                role.role_id,
                role.status.as_str(),
                role.active_binding_id,
                role.current_context_epoch.as_str(),
                role.context_manifest_id,
            ],
        )
        .map_err(write_failure)?;
    if changed != 1 {
        return Err(refused(&role.role_id, RoleChangeRefusal::StaleExpectation));
    }
    Ok(())
}

fn refused(role_id: &str, reason: RoleChangeRefusal) -> StateError {
    StateError::LogicalRoleChangeRefused {
        role_id: role_id.to_string(),
        reason,
    }
}

fn decode_failure(detail: &str) -> StateError {
    StateError::LogicalRoleDecodeFailed {
        detail: detail.to_string(),
    }
}

fn write_failure(error: rusqlite::Error) -> StateError {
    StateError::LogicalRoleWriteFailed {
        detail: error.to_string(),
    }
}

fn query_failure(error: rusqlite::Error) -> StateError {
    StateError::InternalQueryFailed {
        detail: error.to_string(),
    }
}

fn change_context(
    conn: &Connection,
    role_id: &str,
    expected_epoch: &StateEpochValueV1,
    expected_manifest_id: Option<&str>,
    new_epoch: &StateEpochValueV1,
    new_manifest_id: Option<&str>,
    evidence: &RoleChangeEvidenceV1,
) -> Result<(), StateError> {
    let role = current_role_any(conn, role_id)?;
    if &role.current_context_epoch != expected_epoch
        || role.context_manifest_id.as_deref() != expected_manifest_id
    {
        return Err(refused(role_id, RoleChangeRefusal::StaleExpectation));
    }
    if role.status == LogicalRoleStatus::Retired {
        return Err(refused(role_id, RoleChangeRefusal::RoleRetired));
    }
    if new_epoch == expected_epoch && new_manifest_id == expected_manifest_id {
        return Err(refused(role_id, RoleChangeRefusal::NoChange));
    }
    if new_epoch < expected_epoch {
        return Err(refused(role_id, RoleChangeRefusal::EpochRegression));
    }
    if expected_manifest_id.is_some() && new_manifest_id.is_none() {
        return Err(refused(role_id, RoleChangeRefusal::ManifestClear));
    }
    if new_epoch != expected_epoch && !epoch_exists(conn, &role.project_id, new_epoch)? {
        return Err(refused(role_id, RoleChangeRefusal::EpochUnknown));
    }
    if let Some(manifest_id) = new_manifest_id {
        let (owner, project, epoch) = manifest_identity(conn, manifest_id)?
            .ok_or_else(|| refused(role_id, RoleChangeRefusal::ManifestNotFound))?;
        if owner != role_id || project != role.project_id || &epoch != new_epoch {
            return Err(refused(role_id, RoleChangeRefusal::ManifestMismatch));
        }
    }
    let mut next = pointers(&role);
    next.epoch = new_epoch.clone();
    next.manifest = new_manifest_id.map(str::to_string);
    apply(conn, &role, RoleChangeKind::Context, next, evidence)
}
