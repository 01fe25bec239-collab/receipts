use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use receipts_workspace_execution::execution::{
    ExecutionError, ProcessRunRequest, ProcessTermination, ProcessTimeoutPolicy,
};

use crate::codex_probe_execution::{
    CodexProbeExecutionError, ProbeCaptureSnapshot, execute_codex_capability_probe,
    execute_with_runner,
};
use crate::{CodexCapabilityEvidence, CodexProbeChannel, CodexProbeError, CodexProbeKind};

const VALID_VERSION_STDOUT: &[u8] = b"codex-cli 0.152.1\n";

const VALID_HELP: &[u8] = b"Codex execution\n\nUsage: codex exec [OPTIONS] [PROMPT]\n\nOptions:\n  -s, --sandbox <SANDBOX_MODE>\n          Select the sandbox policy\n\n      --output-schema <FILE>\n          Path to a JSON Schema\n\n      --json\n          Print events as JSONL\n";

const HELP_WITHOUT_TARGETS: &[u8] =
    b"Codex execution\n\nUsage: codex exec [OPTIONS] [PROMPT]\n\nOptions:\n  --color <WHEN>\n      Control terminal colors\n";

fn test_policy() -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(Duration::from_secs(10), Duration::from_secs(2))
        .expect("test timeout policy is valid")
}

fn test_roots() -> (PathBuf, PathBuf, PathBuf) {
    (
        PathBuf::from("/tmp/fake-codex-bin"),
        PathBuf::from("/tmp"),
        PathBuf::from("/tmp"),
    )
}

fn completed_snapshot(
    stdout: &[u8],
    stderr: &[u8],
    exit_code: Option<i32>,
) -> ProbeCaptureSnapshot {
    ProbeCaptureSnapshot::new(
        ProcessTermination::Completed,
        exit_code,
        stdout.to_vec(),
        Vec::new(),
        false,
        stderr.to_vec(),
        Vec::new(),
        false,
    )
}

fn timed_snapshot(termination: ProcessTermination) -> ProbeCaptureSnapshot {
    assert!(termination.is_timed_out());
    ProbeCaptureSnapshot::new(
        termination,
        None,
        Vec::new(),
        Vec::new(),
        false,
        Vec::new(),
        Vec::new(),
        false,
    )
}

#[derive(Debug)]
struct RecordedCall {
    probe: CodexProbeKind,
    executable: PathBuf,
    args: Vec<OsString>,
}

fn recorded_args(call: &RecordedCall) -> Vec<String> {
    call.args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn exact_two_command_plans_use_absolute_executable_and_split_argv() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let calls: RefCell<Vec<RecordedCall>> = RefCell::new(Vec::new());
    let report = execute_with_runner(&codex, &root, &cwd, &policy, |probe, request, _| {
        calls.borrow_mut().push(RecordedCall {
            probe,
            executable: request.executable().to_path_buf(),
            args: request.arguments().to_vec(),
        });
        match probe {
            CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
            CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
        }
    })
    .expect("valid probe must succeed");

    assert_eq!(report.version, "codex-cli 0.152.1");
    let calls = calls.borrow();
    assert_eq!(calls.len(), 2, "bridge must run exactly two children");
    assert_eq!(calls[0].probe, CodexProbeKind::Version);
    assert_eq!(calls[0].executable, codex);
    assert_eq!(recorded_args(&calls[0]), vec!["--version"]);
    assert_eq!(calls[1].probe, CodexProbeKind::ExecHelp);
    assert_eq!(calls[1].executable, codex);
    assert_eq!(recorded_args(&calls[1]), vec!["exec", "--help"]);
}

#[test]
fn version_child_runs_before_exec_help_child() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let order: RefCell<Vec<CodexProbeKind>> = RefCell::new(Vec::new());
    execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| {
        order.borrow_mut().push(probe);
        match probe {
            CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
            CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
        }
    })
    .expect("valid probe must succeed");
    assert_eq!(
        *order.borrow(),
        vec![CodexProbeKind::Version, CodexProbeKind::ExecHelp]
    );
}

