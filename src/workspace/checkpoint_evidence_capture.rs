//! Read-only local Git evidence for a non-temporal checkpoint.

use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::execution::STREAM_CAPTURE_LIMIT_BYTES;
use crate::{
    CommitSha, WorkspaceCheckpointCaptureCore, WorkspaceCheckpointCaptureCoreError,
    WorkspaceCheckpointExecutedCheckCore, WorkspaceCheckpointKind, WorkspaceCheckpointRef,
    WorkspaceError, git,
};

/// Caller-authorized context and inert evidence. `directory` must be in the
/// intended existing worktree; it may be a subdirectory. No handle SHA is used.
#[derive(Debug)]
pub struct WorkspaceCheckpointEvidenceCaptureRequest<'a> {
    pub directory: &'a Path,
    pub checkpoint_id: String,
    pub workspace_id: String,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub kind: WorkspaceCheckpointKind,
    pub base_sha: Option<&'a str>,
    pub dirty_diff_ref: Option<WorkspaceCheckpointRef>,
    pub executed_checks: Vec<WorkspaceCheckpointExecutedCheckCore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCheckpointGitObservation {
    WorktreeRoot,
    Head,
    StagedPaths,
    UnstagedPaths,
    UntrackedPaths,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceCheckpointGitStream {
    Stdout,
    Stderr,
}

/// Capture fails as a whole. Diagnostic bytes are raw and separately bounded;
/// oversized stdout is never attached to an error or offered as path evidence.
#[derive(Debug)]
pub enum WorkspaceCheckpointEvidenceCaptureError {
    GitPreparation {
        observation: WorkspaceCheckpointGitObservation,
        source: WorkspaceError,
    },
    GitIo {
        observation: WorkspaceCheckpointGitObservation,
        stage: &'static str,
        source: io::Error,
    },
    GitCommandFailed {
        observation: WorkspaceCheckpointGitObservation,
        status: Option<i32>,
        stderr: Vec<u8>,
        stderr_total_bytes: u64,
        stderr_truncated: bool,
    },
    InvalidHeadSha,
    InvalidBaseSha,
    GitEvidenceTooLarge {
        observation: WorkspaceCheckpointGitObservation,
        total_bytes: u64,
        limit_bytes: u64,
    },
    InvalidUtf8 {
        observation: WorkspaceCheckpointGitObservation,
    },
    MalformedEvidence {
        observation: WorkspaceCheckpointGitObservation,
        reason: &'static str,
    },
    StreamRead {
        observation: WorkspaceCheckpointGitObservation,
        stream: WorkspaceCheckpointGitStream,
        source: io::Error,
    },
    StreamCountOverflow {
        observation: WorkspaceCheckpointGitObservation,
        stream: WorkspaceCheckpointGitStream,
        counted: u64,
        chunk: usize,
    },
    StreamAllocation {
        observation: WorkspaceCheckpointGitObservation,
        source: std::collections::TryReserveError,
    },
    StreamReaderPanicked {
        observation: WorkspaceCheckpointGitObservation,
    },
    Core(WorkspaceCheckpointCaptureCoreError),
}

impl std::fmt::Display for WorkspaceCheckpointEvidenceCaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Keep raw stderr out of ordinary log messages.
        if let Self::GitCommandFailed {
            observation,
            status,
            stderr_total_bytes,
            stderr_truncated,
            ..
        } = self
        {
            write!(
                f,
                "checkpoint Git {observation:?} failed ({status:?}); stderr bytes: {stderr_total_bytes}, truncated: {stderr_truncated}"
            )
        } else {
            write!(f, "checkpoint evidence capture error: {self:?}")
        }
    }
}

impl std::error::Error for WorkspaceCheckpointEvidenceCaptureError {}

use WorkspaceCheckpointEvidenceCaptureError as E;
use WorkspaceCheckpointGitObservation as Observation;
use WorkspaceCheckpointGitStream as Stream;

