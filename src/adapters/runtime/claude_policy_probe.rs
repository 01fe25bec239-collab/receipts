//! Optional policy controls require fresh, complete evidence from the configured CLI.
use crate::*;
use receipts_workspace_execution::{
    WorkspaceHandle,
    execution::{ChildEnvPolicy, ProcessTimeoutPolicy},
};
use std::path::Path;

#[cfg(any(unix, test))]
const FLAGS: [&str; 3] = ["--max-turns", "--resume", "--json-schema"];
#[cfg(unix)]
const INVALID_TURN_OPTION: &str = "--max-turns=receipts-invalid";
#[cfg(unix)]
const INVALID_TURN_ERROR: &[u8] =
    b"error: option '--max-turns <turns>' argument 'receipts-invalid' is invalid. must be a number\n";

#[cfg(unix)]
pub(crate) fn probe(
    exe: &Path,
    workspace: &WorkspaceHandle,
    timeout: &ProcessTimeoutPolicy,
    env: Option<&ChildEnvPolicy>,
) -> RuntimeCapabilities {
    use receipts_workspace_execution::execution::{
        ProcessRunRequest, ProcessTermination, run_with_timeout_and_capture,
    };
    use std::{
        fs,
        os::unix::fs::MetadataExt,
        time::{Duration, Instant},
    };

    if !exe.is_absolute() {
        return RuntimeCapabilities::Unknown;
    }
    let identity = || {
        let path = fs::canonicalize(exe).ok()?;
        let m = fs::metadata(&path).ok()?;
        m.is_file()
            .then_some((path, m.dev(), m.ino(), m.len(), m.ctime(), m.ctime_nsec()))
    };
    let start = Instant::now();
    let Some(original) = identity() else {
        return RuntimeCapabilities::Unknown;
    };
    let run = |args: &[&str], budget: &ProcessTimeoutPolicy| {
        if identity().as_ref() != Some(&original) {
            return None;
        }
        let cwd = workspace.worktree_path();
        let request = ProcessRunRequest::new(exe, args.iter().copied(), cwd, cwd).ok()?;
        let request = match env {
            Some(env) => request.with_env_policy(env.clone()),
            None => request,
        };
        let capture = run_with_timeout_and_capture(&request, budget).ok()?;
        (capture.outcome().termination() == ProcessTermination::Completed
            && !capture.stdout().truncated()
            && !capture.stderr().truncated()
            && identity().as_ref() == Some(&original))
        .then_some(capture)
    };
    let Some(help) = run(&["--help"], timeout).filter(|c| c.outcome().success()) else {
        return RuntimeCapabilities::Unknown;
    };
    let stdout = [help.stdout().head(), help.stdout().tail()].concat();
    let stderr = [help.stderr().head(), help.stderr().tail()].concat();
    let entries = policy_entries(&stdout, &stderr, || {
        let remaining = timeout.run_timeout().saturating_sub(start.elapsed());
        let Ok(budget) = ProcessTimeoutPolicy::new(
            remaining.min(Duration::from_secs(5)),
            timeout.termination_grace(),
        ) else {
            return false;
        };
        // Help and a rejected numeric value both terminate before the print action.
        // The value is attached and stdin stays closed, so there is no prompt operand.
        run(
            &["--bare", "--print", "--help", INVALID_TURN_OPTION],
            &budget,
        )
        .is_some_and(|c| {
            c.outcome().exit_code() == Some(1)
                && c.stdout().total_bytes() == 0
                && c.stderr().head() == INVALID_TURN_ERROR
                && c.stderr().tail().is_empty()
        })
    });
    if identity().as_ref() != Some(&original) {
        return RuntimeCapabilities::Unknown;
    }
    RuntimeCapabilities::Extension(
        ExtensionCapabilityReport::new("claude-policy", entries)
            .expect("fixed distinct nonempty names"),
    )
}

#[cfg(not(unix))]
pub(crate) fn probe(
    _: &Path,
    _: &WorkspaceHandle,
    _: &ProcessTimeoutPolicy,
    _: Option<&ChildEnvPolicy>,
) -> RuntimeCapabilities {
    RuntimeCapabilities::Unknown
}