#[test]
fn successful_complete_capture_reports_supported_capabilities() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let report = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
        CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
    })
    .expect("valid probe must succeed");
    assert_eq!(report.version, "codex-cli 0.152.1");
    assert_eq!(report.json, CodexCapabilityEvidence::Supported);
    assert_eq!(report.output_schema, CodexCapabilityEvidence::Supported);
    assert_eq!(report.sandbox, CodexCapabilityEvidence::Supported);
}

#[test]
fn explicit_timeout_policy_is_threaded_to_both_children() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let seen_run_timeout: RefCell<Vec<Duration>> = RefCell::new(Vec::new());
    execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, seen| {
        seen_run_timeout.borrow_mut().push(seen.run_timeout());
        match probe {
            CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
            CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
        }
    })
    .expect("valid probe must succeed");
    assert_eq!(
        *seen_run_timeout.borrow(),
        vec![Duration::from_secs(10), Duration::from_secs(10)]
    );
}

#[test]
fn relative_codex_path_fails_through_real_request_validation() {
    let policy = test_policy();
    let root = Path::new("/tmp");
    let cwd = Path::new("/tmp");
    let relative = Path::new("relative/codex");
    let calls: RefCell<u32> = RefCell::new(0);
    let error = execute_with_runner(relative, root, cwd, &policy, |probe, _, _| {
        *calls.borrow_mut() += 1;
        match probe {
            CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
            CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
        }
    })
    .expect_err("relative executable must fail");
    assert_eq!(
        *calls.borrow(),
        0,
        "no child may run after validation failure"
    );
    match error {
        CodexProbeExecutionError::WorkspaceExecution { probe, .. } => {
            assert_eq!(probe, CodexProbeKind::Version);
        }
        other => panic!("expected WorkspaceExecution, got {other:?}"),
    }

    let direct = ProcessRunRequest::new(
        relative,
        ["--version"],
        Path::new("/tmp"),
        Path::new("/tmp"),
    );
    assert!(
        matches!(
            direct,
            Err(ExecutionError::ExecutablePathNotAbsolute { .. })
        ),
        "authoritative check remains workspace-owned: {direct:?}"
    );
}

#[test]
fn ordinary_nonzero_version_exit_still_runs_help_then_fails_closed() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let calls: RefCell<Vec<CodexProbeKind>> = RefCell::new(Vec::new());
    let error = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| {
        calls.borrow_mut().push(probe);
        match probe {
            CodexProbeKind::Version => Ok(completed_snapshot(b"codex-cli 0.152.1\n", b"", Some(7))),
            CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
        }
    })
    .expect_err("non-zero version must fail closed");
    assert_eq!(
        *calls.borrow(),
        vec![CodexProbeKind::Version, CodexProbeKind::ExecHelp],
        "ordinary non-zero version must still run exec-help"
    );
    match error {
        CodexProbeExecutionError::Parse(CodexProbeError::NonSuccessStatus(
            CodexProbeKind::Version,
            7,
        )) => {}
        other => panic!("expected NonSuccessStatus(Version, 7), got {other:?}"),
    }
}

#[test]
fn ordinary_nonzero_help_exit_fails_closed() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let error = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
        CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(9))),
    })
    .expect_err("non-zero help must fail closed");
    match error {
        CodexProbeExecutionError::Parse(CodexProbeError::NonSuccessStatus(
            CodexProbeKind::ExecHelp,
            9,
        )) => {}
        other => panic!("expected NonSuccessStatus(ExecHelp, 9), got {other:?}"),
    }
}

#[test]
fn missing_numeric_status_is_not_fabricated() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let error = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", None)),
        CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
    })
    .expect_err("missing status must fail closed");
    match error {
        CodexProbeExecutionError::Parse(CodexProbeError::MissingStatus(
            CodexProbeKind::Version,
        )) => {}
        other => panic!("expected MissingStatus(Version), got {other:?}"),
    }
}

