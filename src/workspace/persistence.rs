//! Narrow trusted adapter for existing admitted State checkpoint/output writes.
//! The caller stores immutable content first. This adapter verifies metadata
//! bindings, not artifact bytes, process identity or physical worktree continuity.
use receipts_state::{
    AttemptId, CheckpointRecord, OutputRef, SetupRefusalV1, StateError,
    StateWriteOperationV1 as Op, StateWriteSubjectV1, TrustedCoreStore,
};
use sha2::{Digest, Sha256};

/// Only the trusted adapter constructs this durable metadata acknowledgement.
/// It is not signal, recovery, acceptance or artifact-integrity authority.
pub struct CheckpointPersistenceReceiptV1 {
    project_id: String,
    workspace_id: String,
    attempt_id: AttemptId,
    checkpoint_id: String,
    checkpoint_sha256: String,
}
impl CheckpointPersistenceReceiptV1 {
    pub fn project_id(&self) -> &str {
        &self.project_id
    }
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }
    pub fn attempt_id(&self) -> &AttemptId {
        &self.attempt_id
    }
    pub fn checkpoint_id(&self) -> &str {
        &self.checkpoint_id
    }
    pub fn checkpoint_sha256(&self) -> &str {
        &self.checkpoint_sha256
    }
}

/// Owned by trusted composition, never supplied to a worker. A fake sink can
/// exercise Workspace scheduling without importing a raw repository writer.
pub trait AttemptEvidenceSinkV1 {
    type Receipt;
    fn persist_checkpoint(
        &mut self,
        checkpoint: &CheckpointRecord,
        outputs: &[OutputRef],
    ) -> Result<Self::Receipt, StateError>;
}
pub struct TrustedAttemptEvidenceSinkV1<'a> {
    store: &'a mut TrustedCoreStore,
}
impl<'a> TrustedAttemptEvidenceSinkV1<'a> {
    pub fn new(store: &'a mut TrustedCoreStore) -> Self {
        Self { store }
    }
}
fn refused(reason: SetupRefusalV1) -> StateError {
    StateError::SetupRefused { reason }
}
fn subject(op: Op, project: &str) -> StateWriteSubjectV1 {
    StateWriteSubjectV1 {
        operation: op,
        project_id: project.into(),
        graph_id: None,
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    }
}
impl AttemptEvidenceSinkV1 for TrustedAttemptEvidenceSinkV1<'_> {
    type Receipt = CheckpointPersistenceReceiptV1;
    /// Output rows precede the checkpoint. A failed call may leave matching
    /// output metadata; stable identities make that interruption retry-safe.
    fn persist_checkpoint(
        &mut self,
        checkpoint: &CheckpointRecord,
        outputs: &[OutputRef],
    ) -> Result<CheckpointPersistenceReceiptV1, StateError> {
        let json = checkpoint.to_json()?;
        let fields: serde_json::Value =
            serde_json::from_str(&json).map_err(|_| refused(SetupRefusalV1::InvalidShape))?;
        let id = fields["attempt_id"]
            .as_str()
            .ok_or_else(|| refused(SetupRefusalV1::MissingEvidence))?;
        let id = AttemptId::parse(id)?;
        let attempt = self
            .store
            .reader()
            .find_attempt(&id)?
            .ok_or_else(|| refused(SetupRefusalV1::MissingEvidence))?;
        if attempt.project_id() != checkpoint.project_id()
            || attempt.workspace_id() != checkpoint.workspace_id()
            || outputs
                .iter()
                .any(|o| o.project_id() != checkpoint.project_id())
        {
            return Err(refused(SetupRefusalV1::ForeignEvidence));
        }
        let mut supplied = std::collections::BTreeMap::new();
        for output in outputs {
            if let Some(previous) = supplied.insert(output.output_ref(), output)
                && previous != output
            {
                return Err(refused(SetupRefusalV1::ConflictingReplay));
            }
        }
        // Detect every conflict before appending any metadata.
        for output in outputs {
            if let Some(existing) = self
                .store
                .reader()
                .find_output_ref(output.project_id(), output.output_ref())?
                && existing != *output
            {
                return Err(refused(SetupRefusalV1::ConflictingReplay));
            }
        }
        let existing = self
            .store
            .reader()
            .find_checkpoint(checkpoint.project_id(), checkpoint.checkpoint_id())?;
        if let Some(existing) = &existing
            && existing.to_json()? != json
        {
            return Err(refused(SetupRefusalV1::ConflictingReplay));
        }
        for output in outputs {
            if self
                .store
                .reader()
                .find_output_ref(output.project_id(), output.output_ref())?
                .is_none()
            {
                let admission = self
                    .store
                    .admit(subject(Op::OutputRef, output.project_id()));
                self.store.append_output_ref(admission, output.clone())?;
            }
        }
        if existing.is_none() {
            let admission = self
                .store
                .admit(subject(Op::Checkpoint, checkpoint.project_id()));
            self.store.append_checkpoint(
                admission,
                CheckpointRecord::parse_json(checkpoint.project_id(), &json)?,
            )?;
        }
        Ok(CheckpointPersistenceReceiptV1 {
            project_id: checkpoint.project_id().into(),
            workspace_id: checkpoint.workspace_id().into(),
            attempt_id: id,
            checkpoint_id: checkpoint.checkpoint_id().into(),
            checkpoint_sha256: Sha256::digest(json.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        })
    }
}

