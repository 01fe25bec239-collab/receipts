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
pub(super) const AUTH_MODE: &str = "RECEIPTS_LIVE_AUTH_MODE";

fn live(flag: &str, exe: &str, provider: ChildEnvProvider) -> (PathBuf, PathBuf, ChildEnvPolicy) {
    assert!(
        std::env::var(flag).ok().as_deref() == Some("1"),
        "{flag} must be set to 1 for an explicitly selected qualification test"
    );
    let env = live_env(provider, ChildEnvAuthMode::ApiKey);
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
    (executable, evidence, env)
}
fn policy(read_only: bool) -> RuntimeExecutionPolicy {
    RuntimeExecutionPolicy::new(
        ProcessTimeoutPolicy::new(Duration::from_secs(180), Duration::from_secs(5)).unwrap(),
        read_only,
    )
}
pub(super) fn live_env(provider: ChildEnvProvider, default: ChildEnvAuthMode) -> ChildEnvPolicy {
    let mode = match std::env::var_os(AUTH_MODE).as_deref() {
        None => default,
        Some(value) if value == "api-key" => ChildEnvAuthMode::ApiKey,
        Some(value) if value == "subscription" => ChildEnvAuthMode::Subscription,
        Some(_) => panic!("invalid {AUTH_MODE}: expected exactly api-key or subscription"),
    };
    let p = ChildEnvPolicy::new(provider, mode).unwrap();
    println!(
        "auth_mode={mode:?} admitted environment names: {:?}",
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
    write_private(
        &dir.join(format!("{name}.process")),
        format!(
            "cause={:?} exit={:?} forced_kill={} stdout_truncated={} stderr_truncated={}\n",
            p.terminal_cause(),
            p.exit_code(),
            p.forced_kill_required(),
            p.stdout().truncated(),
            p.stderr().truncated(),
        )
        .as_bytes(),
    )
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
        && fs::read(worktree.join("tracked.txt")).is_ok_and(|bytes| bytes == b"original\n")
}
fn clean(root: &Path) -> bool {
    fs::remove_dir_all(root).is_ok() && matches!(root.try_exists(), Ok(false))
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
    println!(
        "provider={:?} protected={} reduced_isolation={}",
        result.provider_result(),
        result.protected(),
        result.reduced_isolation,
    );
    if !result.outcome.process().success()
        || result.outcome.process().stderr().truncated()
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
fn claude_cases(exe: &Path, evidence: &Path, env: ChildEnvPolicy) -> bool {
    let f = Fixture::without_worker(Some("task"));
    let a = ClaudeAdapter::new(exe).with_env_policy(env);
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
            if saved && refused && claude_value(r, false, "FIRST") {
                let resumed = a
                    .resume_with_policy(&task, &f.workspace, &p, ticket)
                    .and_then(|h| a.collect_result(&h));
                resume_ok = resumed.as_ref().is_ok_and(|r| {
                    capture(evidence, "claude-resume-second", r.outcome.process()).is_ok()
                        && claude_value(r, false, "SECOND")
                }) && matches!(
                    a.resume_with_policy(&task, &f.workspace, &p, ticket),
                    Err(FailureClass::PolicyBlocked)
                );
            }
        }
    }
    resume_ok &= unchanged(f.workspace.worktree_path(), f.workspace.base_sha().as_str());
    outcomes.push(("claude-resume", resume_ok));
    let root = f.root.clone();
    drop(f);
    let cleanup = matches!(root.try_exists(), Ok(false));
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
fn codex_case(exe: &Path, evidence: &Path, env: ChildEnvPolicy) -> bool {
    use crate::protected_launch_tests::launcher;
    use std::mem::ManuallyDrop;
    let launcher = launcher();
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
    let a = CodexAdapter::new(exe).with_env_policy(env);
    let p = policy(true).with_protected_launch(ProtectedLaunch::new(launcher, &f.repo));
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
            released = matches!(h.release_custody(), Some(Ok(())));
            // Git inspection runs under the original owner after custody is restored.
            ok = saved
                && released
                && r.protected()
                && process.success()
                && r.failure().is_none()
                && r.protocol_termination() == CodexProtocolTermination::Completed
                && !s.truncated()
                && !process.stderr().truncated()
                && denied_write(&[s.head(), s.tail()].concat(), &command)
                && unchanged(ws.worktree_path(), ws.base_sha().as_str());
            println!(
                "case=codex-protected-review protected={} protocol={:?} release={released}",
                r.protected(),
                r.protocol_termination()
            );
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
    let (exe, evidence, env) = live(
        "RECEIPTS_QUALIFY_CLAUDE",
        "RECEIPTS_LIVE_CLAUDE_EXE",
        ChildEnvProvider::Claude,
    );
    assert!(claude_cases(&exe, &evidence, env));
}
#[cfg(target_os = "linux")]
#[test]
#[ignore = "live provider; disposable admitted Linux host and A1 disposition required"]
fn live_codex_protected_reviewer() {
    let (exe, evidence, env) = live(
        "RECEIPTS_QUALIFY_CODEX",
        "RECEIPTS_LIVE_CODEX_EXE",
        ChildEnvProvider::Codex,
    );
    assert!(codex_case(&exe, &evidence, env));
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
    let script = r##"
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
"##;
    for (selection, expected_key) in [
        (None, "claude-sentinel"),
        (Some("api-key"), "claude-sentinel"),
        (Some("subscription"), "absent"),
    ] {
        fs::write(
            &exe,
            format!(
                "#!/bin/sh\n[ \"${{ANTHROPIC_API_KEY-absent}}\" = '{expected_key}' ] && \
                 [ -z \"${{OPENAI_API_KEY+x}}${{CODEX_HOME+x}}\" ] && \
                 [ \"${{CLAUDE_CONFIG_DIR-absent}}\" = 'claude-config' ] || exit 9\n{script}"
            ),
        )
        .unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        let good = root.join(selection.unwrap_or("default"));
        fs::create_dir(&good).unwrap();
        fs::set_permissions(&good, fs::Permissions::from_mode(0o700)).unwrap();
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.env_clear()
            .env("RECEIPTS_QUALIFY_CLAUDE", "1")
            .env("RECEIPTS_LIVE_CLAUDE_EXE", &exe)
            .env("RECEIPTS_LIVE_EVIDENCE_DIR", &good)
            .env("ANTHROPIC_API_KEY", "claude-sentinel")
            .env("OPENAI_API_KEY", "codex-sentinel")
            .env("CLAUDE_CONFIG_DIR", "claude-config")
            .env("CODEX_HOME", "codex-config")
            .args([
                "qualification_tests::live_claude_qualification",
                "--ignored",
                "--exact",
            ]);
        if let Some(selection) = selection {
            cmd.env(AUTH_MODE, selection);
        }
        let out = cmd.output().unwrap();
        assert!(
            out.status.success(),
            "synthetic Claude {selection:?}: {out:?}"
        );
        for name in ["claude-plan", "claude-schema-turns", "claude-resume"] {
            let summary = fs::read_to_string(good.join(format!("{name}.summary"))).unwrap();
            assert!(summary.contains("verdict=PASS cleanup=true"));
        }
    }
    fs::write(&exe, "#!/bin/sh\nexit 0\n").unwrap();
    let bad = root.join("bad");
    fs::create_dir(&bad).unwrap();
    let env =
        ChildEnvPolicy::new(ChildEnvProvider::Claude, ChildEnvAuthMode::Subscription).unwrap();
    assert!(!claude_cases(&exe, &bad, env));
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

#[test]
fn live_entry_points_require_opt_in_and_reject_invalid_selection() {
    let root = std::env::temp_dir().join(format!("ra-qualification-optin-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let exe = root.join("provider");
    let marker = root.join("provider-started");
    fs::write(&exe, format!("#!/bin/sh\n{}\n", write_command(&marker))).unwrap();
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
    let cases = [
        (
            "qualification_tests::live_claude_qualification",
            "RECEIPTS_QUALIFY_CLAUDE",
            "RECEIPTS_LIVE_CLAUDE_EXE",
        ),
        (
            "live_tests::live_claude_ordinary_success_and_cancel",
            "RECEIPTS_LIVE_CLAUDE",
            "RECEIPTS_LIVE_CLAUDE_EXE",
        ),
        (
            "live_tests::live_codex_ordinary_success_and_cancel",
            "RECEIPTS_LIVE_CODEX",
            "RECEIPTS_LIVE_CODEX_EXE",
        ),
        #[cfg(target_os = "linux")]
        (
            "qualification_tests::live_codex_protected_reviewer",
            "RECEIPTS_QUALIFY_CODEX",
            "RECEIPTS_LIVE_CODEX_EXE",
        ),
    ];
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    for (test, flag, exe_var) in cases {
        for value in [None, Some("0"), Some("")] {
            for prepared in [false, true] {
                let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
                cmd.env_clear().current_dir(&root).args([
                    test,
                    "--ignored",
                    "--exact",
                    "--nocapture",
                ]);
                if prepared {
                    cmd.env(exe_var, &exe)
                        .env("RECEIPTS_LIVE_EVIDENCE_DIR", &root);
                }
                if let Some(value) = value {
                    cmd.env(flag, value);
                }
                let out = cmd.output().unwrap();
                assert!(!out.status.success(), "{test}: {value:?} passed");
                let text = String::from_utf8_lossy(&out.stderr);
                assert!(
                    text.contains(&format!("{flag} must be set to 1")),
                    "unexpected refusal: {text}"
                );
                assert!(!root.join("provider-started").exists());
            }
        }
        use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
        for value in [
            OsStr::new("api-key,subscription"),
            OsStr::new("Subscription"),
            OsStr::new(""),
            OsStr::from_bytes(b"subscription\xff"),
        ] {
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .env_clear()
                .env(flag, "1")
                .env(exe_var, &exe)
                .env("RECEIPTS_LIVE_EVIDENCE_DIR", &root)
                .env(AUTH_MODE, value)
                .args([test, "--ignored", "--exact", "--nocapture"])
                .output()
                .unwrap();
            assert!(!out.status.success());
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                stderr.contains("invalid RECEIPTS_LIVE_AUTH_MODE"),
                "{stderr}"
            );
            assert!(
                !marker.exists(),
                "{test}: invalid selector started the synthetic CLI"
            );
        }
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .env_clear()
            .args([test, "--exact"])
            .output()
            .unwrap();
        assert!(out.status.success());
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("1 ignored"), "not skipped: {text}");
    }
    assert!(clean(&root));
}
