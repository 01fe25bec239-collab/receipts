use crate::{
    claude_probe_execution::{execute_with_runner, probe_request},
    claude_probe_tests::{HELP, VERSION},
    *,
};
use receipts_workspace_execution::execution::{
    ExecutionError, ProcessRunRequest, ProcessStdin, ProcessTermination, ProcessTimeoutPolicy,
    STREAM_CAPTURE_LIMIT_BYTES, run_with_timeout_and_capture,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

const MARKER: &str = "synthetic-provider-payload-not-a-credential";
const HELPER: &str = r#"
use std::{fs, io::{Read, Write}, path::Path, time::Duration};
fn main() {
    assert!(std::env::vars_os().next().is_none());
    let mut stdin = Vec::new();
    std::io::stdin().read_to_end(&mut stdin).unwrap();
    assert!(stdin.is_empty());
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert!(args == ["--version"] || args == ["--help"]);
    let kind = if args[0] == "--version" { "version" } else { "help" };
    if Path::new("force").exists() {
        unsafe extern "C" { fn signal(sig: i32, handler: usize) -> usize; }
        unsafe { signal(15, 1); }
    }
    fs::write(format!("{kind}-observed"), b"empty environment; closed stdin; exact argv; signal ready").unwrap();
    if Path::new("timeout").exists() { std::thread::sleep(Duration::from_secs(30)); }
    std::io::stdout().write_all(&fs::read(format!("{kind}-stdout")).unwrap()).unwrap();
    std::io::stderr().write_all(&fs::read(format!("{kind}-stderr")).unwrap()).unwrap();
    std::process::exit(fs::read_to_string(format!("{kind}-exit")).unwrap().parse().unwrap());
}
"#;
struct Fixture {
    root: PathBuf,
    cwd: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "receipts-claude-probe-019-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let cwd = root.join("cwd");
        fs::create_dir_all(&cwd).unwrap();
        let fixture = Self { root, cwd };
        for (kind, output) in [("version", VERSION), ("help", HELP)] {
            fixture.write(&format!("{kind}-stdout"), output);
            fixture.write(&format!("{kind}-stderr"), b"");
            fixture.write(&format!("{kind}-exit"), b"0");
        }
        fixture
    }
    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.cwd.join(name), bytes).unwrap();
    }
    fn run(&self) -> Result<ClaudeCapabilityProbeReport, ClaudeProbeExecutionError> {
        execute_claude_capability_probe(helper(), &self.root, &self.cwd, &policy())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn policy() -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(Duration::from_secs(10), Duration::from_secs(1)).unwrap()
}
fn helper() -> &'static Path {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        let root = std::env::temp_dir().join(format!(
            "receipts-claude-probe-019-helper-{}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("helper.rs");
        let binary = root.join("helper");
        fs::write(&source, HELPER).unwrap();
        // Test fixture compilation only; production spawning belongs to Workspace.
        let output = std::process::Command::new("rustc")
            .arg(&source)
            .args(["--edition", "2024", "-o"])
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        binary
    })
}

#[test]
fn exact_argv_absolute_executable_root_cwd_policy_and_closed_stdin_reach_workspace() {
    let fixture = Fixture::new();
    let policy = policy();
    let mut calls = Vec::new();
    let report = execute_with_runner(
        helper(),
        &fixture.root,
        &fixture.cwd,
        &policy,
        |probe, request, seen| {
            calls.push(probe);
            assert_eq!(request.executable(), helper());
            assert_eq!(request.workspace_root(), fixture.root);
            assert_eq!(request.cwd(), fixture.cwd);
            assert_eq!(request.stdin(), &ProcessStdin::Closed);
            assert!(std::ptr::eq(seen, &policy));
            let expected = match probe {
                ClaudeProbeKind::Version => "--version",
                ClaudeProbeKind::Help => "--help",
            };
            assert_eq!(request.arguments(), &[std::ffi::OsString::from(expected)]);
            run_with_timeout_and_capture(request, seen)
                .map_err(|_| ClaudeProbeExecutionError::WorkspaceExecution { probe })
        },
    )
    .unwrap();
    assert_eq!(calls, [ClaudeProbeKind::Version, ClaudeProbeKind::Help]);
    assert_eq!(report.version, "2.1.246 (Claude Code)");
    assert_eq!(report.verbose, ClaudeCapabilityEvidence::Supported);
    for kind in ["version", "help"] {
        assert!(fixture.cwd.join(format!("{kind}-observed")).exists());
    }
}

#[test]
fn production_runner_observes_empty_environment_and_closed_stdin() {
    let fixture = Fixture::new();
    assert_eq!(
        fixture.run().unwrap().print,
        ClaudeCapabilityEvidence::Supported
    );
    assert!(fixture.cwd.join("version-observed").exists());
    assert!(fixture.cwd.join("help-observed").exists());
}

#[test]
fn relative_executable_fails_at_workspace_validation() {
    let fixture = Fixture::new();
    let relative = Path::new("relative/claude");
    assert!(matches!(
        ProcessRunRequest::new(relative, ["--version"], &fixture.root, &fixture.cwd),
        Err(ExecutionError::ExecutablePathNotAbsolute { .. })
    ));
    let error = probe_request(
        relative,
        &fixture.root,
        &fixture.cwd,
        ClaudeProbeKind::Version,
    )
    .unwrap_err();
    assert_eq!(
        error,
        ClaudeProbeExecutionError::WorkspaceExecution {
            probe: ClaudeProbeKind::Version
        }
    );
    assert_eq!(
        classify_claude_probe_execution_error(&error),
        FailureClass::Unknown
    );
}