#[cfg(any(unix, test))]
fn policy_entries(
    stdout: &[u8],
    stderr: &[u8],
    hidden_turns: impl FnOnce() -> bool,
) -> [(&'static str, CapabilityStatus); 3] {
    let unknown = FLAGS.map(|flag| (flag, CapabilityStatus::Unknown));
    let (Ok(stdout), Ok(stderr)) = (std::str::from_utf8(stdout), std::str::from_utf8(stderr))
    else {
        return unknown;
    };
    let help = match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (false, true) => stdout,
        (true, false) => stderr,
        _ => return unknown,
    };
    let Some(rows) = option_rows(help) else {
        return unknown;
    };
    for (name, prefix) in [
        ("--max-turns", "<"),
        ("--json-schema", "<"),
        ("--resume", "<["),
    ] {
        if rows.get(name).is_some_and(|value| {
            !value.starts_with(|c| prefix.contains(c)) || value.contains("...")
        }) {
            return unknown;
        }
    }
    if ["--bare", "--print"]
        .iter()
        .any(|flag| rows.get(flag).is_some_and(|value| !value.is_empty()))
    {
        return unknown;
    }
    let mut entries = FLAGS.map(|flag| (flag, status(rows.contains_key(flag))));
    // Only absence can use the hidden-option check. Ambiguous declarations stay Unknown.
    if !rows.contains_key("--max-turns")
        && rows.contains_key("--bare")
        && rows.contains_key("--print")
        && hidden_turns()
    {
        entries[0].1 = CapabilityStatus::Supported;
    }
    entries
}

#[cfg(any(unix, test))]
fn status(supported: bool) -> CapabilityStatus {
    if supported {
        CapabilityStatus::Supported
    } else {
        CapabilityStatus::Unknown
    }
}

#[cfg(any(unix, test))]
fn option_rows(text: &str) -> Option<std::collections::BTreeMap<&str, &str>> {
    let mut lines = text.lines();
    if !matches!(
        lines.find(|line| !line.trim().is_empty())?,
        "Usage: claude"
            | "Usage: claude [options]"
            | "Usage: claude [options] [prompt]"
            | "Usage: claude [options] [command] [prompt]"
    ) {
        return None;
    }
    // Check section order across the whole page, including after the option table.
    let mut headings = lines
        .clone()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("Usage:")
                || matches!(trimmed, "Arguments:" | "Options:" | "Commands:")
                || (!line.starts_with(' ') && trimmed.ends_with(':'))
        })
        .peekable();
    if headings.peek() == Some(&"Arguments:") {
        headings.next();
    }
    if headings.next() != Some("Options:")
        || !matches!(
            (headings.next(), headings.next()),
            (None | Some("Commands:"), None)
        )
    {
        return None;
    }

    lines.find(|line| *line == "Options:")?;
    let mut rows = std::collections::BTreeMap::new();
    // The two-space declaration column excludes wrapped option descriptions.
    for line in lines.take_while(|line| *line != "Commands:") {
        if line.trim().is_empty() || line.starts_with("   ") {
            continue;
        }
        let row = line.strip_prefix("  ")?;
        let declaration = row.split_once("  ").map_or(row, |(before, _)| before);
        let (names, value) = match declaration.rsplit_once(' ') {
            Some((names, value)) if value.starts_with(['<', '[']) => {
                let metavar = if let Some(value) = value.strip_prefix('<') {
                    value.strip_suffix('>')?
                } else {
                    value.strip_prefix('[')?.strip_suffix(']')?
                };
                if !metavar.split('|').all(|part| {
                    let word = part.strip_suffix("...").unwrap_or(part);
                    word.starts_with(|c: char| c.is_ascii_alphanumeric())
                        && word
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                }) {
                    return None;
                }
                (names, value)
            }
            _ => (declaration, ""),
        };
        if names
            .split(", ")
            .filter(|name| FLAGS.contains(name) || matches!(*name, "--bare" | "--print"))
            .count()
            > 1
        {
            return None;
        }
        for name in names.split(", ") {
            let valid = if let Some(long) = name.strip_prefix("--") {
                long.starts_with(|c: char| c.is_ascii_alphanumeric())
                    && long.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            } else {
                matches!(name.as_bytes(), [b'-', short] if short.is_ascii_alphanumeric())
            };
            if !valid || rows.insert(name, value).is_some() {
                return None;
            }
        }
    }
    Some(rows)
}

