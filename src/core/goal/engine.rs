//! Goal-owned consumer boundary. Implementations are trusted composition, never workers.
use super::GoalEvaluationState;
use super::evaluate::GoalAssessmentV1;
use super::persistence::*;
use crate::compiler::{
    CompileContextV1, CompileErrorV1, CompiledGraphV1, DecompositionProposalV1, compile_graph_v1,
};
use crate::scheduler::WriteScopePort;
use receipts_state::{SetupRefusalV1, StateError};

/// Durable continuation and reconciliation belong to trusted composition.
/// Artifacts are immutable and bind goal/graph/epoch, runtime selection, pending
/// dispatch identities, review tickets, repair progress and budget counters.
/// Every method must refuse missing/foreign artifacts. Reconciliation must use
/// the existing mutation validators and admitted State writes for completions,
/// reviews and repairs; partial execution never establishes acceptance.
pub trait GoalLoopPortV1 {
    fn persist_compilation(&mut self, compiled: &CompiledGraphV1) -> Result<(), StateError>;
    fn reconcile(&mut self, goal: &GoalRecordV1) -> Result<GoalRoundV1, StateError>;
    fn launch(
        &mut self,
        goal: &GoalRecordV1,
        dispatch: &PreparedGoalDispatchV1,
    ) -> Result<(LaunchResolutionV1, String), StateError>;
    fn save_continuation(
        &mut self,
        goal: &GoalRecordV1,
        assessment: &GoalAssessmentV1,
    ) -> Result<String, StateError>;
}

pub struct PreparedGoalDispatchV1 {
    pub provider_id: String,
    pub runtime_id: String,
    pub eligible_host_native: bool,
    pub request: GoalDispatchV1,
    pub start: AttemptStart,
}

/// Reconciliation precedes readiness/admission/routing. A prepared dispatch must
/// have passed the shared scheduler, capsule and routing validators at this
/// round's epoch. The engine still relies on State's atomic binding checks.
pub struct GoalRoundV1 {
    pub current_epoch: String,
    pub dispatch: Option<PreparedGoalDispatchV1>,
    pub assessment: GoalAssessmentV1,
}

pub enum StartGoalOutcomeV1 {
    Accepted(GoalRecordV1),
    ClarificationRequired(CompileErrorV1),
}

pub fn start_goal_v1(
    state: &mut impl GoalPersistencePortV1,
    port: &mut impl GoalLoopPortV1,
    context: &CompileContextV1<'_>,
    proposal: &DecompositionProposalV1,
    scope: &impl WriteScopePort,
    dispatch_limit: String,
) -> Result<StartGoalOutcomeV1, StateError> {
    let compiled = match compile_graph_v1(context, proposal, scope) {
        Ok(c) => c,
        Err(
            e @ (CompileErrorV1::NoAcceptanceCriteria
            | CompileErrorV1::CriteriaRequireConfirmation(_)),
        ) => return Ok(StartGoalOutcomeV1::ClarificationRequired(e)),
        Err(_) => return Err(refused(SetupRefusalV1::InvalidShape)),
    };
    let epoch = context
        .context_epoch
        .as_ref()
        .ok_or_else(|| refused(SetupRefusalV1::MissingEvidence))?
        .decimal_digits();
    // Persist verbatim bytes and genesis before recording the goal reference.
    // Exact retries must compare artifacts and must not replace existing history.
    port.persist_compilation(&compiled)?;
    state.create_goal(
        &GoalDefinitionV1 {
            project_id: context.project_id.into(),
            goal_id: context.goal_id.into(),
            graph_id: context.graph_id.into(),
            spec_ref: context.spec_ref.into(),
            spec_sha256: compiled.spec_sha256,
            dispatch_limit,
        },
        epoch,
    )?;
    Ok(StartGoalOutcomeV1::Accepted(snapshot_goal_v1(
        state,
        context.project_id,
        context.goal_id,
    )?))
}

pub fn snapshot_goal_v1(
    state: &impl GoalPersistencePortV1,
    project: &str,
    goal: &str,
) -> Result<GoalRecordV1, StateError> {
    state
        .load_goal(project, goal)?
        .ok_or_else(|| refused(SetupRefusalV1::MissingEvidence))
}

