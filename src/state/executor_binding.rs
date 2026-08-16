//! Durable ExecutorBinding persistence (migration 0003 storage; bounded
//! lease-renewal persistence added by the A3-008 slice).
//!
//! [`ExecutorBinding`] is the frozen contract for the temporary association
//! between a durable [`LogicalRole`](crate::logical_role::LogicalRole)
//! identity and a provider/model/runtime/session executor. The durable role
//! remains distinct from its current or historical executors: executor
//! replacement must never replace role identity, so a binding row always
//! references — and never mutates — an existing persisted role.
//!
//! Scope: immutable create guarded by the single-active-binding persistence
//! invariant, durable read by `binding_id`, the guarded renewal of
//! `lease_expires_at` on an existing non-released binding, and the one-time
//! terminal release of an existing binding. Binding history is append-only
//! durable evidence: the only authorized mutations of a persisted binding
//! are the bounded lease renewal of `lease_expires_at` while the binding is
//! non-released (see
//! [`SqliteStateRepository::renew_executor_binding_lease`]) and the single
//! write-once terminal transition of `released_at`/`release_reason` from
//! NULL to recorded (see
//! [`SqliteStateRepository::release_executor_binding`]); every other
//! binding field is immutable, and there is deliberately no public update,
//! delete, or expiry-evaluation path, and no failover orchestration
//! semantics. Creation is refused while the target role already has a
//! binding that is not conclusively fully released — enforced once as a
//! transactional create-time pre-check (see
//! [`SqliteStateRepository::create_executor_binding`]) and once durably by
//! the migration-0005 partial unique index over `executor_binding(role_id)`
//! — so at most one not-fully-released binding per role can ever persist.
//! `provider_id`, `model_id`, and `runtime_id` are
//! opaque non-empty strings: State persists them but never decides
//! provider, model, runtime, or routing eligibility.
//!
//! `bound_at`, `lease_expires_at`, and `released_at` are persisted contract
//! date-time strings, stored exactly as provided: State never inspects the
//! wall clock, parses, compares, or evaluates them, and exposes no
//! `is_active`/`is_expired`/`lease_due` semantics — recording a renewed
//! `lease_expires_at` value supplied by the trusted orchestrator-core
//! boundary is a pure persistence write, not a decision that the lease is
//! currently unexpired. `session_ref` is an
//! opaque persistence reference only and is never credential storage.
//! `rehydration_completed` is persistence data only; absence stays
//! distinguishable from explicit `true`/`false`.

use rusqlite::{Connection, OptionalExtension, params};

use crate::error::StateError;
use crate::repository::{SqliteStateRepository, UnitOfWork};

/// Maximum allowed length, in Unicode scalar values, of every constrained
/// ExecutorBinding identifier (frozen contract: 200).
pub const MAX_IDENTIFIER_LENGTH: usize = 200;

/// Why a binding was released (frozen closed enum of exactly nine values).
///
/// There is no `UNKNOWN`, `OTHER`, `CUSTOM`, fallback, or provider-specific
/// variant: any value outside the nine contract reasons fails closed at the
/// decode boundary and is rejected by the storage backstop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseReason {
    /// `RATE_LIMITED`.
    RateLimited,
    /// `SESSION_EXHAUSTED`.
    SessionExhausted,
    /// `AUTH_REQUIRED`.
    AuthRequired,
    /// `PROVIDER_DOWN`.
    ProviderDown,
    /// `CRASH`.
    Crash,
    /// `HOST_SWITCH`.
    HostSwitch,
    /// `USER_REQUEST`.
    UserRequest,
    /// `COMPLETED`.
    Completed,
    /// `LEASE_EXPIRED`.
    LeaseExpired,
}

