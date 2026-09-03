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
    let version = preferred_nonempty(version_stdout, version_stderr)
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

fn preferred_nonempty(stdout: &str, stderr: &str) -> Option<String> {
    [stdout.trim(), stderr.trim()]
        .into_iter()
        .find(|text| !text.is_empty())
        .map(str::to_owned)
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
    let Some(tokens) = declaration_tokens(line) else {
        return 0;
    };
    tokens.into_iter().filter(|token| *token == target).count()
}

fn declaration_tokens(line: &str) -> Option<Vec<&str>> {
    let tokens: Vec<_> = line
        .split(|character: char| character.is_whitespace() || character == ',')
        .filter(|token| !token.is_empty())
        .take_while(|token| token.starts_with('-') || is_placeholder(token))
        .collect();

    (!tokens.is_empty() && tokens[0].starts_with('-')).then_some(tokens)
}

fn is_placeholder(token: &str) -> bool {
    token.len() > 2
        && ((token.starts_with('<') && token.ends_with('>'))
            || (token.starts_with('[') && token.ends_with(']')))
}

fn option_declarations(text: &str) -> Vec<&str> {
    let mut usage_seen = false;
    let mut in_options = false;
    let mut declarations = Vec::new();
    let mut declaration_indent = None;
    let mut aligned_long_indent = None;
    let mut body_indent = None;
    let mut blank_line = false;

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
        } else if trimmed.is_empty() {
            blank_line = declaration_indent.is_some();
        } else {
            let indent = line.len() - line.trim_start().len();
            let tokens = declaration_tokens(trimmed);
            let starts_entry = tokens.is_some()
                && declaration_indent.is_none_or(|current_indent| {
                    indent <= current_indent
                        || (blank_line
                            && aligned_long_indent == Some(indent)
                            && body_indent.is_none_or(|body_indent| indent < body_indent))
                });

            if starts_entry {
                declarations.push(trimmed);
                declaration_indent = Some(indent);
                aligned_long_indent = tokens.and_then(|tokens| {
                    let short = tokens.first()?;
                    let long = tokens
                        .iter()
                        .skip(1)
                        .find(|token| token.starts_with("--"))?;
                    (!short.starts_with("--"))
                        .then(|| indent + trimmed.find(long).expect("token came from line"))
                });
                body_indent = None;
            } else if declaration_indent.is_some_and(|current_indent| indent > current_indent) {
                body_indent =
                    Some(body_indent.map_or(indent, |body_indent: usize| body_indent.min(indent)));
            }
            blank_line = false;
        }
    }

    declarations
}
