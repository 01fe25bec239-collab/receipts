//! Prepared qualification cases. Ignored live tests require a separately
//! dispositioned provider context; synthetic controls are not provider proof.
use super::*;
use crate::protected_launch_tests::Fixture;
use receipts_runtime_bindings::*;
use receipts_workspace_execution::execution::{
    ChildEnvAuthMode, ChildEnvPolicy, ChildEnvProvider, LiveProcessOutcome, ProcessTimeoutPolicy,
};
use receipts_workspace_execution::{
    CommitSha, ReviewWorkspaceRequest, WorkspaceCheckpointRef, WorkspaceCheckpointRefType,
    provision_review_workspace,
};
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

const SCHEMA: &str = r#"{"type":"object","properties":{"answer":{"type":"string","enum":["OK"]}},"required":["answer"],"additionalProperties":false}"#;
const WRITE: &str = "printf qualification-marker > ";

fn live(flag: &str, exe: &str) -> Option<(PathBuf, PathBuf)> {
    if std::env::var(flag).ok().as_deref() != Some("1") {
        return None;
    }
    let executable = PathBuf::from(std::env::var_os(exe).expect("explicit executable"));
    let evidence =
        PathBuf::from(std::env::var_os("RECEIPTS_LIVE_EVIDENCE_DIR").expect("evidence directory"));
    assert!(executable.is_absolute() && evidence.is_absolute());
    let evidence = fs::canonicalize(evidence).expect("existing evidence directory");
    let repo = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).unwrap();
    assert!(
        !evidence.starts_with(repo),
        "evidence must be outside repository"
    );
    assert_eq!(
        fs::metadata(&evidence).unwrap().permissions().mode() & 0o077,
        0,
        "private evidence directory required"
    );
    Some((executable, evidence))
}
fn policy(read_only: bool) -> RuntimeExecutionPolicy {
    RuntimeExecutionPolicy::new(
        ProcessTimeoutPolicy::new(Duration::from_secs(180), Duration::from_secs(5)).unwrap(),
        read_only,
    )
}
fn env(provider: ChildEnvProvider) -> ChildEnvPolicy {
    let p = ChildEnvPolicy::new(provider, ChildEnvAuthMode::ApiKey).unwrap();
    println!(
        "admitted environment names: {:?}",
        p.names().collect::<Vec<_>>()
    );
    p
}
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?
        .write_all(bytes)
}
fn capture(dir: &Path, name: &str, p: &LiveProcessOutcome) -> std::io::Result<()> {
    for (label, s) in [("stdout", p.stdout()), ("stderr", p.stderr())] {
        if s.truncated() {
            write_private(&dir.join(format!("{name}.{label}.head")), s.head())?;
            write_private(&dir.join(format!("{name}.{label}.tail")), s.tail())?;
        } else {
            write_private(
                &dir.join(format!("{name}.{label}")),
                &[s.head(), s.tail()].concat(),
            )?;
        }
    }
    Ok(())
}
fn unchanged(worktree: &Path, base: &str) -> bool {
    let git = |args: &[&str]| {
        std::process::Command::new("/usr/bin/git")
            .current_dir(worktree)
            .args(args)
            .output()
    };
    matches!(git(&["status", "--porcelain", "--untracked-files=all", "--ignored=matching"]), Ok(o) if o.status.success() && o.stdout.is_empty())
        && matches!(git(&["rev-parse", "HEAD"]), Ok(o) if o.status.success() && o.stdout.trim_ascii() == base.as_bytes())
}
fn clean(root: &Path) -> bool {
    fs::remove_dir_all(root).is_ok() && !root.exists()
}
fn cleanup_after_release(released: bool, root: &Path) -> bool {
    released && clean(root)
}
fn summary(dir: &Path, name: &str, pass: bool, cleanup: bool) {
    let line = format!(
        "case={name} verdict={} cleanup={}\n",
        if pass && cleanup { "PASS" } else { "FAIL" },
        cleanup
    );
    write_private(&dir.join(format!("{name}.summary")), line.as_bytes()).unwrap();
    print!("{line}");
}
fn claude_value(result: &ClaudeAttemptResult, schema: bool, expected: &str) -> bool {
    if !result.outcome.process().success()
        || result.failure().is_some()
        || result.protected()
        || !result.reduced_isolation
    {
        return false;
    }
    let Ok(protocol) = result.outcome.protocol() else {
        return false;
    };
    let Ok(parsed) = protocol.result() else {
        return false;
    };
    if parsed.provider_result() != ClaudeProviderResult::Success {
        return false;
    }
    let records: Vec<_> = protocol
        .records()
        .iter()
        .filter(|r| r.value()["type"] == "result")
        .collect();
    let value = records[0].value();
    if schema {
        value["structured_output"] == serde_json::json!({"answer":"OK"})
            && value["num_turns"]
                .as_u64()
                .is_some_and(|n| (1..=2).contains(&n))
    } else {
        parsed.text() == Some(expected)
    }
}
fn claude_cases(exe: &Path, evidence: &Path, env: Option<ChildEnvPolicy>) -> bool {
    let f = Fixture::without_worker(Some("task"));
    let a = ClaudeAdapter::new(exe);
    let a = match env {
        Some(env) => a.with_env_policy(env),
        None => a,
    };
    let mut outcomes = Vec::new();
    // Plan mode is ordinary and reduced isolation; ask for a harmless read-only answer.
    let task = f.task_with_objective("Reply with exactly OK. Do not run commands or change files.");
    let p = policy(true);
    let plan = a
        .start(&task, &f.workspace, &p)
        .and_then(|h| a.collect_result(&h));
    let ok = plan.as_ref().is_ok_and(|r| {
        capture(evidence, "claude-plan", r.outcome.process()).is_ok()
            && claude_value(r, false, "OK")
    }) && unchanged(f.workspace.worktree_path(), f.workspace.base_sha().as_str());
    outcomes.push(("claude-plan", ok));
    let mut p = policy(false);
    p.turn_budget = Some(2);
    p.structured_output_schema = Some(SCHEMA.into());
    let schema = a
        .start(&task, &f.workspace, &p)
        .and_then(|h| a.collect_result(&h));
    let ok = schema.as_ref().is_ok_and(|r| {
        capture(evidence, "claude-schema-turns", r.outcome.process()).is_ok()
            && claude_value(r, true, "")
    }) && unchanged(f.workspace.worktree_path(), f.workspace.base_sha().as_str());
    outcomes.push(("claude-schema-turns", ok));
    let task = f.task_with_objective("In this session, on your first response reply exactly FIRST. If this same request is resumed, reply exactly SECOND. Do not change files or run commands.");
    let mut p = policy(false);
    p.session_persistence = true;
    let initial = a
        .start(&task, &f.workspace, &p)
        .and_then(|h| a.collect_result(&h));
    let mut resume_ok = false;
    if let Ok(r) = initial.as_ref() {
        let saved = capture(evidence, "claude-resume-first", r.outcome.process()).is_ok();
        if let Some(ticket) = r.resume_ticket() {
            let mut refused = p.clone();
            refused.read_only = true;
            let refused = matches!(
                a.resume_with_policy(&task, &f.workspace, &refused, ticket),
                Err(FailureClass::PolicyBlocked)
            );
            let resumed = a
                .resume_with_policy(&task, &f.workspace, &p, ticket)
                .and_then(|h| a.collect_result(&h));
            let resumed_ok = resumed.as_ref().is_ok_and(|r| {
                let saved = capture(evidence, "claude-resume-second", r.outcome.process()).is_ok();
                saved && claude_value(r, false, "SECOND")
            });
            resume_ok = saved
                && refused
                && claude_value(r, false, "FIRST")
                && resumed_ok
                && matches!(
                    a.resume_with_policy(&task, &f.workspace, &p, ticket),
                    Err(FailureClass::PolicyBlocked)
                );
        }
    }
    resume_ok &= unchanged(f.workspace.worktree_path(), f.workspace.base_sha().as_str());
    outcomes.push(("claude-resume", resume_ok));
    let root = f.root.clone();
    drop(f);
    let cleanup = !root.exists();
    for &(name, ok) in &outcomes {
        summary(evidence, name, ok, cleanup);
    }
    cleanup && outcomes.iter().all(|(_, ok)| *ok)
}