impl TrustedAttemptEvidenceSinkV1<'_> {
    /// Atomically commits successful finish and its State-validated mechanical
    /// receipt. This is not review acceptance.
    pub fn persist_finalization(
        &mut self,
        record: &receipts_state::FinalizationRecordV1,
        finish: &receipts_state::AttemptFinish,
    ) -> Result<receipts_state::SetupWriteOutcomeV1, StateError> {
        let mut binding = subject(Op::Finalization, &record.project_id);
        binding.graph_id = Some(record.graph_id.clone());
        let admission = self.store.admit(binding);
        self.store
            .finish_with_finalization_v1(admission, record, finish)
    }
}

impl TrustedAttemptEvidenceSinkV1<'_> {
    /// Bind fresh mechanical finalization to the exact persisted checkpoint,
    /// then use State's atomic finish/receipt operation. No review is inferred.
    pub fn persist_verified_finalization(
        &mut self,
        evidence: &crate::WorkspaceAttemptFinalizationEvidence,
        receipt: &CheckpointPersistenceReceiptV1,
        finish: &receipts_state::AttemptFinish,
        output_refs: Vec<String>,
    ) -> Result<receipts_state::SetupWriteOutcomeV1, StateError> {
        if evidence.workspace_id() != receipt.workspace_id()
            || evidence.attempt_id() != Some(receipt.attempt_id().as_str())
            || evidence.checkpoint().checkpoint_id() != receipt.checkpoint_id()
            || finish.attempt_id() != receipt.attempt_id()
        {
            return Err(refused(SetupRefusalV1::ForeignEvidence));
        }
        let checkpoint = self
            .store
            .reader()
            .find_checkpoint(receipt.project_id(), receipt.checkpoint_id())?
            .ok_or_else(|| refused(SetupRefusalV1::MissingEvidence))?;
        let json = checkpoint.to_json()?;
        let fields: serde_json::Value =
            serde_json::from_str(&json).map_err(|_| refused(SetupRefusalV1::InvalidShape))?;
        let captured_at = fields["captured_at"]
            .as_str()
            .ok_or_else(|| refused(SetupRefusalV1::MissingEvidence))?;
        let temporal = crate::WorkspaceCheckpointTemporalCaptureCore::new(
            evidence.checkpoint().clone(),
            crate::WorkspaceDateTimeV1::try_new(captured_at)
                .map_err(|_| refused(SetupRefusalV1::InvalidShape))?,
        );
        let observed = crate::workspace_checkpoint_to_json(&temporal)
            .map_err(|_| refused(SetupRefusalV1::InvalidShape))?;
        let observed: serde_json::Value =
            serde_json::from_str(&observed).map_err(|_| refused(SetupRefusalV1::InvalidShape))?;
        if observed != fields {
            return Err(refused(SetupRefusalV1::ConflictingReplay));
        }
        let attempt = self
            .store
            .reader()
            .find_attempt(receipt.attempt_id())?
            .ok_or_else(|| refused(SetupRefusalV1::MissingEvidence))?;
        let record = receipts_state::FinalizationRecordV1 {
            project_id: receipt.project_id().into(),
            graph_id: attempt.graph_id().into(),
            attempt_id: receipt.attempt_id().as_str().into(),
            workspace_id: receipt.workspace_id().into(),
            checkpoint_id: receipt.checkpoint_id().into(),
            final_sha: evidence.final_sha().as_str().into(),
            write_scope_verdict: "PASS".into(),
            check_results: fields["executed_checks"]
                .as_array()
                .cloned()
                .unwrap_or_default(),
            output_refs,
        };
        self.persist_finalization(&record, finish)
    }
}