pub(crate) fn supports(report: &RuntimeCapabilities, flag: &str) -> bool {
    matches!(report, RuntimeCapabilities::Extension(r) if r.runtime_id() == "claude-policy" && r.status(flag) == CapabilityStatus::Supported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_unproven_policy_help_without_hidden_validation() {
        let valid =
            "Usage: claude [options] [command] [prompt]\nOptions:\n  --bare\n  -p, --print\n";
        let cases = [
            (
                "subcommand usage",
                "Usage: claude inspect [options]\n\nOptions:\n  --json-schema <schema>  inspect option\n".into(),
                "",
            ),
            (
                "malformed metavar",
                format!("{valid}  --max-turns <>x>  broken declaration\n"),
                "",
            ),
            (
                "conflicting help pages",
                "Usage: claude [options]\nOptions:\n  --resume <id>  resume\n\nUsage: claude [options]\nOptions:\n  --resume  interactive switch\n".into(),
                "",
            ),
            (
                "second Options section",
                format!("{valid}  --json-schema <schema>\n\nOptions:\n  --json-schema\n"),
                "",
            ),
            (
                "cross-stream section",
                format!("{valid}  --json-schema <schema>\n"),
                "Options:\n  --json-schema\n",
            ),
            (
                "cross-stream help page",
                format!("{valid}  --json-schema <schema>\n"),
                "Usage: claude inspect [options]\nOptions:\n  --resume [id]\n",
            ),
            (
                "duplicate alias",
                format!("{valid}  -r, --resume [id]\n  --resume <id>\n  --json-schema <schema>\n"),
                "",
            ),
            (
                "malformed option declaration",
                format!("{valid}  --max-turns=<n>\n  --json-schema <schema>\n"),
                "",
            ),
            (
                "second Options section after commands",
                format!("{valid}  --resume [id]\n\nCommands:\n  inspect\n\nOptions:\n  --resume\n"),
                "",
            ),
            (
                "distinct policies declared as aliases",
                format!("{valid}  --resume, --max-turns <id>\n"),
                "",
            ),
        ];
        let mut unexpected = Vec::new();
        for (label, stdout, stderr) in cases {
            let mut validated = false;
            let entries = policy_entries(stdout.as_bytes(), stderr.as_bytes(), || {
                validated = true;
                true
            });
            if validated
                || entries
                    .iter()
                    .any(|(_, status)| *status != CapabilityStatus::Unknown)
            {
                unexpected.push((label, validated, entries));
            }
        }
        assert!(unexpected.is_empty(), "{unexpected:?}");
    }

    #[test]
    fn missing_ambiguous_or_prose_flags_never_admit_policy() {
        let entries = policy_entries(b"Usage: claude\nOptions:\n  --max-turns <n>  limit turns\n  --resume <id>  resume\n  --other <x>  --json-schema <file>\n    --resume <id> in wrapped prose\nCommands:\n  --resume <id> in another section\n", b"", || panic!("no validation needed"));
        assert_eq!(
            entries.map(|(_, s)| s),
            [
                CapabilityStatus::Supported,
                CapabilityStatus::Supported,
                CapabilityStatus::Unknown,
            ]
        );
        for (stdout, stderr) in [
            (
                b"Usage: claude\nOptions:\n  --resume <id>\n  --resume <id>\n".as_slice(),
                b"".as_slice(),
            ),
            (b"Usage: claude\nOptions:\n  --resume <id>\n", b"\xff"),
            (b"Usage: claude", b"Options:\n  --resume <id>\n"),
            (b"--max-turns", b""),
            (
                b"Usage: claude\nOptions:\n  -p, --print\n  --bare\n  --max-turns [n]\n",
                b"",
            ),
        ] {
            assert_eq!(
                policy_entries(stdout, stderr, || panic!("ambiguous or malformed help")),
                FLAGS.map(|flag| (flag, CapabilityStatus::Unknown)),
            );
        }
    }
}
