//! Explicit-argv Git invocation boundary for provisioning.
//!
//! Every Git operation in this crate spawns the `git` executable through
//! [`std::process::Command`] with arguments supplied individually as argv
//! values. No shell is ever involved: there is no `sh -c`, `bash -c`, or
//! `zsh -c` anywhere in this crate, no constructed shell command strings,
//! and no interpolation of repository contents, branch names, paths, or any
//! other data into shell syntax. Arguments travel as separate process
//! arguments and directories as path values, so untrusted input can never
//! become command syntax.
//!
//! This boundary is deliberately Git-specific and minimal. It is not a
//! generic process-runner facility: no timeouts, no terminate/kill
//! handling, no streaming, and no output digests exist at this slice. The
//! only capability provided is running one Git command and capturing its
//! exit status plus its stdout/stderr text.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

use crate::error::WorkspaceError;

/// Captured result of one completed Git invocation.
#[derive(Debug)]
pub(crate) struct GitCapture {
    /// Whether the process exited successfully.
    pub success: bool,
    /// Exit status rendering, e.g. `exit status: 128`.
    pub exit_status: String,
    /// Lossily decoded stdout.
    pub stdout: String,
    /// Lossily decoded stderr.
    pub stderr: String,
}

impl GitCapture {
    /// Converts an unsuccessful capture into the typed Git-command failure
    /// for `operation`; passes successful captures through unchanged.
    pub(crate) fn require_success(self, operation: &'static str) -> Result<Self, WorkspaceError> {
        if self.success {
            Ok(self)
        } else {
            Err(WorkspaceError::GitCommandFailed {
                operation,
                detail: format!("{}; stderr: {}", self.exit_status, self.stderr.trim()),
            })
        }
    }
}

/// Runs one `git` command inside `directory` and captures its output.
///
/// The program name is fixed (`git`); `args` are appended verbatim as
/// individual argv values and never interpreted by any shell.
pub(crate) fn capture(
    directory: &Path,
    operation: &'static str,
    args: &[&OsStr],
) -> Result<GitCapture, WorkspaceError> {
    let output = Command::new("git")
        .current_dir(directory)
        .args(args)
        .output()
        .map_err(|error| WorkspaceError::GitSpawnFailed {
            operation,
            detail: error.to_string(),
        })?;
    Ok(GitCapture {
        success: output.status.success(),
        exit_status: output.status.to_string(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}
