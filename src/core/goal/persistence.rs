//! Core-owned goal persistence port. Implementations run in trusted composition;
//! neither the port nor a reservation is permission to launch a Runtime process.
pub use receipts_state::{
    AttemptStart, GoalDefinitionV1, GoalDispatchRecordV1, GoalDispatchV1, GoalProgressV1,
    GoalRecordV1, LaunchResolutionV1, SetupWriteOutcomeV1,
};
use receipts_state::{
    StateError, StateWriteOperationV1 as Op, StateWriteSubjectV1, TrustedCoreStore,
};

pub trait GoalPersistencePortV1 {
    fn load_goal(&self, project: &str, goal: &str) -> Result<Option<GoalRecordV1>, StateError>;
    fn load_dispatch(
        &self,
        project: &str,
        dispatch: &str,
    ) -> Result<Option<GoalDispatchRecordV1>, StateError>;
    fn create_goal(
        &mut self,
        definition: &GoalDefinitionV1,
        epoch: &str,
    ) -> Result<SetupWriteOutcomeV1, StateError>;
    fn advance_goal(
        &mut self,
        progress: &GoalProgressV1,
    ) -> Result<SetupWriteOutcomeV1, StateError>;
    fn reserve_dispatch(
        &mut self,
        request: &GoalDispatchV1,
        start: &AttemptStart,
    ) -> Result<SetupWriteOutcomeV1, StateError>;
    fn resolve_dispatch(
        &mut self,
        project: &str,
        dispatch: &str,
        resolution: LaunchResolutionV1,
        evidence_ref: &str,
    ) -> Result<SetupWriteOutcomeV1, StateError>;
}
fn subject(op: Op, project: &str, graph: &str) -> StateWriteSubjectV1 {
    StateWriteSubjectV1 {
        operation: op,
        project_id: project.into(),
        graph_id: Some(graph.into()),
        source_ref: None,
        compiler_id: None,
        policy_id: None,
    }
}
fn missing() -> StateError {
    StateError::SetupRefused {
        reason: receipts_state::SetupRefusalV1::MissingEvidence,
    }
}

impl GoalPersistencePortV1 for TrustedCoreStore {
    fn load_goal(&self, project: &str, goal: &str) -> Result<Option<GoalRecordV1>, StateError> {
        self.reader().find_goal_v1(project, goal)
    }
    fn load_dispatch(
        &self,
        project: &str,
        dispatch: &str,
    ) -> Result<Option<GoalDispatchRecordV1>, StateError> {
        self.reader().find_goal_dispatch_v1(project, dispatch)
    }
    fn create_goal(
        &mut self,
        definition: &GoalDefinitionV1,
        epoch: &str,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        let admission = self.admit(subject(
            Op::Goal,
            &definition.project_id,
            &definition.graph_id,
        ));
        self.create_goal_v1(admission, definition, epoch)
    }
    fn advance_goal(
        &mut self,
        progress: &GoalProgressV1,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        let goal = self
            .load_goal(&progress.project_id, &progress.goal_id)?
            .ok_or_else(missing)?;
        let admission = self.admit(subject(
            Op::Goal,
            &progress.project_id,
            &goal.definition.graph_id,
        ));
        self.advance_goal_v1(admission, progress)
    }
    fn reserve_dispatch(
        &mut self,
        request: &GoalDispatchV1,
        start: &AttemptStart,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        let admission = self.admit(subject(
            Op::GoalDispatch,
            &request.project_id,
            start.graph_id(),
        ));
        self.reserve_goal_dispatch_v1(admission, request, start)
    }
    fn resolve_dispatch(
        &mut self,
        project: &str,
        dispatch: &str,
        resolution: LaunchResolutionV1,
        evidence_ref: &str,
    ) -> Result<SetupWriteOutcomeV1, StateError> {
        let record = self.load_dispatch(project, dispatch)?.ok_or_else(missing)?;
        let attempt = self
            .reader()
            .find_attempt(&record.attempt_id)?
            .ok_or_else(missing)?;
        let admission = self.admit(subject(Op::GoalDispatch, project, attempt.graph_id()));
        self.resolve_goal_dispatch_v1(admission, project, dispatch, resolution, evidence_ref)
    }
}
