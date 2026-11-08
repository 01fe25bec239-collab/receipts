//! The trusted-core State write boundary.
//!
//! [`TrustedCoreStore`] is the only public way to obtain State write access.
//! Every mutation consumes one [`StateWriteAdmission`]: a non-cloneable value
//! that only the store's holder can mint, bound to this store's durable
//! identity, one closed operation family, the project and, where the payload
//! carries them, the graph, admitted source revision, compiler and policy.
//! State re-checks each binding against the actual payload (or the durable
//! record it targets) before writing; a mismatch writes nothing.
//!
//! Limits (deliberate, documented): this is an object-capability boundary
//! inside the trusted core process. It does not authenticate arbitrary code in
//! the same address space, and file modes plus a cooperative `flock` do not
//! exclude another process running with the same filesystem authority. Actual
//! exclusion of lower-trust principals (separate identity, no access to the
//! State directory, no inherited descriptor or writer channel) is a deployment
//! requirement that these library checks cannot prove.
//!
//! No legacy writer escapes the crate:
//!
//! ```compile_fail
//! let _ = receipts_state::SqliteStateRepository::open("/tmp/state.db");
//! ```
//!
//! ```compile_fail
//! fn write(repo: &mut receipts_state::SqliteStateRepository, e: receipts_state::EventEnvelope) {
//!     let _ = repo.append_event(e);
//! }
//! ```
//!
//! ```compile_fail
//! fn raw(repo: &mut receipts_state::SqliteStateRepository) {
//!     let _ = repo.run_transaction(|_| Ok(()));
//! }
//! ```
//!
//! ```compile_fail
//! let _ = receipts_state::SqliteStateRepository::migrate_existing_to_current("/tmp/state.db");
//! ```
//!
//! An admission is neither constructible outside State nor cloneable:
//!
//! ```compile_fail
//! fn forge(s: receipts_state::StateWriteSubjectV1) -> receipts_state::StateWriteAdmission {
//!     receipts_state::StateWriteAdmission { store_id: String::new(), subject: s }
//! }
//! ```
//!
//! ```compile_fail
//! fn reuse(a: &receipts_state::StateWriteAdmission) -> receipts_state::StateWriteAdmission {
//!     a.clone()
//! }
//! ```
//!
//! The only public write path is the exclusive store:
//!
//! ```no_run
//! fn open() -> Result<receipts_state::TrustedCoreStore, receipts_state::StateError> {
//!     receipts_state::TrustedCoreStore::open_exclusive("/var/lib/receipts/state.db")
//! }
//! ```

use std::fmt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::attempt::{AttemptFinish, AttemptStart};
use crate::availability_facts::{AvailabilityFactReceiptV1, AvailabilityFactV1};
use crate::checkpoint_ref::CheckpointRecord;
use crate::context_epoch::{ChangedSources, ContextEpoch, ContextEpochTrigger};
use crate::context_manifest::ContextManifest;
use crate::context_rehydration::{
    ArtifactMaterializer, ContextRehydratedEventSupplier, ContextRehydrationOutcome,
    ContextRehydrationRequest, RepositorySnapshotMaterializer,
};
use crate::decision_records::{DecisionRecordV1, ProvenanceLinkKind};
use crate::epoch_value::StateEpochValueV1;
use crate::error::StateError;
use crate::event_proposal::{EventPersistReceiptV1, EventProposalV1};
use crate::executor_binding::{ExecutorBinding, ReleaseReason};
use crate::executor_binding_lease_expiry::{
    ExecutorLeaseExpiryOutcomeV1, ExecutorLeaseExpiryRequestV2,
};
use crate::graph_history::StateGraphMutationV1;
use crate::graph_node_result::StateGraphNodeResultV1;
use crate::graph_repository::{StateGraphGenesisV1, StateGraphSnapshotV1};
use crate::logical_role::{LogicalRole, LogicalRoleStatus};
use crate::logical_role_lifecycle::RoleChangeEvidenceV1;
use crate::output_ref::OutputRef;
use crate::repository::SqliteStateRepository;
use crate::trusted_time::TrustedClockV1;

