//! Explicit requirements, not admission or a claim of OS confinement.
use receipts_workspace_execution::execution::ProcessTimeoutPolicy;

use crate::ProtectedLaunch;

/// `None` means no mandatory constraint on that axis. Unsupported mandatory
/// requirements are refused, never translated into prompt-only enforcement.
#[derive(Clone)]
pub struct RuntimeExecutionPolicy {
    pub timeout: ProcessTimeoutPolicy,
    pub read_only: bool,
    pub sandbox_mode: Option<String>,
    pub network_access: Option<bool>,
    pub filesystem_scope: Option<Vec<String>>,
    pub turn_budget: Option<u64>,
    pub structured_output_schema: Option<String>,
    /// Opt-in only for eligible task sessions; reviewers always refuse.
    pub session_persistence: bool,
    protected: Option<ProtectedLaunch>,
}
impl RuntimeExecutionPolicy {
    pub fn new(timeout: ProcessTimeoutPolicy, read_only: bool) -> Self {
        Self {
            timeout,
            read_only,
            sandbox_mode: None,
            network_access: None,
            filesystem_scope: None,
            turn_budget: None,
            structured_output_schema: None,
            session_persistence: false,
            protected: None,
        }
    }
    /// Trusted composition only: every start under this policy is a Workspace
    /// protected launch or a refusal, never an ordinary run.
    pub fn with_protected_launch(mut self, launch: ProtectedLaunch) -> Self {
        self.protected = Some(launch);
        self
    }
    pub fn protected_launch(&self) -> Option<&ProtectedLaunch> {
        self.protected.as_ref()
    }
    /// True only when no mandatory constraint beyond timeout/read-only is
    /// requested; adapters refuse otherwise instead of prompt-only enforcement.
    pub fn supported(&self) -> bool {
        self.sandbox_mode.is_none()
            && self.network_access.is_none()
            && self.filesystem_scope.is_none()
            && !self.session_persistence
            && self.turn_budget.is_none()
            && self.structured_output_schema.is_none()
    }
}