#[test]
fn graceful_timeout_fails_closed_without_parsing() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let calls: RefCell<Vec<CodexProbeKind>> = RefCell::new(Vec::new());
    let error = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| {
        calls.borrow_mut().push(probe);
        match probe {
            CodexProbeKind::Version => Ok(timed_snapshot(
                ProcessTermination::TimedOutGracefullyTerminated,
            )),
            CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
        }
    })
    .expect_err("timeout must fail closed");
    assert_eq!(
        *calls.borrow(),
        vec![CodexProbeKind::Version],
        "timeout on version must not launch exec-help"
    );
    match error {
        CodexProbeExecutionError::TimedOut { probe, termination } => {
            assert_eq!(probe, CodexProbeKind::Version);
            assert_eq!(
                termination,
                ProcessTermination::TimedOutGracefullyTerminated
            );
        }
        other => panic!("expected TimedOut gracefully, got {other:?}"),
    }
}

#[test]
fn force_killed_timeout_is_distinguishable() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let error = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
        CodexProbeKind::ExecHelp => Ok(timed_snapshot(ProcessTermination::TimedOutForceKilled)),
    })
    .expect_err("force-killed timeout must fail closed");
    match error {
        CodexProbeExecutionError::TimedOut { probe, termination } => {
            assert_eq!(probe, CodexProbeKind::ExecHelp);
            assert_eq!(termination, ProcessTermination::TimedOutForceKilled);
        }
        other => panic!("expected TimedOut force-killed, got {other:?}"),
    }
}

#[test]
fn truncated_stdout_fails_closed() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let error = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(ProbeCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(0),
            b"codex-cli".to_vec(),
            b"0.152.1".to_vec(),
            true,
            Vec::new(),
            Vec::new(),
            false,
        )),
        CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
    })
    .expect_err("truncated stdout must fail closed");
    match error {
        CodexProbeExecutionError::TruncatedStream { probe, channel } => {
            assert_eq!(probe, CodexProbeKind::Version);
            assert_eq!(channel, CodexProbeChannel::Stdout);
        }
        other => panic!("expected truncated stdout, got {other:?}"),
    }
}

#[test]
fn truncated_stderr_fails_closed() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let error = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
        CodexProbeKind::ExecHelp => Ok(ProbeCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(0),
            VALID_HELP.to_vec(),
            Vec::new(),
            false,
            b"warning".to_vec(),
            b"tail".to_vec(),
            true,
        )),
    })
    .expect_err("truncated stderr must fail closed");
    match error {
        CodexProbeExecutionError::TruncatedStream { probe, channel } => {
            assert_eq!(probe, CodexProbeKind::ExecHelp);
            assert_eq!(channel, CodexProbeChannel::Stderr);
        }
        other => panic!("expected truncated stderr, got {other:?}"),
    }
}

#[test]
fn complete_head_plus_tail_is_reconstructed_without_synthetic_bytes() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let split = VALID_HELP.len() / 2;
    let (head, tail) = VALID_HELP.split_at(split);
    assert!(!head.is_empty() && !tail.is_empty());
    let report = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
        CodexProbeKind::ExecHelp => Ok(ProbeCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(0),
            head.to_vec(),
            tail.to_vec(),
            false,
            Vec::new(),
            Vec::new(),
            false,
        )),
    })
    .expect("split head/tail must reconstruct exactly");
    assert_eq!(report.json, CodexCapabilityEvidence::Supported);
    assert_eq!(report.output_schema, CodexCapabilityEvidence::Supported);
    assert_eq!(report.sandbox, CodexCapabilityEvidence::Supported);

    let version_split = VALID_VERSION_STDOUT.len() / 2;
    let (vhead, vtail) = VALID_VERSION_STDOUT.split_at(version_split);
    let report = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(ProbeCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(0),
            vhead.to_vec(),
            vtail.to_vec(),
            false,
            Vec::new(),
            Vec::new(),
            false,
        )),
        CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
    })
    .expect("split version head/tail must reconstruct exactly");
    assert_eq!(report.version, "codex-cli 0.152.1");
}