impl ReleaseReason {
    /// The durable storage representation required by the contract.
    pub fn as_str(self) -> &'static str {
        match self {
            ReleaseReason::RateLimited => "RATE_LIMITED",
            ReleaseReason::SessionExhausted => "SESSION_EXHAUSTED",
            ReleaseReason::AuthRequired => "AUTH_REQUIRED",
            ReleaseReason::ProviderDown => "PROVIDER_DOWN",
            ReleaseReason::Crash => "CRASH",
            ReleaseReason::HostSwitch => "HOST_SWITCH",
            ReleaseReason::UserRequest => "USER_REQUEST",
            ReleaseReason::Completed => "COMPLETED",
            ReleaseReason::LeaseExpired => "LEASE_EXPIRED",
        }
    }

    /// Decodes the durable representation, failing closed on anything other
    /// than the nine contract reasons.
    pub(crate) fn from_storage(value: &str) -> Result<Self, StateError> {
        match value {
            "RATE_LIMITED" => Ok(ReleaseReason::RateLimited),
            "SESSION_EXHAUSTED" => Ok(ReleaseReason::SessionExhausted),
            "AUTH_REQUIRED" => Ok(ReleaseReason::AuthRequired),
            "PROVIDER_DOWN" => Ok(ReleaseReason::ProviderDown),
            "CRASH" => Ok(ReleaseReason::Crash),
            "HOST_SWITCH" => Ok(ReleaseReason::HostSwitch),
            "USER_REQUEST" => Ok(ReleaseReason::UserRequest),
            "COMPLETED" => Ok(ReleaseReason::Completed),
            "LEASE_EXPIRED" => Ok(ReleaseReason::LeaseExpired),
            other => Err(StateError::ExecutorBindingDecodeFailed {
                detail: format!(
                    "unknown release_reason {other:?}: only the nine frozen release reasons are representable"
                ),
            }),
        }
    }
}

/// An immutable ExecutorBinding row (frozen contract).
///
/// Required fields are non-nullable; optional fields are nullable exactly as
/// the contract lists them. `rehydration_completed` stays `Option<bool>` so
/// absence remains distinguishable from explicit `true`/`false`. All fields
/// are set only at construction; the repository offers no mutation path for
/// a persisted binding.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecutorBinding {
    /// Durable binding identity. Non-empty, at most
    /// [`MAX_IDENTIFIER_LENGTH`] scalar values.
    pub binding_id: String,
    /// The durable LogicalRole this binding associates an executor with.
    /// Non-empty, at most [`MAX_IDENTIFIER_LENGTH`] scalar values; must
    /// reference an existing persisted role.
    pub role_id: String,
    /// Opaque provider identifier. Non-empty; never enum-constrained and
    /// never validated against a provider registry.
    pub provider_id: String,
    /// Opaque model identifier. Non-empty; never enum-constrained and never
    /// validated against a model registry.
    pub model_id: String,
    /// Opaque runtime identifier. Non-empty; never enum-constrained and
    /// never validated against a runtime registry.
    pub runtime_id: String,
    /// Opaque session persistence reference. Never credential storage.
    pub session_ref: Option<String>,
    /// Opaque routing-decision reference. When present: non-empty, at most
    /// [`MAX_IDENTIFIER_LENGTH`] scalar values. State never dereferences it.
    pub routing_decision_id: Option<String>,
    /// Contract date-time string, stored as provided. Never parsed,
    /// compared, or evaluated by State.
    pub bound_at: String,
    /// Contract date-time string, stored as provided. Never parsed,
    /// compared, or evaluated by State; the only mutation is the guarded
    /// renewal write, which records a newly supplied value without
    /// evaluating it.
    pub lease_expires_at: String,
    /// Contract date-time string when present, stored as provided.
    pub released_at: Option<String>,
    /// Why the binding was released, when it was. Exactly the nine frozen
    /// reasons; unknown persisted values fail closed during decode.
    pub release_reason: Option<ReleaseReason>,
    /// Persistence data only: absence is distinguishable from explicit
    /// `true`/`false`, and State never initiates or gates rehydration.
    pub rehydration_completed: Option<bool>,
}

