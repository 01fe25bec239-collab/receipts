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
//! Every operation is additionally hardened (WE-W1-1): fixed `-c` overrides
//! disable hooks, fsmonitor, signing programs and transports
//! ([`HARDENED_CONFIG`]); a fixed environment pins configuration sources and
//! disables prompts ([`HARDENED_ENV`]); repositories declaring filters or
//! includes are refused before content-touching operations; and every run
//! goes through the crate's bounded runner with a deadline, process-group
//! cleanup and bounded capture. Oversize output is a typed error, never
//! parsed. This is Git-specific hardening, not a sandbox.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use crate::error::WorkspaceError;
use crate::execution::{ProcessRunRequest, ProcessTimeoutPolicy, run_with_timeout_and_capture};

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
    /// Exact stdout bytes; identity evidence must never pass through the
    /// lossy text above.
    raw_stdout: Vec<u8>,
}

impl GitCapture {
    /// The exact bytes Git wrote to stdout.
    pub(crate) fn raw_stdout(&self) -> &[u8] {
        &self.raw_stdout
    }

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

/// Repository-independent configuration pinned on every Workspace Git call.
///
/// Command-line `-c` entries outrank repository configuration, so repository
/// hooks, fsmonitor daemons, signing programs and transports can never run
/// inside an orchestrator-owned Git operation. Filters and includes cannot be
/// neutralized without changing content semantics; they are refused by
/// [`refuse_untrusted_repository_config`] instead.
pub(crate) const HARDENED_CONFIG: [&str; 14] = [
    "-c",
    "core.hooksPath=/dev/null",
    "-c",
    "core.fsmonitor=false",
    "-c",
    "core.untrackedCache=false",
    "-c",
    "commit.gpgSign=false",
    "-c",
    "tag.gpgSign=false",
    "-c",
    "protocol.allow=never",
    "-c",
    "core.splitIndex=false",
];

/// Fixed child environment; nothing is inherited from the parent.
///
/// * `GIT_CONFIG_NOSYSTEM`/`GIT_CONFIG_GLOBAL` — host and user configuration
///   cannot alter repository operations (`HOME` is not inherited either).
/// * `GIT_TERMINAL_PROMPT=0` — Git never waits for interactive input.
/// * `GIT_NO_REPLACE_OBJECTS=1` — replace refs cannot substitute objects.
pub(crate) const HARDENED_ENV: [(&str, &str); 4] = [
    ("GIT_CONFIG_NOSYSTEM", "1"),
    ("GIT_CONFIG_GLOBAL", "/dev/null"),
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_NO_REPLACE_OBJECTS", "1"),
];

/// Deadline for one local Git operation. Local Workspace operations never
/// fetch; exceeding this is a typed failure, never a partial success.
pub(crate) const GIT_RUN_TIMEOUT: Duration = Duration::from_secs(120);
/// Grace between group SIGTERM and SIGKILL for a timed-out Git operation.
pub(crate) const GIT_TERMINATION_GRACE: Duration = Duration::from_secs(2);

fn hardened_args(args: &[&OsStr]) -> Vec<OsString> {
    HARDENED_CONFIG
        .iter()
        .map(OsString::from)
        .chain(args.iter().map(|arg| arg.to_os_string()))
        .collect()
}

/// Builds the exact hardened child-Git [`Command`] for `operation`.
///
/// The command runs the resolved absolute canonical `git` executable inside
/// the canonicalized form of `directory`, with [`HARDENED_CONFIG`] followed
/// by `args` as individual argv values, under [`HARDENED_ENV`] only. Nothing
/// here spawns a process; tests use it to inspect and drive fixtures.
#[cfg(test)]
pub(crate) fn prepared_command(
    directory: &Path,
    operation: &'static str,
    args: &[&OsStr],
) -> Result<Command, WorkspaceError> {
    let executable = resolved_git_executable()?;
    let cwd = subprocess_cwd(directory, operation)?;

    let mut command = Command::new(executable);
    command.env_clear();
    command.envs(HARDENED_ENV);
    command.current_dir(cwd);
    command.args(hardened_args(args));
    Ok(command)
}

/// One completed hardened Git run with complete, bounded raw streams.
#[derive(Debug)]
pub(crate) struct GitRun {
    pub(crate) success: bool,
    pub(crate) code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) stdout_total: u64,
    pub(crate) stderr_total: u64,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
}

