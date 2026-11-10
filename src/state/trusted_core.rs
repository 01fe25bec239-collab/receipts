//! The trusted-core State write boundary.
//!
//! [`TrustedCoreStore`] is the only public way to obtain State write access.
//! Every mutation consumes one [`StateWriteAdmission`]: a non-cloneable value
//! that only the store's holder can mint, bound to the issuing live store
//! instance (a private per-open identity, so a copied database file or a
//! closed and reopened store never accepts it) as well as this store's
//! durable identity, one closed operation family, the project and, where the
//! payload carries them, the graph, admitted source revision, compiler and
//! policy.
//! State re-checks each binding against the actual payload (or the durable
//! record it targets) before writing; a mismatch writes nothing.
//!
//! Limits (deliberate, documented): this is an object-capability boundary
//! inside the trusted core process. It does not authenticate arbitrary code in
//! the same address space, and file modes plus a cooperative `flock` do not
//! exclude another process running with the same filesystem authority. Actual
//! exclusion of lower-trust principals (separate identity, no access to the
//! State directory, no inherited descriptor or writer channel) is a deployment
//! requirement that these library checks cannot prove. The durable
//! `store_id` (also written into event receipts) is copyable logical history,
//! not a physical attestation: a copied file carries the same value, and
//! receipts are local unsigned records.
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
//!     receipts_state::StateWriteAdmission { store_id: String::new(), instance: 0, subject: s }
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
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::atomic_publication::{
    DecisionPublicationReceiptV1, DecisionPublicationV1, GraphMutationPublicationV1,
    GraphPublicationOutcomeV1,
};
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
    ExecutorReleasedLifecycleAuthorityV1,
};
use crate::graph_history::StateGraphMutationV1;
use crate::graph_node_result::StateGraphNodeResultV1;
use crate::graph_repository::{StateGraphGenesisV1, StateGraphSnapshotV1};
use crate::logical_role::{LogicalRole, LogicalRoleStatus};
use crate::logical_role_lifecycle::RoleChangeEvidenceV1;
use crate::output_ref::OutputRef;
use crate::recovery::{
    ChangedSourceSuccessionRequestV1, ChangedSourceSuccessionV1, RehydrationRequiredV1,
    RoleRebindRequestV1, StartupRecoveryReportV1,
};
use crate::repository::SqliteStateRepository;
use crate::review_claim::{
    ReviewClaimAbandonV1, ReviewClaimOutcomeV1, ReviewClaimRecordV1, ReviewClaimRequestV1,
    ReviewClaimSettlementV1, ReviewClaimTicketV1, ReviewSealRecordV1, ReviewSealRequestV1,
};
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
    /// One atomic decision publication package (records, events, facts).
    DecisionPublication,
    /// Durable review claims (claim, settle, abandon).
    ReviewClaim,
}

/// Why State refused a write admission or an exclusive open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateWriteRefusalV1 {
    /// The admission was minted by another store or another open instance
    /// of this store (including a copy sharing its durable identity).
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
    /// Directory or file is not owned by the effective user (ancestors: not
    /// owned by the effective user or root).
    InsecureOwnership,
    /// Directory or file is accessible to group/other, or an ancestor
    /// directory is writable by another principal without the sticky-bit
    /// protection of an entry owned by this user or root.
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
    instance: u64,
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
    // Fields drop in declaration order: SQLite must close before WriterLock unlocks.
    repo: SqliteStateRepository,
    store_id: String,
    /// Process-unique identity of this open writer; never reused, never
    /// persisted, so admissions cannot outlive or leave this instance.
    instance: u64,
    // Held for the store's lifetime: the cooperative exclusive-writer lock,
    // the validated database object and its validated directory.
    _lock: secure::WriterLock,
    _db: File,
    _dir: File,
}

