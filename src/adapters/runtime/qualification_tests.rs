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
pub(super) fn claude_rate_limited(protocol: &ClaudeStreamJson<'_>) -> bool {
    protocol.records().iter().any(|record| {
        let value = record.value();
        match value["type"].as_str() {
            Some("system") => {
                value["subtype"] == "api_retry"
                    && (value["error_status"].as_u64() == Some(429)
                        || value["error"] == "rate_limit")
            }
            Some("rate_limit_event") => value["rate_limit_info"]["status"] == "rejected",
            _ => false,
        }
    })
}

fn claude_value(result: &ClaudeAttemptResult, schema: bool, expected: &str) -> bool {
    let protocol = result.outcome.protocol();
    let rate_limited = protocol.as_ref().is_ok_and(claude_rate_limited);
    println!(
        "provider={:?} protected={} reduced_isolation={} rate_limited={rate_limited}",
        result.provider_result(),
        result.protected(),
        result.reduced_isolation,
    );
    if !result.outcome.process().success()
        || result.outcome.process().stderr().truncated()
        || result.failure().is_some()
        || rate_limited
        || result.protected()
        || !result.reduced_isolation
    {
        return false;
    }
    let Ok(protocol) = protocol else {
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
    run_claude_cases(&f, &a, evidence, &mut outcomes);
    let root = f.root.clone();
    drop(f);
    let cleanup = matches!(root.try_exists(), Ok(false));
    for &(name, ok) in &outcomes {
        summary(evidence, name, ok, cleanup);
    }
    cleanup && outcomes.len() == 3 && outcomes.iter().all(|(_, ok)| *ok)
}

fn collect_claude_case(
    adapter: &ClaudeAdapter,
    evidence: &Path,
    name: &str,
    started: Result<ClaudeAttemptHandle, FailureClass>,
) -> Option<ClaudeAttemptResult> {
    let failure = |stage, class: FailureClass| {
        eprintln!("case={name} stage={stage} failure={}", class.as_str());
    };
    let handle = started.map_err(|class| failure("start", class)).ok()?;
    let result = adapter
        .collect_result(&handle)
        .map_err(|class| failure("collect", class))
        .ok()?;
    capture(evidence, name, result.outcome.process())
        .map_err(|error| eprintln!("case={name} stage=capture failure={:?}", error.kind()))
        .ok()?;
    Some(result)
}

fn run_claude_cases(
    f: &Fixture,
    a: &ClaudeAdapter,
    evidence: &Path,
    outcomes: &mut Vec<(&'static str, bool)>,
) {
    // Plan mode is ordinary and reduced isolation; ask for a harmless read-only answer.
    let task = f.task_with_objective("Reply with exactly OK. Do not run commands or change files.");
    let p = policy(true);
    let plan = collect_claude_case(a, evidence, "claude-plan", a.start(&task, &f.workspace, &p));
    let ok = plan.as_ref().is_some_and(|r| claude_value(r, false, "OK"))
        && unchanged(f.workspace.worktree_path(), f.workspace.base_sha().as_str());
    outcomes.push(("claude-plan", ok));
    if !ok {
        return;
    }
    let mut p = policy(false);
    p.turn_budget = Some(2);
    p.structured_output_schema = Some(SCHEMA.into());
    let schema = collect_claude_case(
        a,
        evidence,
        "claude-schema-turns",
        a.start(&task, &f.workspace, &p),
    );
    let ok = schema.as_ref().is_some_and(|r| claude_value(r, true, ""))
        && unchanged(f.workspace.worktree_path(), f.workspace.base_sha().as_str());
    outcomes.push(("claude-schema-turns", ok));
    if !ok {
        return;
    }
    let task = f.task_with_objective("In this session, on your first response reply exactly FIRST. If this same request is resumed, reply exactly SECOND. Do not change files or run commands.");
    let mut p = policy(false);
    p.session_persistence = true;
    let initial = collect_claude_case(
        a,
        evidence,
        "claude-resume-first",
        a.start(&task, &f.workspace, &p),
    );
    let mut resume_ok = false;
    if let Some(r) = initial.as_ref()
        && let Some(ticket) = r.resume_ticket()
    {
        let mut refused = p.clone();
        refused.read_only = true;
        let refused = matches!(
            a.resume_with_policy(&task, &f.workspace, &refused, ticket),
            Err(FailureClass::PolicyBlocked)
        );
        if refused && claude_value(r, false, "FIRST") {
            let resumed = collect_claude_case(
                a,
                evidence,
                "claude-resume-second",
                a.resume_with_policy(&task, &f.workspace, &p, ticket),
            );
            resume_ok = resumed
                .as_ref()
                .is_some_and(|r| claude_value(r, false, "SECOND"))
                && matches!(
                    a.resume_with_policy(&task, &f.workspace, &p, ticket),
                    Err(FailureClass::PolicyBlocked)
                );
        }
    }
    resume_ok &= unchanged(f.workspace.worktree_path(), f.workspace.base_sha().as_str());
    outcomes.push(("claude-resume", resume_ok));
}

fn exact_write(actual: &str, command: &str) -> bool {
    if actual == command {
        return true;
    }
    let mut parts = actual.splitn(3, ' ');
    let (Some(shell), Some("-c" | "-lc"), Some(body)) = (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let shell = shell
        .strip_prefix("/bin/")
        .or_else(|| shell.strip_prefix("/usr/bin/"));
    if !matches!(shell, Some("bash" | "sh" | "dash" | "zsh")) {
        return false;
    }
    // Codex displays the shell argv as a shell-quoted string. Compare full arguments without evaluating them.
    body == format!("'{}'", command.replace('\'', "'\\''"))
        || body
            == format!(
                "\"{}\"",
                command
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('$', "\\$")
                    .replace('`', "\\`")
            )
}

/// A denied command is evidence only if the actual command execution record
/// names the bounded write and reports a nonzero exit. Refusal to try is inconclusive.
fn denied_write(bytes: &[u8], command: &str, target: &Path) -> bool {
    let target = target.to_str().unwrap();
    bytes
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
        .any(|v| {
            v["type"] == "item.completed"
                && v["item"]["type"] == "command_execution"
                && v["item"]["command"]
                    .as_str()
                    .is_some_and(|s| exact_write(s.trim(), command))
                && v["item"]["exit_code"].as_i64().is_some_and(|n| n != 0)
                && v["item"]["aggregated_output"].as_str().is_some_and(|s| {
                    s.lines().any(|line| {
                        let Some((subject, reason)) = line.rsplit_once(": ") else {
                            return false;
                        };
                        let path = subject.rsplit(": ").next().unwrap_or(subject);
                        (path == target || path.strip_prefix("cannot create ") == Some(target))
                            && matches!(
                                reason.trim().to_ascii_lowercase().as_str(),
                                "permission denied" | "read-only file system"
                            )
                    })
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
    let target = ws.worktree_path().join("tracked.txt");
    let command = write_command(&target);
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
                && denied_write(&[s.head(), s.tail()].concat(), &command, &target)
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
    let target = Path::new("/tmp/fixture/tracked.txt");
    let command = &write_command(target);
    let event = |cmd: &str, exit| {
        serde_json::json!({"type":"item.completed","item":{"type":"command_execution","command":cmd,"exit_code":exit,"aggregated_output":"/bin/bash: line 1: /tmp/fixture/tracked.txt: Permission denied"}}).to_string()
    };
    let denied = |s: &str| denied_write(s.as_bytes(), command, target);
    assert_eq!(
        write_command(Path::new("/tmp/a'b")),
        "printf qualification-marker > '/tmp/a'\\''b'"
    );
    assert!(denied(&event(command, 1)));
    let wrapped = r#"/bin/bash -lc "printf qualification-marker > '/tmp/fixture/tracked.txt'""#;
    assert!(denied(&event(wrapped, 1)));
    assert!(denied(&event(wrapped, 1).replace(
        "/bin/bash: line 1: /tmp/fixture/tracked.txt: Permission denied",
        "/bin/sh: 1: cannot create /tmp/fixture/tracked.txt: Read-only file system",
    )));
    assert!(denied(&event(
        &format!("/bin/sh -c '{}'", command.replace('\'', "'\\''")),
        1
    )));
    assert!(!denied(&event(wrapped, 0)));
    assert!(!denied(&event(&format!("{wrapped} extra"), 1)));
    assert!(!denied(&event(
        &wrapped.replace("/bin/bash", "/tmp/bash"),
        1
    )));
    assert!(!denied(&event(
        &wrapped.replace("tracked.txt", "other.txt"),
        1
    )));
    assert!(!denied(&event(command, 0)));
    assert!(!denied(&event("echo denied", 1)));
    assert!(!denied(&event(&format!("echo {command}"), 1)));
    assert!(!denied("refused"));
    assert!(!denied(&event(wrapped, 1).replace(
        "/bin/bash: line 1: /tmp/fixture/tracked.txt:",
        "bwrap: unshare:"
    )));
    assert!(!denied(
        &event(wrapped, 1).replace("Permission denied", "other error")
    ));
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
  printf 'Usage: claude\nOptions:\n  --bare  minimal\n  -p, --print  print\n  --max-turns <n>  limit\n  -r, --resume [value]  resume\n  --json-schema <schema>  schema\n'
  exit 0
fi
mode=plan
for arg in "$@"; do
  case "$arg" in --json-schema) mode=schema;; --resume) mode=resume;; --no-session-persistence) :;; esac
done
/bin/cat >/dev/null
case "$mode" in
  schema) case " $* " in *" --max-turns 2 "*) :;; *) exit 8;; esac
          printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","structured_output":{"answer":"OK"},"num_turns":1}';;
  resume) printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"SECOND"}';;
  *) case " $* " in *" --no-session-persistence "*) text=OK;; *) text=FIRST;; esac
     printf '{"type":"result","subtype":"success","is_error":false,"result":"%s","session_id":"12345678-1234-1234-1234-123456789abc"}\n' "$text";;
