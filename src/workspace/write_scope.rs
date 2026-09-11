//! Read-only post-hoc write-scope verification under A0 WRITE-PATH CONTRACT V1.
//! Git path identities stay strict UTF-8 strings; no filesystem normalization
//! or alternative glob dialect participates in matching.

use std::path::Path;

use crate::checkpoint_evidence_capture::{RawStream, observe};
use crate::{
    CommitSha, WorkspaceCheckpointEvidenceCaptureError, WorkspaceCheckpointGitObservation,
    WorkspaceError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteScopeVerificationStatus {
    Pass,
    Fail,
}

impl WriteScopeVerificationStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Fail => "FAIL",
        }
    }
}

/// Exact forbidden-match evidence, ordered by path then pattern.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ForbiddenWriteMatch {
    path: String,
    pattern: String,
}

impl ForbiddenWriteMatch {
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn pattern(&self) -> &str {
        &self.pattern
    }
}

/// Immutable mechanical evidence; only the verifier can construct it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteScopeVerification {
    verification: WriteScopeVerificationStatus,
    baseline_sha: CommitSha,
    candidate_sha: CommitSha,
    changed_paths: Vec<String>,
    unauthorized_paths: Vec<String>,
    forbidden_matches: Vec<ForbiddenWriteMatch>,
}

impl WriteScopeVerification {
    pub fn verification(&self) -> WriteScopeVerificationStatus {
        self.verification
    }
    pub fn baseline_sha(&self) -> &CommitSha {
        &self.baseline_sha
    }
    pub fn candidate_sha(&self) -> &CommitSha {
        &self.candidate_sha
    }
    pub fn changed_paths(&self) -> &[String] {
        &self.changed_paths
    }
    pub fn unauthorized_paths(&self) -> &[String] {
        &self.unauthorized_paths
    }
    pub fn forbidden_matches(&self) -> &[ForbiddenWriteMatch] {
        &self.forbidden_matches
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteScopePatternSet {
    Allowed,
    Forbidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteScopeGitOperation {
    BaselineCommit,
    CandidateCommit,
    ChangedPaths,
}

/// Input/collector failures are distinct from a successful evaluation of FAIL.
/// Path and stderr evidence are retained without lossy decoding.
#[derive(Debug)]
pub enum WriteScopeVerificationError {
    Evidence {
        operation: WriteScopeGitOperation,
        source: WorkspaceCheckpointEvidenceCaptureError,
    },
    InvalidBaselineSha,
    InvalidCandidateSha,
    CommitUnavailable {
        operation: WriteScopeGitOperation,
        sha: CommitSha,
        status: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    GitPreparation {
        operation: WriteScopeGitOperation,
        source: WorkspaceError,
    },
    GitExecution {
        operation: WriteScopeGitOperation,
        source: std::io::Error,
    },
    GitCommandFailed {
        operation: WriteScopeGitOperation,
        status: Option<i32>,
        stderr: Vec<u8>,
    },
    MalformedGitOutput {
        reason: &'static str,
    },
    InvalidUtf8Candidate {
        bytes: Vec<u8>,
    },
    MalformedCandidatePath {
        path: String,
        reason: &'static str,
    },
    InvalidPattern {
        set: WriteScopePatternSet,
        index: usize,
        pattern: String,
        reason: &'static str,
    },
}

impl std::fmt::Display for WriteScopeVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Evidence { operation, source } => {
                write!(f, "write-scope {operation:?}: {source}")
            }
            Self::CommitUnavailable { operation, .. } => {
                write!(f, "write-scope {operation:?}: commit unavailable")
            }
            Self::GitCommandFailed {
                operation, status, ..
            } => write!(f, "write-scope {operation:?}: Git failed ({status:?})"),
            _ => write!(f, "write-scope verification error: {self:?}"),
        }
    }
}
impl std::error::Error for WriteScopeVerificationError {}

/// Verifies the exact commit-to-commit changed path set. No ancestry is required.
/// All policy entries are validated before Git collection or any scope decision.
/// Renames are deletion + addition, so both source and destination are checked.
pub fn verify_write_scope(
    repository_root: &Path,
    baseline_sha: &str,
    candidate_sha: &str,
    allowed_write_paths: &[String],
    forbidden_write_paths: &[String],
) -> Result<WriteScopeVerification, WriteScopeVerificationError> {
    use WriteScopeVerificationError as E;
    let baseline_sha = CommitSha::parse(baseline_sha).map_err(|_| E::InvalidBaselineSha)?;
    let candidate_sha = CommitSha::parse(candidate_sha).map_err(|_| E::InvalidCandidateSha)?;
    let allowed = parse_policy(allowed_write_paths, WriteScopePatternSet::Allowed)?;
    let forbidden = parse_policy(forbidden_write_paths, WriteScopePatternSet::Forbidden)?;
    verify_commit(
        repository_root,
        &baseline_sha,
        WriteScopeGitOperation::BaselineCommit,
    )?;
    verify_commit(
        repository_root,
        &candidate_sha,
        WriteScopeGitOperation::CandidateCommit,
    )?;
    let operation = WriteScopeGitOperation::ChangedPaths;
    let output = run_git(
        repository_root,
        operation,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--no-relative",
            "--ignore-submodules=none",
            baseline_sha.as_str(),
            candidate_sha.as_str(),
            "--",
        ],
    )?;
    let changed_paths = parse_paths(output)?;
    let mut unauthorized_paths = Vec::new();
    let mut forbidden_matches = Vec::new();
    for path in &changed_paths {
        for (pattern, parsed) in forbidden_write_paths.iter().zip(&forbidden) {
            if parsed.matches(path) {
                forbidden_matches.push(ForbiddenWriteMatch {
                    path: path.clone(),
                    pattern: pattern.clone(),
                });
            }
        }
        if !allowed.iter().any(|pattern| pattern.matches(path)) {
            unauthorized_paths.push(path.clone());
        }
    }
    unauthorized_paths.sort();
    forbidden_matches.sort();
    forbidden_matches.dedup();
    let verification = if unauthorized_paths.is_empty() && forbidden_matches.is_empty() {
        WriteScopeVerificationStatus::Pass
    } else {
        WriteScopeVerificationStatus::Fail
    };
    Ok(WriteScopeVerification {
        verification,
        baseline_sha,
        candidate_sha,
        changed_paths,
        unauthorized_paths,
        forbidden_matches,
    })
}