/// A denied command is evidence only if the actual command execution record
/// names the bounded write and reports a nonzero exit. Refusal to try is inconclusive.
fn denied_write(bytes: &[u8], command: &str) -> bool {
    bytes
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .any(|v| {
            v["type"] == "item.completed"
                && v["item"]["type"] == "command_execution"
                && v["item"]["command"]
                    .as_str()
                    .is_some_and(|s| s.trim() == command)
                && v["item"]["exit_code"].as_i64().is_some_and(|n| n != 0)
                && v["item"]["aggregated_output"].as_str().is_some_and(|s| {
                    let s = s.to_ascii_lowercase();
                    s.contains("permission denied") || s.contains("read-only file system")
                })
        })
}
fn write_command(path: &Path) -> String {
    format!("{WRITE}'{}'", path.to_str().unwrap().replace('\'', "'\\''"))
}
fn review(sha: &str, command: &str) -> RuntimeCapsuleFamily {
    RuntimeCapsuleFamily::Review(ReviewCapsuleNonTemporalCore::new(
        "review".into(), "task".into(), "attempt".into(), sha.into(), sha.into(),
        format!("Test whether the write is refused: run exactly `{command}` once, then report the result."),
        vec![ReviewCapsuleCriterion::new("write".into(), "Report the write attempt result".into(), ReviewCapsuleCriterionKind::Semantic, None, None).unwrap()],
        None, None, None, WorkspaceCheckpointRef::new(WorkspaceCheckpointRefType::RepoPath, "tracked.txt", None, None).unwrap(),
        vec!["tracked.txt".into()], None, None, ReviewCapsuleReviewScope::Full, ReviewCapsuleSeverityPolicy::new(vec!["HIGH".into()], None).unwrap(), false, None, 0,
    ).unwrap())
}
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn codex_case(exe: &Path, evidence: &Path) -> bool {
    use crate::protected_launch_tests::launcher;
    use std::mem::ManuallyDrop;
    let f = ManuallyDrop::new(Fixture::without_worker(Some("task")));
    let review_ws = provision_review_workspace(&ReviewWorkspaceRequest {
        repository_root: f.repo.clone(),
        workspace_id: "ws-qualification-review".into(),
        task_id: "task".into(),
        reviewed_sha: CommitSha::parse(f.workspace.base_sha().as_str()).unwrap(),
        worktree_path: f.root.join("review"),
    })
    .unwrap();
    let ws = review_ws.handle();
    let a = CodexAdapter::new(exe).with_env_policy(env(ChildEnvProvider::Codex));
    let p = policy(true).with_protected_launch(ProtectedLaunch::new(launcher(), &f.repo));
    let command = write_command(&ws.worktree_path().join("tracked.txt"));
    let started = a.start(&review(ws.base_sha().as_str(), &command), ws, &p);
    let mut released = false;
    let mut ok = false;
    if let Ok(h) = started {
        if h.protected()
            && let Ok(r) = a.collect_result(&h)
        {
            let process = r.outcome.process();
            let saved = capture(evidence, "codex-protected-review", process).is_ok();
            let s = process.stdout();
            ok = saved
                && process.success()
                && r.failure().is_none()
                && r.protocol_termination() == CodexProtocolTermination::Completed
                && !s.truncated()
                && denied_write(&[s.head(), s.tail()].concat(), &command)
                && unchanged(ws.worktree_path(), ws.base_sha().as_str())
                && fs::read(ws.worktree_path().join("tracked.txt"))
                    .is_ok_and(|v| v == b"original\n");
            released = matches!(h.release_custody(), Some(Ok(())));
        }
        drop(h);
    }
    // Unknown custody is quarantined; do not remove the fixture.
    let cleanup = cleanup_after_release(released, &f.root);
    summary(evidence, "codex-protected-review", ok, cleanup);
    ok && cleanup
}
#[test]
#[ignore = "live provider; A1 context disposition required"]
fn live_claude_qualification() {
    if let Some((exe, evidence)) = live("RECEIPTS_QUALIFY_CLAUDE", "RECEIPTS_LIVE_CLAUDE_EXE") {
        assert!(claude_cases(
            &exe,
            &evidence,
            Some(env(ChildEnvProvider::Claude))
        ));
    }
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "live provider; disposable admitted Linux host and A1 disposition required"]
fn live_codex_protected_reviewer() {
    if let Some((exe, evidence)) = live("RECEIPTS_QUALIFY_CODEX", "RECEIPTS_LIVE_CODEX_EXE") {
        assert!(codex_case(&exe, &evidence));
    }
}
#[test]
fn refusal_requires_a_real_failed_target_command() {
    let event = |cmd: &str, exit| {
        serde_json::json!({"type":"item.completed","item":{"type":"command_execution","command":cmd,"exit_code":exit,"aggregated_output":"Permission denied"}}).to_string()
    };
    let command = &write_command(Path::new("/tmp/fixture/tracked.txt"));
    assert_eq!(
        write_command(Path::new("/tmp/a'b")),
        "printf qualification-marker > '/tmp/a'\\''b'"
    );
    assert!(denied_write(event(command, 1).as_bytes(), command));
    assert!(!denied_write(event(command, 0).as_bytes(), command));
    assert!(!denied_write(event("echo denied", 1).as_bytes(), command));
    assert!(!denied_write(
        event(&format!("echo {command}"), 1).as_bytes(),
        command
    ));
    assert!(!denied_write(b"refused", command));
    let no_permission = serde_json::json!({"type":"item.completed","item":{"type":"command_execution","command":command,"exit_code":1,"aggregated_output":"other error"}}).to_string();
    assert!(!denied_write(no_permission.as_bytes(), command));
}
#[test]
fn cleanup_control_and_private_create_new() {
    let root =
        std::env::temp_dir().join(format!("ra-qualification-control-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let file = root.join("evidence");
    write_private(&file, b"first").unwrap();
    assert!(write_private(&file, b"second").is_err());
    assert_eq!(fs::read(&file).unwrap(), b"first");
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!cleanup_after_release(false, &root));
    assert!(file.exists(), "unproven custody must be retained");
    assert!(cleanup_after_release(true, &root));
    assert!(!root.exists());
    assert!(!clean(&root));
}

#[test]
fn synthetic_claude_cases_and_failure_are_bounded() {
    let root = std::env::temp_dir().join(format!("ra-qualification-cli-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let exe = root.join("claude");
    fs::write(&exe, r##"#!/bin/sh
[ -z "${ANTHROPIC_API_KEY+x}" ] || exit 9
if [ "$1" = --help ]; then
  printf 'Usage: claude\nOptions:\n  --max-turns <n>  limit\n  --resume <id>  resume\n  --json-schema <schema>  schema\n'
  exit 0
fi
mode=plan
for arg in "$@"; do
  case "$arg" in --json-schema) mode=schema;; --resume) mode=resume;; --no-session-persistence) :;; esac
done
/bin/cat >/dev/null
case "$mode" in
  schema) printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","structured_output":{"answer":"OK"},"num_turns":1}';;
  resume) printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"SECOND"}';;
  *) case " $* " in *" --no-session-persistence "*) text=OK;; *) text=FIRST;; esac
     printf '{"type":"result","subtype":"success","is_error":false,"result":"%s","session_id":"12345678-1234-1234-1234-123456789abc"}\n' "$text";;
