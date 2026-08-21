//! Durable LogicalRole identity persistence (migration 0002 slice).
//!
//! [`LogicalRole`] is the frozen durable-identity contract for orchestration
//! roles. A LogicalRole is durable identity only and is explicitly **not**
//! an LLM session. Only `RUNTIME_A1` and `RUNTIME_A2` may hold durable role
//! identities; `RUNTIME_A3` and `RUNTIME_A4` are ephemeral by contract and
//! are not representable in [`LogicalRoleType`].
//!
//! Scope of this slice: create + durable read. There is deliberately no
//! public mutation path (status, context epoch, active binding, retirement)
//! and no deletion path. `context_manifest_id` and `active_binding_id` are
//! stored as opaque nullable references: State does not create, validate,
//! dereference, or assign behavior to ContextManifest or ExecutorBinding.
//!
//! State persists LogicalRole identity only; it does not decide when roles
//! are created or how orchestration uses them.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::StateError;
use crate::repository::{SqliteStateRepository, UnitOfWork};

/// Maximum allowed length, in Unicode scalar values, of every constrained
/// LogicalRole identifier (frozen contract: 200).
pub const MAX_IDENTIFIER_LENGTH: usize = 200;

/// Durable role type (frozen contract: `RUNTIME_A1` or `RUNTIME_A2`).
///
/// Ephemeral runtimes (A3/A4) must never receive durable LogicalRole
/// identities and therefore have no variant here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalRoleType {
    /// `RUNTIME_A1`.
    RuntimeA1,
    /// `RUNTIME_A2`.
    RuntimeA2,
}

impl LogicalRoleType {
    /// The durable storage representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            LogicalRoleType::RuntimeA1 => "RUNTIME_A1",
            LogicalRoleType::RuntimeA2 => "RUNTIME_A2",
        }
    }

    /// Decodes the durable representation, failing closed on anything other
    /// than the two durable role types.
    pub(crate) fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "RUNTIME_A1" => Ok(LogicalRoleType::RuntimeA1),
            "RUNTIME_A2" => Ok(LogicalRoleType::RuntimeA2),
            other => Err(StateError::LogicalRoleDecodeFailed {
                detail: format!(
                    "unknown durable role_type {other:?}: only RUNTIME_A1 and RUNTIME_A2 may hold durable identities"
                ),
            }),
        }
    }
}

/// Durable role status (frozen contract: `ACTIVE`, `SUSPENDED`, `RETIRED`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogicalRoleStatus {
    /// `ACTIVE`.
    Active,
    /// `SUSPENDED`.
    Suspended,
    /// `RETIRED`.
    Retired,
}

impl LogicalRoleStatus {
    /// The durable storage representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            LogicalRoleStatus::Active => "ACTIVE",
            LogicalRoleStatus::Suspended => "SUSPENDED",
            LogicalRoleStatus::Retired => "RETIRED",
        }
    }

    /// Decodes the durable representation, failing closed on anything other
    /// than the three contract statuses.
    pub(crate) fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "ACTIVE" => Ok(LogicalRoleStatus::Active),
            "SUSPENDED" => Ok(LogicalRoleStatus::Suspended),
            "RETIRED" => Ok(LogicalRoleStatus::Retired),
            other => Err(StateError::LogicalRoleDecodeFailed {
                detail: format!("unknown LogicalRole status {other:?}"),
            }),
        }
    }
}