/// Resume is a fresh tick from durable inputs, with no in-memory launch ticket.
/// A failed launch call leaves its reservation unresolved and never relaunches it.
pub fn tick_goal_v1(
    policy: &crate::policy::FreePolicyV1,
    state: &mut impl GoalPersistencePortV1,
    port: &mut impl GoalLoopPortV1,
    project: &str,
    goal_id: &str,
) -> Result<GoalAssessmentV1, StateError> {
    let mut goal = snapshot_goal_v1(state, project, goal_id)?;
    let round = port.reconcile(&goal)?;
    if round.current_epoch != goal.context_epoch {
        return Err(refused(SetupRefusalV1::StaleEpoch));
    }
    let mut assessment = round.assessment;
    if let Some(dispatch) = round.dispatch {
        if assessment.state != GoalEvaluationState::Incomplete {
            return Err(refused(SetupRefusalV1::InvalidShape));
        }
        if !policy.permits(
            &dispatch.provider_id,
            &dispatch.runtime_id,
            dispatch.eligible_host_native,
        ) {
            return Err(refused(SetupRefusalV1::ForeignEvidence));
        }
        if dispatch.request.project_id != project || dispatch.request.goal_id != goal_id {
            return Err(refused(SetupRefusalV1::ForeignEvidence));
        }
        match state.reserve_dispatch(&dispatch.request, &dispatch.start) {
            Ok(SetupWriteOutcomeV1::Applied) => {
                let (resolution, evidence) = port.launch(&goal, &dispatch)?;
                state.resolve_dispatch(
                    project,
                    &dispatch.request.dispatch_id,
                    resolution,
                    &evidence,
                )?;
                assessment = GoalAssessmentV1 {
                    state: GoalEvaluationState::Incomplete,
                    gaps: vec!["dispatched work requires completion and verification".into()],
                };
                if resolution == LaunchResolutionV1::Uncertain {
                    assessment =
                        blocked("launch outcome unresolved; reconcile evidence before retry");
                }
            }
            Ok(SetupWriteOutcomeV1::Replayed) => {
                let existing = state
                    .load_dispatch(project, &dispatch.request.dispatch_id)?
                    .ok_or_else(|| refused(SetupRefusalV1::MissingEvidence))?;
                assessment = match existing.resolution {
                    None | Some(LaunchResolutionV1::Uncertain) => {
                        blocked("launch outcome unresolved; reconcile evidence before retry")
                    }
                    _ => GoalAssessmentV1 {
                        state: GoalEvaluationState::Incomplete,
                        gaps: vec!["dispatch replay; reconcile persisted attempt".into()],
                    },
                };
            }
            Err(StateError::SetupRefused {
                reason: SetupRefusalV1::BudgetExhausted,
            }) => {
                assessment = blocked(
                    "dispatch budget exhausted; resume requires an admitted budget disposition",
                )
            }
            Err(StateError::SetupRefused {
                reason: SetupRefusalV1::UnresolvedLaunch,
            }) => assessment = blocked("previous launch unresolved; reconcile before retry"),
            Err(e) => return Err(e),
        }
        goal = snapshot_goal_v1(state, project, goal_id)?;
    }
    if assessment.state == GoalEvaluationState::Complete && !assessment.gaps.is_empty() {
        return Err(refused(SetupRefusalV1::MissingEvidence));
    }
    let reference = port.save_continuation(&goal, &assessment)?;
    state.advance_goal(&GoalProgressV1 {
        project_id: project.into(),
        goal_id: goal_id.into(),
        expected_revision: goal.revision,
        expected_epoch: goal.context_epoch.clone(),
        context_epoch: goal.context_epoch,
        continuation_ref: Some(reference),
    })?;
    Ok(assessment)
}