/// Observes a fixed set of local Git queries, then constructs the existing core.
/// References and completed checks are moved unchanged, never resolved or run.
/// Recovery remains unselected. The caller must exclude concurrent writers if
/// it needs a consistent snapshot across these observations; this is not a Git
/// transaction. Path records must be strict UTF-8 on every supported host.
pub fn capture_workspace_checkpoint_evidence(
    request: WorkspaceCheckpointEvidenceCaptureRequest<'_>,
) -> Result<WorkspaceCheckpointCaptureCore, E> {
    let base_sha = request
        .base_sha
        .map(|sha| CommitSha::parse(sha).map_err(|_| E::InvalidBaseSha))
        .transpose()?;
    let root_output = observe(
        request.directory,
        Observation::WorktreeRoot,
        &["rev-parse", "--show-toplevel"],
    )?;
    let root = parse_root(root_output)?;
    let head = observe(
        &root,
        Observation::Head,
        &["rev-parse", "--verify", "HEAD^{commit}"],
    )?;
    let head_sha = parse_head(head)?;
    let mut modified_files = Vec::new();
    for observation in [Observation::StagedPaths, Observation::UnstagedPaths] {
        let mut args = vec![
            "diff",
            "--name-only",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--no-relative",
            "--ignore-submodules=none",
        ];
        if observation == Observation::StagedPaths {
            args.extend(["--cached", head_sha.as_str()]);
        }
        args.push("--");
        modified_files.extend(parse_paths(
            observe(&root, observation, &args)?,
            observation,
        )?);
    }
    let mut untracked_files = parse_paths(
        observe(
            &root,
            Observation::UntrackedPaths,
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "--full-name",
                "-z",
                "--",
            ],
        )?,
        Observation::UntrackedPaths,
    )?;
    modified_files.sort();
    modified_files.dedup();
    untracked_files.sort();
    untracked_files.dedup();
    WorkspaceCheckpointCaptureCore::new(
        request.checkpoint_id,
        request.workspace_id,
        request.task_id,
        request.attempt_id,
        request.kind,
        head_sha,
        base_sha,
        request.dirty_diff_ref,
        modified_files,
        untracked_files,
        request.executed_checks,
        None,
        None,
    )
    .map_err(E::Core)
}

// Execution's retention module is private and its runner cannot take a prepared
// Git Command. This Git-only prefix retention drains through EOF and shares the
// public Execution limit; truncated bytes never enter the path parser.
#[derive(Debug)]
struct RawStream {
    bytes: Vec<u8>,
    total_bytes: u64,
    truncated: bool,
}

impl RawStream {
    fn new(observation: Observation) -> Result<Self, E> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(STREAM_CAPTURE_LIMIT_BYTES as usize)
            .map_err(|source| E::StreamAllocation {
                observation,
                source,
            })?;
        Ok(Self {
            bytes,
            total_bytes: 0,
            truncated: false,
        })
    }

    fn push(&mut self, chunk: &[u8], observation: Observation, stream: Stream) -> Result<(), E> {
        self.total_bytes =
            self.total_bytes
                .checked_add(chunk.len() as u64)
                .ok_or(E::StreamCountOverflow {
                    observation,
                    stream,
                    counted: self.total_bytes,
                    chunk: chunk.len(),
                })?;
        let take = chunk
            .len()
            .min(STREAM_CAPTURE_LIMIT_BYTES as usize - self.bytes.len());
        self.bytes.extend_from_slice(&chunk[..take]);
        self.truncated = self.total_bytes > STREAM_CAPTURE_LIMIT_BYTES;
        Ok(())
    }

    fn require_complete(self, observation: Observation) -> Result<Vec<u8>, E> {
        if self.truncated || self.total_bytes > STREAM_CAPTURE_LIMIT_BYTES {
            return Err(E::GitEvidenceTooLarge {
                observation,
                total_bytes: self.total_bytes,
                limit_bytes: STREAM_CAPTURE_LIMIT_BYTES,
            });
        }
        if self.total_bytes != self.bytes.len() as u64 {
            return Err(E::MalformedEvidence {
                observation,
                reason: "incomplete stream",
            });
        }
        Ok(self.bytes)
    }
}