/// Runs one hardened Git command through the crate's accepted bounded
/// runner: dedicated process group, monotonic deadline, SIGTERM → grace →
/// SIGKILL, verified reap/group emptiness and separate bounded capture.
///
/// A timeout is [`WorkspaceError::GitTimedOut`]; any other runner failure is
/// [`WorkspaceError::GitSpawnFailed`]. Streams beyond the runner's retention
/// bound are reported with `*_truncated` and must not be parsed.
pub(crate) fn run(
    directory: &Path,
    operation: &'static str,
    args: &[&OsStr],
) -> Result<GitRun, WorkspaceError> {
    let executable = resolved_git_executable()?;
    let cwd = subprocess_cwd(directory, operation)?;
    let request = ProcessRunRequest::new(executable, hardened_args(args), &cwd, &cwd)
        .map_err(|error| WorkspaceError::GitSpawnFailed {
            operation,
            detail: error.to_string(),
        })?
        .with_fixed_env(&HARDENED_ENV);
    run_request(&request, operation)
}

/// Supplies the confinement for Git run by the protected controller.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) trait ControllerLaunch: Send + Sync {
    fn customization(&self) -> Result<crate::execution::LaunchCustomization, String>;

    /// A confined run failed to start or to be collected, so its process
    /// containment is not proven. The protected release must then retain
    /// custody instead of returning ownership.
    fn cleanup_unproven(&self) {}
}

thread_local! {
    static CONTROLLER: std::cell::RefCell<Option<std::sync::Arc<dyn ControllerLaunch>>> =
        const { std::cell::RefCell::new(None) };
}

/// While alive, every hardened Git call on this thread runs as the confined
/// controller (fresh confinement and containment per call). Restores the
/// previous scope on drop.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct ControllerScope(Option<std::sync::Arc<dyn ControllerLaunch>>);

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl ControllerScope {
    pub(crate) fn enter(launch: std::sync::Arc<dyn ControllerLaunch>) -> Self {
        Self(CONTROLLER.with(|slot| slot.borrow_mut().replace(launch)))
    }
}

