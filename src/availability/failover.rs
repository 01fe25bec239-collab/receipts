//! Failover without task loss (S6, S16, P-10).
//!
//! A failover is a new immutable decision for the same request and task,
//! linked to the failed one by `fallback_from`; the capsule, workspace and
//! results stay where they are. Only an availability limit re-routes. A
//! safety/policy hold BLOCKS instead (I-12: never shop for a provider that
//! will not refuse). Re-routing is bounded: past the backoff policy's attempt
//! bound, or past `max_failovers` links, the decision ASKs.

use super::ObservationEffect;
use crate::router::{RouteContext, RouteError, RoutedDecision, route};
use crate::{
    RegistryFreshness, RoutingDecisionCoreError, RoutingDecisionNonTemporalCore,
    RoutingDecisionOutcome, RoutingRequest, SelectionReason,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailoverError {
    /// Only a SELECTED decision for this exact request/task can fail over.
    NotTheDispatchedDecision,
    /// Timeouts, crashes, invalid output, auth or unknown failures are for
    /// retry/repair policy, not provider failover.
    NotAnAvailabilitySignal,
    Route(RouteError),
    Decision(RoutingDecisionCoreError),
}

/// Re-routes `request` after `previous` failed with `effect`, which the
/// caller has already committed to the manager whose view `ctx` carries.
pub fn fail_over(
    request: &RoutingRequest,
    previous: &RoutingDecisionNonTemporalCore,
    effect: &ObservationEffect,
    prior_failovers: u32,
    max_failovers: u32,
    ctx: &RouteContext<'_>,
    decision_id: String,
) -> Result<RoutedDecision, FailoverError> {
    let core = request.core();
    if previous.outcome() != RoutingDecisionOutcome::Selected
        || previous.request_id() != core.request_id()
        || previous.task_id() != core.task_id()
    {
        return Err(FailoverError::NotTheDispatchedDecision);
    }
    let terminal = |outcome, factor: &str| -> Result<RoutedDecision, FailoverError> {
        let decision = RoutingDecisionNonTemporalCore::try_new(
            ctx.now.clone(),
            decision_id.clone(),
            core.request_id().to_owned(),
            core.task_id().map(str::to_owned),
            outcome,
            None,
            None,
            None,
            Some(SelectionReason::new(
                Some(core.quality_floor()),
                None,
                None,
                Some(factor.to_owned()),
            )),
            None,
            None,
            None,
            None,
            None,
            RegistryFreshness::new(None, None, None, false),
            ctx.mode,
            Some(false),
            None,
            Some(previous.decision_id().to_owned()),
        )
        .map_err(FailoverError::Decision)?;
        Ok(RoutedDecision {
            decision,
            wait_after_seconds: None,
        })
    };
    match effect {
        ObservationEffect::SafetyOrPolicyHold(class) => {
            terminal(RoutingDecisionOutcome::Blocked, class.as_str())
        }
        ObservationEffect::NotAnAvailabilitySignal(_) => {
            Err(FailoverError::NotAnAvailabilitySignal)
        }
        ObservationEffect::Limited {
            exhausted: true, ..
        } => terminal(
            RoutingDecisionOutcome::UserInputRequired,
            "ASK_BACKOFF_ATTEMPTS_EXHAUSTED",
        ),
        ObservationEffect::Limited { .. } if prior_failovers >= max_failovers => terminal(
            RoutingDecisionOutcome::UserInputRequired,
            "ASK_FAILOVER_BOUND_REACHED",
        ),
        ObservationEffect::Limited { .. } => route(
            request,
            ctx,
            decision_id,
            Some(previous.decision_id().to_owned()),
        )
        .map_err(FailoverError::Route),
    }
}
