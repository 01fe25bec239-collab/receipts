//! Trusted-core issuer for State writes.
//!
//! [`CoreStateWriter`] is the core's sole State writer object. It owns the
//! exclusive [`TrustedCoreStore`] and an immutable [`CoreAdmissionIdentity`]
//! supplied by the trusted composition: the admitted compiler and policy and,
//! per admitted project, the admitted source revision. Every State operation
//! family goes through it. For each write it resolves the subject from that
//! context and from the stored records the write targets (graph, attempt,
//! role, binding), never from a caller-supplied flag and never by copying a
//! value out of the payload to compare it back. It refuses non-admitted
//! projects, unknown graphs and foreign compiler/policy work, and only then
//! mints the single-use State admission. State independently re-checks the
//! physical store/project/graph/source/compiler/policy tuple against the
//! actual payload.
//!
//! Event proposals additionally have their referenced content resolved
//! through the composition's [`CoreContentResolverV1`] and its actual bytes
//! verified against the proposal's `sha256:` digest before anything is
//! persisted.
//!
//! Lower-trust components (Routing, Review, Runtime, Workspace, Host) never
//! receive this object: they hand the core immutable proposals. This is an
//! in-process object-capability boundary; its public methods do not exclude
//! arbitrary same-process or same-user code, and separating lower-trust
//! principals from the State files is a deployment requirement (see State's
//! `trusted_core` module limits).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use receipts_state::{
    ArtifactMaterializer, AttemptFinish, AttemptStart, AvailabilityFactReceiptV1,
    AvailabilityFactV1, ChangedSourceSuccessionRequestV1, ChangedSourceSuccessionV1,
    ChangedSources, CheckpointRecord, ContextEpoch, ContextEpochTrigger, ContextManifest,
    ContextRehydratedEventSupplier, ContextRehydrationOutcome, ContextRehydrationRequest,
    DecisionRecordV1, EventPersistReceiptV1, EventProposalV1, ExecutorBinding,
    ExecutorLeaseExpiryOutcomeV1, ExecutorLeaseExpiryRequestV2,
    ExecutorReleasedLifecycleAuthorityV1, GraphCompilerRefV1, GraphMutationPublicationV1,
    GraphPublicationOutcomeV1, GraphResultAttachmentV1, GraphSourceEvidenceRefV1, LogicalRole,
    LogicalRoleStatus, OutputRef, ProvenanceLinkKind, RehydrationRequiredV1, ReleaseReason,
    RepositorySnapshotMaterializer, ReviewClaimTicketV1, RoleChangeEvidenceV1, RoleRebindRequestV1,
    SqliteStateRepository, StartupRecoveryReportV1, StateEpochValueV1, StateError,
    StateGraphGenesisV1, StateGraphMutationV1, StateGraphNodeResultV1, StateGraphSnapshotV1,
    StateWriteAdmission, StateWriteOperationV1 as Op, StateWriteSubjectV1, TrustedClockV1,
    TrustedCoreStore,
};
use sha2::{Digest, Sha256};

/// The immutable admission context the trusted composition supplies: the
/// compiler and policy this core admits and, per admitted project, the
/// admitted source revision (a typed evidence reference, not a commit SHA).
///
/// Two identities are equal only when they are the same admitted object (or a
/// clone of it): the abandonment grants it mints (see
/// [`CoreAdmissionIdentity::abandonment_grant`]) are bound to that object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreAdmissionIdentity {
    compiler_id: GraphCompilerRefV1,
    policy_id: String,
    projects: BTreeMap<String, GraphSourceEvidenceRefV1>,
    /// Per project, the admitted authority that may abandon a review claim.
    abandonment: BTreeMap<String, String>,
    mark: IdentityMark,
}

/// Identity of one admitted-identity object; clones share it.
#[derive(Debug, Clone)]
pub(super) struct IdentityMark(Arc<()>);