/// Closed operation families a single admission can authorize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateWriteOperationV1 {
    GraphGenesis,
    GraphMutation,
    GraphNodeResult,
    Attempt,
    Checkpoint,
    OutputRef,
    DecisionRecord,
    ProvenanceLink,
    EventProposal,
    ContextEpoch,
    ContextManifest,
    ContextRehydration,
    LogicalRole,
    ExecutorBinding,
    AvailabilityFact,
}

/// Why State refused a write admission or an exclusive open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateWriteRefusalV1 {
    /// The admission was minted by another store.
    WrongStore,
    /// The durable store identity no longer matches the opened store.
    StoreIdentityChanged,
    WrongOperation,
    ProjectMismatch,
    GraphMismatch,
    SourceMismatch,
    CompilerMismatch,
    PolicyMismatch,
    /// The targeted role/binding/attempt does not exist, so its project is unbound.
    UnknownSubject,
    /// Path is relative, a symlink, not a regular file/directory, or hard-linked.
    InsecurePath,
    /// Directory or file is not owned by the effective user.
    InsecureOwnership,
    /// Directory or file is accessible to group/other.
    InsecureMode,
    /// Another holder has the exclusive writer lock.
    WriterLockHeld,
    /// The opened file is not the object that was validated.
    IdentityRace,
    /// No secure open is implemented for this platform.
    UnsupportedPlatform,
}

impl StateWriteRefusalV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WrongStore => "WRONG_STORE",
            Self::StoreIdentityChanged => "STORE_IDENTITY_CHANGED",
            Self::WrongOperation => "WRONG_OPERATION",
            Self::ProjectMismatch => "PROJECT_MISMATCH",
            Self::GraphMismatch => "GRAPH_MISMATCH",
            Self::SourceMismatch => "SOURCE_MISMATCH",
            Self::CompilerMismatch => "COMPILER_MISMATCH",
            Self::PolicyMismatch => "POLICY_MISMATCH",
            Self::UnknownSubject => "UNKNOWN_SUBJECT",
            Self::InsecurePath => "INSECURE_PATH",
            Self::InsecureOwnership => "INSECURE_OWNERSHIP",
            Self::InsecureMode => "INSECURE_MODE",
            Self::WriterLockHeld => "WRITER_LOCK_HELD",
            Self::IdentityRace => "IDENTITY_RACE",
            Self::UnsupportedPlatform => "UNSUPPORTED_PLATFORM",
        }
    }
}

fn refused(reason: StateWriteRefusalV1) -> StateError {
    StateError::WriteAuthorityRefused { reason }
}

/// The physical subject the trusted core resolved for one write.
///
/// Optional bindings are optional only where the payload has no such field;
/// State requires exact equality with the payload, so an unused binding or a
/// missing required one both refuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateWriteSubjectV1 {
    pub operation: StateWriteOperationV1,
    pub project_id: String,
    pub graph_id: Option<String>,
    pub source_ref: Option<String>,
    pub compiler_id: Option<String>,
    pub policy_id: Option<String>,
}

/// One single-use write admission. Not `Clone`, not constructible outside
/// State, consumed by the one mutation it authorizes.
pub struct StateWriteAdmission {
    store_id: String,
    subject: StateWriteSubjectV1,
}

impl fmt::Debug for StateWriteAdmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StateWriteAdmission")
            .field("operation", &self.subject.operation)
            .finish_non_exhaustive()
    }
}

impl StateWriteAdmission {
    pub fn subject(&self) -> &StateWriteSubjectV1 {
        &self.subject
    }
}

/// The trusted core's exclusive State writer.
pub struct TrustedCoreStore {
    repo: SqliteStateRepository,
    store_id: String,
    // Held for the store's lifetime: the cooperative exclusive-writer lock and
    // the validated database object.
    _lock: File,
    _db: File,
}

impl fmt::Debug for TrustedCoreStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TrustedCoreStore")
    }
}

impl TrustedCoreStore {
    /// Opens (creating if absent) a current store for exclusive writing.
    ///
    /// Refuses relative or symlinked paths, a database or directory not owned
    /// by the effective user or accessible to group/other, hard links, a held
    /// writer lock and any swap between validation and open. Never repairs
    /// permissions and never upgrades an older schema.
    pub fn open_exclusive(path: impl AsRef<Path>) -> Result<Self, StateError> {
        let (db_path, lock, db) = secure::prepare(path.as_ref(), true)?;
        let repo = SqliteStateRepository::open(&db_path)?;
        Self::finish(repo, &db_path, lock, db)
    }