impl SqliteStateRepository {
    /// Durably creates a new immutable ExecutorBinding.
    ///
    /// Creation is atomic: the row commits or not at all. A `binding_id`
    /// that already exists fails explicitly with
    /// [`StateError::ExecutorBindingAlreadyExists`]; a `role_id` that does
    /// not reference an existing persisted LogicalRole fails explicitly with
    /// [`StateError::ExecutorBindingRoleNotFound`]. A `role_id` that already
    /// has one not-fully-released binding fails explicitly with
    /// [`StateError::ExecutorBindingUnreleasedConflict`]: duplicate durable
    /// `binding_id` and same-role unreleased-binding conflict remain two
    /// independently observable failure conditions, and duplicate-ID
    /// semantics keep precedence.
    ///
    /// The single-active-binding persistence guard reasons only from the
    /// durable terminal pair `released_at`/`release_reason`, inside the
    /// same transaction as the INSERT: a role is rebindable only once its
    /// existing binding is conclusively fully released (both terminal
    /// fields recorded by an authorized explicit release). A valid
    /// unreleased binding, a renewed-but-unreleased binding, a binding
    /// whose `lease_expires_at` merely looks old, and either corrupt
    /// partial terminal shape all block creation — `lease_expires_at` is
    /// never evaluated against any clock, and this operation never claims
    /// or decides current lease validity, executor authority, or failover.
    /// The blocking row is never mutated, released, repaired, deleted, or
    /// replaced by the refusal. Behind the pre-check, the migration-0005
    /// partial unique index over `executor_binding(role_id)` is the durable
    /// storage backstop, so even a write that bypassed the typed check can
    /// never durably produce a second not-fully-released binding for one
    /// role; a constraint hit there also fails closed and is never reported
    /// as successful creation.
    ///
    /// This repository never overwrites, replaces, upserts, merges, or
    /// deletes-and-reinserts binding history, and creating a binding never
    /// mutates the referenced LogicalRole (its `active_binding_id`,
    /// status, epoch, and identity all remain untouched).
    ///
    /// Contract-level validation runs before any storage access, and the
    /// primary-key, foreign-key, and partial-unique-index constraints
    /// re-enforce the same rules as durable backstops.
    ///
    /// This is the create half of the binding lifecycle offered here: the
    /// only later transitions this repository offers for a persisted
    /// binding are the bounded lease renewal while it is non-released and
    /// the one-time terminal release. No expiry evaluation or failover
    /// behavior is offered, and persisting a binding does not make that
    /// executor active, authoritative, rehydrated, or permitted to mutate
    /// State.
    pub fn create_executor_binding(&mut self, binding: ExecutorBinding) -> Result<(), StateError> {
        validate_for_create(&binding)?;
        self.run_transaction(|uow| uow.insert_executor_binding(&binding))
    }

    /// Reads one ExecutorBinding by durable identity.
    ///
    /// `Ok(None)` is the deterministic absence result for a binding that
    /// does not exist. Any decoding failure (a stored value that no longer
    /// satisfies the contract, such as an unknown `release_reason` or a
    /// corrupt `rehydration_completed`) fails closed instead of returning
    /// partially decoded data or a fabricated default binding.
    pub fn find_executor_binding(
        &self,
        binding_id: &str,
    ) -> Result<Option<ExecutorBinding>, StateError> {
        let conn = self.connection();
        let tx = conn
            .unchecked_transaction()
            .map_err(internal_query_failure)?;
        let found = read_executor_binding(&tx, binding_id)?;
        // Read-only snapshot: commit merely ends it. On the error path above
        // the transaction rolls back on drop, with nothing written to undo.
        tx.commit().map_err(internal_query_failure)?;
        Ok(found)
    }