fn drain(
    mut reader: impl Read,
    mut retained: RawStream,
    observation: Observation,
    stream: Stream,
) -> Result<RawStream, E> {
    let mut buffer = [0; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return Ok(retained),
            Ok(count) => retained.push(&buffer[..count], observation, stream)?,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(source) => {
                return Err(E::StreamRead {
                    observation,
                    stream,
                    source,
                });
            }
        }
    }
}

fn observe(root: &Path, observation: Observation, args: &[&str]) -> Result<RawStream, E> {
    let args: Vec<&OsStr> = [
        "--no-pager",
        "--no-replace-objects",
        "--no-optional-locks",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.untrackedCache=false",
        "-c",
        "protocol.allow=never",
    ]
    .iter()
    .chain(args)
    .map(OsStr::new)
    .collect();
    let mut command =
        git::prepared_command(root, "capture checkpoint evidence", &args).map_err(|source| {
            E::GitPreparation {
                observation,
                source,
            }
        })?;
    let stdout_retention = RawStream::new(observation)?;
    let stderr_retention = RawStream::new(observation)?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| E::GitIo {
            observation,
            stage: "spawn",
            source,
        })?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let stderr_reader = match std::thread::Builder::new()
        .spawn(move || drain(stderr, stderr_retention, observation, Stream::Stderr))
    {
        Ok(reader) => reader,
        Err(source) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(E::GitIo {
                observation,
                stage: "start stderr reader",
                source,
            });
        }
    };
    // Both pipes are drained concurrently. A failed drain drops its pipe; still
    // join the other reader and reap Git before propagating any failure.
    let stdout = drain(stdout, stdout_retention, observation, Stream::Stdout);
    let stderr = stderr_reader.join();
    let status = child.wait();
    let stdout = stdout?;
    let stderr = stderr.map_err(|_| E::StreamReaderPanicked { observation })??;
    let status = status.map_err(|source| E::GitIo {
        observation,
        stage: "wait",
        source,
    })?;
    if !status.success() {
        return Err(E::GitCommandFailed {
            observation,
            status: status.code(),
            stderr: stderr.bytes,
            stderr_total_bytes: stderr.total_bytes,
            stderr_truncated: stderr.truncated,
        });
    }
    Ok(stdout)
}

fn parse_head(raw: RawStream) -> Result<CommitSha, E> {
    let bytes = raw.require_complete(Observation::Head)?;
    let sha = bytes.strip_suffix(b"\n").ok_or(E::InvalidHeadSha)?;
    let sha = std::str::from_utf8(sha).map_err(|_| E::InvalidHeadSha)?;
    CommitSha::parse(sha).map_err(|_| E::InvalidHeadSha)
}

fn parse_root(raw: RawStream) -> Result<PathBuf, E> {
    let observation = Observation::WorktreeRoot;
    let bytes = raw.require_complete(observation)?;
    // rev-parse emits one raw absolute root followed by one LF. Remove only
    // that terminator: a worktree root itself can contain whitespace or LF.
    let bytes = bytes.strip_suffix(b"\n").ok_or(E::MalformedEvidence {
        observation,
        reason: "missing root terminator",
    })?;
    let root = std::str::from_utf8(bytes).map_err(|_| E::InvalidUtf8 { observation })?;
    if root.contains('\0') || !Path::new(root).is_absolute() {
        return Err(E::MalformedEvidence {
            observation,
            reason: "invalid absolute worktree root",
        });
    }
    Ok(PathBuf::from(root))
}

fn parse_paths(raw: RawStream, observation: Observation) -> Result<Vec<String>, E> {
    // This gate precedes ALL decoding, framing, filtering and deduplication.
    let bytes = raw.require_complete(observation)?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let records = bytes.strip_suffix(&[0]).ok_or(E::MalformedEvidence {
        observation,
        reason: "missing terminal NUL",
    })?;
    let mut paths = Vec::new();
    for record in records.split(|byte| *byte == 0) {
        if record.is_empty() {
            return Err(E::MalformedEvidence {
                observation,
                reason: "empty NUL record",
            });
        }
        let path = std::str::from_utf8(record).map_err(|_| E::InvalidUtf8 { observation })?;
        paths.push(path.to_owned());
    }
    Ok(paths)
}

#[cfg(test)]
#[path = "checkpoint_evidence_capture_tests.rs"]
mod tests;