    /// Explicitly upgrades an existing v11–v19 store to the current schema and
    /// opens it, under the same ownership/lock checks. Returns the new head.
    pub fn migrate_exclusive(path: impl AsRef<Path>) -> Result<(Self, u32), StateError> {
        let (db_path, lock, db) = secure::prepare(path.as_ref(), false)?;
        let version = SqliteStateRepository::migrate_existing_to_current(&db_path)?;
        let repo = SqliteStateRepository::open(&db_path)?;
        Ok((Self::finish(repo, &db_path, lock, db)?, version))
    }

    fn finish(
        repo: SqliteStateRepository,
        db_path: &Path,
        lock: File,
        db: File,
    ) -> Result<Self, StateError> {
        secure::verify_same_object(db_path, &db)?;
        secure::verify_sidecars(db_path)?;
        let store_id = repo.store_identity()?;
        Ok(Self {
            repo,
            store_id,
            _lock: lock,
            _db: db,
        })
    }

    /// This store's durable identity (random, fixed at migration 20).
    pub fn store_id(&self) -> &str {
        &self.store_id
    }

    /// Read access to the store. Reads never write; mutators are crate-private.
    pub fn reader(&self) -> &SqliteStateRepository {
        &self.repo
    }

    /// Mints one admission for the subject the trusted core resolved. Only the
    /// holder of this store can call it; State re-checks the subject against
    /// the actual payload when it is consumed.
    pub fn admit(&self, subject: StateWriteSubjectV1) -> StateWriteAdmission {
        StateWriteAdmission {
            store_id: self.store_id.clone(),
            subject,
        }
    }

    fn check(
        &self,
        admission: StateWriteAdmission,
        operation: StateWriteOperationV1,
        bound: Bound<'_>,
    ) -> Result<(), StateError> {
        use StateWriteRefusalV1 as R;
        let subject = admission.subject;
        if admission.store_id != self.store_id {
            return Err(refused(R::WrongStore));
        }
        if self.repo.store_identity()? != self.store_id {
            return Err(refused(R::StoreIdentityChanged));
        }
        if subject.operation != operation {
            return Err(refused(R::WrongOperation));
        }
        let pairs = [
            (
                Some(subject.project_id.as_str()),
                Some(bound.project),
                R::ProjectMismatch,
            ),
            (subject.graph_id.as_deref(), bound.graph, R::GraphMismatch),
            (
                subject.source_ref.as_deref(),
                bound.source,
                R::SourceMismatch,
            ),
            (
                subject.compiler_id.as_deref(),
                bound.compiler,
                R::CompilerMismatch,
            ),
            (
                subject.policy_id.as_deref(),
                bound.policy,
                R::PolicyMismatch,
            ),
        ];
        for (admitted, actual, reason) in pairs {
            if admitted != actual {
                return Err(refused(reason));
            }
        }
        Ok(())
    }

    fn role_project(&self, role_id: &str) -> Result<String, StateError> {
        self.repo
            .find_logical_role(role_id)?
            .map(|role| role.project_id)
            .ok_or_else(|| refused(StateWriteRefusalV1::UnknownSubject))
    }

    fn binding_project(&self, binding_id: &str) -> Result<String, StateError> {
        let binding = self
            .repo
            .find_executor_binding(binding_id)?
            .ok_or_else(|| refused(StateWriteRefusalV1::UnknownSubject))?;
        self.role_project(&binding.role_id)
    }

    // ---- graph -------------------------------------------------------------

    pub fn create_initial_graph_v1(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        request: StateGraphGenesisV1,
    ) -> Result<(), StateError> {
        let provenance = request.genesis_provenance();
        self.check(
            admission,
            StateWriteOperationV1::GraphGenesis,
            Bound {
                project: request.project_id(),
                graph: Some(request.graph_id()),
                source: Some(provenance.source_ref().as_str()),
                compiler: Some(provenance.compiler_id().as_str()),
                policy: Some(request.policy_id()),
            },
        )?;
        self.repo.create_initial_graph_v1(clock, request)
    }

