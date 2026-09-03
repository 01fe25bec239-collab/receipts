use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use receipts_workspace_execution::execution::{
    ExecutionError, ProcessRunRequest, ProcessTermination, ProcessTimeoutPolicy,
};

use crate::CodexTaskSandboxMode;
use crate::codex_task_execution::{
    CodexTaskExecutionError, CodexTaskExecutionRequest, CodexTaskOutputChannel,
    TaskCaptureSnapshot, execute_with_runner,
};

fn test_policy() -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(Duration::from_secs(30), Duration::from_secs(3))
        .expect("test timeout policy is valid")
}

fn test_request(sandbox_mode: CodexTaskSandboxMode, prompt: &str) -> CodexTaskExecutionRequest {
    CodexTaskExecutionRequest::new(
        "/tmp/fake-codex-bin",
        "/tmp",
        "/tmp",
        test_policy(),
        sandbox_mode,
        prompt,
    )
}

fn completed_snapshot(stdout: &[u8], stderr: &[u8], exit_code: Option<i32>) -> TaskCaptureSnapshot {
    TaskCaptureSnapshot::new(
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

fn timed_snapshot(termination: ProcessTermination) -> TaskCaptureSnapshot {
    assert!(termination.is_timed_out());
    TaskCaptureSnapshot::new(
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

fn recorded_args(args: &[OsString]) -> Vec<String> {
    args.iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn read_only_uses_exact_argv() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let seen: RefCell<Vec<OsString>> = RefCell::new(Vec::new());
    let result = execute_with_runner(&request, |process_request, _| {
        *seen.borrow_mut() = process_request.arguments().to_vec();
        Ok(completed_snapshot(b"out", b"err", Some(0)))
    })
    .expect("read-only smoke task must succeed");
    assert_eq!(
        recorded_args(&seen.borrow()),
        vec!["exec", "--json", "--sandbox", "read-only", "smoke"]
    );
    assert_eq!(result.exit_code(), 0);
}

#[test]
fn workspace_write_uses_exact_sandbox_value() {
    let request = test_request(CodexTaskSandboxMode::WorkspaceWrite, "smoke");
    let seen: RefCell<Vec<OsString>> = RefCell::new(Vec::new());
    execute_with_runner(&request, |process_request, _| {
        *seen.borrow_mut() = process_request.arguments().to_vec();
        Ok(completed_snapshot(b"out", b"err", Some(0)))
    })
    .expect("workspace-write smoke task must succeed");
    let args = recorded_args(&seen.borrow());
    assert_eq!(args.len(), 5, "no additional production flags allowed");
    assert_eq!(&args[..3], ["exec", "--json", "--sandbox"]);
    assert_eq!(args[3], "workspace-write");
    assert_eq!(args[4], "smoke");
}

// Exhaustive with no wildcard: adding a third public sandbox variant breaks
// this match at compile time, proving exactly two representable modes.
fn sandbox_expectation(mode: CodexTaskSandboxMode) -> &'static str {
    match mode {
        CodexTaskSandboxMode::ReadOnly => "read-only",
        CodexTaskSandboxMode::WorkspaceWrite => "workspace-write",
    }
}

#[test]
fn sandbox_enum_has_exactly_two_public_variants() {
    for mode in [
        CodexTaskSandboxMode::ReadOnly,
        CodexTaskSandboxMode::WorkspaceWrite,
    ] {
        assert_eq!(mode.cli_value(), sandbox_expectation(mode));
    }
    assert_eq!(CodexTaskSandboxMode::ReadOnly.cli_value(), "read-only");
    assert_eq!(
        CodexTaskSandboxMode::WorkspaceWrite.cli_value(),
        "workspace-write"
    );
    // Negative assertion (not a capability): the unsafe full-access mode
    // must not be reachable through any public variant.
    for mode in [
        CodexTaskSandboxMode::ReadOnly,
        CodexTaskSandboxMode::WorkspaceWrite,
    ] {
        assert_ne!(mode.cli_value(), "danger-full-access");
    }
}

#[test]
fn empty_prompt_is_rejected_without_running() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "");
    let calls: RefCell<u32> = RefCell::new(0);
    let error = execute_with_runner(&request, |_, _| {
        *calls.borrow_mut() += 1;
        Ok(completed_snapshot(b"out", b"err", Some(0)))
    })
    .expect_err("empty prompt must fail closed");
    assert_eq!(*calls.borrow(), 0, "no child may run for an empty prompt");
    assert!(
        matches!(error, CodexTaskExecutionError::EmptyPrompt),
        "expected EmptyPrompt, got {error:?}"
    );
}

#[test]
fn whitespace_only_prompt_is_accepted_exactly() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "   ");
    let seen: RefCell<Vec<OsString>> = RefCell::new(Vec::new());
    execute_with_runner(&request, |process_request, _| {
        *seen.borrow_mut() = process_request.arguments().to_vec();
        Ok(completed_snapshot(b"out", b"err", Some(0)))
    })
    .expect("whitespace-only prompt must be accepted");
    let args = seen.borrow();
    assert_eq!(args.len(), 5);
    assert_eq!(args[4], OsString::from("   "));
}

