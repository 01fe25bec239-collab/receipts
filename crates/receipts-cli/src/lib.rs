//! Admitted composition of Host's probe port over the Workspace runner.
//!
//! [`WorkspaceHostProbeRunner`] is the only place a Host probe process is
//! launched. It holds one absolute, canonical, verified executable (with its
//! content digest), an explicit [`ChildEnvPolicy`], and runs each probe
//! through Workspace's bounded runner: cleared environment plus the policy's
//! names only, deadline with process-group termination, bounded capture.
//! There is no PATH lookup and no fallback to an unqualified `claude`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use receipts_host_integration::host_capability_observation::HostCapabilityObservationError as Error;
use receipts_host_integration::{
    HostExecutableIdentityV1, HostProbeCommandV1, HostProbeProcessPort,
};
use receipts_workspace_execution::execution::{
    ChildEnvPolicy, ExecutionError, ProcessRunRequest, ProcessTimeoutPolicy,
    run_with_timeout_and_capture,
};
use sha2::{Digest, Sha256};

/// Grace after the deadline before the probe's process group is force-killed.
pub const HOST_PROBE_TERMINATION_GRACE: Duration = Duration::from_secs(1);

/// Why a runner could not be admitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostProbeRunnerError {
    NotAbsolute,
    Unresolvable,
    NotRegularFile,
    NotExecutable,
    Unreadable,
}

/// Host probe port backed by the Workspace process runner.
#[derive(Debug, Clone)]
pub struct WorkspaceHostProbeRunner {
    identity: HostExecutableIdentityV1,
    env: ChildEnvPolicy,
}

impl WorkspaceHostProbeRunner {
    /// Admits `executable` only if it is absolute and resolves to a regular,
    /// executable file; records its canonical path and content digest.
    pub fn new(executable: &Path, env: ChildEnvPolicy) -> Result<Self, HostProbeRunnerError> {
        use std::os::unix::fs::PermissionsExt;
        if !executable.is_absolute() {
            return Err(HostProbeRunnerError::NotAbsolute);
        }
        let canonical =
            std::fs::canonicalize(executable).map_err(|_| HostProbeRunnerError::Unresolvable)?;
        let meta = std::fs::metadata(&canonical).map_err(|_| HostProbeRunnerError::Unresolvable)?;
        if !meta.is_file() {
            return Err(HostProbeRunnerError::NotRegularFile);
        }
        if meta.permissions().mode() & 0o111 == 0 {
            return Err(HostProbeRunnerError::NotExecutable);
        }
        let bytes = std::fs::read(&canonical).map_err(|_| HostProbeRunnerError::Unreadable)?;
        let digest = Sha256::digest(&bytes);
        Ok(Self {
            identity: HostExecutableIdentityV1 {
                canonical_path: canonical,
                content_sha256: digest.iter().map(|b| format!("{b:02x}")).collect(),
            },
            env,
        })
    }

    fn executable_path(&self) -> &PathBuf {
        &self.identity.canonical_path
    }
}

fn map_error(error: ExecutionError) -> Error {
    match error {
        ExecutionError::GracefulTerminationFailed { .. }
        | ExecutionError::TimeoutGraceWaitFailed { .. }
        | ExecutionError::ForceKillFailed { .. }
        | ExecutionError::TimeoutFinalWaitFailed { .. }
        | ExecutionError::ProcessGroupOwnershipFailed { .. }
        | ExecutionError::ProcessGroupControlFailed { .. } => Error::ProbeCleanupFailed,
        _ => Error::ProbeFailed,
    }
}

impl HostProbeProcessPort for WorkspaceHostProbeRunner {
    fn executable(&self) -> Result<HostExecutableIdentityV1, Error> {
        Ok(self.identity.clone())
    }

    fn run(&self, command: &HostProbeCommandV1) -> Result<Vec<u8>, Error> {
        let cwd = std::fs::canonicalize(&command.current_dir).map_err(|_| Error::Unavailable)?;
        let request = ProcessRunRequest::new(self.executable_path(), &command.args, &cwd, &cwd)
            .map_err(map_error)?
            .with_env_policy(self.env.clone());
        let policy = ProcessTimeoutPolicy::new(command.timeout, HOST_PROBE_TERMINATION_GRACE)
            .map_err(map_error)?;
        let run = run_with_timeout_and_capture(&request, &policy).map_err(map_error)?;
        if run.outcome().timed_out() {
            return Err(Error::ProbeTimeout);
        }
        let (stdout, stderr) = (run.stdout(), run.stderr());
        if stdout.truncated() || stdout.total_bytes() > command.stdout_max_bytes as u64 {
            return Err(Error::ProbeStdoutTooLarge);
        }
        if stderr.total_bytes() > command.stderr_max_bytes as u64 {
            return Err(Error::ProbeStderrTooLarge);
        }
        if !run.outcome().success() {
            // Partial stdout of a failed probe is never evidence.
            return Err(Error::ProbeFailed);
        }
        Ok([stdout.head(), stdout.tail()].concat())
    }
}
