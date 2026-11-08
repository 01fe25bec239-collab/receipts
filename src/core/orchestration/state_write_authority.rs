//! Trusted-core issuer for State writes.
//!
//! [`CoreStateWriter`] is the core's sole State writer object. It owns the
//! exclusive [`TrustedCoreStore`] and the immutable compiler/policy identity
//! this core admits. For each write it resolves the subject from the actual
//! payload and its own identity (never from a caller-supplied flag), refuses
//! work produced under another compiler or policy, and only then mints the
//! single-use State admission. State independently re-checks the physical
//! store/project/graph/source/compiler/policy tuple.
//!
//! Lower-trust components (Routing, Review, Runtime, Workspace, Host) never
//! receive this object: they hand the core immutable proposals. This is an
//! in-process object-capability boundary; separating lower-trust principals
//! from the State files is a deployment requirement (see State's
//! `trusted_core` module limits).

use std::path::Path;

use receipts_state::{
    AvailabilityFactReceiptV1, AvailabilityFactV1, EventPersistReceiptV1, EventProposalV1,
    GraphCompilerRefV1, SqliteStateRepository, StateError, StateGraphGenesisV1,
    StateGraphMutationV1, StateGraphNodeResultV1, StateGraphSnapshotV1, StateWriteOperationV1,
    StateWriteSubjectV1, TrustedClockV1, TrustedCoreStore,
};

/// The immutable compiler and policy identity this core admits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreAdmissionIdentity {
    compiler_id: GraphCompilerRefV1,
    policy_id: String,
}

impl CoreAdmissionIdentity {
    /// `policy_id` must be non-empty and at most 200 characters.
    pub fn new(compiler_id: GraphCompilerRefV1, policy_id: String) -> Option<Self> {
        (1..=200)
            .contains(&policy_id.chars().count())
            .then_some(Self {
                compiler_id,
                policy_id,
            })
    }
}

/// Core-policy refusals, distinct from State's physical refusals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorePolicyRefusal {
    /// The graph was compiled by a compiler this core does not admit.
    CompilerNotAdmitted,
    /// The graph names a policy this core does not admit.
    PolicyNotAdmitted,
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

impl std::fmt::Display for CoreStateWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Policy(refusal) => write!(f, "core policy refused write: {refusal:?}"),
            Self::State(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for CoreStateWriteError {}

/// The trusted core's State writer.
#[derive(Debug)]
pub struct CoreStateWriter {
    store: TrustedCoreStore,
    identity: CoreAdmissionIdentity,
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
        })
    }

    pub fn store_id(&self) -> &str {
        self.store.store_id()
    }

    /// Read access for the core; reads never write.
    pub fn reader(&self) -> &SqliteStateRepository {
        self.store.reader()
    }

    fn admit_graph(
        &self,
        operation: StateWriteOperationV1,
        content: &StateGraphGenesisV1,
        graph_id: &str,
    ) -> Result<receipts_state::StateWriteAdmission, CoreStateWriteError> {
        let provenance = content.genesis_provenance();
        if provenance.compiler_id() != &self.identity.compiler_id {
            return Err(CoreStateWriteError::Policy(
                CorePolicyRefusal::CompilerNotAdmitted,
            ));
        }
        if content.policy_id() != self.identity.policy_id {
            return Err(CoreStateWriteError::Policy(
                CorePolicyRefusal::PolicyNotAdmitted,
            ));
        }
        Ok(self.store.admit(StateWriteSubjectV1 {
            operation,
            project_id: content.project_id().to_owned(),
            graph_id: Some(graph_id.to_owned()),
            source_ref: Some(provenance.source_ref().as_str().to_owned()),
            compiler_id: Some(self.identity.compiler_id.as_str().to_owned()),
            policy_id: Some(self.identity.policy_id.clone()),
        }))
    }

    fn admit_project(
        &self,
        operation: StateWriteOperationV1,
        project_id: &str,
        graph_id: Option<&str>,
    ) -> receipts_state::StateWriteAdmission {
        self.store.admit(StateWriteSubjectV1 {
            operation,
            project_id: project_id.to_owned(),
            graph_id: graph_id.map(str::to_owned),
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        })
    }

    /// Persists an admitted graph genesis (OR-W2-5 durable graph entry).
    pub fn create_initial_graph(
        &mut self,
        clock: &dyn TrustedClockV1,
        genesis: StateGraphGenesisV1,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit_graph(
            StateWriteOperationV1::GraphGenesis,
            &genesis,
            genesis.graph_id(),
        )?;
        Ok(self
            .store
            .create_initial_graph_v1(admission, clock, genesis)?)
    }

    /// Persists one admitted graph mutation and its successor snapshot.
    pub fn append_graph_mutation(
        &mut self,
        clock: &dyn TrustedClockV1,
        mutation: StateGraphMutationV1,
        snapshot: StateGraphSnapshotV1,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit_graph(
            StateWriteOperationV1::GraphMutation,
            snapshot.content(),
            mutation.graph_id(),
        )?;
        Ok(self
            .store
            .append_graph_mutation_v1(admission, clock, mutation, snapshot)?)
    }

    /// Attaches one node result to a persisted graph version.
    pub fn append_node_result(
        &mut self,
        project: &str,
        version: &str,
        reference: &str,
        result: StateGraphNodeResultV1,
    ) -> Result<(), CoreStateWriteError> {
        let admission = self.admit_project(
            StateWriteOperationV1::GraphNodeResult,
            project,
            Some(result.graph_id()),
        );
        Ok(self
            .store
            .append_node_result_v1(admission, project, version, reference, result)?)
    }

    /// Persists a lower-trust (or core) event proposal with a core-minted id.
    pub fn persist_event_proposal(
        &mut self,
        clock: &dyn TrustedClockV1,
        proposal: &EventProposalV1,
        event_id: &str,
    ) -> Result<EventPersistReceiptV1, CoreStateWriteError> {
        let admission = self.admit_project(
            StateWriteOperationV1::EventProposal,
            proposal.project_id(),
            None,
        );
        Ok(self
            .store
            .persist_event_proposal(admission, clock, proposal, event_id)?)
    }

    /// Records one durable availability fact (D5 ordering/refusal rules).
    pub fn append_availability_fact(
        &mut self,
        clock: &dyn TrustedClockV1,
        fact: &AvailabilityFactV1,
    ) -> Result<AvailabilityFactReceiptV1, CoreStateWriteError> {
        let admission = self.admit_project(
            StateWriteOperationV1::AvailabilityFact,
            fact.project_id(),
            None,
        );
        Ok(self
            .store
            .append_availability_fact(admission, clock, fact)?)
    }
}
