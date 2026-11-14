//! Synthetic OS-parent qualification: this test executable stands in for Codex.
//! Installed trust/enablement controls are separate live qualifications.
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Command, Stdio},
};
const BIN: &str = env!("CARGO_BIN_EXE_receipts");
fn hook(directory: &Path, event: &str, input: &[u8]) -> std::process::Output {
    let mut child = Command::new(BIN)
        .args(["hook", "codex", event])
        .env_clear()
        .env("RECEIPTS_CODEX_ENROLLMENT", directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}
#[test]
fn native_delivery_binds_parent_bytes_project_session_and_bounded_input() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("receipts-native-test-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let directory = root.join("enrollment");
    let project = root.join("repo");
    let plugin = root.join("plugin");
    for path in [&directory, &project, &plugin] {
        fs::create_dir(path).unwrap();
    }
    for file in [
        ".codex-plugin/plugin.json",
        "hooks/hooks.json",
        "bin/receipts-hook",
    ] {
        let path = plugin.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"synthetic reviewed bytes").unwrap();
    }
    let exe = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let enroll = |exe: &Path| {
        receipts_host_integration::codex::native::enroll(&directory, exe, &project, &plugin)
            .unwrap()
    };
    let input = |event: &str, session: &str, cwd: &Path| {
        format!(
            r#"{{"hook_event_name":"{event}","session_id":"{session}","cwd":"{}","tool_name":"shell_command","source":"startup","reason":"other","trigger":"manual","tool_input":{{"secret":"DISCARD_ME"}}}}"#,
            cwd.display()
        )
    };
    let spool = project.join(".receipts/inbox/host-events.jsonl");
    let start = input("SessionStart", "s", &project);
    assert!(
        !hook(&directory, "SessionStart", start.as_bytes())
            .stderr
            .is_empty()
    );
    assert!(!spool.exists());
    // Cargo can hard-link its binary artifacts on Linux; enrollment requires one link.
    let other_exe = root.join("receipts");
    fs::copy(BIN, &other_exe).unwrap();
    let alias = root.join("receipts-alias");
    fs::hard_link(&other_exe, &alias).unwrap();
    assert_eq!(
        receipts_host_integration::codex::native::enroll(&directory, &other_exe, &project, &plugin),
        Err("unsafe identity file")
    );
    fs::remove_file(alias).unwrap();
    enroll(&other_exe); // This caller is not the enrolled executable.
    assert!(
        !hook(&directory, "SessionStart", start.as_bytes())
            .stderr
            .is_empty()
    );
    assert!(!spool.exists());
    enroll(&exe);
    for (event, bytes) in [
        ("PostToolUse", input("PostToolUse", "s", &project)),
        ("SessionStart", input("SessionStart", "s", &root)),
    ] {
        assert!(!hook(&directory, event, bytes.as_bytes()).stderr.is_empty());
        assert!(!spool.exists());
    }
    for event in [
        "SessionStart",
        "PostToolUse",
        "PreCompact",
        "PostCompact",
        "SessionEnd",
    ] {
        let out = hook(&directory, event, input(event, "s", &project).as_bytes());
        assert!(out.status.success());
        assert!(out.stdout.is_empty());
        assert!(
            out.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let before = fs::read(&spool).unwrap();
    let text = String::from_utf8(before.clone()).unwrap();
    assert_eq!(text.lines().count(), 5);
    assert!(!text.contains("DISCARD_ME"));
    assert!(text.contains("OBSERVED"));
    for (event, bytes) in [
        (
            "PostToolUse",
            input("PostToolUse", "wrong-session", &project).into_bytes(),
        ),
        ("SessionStart", b"{".to_vec()),
        ("SessionStart", vec![b' '; 1_048_577]),
        ("SessionEnd", start.into_bytes()),
        (
            "WorktreeCreate",
            input("WorktreeCreate", "s", &project).into_bytes(),
        ),
    ] {
        assert!(!hook(&directory, event, &bytes).stderr.is_empty());
        assert_eq!(fs::read(&spool).unwrap(), before);
    }
    fs::write(plugin.join("hooks/hooks.json"), b"changed").unwrap();
    assert!(
        !hook(
            &directory,
            "SessionStart",
            input("SessionStart", "s", &project).as_bytes()
        )
        .stderr
        .is_empty()
    );
    assert_eq!(fs::read(&spool).unwrap(), before);
    fs::remove_dir_all(root).unwrap();
}
