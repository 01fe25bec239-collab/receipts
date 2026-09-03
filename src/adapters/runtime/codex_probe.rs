use std::{error::Error, fmt, str};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodexProbeCommand {
    pub program: &'static str,
    pub args: &'static [&'static str],
}

pub const CODEX_VERSION_PROBE: CodexProbeCommand = CodexProbeCommand {
    program: "codex",
    args: &["--version"],
};

pub const CODEX_EXEC_HELP_PROBE: CodexProbeCommand = CodexProbeCommand {
    program: "codex",
    args: &["exec", "--help"],
};

#[derive(Debug, Clone, Copy)]
pub struct CodexProbeObservation<'a> {
    pub stdout: &'a [u8],
    pub stderr: &'a [u8],
    pub exit_code: Option<i32>,
    pub capture_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexCapabilityEvidence {
    Supported,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexCapabilityProbeReport {
    pub version: String,
    pub json: CodexCapabilityEvidence,
    pub output_schema: CodexCapabilityEvidence,
    pub sandbox: CodexCapabilityEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexProbeKind {
    Version,
    ExecHelp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexProbeChannel {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexCapability {
    Json,
    OutputSchema,
    Sandbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexProbeError {
    MissingStatus(CodexProbeKind),
    NonSuccessStatus(CodexProbeKind, i32),
    IncompleteCapture(CodexProbeKind),
    MissingVersionEvidence,
    MissingHelpEvidence,
    InvalidEncoding(CodexProbeKind, CodexProbeChannel),
    InvalidHelpShape,
    AmbiguousCapabilityEvidence(CodexCapability),
}

impl fmt::Display for CodexProbeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid Codex capability probe: {self:?}")
    }
}

impl Error for CodexProbeError {}

pub fn parse_codex_probe(
    version: CodexProbeObservation<'_>,
    exec_help: CodexProbeObservation<'_>,
) -> Result<CodexCapabilityProbeReport, CodexProbeError> {
    let (version_stdout, version_stderr) = validate_observation(version, CodexProbeKind::Version)?;
    let version = joined_nonempty(version_stdout, version_stderr)
        .ok_or(CodexProbeError::MissingVersionEvidence)?;

    let (help_stdout, help_stderr) = validate_observation(exec_help, CodexProbeKind::ExecHelp)?;
    if help_stdout.trim().is_empty() && help_stderr.trim().is_empty() {
        return Err(CodexProbeError::MissingHelpEvidence);
    }

    let help_sources: Vec<_> = [help_stdout, help_stderr]
        .into_iter()
        .filter(|text| has_exec_help_shape(text))
        .collect();
    if help_sources.is_empty() {
        return Err(CodexProbeError::InvalidHelpShape);
    }

    Ok(CodexCapabilityProbeReport {
        version,
        json: capability_evidence(&help_sources, "--json", CodexCapability::Json)?,
        output_schema: capability_evidence(
            &help_sources,
            "--output-schema",
            CodexCapability::OutputSchema,
        )?,
        sandbox: capability_evidence(&help_sources, "--sandbox", CodexCapability::Sandbox)?,
    })
}

fn validate_observation(
    observation: CodexProbeObservation<'_>,
    kind: CodexProbeKind,
) -> Result<(&str, &str), CodexProbeError> {
    match observation.exit_code {
        None => return Err(CodexProbeError::MissingStatus(kind)),
        Some(code) if code != 0 => return Err(CodexProbeError::NonSuccessStatus(kind, code)),
        Some(_) => {}
    }
    if !observation.capture_complete {
        return Err(CodexProbeError::IncompleteCapture(kind));
    }

    let stdout = str::from_utf8(observation.stdout)
        .map_err(|_| CodexProbeError::InvalidEncoding(kind, CodexProbeChannel::Stdout))?;
    let stderr = str::from_utf8(observation.stderr)
        .map_err(|_| CodexProbeError::InvalidEncoding(kind, CodexProbeChannel::Stderr))?;
    Ok((stdout, stderr))
}

fn joined_nonempty(stdout: &str, stderr: &str) -> Option<String> {
    match (stdout.trim(), stderr.trim()) {
        ("", "") => None,
        (stdout, "") => Some(stdout.to_owned()),
        ("", stderr) => Some(stderr.to_owned()),
        (stdout, stderr) => Some(format!("{stdout}\n{stderr}")),
    }
}

fn has_exec_help_shape(text: &str) -> bool {
    let mut usage_seen = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(usage) = trimmed.strip_prefix("Usage:") {
            let mut words = usage.split_whitespace();
            usage_seen = words.next() == Some("codex") && words.next() == Some("exec");
        } else if usage_seen && line == trimmed && trimmed == "Options:" {
            return true;
        }
    }
    false
}

fn capability_evidence(
    help_sources: &[&str],
    target: &str,
    capability: CodexCapability,
) -> Result<CodexCapabilityEvidence, CodexProbeError> {
    let declarations = help_sources
        .iter()
        .flat_map(|source| option_declarations(source))
        .map(|line| declaration_flag_count(line, target))
        .sum();

    match declarations {
        0 => Ok(CodexCapabilityEvidence::Unknown),
        1 => Ok(CodexCapabilityEvidence::Supported),
        _ => Err(CodexProbeError::AmbiguousCapabilityEvidence(capability)),
    }
}

fn declaration_flag_count(line: &str, target: &str) -> usize {
    let tokens: Vec<_> = line
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter(|token| !token.is_empty())
        .collect();
    if tokens
        .iter()
        .any(|token| !token.starts_with('-') && !token.starts_with('<') && !token.starts_with('['))
    {
        return 0;
    }
    tokens.into_iter().filter(|token| *token == target).count()
}

fn option_declarations(text: &str) -> Vec<&str> {
    let mut usage_seen = false;
    let mut in_options = false;
    let mut section = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if !in_options {
            if let Some(usage) = trimmed.strip_prefix("Usage:") {
                let mut words = usage.split_whitespace();
                usage_seen = words.next() == Some("codex") && words.next() == Some("exec");
            } else if usage_seen && line == trimmed && trimmed == "Options:" {
                in_options = true;
            }
        } else if !trimmed.is_empty() && line == line.trim_start() {
            break;
        } else {
            section.push(line);
        }
    }

    let Some(declaration_indent) = section
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
    else {
        return Vec::new();
    };

    section
        .into_iter()
        .filter(|line| {
            line.len() - line.trim_start().len() == declaration_indent
                && line.trim_start().starts_with('-')
        })
        .map(str::trim)
        .collect()
}