    /// Durably records the one-time terminal release of an existing
    /// persisted ExecutorBinding.
    ///
    /// Exactly two fields change, exactly once, in one atomic transaction:
    /// `released_at` and `release_reason`. `released_at` is stored exactly
    /// as supplied — State never reads the wall clock, generates,
    /// parses, compares, or orders it — and `release_reason` is one of the
    /// nine frozen reasons supplied by the caller: State never decides lease
    /// validity or expiry, including for
    /// [`ReleaseReason::LeaseExpired`]. Every other binding field is
    /// immutable, the binding is never deleted or replaced, no other
    /// binding is created, and the referenced LogicalRole (including its
    /// `active_binding_id`) is never touched. Recording a release does not
    /// select, authorize, activate, or rehydrate any other executor.
    ///
    /// Fail-closed results:
    ///
    /// * an unknown `binding_id` fails with
    ///   [`StateError::ExecutorBindingNotFound`]: release requires an
    ///   already persisted binding and never fabricates one;
    /// * a binding whose terminal release slot is already occupied —
    ///   released through this API, created already-released, or carrying
    ///   any partial terminal shape — fails with
    ///   [`StateError::ExecutorBindingAlreadyReleased`], leaving the
    ///   originally recorded evidence untouched: a repeat release is never
    ///   treated as idempotent success;
    /// * an invalid input shape fails with
    ///   [`StateError::ExecutorBindingValidation`] before any storage
    ///   access;
    /// * storage, transaction, and corrupt-row failures surface as explicit
    ///   errors and never persist a partial release pair.
    pub fn release_executor_binding(
        &mut self,
        binding_id: &str,
        released_at: &str,
        release_reason: ReleaseReason,
    ) -> Result<(), StateError> {
        ensure_identifier("binding_id", binding_id)?;
        ensure_non_empty("released_at", released_at)?;
        self.run_transaction(|uow| apply_release(uow.tx(), binding_id, released_at, release_reason))
    }

    /// Durably records a renewed `lease_expires_at` for exactly one
    /// existing, non-released ExecutorBinding.
    ///
    /// Exactly one field changes, in one atomic transaction:
    /// `lease_expires_at` takes the caller-supplied value. That value is an
    /// opaque contract date-time string stored exactly as supplied — State
    /// never reads the wall clock, generates, parses, compares, or orders
    /// it, and never decides lease duration, validity, or expiry: the
    /// trusted orchestrator-core boundary supplies the value after already
    /// deciding to record a renewal, and this operation does not observe
    /// heartbeats or schedule any later renewal or release. Every other
    /// binding field is immutable, the binding is never deleted, replaced,
    /// or re-created, no additional binding row appears, and the referenced
    /// LogicalRole (including its `active_binding_id`) is never touched.
    ///
    /// Eligibility is terminal-state only: `released_at IS NULL AND
    /// release_reason IS NULL`. "Non-released" is not an expiry decision —
    /// this operation never claims the lease is currently unexpired.
    ///
    /// Fail-closed results:
    ///
    /// * an unknown `binding_id` fails with
    ///   [`StateError::ExecutorBindingNotFound`]: renewal requires an
    ///   already persisted binding and never fabricates one;
    /// * a binding whose terminal release slot is occupied — released
    ///   through this API (with any reason, including
    ///   [`ReleaseReason::LeaseExpired`]), created already-released, or
    ///   carrying any partial terminal shape — fails with
    ///   [`StateError::ExecutorBindingAlreadyReleased`], leaving the
    ///   terminal evidence and every other field untouched: renewal never
    ///   clears, overwrites, or replaces `released_at`/`release_reason`,
    ///   and never reopens the binding;
    /// * an invalid input shape fails with
    ///   [`StateError::ExecutorBindingValidation`] before any storage
    ///   access;
    /// * storage, transaction, and corrupt-row failures surface as explicit
    ///   errors and never persist a partial renewal.
    pub fn renew_executor_binding_lease(
        &mut self,
        binding_id: &str,
        lease_expires_at: &str,
    ) -> Result<(), StateError> {
        ensure_identifier("binding_id", binding_id)?;
        ensure_non_empty("lease_expires_at", lease_expires_at)?;
        self.run_transaction(|uow| apply_lease_renewal(uow.tx(), binding_id, lease_expires_at))
    }
}

impl UnitOfWork<'_> {
    /// Inserts one validated ExecutorBinding inside the open transaction.
    /// Crate-private; invoked by
    /// [`SqliteStateRepository::create_executor_binding`].
    pub(crate) fn insert_executor_binding(
        &self,
        binding: &ExecutorBinding,
    ) -> Result<(), StateError> {
        insert_executor_binding(self.tx(), binding)
    }
}