#[test]
fn prompt_is_preserved_byte_for_byte() {
    let prompt = "  line one\n\t--json is prompt text \u{2713}  ";
    let request = test_request(CodexTaskSandboxMode::ReadOnly, prompt);
    let seen: RefCell<Vec<OsString>> = RefCell::new(Vec::new());
    execute_with_runner(&request, |process_request, _| {
        *seen.borrow_mut() = process_request.arguments().to_vec();
        Ok(completed_snapshot(b"out", b"err", Some(0)))
    })
    .expect("rich prompt must be accepted");
    let args = seen.borrow();
    assert_eq!(args.len(), 5, "prompt must be exactly one argv element");
    assert_eq!(
        args[4],
        OsString::from(prompt),
        "leading/trailing spaces, newline, tab, unicode, and flag-looking text must survive untouched"
    );
    assert_eq!(args[4].as_encoded_bytes(), prompt.as_bytes());
}

#[test]
fn relative_executable_fails_through_real_request_validation() {
    let request = CodexTaskExecutionRequest::new(
        "relative/codex",
        "/tmp",
        "/tmp",
        test_policy(),
        CodexTaskSandboxMode::ReadOnly,
        "smoke",
    );
    let calls: RefCell<u32> = RefCell::new(0);
    let error = execute_with_runner(&request, |_, _| {
        *calls.borrow_mut() += 1;
        Ok(completed_snapshot(b"out", b"err", Some(0)))
    })
    .expect_err("relative executable must fail");
    assert_eq!(
        *calls.borrow(),
        0,
        "no child may run after validation failure"
    );
    assert!(
        matches!(error, CodexTaskExecutionError::WorkspaceExecution { .. }),
        "expected WorkspaceExecution, got {error:?}"
    );

    let direct = ProcessRunRequest::new(
        "relative/codex",
        ["exec"],
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
fn explicit_workspace_root_and_cwd_reach_the_runner() {
    let workspace_root = PathBuf::from("/tmp/codex-task-ws-root");
    let cwd = PathBuf::from("/tmp/codex-task-ws-root/sub");
    let request = CodexTaskExecutionRequest::new(
        "/tmp/fake-codex-bin",
        workspace_root.clone(),
        cwd.clone(),
        test_policy(),
        CodexTaskSandboxMode::ReadOnly,
        "smoke",
    );
    let seen_root: RefCell<Option<PathBuf>> = RefCell::new(None);
    let seen_cwd: RefCell<Option<PathBuf>> = RefCell::new(None);
    execute_with_runner(&request, |process_request, _| {
        *seen_root.borrow_mut() = Some(process_request.workspace_root().to_path_buf());
        *seen_cwd.borrow_mut() = Some(process_request.cwd().to_path_buf());
        Ok(completed_snapshot(b"out", b"err", Some(0)))
    })
    .expect("explicit roots must succeed through the seam");
    assert_eq!(seen_root.borrow().as_ref(), Some(&workspace_root));
    assert_eq!(seen_cwd.borrow().as_ref(), Some(&cwd));
}

#[test]
fn explicit_timeout_policy_is_threaded_to_the_runner() {
    let policy = ProcessTimeoutPolicy::new(Duration::from_secs(45), Duration::from_secs(7))
        .expect("test timeout policy is valid");
    let request = CodexTaskExecutionRequest::new(
        "/tmp/fake-codex-bin",
        "/tmp",
        "/tmp",
        policy,
        CodexTaskSandboxMode::ReadOnly,
        "smoke",
    );
    let seen_run: RefCell<Vec<Duration>> = RefCell::new(Vec::new());
    let seen_grace: RefCell<Vec<Duration>> = RefCell::new(Vec::new());
    execute_with_runner(&request, |_, seen| {
        seen_run.borrow_mut().push(seen.run_timeout());
        seen_grace.borrow_mut().push(seen.termination_grace());
        Ok(completed_snapshot(b"out", b"err", Some(0)))
    })
    .expect("explicit timeout must succeed through the seam");
    assert_eq!(*seen_run.borrow(), vec![Duration::from_secs(45)]);
    assert_eq!(*seen_grace.borrow(), vec![Duration::from_secs(7)]);
}

#[test]
fn successful_zero_exit_preserves_exact_streams_and_sandbox() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let result = execute_with_runner(&request, |_, _| {
        Ok(completed_snapshot(
            b"exact stdout bytes",
            b"exact stderr bytes",
            Some(0),
        ))
    })
    .expect("zero exit must succeed");
    assert_eq!(result.exit_code(), 0);
    assert_eq!(result.stdout(), b"exact stdout bytes");
    assert_eq!(result.stderr(), b"exact stderr bytes");
    assert_eq!(result.sandbox_mode(), CodexTaskSandboxMode::ReadOnly);
}

#[test]
fn completed_nonzero_exit_is_preserved_as_result() {
    let request = test_request(CodexTaskSandboxMode::WorkspaceWrite, "smoke");
    let result = execute_with_runner(&request, |_, _| {
        Ok(completed_snapshot(
            b"partial work",
            b"agent failed",
            Some(7),
        ))
    })
    .expect("completed non-zero exit must be a result, not an error");
    assert_eq!(result.exit_code(), 7);
    assert_eq!(result.stdout(), b"partial work");
    assert_eq!(result.stderr(), b"agent failed");
    assert_eq!(result.sandbox_mode(), CodexTaskSandboxMode::WorkspaceWrite);
}

#[test]
fn missing_exit_status_fails_closed() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let error = execute_with_runner(&request, |_, _| {
        Ok(completed_snapshot(b"out", b"err", None))
    })
    .expect_err("missing status must fail closed");
    assert!(
        matches!(error, CodexTaskExecutionError::MissingExitStatus),
        "expected MissingExitStatus, got {error:?}"
    );
}