/// A durable LogicalRole identity (frozen contract).
///
/// Required fields are non-nullable; optional fields are nullable exactly as
/// the contract lists them. `ownership_paths` is the ordered path list: an
/// empty vector means "no ownership paths", and the stored order round-trips
/// exactly (duplicates included).
#[derive(Debug, Clone, PartialEq)]
pub struct LogicalRole {
    /// Durable role identity. Non-empty, at most
    /// [`MAX_IDENTIFIER_LENGTH`] scalar values.
    pub role_id: String,
    /// Owning project. Non-empty, at most [`MAX_IDENTIFIER_LENGTH`] scalar
    /// values.
    pub project_id: String,
    /// Durable role type. Only A1/A2 can be durable.
    pub role_type: LogicalRoleType,
    /// Lifecycle status.
    pub status: LogicalRoleStatus,
    /// Current context epoch. Must be >= 0.
    pub current_context_epoch: i64,
    /// Optional human-readable name. Stored as provided.
    pub name: Option<String>,
    /// Optional workstream. When present: non-empty, at most
    /// [`MAX_IDENTIFIER_LENGTH`] scalar values.
    pub workstream_id: Option<String>,
    /// Ordered durable ownership paths. Absence is represented by an empty
    /// vector; order and duplicates round-trip exactly.
    pub ownership_paths: Vec<String>,
    /// Optional integration branch name. Stored as provided.
    pub integration_branch: Option<String>,
    /// Opaque nullable reference to a ContextManifest. Never created,
    /// validated, dereferenced, or mutated by State.
    pub context_manifest_id: Option<String>,
    /// Opaque nullable reference to an ExecutorBinding. Never created,
    /// validated, dereferenced, or mutated by State.
    pub active_binding_id: Option<String>,
    /// Optional creation timestamp, opaque to State and stored as provided.
    pub created_at: Option<String>,
}

impl SqliteStateRepository {
    /// Durably creates a new LogicalRole.
    ///
    /// Creation is atomic: the role row and its ownership-path rows commit
    /// together or not at all. A `role_id` that already exists fails
    /// explicitly with [`StateError::LogicalRoleAlreadyExists`]; this
    /// repository never overwrites, replaces, upserts, or merges durable
    /// role identities. Contract-level validation runs before any storage
    /// access, and the schema re-enforces the same constraints as a backstop.
    ///
    /// This is the create half of the create/read-only slice: no status,
    /// context-epoch, or binding mutation, and no deletion, is offered.
    pub fn create_logical_role(&mut self, role: LogicalRole) -> Result<(), StateError> {
        validate_for_create(&role)?;
        self.run_transaction(|uow| uow.insert_logical_role(&role))
    }

    /// Reads one LogicalRole by durable identity.
    ///
    /// `Ok(None)` is the deterministic absence result for a role that does
    /// not exist. The main row and its ownership paths are read on a single
    /// snapshot; any decoding failure (a stored value that no longer
    /// satisfies the contract) fails closed instead of returning partially
    /// decoded data.
    pub fn find_logical_role(&self, role_id: &str) -> Result<Option<LogicalRole>, StateError> {
        let conn = self.connection();
        let tx = conn
            .unchecked_transaction()
            .map_err(internal_query_failure)?;
        let found = read_logical_role(&tx, role_id)?;
        // Read-only snapshot: commit merely ends it. On the error path above
        // the transaction rolls back on drop, with nothing written to undo.
        tx.commit().map_err(internal_query_failure)?;
        Ok(found)
    }
}

impl UnitOfWork<'_> {
    /// Inserts one validated LogicalRole and its ownership paths inside the
    /// open transaction. Crate-private; invoked by
    /// [`SqliteStateRepository::create_logical_role`].
    pub(crate) fn insert_logical_role(&self, role: &LogicalRole) -> Result<(), StateError> {
        insert_logical_role(self.tx(), role)
    }
}

const ROLE_EXISTS_SQL: &str = "SELECT 1 FROM logical_role WHERE role_id = ?1";

const INSERT_ROLE_SQL: &str = "INSERT INTO logical_role (
    role_id,
    project_id,
    role_type,
    status,
    current_context_epoch,
    name,
    workstream_id,
    integration_branch,
    context_manifest_id,
    active_binding_id,
    created_at
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)";

