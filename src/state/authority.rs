//! State-local write-authority matrix and a genuinely read-only handle.
//!
//! Every one of the 53 frozen event types and every context-epoch advance
//! has exactly one authoritative publisher: the trusted orchestrator core.
//! `EventActor` kind/id remain recorded causal data (who caused it), never a
//! writer credential: State does not admit or refuse an append by actor.
//! Lower-trust input (workers, adapters, hosts, model output) is a proposal
//! for the core to evaluate, never a State writer. This module does not
//! secure the legacy writable [`SqliteStateRepository`] against arbitrary
//! in-process code; the deployed issuer and all-writer conversion remain the
//! adopted next-wave obligation.
//!
//! [`StateReader`] opens an existing current store with SQLite's read-only
//! flag and exposes reads only; it has no mutating method:
//!
//! ```compile_fail
//! fn try_write(reader: &mut receipts_state::StateReader, event: receipts_state::EventEnvelope) {
//!     reader.append_event(event);
//! }
//! ```
//!
//! ```compile_fail
//! fn try_transaction(reader: &mut receipts_state::StateReader) {
//!     let _ = reader.run_transaction(|_| Ok(()));
//! }
//! ```
//!
//! ```no_run
//! fn read(reader: &receipts_state::StateReader) {
//!     let _ = reader.find_event("01ARZ3NDEKTSV4RRFFQ69G5FAV");
//! }
//! ```

use std::fmt;
use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};

use crate::context_epoch::ContextEpoch;
use crate::context_manifest::ContextManifest;
use crate::decision_records::{DecisionRecordKind, DecisionRecordV1, ProvenanceChainV1};
use crate::epoch_value::StateEpochValueV1;
use crate::error::{GraphPhase, StateError};
use crate::event::{EventEnvelope, EventQuery, EventTimeQueryV1, EventType};
use crate::executor_binding::ExecutorBinding;
use crate::graph_repository;
use crate::logical_role::LogicalRole;
use crate::logical_role_lifecycle::LogicalRoleHistoryEntryV1;
use crate::repository::SqliteStateRepository;
use crate::trusted_time::TrustedTimeWatermarkV1;

/// The only authoritative publisher of State events and epoch advances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatePublisherV1 {
    /// The trusted orchestrator core (single writer process).
    TrustedCore,
}

/// The local event matrix: the authoritative publisher for `event_type`.
pub fn authoritative_event_publisher(event_type: EventType) -> StatePublisherV1 {
    match event_type {
        // Exhaustive over the closed set: a new type must be placed here.
        EventType::GoalCreated
        | EventType::GoalDecomposed
        | EventType::GoalEvaluated
        | EventType::GoalCompleted
        | EventType::GoalBlocked
        | EventType::RoleCreated
        | EventType::ExecutorSelected
        | EventType::ExecutorBound
        | EventType::ExecutorReleased
        | EventType::ExecutorReplaced
        | EventType::RoutingRequested
        | EventType::RoutingDecided
        | EventType::RoutingFailedNoCandidate
        | EventType::UserRoutingInput
        | EventType::TaskCreated
        | EventType::TaskReady
        | EventType::TaskDispatched
        | EventType::TaskStarted
        | EventType::TaskCompleted
        | EventType::TaskFailed
        | EventType::TaskCancelled
        | EventType::SubtaskRequested
        | EventType::SubtaskDispositioned
        | EventType::WorkspaceCreated
        | EventType::CheckpointWritten
        | EventType::WorkspaceRecovered
        | EventType::WorkspaceRemoved
        | EventType::ReviewDispatched
        | EventType::ReviewPassed
        | EventType::ReviewRejected
        | EventType::FindingRaised
        | EventType::FindingDispositioned
        | EventType::RepairIssued
        | EventType::RepairLimitReached
        | EventType::RateLimitObserved
        | EventType::ProviderDegraded
        | EventType::ProviderRecovered
        | EventType::AuthRequired
        | EventType::SafetyCheckPending
        | EventType::PolicyBlocked
        | EventType::ModelDiscovered
        | EventType::ModelLifecycleChanged
        | EventType::RegistryRefreshed
        | EventType::ContextRehydrated
        | EventType::ContextEpochAdvanced
        | EventType::ContextCompacted
        | EventType::AcceptanceEvaluated
        | EventType::IntegrationAccepted
        | EventType::IntegrationRejected
        | EventType::IntegrationBlocked
        | EventType::EscalatedToA2
        | EventType::EscalatedToA1
        | EventType::HumanRequired => StatePublisherV1::TrustedCore,
    }
}