impl PartialEq for IdentityMark {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for IdentityMark {}

impl CoreAdmissionIdentity {
    /// `policy_id` must be non-empty and at most 200 characters. No project
    /// is admitted until [`Self::with_project`] names it.
    pub fn new(compiler_id: GraphCompilerRefV1, policy_id: String) -> Option<Self> {
        (1..=200)
            .contains(&policy_id.chars().count())
            .then_some(Self {
                compiler_id,
                policy_id,
                projects: BTreeMap::new(),
                abandonment: BTreeMap::new(),
                mark: IdentityMark(Arc::new(())),
            })
    }

    /// Admits `project_id` with the source revision resolved for it by the
    /// trusted composition. New graphs in that project must cite exactly
    /// this source.
    pub fn with_project(
        mut self,
        project_id: impl Into<String>,
        admitted_source: GraphSourceEvidenceRefV1,
    ) -> Self {
        self.projects.insert(project_id.into(), admitted_source);
        self
    }

    /// Admits `authority_ref` (a bounded id such as `operator:<name>` or
    /// `policy:<name>`) as the authority that may abandon an unfinished review
    /// claim of `project_id`. Without it no abandonment is possible. The
    /// reference is recorded on the claim; the name alone never authorizes
    /// anything (see [`Self::abandonment_grant`]).
    pub fn with_abandonment_authority(
        mut self,
        project_id: impl Into<String>,
        authority_ref: impl Into<String>,
    ) -> Self {
        self.abandonment
            .insert(project_id.into(), authority_ref.into());
        self
    }

    /// Mints the single-use grant to abandon review claim `claim_id` of
    /// `project_id`, or `None` when the project has no admitted abandonment
    /// authority. Only code holding this admitted identity (or a clone of
    /// it, which the trusted composition keeps) can mint one; a writer opened
    /// with another identity refuses it.
    pub fn abandonment_grant(
        &self,
        project_id: &str,
        claim_id: &str,
    ) -> Option<CoreAbandonmentGrantV1> {
        self.abandonment
            .get(project_id)
            .map(|authority_ref| CoreAbandonmentGrantV1 {
                mark: self.mark.clone(),
                project_id: project_id.to_owned(),
                claim_id: claim_id.to_owned(),
                authority_ref: authority_ref.clone(),
            })
    }

    pub(super) fn abandonment_authority(&self, project_id: &str) -> Option<&str> {
        self.abandonment.get(project_id).map(String::as_str)
    }

    pub(super) fn owns(&self, mark: &IdentityMark) -> bool {
        self.mark == *mark
    }
}

/// Permission to abandon one review claim, minted by the admitted identity and
/// consumed by [`CoreStateWriter::abandon_review_claim`]. It is not `Clone`
/// and carries no caller-supplied authority string.
#[derive(Debug)]
pub struct CoreAbandonmentGrantV1 {
    pub(super) mark: IdentityMark,
    pub(super) project_id: String,
    pub(super) claim_id: String,
    pub(super) authority_ref: String,
}

/// The trusted composition's read-only access to content referenced by
/// event proposals. Implementations scope `reference` to the admitted
/// content of `project_id` (never an arbitrary filesystem path) and return
/// its exact bytes, or `None` when it is missing, inaccessible or belongs to
/// another project. The core hashes the bytes itself.
pub trait CoreContentResolverV1 {
    fn resolve(&self, project_id: &str, reference: &str) -> Option<Vec<u8>>;
}

/// Core-policy refusals, distinct from State's physical refusals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePolicyRefusal {
    /// The graph was compiled by a compiler this core does not admit.
    CompilerNotAdmitted,
    /// The graph names a policy this core does not admit.
    PolicyNotAdmitted,
    /// The write targets a project the composition did not admit.
    ProjectNotAdmitted,
    /// The targeted graph, attempt, role or binding is not stored (in the
    /// admitted project).
    UnknownSubject,
    /// The referenced content could not be resolved for this project.
    ContentUnavailable,
    /// The supplied digest is not a `sha256:<64 lowercase hex>` value.
    UnsupportedDigest,
    /// The resolved bytes do not match the supplied digest.
    ContentDigestMismatch,
    /// This writer holds no live ticket for the review claim (it was claimed
    /// by an earlier process or by someone else, or it is already finished).
    NoLiveClaim,
    /// The review claim has no durable seal to publish from.
    NotSealed,
    /// The recovery data does not decode, is of another kind or project, or
    /// does not rebuild exactly what was sealed.
    RecoveryDataInvalid,
    /// The claim may only be completed by publishing its sealed package.
    CompletionRequiresPublication,
    /// The grant was not minted by this writer's admitted identity, names
    /// another claim or project, or the project has no admitted authority.
    AbandonmentNotAuthorized,
    /// A package item or key is outside the bounded shape.
    InvalidShape,
}