#[test]
fn real_workspace_execution_failure_is_bounded_unknown() {
    let fixture = Fixture::new();
    let error = execute_claude_capability_probe(
        &fixture.root.join("missing"),
        &fixture.root,
        &fixture.cwd,
        &policy(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        ClaudeProbeExecutionError::WorkspaceExecution {
            probe: ClaudeProbeKind::Version
        }
    );
    assert_eq!(
        classify_claude_probe_execution_error(&error),
        FailureClass::Unknown
    );
}

#[test]
fn real_graceful_and_forced_timeouts_preserve_workspace_termination() {
    let _ = helper();
    for (force, expected) in [
        (false, ProcessTermination::TimedOutGracefullyTerminated),
        (true, ProcessTermination::TimedOutForceKilled),
    ] {
        let fixture = Fixture::new();
        fixture.write("timeout", b"");
        if force {
            fixture.write("force", b"");
        }
        // Allow finite cold-start scheduling time before exercising termination.
        let policy =
            ProcessTimeoutPolicy::new(Duration::from_secs(3), Duration::from_millis(100)).unwrap();
        let error = execute_claude_capability_probe(helper(), &fixture.root, &fixture.cwd, &policy)
            .unwrap_err();
        assert_eq!(
            error,
            ClaudeProbeExecutionError::TimedOut {
                probe: ClaudeProbeKind::Version,
                termination: expected
            }
        );
        assert_eq!(
            classify_claude_probe_execution_error(&error),
            FailureClass::Timeout
        );
        assert_eq!(
            fs::read(fixture.cwd.join("version-observed")).unwrap(),
            b"empty environment; closed stdin; exact argv; signal ready"
        );
        assert!(!fixture.cwd.join("help-observed").exists());
    }
}

#[test]
fn timeout_shaped_completed_error_is_unknown() {
    let error = ClaudeProbeExecutionError::TimedOut {
        probe: ClaudeProbeKind::Help,
        termination: ProcessTermination::Completed,
    };
    assert_eq!(
        classify_claude_probe_execution_error(&error),
        FailureClass::Unknown
    );
}

#[test]
fn real_truncation_on_either_channel_of_either_probe_is_bounded_unknown() {
    let payload = MARKER.repeat(STREAM_CAPTURE_LIMIT_BYTES as usize / MARKER.len() + 1);
    for (kind, probe) in [
        ("version", ClaudeProbeKind::Version),
        ("help", ClaudeProbeKind::Help),
    ] {
        for (stream, channel) in [
            ("stdout", ClaudeProbeChannel::Stdout),
            ("stderr", ClaudeProbeChannel::Stderr),
        ] {
            let fixture = Fixture::new();
            fixture.write(&format!("{kind}-{stream}"), &payload);
            let error = fixture.run().unwrap_err();
            assert_eq!(
                error,
                ClaudeProbeExecutionError::TruncatedStream { probe, channel }
            );
            assert_eq!(
                classify_claude_probe_execution_error(&error),
                FailureClass::Unknown
            );
            assert!(!format!("{error:?} {error}").contains(MARKER));
        }
    }
}

#[test]
fn real_parser_failures_do_not_retain_provider_output() {
    for (kind, probe) in [
        ("version", ClaudeProbeKind::Version),
        ("help", ClaudeProbeKind::Help),
    ] {
        let fixture = Fixture::new();
        fixture.write(&format!("{kind}-stdout"), MARKER);
        fixture.write(&format!("{kind}-stderr"), MARKER);
        fixture.write(&format!("{kind}-exit"), b"7");
        let error = fixture.run().unwrap_err();
        assert_eq!(
            error,
            ClaudeProbeExecutionError::Parse(ClaudeProbeError::NonSuccessStatus(probe, 7))
        );
        assert_eq!(
            classify_claude_probe_execution_error(&error),
            FailureClass::Unknown
        );
        assert!(!format!("{error:?} {error}").contains(MARKER));
    }
}

#[test]
fn complete_head_and_tail_capture_is_reconstructed() {
    let fixture = Fixture::new();
    let mut help = HELP.to_vec();
    help.extend(vec![b' '; 600_000]);
    fixture.write("help-stdout", help);
    assert_eq!(
        fixture.run().unwrap().verbose,
        ClaudeCapabilityEvidence::Supported
    );
}

#[test]
#[ignore = "requires explicit installed Claude path; runs only --version and --help"]
fn installed_claude_probe_through_workspace() {
    let path = PathBuf::from(
        std::env::var_os("RECEIPTS_TEST_CLAUDE_BIN")
            .expect("explicit absolute Claude executable required"),
    );
    let fixture = Fixture::new();
    let report =
        execute_claude_capability_probe(&path, &fixture.root, &fixture.cwd, &policy()).unwrap();
    assert!(!report.version.is_empty());
    assert_eq!(
        [
            report.print,
            report.input_format,
            report.output_format,
            report.no_session_persistence,
            report.permission_mode,
            report.verbose
        ],
        [ClaudeCapabilityEvidence::Supported; 6]
    );
    println!("Observed Claude version: {}", report.version);
}