const BINDING_EXISTS_SQL: &str = "SELECT 1 FROM executor_binding WHERE binding_id = ?1";

const ROLE_EXISTS_FOR_BINDING_SQL: &str = "SELECT 1 FROM logical_role WHERE role_id = ?1";

/// Create-time single-active-binding pre-check: the first binding for the
/// target role that is not conclusively fully released, if any. The
/// predicate is the same fail-closed shape as the migration-0005 partial
/// unique index — a row blocks rebind unless its terminal pair is complete
/// — and deliberately never mentions `lease_expires_at`. `ORDER BY
/// binding_id LIMIT 1` keeps the reported blocker deterministic even for
/// storage this repository could not itself have produced.
const ROLE_BLOCKING_BINDING_SQL: &str = "SELECT binding_id FROM executor_binding
WHERE role_id = ?1 AND (released_at IS NULL OR release_reason IS NULL)
ORDER BY binding_id
LIMIT 1";

const INSERT_BINDING_SQL: &str = "INSERT INTO executor_binding (
    binding_id,
    role_id,
    provider_id,
    model_id,
    runtime_id,
    session_ref,
    routing_decision_id,
    bound_at,
    lease_expires_at,
    released_at,
    release_reason,
    rehydration_completed
) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";

const SELECT_BINDING_SQL: &str = "SELECT
    binding_id,
    role_id,
    provider_id,
    model_id,
    runtime_id,
    session_ref,
    routing_decision_id,
    bound_at,
    lease_expires_at,
    released_at,
    release_reason,
    rehydration_completed
FROM executor_binding
WHERE binding_id = ?1";

/// The one authorized mutation of a persisted binding: the write-once
/// terminal release transition. Both terminal fields are set by the same
/// single statement — the pair commits atomically or not at all — and the
/// guarded WHERE re-enforces terminal-slot emptiness as a durable backstop
/// behind the explicit pre-check.
const RELEASE_BINDING_SQL: &str = "UPDATE executor_binding
SET released_at = ?1, release_reason = ?2
WHERE binding_id = ?3 AND released_at IS NULL AND release_reason IS NULL";

/// The one authorized non-terminal mutation of a persisted binding: the
/// guarded lease renewal. Only `lease_expires_at` changes, and the guarded
/// WHERE re-enforces the non-released eligibility condition as a durable
/// backstop behind the explicit pre-check, so a terminally released
/// binding can never be renewed or reopened.
const RENEW_LEASE_SQL: &str = "UPDATE executor_binding
SET lease_expires_at = ?1
WHERE binding_id = ?2 AND released_at IS NULL AND release_reason IS NULL";