const INSERT_OWNERSHIP_PATH_SQL: &str =
    "INSERT INTO logical_role_ownership_path (role_id, position, path) VALUES (?1, ?2, ?3)";

const SELECT_ROLE_SQL: &str = "SELECT
    role_id,
    project_id,
    role_type,
    status,
    current_context_epoch,
    name,
    workstream_id,
    integration_branch,
    context_manifest_id,
    active_binding_id,
    created_at
FROM logical_role
WHERE role_id = ?1";

const SELECT_OWNERSHIP_PATHS_SQL: &str =
    "SELECT path FROM logical_role_ownership_path WHERE role_id = ?1 ORDER BY position";

/// Inserts the role row and its ownership-path rows using bound parameters.
///
/// Duplicate durable identities are refused twice: an explicit existence
/// check gives the common path a deterministic, driver-independent error,
/// and the primary-key constraint remains the durable backstop (including
/// against a concurrent writer on the same database file).
fn insert_logical_role(conn: &Connection, role: &LogicalRole) -> Result<(), StateError> {
    let exists: Option<i64> = conn
        .query_row(ROLE_EXISTS_SQL, [&role.role_id], |row| row.get(0))
        .optional()
        .map_err(write_failure)?;
    if exists.is_some() {
        return Err(StateError::LogicalRoleAlreadyExists {
            role_id: role.role_id.clone(),
        });
    }

    conn.execute(
        INSERT_ROLE_SQL,
        params![
            role.role_id,
            role.project_id,
            role.role_type.as_str(),
            role.status.as_str(),
            role.current_context_epoch,
            role.name,
            role.workstream_id,
            role.integration_branch,
            role.context_manifest_id,
            role.active_binding_id,
            role.created_at
        ],
    )
    .map_err(|error| {
        if is_role_id_constraint_violation(&error) {
            StateError::LogicalRoleAlreadyExists {
                role_id: role.role_id.clone(),
            }
        } else {
            write_failure(error)
        }
    })?;

    let mut statement = conn
        .prepare(INSERT_OWNERSHIP_PATH_SQL)
        .map_err(write_failure)?;
    for (index, path) in role.ownership_paths.iter().enumerate() {
        let position = i64::try_from(index).map_err(|_| StateError::LogicalRoleValidation {
            detail: format!(
                "ownership_paths is too long to assign ordered positions ({index} elements)"
            ),
        })?;
        statement
            .execute(params![role.role_id, position, path])
            .map_err(write_failure)?;
    }
    Ok(())
}

/// Reads the main row and, if present, its ownership paths (ordered by
/// stored position) on the caller's snapshot.
fn read_logical_role(conn: &Connection, role_id: &str) -> Result<Option<LogicalRole>, StateError> {
    let Some(row) = conn
        .query_row(SELECT_ROLE_SQL, [role_id], extract_role_row)
        .optional()
        .map_err(internal_query_failure)?
    else {
        return Ok(None);
    };
    let ownership_paths = read_ownership_paths(conn, role_id)?;
    row.into_logical_role(ownership_paths).map(Some)
}

fn read_ownership_paths(conn: &Connection, role_id: &str) -> Result<Vec<String>, StateError> {
    let mut statement = conn
        .prepare(SELECT_OWNERSHIP_PATHS_SQL)
        .map_err(internal_query_failure)?;
    let rows = statement
        .query_map([role_id], |row| row.get::<_, String>(0))
        .map_err(internal_query_failure)?;
    rows.collect::<Result<Vec<String>, _>>()
        .map_err(internal_query_failure)
}

/// Raw column image of one `logical_role` row, before any contract
/// interpretation is applied.
struct RoleRow {
    role_id: String,
    project_id: String,
    role_type: String,
    status: String,
    current_context_epoch: i64,
    name: Option<String>,
    workstream_id: Option<String>,
    integration_branch: Option<String>,
    context_manifest_id: Option<String>,
    active_binding_id: Option<String>,
    created_at: Option<String>,
}