    pub fn append_graph_mutation_v1(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        mutation: StateGraphMutationV1,
        snapshot: StateGraphSnapshotV1,
    ) -> Result<(), StateError> {
        let content = &snapshot.content;
        let provenance = content.genesis_provenance();
        self.check(
            admission,
            StateWriteOperationV1::GraphMutation,
            Bound {
                project: content.project_id(),
                graph: Some(mutation.graph_id()),
                source: Some(provenance.source_ref().as_str()),
                compiler: Some(provenance.compiler_id().as_str()),
                policy: Some(content.policy_id()),
            },
        )?;
        self.repo
            .append_graph_mutation_v1(clock, mutation, snapshot)
    }

    pub fn append_node_result_v1(
        &mut self,
        admission: StateWriteAdmission,
        project: &str,
        version: &str,
        reference: &str,
        result: StateGraphNodeResultV1,
    ) -> Result<(), StateError> {
        self.check(
            admission,
            StateWriteOperationV1::GraphNodeResult,
            Bound::graph(project, result.graph_id()),
        )?;
        self.repo
            .append_node_result_v1(project, version, reference, result)
    }

    // ---- execution records ---------------------------------------------------

    pub fn append_attempt_started(
        &mut self,
        admission: StateWriteAdmission,
        start: AttemptStart,
    ) -> Result<(), StateError> {
        self.check(
            admission,
            StateWriteOperationV1::Attempt,
            Bound::graph(start.project_id(), start.graph_id()),
        )?;
        self.repo.append_attempt_started(start)
    }

    pub fn append_attempt_finished(
        &mut self,
        admission: StateWriteAdmission,
        finish: AttemptFinish,
    ) -> Result<(), StateError> {
        let record = self
            .repo
            .find_attempt(finish.attempt_id())?
            .ok_or_else(|| refused(StateWriteRefusalV1::UnknownSubject))?;
        self.check(
            admission,
            StateWriteOperationV1::Attempt,
            Bound::graph(record.project_id(), record.graph_id()),
        )?;
        self.repo.append_attempt_finished(finish)
    }

    pub fn append_checkpoint(
        &mut self,
        admission: StateWriteAdmission,
        value: CheckpointRecord,
    ) -> Result<(), StateError> {
        self.check(
            admission,
            StateWriteOperationV1::Checkpoint,
            Bound::project(value.project_id()),
        )?;
        self.repo.append_checkpoint(value)
    }

    pub fn append_output_ref(
        &mut self,
        admission: StateWriteAdmission,
        value: OutputRef,
    ) -> Result<(), StateError> {
        self.check(
            admission,
            StateWriteOperationV1::OutputRef,
            Bound::project(value.project_id()),
        )?;
        self.repo.append_output_ref(value)
    }

    // ---- decisions and provenance --------------------------------------------

    pub fn append_decision_record(
        &mut self,
        admission: StateWriteAdmission,
        record: &DecisionRecordV1,
    ) -> Result<(), StateError> {
        self.check(
            admission,
            StateWriteOperationV1::DecisionRecord,
            Bound::project(record.project_id()),
        )?;
        self.repo.append_decision_record(record)
    }

