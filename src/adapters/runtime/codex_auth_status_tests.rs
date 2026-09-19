//! A3-017: deterministic native helpers, never the installed provider or credentials.
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use receipts_workspace_execution::execution::{
    ExecutionError, ProcessTermination, ProcessTimeoutPolicy, STREAM_CAPTURE_LIMIT_BYTES,
};

use crate::{
    CodexAuthStatusError, FailureClass, RuntimeAuthStatus, classify_codex_auth_status_error,
    observe_codex_auth_status,
};

const MARKER: &str = "NOT-A-REAL-CREDENTIAL-A3-017";
const HELPER: &str = r#"
use std::{fs, io::{Read, Write}, path::Path, time::Duration};
unsafe extern "C" { fn signal(sig: i32, handler: usize) -> usize; fn raise(sig: i32) -> i32; }
fn main() {
    assert_eq!(std::env::args().skip(1).collect::<Vec<_>>(), ["login", "status"]);
    assert!(std::env::vars_os().next().is_none());
    assert_eq!(std::env::current_dir().unwrap(), fs::canonicalize(".").unwrap());
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).unwrap();
    assert!(input.is_empty());
    let mode = fs::read_to_string("mode").unwrap();
    if mode == "force" { unsafe { signal(15, 1); } }
    std::io::stdout().write_all(&fs::read("stdout").unwrap()).unwrap();
    std::io::stdout().flush().unwrap();
    std::io::stderr().write_all(&fs::read("stderr").unwrap()).unwrap();
    std::io::stderr().flush().unwrap();
    fs::write("pid", std::process::id().to_string()).unwrap();
    fs::write("boundary-checked", b"argv-only;empty-env;closed-stdin").unwrap();
    if mode == "signal" { unsafe { raise(15); } }
    if mode == "timeout" || mode == "force" { std::thread::sleep(Duration::from_secs(30)); }
    assert!(Path::new("exit").exists());
    std::process::exit(fs::read_to_string("exit").unwrap().parse().unwrap());
}
"#;

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Shell metacharacters remain literal path data.
        let path = std::env::temp_dir().join(format!(
            "receipts-codex-auth-a3-017-{}-{n};literal space",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        let workspace = Self(fs::canonicalize(path).unwrap());
        for (name, value) in [("stdout", ""), ("stderr", ""), ("exit", "0"), ("mode", "")] {
            workspace.write(name, value);
        }
        workspace
    }

    fn write(&self, name: &str, value: impl AsRef<[u8]>) {
        fs::write(self.0.join(name), value).unwrap();
    }

    fn observe(&self, timeout: Duration) -> Result<RuntimeAuthStatus, CodexAuthStatusError> {
        observe_codex_auth_status(helper(), &self.0, &self.0, &policy(timeout))
    }

    fn assert_boundary_and_cleanup(&self) {
        assert_eq!(
            fs::read(self.0.join("boundary-checked")).unwrap(),
            b"argv-only;empty-env;closed-stdin"
        );
        let pid = fs::read_to_string(self.0.join("pid"))
            .unwrap()
            .parse::<i32>()
            .unwrap();
        assert!(pid > 1);
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        // Signal 0 observes cleanup; these tests never signal or reap the child.
        for target in [pid, -pid] {
            assert_eq!(unsafe { kill(target, 0) }, -1);
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(3));
        }
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn helper() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let workspace = Workspace::new();
        let source = workspace.0.join("helper.rs");
        fs::write(&source, HELPER).unwrap();
        let binary = std::env::temp_dir().join(format!(
            "receipts-codex-auth-a3-017-helper-{}",
            std::process::id()
        ));
        // Compiler invocation only; production process authority stays in Workspace.
        let output = std::process::Command::new("rustc")
            .arg("--edition=2024")
            .arg(source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        binary
    })
}

fn policy(timeout: Duration) -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(timeout, Duration::from_millis(100)).unwrap()
}

#[test]
fn exit_status_alone_classifies_despite_either_diagnostic_channel() {
    for code in [0, 1, 2, 7, 42, 255] {
        for prose in [
            "logged in",
            "not logged in",
            "token expired",
            "authentication required",
            "login failed",
            "rate limited",
        ] {
            for channel in ["stdout", "stderr"] {
                let workspace = Workspace::new();
                workspace.write("exit", code.to_string());
                workspace.write(channel, format!("{prose}\n{MARKER}"));
                let status = workspace.observe(Duration::from_secs(10)).unwrap();
                assert_eq!(
                    status,
                    if code == 0 {
                        RuntimeAuthStatus::Connected
                    } else {
                        RuntimeAuthStatus::Unknown
                    },
                    "exit={code}, channel={channel}, prose={prose}"
                );
                assert!(!format!("{status:?}").contains(MARKER));
                workspace.assert_boundary_and_cleanup();
            }
        }
    }
}