fn run_git(
    root: &Path,
    operation: WriteScopeGitOperation,
    args: &[&str],
) -> Result<RawStream, WriteScopeVerificationError> {
    use WorkspaceCheckpointEvidenceCaptureError as C;
    use WriteScopeVerificationError as E;
    // Reuse the private bounded collector; the outer operation is authoritative
    // for write-scope diagnostics, without adding checkpoint vocabulary.
    let observation = WorkspaceCheckpointGitObservation::StagedPaths;
    observe(root, observation, args).map_err(|source| match source {
        C::GitPreparation { source, .. } => E::GitPreparation { operation, source },
        C::GitCommandFailed { status, stderr, .. } => E::GitCommandFailed {
            operation,
            status,
            stderr,
        },
        source => E::Evidence { operation, source },
    })
}

fn verify_commit(
    root: &Path,
    sha: &CommitSha,
    operation: WriteScopeGitOperation,
) -> Result<(), WriteScopeVerificationError> {
    // Exact object type, never tag peeling.
    let output = run_git(root, operation, &["cat-file", "-t", sha.as_str()])
        .and_then(|raw| complete(raw, operation));
    let (status, stdout, stderr) = match output {
        Ok(stdout) if stdout == b"commit\n" => return Ok(()),
        Ok(stdout) => (Some(0), stdout, vec![]),
        Err(WriteScopeVerificationError::GitCommandFailed { status, stderr, .. }) => {
            (status, vec![], stderr)
        }
        Err(error) => return Err(error),
    };
    Err(WriteScopeVerificationError::CommitUnavailable {
        operation,
        sha: sha.clone(),
        status,
        stdout,
        stderr,
    })
}

fn validate_structure(value: &str) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("empty path");
    }
    if value.contains('\\') {
        return Err("backslash is forbidden");
    }
    for component in value.split('/') {
        if component.is_empty() {
            return Err("empty component");
        }
        if component == "." || component == ".." {
            return Err("dot component");
        }
    }
    Ok(())
}

