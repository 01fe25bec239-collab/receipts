//! The argv-only local process-runner foundation.
//!
//! [`run`] executes exactly one program described by a
//! [`ProcessRunRequest`]: one explicit absolute executable path, arguments
//! traveling as discrete argv values, and a requested child working
//! directory that is proven — by canonical filesystem resolution, never by
//! lexical string comparison — to lie at or inside the canonical workspace
//! root.
//!
//! Frozen security properties of this slice:
//!
//! * **Absolute executable only.** The caller supplies an absolute path;
//!   there is no `PATH` lookup anywhere in this foundation, and
//!   unqualified names are rejected before the filesystem is touched.
//! * **Validated canonical executable.** The supplied path must resolve to
//!   an existing regular file carrying executable permission bits (on
//!   platforms exposing them), and it is the canonical (`realpath`) form —
//!   with symlink indirection removed — that the child actually executes.
//!   Recognized common shell basenames (`sh`, `bash`, `zsh`, `fish`,
//!   `dash`, `ksh`) are rejected both before and after resolution, so this
//!   runner cannot be used as a shell-command escape hatch such as
//!   `/bin/sh -c ...`. This is a bounded rejection list of common local
//!   shells, not a global shell database.
//! * **Realpath workspace boundary.** Both the workspace root and the
//!   requested working directory are canonicalized; membership requires
//!   component-wise containment of the resolved locations. A symlink
//!   leading outside the workspace or a `..` chain resolving past the root
//!   is refused; a sibling directory whose name merely shares a textual
//!   prefix with the root is not mistaken for containment. Any failed
//!   canonicalization fails closed.
//! * **Empty child environment.** Command construction starts from
//!   [`Command::env_clear`] and adds nothing back: the child inherits no
//!   parent variable — not `PATH`, `HOME`, credentials, proxies, or any
//!   other ambient state. Because the executable is an absolute validated
//!   path, no environment entry is required to locate it, and none is
//!   required by the spawn mechanism itself on supported platforms.
//! * **Non-interactive silent child.** stdin is null (the child observes
//!   immediate EOF), and stdout/stderr are null: this slice returns exit
//!   metadata only and deliberately implements no output capture.
//!
//! The runner waits normally for the child. A child that exits non-zero is
//! a successful runner invocation reported as typed [`ProcessRunOutcome`]
//! metadata, never an error; [`ExecutionError`] is reserved for validation
//! failures plus the typed spawn/wait boundaries. There is no timeout, no
//! terminate/kill handling, no retry, and no detach in this slice; those
//! belong to later runner slices.
//!
//! The workspace boundary provides workspace isolation only. It is NOT a
//! security sandbox: nothing here prevents a contained child from reaching
//! other filesystem locations, the network, other processes, or host
//! credentials. True isolation belongs to the runtime/host sandbox layer.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::execution::error::ExecutionError;
use crate::execution::outcome::ProcessRunOutcome;
use crate::execution::request::ProcessRunRequest;

/// Common local shell basenames rejected at executable validation.
const SHELL_BASENAMES: [&str; 6] = ["sh", "bash", "zsh", "fish", "dash", "ksh"];

/// Runs one validated process request to completion and returns its exit
/// metadata.
///
/// Validation order is fail-closed before any child exists: shell-basename
/// rejection, existence, canonicalization, regular-file and executable-
/// permission checks on the executable; then canonicalization and
/// directory/containment checks establishing the realpath workspace
/// boundary for the working directory. Only after every check passes does
/// construction of the child command begin, using the canonical executable
/// path and the canonical working directory.
pub fn run(request: &ProcessRunRequest) -> Result<ProcessRunOutcome, ExecutionError> {
    let executable = validated_executable(request.executable())?;
    let cwd = validated_workspace_cwd(request.workspace_root(), request.cwd())?;

    let mut command = prepared_command(&executable, &cwd);
    // Arguments travel verbatim as individual argv values; nothing joins,
    // splits, quotes, or interprets them because no shell exists here.
    command.args(request.arguments());

    let mut child = command.spawn().map_err(spawn_failed)?;
    let status = child.wait().map_err(wait_failed)?;

    Ok(ProcessRunOutcome::new(status.success(), status.code()))
}

/// Maps an `std::io::Error` from [`Command::spawn`] into the typed
/// spawn-failure variant. Extracted so tests can exercise the mapping
/// directly; see the spawn-failure test for why kernel-side argv overflow
/// is the only race-free way to reach this boundary once pre-validation
/// has closed every validation-shaped failure mode.
pub(crate) fn spawn_failed(error: std::io::Error) -> ExecutionError {
    ExecutionError::ProcessSpawnFailed {
        detail: error.to_string(),
    }
}

/// Maps an `std::io::Error` from [`std::process::Child::wait`] into the
/// typed wait-failure variant, keeping the spawn and wait boundaries
/// distinct.
pub(crate) fn wait_failed(error: std::io::Error) -> ExecutionError {
    ExecutionError::ProcessWaitFailed {
        detail: error.to_string(),
    }
}