fn next_instance() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    // ponytail: a process-local counter; admissions are in-memory only.
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
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
        let held = secure::prepare(path.as_ref(), true)?;
        let repo = SqliteStateRepository::open(&held.db_path)?;
        Self::finish(repo, held)
    }

    /// Explicitly upgrades an existing v11–v21 store to the current schema and
    /// opens it, under the same ownership/lock checks. Returns the new head.
    pub fn migrate_exclusive(path: impl AsRef<Path>) -> Result<(Self, u32), StateError> {
        let held = secure::prepare(path.as_ref(), false)?;
        let version = SqliteStateRepository::migrate_existing_to_current(&held.db_path)?;
        let repo = SqliteStateRepository::open(&held.db_path)?;
        Ok((Self::finish(repo, held)?, version))
    }

    fn finish(repo: SqliteStateRepository, held: secure::Held) -> Result<Self, StateError> {
        let store_id = match secure::verify_after_open(&held).and_then(|()| repo.store_identity()) {
            Ok(id) => id,
            Err(error) => {
                // Parameters drop in reverse order; close SQLite before releasing held.
                drop(repo);
                return Err(error);
            }
        };
        Ok(Self {
            repo,
            store_id,
            instance: next_instance(),
            _lock: held.lock,
            _db: held.db,
            _dir: held.dir,
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
            instance: self.instance,
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
        // Physical instance first: equal durable UUIDs (a copied file, or the
        // same file reopened) never transfer an admission.
        if admission.instance != self.instance || admission.store_id != self.store_id {
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

    // ---- recovery (ST-W2) -------------------------------------------------------

    /// Startup recovery scan for one project (ExecutorBinding family). See
    /// [`crate::recovery`]: expired leases are released with their events,
    /// every role's continuation requirement and the unfinished attempts are
    /// reported from the same snapshot.
    pub fn recover_startup(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        project_id: &str,
        authority: &ExecutorReleasedLifecycleAuthorityV1,
    ) -> Result<StartupRecoveryReportV1, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ExecutorBinding,
            Bound::project(project_id),
        )?;
        self.repo.recover_startup(clock, project_id, authority)
    }

    /// Atomic expected-current-CAS failover rebind (ExecutorBinding family);
    /// the result names the rehydration the new executor must complete first.
    pub fn rebind_role(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        request: &RoleRebindRequestV1,
    ) -> Result<RehydrationRequiredV1, StateError> {
        let project = self.role_project(&request.role_id)?;
        self.check(
            admission,
            StateWriteOperationV1::ExecutorBinding,
            Bound::project(&project),
        )?;
        self.repo.rebind_role(clock, request)
    }

    /// Changed-source epoch/successor transaction (ContextEpoch family).
    pub fn succeed_changed_context_sources(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        request: &ChangedSourceSuccessionRequestV1,
    ) -> Result<ChangedSourceSuccessionV1, StateError> {
        let project = self.role_project(&request.role_id)?;
        self.check(
            admission,
            StateWriteOperationV1::ContextEpoch,
            Bound::project(&project),
        )?;
        self.repo.succeed_changed_context_sources(clock, request)
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

    // ---- shared atomic publication and review claims ----------------------------

    /// Publishes a graph mutation together with the node results it attaches,
    /// its snapshot, history, seal, watermark and head CAS in one transaction
    /// (GraphMutation family; the same project/graph/source/compiler/policy
    /// binding as [`Self::append_graph_mutation_v1`]). Any failure writes
    /// nothing. See [`crate::atomic_publication`].
    pub fn publish_graph_mutation_v1(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        publication: &GraphMutationPublicationV1,
    ) -> Result<GraphPublicationOutcomeV1, StateError> {
        let content = publication.snapshot().content();
        let provenance = content.genesis_provenance();
        self.check(
            admission,
            StateWriteOperationV1::GraphMutation,
            Bound {
                project: content.project_id(),
                graph: Some(publication.graph_id()),
                source: Some(provenance.source_ref().as_str()),
                compiler: Some(provenance.compiler_id().as_str()),
                policy: Some(content.policy_id()),
            },
        )?;
        self.repo.publish_graph_mutation_v1(clock, publication)
    }

    /// Publishes one decision package (records, events with receipts,
    /// availability facts and an optional claim settlement) atomically
    /// (DecisionPublication family, bound to the package's project). Exact
    /// retries return the recorded outcome. See [`crate::atomic_publication`].
    pub fn publish_decision_package_v1(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        package: &DecisionPublicationV1,
    ) -> Result<DecisionPublicationReceiptV1, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::DecisionPublication,
            Bound::project(package.project_id()),
        )?;
        if let Some((store_id, instance)) = package.claim_binding()
            && (store_id != self.store_id || instance != self.instance)
        {
            return Err(refused(StateWriteRefusalV1::WrongStore));
        }
        let store_id = self.store_id.clone();
        self.repo
            .publish_decision_package_v1(clock, package, &store_id)
    }

    /// Claims one unit of review work BEFORE it is routed or executed. Only
    /// [`ReviewClaimOutcomeV1::Claimed`] permits execution. See
    /// [`crate::review_claim`].
    pub fn claim_review_v1(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        request: &ReviewClaimRequestV1,
    ) -> Result<ReviewClaimOutcomeV1, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ReviewClaim,
            Bound::project(request.project_id()),
        )?;
        let (inserted, record) = self.repo.claim_review_v1(clock, request)?;
        Ok(if inserted {
            ReviewClaimOutcomeV1::Claimed(ReviewClaimTicketV1 {
                store_id: self.store_id.clone(),
                instance: self.instance,
                project_id: record.project_id,
                claim_id: record.claim_id,
            })
        } else {
            ReviewClaimOutcomeV1::Existing(record)
        })
    }

    /// Settles the claim this live store instance issued `ticket` for.
    pub fn settle_review_claim_v1(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        ticket: &ReviewClaimTicketV1,
        settlement: &ReviewClaimSettlementV1,
    ) -> Result<ReviewClaimRecordV1, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ReviewClaim,
            Bound::project(&ticket.project_id),
        )?;
        if ticket.store_id != self.store_id || ticket.instance != self.instance {
            return Err(refused(StateWriteRefusalV1::WrongStore));
        }
        self.repo
            .settle_review_claim_v1(clock, &ticket.project_id, &ticket.claim_id, settlement)
    }

    /// Durably seals the claimant's exact observed result BEFORE anything is
    /// published. Only the live claimant (the `ticket` this instance issued)
    /// can create the initial seal; an identical reseal returns the stored
    /// seal and different content refuses. See [`crate::review_claim`].
    pub fn seal_review_claim_v1(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        ticket: &ReviewClaimTicketV1,
        request: &ReviewSealRequestV1,
    ) -> Result<ReviewSealRecordV1, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ReviewClaim,
            Bound::project(&ticket.project_id),
        )?;
        if ticket.store_id != self.store_id || ticket.instance != self.instance {
            return Err(refused(StateWriteRefusalV1::WrongStore));
        }
        if request.project_id() != ticket.project_id || request.claim_id() != ticket.claim_id {
            return Err(StateError::Publication {
                code: crate::PublicationFailureCode::InvalidShape,
                kind: "review_claim",
            });
        }
        if request.ticket_binding() != (self.store_id.as_str(), self.instance) {
            return Err(refused(StateWriteRefusalV1::WrongStore));
        }
        self.repo.seal_review_claim_v1(clock, request)
    }

    /// Records the explicit, authority-named abandonment of an unfinished
    /// claim. Never automatic; it re-runs nothing.
    pub fn abandon_review_claim_v1(
        &mut self,
        admission: StateWriteAdmission,
        clock: &dyn TrustedClockV1,
        request: &ReviewClaimAbandonV1,
    ) -> Result<ReviewClaimRecordV1, StateError> {
        self.check(
            admission,
            StateWriteOperationV1::ReviewClaim,
            Bound::project(request.project_id()),
        )?;
        self.repo.abandon_review_claim_v1(clock, request)
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

/// Secure open, anchored on directory descriptors.
///
/// The State directory is reached by walking its canonical path from `/`
/// with `openat(O_DIRECTORY|O_NOFOLLOW)`, validating each held directory
/// descriptor: owned by root or the effective user, and not writable by
/// group/other unless sticky with the next entry owned by root or this user.
/// The State directory itself must be private and owned by this user. The
/// lock, database and any pre-existing `-wal`/`-shm`/`-journal` sidecars are
/// examined with `fstatat(AT_SYMLINK_NOFOLLOW)` relative to that descriptor
/// before anything opens them, so a substituted FIFO/device/symlink is
/// refused without being opened; regular files are then opened with
/// `O_NOFOLLOW|O_NONBLOCK` and their identity re-compared. SQLite itself opens
/// by pathname; that is safe only because the validated chain cannot be
/// altered by other principals, and the directory and database identities
/// plus the sidecars are re-checked after SQLite opens. Same-user hostile
/// code is outside this model.
#[cfg(unix)]
mod secure {
    use super::{File, Path, PathBuf, StateError, StateWriteRefusalV1 as R, refused};
    use std::ffi::{CString, OsStr};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::io::{AsRawFd, FromRawFd};

    pub(super) struct WriterLock {
        file: File,
        owner: libc::pid_t,
    }

    impl Drop for WriterLock {
        fn drop(&mut self) {
            // A forked copy must neither prolong ownership nor unlock a live parent.
            // SAFETY: owned, open descriptor; getpid and flock have no pointer arguments.
            unsafe {
                if libc::getpid() == self.owner {
                    libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
                }
            }
        }
    }

    pub(super) struct Held {
        pub(super) db_path: PathBuf,
        pub(super) lock: WriterLock,
        pub(super) db: File,
        pub(super) dir: File,
    }

    fn euid() -> u32 {
        // SAFETY: geteuid has no preconditions and cannot fail.
        unsafe { libc::geteuid() }
    }

    fn io(error: std::io::Error) -> StateError {
        StateError::OpenFailed {
            detail: error.kind().to_string(),
        }
    }

    fn name(component: &OsStr) -> Result<CString, StateError> {
        CString::new(component.as_bytes()).map_err(|_| refused(R::InsecurePath))
    }

    /// The facts checked for every object, from `fstat` or `fstatat`.
    #[derive(PartialEq)]
    struct Facts {
        kind: libc::mode_t,
        uid: u32,
        mode: u32,
        nlink: u64,
        dev: u64,
        ino: u64,
    }

    impl Facts {
        fn of(file: &File) -> Result<Self, StateError> {
            let m = file.metadata().map_err(io)?;
            // MetadataExt uses u32; libc mode_t is u16 on macOS and u32 on Linux.
            #[allow(clippy::unnecessary_cast)]
            let mode = m.mode() as libc::mode_t;
            Ok(Self {
                kind: mode & libc::S_IFMT,
                uid: m.uid(),
                mode: m.mode() & 0o7777,
                nlink: m.nlink(),
                dev: m.dev(),
                ino: m.ino(),
            })
        }
        fn regular(&self) -> bool {
            self.kind == libc::S_IFREG
        }
        fn same_object(&self, other: &Self) -> bool {
            (self.dev, self.ino) == (other.dev, other.ino)
        }
        fn private(&self) -> Result<(), StateError> {
            if self.uid != euid() {
                return Err(refused(R::InsecureOwnership));
            }
            if self.mode & 0o077 != 0 {
                return Err(refused(R::InsecureMode));
            }
            Ok(())
        }
        /// A private, singly linked regular file owned by this user.
        fn private_regular(&self) -> Result<(), StateError> {
            if !self.regular() || self.nlink != 1 {
                return Err(refused(R::InsecurePath));
            }
            self.private()
        }
    }

    /// `fstatat(dir, name, AT_SYMLINK_NOFOLLOW)`; `None` when absent.
    fn stat_at(dir: &File, entry: &CString) -> Result<Option<Facts>, StateError> {
        let mut st = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: valid descriptor, NUL-terminated name, writable stat buffer.
        let rc = unsafe {
            libc::fstatat(
                dir.as_raw_fd(),
                entry.as_ptr(),
                st.as_mut_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if rc != 0 {
            let error = std::io::Error::last_os_error();
            return match error.kind() {
                std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(io(error)),
            };
        }
        // SAFETY: fstatat succeeded and initialised the buffer.
        let st = unsafe { st.assume_init() };
        // Field widths differ between platforms; the casts are not redundant everywhere.
        #[allow(clippy::unnecessary_cast)]
        Ok(Some(Facts {
            kind: st.st_mode & libc::S_IFMT,
            uid: st.st_uid,
            mode: (st.st_mode & 0o7777) as u32,
            nlink: st.st_nlink as u64,
            dev: st.st_dev as u64,
            ino: st.st_ino as u64,
        }))
    }

    fn open_at(
        dir: Option<&File>,
        entry: &CString,
        flags: libc::c_int,
    ) -> Result<File, StateError> {
        let base = dir.map_or(libc::AT_FDCWD, AsRawFd::as_raw_fd);
        // SAFETY: valid descriptor (or AT_FDCWD), NUL-terminated name; mode
        // is used only with O_CREAT.
        let fd = unsafe {
            libc::openat(
                base,
                entry.as_ptr(),
                flags | libc::O_CLOEXEC | libc::O_NOFOLLOW,
                0o600 as libc::c_uint,
            )
        };
        if fd < 0 {
            let error = std::io::Error::last_os_error();
            // ELOOP: the final component is a symlink.
            return Err(match error.raw_os_error() {
                Some(libc::ELOOP) | Some(libc::ENOTDIR) => refused(R::InsecurePath),
                _ => io(error),
            });
        }
        // SAFETY: fd is a freshly opened descriptor we now own.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    /// An ancestor may be shared only if no other principal can replace the
    /// entry below it.
    fn protected_ancestor(dir: &Facts, child: &Facts) -> Result<(), StateError> {
        if dir.kind != libc::S_IFDIR {
            return Err(refused(R::InsecurePath));
        }
        if dir.uid != 0 && dir.uid != euid() {
            return Err(refused(R::InsecureOwnership));
        }
        let sticky_protects = dir.mode & 0o1000 != 0 && (child.uid == 0 || child.uid == euid());
        if dir.mode & 0o022 != 0 && !sticky_protects {
            return Err(refused(R::InsecureMode));
        }
        Ok(())
    }

    fn walk(parent: &Path) -> Result<File, StateError> {
        let mut dir = open_at(
            None,
            &name(OsStr::new("/"))?,
            libc::O_RDONLY | libc::O_DIRECTORY,
        )?;
        for component in parent.components().skip(1) {
            let entry = name(component.as_os_str())?;
            let child = open_at(Some(&dir), &entry, libc::O_RDONLY | libc::O_DIRECTORY)?;
            protected_ancestor(&Facts::of(&dir)?, &Facts::of(&child)?)?;
            dir = child;
        }
        let facts = Facts::of(&dir)?;
        if facts.kind != libc::S_IFDIR {
            return Err(refused(R::InsecurePath));
        }
        facts.private()?;
        Ok(dir)
    }

    /// Opens (or, when `create`, exclusively creates) one private regular
    /// file below `dir`, examining any existing entry before opening it.
    fn open_file(dir: &File, entry: &CString, create: bool) -> Result<File, StateError> {
        let flags = libc::O_RDWR | libc::O_NONBLOCK;
        let file = match stat_at(dir, entry)? {
            Some(before) => {
                before.private_regular()?;
                let file = open_at(Some(dir), entry, flags)?;
                if !Facts::of(&file)?.same_object(&before) {
                    return Err(refused(R::IdentityRace));
                }
                file
            }
            None if create => open_at(Some(dir), entry, flags | libc::O_CREAT | libc::O_EXCL)?,
            None => {
                return Err(io(std::io::Error::from(std::io::ErrorKind::NotFound)));
            }
        };
        Facts::of(&file)?.private_regular()?;
        Ok(file)
    }

    fn sidecars(dir: &File, entry: &OsStr) -> Result<(), StateError> {
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut sidecar = entry.to_os_string();
            sidecar.push(suffix);
            if let Some(facts) = stat_at(dir, &name(&sidecar)?)? {
                facts.private_regular()?;
            }
        }
        Ok(())
    }

    pub(super) fn prepare(path: &Path, create: bool) -> Result<Held, StateError> {
        if !path.is_absolute() {
            return Err(refused(R::InsecurePath));
        }
        let (Some(parent), Some(entry)) = (path.parent(), path.file_name()) else {
            return Err(refused(R::InsecurePath));
        };
        // Resolve legitimate system aliases (such as /var -> /private/var)
        // once, then validate the real chain through held descriptors.
        let parent = parent.canonicalize().map_err(io)?;
        let dir = walk(&parent)?;
        let mut lock_name = entry.to_os_string();
        lock_name.push(".writer-lock");
        let lock = open_file(&dir, &name(&lock_name)?, true)?;
        // SAFETY: flock on an owned, open descriptor.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(refused(R::WriterLockHeld));
        }
        // Only a successfully acquired lock may unlock on drop, including failed opens.
        let lock = WriterLock {
            file: lock,
            // SAFETY: getpid has no preconditions.
            owner: unsafe { libc::getpid() },
        };
        // Pre-existing sidecars are validated before SQLite can act on them.
        sidecars(&dir, entry)?;
        let db = open_file(&dir, &name(entry)?, create)?;
        Ok(Held {
            db_path: parent.join(entry),
            lock,
            db,
            dir,
        })
    }

    /// After SQLite opened the pathname: the directory and database are the
    /// validated objects and every sidecar is still safe.
    pub(super) fn verify_after_open(held: &Held) -> Result<(), StateError> {
        let (Some(parent), Some(entry)) = (held.db_path.parent(), held.db_path.file_name()) else {
            return Err(refused(R::InsecurePath));
        };
        let dir_now = std::fs::symlink_metadata(parent).map_err(io)?;
        let dir_held = Facts::of(&held.dir)?;
        if !dir_now.file_type().is_dir()
            || (dir_now.dev(), dir_now.ino()) != (dir_held.dev, dir_held.ino)
        {
            return Err(refused(R::IdentityRace));
        }
        let now = stat_at(&held.dir, &name(entry)?)?.ok_or_else(|| refused(R::IdentityRace))?;
        if !now.same_object(&Facts::of(&held.db)?) {
            return Err(refused(R::IdentityRace));
        }
        now.private_regular()?;
        sidecars(&held.dir, entry)
    }
}

#[cfg(not(unix))]
mod secure {
    use super::{File, Path, PathBuf, StateError, StateWriteRefusalV1 as R, refused};
    pub(super) type WriterLock = File;
    pub(super) struct Held {
        pub(super) db_path: PathBuf,
        pub(super) lock: File,
        pub(super) db: File,
        pub(super) dir: File,
    }
    pub(super) fn prepare(_: &Path, _: bool) -> Result<Held, StateError> {
        Err(refused(R::UnsupportedPlatform))
    }
    pub(super) fn verify_after_open(_: &Held) -> Result<(), StateError> {
        Err(refused(R::UnsupportedPlatform))
    }
}