    pub fn append_provenance_link(
        &mut self,
        admission: StateWriteAdmission,
        project_id: &str,
        task_id: &str,
        kind: ProvenanceLinkKind,
        value: &str,
    ) -> Result<(), StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ProvenanceLink,
            Bound::project(project_id),
        )?;
        self.repo
            .append_provenance_link(project_id, task_id, kind, value)
    }

    // ---- events ----------------------------------------------------------------

    /// Persists one validated proposal as an event plus its receipt, atomically.
    /// `event_id` is minted by the trusted core; `occurred_at` is trusted time.
    pub fn persist_event_proposal(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        proposal: &EventProposalV1,
        event_id: &str,
    ) -> Result<EventPersistReceiptV1, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::EventProposal,
            Bound::project(proposal.project_id()),
        )?;
        let store_id = self.store_id.clone();
        self.repo
            .persist_event_proposal(clock, proposal, event_id, &store_id)
    }

    // ---- context -------------------------------------------------------------

    pub fn append_context_epoch(
        &mut self,
        admission: StateWriteAdmission,
        context_epoch: ContextEpoch,
    ) -> Result<(), StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ContextEpoch,
            Bound::project(&context_epoch.project_id),
        )?;
        self.repo.append_context_epoch(context_epoch)
    }

    pub fn append_context_epoch_with_changed_sources(
        &mut self,
        admission: StateWriteAdmission,
        context_epoch: ContextEpoch,
        changed_sources: ChangedSources,
    ) -> Result<(), StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ContextEpoch,
            Bound::project(&context_epoch.project_id),
        )?;
        self.repo
            .append_context_epoch_with_changed_sources(context_epoch, changed_sources)
    }

    pub fn advance_context_epoch(
        &mut self,
        admission: StateWriteAdmission,
        project_id: &str,
        advanced_at: &str,
        trigger: ContextEpochTrigger,
        invalidated_role_ids: &[String],
    ) -> Result<ContextEpoch, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ContextEpoch,
            Bound::project(project_id),
        )?;
        self.repo
            .advance_context_epoch(project_id, advanced_at, trigger, invalidated_role_ids)
    }

    pub fn advance_context_epoch_with_changed_sources(
        &mut self,
        admission: StateWriteAdmission,
        project_id: &str,
        advanced_at: &str,
        trigger: ContextEpochTrigger,
        invalidated_role_ids: &[String],
        changed_sources: ChangedSources,
    ) -> Result<ContextEpoch, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ContextEpoch,
            Bound::project(project_id),
        )?;
        self.repo.advance_context_epoch_with_changed_sources(
            project_id,
            advanced_at,
            trigger,
            invalidated_role_ids,
            changed_sources,
        )
    }

    pub fn create_context_manifest(
        &mut self,
        admission: StateWriteAdmission,
        manifest: ContextManifest,
    ) -> Result<(), StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ContextManifest,
            Bound::project(&manifest.project_id),
        )?;
        self.repo.create_context_manifest(manifest)
    }

    pub fn rehydrate_context(
        &mut self,
        admission: StateWriteAdmission,
        request: ContextRehydrationRequest,
        repository_materializer: &mut impl RepositorySnapshotMaterializer,
        artifact_materializer: &mut impl ArtifactMaterializer,
        event_supplier: &mut impl ContextRehydratedEventSupplier,
    ) -> Result<ContextRehydrationOutcome, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ContextRehydration,
            Bound::project(&request.project_id),
        )?;
        self.repo.rehydrate_context(
            request,
            repository_materializer,
            artifact_materializer,
            event_supplier,
        )
    }

    // ---- roles and bindings ----------------------------------------------------

    pub fn create_logical_role(
        &mut self,
        admission: StateWriteAdmission,
        role: LogicalRole,
    ) -> Result<(), StateError> {
        self.check(
            admission,
            StateWriteOperationV1::LogicalRole,
            Bound::project(&role.project_id),
        )?;
        self.repo.create_logical_role(role)
    }

    pub fn transition_logical_role_status(
        &mut self,
        admission: StateWriteAdmission,
        role_id: &str,
        expected: LogicalRoleStatus,
        new: LogicalRoleStatus,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), StateError> {
        let project = self.role_project(role_id)?;
        self.check(
            admission,
            StateWriteOperationV1::LogicalRole,
            Bound::project(&project),
        )?;
        self.repo
            .transition_logical_role_status(role_id, expected, new, evidence)
    }

    pub fn set_logical_role_active_binding(
        &mut self,
        admission: StateWriteAdmission,
        role_id: &str,
        expected: Option<&str>,
        new: Option<&str>,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), StateError> {
        let project = self.role_project(role_id)?;
        self.check(
            admission,
            StateWriteOperationV1::LogicalRole,
            Bound::project(&project),
        )?;
        self.repo
            .set_logical_role_active_binding(role_id, expected, new, evidence)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_logical_role_context(
        &mut self,
        admission: StateWriteAdmission,
        role_id: &str,
        expected_epoch: &StateEpochValueV1,
        expected_manifest_id: Option<&str>,
        new_epoch: &StateEpochValueV1,
        new_manifest_id: Option<&str>,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), StateError> {
        let project = self.role_project(role_id)?;
        self.check(
            admission,
            StateWriteOperationV1::LogicalRole,
            Bound::project(&project),
        )?;
        self.repo.set_logical_role_context(
            role_id,
            expected_epoch,
            expected_manifest_id,
            new_epoch,
            new_manifest_id,
            evidence,
        )
    }

    pub fn supersede_context_manifest(
        &mut self,
        admission: StateWriteAdmission,
        role_id: &str,
        expected_epoch: &StateEpochValueV1,
        expected_manifest_id: &str,
        manifest: ContextManifest,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), StateError> {
        let project = self.role_project(role_id)?;
        self.check(
            admission,
            StateWriteOperationV1::LogicalRole,
            Bound::project(&project),
        )?;
        self.repo.supersede_context_manifest(
            role_id,
            expected_epoch,
            expected_manifest_id,
            manifest,
            evidence,
        )
    }

    pub fn create_executor_binding(
        &mut self,
        admission: StateWriteAdmission,
        binding: ExecutorBinding,
    ) -> Result<(), StateError> {
        let project = self.role_project(&binding.role_id)?;
        self.check(
            admission,
            StateWriteOperationV1::ExecutorBinding,
            Bound::project(&project),
        )?;
        self.repo.create_executor_binding(binding)
    }

    pub fn release_executor_binding(
        &mut self,
        admission: StateWriteAdmission,
        binding_id: &str,
        released_at: &str,
        release_reason: ReleaseReason,
    ) -> Result<(), StateError> {
        let project = self.binding_project(binding_id)?;
        self.check(
            admission,
            StateWriteOperationV1::ExecutorBinding,
            Bound::project(&project),
        )?;
        self.repo
            .release_executor_binding(binding_id, released_at, release_reason)
    }

    pub fn expire_executor_binding_lease(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        request: ExecutorLeaseExpiryRequestV2,
    ) -> Result<ExecutorLeaseExpiryOutcomeV1, StateError> {
        let project = self.binding_project(&request.binding_id)?;
        self.check(
            admission,
            StateWriteOperationV1::ExecutorBinding,
            Bound::project(&project),
        )?;
        self.repo.expire_executor_binding_lease(clock, request)
    }

    pub fn renew_executor_binding_lease(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        binding_id: &str,
        lease_expires_at: &str,
    ) -> Result<(), StateError> {
        let project = self.binding_project(binding_id)?;
        self.check(
            admission,
            StateWriteOperationV1::ExecutorBinding,
            Bound::project(&project),
        )?;
        self.repo
            .renew_executor_binding_lease(clock, binding_id, lease_expires_at)
    }

    // ---- availability (D5) -----------------------------------------------------

    /// Appends one durable availability fact with a State-assigned monotonic
    /// sequence. See [`crate::availability_facts`] for the promotion rules.
    pub fn append_availability_fact(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        fact: &AvailabilityFactV1,
    ) -> Result<AvailabilityFactReceiptV1, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::AvailabilityFact,
            Bound::project(fact.project_id()),
        )?;
        self.repo.append_availability_fact(clock, fact)
    }
}

