//! Codex translation fixtures and supervised delivery. Installed native hooks
//! remain unqualified until admitted Q01 registration/trust evidence arrives.
use crate::claude::{ClaudeCoreView, ClaudeUserResponse, render_core_view};
use crate::host_event_spool::HostEventSpoolError;
use crate::{
    HostAdapter, HostCapabilityReport, HostCapabilitySelectedMode, HostEventSpool, HostId,
    NormalizedHostEvent, NormalizedHostEventConfidence,
};
use std::future::{Ready, ready};
pub mod hook_map;
pub mod native;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexAdapterError {
    WrongHost,
    NativeQualificationPending,
    Spool(HostEventSpoolError),
}

/// No plugin discovery or native admission is inferred from environment markers.
#[derive(Debug, Clone)]
pub struct CodexHostAdapter {
    detected: bool,
    spool: HostEventSpool,
    report: HostCapabilityReport,
}
impl CodexHostAdapter {
    /// Display one verified read-only graph projection, without Host-specific decoration.
    pub fn present_graph(&self, view: &receipts_orchestration::ReadOnlyGraphViewV1) -> String {
        crate::render_graph_tree(view)
    }

    /// Accepts a report selected by the existing observation composition.
    /// A report grants no authority; native delivery remains unavailable here.
    pub fn new(
        detected: bool,
        spool: HostEventSpool,
        report: HostCapabilityReport,
    ) -> Result<Self, CodexAdapterError> {
        if report.core().host_id() != "codex" {
            return Err(CodexAdapterError::WrongHost);
        }
        if report.core().selected_mode() != HostCapabilitySelectedMode::Supervised {
            return Err(CodexAdapterError::NativeQualificationPending);
        }
        Ok(Self {
            detected,
            spool,
            report,
        })
    }
}
impl HostAdapter for CodexHostAdapter {
    type DetectOutcome = bool;
    type InstallPlan = ();
    type InstallOutcome = &'static str;
    type CoreHandle = HostEventSpool;
    type EmitOutcome = Result<(), CodexAdapterError>;
    type CoreView = ClaudeCoreView;
    type PresentOutcome = String;
    type UserPrompt = String;
    type UserResponse = ClaudeUserResponse;
    type UserInputPending = Ready<ClaudeUserResponse>;
    type ShutdownReason = Option<&'static str>;
    type ShutdownOutcome = ();
    fn id(&self) -> HostId {
        HostId::Codex
    }
    fn detect(&self) -> bool {
        self.detected
    }
    fn install(&self, _: &()) -> Self::InstallOutcome {
        "Native plugin registration and hook trust qualification pending admitted Q01 inputs"
    }
    fn start(&self) -> HostEventSpool {
        self.spool.clone()
    }
    fn emit(&self, event: &NormalizedHostEvent) -> Self::EmitOutcome {
        if event.host().as_str() != "codex" {
            return Err(CodexAdapterError::WrongHost);
        }
        if event.confidence() != NormalizedHostEventConfidence::Inferred {
            return Err(CodexAdapterError::NativeQualificationPending);
        }
        self.spool.append(event).map_err(CodexAdapterError::Spool)
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