#[test]
fn malformed_help_fails_closed_through_existing_parser() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let error = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
        CodexProbeKind::ExecHelp => Ok(completed_snapshot(
            b"arbitrary non-empty text",
            b"",
            Some(0),
        )),
    })
    .expect_err("malformed help must fail closed");
    match error {
        CodexProbeExecutionError::Parse(CodexProbeError::InvalidHelpShape) => {}
        other => panic!("expected InvalidHelpShape, got {other:?}"),
    }
}

#[test]
fn valid_help_without_declarations_preserves_unknown() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let report = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(completed_snapshot(VALID_VERSION_STDOUT, b"", Some(0))),
        CodexProbeKind::ExecHelp => Ok(completed_snapshot(HELP_WITHOUT_TARGETS, b"", Some(0))),
    })
    .expect("valid help without targets must parse");
    assert_eq!(report.json, CodexCapabilityEvidence::Unknown);
    assert_eq!(report.output_schema, CodexCapabilityEvidence::Unknown);
    assert_eq!(report.sandbox, CodexCapabilityEvidence::Unknown);
}

#[test]
fn version_stdout_is_preferred_over_stderr_warning() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let report = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| match probe {
        CodexProbeKind::Version => Ok(completed_snapshot(
            VALID_VERSION_STDOUT,
            b"WARNING: diagnostic",
            Some(0),
        )),
        CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
    })
    .expect("version with warning must parse");
    assert_eq!(report.version, "codex-cli 0.152.1");
}

#[test]
fn workspace_execution_failure_retains_probe_and_source() {
    let (codex, root, cwd) = test_roots();
    let policy = test_policy();
    let calls: RefCell<Vec<CodexProbeKind>> = RefCell::new(Vec::new());
    let error = execute_with_runner(&codex, &root, &cwd, &policy, |probe, _, _| {
        calls.borrow_mut().push(probe);
        match probe {
            CodexProbeKind::Version => Err(CodexProbeExecutionError::WorkspaceExecution {
                probe,
                source: ExecutionError::ExecutableNotFound {
                    detail: "injected test failure".to_string(),
                },
            }),
            CodexProbeKind::ExecHelp => Ok(completed_snapshot(VALID_HELP, b"", Some(0))),
        }
    })
    .expect_err("injected execution failure must surface");
    assert_eq!(
        *calls.borrow(),
        vec![CodexProbeKind::Version],
        "execution failure must stop before second child"
    );
    match error {
        CodexProbeExecutionError::WorkspaceExecution { probe, source } => {
            assert_eq!(probe, CodexProbeKind::Version);
            assert!(
                matches!(source, ExecutionError::ExecutableNotFound { .. }),
                "source must be retained: {source:?}"
            );
        }
        other => panic!("expected WorkspaceExecution, got {other:?}"),
    }
}

#[test]
#[ignore]
fn live_codex_probe_through_real_runner() {
    let bin = std::env::var("RECEIPTS_TEST_CODEX_BIN")
        .expect("RECEIPTS_TEST_CODEX_BIN must hold an absolute executable path");
    let codex = PathBuf::from(bin);
    assert!(
        codex.is_absolute(),
        "live test requires an absolute executable path"
    );
    let workspace_root = PathBuf::from("/tmp");
    let cwd = PathBuf::from("/tmp");
    let policy = ProcessTimeoutPolicy::new(Duration::from_secs(20), Duration::from_secs(2))
        .expect("live test timeout policy is valid");
    let result = execute_codex_capability_probe(&codex, &workspace_root, &cwd, &policy);
    match &result {
        Ok(report) => {
            assert_eq!(report.json, CodexCapabilityEvidence::Supported);
        }
        Err(error) => panic!("live probe failed closed with: {error:?}"),
    }
}
