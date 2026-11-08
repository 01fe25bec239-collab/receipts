//! Admitted composition of Host's probe port over the Workspace runner.
//!
//! [`WorkspaceHostProbeRunner`] is the only place a Host probe process is
//! launched. It holds one absolute, canonical, verified executable (with its
//! content digest), an explicit [`ChildEnvPolicy`], and runs each probe
//! through Workspace's bounded runner: cleared environment plus the policy's
//! names only, deadline with process-group termination, bounded capture.
//! There is no PATH lookup and no fallback to an unqualified `claude`.
//!
//! Drift detection, not immutable binding: before and after every probe, and
//! on every [`HostProbeProcessPort::executable`] call, the canonical path is
//! re-resolved, re-opened without following a final symlink and re-hashed
//! (streaming). Any difference from the digest admitted at construction is
//! refused as `ExecutableChanged` and the probe output is discarded. The
//! process itself is still launched by pathname, so a replacement that is
//! swapped in and restored entirely within one probe's lifetime is not
//! detected; binding execution to the hashed object would need an
//! execute-by-descriptor primitive the Workspace runner does not provide.

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

/// Streaming SHA-256 of the regular, non-symlink file at the already
/// canonical `path`; `None` when it no longer is one.
///
/// Portable checks only (no per-architecture `O_NOFOLLOW` constant): the
/// path must still canonicalize to itself and `lstat` as a regular file, and
/// the opened descriptor must be that same (device, inode) object.
fn content_digest(path: &Path) -> Option<String> {
    use std::io::Read;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    if std::fs::canonicalize(path).ok()?.as_path() != path {
        return None;
    }
    let listed = std::fs::symlink_metadata(path).ok()?;
    if !listed.file_type().is_file() {
        return None;
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    // O_NONBLOCK (same value on macOS and on Linux x86_64/aarch64): a FIFO
    // swapped in after the lstat cannot block the open.
    #[cfg(target_os = "macos")]
    options.custom_flags(0x0004);
    #[cfg(target_os = "linux")]
    options.custom_flags(0x0800);
    let mut file = options.open(path).ok()?;
    let opened = file.metadata().ok()?;
    if !opened.is_file() || (opened.dev(), opened.ino()) != (listed.dev(), listed.ino()) {
        return None;
    }
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    loop {
        match file.read(&mut buffer).ok()? {
            0 => break,
            n => hasher.update(&buffer[..n]),
        }
    }
    Some(
        hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
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
        let content_sha256 = content_digest(&canonical).ok_or(HostProbeRunnerError::Unreadable)?;
        Ok(Self {
            identity: HostExecutableIdentityV1 {
                canonical_path: canonical,
                content_sha256,
                // Value-free policy description (provider, auth mode, names).
                probe_policy: format!("{env:?}"),
            },
            env,
        })
    }

    fn executable_path(&self) -> &PathBuf {
        &self.identity.canonical_path
    }

    /// Refuses unless the pathname still holds exactly the admitted bytes.
    fn verify_unchanged(&self) -> Result<(), Error> {
        match content_digest(self.executable_path()) {
            Some(digest) if digest == self.identity.content_sha256 => Ok(()),
            _ => Err(Error::ExecutableChanged),
        }
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
        self.verify_unchanged()?;
        Ok(self.identity.clone())
    }

    fn run(&self, command: &HostProbeCommandV1) -> Result<Vec<u8>, Error> {
        self.verify_unchanged()?;
        let output = self.run_unverified(command);
        // Output produced while the admitted bytes were replaced is discarded.
        self.verify_unchanged()?;
        output
    }
}

impl WorkspaceHostProbeRunner {
    fn run_unverified(&self, command: &HostProbeCommandV1) -> Result<Vec<u8>, Error> {
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