esac
"##;
    for (case, selection, expected_key, validation) in [
        ("default", None, "claude-sentinel", None),
        ("api-key", Some("api-key"), "claude-sentinel", None),
        ("subscription", Some("subscription"), "absent", None),
        (
            "hidden-turn-cap",
            Some("subscription"),
            "absent",
            Some(true),
        ),
        (
            "unproven-turn-cap",
            Some("subscription"),
            "absent",
            Some(false),
        ),
    ] {
        let case_script = match validation {
            None => script.to_owned(),
            Some(recognized) => {
                let help = script.replace(
                    r"  --max-turns <n>  limit\n",
                    r"  --background  Background a session\n               --resume <id> in the background\n",
                );
                let response = if recognized {
                    r#"printf "%s\n" "error: option '--max-turns <turns>' argument 'receipts-invalid' is invalid. must be a number" >&2; exit 1"#
                } else {
                    r#"printf 'Usage: claude\nOptions:\n  --bare  minimal\n'; exit 0"#
                };
                format!(
                    r#"if [ "$1" = --bare ]; then
  [ "$#" = 4 ] && [ "$2" = --print ] && [ "$3" = --help ] && [ "$4" = --max-turns=receipts-invalid ] || exit 9
  if read -r entry; then exit 9; fi
  {response}
fi
{help}"#
                )
            }
        };
        fs::write(
            &exe,
            format!(
                "#!/bin/sh\n[ \"${{ANTHROPIC_API_KEY-absent}}\" = '{expected_key}' ] && \
                 [ -z \"${{OPENAI_API_KEY+x}}${{CODEX_HOME+x}}\" ] && \
                 [ \"${{CLAUDE_CONFIG_DIR-absent}}\" = 'claude-config' ] || exit 9\n{case_script}"
            ),
        )
        .unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        let good = root.join(case);
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
                "--nocapture",
            ]);
        if let Some(selection) = selection {
            cmd.env(AUTH_MODE, selection);
        }
        let out = cmd.output().unwrap();
        if validation == Some(false) {
            assert!(!out.status.success());
            assert!(
                fs::read_to_string(good.join("claude-plan.summary"))
                    .unwrap()
                    .contains("verdict=PASS cleanup=true")
            );
            assert!(
                fs::read_to_string(good.join("claude-schema-turns.summary"))
                    .unwrap()
                    .contains("verdict=FAIL cleanup=true")
            );
            assert!(!good.join("claude-schema-turns.process").exists());
            assert!(!good.join("claude-resume.summary").exists());
            assert!(
                String::from_utf8_lossy(&out.stderr)
                    .contains("case=claude-schema-turns stage=start failure=POLICY_BLOCKED")
            );
            continue;
        }
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
    assert!(!claude_cases(&exe, &bad, env.clone()));
    let summary = fs::read_to_string(bad.join("claude-plan.summary")).unwrap();
    assert!(summary.contains("verdict=FAIL cleanup=true"));
    assert!(!bad.join("claude-schema-turns.summary").exists());
    assert!(!bad.join("claude-resume.summary").exists());

    let limited = root.join("rate-limit");
    fs::create_dir(&limited).unwrap();
    let rejected = r#"schema) printf '%s\n' '{"type":"rate_limit_event","rate_limit_info":{"status":"rejected"}}'; case"#;
    let limited_script = script.replacen("schema) case", rejected, 1);
    fs::write(&exe, format!("#!/bin/sh\n{limited_script}")).unwrap();
    assert!(!claude_cases(&exe, &limited, env));
    assert!(
        fs::read_to_string(limited.join("claude-plan.summary"))
            .unwrap()
            .contains("verdict=PASS cleanup=true")
    );
    assert!(
        fs::read_to_string(limited.join("claude-schema-turns.summary"))
            .unwrap()
            .contains("verdict=FAIL cleanup=true")
    );
    assert!(!limited.join("claude-resume.summary").exists());
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