/// The authoritative publisher of context-epoch advances.
pub fn context_epoch_advance_publisher() -> StatePublisherV1 {
    StatePublisherV1::TrustedCore
}

impl EventType {
    /// Every frozen event type, in contract order.
    pub fn all() -> [EventType; 53] {
        EventType::ALL
    }
}

/// Read-only State handle for lower-trust consumers (hosts, renderers,
/// inspection). Opened with `SQLITE_OPEN_READ_ONLY`; reads write nothing,
/// including no trusted-clock watermark.
pub struct StateReader {
    inner: SqliteStateRepository,
}

impl fmt::Debug for StateReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StateReader")
    }
}

impl StateReader {
    /// Opens an existing, exactly current store read-only. Never creates,
    /// migrates or repairs a file.
    pub fn open_existing(path: impl AsRef<Path>) -> Result<Self, StateError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| graph_repository::sqlite_error(GraphPhase::Read, e))?;
        conn.busy_timeout(Duration::from_millis(5000))
            .map_err(|e| graph_repository::sqlite_error(GraphPhase::Read, e))?;
        conn.execute_batch("PRAGMA foreign_keys=ON")
            .map_err(|e| graph_repository::sqlite_error(GraphPhase::Read, e))?;
        let head = graph_repository::schema_head(&conn, GraphPhase::Read)?;
        if head.is_some_and(|v| v < 0) {
            return Err(graph_repository::corrupt());
        }
        if head != Some(19) {
            return Err(StateError::GraphPersistence {
                phase: GraphPhase::Read,
                code: crate::error::GraphFailureCode::UnsupportedSchema,
                field: None,
            });
        }
        graph_repository::verify_current(&conn, GraphPhase::Read)?;
        Ok(Self {
            inner: SqliteStateRepository::from_connection(conn),
        })
    }

    #[cfg(test)]
    pub(crate) fn inner_connection_for_test(&self) -> &Connection {
        self.inner.connection()
    }

    pub fn find_logical_role(&self, role_id: &str) -> Result<Option<LogicalRole>, StateError> {
        self.inner.find_logical_role(role_id)
    }
    pub fn list_logical_roles_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<LogicalRole>, StateError> {
        self.inner.list_logical_roles_for_project(project_id)
    }
    pub fn list_logical_role_history(
        &self,
        role_id: &str,
    ) -> Result<Vec<LogicalRoleHistoryEntryV1>, StateError> {
        self.inner.list_logical_role_history(role_id)
    }
    pub fn find_executor_binding(
        &self,
        binding_id: &str,
    ) -> Result<Option<ExecutorBinding>, StateError> {
        self.inner.find_executor_binding(binding_id)
    }
    pub fn find_event(&self, event_id: &str) -> Result<Option<EventEnvelope>, StateError> {
        self.inner.find_event(event_id)
    }
    pub fn list_events(&self, query: &EventQuery) -> Result<Vec<EventEnvelope>, StateError> {
        self.inner.list_events(query)
    }
    pub fn list_events_chronological(
        &self,
        query: &EventTimeQueryV1,
    ) -> Result<Vec<EventEnvelope>, StateError> {
        self.inner.list_events_chronological(query)
    }
    pub fn find_context_manifest(
        &self,
        manifest_id: &str,
    ) -> Result<Option<ContextManifest>, StateError> {
        self.inner.find_context_manifest(manifest_id)
    }
    pub fn find_context_epoch_value(
        &self,
        project_id: &str,
        epoch: &StateEpochValueV1,
    ) -> Result<Option<ContextEpoch>, StateError> {
        self.inner.find_context_epoch_value(project_id, epoch)
    }
    pub fn find_trusted_time_watermark(
        &self,
        project_id: &str,
    ) -> Result<Option<TrustedTimeWatermarkV1>, StateError> {
        self.inner.find_trusted_time_watermark(project_id)
    }
    pub fn find_decision_record(
        &self,
        project_id: &str,
        kind: DecisionRecordKind,
        record_id: &str,
    ) -> Result<Option<DecisionRecordV1>, StateError> {
        self.inner.find_decision_record(project_id, kind, record_id)
    }
    pub fn list_decision_records(
        &self,
        project_id: &str,
        kind: DecisionRecordKind,
        task_id: Option<&str>,
    ) -> Result<Vec<DecisionRecordV1>, StateError> {
        self.inner.list_decision_records(project_id, kind, task_id)
    }
    pub fn provenance_chain(
        &self,
        project_id: &str,
        task_id: &str,
    ) -> Result<ProvenanceChainV1, StateError> {
        self.inner.provenance_chain(project_id, task_id)
    }
}
