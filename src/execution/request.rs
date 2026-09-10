//! The typed request accepted by the argv-only process runner.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::execution::error::ExecutionError;

/// Maximum raw stdin payload, checked before a request can admit it.
pub const MAX_STDIN_BYTES: usize = 1_048_576;

/// Immutable admitted bytes. There is no public mutable view or append operation.
///
/// ```compile_fail
/// use receipts_workspace_execution::execution::ProcessStdin;
/// let ProcessStdin::Bytes(mut bytes) = ProcessStdin::bytes(b"input").unwrap() else { unreachable!() };
/// bytes.as_bytes()[0] = 0;
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct BoundedStdinBytes(Box<[u8]>);

impl BoundedStdinBytes {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl std::fmt::Debug for BoundedStdinBytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundedStdinBytes")
            .field("len", &self.0.len())
            .field("redacted", &true)
            .finish()
    }
}

/// Physical stdin only: immediate EOF or one bounded immutable raw payload.
/// Empty bytes remain distinct from Closed. No encoding or newline conversion occurs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ProcessStdin {
    #[default]
    Closed,
    Bytes(BoundedStdinBytes),
}

impl ProcessStdin {
    pub fn bytes(bytes: impl AsRef<[u8]>) -> Result<Self, ExecutionError> {
        let bytes = bytes.as_ref();
        if bytes.len() > MAX_STDIN_BYTES {
            return Err(ExecutionError::StdinPayloadTooLarge {
                len: bytes.len(),
                max: MAX_STDIN_BYTES,
            });
        }
        Ok(Self::Bytes(BoundedStdinBytes(bytes.into())))
    }
}

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
    stdin: ProcessStdin,
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
            stdin: ProcessStdin::Closed,
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

    /// Select one already-admitted physical stdin value.
    pub fn with_stdin(mut self, stdin: ProcessStdin) -> Self {
        self.stdin = stdin;
        self
    }

    pub fn stdin(&self) -> &ProcessStdin {
        &self.stdin
    }
}