/// Inserts the binding row using bound parameters.
///
/// The frozen relational rules are refused twice each: explicit in-
/// transaction checks give the common path deterministic,
/// driver-independent errors, while the primary-key, foreign-key, and
/// migration-0005 partial-unique-index constraints remain the durable
/// backstops (including against a concurrent writer on the same database
/// file). Check order preserves accepted semantics: duplicate
/// `binding_id` first, then role existence, then the same-role
/// not-fully-released guard, then the INSERT itself.
fn insert_executor_binding(conn: &Connection, binding: &ExecutorBinding) -> Result<(), StateError> {
    let duplicate: Option<i64> = conn
        .query_row(BINDING_EXISTS_SQL, [&binding.binding_id], |row| row.get(0))
        .optional()
        .map_err(write_failure)?;
    if duplicate.is_some() {
        return Err(StateError::ExecutorBindingAlreadyExists {
            binding_id: binding.binding_id.clone(),
        });
    }

    let role: Option<i64> = conn
        .query_row(ROLE_EXISTS_FOR_BINDING_SQL, [&binding.role_id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(write_failure)?;
    if role.is_none() {
        return Err(StateError::ExecutorBindingRoleNotFound {
            role_id: binding.role_id.clone(),
        });
    }

    if let Some(blocking_binding_id) = find_role_blocking_binding(conn, &binding.role_id)? {
        return Err(StateError::ExecutorBindingUnreleasedConflict {
            role_id: binding.role_id.clone(),
            blocking_binding_id,
        });
    }

    conn.execute(
        INSERT_BINDING_SQL,
        params![
            binding.binding_id,
            binding.role_id,
            binding.provider_id,
            binding.model_id,
            binding.runtime_id,
            binding.session_ref,
            binding.routing_decision_id,
            binding.bound_at,
            binding.lease_expires_at,
            binding.released_at,
            binding.release_reason.map(ReleaseReason::as_str),
            binding.rehydration_completed.map(i64::from),
        ],
    )
    .map_err(|error| {
        if is_role_uniqueness_violation(&error) {
            role_conflict_from_backstop(conn, &binding.role_id, error)
        } else if is_binding_id_constraint_violation(&error) {
            StateError::ExecutorBindingAlreadyExists {
                binding_id: binding.binding_id.clone(),
            }
        } else if is_foreign_key_violation(&error) {
            StateError::ExecutorBindingRoleNotFound {
                role_id: binding.role_id.clone(),
            }
        } else {
            write_failure(error)
        }
    })?;
    Ok(())
}

/// Reads the deterministic first not-fully-released binding for `role_id`
/// on the caller's transaction snapshot, if one exists.
///
/// This is the bounded internal role-history inspection behind the
/// create-time guard: one indexed lookup, no public binding-history query
/// surface. Fail-closed predicate: a row blocks unless its terminal pair is
/// completely recorded, so valid unreleased rows and both corrupt partial
/// terminal shapes are all blocking; `lease_expires_at` is never consulted.
fn find_role_blocking_binding(
    conn: &Connection,
    role_id: &str,
) -> Result<Option<String>, StateError> {
    conn.query_row(ROLE_BLOCKING_BINDING_SQL, [role_id], |row| {
        row.get::<_, String>(0)
    })
    .optional()
    .map_err(write_failure)
}

/// Maps a storage-level unique-constraint hit on the single-active-binding
/// partial index to the deterministic domain conflict.
///
/// The failed statement leaves the still-open transaction usable, so the
/// blocking row is re-read to identify the conflict. If it cannot be
/// re-read, the storage failure itself is surfaced: the operation fails
/// closed either way and is never reported as successful creation, and no
/// binding identity is ever fabricated.
fn role_conflict_from_backstop(
    conn: &Connection,
    role_id: &str,
    error: rusqlite::Error,
) -> StateError {
    match find_role_blocking_binding(conn, role_id) {
        Ok(Some(blocking_binding_id)) => StateError::ExecutorBindingUnreleasedConflict {
            role_id: role_id.to_string(),
            blocking_binding_id,
        },
        _ => write_failure(error),
    }
}

/// Reads one binding row on the caller's snapshot and applies contract
/// decoding, failing closed on any violation.
fn read_executor_binding(
    conn: &Connection,
    binding_id: &str,
) -> Result<Option<ExecutorBinding>, StateError> {
    conn.query_row(SELECT_BINDING_SQL, [binding_id], extract_binding_row)
        .optional()
        .map_err(internal_query_failure)?
        .map(BindingRow::into_executor_binding)
        .transpose()
}

/// Applies the one-time terminal release inside the open transaction.
///
/// Crate-private; invoked by
/// [`SqliteStateRepository::release_executor_binding`]. The full row is
/// decoded first so corrupt persisted data fails closed before any write
/// decision is made; the guarded UPDATE then re-enforces write-once
/// emptiness as a durable backstop.
pub(crate) fn apply_release(
    conn: &Connection,
    binding_id: &str,
    released_at: &str,
    release_reason: ReleaseReason,
) -> Result<(), StateError> {
    require_unreleased_binding(conn, binding_id)?;
    let changed = conn
        .execute(
            RELEASE_BINDING_SQL,
            params![released_at, release_reason.as_str(), binding_id],
        )
        .map_err(write_failure)?;
    if changed == 0 {
        // Unreachable behind the pre-check within this single transaction;
        // if the guarded WHERE ever refuses a write, re-decide
        // deterministically instead of fabricating success.
        require_unreleased_binding(conn, binding_id)?;
    }
    Ok(())
}

/// Applies the guarded lease renewal inside the open transaction.
///
/// Crate-private; invoked by
/// [`SqliteStateRepository::renew_executor_binding_lease`]. The full row is
/// decoded first so corrupt persisted data fails closed before any write
/// decision is made; the guarded UPDATE then re-enforces the non-released
/// eligibility condition as a durable backstop, so the renewal can never
/// overwrite a terminally released binding. Only `lease_expires_at` is
/// written: no other column appears in the statement.
pub(crate) fn apply_lease_renewal(
    conn: &Connection,
    binding_id: &str,
    lease_expires_at: &str,
) -> Result<(), StateError> {
    require_unreleased_binding(conn, binding_id)?;
    let changed = conn
        .execute(RENEW_LEASE_SQL, params![lease_expires_at, binding_id])
        .map_err(write_failure)?;
    if changed == 0 {
        // Unreachable behind the pre-check within this single transaction;
        // if the guarded WHERE ever refuses a write, re-decide
        // deterministically instead of fabricating success.
        require_unreleased_binding(conn, binding_id)?;
    }
    Ok(())
}

/// Verifies on the caller's snapshot that the binding exists and that its
/// write-once terminal release slot is completely empty — `released_at`
/// and `release_reason` both NULL — failing closed otherwise.
///
/// This one predicate is the shared eligibility guard for the only two
/// mutations a persisted binding accepts: the one-time terminal release
/// and the lease renewal, which is permitted only while the binding has
/// not been terminally released. Any other stored shape — a complete
/// prior release, a binding created already-released, or a partial
/// terminal write this repository cannot produce — occupies the slot and
/// is refused as already released, so no terminal evidence is ever
/// overwritten and no released binding is ever reopened. The decoded read
/// means corrupt persisted rows fail closed with a decode error before any
/// write decision is made.
fn require_unreleased_binding(conn: &Connection, binding_id: &str) -> Result<(), StateError> {
    match read_executor_binding(conn, binding_id)? {
        None => Err(StateError::ExecutorBindingNotFound {
            binding_id: binding_id.to_string(),
        }),
        Some(binding) => {
            if binding.released_at.is_some() || binding.release_reason.is_some() {
                return Err(StateError::ExecutorBindingAlreadyReleased {
                    binding_id: binding_id.to_string(),
                });
            }
            Ok(())
        }
    }
}

/// Raw column image of one `executor_binding` row, before any contract
/// interpretation is applied.
struct BindingRow {
    binding_id: String,
    role_id: String,
    provider_id: String,
    model_id: String,
    runtime_id: String,
    session_ref: Option<String>,
    routing_decision_id: Option<String>,
    bound_at: String,
    lease_expires_at: String,
    released_at: Option<String>,
    release_reason: Option<String>,
    rehydration_completed: Option<i64>,
}

impl BindingRow {
    /// Applies contract decoding (closed release-reason enumeration,
    /// strictly boolean `rehydration_completed`) and fails closed on any
    /// violation.
    fn into_executor_binding(self) -> Result<ExecutorBinding, StateError> {
        let release_reason = match self.release_reason {
            None => None,
            Some(value) => Some(ReleaseReason::from_storage(&value)?),
        };
        let rehydration_completed = match self.rehydration_completed {
            None => None,
            Some(0) => Some(false),
            Some(1) => Some(true),
            Some(other) => {
                return Err(StateError::ExecutorBindingDecodeFailed {
                    detail: format!(
                        "persisted rehydration_completed {other} is not a boolean (0 or 1)"
                    ),
                });
            }
        };
        Ok(ExecutorBinding {
            binding_id: self.binding_id,
            role_id: self.role_id,
            provider_id: self.provider_id,
            model_id: self.model_id,
            runtime_id: self.runtime_id,
            session_ref: self.session_ref,
            routing_decision_id: self.routing_decision_id,
            bound_at: self.bound_at,
            lease_expires_at: self.lease_expires_at,
            released_at: self.released_at,
            release_reason,
            rehydration_completed,
        })
    }
}

fn extract_binding_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<BindingRow> {
    Ok(BindingRow {
        binding_id: row.get("binding_id")?,
        role_id: row.get("role_id")?,
        provider_id: row.get("provider_id")?,
        model_id: row.get("model_id")?,
        runtime_id: row.get("runtime_id")?,
        session_ref: row.get("session_ref")?,
        routing_decision_id: row.get("routing_decision_id")?,
        bound_at: row.get("bound_at")?,
        lease_expires_at: row.get("lease_expires_at")?,
        released_at: row.get("released_at")?,
        release_reason: row.get("release_reason")?,
        rehydration_completed: row.get("rehydration_completed")?,
    })
}

/// Contract-level validation performed before any storage access on create.
///
/// Exactly the frozen constraints are enforced — no invented orchestration
/// rules: `binding_id` and `role_id` non-empty and at most
/// [`MAX_IDENTIFIER_LENGTH`] scalar values; the opaque `provider_id`,
/// `model_id`, and `runtime_id` non-empty (with no enum, registry, or
/// length restriction); `routing_decision_id` non-empty and at most
/// [`MAX_IDENTIFIER_LENGTH`] scalar values when present. `session_ref`,
/// `bound_at`, `lease_expires_at`, and `released_at` are opaque strings
/// stored as provided, so no presence, ordering, or date-time rule is
/// invented for them, and `rehydration_completed` needs no validation
/// beyond its `Option<bool>` type.
fn validate_for_create(binding: &ExecutorBinding) -> Result<(), StateError> {
    ensure_identifier("binding_id", &binding.binding_id)?;
    ensure_identifier("role_id", &binding.role_id)?;
    ensure_non_empty("provider_id", &binding.provider_id)?;
    ensure_non_empty("model_id", &binding.model_id)?;
    ensure_non_empty("runtime_id", &binding.runtime_id)?;
    if let Some(routing_decision_id) = &binding.routing_decision_id {
        ensure_identifier("routing_decision_id", routing_decision_id)?;
    }
    Ok(())
}

fn ensure_identifier(field: &str, value: &str) -> Result<(), StateError> {
    ensure_non_empty(field, value)?;
    let length = value.chars().count();
    if length > MAX_IDENTIFIER_LENGTH {
        return Err(StateError::ExecutorBindingValidation {
            detail: format!(
                "{field} length {length} exceeds the maximum of {MAX_IDENTIFIER_LENGTH}"
            ),
        });
    }
    Ok(())
}

fn ensure_non_empty(field: &str, value: &str) -> Result<(), StateError> {
    if value.is_empty() {
        return Err(StateError::ExecutorBindingValidation {
            detail: format!("{field} must not be empty"),
        });
    }
    Ok(())
}

fn write_failure(error: rusqlite::Error) -> StateError {
    StateError::ExecutorBindingWriteFailed {
        detail: error.to_string(),
    }
}

fn internal_query_failure(error: rusqlite::Error) -> StateError {
    StateError::InternalQueryFailed {
        detail: error.to_string(),
    }
}

/// SQLite extended result codes for identity constraint violations on the
/// `executor_binding` primary key: 1555 = `SQLITE_CONSTRAINT_PRIMARYKEY`,
/// 2067 = `SQLITE_CONSTRAINT_UNIQUE`. Checked after
/// [`is_role_uniqueness_violation`] so a 2067 raised by the
/// single-active-binding partial index is never misreported as a duplicate
/// `binding_id`.
fn is_binding_id_constraint_violation(error: &rusqlite::Error) -> bool {
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

/// A storage-level violation of the migration-0005 partial unique index over
/// `executor_binding(role_id)`: 2067 = `SQLITE_CONSTRAINT_UNIQUE` with the
/// driver message naming `role_id`, the only column of that index.
fn is_role_uniqueness_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                extended_code: 2067,
                ..
            },
            Some(message),
        ) if message.contains("role_id")
    )
}

/// SQLite extended result code for a foreign-key violation:
/// 787 = `SQLITE_CONSTRAINT_FOREIGNKEY`.
fn is_foreign_key_violation(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                extended_code: 787,
                ..
            },
            _
        )
    )
}
