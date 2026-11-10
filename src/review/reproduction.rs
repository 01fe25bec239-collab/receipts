//! Deterministic check reproduction through the Workspace argv runner.
//!
//! A command's `argv[0]` must name an executable the trusted composition
//! admitted (name → absolute path); nothing else runs, there is no shell and
//! no PATH lookup. A timeout, unadmitted executable or runner error is never
//! PASS.

use receipts_workspace_execution::execution::{
    ChildEnvPolicy, ProcessRunRequest, ProcessTimeoutPolicy, run_with_timeout_and_capture,
};
use std::path::{Path, PathBuf};

/// Trusted-composition configuration for every reproduced check.
#[derive(Debug, Clone)]
pub struct CheckRunnerConfig {
    /// `argv[0]` spelling → admitted absolute executable.
    pub executables: Vec<(String, PathBuf)>,
    pub timeout: ProcessTimeoutPolicy,
    pub env: Option<ChildEnvPolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckRunStatus {
    Passed,
    Failed,
    TimedOut,
    /// Not executed: unadmitted executable, empty argv or runner refusal.
    NotRunnable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRun {
    pub command: Vec<String>,
    pub status: CheckRunStatus,
    pub exit_code: Option<i32>,
    /// SHA-256 of complete stdout, when the stream was fully drained.
    pub stdout_sha256: Option<String>,
    pub detail: Option<String>,
}

impl CheckRun {
    fn not_runnable(command: &[String], detail: String) -> Self {
        Self {
            command: command.to_vec(),
            status: CheckRunStatus::NotRunnable,
            exit_code: None,
            stdout_sha256: None,
            detail: Some(detail),
        }
    }
}

/// Runs one argv command with `worktree` as both workspace root and cwd.
pub fn run_check(config: &CheckRunnerConfig, worktree: &Path, command: &[String]) -> CheckRun {
    let Some((program, args)) = command.split_first() else {
        return CheckRun::not_runnable(command, "empty argv".into());
    };
    let Some((_, executable)) = config.executables.iter().find(|(name, _)| name == program) else {
        return CheckRun::not_runnable(command, format!("executable {program:?} is not admitted"));
    };
    let request = match ProcessRunRequest::new(executable, args, worktree, worktree) {
        Ok(r) => r,
        Err(e) => return CheckRun::not_runnable(command, e.to_string()),
    };
    let request = match &config.env {
        Some(env) => request.with_env_policy(env.clone()),
        None => request,
    };
    match run_with_timeout_and_capture(&request, &config.timeout) {
        Err(e) => CheckRun::not_runnable(command, e.to_string()),
        Ok(run) => {
            let outcome = run.outcome();
            let status = if outcome.timed_out() {
                CheckRunStatus::TimedOut
            } else if outcome.success() {
                CheckRunStatus::Passed
            } else {
                CheckRunStatus::Failed
            };
            CheckRun {
                command: command.to_vec(),
                status,
                exit_code: outcome.exit_code(),
                stdout_sha256: run.stdout().digest().map(str::to_owned),
                detail: None,
            }
        }
    }
}