#[test]
fn graceful_timeout_fails_closed() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let error = execute_with_runner(&request, |_, _| {
        Ok(timed_snapshot(
            ProcessTermination::TimedOutGracefullyTerminated,
        ))
    })
    .expect_err("graceful timeout must fail closed");
    match error {
        CodexTaskExecutionError::TimedOut { termination } => {
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
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let error = execute_with_runner(&request, |_, _| {
        Ok(timed_snapshot(ProcessTermination::TimedOutForceKilled))
    })
    .expect_err("force-killed timeout must fail closed");
    match error {
        CodexTaskExecutionError::TimedOut { termination } => {
            assert_eq!(termination, ProcessTermination::TimedOutForceKilled);
        }
        other => panic!("expected TimedOut force-killed, got {other:?}"),
    }
}

#[test]
fn truncated_stdout_fails_closed() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let error = execute_with_runner(&request, |_, _| {
        Ok(TaskCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(0),
            b"head".to_vec(),
            b"tail".to_vec(),
            true,
            b"err".to_vec(),
            Vec::new(),
            false,
        ))
    })
    .expect_err("truncated stdout must fail closed");
    match error {
        CodexTaskExecutionError::TruncatedStream { channel } => {
            assert_eq!(channel, CodexTaskOutputChannel::Stdout);
        }
        other => panic!("expected truncated stdout, got {other:?}"),
    }
}

