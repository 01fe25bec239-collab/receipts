//! Optional policy controls require fresh, complete CLI help evidence.
use crate::*;
use receipts_workspace_execution::{
    WorkspaceHandle,
    execution::{
        ChildEnvPolicy, ProcessRunRequest, ProcessTimeoutPolicy, run_with_timeout_and_capture,
    },
};
use std::path::Path;

pub(crate) fn probe(
    exe: &Path,
    workspace: &WorkspaceHandle,
    timeout: &ProcessTimeoutPolicy,
    env: Option<&ChildEnvPolicy>,
) -> RuntimeCapabilities {
    let Ok(request) = ProcessRunRequest::new(
        exe,
        ["--help"],
        workspace.worktree_path(),
        workspace.worktree_path(),
    ) else {
        return RuntimeCapabilities::Unknown;
    };
    let request = match env {
        Some(env) => request.with_env_policy(env.clone()),
        None => request,
    };
    let Ok(captured) = run_with_timeout_and_capture(&request, timeout) else {
        return RuntimeCapabilities::Unknown;
    };
    if !captured.outcome().success()
        || captured.stdout().truncated()
        || captured.stderr().truncated()
    {
        return RuntimeCapabilities::Unknown;
    }
    let bytes = [
        captured.stdout().head(),
        captured.stdout().tail(),
        captured.stderr().head(),
        captured.stderr().tail(),
    ]
    .concat();
    parse(&bytes)
}
fn parse(bytes: &[u8]) -> RuntimeCapabilities {
    let Ok(help) = std::str::from_utf8(bytes) else {
        return RuntimeCapabilities::Unknown;
    };
    if !help.contains("Usage:") || !help.contains("Options:") {
        return RuntimeCapabilities::Unknown;
    }
    let entries = ["--max-turns", "--resume", "--json-schema"].map(|flag| {
        let count = help
            .lines()
            .filter(|line| line.starts_with(char::is_whitespace))
            .filter(|line| line.trim_start().starts_with('-'))
            .filter(|line| {
                line.split_whitespace()
                    .take_while(|s| s.starts_with('-') || s.starts_with('<') || s.starts_with('['))
                    .any(|s| s.trim_end_matches(',') == flag)
            })
            .count();
        (
            flag,
            if count == 1 {
                CapabilityStatus::Supported
            } else {
                CapabilityStatus::Unknown
            },
        )
    });
    RuntimeCapabilities::Extension(
        ExtensionCapabilityReport::new("claude-policy", entries)
            .expect("fixed distinct nonempty names"),
    )
}
pub(crate) fn supports(report: &RuntimeCapabilities, flag: &str) -> bool {
    matches!(report, RuntimeCapabilities::Extension(r) if r.runtime_id() == "claude-policy" && r.status(flag) == CapabilityStatus::Supported)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_ambiguous_or_prose_flags_never_admit_policy() {
        let report = parse(b"Usage: claude\nOptions:\n  --max-turns <n>  limit turns\n  --resume <id>  resume\n  --other <x>  description mentions --json-schema\n");
        assert!(supports(&report, "--max-turns"));
        assert!(supports(&report, "--resume"));
        assert!(!supports(&report, "--json-schema"));
        assert!(!supports(
            &parse(b"Usage: claude\nOptions:\n  --resume <id>\n  --resume <id>\n"),
            "--resume"
        ));
        assert!(!supports(&parse(b"--max-turns"), "--max-turns"));
    }
}
