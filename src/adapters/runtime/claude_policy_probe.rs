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
    let (Ok(stdout), Ok(stderr)) = (std::str::from_utf8(stdout), std::str::from_utf8(stderr))
    else {
        return FLAGS.map(|flag| (flag, CapabilityStatus::Unknown));
    };
    let rows: Vec<_> = [stdout, stderr].into_iter().flat_map(option_rows).collect();
    let mut entries = FLAGS.map(|flag| {
        let mut matches = rows.iter().filter(|(name, _)| *name == flag);
        let supported = matches.next().is_some_and(|(_, value)| {
            let required = value.strip_prefix('<').and_then(|v| v.strip_suffix('>'));
            let optional = value.strip_prefix('[').and_then(|v| v.strip_suffix(']'));
            required
                .or(optional.filter(|_| flag == "--resume"))
                .is_some_and(|v| !v.is_empty() && !v.contains(char::is_whitespace))
        }) && matches.next().is_none();
        (flag, status(supported))
    });
    // Only absence can use the hidden-option check. Ambiguous declarations stay Unknown.
    let unique_switch = |flag| {
        let mut matches = rows.iter().filter(|(name, _)| *name == flag);
        matches.next() == Some(&(flag, "")) && matches.next().is_none()
    };
    if !rows.iter().any(|(name, _)| *name == "--max-turns")
        && unique_switch("--bare")
        && unique_switch("--print")
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
fn option_rows(text: &str) -> impl Iterator<Item = (&str, &str)> {
    let mut lines = text.lines();
    let has_options = lines.any(|line| {
        line.strip_prefix("Usage: ")
            .is_some_and(|usage| usage.split_ascii_whitespace().next() == Some("claude"))
    }) && lines.any(|line| line == "Options:");
    // The two-space declaration column excludes wrapped option descriptions.
    lines
        .take_while(move |line| has_options && (line.is_empty() || line.starts_with(' ')))
        .filter_map(|line| {
            let row = line.strip_prefix("  ").filter(|row| row.starts_with('-'))?;
            let declaration = row.split_once("  ").map_or(row, |(before, _)| before);
            let declaration = declaration
                .strip_prefix("-r, ")
                .or_else(|| declaration.strip_prefix("-p, "))
                .unwrap_or(declaration);
            Some(declaration.split_once(' ').unwrap_or((declaration, "")))
        })
}

pub(crate) fn supports(report: &RuntimeCapabilities, flag: &str) -> bool {
    matches!(report, RuntimeCapabilities::Extension(r) if r.runtime_id() == "claude-policy" && r.status(flag) == CapabilityStatus::Supported)
}

#[cfg(test)]
mod tests {
    use super::*;

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
