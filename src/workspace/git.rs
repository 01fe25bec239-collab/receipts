//! Hardened explicit-argv Git invocation boundary for provisioning.
//!
//! Every Git operation in this crate spawns a resolved `git` executable
//! through [`std::process::Command`] with arguments supplied individually
//! as argv values. No shell is ever involved: there is no `sh -c`, `bash
//! -c`, or `zsh -c` anywhere in this crate, no constructed shell command
//! strings, and no interpolation of repository contents, branch names,
//! paths, or any other data into shell syntax. Arguments travel as separate
//! process arguments and directories as path values, so untrusted input can
//! never become command syntax.
//!
//! Three frozen security properties are enforced at this boundary:
//!
//! * **Absolute resolved executable.** The child program is never the
//!   unqualified name `"git"` looked up through an ambient `PATH` at spawn
//!   time. [`resolved_git_executable`] resolves one absolute canonical Git
//!   binary per process by inspecting candidate locations in Rust — never
//!   through a shell or external helper — verifies that the candidate is an
//!   executable regular file where the platform permits, and fails closed
//!   when nothing qualifies.
//! * **Canonical (realpath) working directory.** Every directory handed to
//!   [`std::process::Command::current_dir`] is first passed through
//!   [`std::fs::canonicalize`]; a path that cannot be canonicalized fails
//!   closed instead of executing against a lexical alias. This holds for
//!   every invocation because [`capture`] routes through
//!   [`subprocess_cwd`] unconditionally.
//! * **Allowlisted child environment.** Child Git processes inherit nothing
//!   from the parent environment ([`Command::env_clear`]). Ambient
//!   variables such as `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`,
//!   `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`,
//!   `GIT_COMMON_DIR`, `GIT_CONFIG*`, `GIT_CEILING_DIRECTORIES`,
//!   `GIT_EXEC_PATH`, `GIT_SSH*`, `GIT_ASKPASS`, or
//!   `GIT_TERMINAL_PROMPT` could redirect repository operations or inject
//!   configuration into the child; none of them survive this boundary. Only
//!   two fixed, non-sensitive entries are added back, each documented at
//!   its construction site below.
//!
//! This boundary is deliberately Git-specific and minimal. It is not a
//! generic process-runner facility: no timeouts, no terminate/kill
//! handling, no streaming, and no output digests exist at this slice. The
//! only capability provided is running one resolved-Git command inside one
//! canonical directory and capturing its exit status plus its stdout/stderr
//! text.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::error::WorkspaceError;

/// Process-wide cache of the resolved absolute canonical `git` executable.
///
/// Resolution is deterministic per host, so only successful resolutions are
/// cached; a failed lookup re-runs and keeps failing closed with fresh
/// diagnostics.
static RESOLVED_GIT_EXECUTABLE: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Standard filesystem locations searched after `PATH`, so provisioning
/// still works when `PATH` is empty or missing. All are absolute and
/// conventional for macOS/Linux hosts.
const FALLBACK_GIT_LOCATIONS: [&str; 4] = [
    "/usr/bin/git",
    "/bin/git",
    "/usr/local/bin/git",
    "/opt/homebrew/bin/git",
];

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

/// Resolves the one absolute, canonical `git` executable used by every
/// production subprocess in this crate.
///
/// The result is cached for the process lifetime. The resolver inspects the
/// current process's `PATH` entries plus [`FALLBACK_GIT_LOCATIONS`] purely
/// in Rust — no shell, no `which`, no external helper — accepts the first
/// candidate that is an executable regular file, and returns its canonical
/// (`realpath`) form. The parent's `PATH` is read only to locate the
/// binary; it is never forwarded to any child.
pub(crate) fn resolved_git_executable() -> Result<&'static Path, WorkspaceError> {
    let cached = RESOLVED_GIT_EXECUTABLE.get_or_init(resolve_git_executable);
    cached
        .as_deref()
        .ok_or_else(|| WorkspaceError::GitExecutableUnavailable {
            detail: format!(
                "searched every PATH entry and {}",
                FALLBACK_GIT_LOCATIONS.join(", ")
            ),
        })
}