/// Validates `supplied` as the explicit absolute executable and returns its
/// canonical form: the exact path handed to [`Command::new`].
///
/// Flow: reject recognized shell basenames lexically, require existence,
/// canonicalize (`realpath`), re-reject shells on the resolved name (which
/// catches symlinks aliased onto a shell), then require a regular file
/// carrying executable permission bits where the platform exposes them.
pub(crate) fn validated_executable(supplied: &Path) -> Result<PathBuf, ExecutionError> {
    reject_shell_basename(supplied)?;

    if let Err(error) = std::fs::metadata(supplied) {
        return Err(if error.kind() == ErrorKind::NotFound {
            ExecutionError::ExecutableNotFound {
                detail: format!("{}: {error}", supplied.display()),
            }
        } else {
            ExecutionError::ExecutableUnresolvable {
                detail: format!("{} could not be inspected: {error}", supplied.display()),
            }
        });
    }

    let canonical = std::fs::canonicalize(supplied).map_err(|error| {
        ExecutionError::ExecutableUnresolvable {
            detail: format!("{}: {error}", supplied.display()),
        }
    })?;

    reject_shell_basename(&canonical)?;

    let metadata =
        std::fs::metadata(&canonical).map_err(|error| ExecutionError::ExecutableUnresolvable {
            detail: format!("{}: {error}", canonical.display()),
        })?;
    if !metadata.is_file() {
        return Err(ExecutionError::ExecutableNotRegularFile {
            detail: format!("{} is not a regular file", canonical.display()),
        });
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(ExecutionError::ExecutableNotExecutable {
                detail: format!(
                    "{} carries none of the owner/group/other execute bits",
                    canonical.display()
                ),
            });
        }
    }
    Ok(canonical)
}

fn reject_shell_basename(path: &Path) -> Result<(), ExecutionError> {
    let Some(name) = path.file_name() else {
        return Ok(());
    };
    let name = name.to_string_lossy().to_lowercase();
    if SHELL_BASENAMES.contains(&name.as_str()) {
        Err(ExecutionError::ShellExecutableRejected { name })
    } else {
        Ok(())
    }
}

/// Establishes the realpath workspace boundary for the requested child
/// working directory and returns its canonical form.
///
/// Both paths are canonicalized independently (any failure fails closed),
/// the canonical working directory must be a directory, and membership is
/// decided with component-wise path semantics on the canonical forms:
/// equal to the root or a descendant of it. String-prefix similarity never
/// counts as containment.
pub(crate) fn validated_workspace_cwd(
    workspace_root: &Path,
    requested: &Path,
) -> Result<PathBuf, ExecutionError> {
    let canonical_workspace_root = std::fs::canonicalize(workspace_root).map_err(|error| {
        ExecutionError::WorkspaceRootUnresolvable {
            detail: format!("{}: {error}", workspace_root.display()),
        }
    })?;
    let canonical_cwd =
        std::fs::canonicalize(requested).map_err(|error| ExecutionError::CwdUnresolvable {
            detail: format!("{}: {error}", requested.display()),
        })?;
    if !canonical_cwd.is_dir() {
        return Err(ExecutionError::CwdNotADirectory {
            detail: format!("{} is not a directory", canonical_cwd.display()),
        });
    }
    // `Path::starts_with` compares whole components, so a sibling whose
    // name shares a textual prefix with the root is correctly excluded,
    // while equality and descendant relationships pass.
    if !canonical_cwd.starts_with(&canonical_workspace_root) {
        return Err(ExecutionError::CwdOutsideWorkspace {
            requested: requested.display().to_string(),
            canonical_cwd: canonical_cwd.display().to_string(),
            canonical_workspace_root: canonical_workspace_root.display().to_string(),
        });
    }
    Ok(canonical_cwd)
}

/// Builds the exact production child [`Command`] shape for this foundation.
///
/// The program is the already-validated canonical absolute executable, the
/// working directory is the already-validated canonical location inside the
/// workspace, the environment is cleared and left empty, and all three
/// standard streams are detached: stdin null (immediate EOF for the child),
/// stdout and stderr null (no capture machinery exists in this slice).
/// Nothing here spawns; tests may inspect the configured command.
pub(crate) fn prepared_command(executable: &Path, cwd: &Path) -> Command {
    let mut command = Command::new(executable);
    // Closed minimal environment policy for this foundation: the allowlist
    // is intentionally EMPTY. The child inherits nothing from the parent —
    // not PATH, HOME, USER, SHELL, credential, proxy, or vendor variables.
    // The executable is absolute, so locating it requires no PATH, and the
    // spawn mechanism itself requires no environment entries.
    command.env_clear();
    command.current_dir(cwd);
    command.stdin(Stdio::null());
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command
}