pub use tick_goal_v1 as resume_goal_v1;
fn refused(reason: SetupRefusalV1) -> StateError {
    StateError::SetupRefused { reason }
}
fn blocked(reason: &str) -> GoalAssessmentV1 {
    GoalAssessmentV1 {
        state: GoalEvaluationState::Blocked,
        gaps: vec![reason.into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GraphNodeState;
    use crate::store::{compiled_provenance, state_content_v1};
    use crate::test_support::{CLOCK, COMPILER, Dir, SOURCE, envelope};
    use receipts_state::*;
    fn subject(op: StateWriteOperationV1) -> StateWriteSubjectV1 {
        StateWriteSubjectV1 {
            operation: op,
            project_id: "p".into(),
            graph_id: Some("g".into()),
            source_ref: None,
            compiler_id: None,
            policy_id: None,
        }
    }
    fn seed(limit: &str) -> (Dir, TrustedCoreStore) {
        let dir = Dir::new();
        let mut state = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
        let env = envelope(&[("n", "TASK", GraphNodeState::Planned, Some("w"))], &[]);
        let genesis = state_content_v1(
            &env,
            compiled_provenance(
                GraphCompilerRefV1::parse(COMPILER).unwrap(),
                GraphSourceEvidenceRefV1::parse(SOURCE).unwrap(),
            ),
            true,
        )
        .unwrap();
        let mut auth = subject(StateWriteOperationV1::GraphGenesis);
        auth.compiler_id = Some(COMPILER.into());
        auth.source_ref = Some(SOURCE.into());
        auth.policy_id = Some("policy".into());
        let admission = state.admit(auth);
        state
            .create_initial_graph_v1(admission, &CLOCK, genesis)
            .unwrap();
        let mut epoch_subject = subject(StateWriteOperationV1::ContextEpoch);
        epoch_subject.graph_id = None;
        let admission = state.admit(epoch_subject);
        state
            .append_context_epoch(
                admission,
                ContextEpoch {
                    project_id: "p".into(),
                    epoch: StateEpochValueV1::try_from("0").unwrap(),
                    advanced_at: "opaque".into(),
                    trigger: ContextEpochTrigger::A1Init,
                },
            )
            .unwrap();
        state
            .create_goal(
                &GoalDefinitionV1 {
                    project_id: "p".into(),
                    goal_id: "goal".into(),
                    graph_id: "g".into(),
                    spec_ref: "artifact:verbatim".into(),
                    spec_sha256: "b".repeat(64),
                    dispatch_limit: limit.into(),
                },
                "0",
            )
            .unwrap();
        (dir, state)
    }
    struct Controlled {
        launch_count: usize,
        crash: bool,
        replay: bool,
    }
    impl GoalLoopPortV1 for Controlled {
        fn persist_compilation(&mut self, _: &CompiledGraphV1) -> Result<(), StateError> {
            unreachable!()
        }
        fn reconcile(&mut self, goal: &GoalRecordV1) -> Result<GoalRoundV1, StateError> {
            Ok(GoalRoundV1 {
                current_epoch: "0".into(),
                assessment: GoalAssessmentV1 {
                    state: GoalEvaluationState::Incomplete,
                    gaps: vec!["work remains".into()],
                },
                dispatch: Some(PreparedGoalDispatchV1 {
                    provider_id: "host".into(),
                    runtime_id: "runtime".into(),
                    eligible_host_native: true,
                    request: GoalDispatchV1 {
                        project_id: "p".into(),
                        goal_id: "goal".into(),
                        dispatch_id: "d".into(),
                        expected_revision: if self.replay {
                            "0".into()
                        } else {
                            goal.revision.clone()
                        },
                        context_epoch: "0".into(),
                        graph_version: "1".into(),
                    },
                    start: AttemptStart::new(
                        AttemptId::parse("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
                        "p",
                        "g",
                        "n",
                        "1",
                        AttemptRole::RuntimeA3,
                        "runtime",
                        "w",
                        &"a".repeat(40),
                        "2026-11-08T10:00:00.000000000Z",
                    )
                    .unwrap(),
                }),
            })
        }
        fn launch(
            &mut self,
            _: &GoalRecordV1,
            _: &PreparedGoalDispatchV1,
        ) -> Result<(LaunchResolutionV1, String), StateError> {
            self.launch_count += 1;
            if self.crash {
                return Err(refused(SetupRefusalV1::MissingEvidence));
            }
            Ok((LaunchResolutionV1::Launched, "launch:evidence".into()))
        }
        fn save_continuation(
            &mut self,
            _: &GoalRecordV1,
            _: &GoalAssessmentV1,
        ) -> Result<String, StateError> {
            Ok("immutable:continuation".into())
        }
    }
    #[test]
    fn crash_reopen_replay_never_launches_twice() {
        let (dir, mut state) = seed("2");
        let mut port = Controlled {
            launch_count: 0,
            crash: true,
            replay: false,
        };
        assert!(
            tick_goal_v1(
                &crate::policy::FreePolicyV1::new("host".into(), "runtime".into()).unwrap(),
                &mut state,
                &mut port,
                "p",
                "goal"
            )
            .is_err()
        );
        drop(state);
        let mut state = TrustedCoreStore::open_exclusive(dir.db()).unwrap();
        port.replay = true;
        port.crash = false;
        let outcome = resume_goal_v1(
            &crate::policy::FreePolicyV1::new("host".into(), "runtime".into()).unwrap(),
            &mut state,
            &mut port,
            "p",
            "goal",
        )
        .unwrap();
        assert_eq!(outcome.state, GoalEvaluationState::Blocked);
        assert_eq!(port.launch_count, 1);
        let goal = snapshot_goal_v1(&state, "p", "goal").unwrap();
        assert_eq!(goal.dispatches_used, "1");
        assert_eq!(goal.definition.spec_ref, "artifact:verbatim");
        assert_eq!(
            goal.continuation_ref.as_deref(),
            Some("immutable:continuation")
        );
    }
    #[test]
    fn exhausted_budget_is_blocked_without_launch() {
        let (_dir, mut state) = seed("0");
        let mut port = Controlled {
            launch_count: 0,
            crash: false,
            replay: false,
        };
        assert_eq!(
            tick_goal_v1(
                &crate::policy::FreePolicyV1::new("host".into(), "runtime".into()).unwrap(),
                &mut state,
                &mut port,
                "p",
                "goal"
            )
            .unwrap()
            .state,
            GoalEvaluationState::Blocked
        );
        assert_eq!(port.launch_count, 0);
    }
    #[test]
    fn start_preserves_missing_criteria_clarification_without_launch() {
        use crate::compiler::ProposedNodeV1;
        use crate::orchestration::CoreContentResolverV1;
        use std::collections::{BTreeMap, BTreeSet};
        struct Spec;
        impl CoreContentResolverV1 for Spec {
            fn resolve(&self, _: &str, _: &str) -> Option<Vec<u8>> {
                Some(b"verbatim\r\n".to_vec())
            }
        }
        let (_dir, mut state) = seed("2");
        let admission = BTreeMap::new();
        let confirmed = BTreeSet::new();
        let context = CompileContextV1 {
            project_id: "p",
            graph_id: "g",
            goal_id: "goal",
            policy_id: "policy",
            created_at: crate::test_support::time("2026-11-08T00:00:00Z"),
            context_epoch: Some(
                super::super::GoalEvaluationContextEpoch::from_decimal("0").unwrap(),
            ),
            compiler_id: GraphCompilerRefV1::parse(COMPILER).unwrap(),
            source: GraphSourceEvidenceRefV1::parse(SOURCE).unwrap(),
            spec_ref: "spec",
            content: &Spec,
            capability_admission: &admission,
            entitlement_state: receipts_state::entitlement::ProductEntitlementState::Free,
            confirmed_criteria: &confirmed,
        };
        let proposal = DecompositionProposalV1 {
            workstreams: vec![],
            dependencies: vec![],
            criteria: vec![],
            nodes: vec![ProposedNodeV1 {
                node_id: "n".into(),
                kind: crate::GraphNodeKind::TASK,
                title: None,
                workstream_id: None,
                required_capabilities: None,
            }],
        };
        let mut port = Controlled {
            launch_count: 0,
            crash: false,
            replay: false,
        };
        assert!(matches!(
            start_goal_v1(
                &mut state,
                &mut port,
                &context,
                &proposal,
                &crate::test_support::PORT,
                "2".into()
            )
            .unwrap(),
            StartGoalOutcomeV1::ClarificationRequired(CompileErrorV1::NoAcceptanceCriteria)
        ));
        assert_eq!(port.launch_count, 0);
        assert_eq!(snapshot_goal_v1(&state, "p", "goal").unwrap().revision, "0");
    }
}