/// The bindings an operation's actual payload carries.
struct Bound<'a> {
    project: &'a str,
    graph: Option<&'a str>,
    source: Option<&'a str>,
    compiler: Option<&'a str>,
    policy: Option<&'a str>,
}

impl<'a> Bound<'a> {
    fn project(project: &'a str) -> Self {
        Self {
            project,
            graph: None,
            source: None,
            compiler: None,
            policy: None,
        }
    }
    fn graph(project: &'a str, graph: &'a str) -> Self {
        Self {
            graph: Some(graph),
            ..Self::project(project)
        }
    }
}

#[cfg(unix)]
mod secure {
    use super::{File, OpenOptions, Path, PathBuf, StateError, StateWriteRefusalV1 as R, refused};
    use std::os::raw::c_int;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::os::unix::io::AsRawFd;

    unsafe extern "C" {
        fn geteuid() -> u32;
        fn flock(fd: c_int, operation: c_int) -> c_int;
    }
    const LOCK_EX: c_int = 2;
    const LOCK_NB: c_int = 4;

    fn euid() -> u32 {
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { geteuid() }
    }

    fn io(error: std::io::Error) -> StateError {
        StateError::OpenFailed {
            detail: error.kind().to_string(),
        }
    }

    fn private(meta: &std::fs::Metadata) -> Result<(), StateError> {
        if meta.uid() != euid() {
            return Err(refused(R::InsecureOwnership));
        }
        if meta.mode() & 0o077 != 0 {
            return Err(refused(R::InsecureMode));
        }
        Ok(())
    }