impl RoleRow {
    /// Applies contract decoding (enum values, non-negative epoch) and
    /// fails closed on any violation.
    fn into_logical_role(self, ownership_paths: Vec<String>) -> Result<LogicalRole, StateError> {
        if self.current_context_epoch < 0 {
            return Err(StateError::LogicalRoleDecodeFailed {
                detail: format!(
                    "persisted current_context_epoch {} is negative",
                    self.current_context_epoch
                ),
            });
        }
        Ok(LogicalRole {
            role_id: self.role_id,
            project_id: self.project_id,
            role_type: LogicalRoleType::from_storage(&self.role_type)?,
            status: LogicalRoleStatus::from_storage(&self.status)?,
            current_context_epoch: self.current_context_epoch,
            name: self.name,
            workstream_id: self.workstream_id,
            ownership_paths,
            integration_branch: self.integration_branch,
            context_manifest_id: self.context_manifest_id,
            active_binding_id: self.active_binding_id,
            created_at: self.created_at,
        })
    }
}

fn extract_role_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RoleRow> {
    Ok(RoleRow {
        role_id: row.get("role_id")?,
        project_id: row.get("project_id")?,
        role_type: row.get("role_type")?,
        status: row.get("status")?,
        current_context_epoch: row.get("current_context_epoch")?,
        name: row.get("name")?,
        workstream_id: row.get("workstream_id")?,
        integration_branch: row.get("integration_branch")?,
        context_manifest_id: row.get("context_manifest_id")?,
        active_binding_id: row.get("active_binding_id")?,
        created_at: row.get("created_at")?,
    })
}

/// Contract-level validation performed before any storage access on create.
///
/// Exactly the frozen constraints are enforced — no invented orchestration
/// rules: constrained identifiers non-empty and at most
/// [`MAX_IDENTIFIER_LENGTH`] scalar values, and a non-negative context
/// epoch. Role type and status are already restricted to their contract
/// sets by their enum types.
fn validate_for_create(role: &LogicalRole) -> Result<(), StateError> {
    ensure_identifier("role_id", &role.role_id)?;
    ensure_identifier("project_id", &role.project_id)?;
    if let Some(workstream_id) = &role.workstream_id {
        ensure_identifier("workstream_id", workstream_id)?;
    }
    if let Some(context_manifest_id) = &role.context_manifest_id {
        ensure_identifier("context_manifest_id", context_manifest_id)?;
    }
    if role.current_context_epoch < 0 {
        return Err(StateError::LogicalRoleValidation {
            detail: format!(
                "current_context_epoch must be >= 0, found {}",
                role.current_context_epoch
            ),
        });
    }
    Ok(())
}

fn ensure_identifier(field: &str, value: &str) -> Result<(), StateError> {
    if value.is_empty() {
        return Err(StateError::LogicalRoleValidation {
            detail: format!("{field} must not be empty"),
        });
    }
    let length = value.chars().count();
    if length > MAX_IDENTIFIER_LENGTH {
        return Err(StateError::LogicalRoleValidation {
            detail: format!(
                "{field} length {length} exceeds the maximum of {MAX_IDENTIFIER_LENGTH}"
            ),
        });
    }
    Ok(())
}

fn write_failure(error: rusqlite::Error) -> StateError {
    StateError::LogicalRoleWriteFailed {
        detail: error.to_string(),
    }
}

fn internal_query_failure(error: rusqlite::Error) -> StateError {
    StateError::InternalQueryFailed {
        detail: error.to_string(),
    }
}

/// SQLite extended result codes for identity constraint violations on the
/// `logical_role` primary key: 1555 = `SQLITE_CONSTRAINT_PRIMARYKEY`,
/// 2067 = `SQLITE_CONSTRAINT_UNIQUE`.
fn is_role_id_constraint_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                extended_code: 1555 | 2067,
                ..
            },
            _
        )
    )
}
