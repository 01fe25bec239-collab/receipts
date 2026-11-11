//! Concrete Claude Code host adapter (HI-W2-2c) and hook translation.
//!
//! [`ClaudeHostAdapter`] binds the frozen [`HostAdapter`] placeholders for
//! the embedded Claude plugin. It is translation only: no orchestration,
//! routing, review, workspace or State logic, and no credentials. Its core
//! handle is the untrusted [`HostEventSpool`]; `emit` appends a validated
//! event there and nowhere else. Process execution (probes, Git) stays in
//! the admitted CLI composition; this crate never launches anything.

use std::future::{Ready, ready};
use std::path::{Path, PathBuf};

use receipts_orchestration::orchestration::{OrchestrationJsonObjectV1, OrchestrationJsonValueV1};

use crate::host_event_spool::{HostEventSpool, HostEventSpoolError};
use crate::{HostAdapter, HostCapabilityReport, HostId, NormalizedHostEvent};

pub mod hook_map;
#[cfg(test)]
mod tests;

pub use hook_map::{
    CLAUDE_HOOK_PAYLOAD_MAX_BYTES, ClaudeHookDetail, ClaudeHookError, ClaudeHookEvent,
    ClaudeHookInvocation, parse_claude_hook, utc_from_unix_millis, utc_now, worktree_create_stdout,
};

/// Host-set environment facts the bridge was launched with. Only exact
/// variable names are read by the caller; nothing is enumerated.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaudeHostEnvironment {
    /// `CLAUDECODE=1`, set by Claude Code for its child processes.
    pub claudecode: bool,
}

/// What the user must do; the adapter never installs or edits settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeInstallPlan {
    /// Absolute directory holding `.claude-plugin/plugin.json`.
    pub plugin_dir: PathBuf,
    /// Absolute `receipts` binary, when not bundled at `<plugin>/bin/receipts`.
    pub receipts_bin: Option<PathBuf>,
}

/// Frozen `CoreView` view types (`NORMALIZED_HOST_EVENTS.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeCoreViewType {
    GoalStatus,
    Dag,
    RoutingDecision,
    ReviewResult,
    IntegrationResult,
    GoalEvaluation,
    Error,
}

impl ClaudeCoreViewType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoalStatus => "goal_status",
            Self::Dag => "dag",
            Self::RoutingDecision => "routing_decision",
            Self::ReviewResult => "review_result",
            Self::IntegrationResult => "integration_result",
            Self::GoalEvaluation => "goal_evaluation",
            Self::Error => "error",
        }
    }
}

/// Structured core output; rendered with the zero-model TEMPLATE policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCoreView {
    pub view_type: ClaudeCoreViewType,
    pub content: OrchestrationJsonObjectV1,
}

/// Hooks cannot ask the user anything; elicitation is not available here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeUserResponse {
    Unavailable,
}

/// Claude Code adapter bound to one project's spool and current report.
#[derive(Debug, Clone)]
pub struct ClaudeHostAdapter {
    environment: ClaudeHostEnvironment,
    spool: HostEventSpool,
    report: HostCapabilityReport,
}

impl ClaudeHostAdapter {
    pub fn new(
        environment: ClaudeHostEnvironment,
        spool: HostEventSpool,
        report: HostCapabilityReport,
    ) -> Self {
        Self {
            environment,
            spool,
            report,
        }
    }
}

impl HostAdapter for ClaudeHostAdapter {
    type DetectOutcome = bool;
    type InstallPlan = ClaudeInstallPlan;
    /// Exact user steps; nothing is performed.
    type InstallOutcome = Vec<String>;
    type CoreHandle = HostEventSpool;
    type EmitOutcome = Result<(), HostEventSpoolError>;
    type CoreView = ClaudeCoreView;
    /// Text for the hook to print; display only.
    type PresentOutcome = String;
    type UserPrompt = String;
    type UserResponse = ClaudeUserResponse;
    type UserInputPending = Ready<ClaudeUserResponse>;
    type ShutdownReason = Option<&'static str>;
    type ShutdownOutcome = ();

    fn id(&self) -> HostId {
        HostId::ClaudeCode
    }

    fn detect(&self) -> bool {
        self.environment.claudecode
    }

    fn install(&self, plan: &ClaudeInstallPlan) -> Vec<String> {
        let dir = plan.plugin_dir.display();
        let mut steps = vec![
            format!("Load the plugin for local development: claude --plugin-dir {dir}"),
            "Or install it from a marketplace that lists it: /plugin install receipts@<marketplace>"
                .to_owned(),
        ];
        steps.push(match &plan.receipts_bin {
            Some(bin) => format!(
                "Set RECEIPTS_BIN={} in the environment Claude Code starts with",
                bin.display()
            ),
            None => format!("Place the receipts binary at {dir}/bin/receipts"),
        });
        steps.push(
            "Optional: set RECEIPTS_CLAUDE_EXECUTABLE to the absolute claude executable to \
             record capability reports at session start"
                .to_owned(),
        );
        steps.push(
            "Permission rules are not installed by the plugin; add any you want in your own \
             settings"
                .to_owned(),
        );
        steps
    }

    fn start(&self) -> HostEventSpool {
        self.spool.clone()
    }

    fn emit(&self, event: &NormalizedHostEvent) -> Result<(), HostEventSpoolError> {
        self.spool.append(event)
    }

    fn present(&self, view: &ClaudeCoreView) -> String {
        format!(
            "receipts {}: {}",
            view.view_type.as_str(),
            OrchestrationJsonValueV1::Object(view.content.clone()).to_json()
        )
    }

    fn request_user_input(&mut self, _prompt: String) -> Ready<ClaudeUserResponse> {
        ready(ClaudeUserResponse::Unavailable)
    }

    fn capabilities(&self) -> HostCapabilityReport {
        self.report.clone()
    }

    fn shutdown(self, _reason: Option<&'static str>) {}
}

/// Worktree location and branch for a validated `WorktreeCreate` name:
/// `<repository>/.claude/worktrees/<name>` on branch `worktree-<name>`,
/// matching Claude Code's default layout. Workspace validates both again.
pub fn worktree_target(repository_root: &Path, name: &str) -> (PathBuf, String) {
    (
        repository_root.join(".claude/worktrees").join(name),
        format!("worktree-{name}"),
    )
}
