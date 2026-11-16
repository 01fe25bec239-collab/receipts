//! Narrow forwarding for trusted composition; no recovery activation or issuer.
use super::{CoreStateWriteError, CoreStateWriter};
pub use receipts_state::workspace_recovery::*;
use receipts_state::{StateWriteOperationV1, TrustedClockV1};
impl CoreStateWriter {
    pub fn bind_workspace_acceptance_v1(
        &mut self,
        target: &receipts_state::WorkspaceAcceptedTargetV1,
    ) -> Result<receipts_state::SetupWriteOutcomeV1, CoreStateWriteError> {
        let admission = self.admit(
            StateWriteOperationV1::WorkspaceRecovery,
            &target.subject.project_id,
            Some(&target.subject.graph_id),
        )?;
        Ok(self.store.bind_workspace_acceptance_v1(admission, target)?)
    }
    pub fn admit_workspace_recovery_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: &receipts_state::WorkspaceRecoveryAdmissionV1,
    ) -> Result<String, CoreStateWriteError> {
        let admission = self.admit(
            StateWriteOperationV1::WorkspaceRecovery,
            &request.subject.project_id,
            Some(&request.subject.graph_id),
        )?;
        Ok(self
            .store
            .admit_workspace_recovery_v1(admission, clock, request)?)
    }
    pub fn record_workspace_handoff_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: &receipts_state::WorkspaceHandoffRecordV1,
    ) -> Result<i64, CoreStateWriteError> {
        let admission = self.admit(
            StateWriteOperationV1::WorkspaceRecovery,
            &request.subject.project_id,
            Some(&request.subject.graph_id),
        )?;
        Ok(self
            .store
            .record_workspace_handoff_v1(admission, clock, request)?)
    }
    pub fn claim_workspace_recovery_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        request: &receipts_state::WorkspaceRecoveryClaimRequestV1,
    ) -> Result<receipts_state::WorkspaceRecoveryRecordV1, CoreStateWriteError> {
        let admission = self.admit(
            StateWriteOperationV1::WorkspaceRecovery,
            &request.subject.project_id,
            Some(&request.subject.graph_id),
        )?;
        Ok(self
            .store
            .claim_workspace_recovery_v1(admission, clock, request)?)
    }
    pub fn acknowledge_workspace_recovery_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        authority: &receipts_state::WorkspaceRecoveryAuthorityV1,
        stage: receipts_state::WorkspaceRecoveryEvidenceStageV1,
        evidence: &receipts_state::WorkspaceRecoveryEvidenceV1,
    ) -> Result<receipts_state::WorkspaceRecoveryRecordV1, CoreStateWriteError> {
        let admission = self.admit(
            StateWriteOperationV1::WorkspaceRecovery,
            &authority.subject.project_id,
            Some(&authority.subject.graph_id),
        )?;
        Ok(self
            .store
            .acknowledge_workspace_recovery_v1(admission, clock, authority, stage, evidence)?)
    }
    pub fn begin_workspace_recovery_action_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        authority: &receipts_state::WorkspaceRecoveryAuthorityV1,
        kind: receipts_state::WorkspaceRecoveryActionKindV1,
    ) -> Result<receipts_state::WorkspaceRecoveryActionGuardV1<'_>, CoreStateWriteError> {
        let admission = self.admit(
            StateWriteOperationV1::WorkspaceRecovery,
            &authority.subject.project_id,
            Some(&authority.subject.graph_id),
        )?;
        Ok(self
            .store
            .begin_workspace_recovery_action_v1(admission, clock, authority, kind)?)
    }
    pub fn resolve_workspace_recovery_v1(
        &mut self,
        clock: &dyn TrustedClockV1,
        authority: &receipts_state::WorkspaceRecoveryAuthorityV1,
        observation: &receipts_state::WorkspaceRecoveryResetObservationV1,
    ) -> Result<receipts_state::WorkspaceRecoveryRecordV1, CoreStateWriteError> {
        let admission = self.admit(
            StateWriteOperationV1::WorkspaceRecovery,
            &authority.subject.project_id,
            Some(&authority.subject.graph_id),
        )?;
        Ok(self
            .store
            .resolve_workspace_recovery_v1(admission, clock, authority, observation)?)
    }
}
