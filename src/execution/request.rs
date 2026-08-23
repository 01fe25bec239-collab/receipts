//! The typed request accepted by the argv-only process runner.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::execution::error::ExecutionError;

/// A fully structured request to run exactly one local child process.
///
/// Construction enforces the frozen structural contract:
///
/// * `executable` is one explicit **absolute** path — never an unqualified
///   program name, because this runner performs no `PATH` lookup;
/// * `arguments` travel verbatim as discrete argv values. No argument is
///   ever split, joined, quoted, or otherwise interpreted: there is no
///   shell anywhere in this foundation;
/// * `workspace_root` and `cwd` are explicit absolute paths whose
///   canonical (realpath) containment relationship is proven at run time.
///
/// All fields are private; accessors expose read-only views. Filesystem
/// validation (existence, regular file, executable permission bits,
/// shell-basename rejection, canonicalization, containment) happens inside
/// [`run`](crate::run), immediately before spawning, so no stale
/// validated request can exist and every check fails closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRunRequest {
    executable: PathBuf,
    arguments: Vec<OsString>,
    workspace_root: PathBuf,
    cwd: PathBuf,
}

impl ProcessRunRequest {
    /// Validates and assembles a process-run request.
    ///
    /// Arguments are accepted as an iterator of anything that converts into
    /// an [`OsString`]; each element becomes exactly one argv value.
    pub fn new(
        executable: impl Into<PathBuf>,
        arguments: impl IntoIterator<Item = impl Into<OsString>>,
        workspace_root: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
    ) -> Result<Self, ExecutionError> {
        let executable = executable.into();
        let workspace_root = workspace_root.into();
        let cwd = cwd.into();

        if !executable.is_absolute() {
            return Err(ExecutionError::ExecutablePathNotAbsolute {
                value: executable.display().to_string(),
            });
        }
        if !workspace_root.is_absolute() {
            return Err(ExecutionError::WorkspaceRootNotAbsolute {
                value: workspace_root.display().to_string(),
            });
        }
        if !cwd.is_absolute() {
            return Err(ExecutionError::CwdNotAbsolute {
                value: cwd.display().to_string(),
            });
        }

        Ok(Self {
            executable,
            arguments: arguments.into_iter().map(Into::into).collect(),
            workspace_root,
            cwd,
        })
    }

    /// The absolute path of the program to execute.
    ///
    /// This is the caller-supplied spelling; the canonical form actually
    /// executed is derived at run time by the runner.
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// The discrete argv values, in order, exactly as supplied.
    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    /// The absolute workspace root the child's working directory must lie
    /// within.
    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// The absolute requested child working directory.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }
}