esac
"##).unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    let good = root.join("good");
    fs::create_dir(&good).unwrap();
    assert!(claude_cases(&exe, &good, None));
    for name in ["claude-plan", "claude-schema-turns", "claude-resume"] {
        let summary = fs::read_to_string(good.join(format!("{name}.summary"))).unwrap();
        assert!(summary.contains("verdict=PASS cleanup=true"));
    }
    fs::write(&exe, "#!/bin/sh\nexit 0\n").unwrap();
    let bad = root.join("bad");
    fs::create_dir(&bad).unwrap();
    assert!(!claude_cases(&exe, &bad, None));
    for name in ["claude-plan", "claude-schema-turns", "claude-resume"] {
        let summary = fs::read_to_string(bad.join(format!("{name}.summary"))).unwrap();
        assert!(summary.contains("verdict=FAIL cleanup=true"));
    }
    assert!(clean(&root));
}

#[test]
fn review_fixture_is_bound_and_ordinary_launch_is_refused() {
    let f = Fixture::without_worker(Some("task"));
    let ws = provision_review_workspace(&ReviewWorkspaceRequest {
        repository_root: f.repo.clone(),
        workspace_id: "ws-control-review".into(),
        task_id: "task".into(),
        reviewed_sha: CommitSha::parse(f.workspace.base_sha().as_str()).unwrap(),
        worktree_path: f.root.join("review"),
    })
    .unwrap();
    let task = review(f.workspace.base_sha().as_str(), "command");
    let RuntimeCapsuleFamily::Review(ref capsule) = task else {
        panic!("review expected")
    };
    assert!(crate::review_prompt::binding_matches(capsule, ws.handle()));
    assert!(matches!(
        CodexAdapter::new("/usr/bin/false").start(&task, ws.handle(), &policy(true)),
        Err(FailureClass::PolicyBlocked)
    ));
    let root = f.root.clone();
    drop(ws);
    drop(f);
    assert!(!root.exists());
}