fn complete(
    raw: RawStream,
    operation: WriteScopeGitOperation,
) -> Result<Vec<u8>, WriteScopeVerificationError> {
    raw.require_complete(WorkspaceCheckpointGitObservation::StagedPaths)
        .map_err(|source| WriteScopeVerificationError::Evidence { operation, source })
}

fn parse_paths(raw: RawStream) -> Result<Vec<String>, WriteScopeVerificationError> {
    use WriteScopeVerificationError as E;
    // Complete raw query admission precedes decoding, framing, sorting and matching.
    let bytes = complete(raw, WriteScopeGitOperation::ChangedPaths)?;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    if bytes.last() != Some(&0) {
        return Err(E::MalformedGitOutput {
            reason: "missing terminal NUL",
        });
    }
    let mut paths = Vec::new();
    for record in bytes[..bytes.len() - 1].split(|byte| *byte == 0) {
        if record.is_empty() {
            return Err(E::MalformedGitOutput {
                reason: "empty NUL record",
            });
        }
        let path = std::str::from_utf8(record).map_err(|_| E::InvalidUtf8Candidate {
            bytes: record.to_vec(),
        })?;
        validate_structure(path).map_err(|reason| E::MalformedCandidatePath {
            path: path.to_owned(),
            reason,
        })?;
        paths.push(path.to_owned());
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

struct Pattern {
    components: Vec<Component>,
}
enum Component {
    GlobStar,
    Normal(Vec<char>),
}

fn parse_policy(
    patterns: &[String],
    set: WriteScopePatternSet,
) -> Result<Vec<Pattern>, WriteScopeVerificationError> {
    patterns
        .iter()
        .enumerate()
        .map(|(index, pattern)| {
            Pattern::parse(pattern).map_err(|reason| WriteScopeVerificationError::InvalidPattern {
                set,
                index,
                pattern: pattern.clone(),
                reason,
            })
        })
        .collect()
}

impl Pattern {
    fn parse(pattern: &str) -> Result<Self, &'static str> {
        validate_structure(pattern)?;
        if pattern.starts_with('!') {
            return Err("leading negation is unsupported");
        }
        if pattern.contains(['?', '[', ']', '{', '}']) {
            return Err("unsupported pattern syntax");
        }
        let mut components = Vec::new();
        for component in pattern.split('/') {
            components.push(if component == "**" {
                Component::GlobStar
            } else {
                if component.contains("**") {
                    return Err("globstar must occupy a complete component");
                }
                Component::Normal(component.chars().collect())
            });
        }
        Ok(Self { components })
    }

    fn matches(&self, path: &str) -> bool {
        let parts: Vec<&str> = path.split('/').collect();
        // Each row represents one pattern prefix against every path prefix.
        // O(pattern components * path components), plus normal-component DP.
        let mut row = vec![false; parts.len() + 1];
        row[0] = true;
        for component in &self.components {
            let mut next = vec![false; row.len()];
            match component {
                Component::GlobStar => {
                    next[0] = row[0];
                    for j in 1..row.len() {
                        next[j] = row[j] || next[j - 1];
                    }
                }
                Component::Normal(tokens) => {
                    for j in 1..row.len() {
                        next[j] = row[j - 1] && match_component(tokens, parts[j - 1]);
                    }
                }
            }
            row = next;
        }
        row[parts.len()]
    }
}

fn match_component(tokens: &[char], value: &str) -> bool {
    let scalars: Vec<char> = value.chars().collect();
    // O(pattern scalars * candidate scalars) time, O(candidate scalars) space.
    let mut row = vec![false; scalars.len() + 1];
    row[0] = true;
    for token in tokens {
        let mut next = vec![false; row.len()];
        if *token == '*' {
            next[0] = row[0];
        }
        for j in 1..row.len() {
            next[j] = if *token == '*' {
                row[j] || next[j - 1]
            } else {
                row[j - 1] && *token == scalars[j - 1]
            };
        }
        row = next;
    }
    row[scalars.len()]
}

#[cfg(test)]
#[path = "write_scope_tests.rs"]
mod tests;