#[test]
fn truncated_stderr_fails_closed() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let error = execute_with_runner(&request, |_, _| {
        Ok(TaskCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(0),
            b"out".to_vec(),
            Vec::new(),
            false,
            b"head".to_vec(),
            b"tail".to_vec(),
            true,
        ))
    })
    .expect_err("truncated stderr must fail closed");
    match error {
        CodexTaskExecutionError::TruncatedStream { channel } => {
            assert_eq!(channel, CodexTaskOutputChannel::Stderr);
        }
        other => panic!("expected truncated stderr, got {other:?}"),
    }
}

#[test]
fn stdout_head_plus_tail_reconstructs_without_synthetic_bytes() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let result = execute_with_runner(&request, |_, _| {
        Ok(TaskCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(0),
            b"abc".to_vec(),
            b"def".to_vec(),
            false,
            Vec::new(),
            Vec::new(),
            false,
        ))
    })
    .expect("non-truncated split stdout must reconstruct");
    assert_eq!(result.stdout(), b"abcdef");
    assert_eq!(result.stderr(), b"");
}

#[test]
fn stderr_head_plus_tail_reconstructs_without_synthetic_bytes() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let result = execute_with_runner(&request, |_, _| {
        Ok(TaskCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(0),
            Vec::new(),
            Vec::new(),
            false,
            b"abc".to_vec(),
            b"def".to_vec(),
            false,
        ))
    })
    .expect("non-truncated split stderr must reconstruct");
    assert_eq!(result.stderr(), b"abcdef");
    assert_eq!(result.stdout(), b"");
}

#[test]
fn binary_non_utf8_output_is_preserved() {
    let stdout: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x80, b'a', b'b'];
    let stderr: Vec<u8> = vec![0xC3, 0x28, 0xFF, b'z'];
    assert!(std::str::from_utf8(&stdout).is_err());
    assert!(std::str::from_utf8(&stderr).is_err());
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let result = execute_with_runner(&request, |_, _| {
        Ok(completed_snapshot(&stdout, &stderr, Some(0)))
    })
    .expect("non-UTF-8 output must succeed");
    assert_eq!(result.stdout(), stdout);
    assert_eq!(result.stderr(), stderr);
}

#[test]
fn workspace_execution_error_retains_source() {
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let error = execute_with_runner(&request, |_, _| {
        Err(CodexTaskExecutionError::WorkspaceExecution {
            source: ExecutionError::ExecutableNotFound {
                detail: "injected test failure".to_string(),
            },
        })
    })
    .expect_err("injected execution failure must surface");
    match &error {
        CodexTaskExecutionError::WorkspaceExecution { source } => {
            assert!(
                matches!(source, ExecutionError::ExecutableNotFound { .. }),
                "source must be retained: {source:?}"
            );
        }
        other => panic!("expected WorkspaceExecution, got {other:?}"),
    }
    assert!(format!("{error:?}").contains("ExecutableNotFound"));
}

#[test]
fn jsonl_looking_stdout_is_preserved_without_parsing() {
    let stdout = b"{\"type\":\"item.started\",\"id\":1}\nnot json at all\n{\"type\":\"item.completed\",\"ok\":true}\n";
    let request = test_request(CodexTaskSandboxMode::ReadOnly, "smoke");
    let result = execute_with_runner(&request, |_, _| {
        Ok(completed_snapshot(stdout, b"", Some(0)))
    })
    .expect("JSONL-looking output must succeed as raw bytes");
    assert_eq!(result.stdout(), stdout);
}

#[test]
fn sandbox_mode_is_preserved_in_result_for_both_modes() {
    for mode in [
        CodexTaskSandboxMode::ReadOnly,
        CodexTaskSandboxMode::WorkspaceWrite,
    ] {
        let request = test_request(mode, "smoke");
        let result = execute_with_runner(&request, |_, _| {
            Ok(completed_snapshot(b"out", b"err", Some(0)))
        })
        .expect("task must succeed");
        assert_eq!(result.sandbox_mode(), mode);
    }
}
