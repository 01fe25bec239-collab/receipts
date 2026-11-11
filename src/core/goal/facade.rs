//! Host-neutral goal entrypoints. Hosts supply trusted consumer adapters.
use super::{driver::*, engine::*, evaluate::GoalAssessmentV1, persistence::*};
use crate::ApplyEvidenceV1;
use crate::compiler::{CompileContextV1, DecompositionProposalV1};
use crate::policy::FreePolicyV1;
use crate::scheduler::WriteScopePort;
use receipts_state::StateError;
use std::collections::BTreeSet;

pub struct CoreFacade<'a, S, P> {
    pub state: &'a mut S,
    pub consumers: &'a mut P,
    pub policy: &'a FreePolicyV1,
    pub evidence: ApplyEvidenceV1<'a>,
}
impl<S: GoalPersistencePortV1, P: GoalConsumerPortV1> CoreFacade<'_, S, P> {
    pub fn start_goal(
        &mut self,
        context: &CompileContextV1<'_>,
        proposal: &DecompositionProposalV1,
        scope: &impl WriteScopePort,
        dispatch_limit: String,
    ) -> Result<StartGoalOutcomeV1, StateError> {
        let empty = BTreeSet::new();
        let mut driver = GoalDriverV1 {
            consumers: self.consumers,
            policy: self.policy,
            evidence: self.evidence,
            retry_failed: &empty,
            evaluation_binding: None,
        };
        match start_goal_v1(
            self.state,
            &mut driver,
            context,
            proposal,
            scope,
            dispatch_limit,
        )? {
            StartGoalOutcomeV1::Accepted(_) => {
                let goal = initialize_goal_continuation_v1(
                    self.state,
                    self.consumers,
                    self.policy,
                    context.project_id,
                    context.goal_id,
                )?;
                let snapshot = self.consumers.snapshot(&goal)?;
                let version = snapshot.graph.graph_version().as_str();
                self.consumers
                    .emit(&goal, version, GoalLifecycleEventV1::GoalCreated)?;
                self.consumers
                    .emit(&goal, version, GoalLifecycleEventV1::GoalDecomposed)?;
                for node in snapshot.graph.graph().nodes() {
                    self.consumers.emit(
                        &goal,
                        version,
                        GoalLifecycleEventV1::TaskCreated {
                            node: node.node_id().into(),
                        },
                    )?;
                }
                Ok(StartGoalOutcomeV1::Accepted(goal))
            }
            clarification => Ok(clarification),
        }
    }
    pub fn snapshot(&mut self, project: &str, goal: &str) -> Result<GoalSnapshotV1, StateError> {
        let record = snapshot_goal_v1(self.state, project, goal)?;
        let empty = BTreeSet::new();
        let driver = GoalDriverV1 {
            consumers: self.consumers,
            policy: self.policy,
            evidence: self.evidence,
            retry_failed: &empty,
            evaluation_binding: None,
        };
        driver.durable_snapshot(record)
    }
    pub fn tick(&mut self, project: &str, goal: &str) -> Result<GoalAssessmentV1, StateError> {
        self.retry_failed(project, goal, &BTreeSet::new())
    }
    pub fn resume_goal(
        &mut self,
        project: &str,
        goal: &str,
    ) -> Result<GoalAssessmentV1, StateError> {
        self.tick(project, goal)
    }
    /// Explicit FREE retry names only rejected nodes with attached failure
    /// evidence. Core validates EXPAND_REPAIR and retains the rejected history.
    pub fn retry_failed(
        &mut self,
        project: &str,
        goal: &str,
        nodes: &BTreeSet<String>,
    ) -> Result<GoalAssessmentV1, StateError> {
        initialize_goal_continuation_v1(self.state, self.consumers, self.policy, project, goal)?;
        let mut driver = GoalDriverV1 {
            consumers: self.consumers,
            policy: self.policy,
            evidence: self.evidence,
            retry_failed: nodes,
            evaluation_binding: None,
        };
        tick_goal_v1(self.policy, self.state, &mut driver, project, goal)
    }
}
