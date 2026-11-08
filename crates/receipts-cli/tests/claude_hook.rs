//! Owned synthetic controls for the hook-only binary: exact stdout/stderr/
//! exit, spool contents, WorktreeCreate responses and refusals, capability
//! cache consumption, launcher argv/stdin handling and the no-State control.
//! Every process runs with a cleared environment in a private temp tree; no
//! user settings, credentials or installed host are touched.

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use receipts_host_integration::wire::{
    host_capability_report_from_json, normalized_host_event_from_json,
};
use receipts_host_integration::{
    HostCapabilityInactiveReason, HostCapabilityProbeStatus, HostCapabilitySelectedMode,
    NormalizedHostEventConfidence, NormalizedHostEventType,
};

const BIN: &str = env!("CARGO_BIN_EXE_receipts");

struct Tree(PathBuf);
impl Tree {
    fn new(tag: &str) -> Self {
        let base = std::fs::canonicalize(std::env::temp_dir()).unwrap();
        let path = base.join(format!("receipts-hook-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn join(&self, p: &str) -> PathBuf {
        self.0.join(p)
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Run {
    stdout: String,
    stderr: String,
    code: i32,
    elapsed: Duration,
}

fn run(program: &Path, args: &[&str], env: &[(&str, &Path)], stdin: &[u8]) -> Run {
    let mut command = Command::new(program);
    command
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        command.env(k, v);
    }
    let start = Instant::now();
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    let out = child.wait_with_output().unwrap();
    Run {
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
        code: out.status.code().unwrap(),
        elapsed: start.elapsed(),
    }
}

fn hook(event: &str, env: &[(&str, &Path)], stdin: &[u8]) -> Run {
    run(Path::new(BIN), &["hook", "claude", event], env, stdin)
}

fn payload(event: &str, cwd: &Path, extra: &str) -> Vec<u8> {
    format!(
        r#"{{"session_id":"sess-1","transcript_path":"/x/t.jsonl","cwd":"{}","hook_event_name":"{event}"{extra}}}"#,
        cwd.display()
    )
    .into_bytes()
}

fn spool_events(project: &Path) -> Vec<receipts_host_integration::NormalizedHostEvent> {
    std::fs::read_to_string(project.join(".receipts/inbox/host-events.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|l| normalized_host_event_from_json(l).unwrap())
        .collect()
}

/// Every file below `dir`, relative.
fn files(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if std::fs::symlink_metadata(&p).unwrap().is_dir() {
                stack.push(p);
            } else {
                out.push(p.strip_prefix(dir).unwrap().display().to_string());
            }
        }
    }
    out.sort();
    out
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("/usr/bin/git")
        .args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn repo(tree: &Tree) -> PathBuf {
    let repo = tree.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("a.txt"), "a\n").unwrap();
    git(&repo, &["add", "a.txt"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@e",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    );
    repo
}

#[test]
fn observer_hooks_spool_exact_events_fail_open_and_never_touch_state() {
    let tree = Tree::new("observer");
    let project = tree.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let env = [("CLAUDE_PROJECT_DIR", project.as_path())];
    let cases = [
        (
            "SessionStart",
            r#","source":"startup""#,
            NormalizedHostEventType::HostSessionStarted,
        ),
        (
            "PostToolUse",
            r#","tool_name":"Edit","tool_input":{"x":"SECRET"}"#,
            NormalizedHostEventType::ToolExecuted,
        ),
        (
            "PreCompact",
            r#","trigger":"auto""#,
            NormalizedHostEventType::ContextCompacted,
        ),
        (
            "PostCompact",
            r#","trigger":"auto""#,
            NormalizedHostEventType::ContextCompacted,
        ),
        (
            "SessionEnd",
            r#","reason":"other""#,
            NormalizedHostEventType::HostSessionEnding,
        ),
    ];
    for (event, extra, _) in cases {
        let out = hook(event, &env, &payload(event, &project, extra));
        assert_eq!(
            (out.code, out.stdout.as_str()),
            (0, ""),
            "{event}: {}",
            out.stderr
        );
    }
    let events = spool_events(&project);
    let types: Vec<_> = events.iter().map(|e| e.event_type()).collect();
    assert_eq!(types, cases.map(|c| c.2).to_vec());
    for e in &events {
        assert_eq!(e.confidence(), NormalizedHostEventConfidence::Observed);
        assert_eq!(e.host_session_id(), "sess-1");
    }
    let mut ids: Vec<_> = events
        .iter()
        .map(|e| e.event_id().as_str().to_owned())
        .collect();
    ids.dedup();
    assert_eq!(ids.len(), 5, "unique event ids");
    let raw = std::fs::read_to_string(project.join(".receipts/inbox/host-events.jsonl")).unwrap();
    assert!(!raw.contains("SECRET"), "tool input never spooled");
    // SessionStart without probe configuration says so, exactly once.
    // Malformed, oversized and mismatched input: exit 0, nothing spooled.
    for stdin in [
        b"{".to_vec(),
        payload("SessionEnd", &project, ""),
        vec![b' '; 1_048_577],
    ] {
        let out = hook("SessionStart", &env, &stdin);
        assert_eq!(out.code, 0);
        assert_eq!(out.stdout, "");
        assert!(
            out.stderr.starts_with("receipts: invalid hook input"),
            "{}",
            out.stderr
        );
        assert_eq!(out.stderr.lines().count(), 1);
    }
    // An unregistered or unknown hook name fails open too.
    let out = hook("Stop", &env, &payload("Stop", &project, ""));
    assert_eq!((out.code, out.stdout.as_str()), (0, ""));
    assert_eq!(spool_events(&project).len(), 5);
    // hook_never_opens_state_db: the only file the bridge ever wrote under
    // the project is the spool; no database, journal or lock file exists.
    assert_eq!(files(&project), vec![".receipts/inbox/host-events.jsonl"]);
    // And no Host-owned source names the State crate or SQLite.
    for dir in ["src/hosts", "crates/receipts-cli/src"] {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(dir);
        for f in files(&root) {
            let text = std::fs::read_to_string(root.join(&f)).unwrap();
            for needle in ["receipts_state", "rusqlite", "TrustedCoreStore"] {
                assert!(!text.contains(needle), "{dir}/{f} mentions {needle}");
            }
        }
    }
}

#[test]
fn missing_project_dir_and_bad_usage_are_explicit() {
    let tree = Tree::new("usage");
    let out = hook("SessionEnd", &[], &payload("SessionEnd", &tree.0, ""));
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout, "");
    assert_eq!(
        out.stderr,
        "receipts: no absolute CLAUDE_PROJECT_DIR; event not spooled\n"
    );
    for args in [
        &["hook"][..],
        &["hook", "codex", "SessionStart"],
        &["status"],
    ] {
        let out = run(Path::new(BIN), args, &[], b"");
        assert_eq!(out.code, 1);
        assert_eq!(out.stderr, "usage: receipts hook claude <Event>\n");
    }
}

#[test]
fn worktree_create_response_format_exact_and_p95_latency_under_budget() {
    let tree = Tree::new("wt");
    let repo = repo(&tree);
    let head = git(&repo, &["rev-parse", "HEAD"]).trim().to_owned();
    let env = [("CLAUDE_PROJECT_DIR", repo.as_path())];
    let sub = repo.join("nested");
    std::fs::create_dir_all(&sub).unwrap();
    let mut latencies = Vec::new();
    for n in 0..10 {
        let name = format!("feat-{n}");
        // cwd may be a subdirectory; the toplevel is found from Git's files.
        let out = hook(
            "WorktreeCreate",
            &env,
            &payload("WorktreeCreate", &sub, &format!(r#","name":"{name}""#)),
        );
        let expected = repo.join(".claude/worktrees").join(&name);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(out.stdout, format!("{}\n", expected.display()));
        assert_eq!(out.stderr, "");
        assert_eq!(git(&expected, &["rev-parse", "HEAD"]).trim(), head);
        assert_eq!(
            git(&expected, &["branch", "--show-current"]).trim(),
            format!("worktree-{name}")
        );
        latencies.push(out.elapsed);
    }
    latencies.sort();
    let p95 = latencies[9];
    // Budget: 5s against Claude's 60s hook timeout on an interactive path.
    assert!(p95 < Duration::from_secs(5), "p95 {p95:?}");
    eprintln!("WorktreeCreate latency p50={:?} p95={p95:?}", latencies[5]);
    // Idempotent re-request attaches to the identical worktree.
    let again = hook(
        "WorktreeCreate",
        &env,
        &payload("WorktreeCreate", &repo, r#","name":"feat-0""#),
    );
    assert_eq!(again.code, 0, "{}", again.stderr);
    assert_eq!(
        again.stdout,
        format!("{}\n", repo.join(".claude/worktrees/feat-0").display())
    );
    let created: Vec<_> = spool_events(&repo)
        .into_iter()
        .filter(|e| e.event_type() == NormalizedHostEventType::WorkspaceCreated)
        .collect();
    assert_eq!(created.len(), 11);
    // Plain no-task fallback: core did not drive it, so INFERRED.
    assert!(
        created
            .iter()
            .all(|e| e.confidence() == NormalizedHostEventConfidence::Inferred)
    );
}

#[test]
fn worktree_refusals_are_preserved_and_never_fake_success() {
    let tree = Tree::new("wt-refuse");
    let repo = repo(&tree);
    let env = [("CLAUDE_PROJECT_DIR", repo.as_path())];
    // Existing user data at the target is never adopted or overwritten.
    let target = repo.join(".claude/worktrees/taken");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep.txt"), "mine").unwrap();
    let out = hook(
        "WorktreeCreate",
        &env,
        &payload("WorktreeCreate", &repo, r#","name":"taken""#),
    );
    assert_eq!(out.code, 1);
    assert_eq!(out.stdout, "");
    assert!(
        out.stderr.starts_with("receipts: WorktreeCreate refused: "),
        "{}",
        out.stderr
    );
    assert_eq!(
        std::fs::read_to_string(target.join("keep.txt")).unwrap(),
        "mine"
    );
    // The branch already checked out elsewhere: refused, not reattached.
    git(&repo, &["branch", "worktree-other"]);
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            tree.join("elsewhere").to_str().unwrap(),
            "worktree-other",
        ],
    );
    let out = hook(
        "WorktreeCreate",
        &env,
        &payload("WorktreeCreate", &repo, r#","name":"other""#),
    );
    assert_eq!((out.code, out.stdout.as_str()), (1, ""));
    // Not a repository, invalid names, malformed input: exit 1, no stdout.
    let plain = tree.join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    for (cwd, extra) in [
        (plain.as_path(), r#","name":"x""#),
        (repo.as_path(), r#","name":"../escape""#),
        (repo.as_path(), r#""#),
    ] {
        let out = hook(
            "WorktreeCreate",
            &env,
            &payload("WorktreeCreate", cwd, extra),
        );
        assert_eq!((out.code, out.stdout.as_str()), (1, ""), "{}", out.stderr);
        assert!(!out.stderr.is_empty());
    }
    assert!(!tree.join("escape").exists());
    let created = spool_events(&repo)
        .into_iter()
        .filter(|e| e.event_type() == NormalizedHostEventType::WorkspaceCreated)
        .count();
    assert_eq!(created, 0, "no event for refused creation");
}

#[test]
fn worktree_remove_preserves_the_tree_and_names_it() {
    let tree = Tree::new("wt-remove");
    let repo = repo(&tree);
    let env = [("CLAUDE_PROJECT_DIR", repo.as_path())];
    let out = hook(
        "WorktreeCreate",
        &env,
        &payload("WorktreeCreate", &repo, r#","name":"gone""#),
    );
    assert_eq!(out.code, 0);
    let path = repo.join(".claude/worktrees/gone");
    std::fs::write(path.join("work.txt"), "unsaved").unwrap();
    let out = hook(
        "WorktreeRemove",
        &env,
        &payload(
            "WorktreeRemove",
            &repo,
            &format!(r#","worktree_path":"{}""#, path.display()),
        ),
    );
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout, "");
    assert_eq!(
        out.stderr,
        format!(
            "receipts: worktree kept at {}; remove it with `git worktree remove` when finished\n",
            path.display()
        )
    );
    assert_eq!(
        std::fs::read_to_string(path.join("work.txt")).unwrap(),
        "unsaved"
    );
    assert!(git(&repo, &["worktree", "list"]).contains(".claude/worktrees/gone"));
    assert!(
        spool_events(&repo)
            .iter()
            .all(|e| e.event_type() != NormalizedHostEventType::WorkspaceRemoved)
    );
}

fn fake_claude(path: &Path, install_path: &Path, comment: &str) {
    std::fs::write(
        path,
        format!(
            "#!/bin/sh\n# {comment}\nif [ \"$1\" = --version ]; then echo '2.1.319 (Claude Code)'; else echo '[{{\"id\":\"receipts@local\",\"enabled\":true,\"version\":\"0.1.0\",\"installPath\":\"{}\"}}]'; fi\n",
            install_path.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn session_start_records_and_reuses_a_coupled_capability_report() {
    if Path::new("/Library/Application Support/ClaudeCode").exists()
        || Path::new("/etc/claude-code").exists()
    {
        eprintln!("SKIPPED: host-managed Claude settings present; outcome not controlled");
        return;
    }
    let tree = Tree::new("caps");
    let project = tree.join("project");
    let plugin = tree.join("plugin");
    let data = tree.join("data");
    let config = tree.join("config");
    for d in [&project, &plugin.join("hooks"), &config] {
        std::fs::create_dir_all(d).unwrap();
    }
    let manifest =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/claude/hooks/hooks.json");
    std::fs::copy(&manifest, plugin.join("hooks/hooks.json")).unwrap();
    let exe = tree.join("claude");
    fake_claude(&exe, &plugin, "v1");
    let env = |root: &Path| -> Vec<(&'static str, PathBuf)> {
        vec![
            ("HOME", tree.0.clone()),
            ("CLAUDE_CONFIG_DIR", config.clone()),
            ("CLAUDE_PROJECT_DIR", project.clone()),
            ("CLAUDE_PLUGIN_ROOT", root.to_path_buf()),
            ("CLAUDE_PLUGIN_DATA", data.clone()),
            ("RECEIPTS_CLAUDE_EXECUTABLE", exe.clone()),
        ]
    };
    let start = |root: &Path| {
        let env = env(root);
        let env: Vec<(&str, &Path)> = env.iter().map(|(k, v)| (*k, v.as_path())).collect();
        hook(
            "SessionStart",
            &env,
            &payload("SessionStart", &project, r#","source":"startup""#),
        )
    };
    let report_path = data.join("host-capability-report.json");
    let read = || {
        host_capability_report_from_json(&std::fs::read_to_string(&report_path).unwrap()).unwrap()
    };

    // 1. Our hook runs from the installed plugin root: EMBEDDED, COMPLETE.
    let out = start(&plugin);
    assert_eq!(
        (out.code, out.stdout.as_str(), out.stderr.as_str()),
        (0, "", "")
    );
    let first = read();
    let core = first.core();
    assert_eq!(core.selected_mode(), HostCapabilitySelectedMode::Embedded);
    assert_eq!(core.probe_status(), HostCapabilityProbeStatus::Complete);
    assert_eq!(
        core.inactive_reason(),
        Some(HostCapabilityInactiveReason::None)
    );
    assert_eq!(core.host_version(), Some("2.1.319"));
    assert!(core.validity_fingerprint().unwrap().starts_with("sha256:"));
    assert!(
        core.hook_definition_digest()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(
        std::fs::metadata(&report_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    // 2. Unchanged: reused, original probed_at kept, last_verified_at advances.
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(start(&plugin).code, 0);
    let second = read();
    assert_eq!(second.core(), first.core());
    assert_eq!(second.probed_at(), first.probed_at());
    assert_ne!(second.last_verified_at(), first.last_verified_at());

    // 3. Plugin hook definition drift: new token, re-probed report.
    let mut text = std::fs::read_to_string(plugin.join("hooks/hooks.json")).unwrap();
    text.push(' ');
    std::fs::write(plugin.join("hooks/hooks.json"), &text).unwrap();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(start(&plugin).code, 0);
    let drifted = read();
    assert_ne!(
        drifted.core().validity_fingerprint(),
        first.core().validity_fingerprint()
    );
    assert_ne!(
        drifted.core().hook_definition_digest(),
        first.core().hook_definition_digest()
    );
    assert_ne!(drifted.probed_at(), first.probed_at());

    // 4. A hook launched from another plugin root proves nothing: not EMBEDDED.
    let foreign = tree.join("foreign-plugin");
    std::fs::create_dir_all(&foreign).unwrap();
    let out = start(&foreign);
    assert_eq!(out.code, 0);
    // Enablement/admin stay unknown: PARTIAL, conservative SUPERVISED/UNKNOWN.
    assert_eq!(out.stderr, "");
    let unproven = read();
    assert_eq!(
        unproven.core().probe_status(),
        HostCapabilityProbeStatus::Partial
    );
    assert_eq!(
        unproven.core().selected_mode(),
        HostCapabilitySelectedMode::Supervised
    );
    assert_eq!(
        unproven.core().inactive_reason(),
        Some(HostCapabilityInactiveReason::Unknown)
    );
    assert_eq!(unproven.core().hooks_enabled(), None);

    // 5. A forged cached report claiming EMBEDDED under a fake token is not
    //    reused: the fresh observation replaces it.
    assert_eq!(start(&plugin).code, 0);
    let honest = read();
    let forged = std::fs::read_to_string(&report_path).unwrap().replace(
        honest.core().validity_fingerprint().unwrap(),
        "sha256:forged",
    );
    std::fs::write(&report_path, forged).unwrap();
    assert_eq!(start(&plugin).code, 0);
    let replaced = read();
    assert_eq!(
        replaced.core().validity_fingerprint(),
        honest.core().validity_fingerprint()
    );
    assert_ne!(
        replaced.probed_at(),
        honest.probed_at(),
        "forged report was not reused"
    );

    // 6. Executable drift after admission is a fresh runner per hook process:
    //    a changed claude yields a new fingerprint, never the old report.
    fake_claude(&exe, &plugin, "v2");
    assert_eq!(start(&plugin).code, 0);
    assert_ne!(
        read().core().validity_fingerprint(),
        honest.core().validity_fingerprint()
    );

    // 7. Relative or missing executable: no probe, explicit note, no write.
    std::fs::remove_file(&report_path).unwrap();
    std::fs::remove_file(&exe).unwrap();
    let out = start(&plugin);
    assert_eq!(out.code, 0);
    assert!(
        out.stderr
            .starts_with("receipts: claude executable not admitted"),
        "{}",
        out.stderr
    );
    assert!(!report_path.exists());
    // The SessionStart event itself is still spooled every time.
    assert_eq!(
        spool_events(&project)
            .iter()
            .filter(|e| e.event_type() == NormalizedHostEventType::HostSessionStarted)
            .count(),
        8
    );
}

#[test]
fn launcher_passes_input_via_stdin_not_argv_interpolation() {
    let tree = Tree::new("launcher");
    let launcher =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/claude/bin/receipts-hook");
    let recorder = tree.join("recorder");
    let log = tree.join("log");
    std::fs::write(
        &recorder,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{0}.argv'\ncat > '{0}.stdin'\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&recorder, std::fs::Permissions::from_mode(0o700)).unwrap();
    let marker = tree.join("pwned");
    let hostile = format!(
        r#"{{"session_id":"$(touch {0})","cwd":"`touch {0}`","hook_event_name":"SessionStart"}}"#,
        marker.display()
    );
    let out = run(
        Path::new("/bin/sh"),
        &[launcher.to_str().unwrap(), "SessionStart"],
        &[("RECEIPTS_BIN", recorder.as_path())],
        hostile.as_bytes(),
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        std::fs::read_to_string(tree.join("log.argv")).unwrap(),
        "hook\nclaude\nSessionStart\n"
    );
    assert_eq!(
        std::fs::read_to_string(tree.join("log.stdin")).unwrap(),
        hostile
    );
    assert!(!marker.exists());
    // Bundled binary via CLAUDE_PLUGIN_ROOT when RECEIPTS_BIN is not absolute.
    let plugin = tree.join("plugin");
    std::fs::create_dir_all(plugin.join("bin")).unwrap();
    std::fs::copy(&recorder, plugin.join("bin/receipts")).unwrap();
    let out = run(
        Path::new("/bin/sh"),
        &[launcher.to_str().unwrap(), "PostToolUse"],
        &[
            ("RECEIPTS_BIN", Path::new("relative")),
            ("CLAUDE_PLUGIN_ROOT", plugin.as_path()),
        ],
        b"{}",
    );
    assert_eq!(out.code, 0);
    assert_eq!(
        std::fs::read_to_string(tree.join("log.argv")).unwrap(),
        "hook\nclaude\nPostToolUse\n"
    );
    // No absolute binary: observers fail open, WorktreeCreate refuses.
    for (event, code) in [("SessionStart", 0), ("WorktreeCreate", 1)] {
        let out = run(
            Path::new("/bin/sh"),
            &[launcher.to_str().unwrap(), event],
            &[],
            b"{}",
        );
        assert_eq!(out.code, code, "{event}");
        assert_eq!(out.stdout, "");
        assert!(
            out.stderr
                .starts_with("receipts: no absolute receipts binary")
        );
    }
}

#[test]
fn plugin_manifest_validates_against_packaging_doc_fields() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/claude");
    let manifest = std::fs::read_to_string(root.join(".claude-plugin/plugin.json")).unwrap();
    for field in [
        r#""name": "receipts""#,
        r#""version": "0.1.0""#,
        r#""description""#,
        r#""license": "Apache-2.0""#,
    ] {
        assert!(manifest.contains(field), "{field}");
    }
    // hooks/hooks.json is auto-discovered; referencing it again would load it twice.
    assert!(!manifest.contains(r#""hooks":"#));
    // Packaging layout: hooks, commands, skills, bin; no agents/ or .mcp.json.
    for p in [
        "hooks/hooks.json",
        "commands/capabilities.md",
        "skills/receipts/SKILL.md",
        "bin/receipts-hook",
    ] {
        assert!(root.join(p).is_file(), "{p}");
    }
    assert!(!root.join("agents").exists() && !root.join(".mcp.json").exists());
    let mode = std::fs::metadata(root.join("bin/receipts-hook"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "launcher is executable");
    let skill = std::fs::read_to_string(root.join("skills/receipts/SKILL.md")).unwrap();
    assert!(skill.starts_with("---\nname: receipts\ndescription: "));
    let command = std::fs::read_to_string(root.join("commands/capabilities.md")).unwrap();
    assert!(command.starts_with("---\ndescription: "));
}