#[test]
fn truncated_and_non_utf8_streams_do_not_supply_auth_truth() {
    for code in [0, 1] {
        let workspace = Workspace::new();
        workspace.write("exit", code.to_string());
        let output = vec![0xff; STREAM_CAPTURE_LIMIT_BYTES as usize + 1];
        workspace.write("stdout", &output);
        workspace.write("stderr", &output);
        assert_eq!(
            workspace.observe(Duration::from_secs(10)).unwrap(),
            if code == 0 {
                RuntimeAuthStatus::Connected
            } else {
                RuntimeAuthStatus::Unknown
            }
        );
        workspace.assert_boundary_and_cleanup();
    }
}

#[test]
fn missing_exit_status_remains_unknown() {
    let workspace = Workspace::new();
    workspace.write("mode", "signal");
    workspace.write("stdout", "logged in");
    assert_eq!(
        workspace.observe(Duration::from_secs(10)).unwrap(),
        RuntimeAuthStatus::Unknown
    );
    workspace.assert_boundary_and_cleanup();
}

#[test]
fn workspace_timeout_preserves_graceful_and_forced_evidence() {
    for (mode, expected) in [
        ("timeout", ProcessTermination::TimedOutGracefullyTerminated),
        ("force", ProcessTermination::TimedOutForceKilled),
    ] {
        let workspace = Workspace::new();
        workspace.write("mode", mode);
        workspace.write("stdout", format!("logged in {MARKER}"));
        workspace.write("stderr", "token expired");
        let error = workspace.observe(Duration::from_secs(1)).unwrap_err();
        assert!(matches!(error, CodexAuthStatusError::TimedOut(value) if value == expected));
        assert_eq!(
            classify_codex_auth_status_error(&error),
            FailureClass::Timeout
        );
        assert!(!format!("{error:?} {error}").contains(MARKER));
        assert!(error.source().is_none());
        workspace.assert_boundary_and_cleanup();
    }
    assert_eq!(
        classify_codex_auth_status_error(&CodexAuthStatusError::TimedOut(
            ProcessTermination::Completed
        )),
        FailureClass::Unknown
    );
}

#[test]
fn absolute_paths_shell_rejection_and_workspace_containment_remain_enforced() {
    let workspace = Workspace::new();
    let timeout = policy(Duration::from_secs(10));
    let cases = [
        (
            Path::new("codex"),
            workspace.0.as_path(),
            workspace.0.as_path(),
        ),
        (
            Path::new("/bin/sh"),
            workspace.0.as_path(),
            workspace.0.as_path(),
        ),
        (helper(), Path::new("relative"), workspace.0.as_path()),
        (helper(), workspace.0.as_path(), Path::new("relative")),
        (
            helper(),
            workspace.0.as_path(),
            workspace.0.parent().unwrap(),
        ),
    ];
    for (index, (executable, root, cwd)) in cases.into_iter().enumerate() {
        let error = observe_codex_auth_status(executable, root, cwd, &timeout).unwrap_err();
        assert_eq!(
            classify_codex_auth_status_error(&error),
            FailureClass::Unknown
        );
        assert!(matches!(
            (index, error),
            (
                0,
                CodexAuthStatusError::Workspace(ExecutionError::ExecutablePathNotAbsolute { .. })
            ) | (
                1,
                CodexAuthStatusError::Workspace(ExecutionError::ShellExecutableRejected { .. })
            ) | (
                2,
                CodexAuthStatusError::Workspace(ExecutionError::WorkspaceRootNotAbsolute { .. })
            ) | (
                3,
                CodexAuthStatusError::Workspace(ExecutionError::CwdNotAbsolute { .. })
            ) | (
                4,
                CodexAuthStatusError::Workspace(ExecutionError::CwdOutsideWorkspace { .. })
            )
        ));
    }
    assert!(!workspace.0.join("boundary-checked").exists());
}

#[test]
fn workspace_errors_preserve_type_without_formatting_sensitive_details() {
    let workspace = Workspace::new();
    let error = observe_codex_auth_status(
        &workspace.0.join(MARKER),
        &workspace.0,
        &workspace.0,
        &policy(Duration::from_secs(10)),
    )
    .unwrap_err();
    assert!(matches!(
        error,
        CodexAuthStatusError::Workspace(ExecutionError::ExecutableNotFound { .. })
    ));
    assert_eq!(
        classify_codex_auth_status_error(&error),
        FailureClass::Unknown
    );
    assert!(!format!("{error:?} {error}").contains(MARKER));
    assert!(error.source().is_none());
}

#[test]
fn observation_has_no_credential_io_or_independent_process_authority() {
    let source = include_str!("codex_auth_status.rs");
    for forbidden in [
        "std::fs",
        "fs::",
        "File::",
        "std::env",
        "env::",
        "Command",
        "std::process",
        "auth.json",
        "config.toml",
        "keychain",
        "access_token",
        "refresh_token",
        "id_token",
        "OPENAI_API_KEY",
        "CODEX_HOME",
        ".stdout()",
        ".stderr()",
        "serde_json",
    ] {
        assert!(
            !source.contains(forbidden),
            "unexpected authority: {forbidden}"
        );
    }
}