/// Locates the first executable regular-file `git` among the candidate
/// locations and returns its canonical absolute path, if any.
fn resolve_git_executable() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path_value) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&path_value).map(|entry| entry.join("git")));
    }
    candidates.extend(FALLBACK_GIT_LOCATIONS.iter().map(PathBuf::from));

    for candidate in candidates {
        if !is_executable_file(&candidate) {
            continue;
        }
        // Canonicalize before accepting so the stored program path is both
        // absolute and free of symlink indirection.
        match std::fs::canonicalize(&candidate) {
            Ok(canonical) => return Some(canonical),
            Err(_) => continue,
        }
    }
    None
}

/// Whether `path` names an existing regular file that passes the platform's
/// executable-permission check.
#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// Whether `path` names an existing regular file (non-Unix platforms expose
/// no permission bits through the standard library).
#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.is_file(),
        Err(_) => false,
    }
}

/// Resolves `directory` to the exact working directory a production Git
/// subprocess must run from: its canonical (`realpath`) form.
///
/// Fails closed when the path cannot be canonicalized — nonexistent paths,
/// dangling links, and non-directory targets included — rather than falling
/// back to the original lexical path.
pub(crate) fn subprocess_cwd(
    directory: &Path,
    operation: &'static str,
) -> Result<PathBuf, WorkspaceError> {
    std::fs::canonicalize(directory).map_err(|error| WorkspaceError::SubprocessCwdUnresolvable {
        operation,
        detail: format!(
            "{:?} could not be canonicalized: {error}",
            directory.display()
        ),
    })
}

/// Builds the exact production child-Git [`Command`] for `operation`.
///
/// The returned command runs the resolved absolute canonical `git`
/// executable inside the canonicalized form of `directory`, with `args`
/// appended verbatim as individual argv values, under the fixed allowlisted
/// environment described below. Nothing here spawns a process; tests may
/// inspect the fully configured command without executing it.
///
/// Exposed at crate visibility only, and only for this Git boundary — this
/// is not a generic execution surface.
pub(crate) fn prepared_command(
    directory: &Path,
    operation: &'static str,
    args: &[&OsStr],
) -> Result<Command, WorkspaceError> {
    let executable = resolved_git_executable()?;
    let cwd = subprocess_cwd(directory, operation)?;

    let mut command = Command::new(executable);
    // The child inherits nothing: ambient Git-control variables (GIT_DIR,
    // GIT_WORK_TREE, GIT_INDEX_FILE, GIT_OBJECT_DIRECTORY,
    // GIT_ALTERNATE_OBJECT_DIRECTORIES, GIT_COMMON_DIR, GIT_CONFIG*,
    // GIT_CEILING_DIRECTORIES, GIT_EXEC_PATH, GIT_SSH*, GIT_ASKPASS,
    // GIT_TERMINAL_PROMPT) and arbitrary credential-related state must
    // never influence this bounded local operation.
    command.env_clear();
    // Fixed, non-sensitive allowlist entries required for deterministic
    // local behavior across macOS/Linux hosts:
    //
    // * GIT_CONFIG_NOSYSTEM=1 — ignore the system-wide /etc/gitconfig so
    //   ambient host configuration cannot alter provisioning behavior.
    // * GIT_CONFIG_GLOBAL=/dev/null — pin global configuration lookup to an
    //   empty source instead of depending on $HOME-derived paths (HOME is
    //   intentionally not inherited either). Repository-local .git/config
    //   still applies, as the repository being operated on.
    command.env("GIT_CONFIG_NOSYSTEM", "1");
    command.env("GIT_CONFIG_GLOBAL", "/dev/null");
    command.current_dir(cwd);
    command.args(args);
    Ok(command)
}

/// Runs one resolved-Git command whose working directory is the canonical
/// form of `directory`, capturing its output.
///
/// The program is the process-wide resolved absolute canonical Git binary;
/// `args` are appended verbatim as individual argv values and never
/// interpreted by any shell. Any preparation failure (unresolvable
/// executable or uncanonicalizable working directory) fails closed before a
/// child exists.
pub(crate) fn capture(
    directory: &Path,
    operation: &'static str,
    args: &[&OsStr],
) -> Result<GitCapture, WorkspaceError> {
    let mut command = prepared_command(directory, operation, args)?;
    let output = command
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
