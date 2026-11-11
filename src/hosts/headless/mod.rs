//! Terminal/CI translation only. No hook, process, State or goal-loop ownership.
use crate::claude::{ClaudeCoreView, ClaudeUserResponse, render_core_view};
use crate::host_event_spool::HostEventSpoolError;
use crate::{
    HostAdapter, HostCapabilityReport, HostEventSpool, HostId, NormalizedHostEvent,
    NormalizedHostEventConfidence, NormalizedHostEventSourceClass,
    validate_normalized_host_event_source,
};
use std::future::{Ready, ready};

/// Origin supplied by the terminal composition; neither origin proves authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventProvenance {
    Inferred,
    CliSupplied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadlessEventError {
    WrongHost,
    ObservedConfidence,
    InvalidSource,
    Spool(HostEventSpoolError),
}

/// Composition implements this port using the Core goal entrypoint.
/// Host forwards the request verbatim and preserves both success and refusal.
pub trait GoalEntrypointPort {
    type Request;
    type Outcome;
    fn start_goal(&mut self, request: Self::Request) -> Self::Outcome;
}

#[derive(Debug, Clone)]
pub struct HeadlessAdapter {
    spool: HostEventSpool,
    report: HostCapabilityReport,
}
impl HeadlessAdapter {
    pub fn new(spool: HostEventSpool, report: HostCapabilityReport) -> Self {
        Self { spool, report }
    }
    pub fn start_goal<P: GoalEntrypointPort>(
        &self,
        port: &mut P,
        request: P::Request,
    ) -> P::Outcome {
        port.start_goal(request)
    }
    /// Explicit provenance receipt; CLI-supplied events remain INFERRED because
    /// this adapter cannot independently verify the CLI's observations.
    pub fn emit_with_provenance(
        &self,
        event: &NormalizedHostEvent,
        source: NormalizedHostEventSourceClass,
        provenance: EventProvenance,
    ) -> Result<EventProvenance, HeadlessEventError> {
        validate_normalized_host_event_source(source, event)
            .map_err(|_| HeadlessEventError::InvalidSource)?;
        self.emit(event)?;
        Ok(provenance)
    }
}
impl HostAdapter for HeadlessAdapter {
    type DetectOutcome = bool;
    type InstallPlan = ();
    type InstallOutcome = ();
    type CoreHandle = HostEventSpool;
    type EmitOutcome = Result<(), HeadlessEventError>;
    type CoreView = ClaudeCoreView;
    type PresentOutcome = String;
    type UserPrompt = String;
    type UserResponse = ClaudeUserResponse;
    type UserInputPending = Ready<ClaudeUserResponse>;
    type ShutdownReason = Option<&'static str>;
    type ShutdownOutcome = ();
    fn id(&self) -> HostId {
        HostId::Headless
    }
    fn detect(&self) -> bool {
        true
    }
    fn install(&self, _: &()) {}
    fn start(&self) -> HostEventSpool {
        self.spool.clone()
    }
    fn emit(&self, event: &NormalizedHostEvent) -> Self::EmitOutcome {
        if event.host().as_str() != "headless" {
            return Err(HeadlessEventError::WrongHost);
        }
        if event.confidence() != NormalizedHostEventConfidence::Inferred {
            return Err(HeadlessEventError::ObservedConfidence);
        }
        self.spool.append(event).map_err(HeadlessEventError::Spool)
    }
    fn present(&self, view: &ClaudeCoreView) -> String {
        render_core_view(view)
    }
    fn request_user_input(&mut self, _: String) -> Self::UserInputPending {
        ready(ClaudeUserResponse::Unavailable)
    }
    fn capabilities(&self) -> HostCapabilityReport {
        self.report.clone()
    }
    fn shutdown(self, _: Self::ShutdownReason) {}
}
#[cfg(test)]
mod tests;
