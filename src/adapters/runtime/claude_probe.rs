//! Observed Claude flag surface only; no value-domain, policy or admission claims.
use std::{error::Error, fmt, str};

use receipts_workspace_execution::execution::{ProcessTermination, STREAM_CAPTURE_LIMIT_BYTES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeProbeKind {
    Version,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeProbeChannel {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeCapabilityEvidence {
    Supported,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeCapability {
    Print,
    InputFormat,
    OutputFormat,
    NoSessionPersistence,
    PermissionMode,
    Verbose,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeCapabilityProbeReport {
    pub version: String,
    pub print: ClaudeCapabilityEvidence,
    pub input_format: ClaudeCapabilityEvidence,
    pub output_format: ClaudeCapabilityEvidence,
    pub no_session_persistence: ClaudeCapabilityEvidence,
    pub permission_mode: ClaudeCapabilityEvidence,
    pub verbose: ClaudeCapabilityEvidence,
}

/// Caller-supplied observations must represent complete bounded captures.
#[derive(Clone, Copy)]
pub struct ClaudeProbeObservation<'a> {
    pub termination: ProcessTermination,
    pub exit_code: Option<i32>,
    pub capture_complete: bool,
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeProbeError {
    IncompleteProcess(ClaudeProbeKind),
    MissingStatus(ClaudeProbeKind),
    NonSuccessStatus(ClaudeProbeKind, i32),
    IncompleteCapture(ClaudeProbeKind),
    CaptureLimitExceeded(ClaudeProbeKind, ClaudeProbeChannel),
    InvalidEncoding(ClaudeProbeKind, ClaudeProbeChannel),
    MissingVersionEvidence,
    MissingHelpEvidence,
    InvalidHelpShape,
    AmbiguousCapabilityEvidence(ClaudeCapability),
}

impl fmt::Display for ClaudeProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid Claude capability probe: {self:?}")
    }
}
impl Error for ClaudeProbeError {}

pub fn parse_claude_probe(
    version: ClaudeProbeObservation<'_>,
    help: ClaudeProbeObservation<'_>,
) -> Result<ClaudeCapabilityProbeReport, ClaudeProbeError> {
    let version = validate(version, ClaudeProbeKind::Version)?;
    // Match the existing probe convention: prefer stdout, never concatenate channels.
    let version = version
        .into_iter()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .ok_or(ClaudeProbeError::MissingVersionEvidence)?
        .to_owned();
    let help = validate(help, ClaudeProbeKind::Help)?;
    if help.iter().all(|s| s.trim().is_empty()) {
        return Err(ClaudeProbeError::MissingHelpEvidence);
    }
    let sources: Vec<_> = help.into_iter().filter_map(option_lines).collect();
    if sources.is_empty() {
        return Err(ClaudeProbeError::InvalidHelpShape);
    }
    let evidence = |flag, capability| {
        let count: usize = sources
            .iter()
            .flatten()
            .map(|line| {
                // The declaration ends at the first description column (2+ spaces).
                let declaration = line.split("  ").next().unwrap_or("");
                declaration
                    .split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|token| !token.is_empty())
                    .take_while(|token| {
                        token.starts_with('-')
                            || (token.starts_with('<') && token.ends_with('>'))
                            || (token.starts_with('[') && token.ends_with(']'))
                    })
                    .filter(|token| *token == flag)
                    .count()
            })
            .sum();
        match count {
            0 => Ok(ClaudeCapabilityEvidence::Unknown),
            1 => Ok(ClaudeCapabilityEvidence::Supported),
            _ => Err(ClaudeProbeError::AmbiguousCapabilityEvidence(capability)),
        }
    };
    Ok(ClaudeCapabilityProbeReport {
        version,
        print: evidence("--print", ClaudeCapability::Print)?,
        input_format: evidence("--input-format", ClaudeCapability::InputFormat)?,
        output_format: evidence("--output-format", ClaudeCapability::OutputFormat)?,
        no_session_persistence: evidence(
            "--no-session-persistence",
            ClaudeCapability::NoSessionPersistence,
        )?,
        permission_mode: evidence("--permission-mode", ClaudeCapability::PermissionMode)?,
        verbose: evidence("--verbose", ClaudeCapability::Verbose)?,
    })
}

fn validate(
    observation: ClaudeProbeObservation<'_>,
    kind: ClaudeProbeKind,
) -> Result<[&str; 2], ClaudeProbeError> {
    if observation.termination != ProcessTermination::Completed {
        return Err(ClaudeProbeError::IncompleteProcess(kind));
    }
    match observation.exit_code {
        None => return Err(ClaudeProbeError::MissingStatus(kind)),
        Some(0) => {}
        Some(code) => return Err(ClaudeProbeError::NonSuccessStatus(kind, code)),
    }
    if !observation.capture_complete {
        return Err(ClaudeProbeError::IncompleteCapture(kind));
    }
    let decode = |bytes: &[u8], channel| {
        if bytes.len() as u64 > STREAM_CAPTURE_LIMIT_BYTES {
            Err(ClaudeProbeError::CaptureLimitExceeded(kind, channel))
        } else {
            Ok(())
        }
    };
    decode(observation.stdout, ClaudeProbeChannel::Stdout)?;
    decode(observation.stderr, ClaudeProbeChannel::Stderr)?;
    Ok([
        str::from_utf8(observation.stdout)
            .map_err(|_| ClaudeProbeError::InvalidEncoding(kind, ClaudeProbeChannel::Stdout))?,
        str::from_utf8(observation.stderr)
            .map_err(|_| ClaudeProbeError::InvalidEncoding(kind, ClaudeProbeChannel::Stderr))?,
    ])
}

fn option_lines(text: &str) -> Option<Vec<&str>> {
    let mut lines = text.lines();
    lines.find(|line| {
        line.strip_prefix("Usage:")
            .is_some_and(|usage| usage.split_whitespace().next() == Some("claude"))
    })?;
    lines.find(|line| *line == "Options:")?;
    // Claude's observed declarations occupy exactly the two-space option column.
    // Wrapped prose is further indented; other sections end the option table.
    Some(
        lines
            .take_while(|line| line.is_empty() || line.starts_with(char::is_whitespace))
            .filter_map(|line| line.strip_prefix("  ").filter(|line| line.starts_with('-')))
            .collect(),
    )
}