    /// Validates an existing private regular file at `path` and opens it,
    /// proving the opened object is the validated one.
    fn open_regular(path: &Path, write: bool) -> Result<File, StateError> {
        let before = std::fs::symlink_metadata(path).map_err(io)?;
        if !before.file_type().is_file() || before.nlink() != 1 {
            return Err(refused(R::InsecurePath));
        }
        private(&before)?;
        let file = OpenOptions::new()
            .read(true)
            .write(write)
            .open(path)
            .map_err(io)?;
        let opened = file.metadata().map_err(io)?;
        if (opened.dev(), opened.ino()) != (before.dev(), before.ino()) {
            return Err(refused(R::IdentityRace));
        }
        Ok(file)
    }

    fn create_private(path: &Path) -> Result<File, StateError> {
        // O_CREAT|O_EXCL never follows a symlink and fails if anything exists.
        OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(io)
    }

    fn open_or_create(path: &Path, create: bool) -> Result<File, StateError> {
        match std::fs::symlink_metadata(path) {
            Ok(_) => open_regular(path, true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => create_private(path),
            Err(e) => Err(io(e)),
        }
    }

    pub(super) fn prepare(path: &Path, create: bool) -> Result<(PathBuf, File, File), StateError> {
        if !path.is_absolute() {
            return Err(refused(R::InsecurePath));
        }
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(refused(R::InsecurePath));
        };
        // Ancestors are resolved once; the State directory itself must be a
        // private real directory owned by this user.
        let parent = parent.canonicalize().map_err(io)?;
        let dir = std::fs::symlink_metadata(&parent).map_err(io)?;
        if !dir.file_type().is_dir() {
            return Err(refused(R::InsecurePath));
        }
        private(&dir)?;
        let db_path = parent.join(name);
        let mut lock_name = name.to_os_string();
        lock_name.push(".writer-lock");
        let lock = open_or_create(&parent.join(lock_name), true)?;
        // SAFETY: flock on an owned, open descriptor.
        if unsafe { flock(lock.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {
            return Err(refused(R::WriterLockHeld));
        }
        let db = open_or_create(&db_path, create)?;
        Ok((db_path, lock, db))
    }

    pub(super) fn verify_same_object(path: &Path, db: &File) -> Result<(), StateError> {
        let now = std::fs::symlink_metadata(path).map_err(io)?;
        let held = db.metadata().map_err(io)?;
        if !now.file_type().is_file() || (now.dev(), now.ino()) != (held.dev(), held.ino()) {
            return Err(refused(R::IdentityRace));
        }
        private(&now)
    }

    pub(super) fn verify_sidecars(path: &Path) -> Result<(), StateError> {
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            match std::fs::symlink_metadata(PathBuf::from(name)) {
                Ok(meta) => {
                    if !meta.file_type().is_file() {
                        return Err(refused(R::InsecurePath));
                    }
                    private(&meta)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(io(e)),
            }
        }
        Ok(())
    }
}

#[cfg(not(unix))]
mod secure {
    use super::{File, Path, PathBuf, StateError, StateWriteRefusalV1 as R, refused};
    pub(super) fn prepare(_: &Path, _: bool) -> Result<(PathBuf, File, File), StateError> {
        Err(refused(R::UnsupportedPlatform))
    }
    pub(super) fn verify_same_object(_: &Path, _: &File) -> Result<(), StateError> {
        Err(refused(R::UnsupportedPlatform))
    }
    pub(super) fn verify_sidecars(_: &Path) -> Result<(), StateError> {
        Err(refused(R::UnsupportedPlatform))
    }
}
