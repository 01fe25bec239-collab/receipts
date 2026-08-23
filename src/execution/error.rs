//! Fail-closed error surface for the argv-only local process-runner
//! foundation.
//!
//! Every failure mode of validating a run request or executing one child
//! process surfaces as an explicit [`ExecutionError`]. A child that exits
//! non-zero is deliberately NOT an error: it is a successful runner
//! invocation reported as typed exit metadata.

use std::fmt;

/// Errors produced by the local process-runner foundation.
///
/// Validation errors are raised before any child exists; execution errors
/// distinguish the spawn boundary from the wait boundary so callers can
/// tell "the program never started" from "the program started but waiting
/// for it failed".
#[derive(Debug)]
#[non_exhaustive]
pub enum ExecutionError {
    /// The supplied executable path was not syntactically absolute.
    ///
    /// This runner never performs `PATH` lookups, so unqualified names
    /// such as `git` or `python` are rejected before anything touches the
    /// filesystem.
    ExecutablePathNotAbsolute {
        /// The rejected value, preserved exactly for diagnostics.
        value: String,
    },
    /// The supplied workspace root was not syntactically absolute.
    WorkspaceRootNotAbsolute {
        /// The rejected value, preserved exactly for diagnostics.
        value: String,
    },
    /// The requested child working directory was not syntactically
    /// absolute.
    CwdNotAbsolute {
        /// The rejected value, preserved exactly for diagnostics.
        value: String,
    },
    /// The supplied executable path does not exist on the filesystem.
    ExecutableNotFound {
        /// The rejected path and the underlying filesystem failure.
        detail: String,
    },
    /// The executable path could not be resolved to its canonical
    /// (realpath) form, so no validated absolute executable exists.
    ExecutableUnresolvable {
        /// The rejected path and the underlying resolution failure.
        detail: String,
    },
    /// The resolved executable exists but is not a regular file (for
    /// example a directory or a device).
    ExecutableNotRegularFile {
        /// The rejected path and why it is not a regular file.
        detail: String,
    },
    /// The resolved executable is a regular file but carries no
    /// executable permission bits (platforms that expose permission
    /// semantics only).
    ExecutableNotExecutable {
        /// The rejected path and the observed permission state.
        detail: String,
    },
    /// The executable's basename is a recognized common shell interpreter.
    ///
    /// This is a process runner, not a shell runner: rejecting common
    /// shells at validation prevents the foundation from becoming a shell
    /// command-string escape hatch such as `/bin/sh -c ...`.
    ShellExecutableRejected {
        /// The rejected basename.
        name: String,
    },
    /// The workspace root could not be canonicalized to its realpath form,
    /// so the containment boundary cannot be established. Fails closed.
    WorkspaceRootUnresolvable {
        /// The rejected path and the underlying resolution failure.
        detail: String,
    },
    /// The requested child working directory could not be canonicalized to
    /// its realpath form (nonexistent path, dangling link). Fails closed.
    CwdUnresolvable {
        /// The rejected path and the underlying resolution failure.
        detail: String,
    },
    /// The canonicalized requested working directory is not a directory.
    CwdNotADirectory {
        /// The rejected path and what was observed instead.
        detail: String,
    },
    /// The canonicalized requested working directory lies outside the
    /// canonicalized workspace root — including symlink escapes, `..`
    /// chains that resolve past the root, and sibling directories whose
    /// names merely share a textual prefix with the root.
    CwdOutsideWorkspace {
        /// The originally requested working-directory path.
        requested: String,
        /// Where the requested path actually resolves on the filesystem.
        canonical_cwd: String,
        /// The canonical workspace root membership was checked against.
        canonical_workspace_root: String,
    },
    /// The child process could not be spawned.
    ///
    /// Distinct from [`ExecutionError::ProcessWaitFailed`]: the process
    /// never ran, so no exit status exists.
    ProcessSpawnFailed {
        /// Underlying spawn failure detail.
        detail: String,
    },
    /// The child was spawned successfully but waiting for it to complete
    /// failed.
    ProcessWaitFailed {
        /// Underlying wait failure detail.
        detail: String,
    },
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExecutionError::ExecutablePathNotAbsolute { value } => write!(
                f,
                "executable path {value:?} is not absolute; this runner requires one explicit absolute executable and performs no PATH lookup"
            ),
            ExecutionError::WorkspaceRootNotAbsolute { value } => write!(
                f,
                "workspace root {value:?} is not absolute; supply an explicit absolute workspace root"
            ),
            ExecutionError::CwdNotAbsolute { value } => write!(
                f,
                "requested working directory {value:?} is not absolute; supply an explicit absolute working directory"
            ),
            ExecutionError::ExecutableNotFound { detail } => {
                write!(f, "executable does not exist: {detail}")
            }
            ExecutionError::ExecutableUnresolvable { detail } => write!(
                f,
                "executable path could not be resolved to its canonical location: {detail}"
            ),
            ExecutionError::ExecutableNotRegularFile { detail } => {
                write!(f, "executable is not a regular file: {detail}")
            }
            ExecutionError::ExecutableNotExecutable { detail } => write!(
                f,
                "executable is a regular file without executable permission bits: {detail}"
            ),
            ExecutionError::ShellExecutableRejected { name } => write!(
                f,
                "executable {name:?} is a recognized shell interpreter; this foundation runs programs through argv, not shells"
            ),
            ExecutionError::WorkspaceRootUnresolvable { detail } => write!(
                f,
                "workspace root could not be resolved to its canonical location: {detail}"
            ),
            ExecutionError::CwdUnresolvable { detail } => write!(
                f,
                "requested working directory could not be resolved to its canonical location: {detail}"
            ),
            ExecutionError::CwdNotADirectory { detail } => {
                write!(
                    f,
                    "requested working directory is not a directory: {detail}"
                )
            }
            ExecutionError::CwdOutsideWorkspace {
                requested,
                canonical_cwd,
                canonical_workspace_root,
            } => write!(
                f,
                "requested working directory {requested:?} resolves to {canonical_cwd:?}, which is outside the workspace root {canonical_workspace_root:?}"
            ),
            ExecutionError::ProcessSpawnFailed { detail } => {
                write!(f, "failed to spawn child process: {detail}")
            }
            ExecutionError::ProcessWaitFailed { detail } => {
                write!(f, "failed while waiting for child process: {detail}")
            }
        }
    }
}

impl std::error::Error for ExecutionError {}