impl Drop for ControllerScope {
    fn drop(&mut self) {
        let previous = self.0.take();
        CONTROLLER.with(|slot| *slot.borrow_mut() = previous);
    }
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn run_confined(
    request: &ProcessRunRequest,
    operation: &'static str,
    launch: &dyn ControllerLaunch,
    policy: &ProcessTimeoutPolicy,
) -> Result<GitRun, WorkspaceError> {
    use crate::execution::LiveProcessTerminalCause;
    let failed = |detail: String| WorkspaceError::GitSpawnFailed { operation, detail };
    let customization = launch.customization().map_err(failed)?;
    let attempt = crate::execution::live_attempt_start_customized(request, policy, customization)
        .map_err(|error| {
        launch.cleanup_unproven();
        failed(error.to_string())
    })?;
    let outcome = attempt.wait_collect().map_err(|error| {
        launch.cleanup_unproven();
        failed(error.to_string())
    })?;
    if outcome.terminal_cause() == LiveProcessTerminalCause::TimedOut {
        return Err(WorkspaceError::GitTimedOut {
            operation,
            timeout: policy.run_timeout(),
        });
    }
    let joined = |stream: &crate::execution::CapturedStream| {
        let mut bytes = stream.head().to_vec();
        bytes.extend_from_slice(stream.tail());
        bytes
    };
    Ok(GitRun {
        success: outcome.success(),
        code: outcome.exit_code(),
        stdout: joined(outcome.stdout()),
        stderr: joined(outcome.stderr()),
        stdout_total: outcome.stdout().total_bytes(),
        stderr_total: outcome.stderr().total_bytes(),
        stdout_truncated: outcome.stdout().truncated(),
        stderr_truncated: outcome.stderr().truncated(),
    })
}

/// Runs one already-built request under the Git deadline policy.
pub(crate) fn run_request(
    request: &ProcessRunRequest,
    operation: &'static str,
) -> Result<GitRun, WorkspaceError> {
    let failed = |error: crate::execution::ExecutionError| WorkspaceError::GitSpawnFailed {
        operation,
        detail: error.to_string(),
    };
    let policy =
        ProcessTimeoutPolicy::new(git_run_timeout(), GIT_TERMINATION_GRACE).map_err(failed)?;
    if let Some(launch) = CONTROLLER.with(|slot| slot.borrow().clone()) {
        return run_confined(request, operation, launch.as_ref(), &policy);
    }
    let captured = run_with_timeout_and_capture(request, &policy).map_err(failed)?;
    let outcome = captured.outcome();
    if outcome.timed_out() {
        return Err(WorkspaceError::GitTimedOut {
            operation,
            timeout: git_run_timeout(),
        });
    }
    let joined = |stream: &crate::execution::CapturedStream| {
        let mut bytes = stream.head().to_vec();
        bytes.extend_from_slice(stream.tail());
        bytes
    };
    Ok(GitRun {
        success: outcome.success(),
        code: outcome.exit_code(),
        stdout: joined(captured.stdout()),
        stderr: joined(captured.stderr()),
        stdout_total: captured.stdout().total_bytes(),
        stderr_total: captured.stderr().total_bytes(),
        stdout_truncated: captured.stdout().truncated(),
        stderr_truncated: captured.stderr().truncated(),
    })
}

#[cfg(test)]
thread_local! {
    static TEST_GIT_TIMEOUT: std::cell::Cell<Option<Duration>> = const { std::cell::Cell::new(None) };
}

#[cfg(test)]
pub(crate) struct GitTimeoutGuard(Option<Duration>);

#[cfg(test)]
impl Drop for GitTimeoutGuard {
    fn drop(&mut self) {
        TEST_GIT_TIMEOUT.set(self.0);
    }
}

/// Test-only deadline override for this thread.
#[cfg(test)]
pub(crate) fn override_git_timeout_for_test(timeout: Duration) -> GitTimeoutGuard {
    GitTimeoutGuard(TEST_GIT_TIMEOUT.replace(Some(timeout)))
}

fn git_run_timeout() -> Duration {
    #[cfg(test)]
    if let Some(timeout) = TEST_GIT_TIMEOUT.get() {
        return timeout;
    }
    GIT_RUN_TIMEOUT
}

/// Runs one hardened Git command and requires complete (untruncated) stdout,
/// which parsers may then consume byte-exactly. Exit status is reported, not
/// judged: callers decide what a nonzero exit means.
pub(crate) fn capture(
    directory: &Path,
    operation: &'static str,
    args: &[&OsStr],
) -> Result<GitCapture, WorkspaceError> {
    let run = run(directory, operation, args)?;
    if run.stdout_truncated {
        return Err(WorkspaceError::GitOutputTooLarge {
            operation,
            total_bytes: run.stdout_total,
        });
    }
    let exit_status = match run.code {
        Some(code) => format!("exit status: {code}"),
        None => "terminated by signal".to_string(),
    };
    Ok(GitCapture {
        success: run.success,
        exit_status,
        stdout: String::from_utf8_lossy(&run.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&run.stderr).into_owned(),
        raw_stdout: run.stdout,
    })
}

/// Refuses repositories whose effective configuration declares content
/// filters or configuration includes.
///
/// Filters (`filter.*`) run arbitrary programs during checkout/status and
/// cannot be disabled without changing content semantics. Includes
/// (`include.*`, `includeIf.*`) can pull configuration from outside the
/// protected repository metadata; this boundary refuses all of them rather
/// than reasoning about include custody. Values are never read or echoed.
pub(crate) fn refuse_untrusted_repository_config(
    root: &Path,
    operation: &'static str,
) -> Result<(), WorkspaceError> {
    let listing = capture(
        root,
        operation,
        &[
            OsStr::new("config"),
            OsStr::new("--null"),
            OsStr::new("--name-only"),
            OsStr::new("--get-regexp"),
            OsStr::new("^(filter|include|includeif)\\."),
        ],
    )?;
    if listing.success {
        let key = listing
            .raw_stdout()
            .split(|byte| *byte == 0)
            .next()
            .map(|key| String::from_utf8_lossy(key).into_owned())
            .unwrap_or_default();
        return Err(WorkspaceError::RepositoryConfigRefused { key });
    }
    // `git config --get-regexp` exits 1 exactly when nothing matched.
    if listing.exit_status == "exit status: 1" && listing.raw_stdout().is_empty() {
        return Ok(());
    }
    Err(WorkspaceError::GitCommandFailed {
        operation,
        detail: format!("{}; stderr: {}", listing.exit_status, listing.stderr.trim()),
    })
}

/// Parses `git version` output and requires at least `major.minor`.
pub(crate) fn git_version_at_least(output: &str, major: u32, minor: u32) -> Option<bool> {
    let rest = output.trim().strip_prefix("git version ")?;
    let mut parts = rest.split(|c: char| !c.is_ascii_digit());
    let found_major: u32 = parts.next()?.parse().ok()?;
    let found_minor: u32 = parts.next()?.parse().ok()?;
    Some((found_major, found_minor) >= (major, minor))
}