#[derive(Debug)]
pub enum CoreStateWriteError {
    Policy(CorePolicyRefusal),
    State(StateError),
}

impl From<StateError> for CoreStateWriteError {
    fn from(error: StateError) -> Self {
        Self::State(error)
    }
}

impl From<CorePolicyRefusal> for CoreStateWriteError {
    fn from(refusal: CorePolicyRefusal) -> Self {
        Self::Policy(refusal)
    }
}

impl std::fmt::Display for CoreStateWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Policy(refusal) => write!(f, "core policy refused write: {refusal:?}"),
            Self::State(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CoreStateWriteError {}

pub(super) type Admitted = Result<StateWriteAdmission, CoreStateWriteError>;

/// The trusted core's State writer.
#[derive(Debug)]
pub struct CoreStateWriter {
    pub(super) store: TrustedCoreStore,
    pub(super) identity: CoreAdmissionIdentity,
    /// Live review-claim tickets by `(project, claim)`: non-`Clone`, never
    /// persisted, gone after a restart.
    pub(super) review_tickets: BTreeMap<(String, String), ReviewClaimTicketV1>,
}

impl CoreStateWriter {
    /// Opens the exclusive store (see `TrustedCoreStore::open_exclusive`).
    pub fn open(
        path: impl AsRef<Path>,
        identity: CoreAdmissionIdentity,
    ) -> Result<Self, StateError> {
        Ok(Self {
            store: TrustedCoreStore::open_exclusive(path)?,
            identity,
            review_tickets: BTreeMap::new(),
        })
    }

    pub fn store_id(&self) -> &str {
        self.store.store_id()
    }

    /// Read access for the core; reads never write.
    pub fn reader(&self) -> &SqliteStateRepository {
        self.store.reader()
    }

    pub(super) fn admitted_source(
        &self,
        project_id: &str,
    ) -> Result<&GraphSourceEvidenceRefV1, CorePolicyRefusal> {
        self.identity
            .projects
            .get(project_id)
            .ok_or(CorePolicyRefusal::ProjectNotAdmitted)
    }

    fn admit_graph_content(
        &self,
        operation: Op,
        content: &StateGraphGenesisV1,
        graph_id: &str,
        source: &GraphSourceEvidenceRefV1,
    ) -> Admitted {
        let provenance = content.genesis_provenance();
        if provenance.compiler_id() != &self.identity.compiler_id {
            return Err(CorePolicyRefusal::CompilerNotAdmitted.into());
        }
        if content.policy_id() != self.identity.policy_id {
            return Err(CorePolicyRefusal::PolicyNotAdmitted.into());
        }
        Ok(self.store.admit(StateWriteSubjectV1 {
            operation,
            project_id: content.project_id().to_owned(),
            graph_id: Some(graph_id.to_owned()),
            // From the trusted context or the stored graph, never the payload.
            source_ref: Some(source.as_str().to_owned()),
            compiler_id: Some(self.identity.compiler_id.as_str().to_owned()),
            policy_id: Some(self.identity.policy_id.clone()),
        }))
    }

    /// Admits a project-scoped write; a named graph must already be stored
    /// in that project.
    pub(super) fn admit(
        &self,
        operation: Op,
        project_id: &str,
        graph_id: Option<&str>,
    ) -> Admitted {
        self.admitted_source(project_id)?;
        if let Some(graph) = graph_id
            && self
                .reader()
                .load_current_graph_v1(project_id, graph)?
                .is_none()
        {
            return Err(CorePolicyRefusal::UnknownSubject.into());
        }
        Ok(self.store.admit(StateWriteSubjectV1 {
            operation,
            project_id: project_id.to_owned(),
            graph_id: graph_id.map(str::to_owned),
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        }))
    }

    fn role_project(&self, role_id: &str) -> Result<String, CoreStateWriteError> {
        Ok(self
            .reader()
            .find_logical_role(role_id)?
            .ok_or(CorePolicyRefusal::UnknownSubject)?
            .project_id)
    }

    fn admit_role(&self, operation: Op, role_id: &str) -> Admitted {
        let project = self.role_project(role_id)?;
        self.admit(operation, &project, None)
    }

    fn admit_binding(&self, binding_id: &str) -> Admitted {
        let binding = self
            .reader()
            .find_executor_binding(binding_id)?
            .ok_or(CorePolicyRefusal::UnknownSubject)?;
        self.admit_role(Op::ExecutorBinding, &binding.role_id)
    }

    // ---- graph -------------------------------------------------------------

    /// Persists an admitted graph genesis citing the project's admitted source.
    pub fn create_initial_graph(
        &mut self,
        clock: &dyn TrustedClockV1,
        genesis: StateGraphGenesisV1,
    ) -> Result<(), CoreStateWriteError> {
        let source = self.admitted_source(genesis.project_id())?.clone();
        let admission =
            self.admit_graph_content(Op::GraphGenesis, &genesis, genesis.graph_id(), &source)?;
        Ok(self
            .store
            .create_initial_graph_v1(admission, clock, genesis)?)
    }

    /// LEGACY, non-atomic: appends a mutation whose results were attached by
    /// separate [`Self::append_node_result`] calls. New work uses
    /// [`Self::publish_graph_mutation`]. Persists one admitted mutation of a
    /// stored graph; the source binding is the stored graph's genesis source.
    pub fn append_graph_mutation(
        &mut self,
        clock: &dyn TrustedClockV1,
        mutation: StateGraphMutationV1,
        snapshot: StateGraphSnapshotV1,
    ) -> Result<(), CoreStateWriteError> {
        let project = snapshot.content().project_id().to_owned();
        self.admitted_source(&project)?;
        let stored = self
            .reader()
            .load_current_graph_v1(&project, mutation.graph_id())?
            .ok_or(CorePolicyRefusal::UnknownSubject)?;
        let source = stored.genesis_provenance().source_ref().clone();
        let admission = self.admit_graph_content(
            Op::GraphMutation,
            snapshot.content(),
            mutation.graph_id(),
            &source,
        )?;
        Ok(self
            .store
            .append_graph_mutation_v1(admission, clock, mutation, snapshot)?)
    }

    /// LEGACY, non-atomic: attaches one node result to a stored graph version
    /// in its own transaction. New work uses [`Self::publish_graph_mutation`].
    pub fn append_node_result(
        &mut self,
        project: &str,
        version: &str,
        reference: &str,
        result: StateGraphNodeResultV1,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit(Op::GraphNodeResult, project, Some(result.graph_id()))?;
        Ok(self
            .store
            .append_node_result_v1(admission, project, version, reference, result)?)
    }

    /// Publishes one graph mutation with the node results it attaches, its
    /// successor snapshot, history, seal, watermark and head move in ONE State
    /// transaction (the admission binds the stored graph's source and this
    /// core's compiler and policy exactly as [`Self::append_graph_mutation`]).
    /// Any failure leaves none of them; an exact retry is `Replayed`.
    /// `results` are `(graph version, result reference, result)`.
    pub fn publish_graph_mutation(
        &mut self,
        clock: &dyn TrustedClockV1,
        results: Vec<(String, String, StateGraphNodeResultV1)>,
        mutation: StateGraphMutationV1,
        snapshot: StateGraphSnapshotV1,
    ) -> Result<GraphPublicationOutcomeV1, CoreStateWriteError> {
        let project = snapshot.content().project_id().to_owned();
        self.admitted_source(&project)?;
        let stored = self
            .reader()
            .load_current_graph_v1(&project, mutation.graph_id())?
            .ok_or(CorePolicyRefusal::UnknownSubject)?;
        let source = stored.genesis_provenance().source_ref().clone();
        let admission = self.admit_graph_content(
            Op::GraphMutation,
            snapshot.content(),
            mutation.graph_id(),
            &source,
        )?;
        let attachments = results
            .into_iter()
            .map(|(version, reference, result)| {
                GraphResultAttachmentV1::new(&version, &reference, result)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let publication = GraphMutationPublicationV1::new(attachments, mutation, snapshot)?;
        Ok(self
            .store
            .publish_graph_mutation_v1(admission, clock, &publication)?)
    }

    // ---- execution records ---------------------------------------------------

    pub fn append_attempt_started(
        &mut self,
        start: AttemptStart,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit(Op::Attempt, start.project_id(), Some(start.graph_id()))?;
        Ok(self.store.append_attempt_started(admission, start)?)
    }

    pub fn append_attempt_finished(
        &mut self,
        finish: AttemptFinish,
    ) -> Result<(), CoreStateWriteError> {
        let record = self
            .reader()
            .find_attempt(finish.attempt_id())?
            .ok_or(CorePolicyRefusal::UnknownSubject)?;
        let admission = self.admit(Op::Attempt, record.project_id(), Some(record.graph_id()))?;
        Ok(self.store.append_attempt_finished(admission, finish)?)
    }

    pub fn append_checkpoint(
        &mut self,
        value: CheckpointRecord,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit(Op::Checkpoint, value.project_id(), None)?;
        Ok(self.store.append_checkpoint(admission, value)?)
    }

    pub fn append_output_ref(&mut self, value: OutputRef) -> Result<(), CoreStateWriteError> {
        let admission = self.admit(Op::OutputRef, value.project_id(), None)?;
        Ok(self.store.append_output_ref(admission, value)?)
    }

    // ---- decisions and provenance --------------------------------------------

    pub fn append_decision_record(
        &mut self,
        record: &DecisionRecordV1,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit(Op::DecisionRecord, record.project_id(), None)?;
        Ok(self.store.append_decision_record(admission, record)?)
    }

    pub fn append_provenance_link(
        &mut self,
        project_id: &str,
        task_id: &str,
        kind: ProvenanceLinkKind,
        value: &str,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit(Op::ProvenanceLink, project_id, None)?;
        Ok(self
            .store
            .append_provenance_link(admission, project_id, task_id, kind, value)?)
    }

    // ---- events ----------------------------------------------------------------

    /// Persists a lower-trust (or core) event proposal with a core-minted id,
    /// after resolving its referenced content through the composition's
    /// resolver and verifying the actual bytes against the supplied digest.
    /// Any refusal persists nothing (no event, receipt or watermark change).
    pub fn persist_event_proposal(
        &mut self,
        clock: &dyn TrustedClockV1,
        content: &dyn CoreContentResolverV1,
        proposal: &EventProposalV1,
        event_id: &str,
    ) -> Result<EventPersistReceiptV1, CoreStateWriteError> {
        let project = proposal.project_id();
        self.admitted_source(project)?;
        verify_event_content(content, proposal)?;
        let admission = self.admit(Op::EventProposal, project, None)?;
        Ok(self
            .store
            .persist_event_proposal(admission, clock, proposal, event_id)?)
    }

    // ---- context -------------------------------------------------------------

    pub fn append_context_epoch(
        &mut self,
        context_epoch: ContextEpoch,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit(Op::ContextEpoch, &context_epoch.project_id, None)?;
        Ok(self.store.append_context_epoch(admission, context_epoch)?)
    }

    pub fn append_context_epoch_with_changed_sources(
        &mut self,
        context_epoch: ContextEpoch,
        changed_sources: ChangedSources,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit(Op::ContextEpoch, &context_epoch.project_id, None)?;
        Ok(self.store.append_context_epoch_with_changed_sources(
            admission,
            context_epoch,
            changed_sources,
        )?)
    }

    pub fn advance_context_epoch(
        &mut self,
        project_id: &str,
        advanced_at: &str,
        trigger: ContextEpochTrigger,
        invalidated_role_ids: &[String],
    ) -> Result<ContextEpoch, CoreStateWriteError> {
        let admission = self.admit(Op::ContextEpoch, project_id, None)?;
        Ok(self.store.advance_context_epoch(
            admission,
            project_id,
            advanced_at,
            trigger,
            invalidated_role_ids,
        )?)
    }

    pub fn advance_context_epoch_with_changed_sources(
        &mut self,
        project_id: &str,
        advanced_at: &str,
        trigger: ContextEpochTrigger,
        invalidated_role_ids: &[String],
        changed_sources: ChangedSources,
    ) -> Result<ContextEpoch, CoreStateWriteError> {
        let admission = self.admit(Op::ContextEpoch, project_id, None)?;
        Ok(self.store.advance_context_epoch_with_changed_sources(
            admission,
            project_id,
            advanced_at,
            trigger,
            invalidated_role_ids,
            changed_sources,
        )?)
    }

    pub fn create_context_manifest(
        &mut self,
        manifest: ContextManifest,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit(Op::ContextManifest, &manifest.project_id, None)?;
        Ok(self.store.create_context_manifest(admission, manifest)?)
    }

    pub fn rehydrate_context(
        &mut self,
        request: ContextRehydrationRequest,
        repository_materializer: &mut impl RepositorySnapshotMaterializer,
        artifact_materializer: &mut impl ArtifactMaterializer,
        event_supplier: &mut impl ContextRehydratedEventSupplier,
    ) -> Result<ContextRehydrationOutcome, CoreStateWriteError> {
        let admission = self.admit(Op::ContextRehydration, &request.project_id, None)?;
        Ok(self.store.rehydrate_context(
            admission,
            request,
            repository_materializer,
            artifact_materializer,
            event_supplier,
        )?)
    }

    // ---- roles and bindings ----------------------------------------------------

    pub fn create_logical_role(&mut self, role: LogicalRole) -> Result<(), CoreStateWriteError> {
        let admission = self.admit(Op::LogicalRole, &role.project_id, None)?;
        Ok(self.store.create_logical_role(admission, role)?)
    }

    pub fn transition_logical_role_status(
        &mut self,
        role_id: &str,
        expected: LogicalRoleStatus,
        new: LogicalRoleStatus,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit_role(Op::LogicalRole, role_id)?;
        Ok(self
            .store
            .transition_logical_role_status(admission, role_id, expected, new, evidence)?)
    }

    pub fn set_logical_role_active_binding(
        &mut self,
        role_id: &str,
        expected: Option<&str>,
        new: Option<&str>,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit_role(Op::LogicalRole, role_id)?;
        Ok(self
            .store
            .set_logical_role_active_binding(admission, role_id, expected, new, evidence)?)
    }

    pub fn set_logical_role_context(
        &mut self,
        role_id: &str,
        expected_epoch: &StateEpochValueV1,
        expected_manifest_id: Option<&str>,
        new_epoch: &StateEpochValueV1,
        new_manifest_id: Option<&str>,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit_role(Op::LogicalRole, role_id)?;
        Ok(self.store.set_logical_role_context(
            admission,
            role_id,
            expected_epoch,
            expected_manifest_id,
            new_epoch,
            new_manifest_id,
            evidence,
        )?)
    }

    pub fn supersede_context_manifest(
        &mut self,
        role_id: &str,
        expected_epoch: &StateEpochValueV1,
        expected_manifest_id: &str,
        manifest: ContextManifest,
        evidence: &RoleChangeEvidenceV1,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit_role(Op::LogicalRole, role_id)?;
        Ok(self.store.supersede_context_manifest(
            admission,
            role_id,
            expected_epoch,
            expected_manifest_id,
            manifest,
            evidence,
        )?)
    }

    pub fn create_executor_binding(
        &mut self,
        binding: ExecutorBinding,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit_role(Op::ExecutorBinding, &binding.role_id)?;
        Ok(self.store.create_executor_binding(admission, binding)?)
    }

    pub fn release_executor_binding(
        &mut self,
        binding_id: &str,
        released_at: &str,
        release_reason: ReleaseReason,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit_binding(binding_id)?;
        Ok(self.store.release_executor_binding(
            admission,
            binding_id,
            released_at,
            release_reason,
        )?)
    }

    pub fn expire_executor_binding_lease(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: ExecutorLeaseExpiryRequestV2,
    ) -> Result<ExecutorLeaseExpiryOutcomeV1, CoreStateWriteError> {
        let admission = self.admit_binding(&request.binding_id)?;
        Ok(self
            .store
            .expire_executor_binding_lease(admission, clock, request)?)
    }

    pub fn renew_executor_binding_lease(
        &mut self,
        clock: &dyn TrustedClockV1,
        binding_id: &str,
        lease_expires_at: &str,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit_binding(binding_id)?;
        Ok(self.store.renew_executor_binding_lease(
            admission,
            clock,
            binding_id,
            lease_expires_at,
        )?)
    }

    // ---- recovery -------------------------------------------------------------

    /// Startup recovery scan for an admitted project: expired leases are
    /// released with their events and each role's continuation requirement is
    /// reported (see State's `recovery`). A project the composition did not
    /// admit is refused before State.
    pub fn recover_startup(
        &mut self,
        clock: &dyn TrustedClockV1,
        project_id: &str,
        authority: &ExecutorReleasedLifecycleAuthorityV1,
    ) -> Result<StartupRecoveryReportV1, CoreStateWriteError> {
        let admission = self.admit(Op::ExecutorBinding, project_id, None)?;
        Ok(self
            .store
            .recover_startup(admission, clock, project_id, authority)?)
    }

    /// Failover rebind of a stored role: the project is the role's own, never
    /// a caller-supplied one. An unknown role is refused before State.
    pub fn rebind_role(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: &RoleRebindRequestV1,
    ) -> Result<RehydrationRequiredV1, CoreStateWriteError> {
        let admission = self.admit_role(Op::ExecutorBinding, &request.role_id)?;
        Ok(self.store.rebind_role(admission, clock, request)?)
    }

    /// Changed-source epoch and manifest succession for a stored role, in the
    /// role's own project.
    pub fn succeed_changed_context_sources(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: &ChangedSourceSuccessionRequestV1,
    ) -> Result<ChangedSourceSuccessionV1, CoreStateWriteError> {
        let admission = self.admit_role(Op::ContextEpoch, &request.role_id)?;
        Ok(self
            .store
            .succeed_changed_context_sources(admission, clock, request)?)
    }

    // ---- availability (D5) -----------------------------------------------------

    /// Records one durable availability fact (D5 ordering/refusal rules).
    pub fn append_availability_fact(
        &mut self,
        clock: &dyn TrustedClockV1,
        fact: &AvailabilityFactV1,
    ) -> Result<AvailabilityFactReceiptV1, CoreStateWriteError> {
        let admission = self.admit(Op::AvailabilityFact, fact.project_id(), None)?;
        Ok(self
            .store
            .append_availability_fact(admission, clock, fact)?)
    }
}

/// Resolves the content an event proposal references through the
/// composition's resolver and verifies the actual bytes against the
/// proposal's `sha256:` digest. Used for every event that reaches State
/// through the core, alone or inside a package.
pub(super) fn verify_event_content(
    content: &dyn CoreContentResolverV1,
    proposal: &EventProposalV1,
) -> Result<(), CoreStateWriteError> {
    let payload = proposal.payload();
    let expected = payload
        .digest
        .strip_prefix("sha256:")
        .filter(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        .ok_or(CorePolicyRefusal::UnsupportedDigest)?;
    let bytes = content
        .resolve(proposal.project_id(), &payload.reference)
        .ok_or(CorePolicyRefusal::ContentUnavailable)?;
    let actual: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if actual != expected {
        return Err(CorePolicyRefusal::ContentDigestMismatch.into());
    }
    Ok(())
}
