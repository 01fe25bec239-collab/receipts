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
    pub(crate) fn fits_time_budget(&self, seconds: i64) -> bool {
        u64::try_from(seconds).is_ok_and(|seconds| {
            self.timeout.run_timeout() <= std::time::Duration::from_secs(seconds)
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn integer_time_budgets_never_round_up() {
        let seconds = 9_007_199_254_740_993_i64;
        let cap = Duration::from_secs(u64::try_from(seconds).unwrap());
        let policy = RuntimeExecutionPolicy::new(
            ProcessTimeoutPolicy::new(cap + Duration::from_nanos(1), Duration::from_secs(1))
                .unwrap(),
            false,
        );
        assert!(!policy.fits_time_budget(seconds));
        let policy = RuntimeExecutionPolicy::new(
            ProcessTimeoutPolicy::new(cap, Duration::from_secs(1)).unwrap(),
            false,
        );
        assert!(policy.fits_time_budget(seconds));
        assert!(!policy.fits_time_budget(0));
        assert!(!policy.fits_time_budget(-1));
    }
}
