use std::time::Duration;

use receipts_workspace_execution::execution::{ProcessTermination, ProcessTimeoutPolicy};

use crate::codex_task_execution::{TaskCaptureSnapshot, execute_with_runner};
use crate::{
    CodexTaskExecutionError, CodexTaskExecutionRequest, CodexTaskExecutionResult,
    CodexTaskOutputChannel, CodexTaskSandboxMode, FailureClass,
    classify_codex_task_execution_error, classify_codex_task_execution_result,
};

fn request(executable: &str) -> CodexTaskExecutionRequest {
    CodexTaskExecutionRequest::new(
        executable,
        "/tmp",
        "/tmp",
        ProcessTimeoutPolicy::new(Duration::from_secs(30), Duration::from_secs(3))
            .expect("test timeout policy is valid"),
        CodexTaskSandboxMode::ReadOnly,
        "test",
    )
}

fn completed_result(exit_code: i32, stdout: &[u8], stderr: &[u8]) -> CodexTaskExecutionResult {
    execute_with_runner(&request("/tmp/fake-codex-bin"), |_, _| {
        Ok(TaskCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(exit_code),
            stdout.to_vec(),
            Vec::new(),
            false,
            stderr.to_vec(),
            Vec::new(),
            false,
        ))
    })
    .expect("completed fixture must produce a result")
}

fn workspace_error() -> CodexTaskExecutionError {
    execute_with_runner(&request("relative/codex"), |_, _| {
        unreachable!("request validation must fail before execution")
    })
    .expect_err("relative executable must produce a workspace error")
}

#[test]
fn graceful_timeout_is_timeout() {
    let error = CodexTaskExecutionError::TimedOut {
        termination: ProcessTermination::TimedOutGracefullyTerminated,
    };

    assert_eq!(
        classify_codex_task_execution_error(&error),
        FailureClass::Timeout
    );
}

#[test]
fn force_killed_timeout_is_timeout() {
    let error = CodexTaskExecutionError::TimedOut {
        termination: ProcessTermination::TimedOutForceKilled,
    };

    assert_eq!(
        classify_codex_task_execution_error(&error),
        FailureClass::Timeout
    );
}

#[test]
fn empty_prompt_is_unknown() {
    assert_eq!(
        classify_codex_task_execution_error(&CodexTaskExecutionError::EmptyPrompt),
        FailureClass::Unknown
    );
}

#[test]
fn stdout_truncation_is_unknown() {
    let error = CodexTaskExecutionError::TruncatedStream {
        channel: CodexTaskOutputChannel::Stdout,
    };

    assert_eq!(
        classify_codex_task_execution_error(&error),
        FailureClass::Unknown
    );
}

#[test]
fn stderr_truncation_is_unknown() {
    let error = CodexTaskExecutionError::TruncatedStream {
        channel: CodexTaskOutputChannel::Stderr,
    };

    assert_eq!(
        classify_codex_task_execution_error(&error),
        FailureClass::Unknown
    );
}

#[test]
fn missing_exit_status_is_unknown() {
    assert_eq!(
        classify_codex_task_execution_error(&CodexTaskExecutionError::MissingExitStatus),
        FailureClass::Unknown
    );
}

#[test]
fn workspace_execution_is_unknown() {
    assert!(matches!(
        workspace_error(),
        CodexTaskExecutionError::WorkspaceExecution { .. }
    ));
    assert_eq!(
        classify_codex_task_execution_error(&workspace_error()),
        FailureClass::Unknown
    );
}

#[test]
fn local_workspace_error_cannot_become_provider_failure() {
    let class = classify_codex_task_execution_error(&workspace_error());

    assert_eq!(class, FailureClass::Unknown);
    for forbidden in [
        FailureClass::ProviderDown,
        FailureClass::SandboxDenied,
        FailureClass::PolicyBlocked,
        FailureClass::RuntimeCrash,
        FailureClass::InvalidOutput,
        FailureClass::Timeout,
    ] {
        assert_ne!(class, forbidden);
    }
}

#[test]
fn completed_zero_exit_has_no_failure_class() {
    assert_eq!(
        classify_codex_task_execution_result(&completed_result(0, b"out", b"err")),
        None
    );
}

#[test]
fn completed_nonzero_exit_is_unknown() {
    assert_eq!(
        classify_codex_task_execution_result(&completed_result(7, b"arbitrary", b"bytes")),
        Some(FailureClass::Unknown)
    );
}

#[test]
fn adversarial_exit_codes_are_unknown() {
    for exit_code in [1, 2, 75, 124, 126, 127, 137, 255] {
        assert_eq!(
            classify_codex_task_execution_result(&completed_result(exit_code, b"", b"")),
            Some(FailureClass::Unknown),
            "exit code {exit_code}"
        );
    }
}

#[test]
fn rate_limit_prose_is_unknown() {
    assert_prose_is_unknown(b"rate limit exceeded");
}

#[test]
fn auth_prose_is_unknown() {
    assert_prose_is_unknown(b"authentication required; please login");
}

#[test]
fn provider_down_prose_is_unknown() {
    assert_prose_is_unknown(b"provider unavailable / network down");
}

#[test]
fn policy_prose_is_unknown() {
    assert_prose_is_unknown(b"policy blocked");
}

#[test]
fn safety_prose_is_unknown() {
    assert_prose_is_unknown(b"safety check pending");
}

#[test]
fn sandbox_prose_is_unknown() {
    assert_prose_is_unknown(b"sandbox denied");
}

fn assert_prose_is_unknown(stderr: &[u8]) {
    assert_eq!(
        classify_codex_task_execution_result(&completed_result(1, b"", stderr)),
        Some(FailureClass::Unknown)
    );
}

#[test]
fn all_scary_prose_is_unknown() {
    let stdout = b"rate limit\nsession exhausted\nauth required\nprovider down\nsandbox denied";
    let stderr =
        b"safety check pending\npolicy blocked\nruntime crash\ninvalid output\nuser cancelled";

    assert_eq!(
        classify_codex_task_execution_result(&completed_result(1, stdout, stderr)),
        Some(FailureClass::Unknown)
    );
}

#[test]
fn classification_is_independent_of_output() {
    let quiet = completed_result(7, b"", b"");
    let noisy = completed_result(7, b"radically different stdout", b"and stderr");

    assert_eq!(
        classify_codex_task_execution_result(&quiet),
        classify_codex_task_execution_result(&noisy)
    );
    assert_eq!(
        classify_codex_task_execution_result(&quiet),
        Some(FailureClass::Unknown)
    );
}

#[test]
fn only_timeout_or_unknown_are_returned() {
    let errors = [
        CodexTaskExecutionError::TimedOut {
            termination: ProcessTermination::TimedOutGracefullyTerminated,
        },
        CodexTaskExecutionError::TimedOut {
            termination: ProcessTermination::TimedOutForceKilled,
        },
        CodexTaskExecutionError::EmptyPrompt,
        CodexTaskExecutionError::TruncatedStream {
            channel: CodexTaskOutputChannel::Stdout,
        },
        CodexTaskExecutionError::TruncatedStream {
            channel: CodexTaskOutputChannel::Stderr,
        },
        CodexTaskExecutionError::MissingExitStatus,
        workspace_error(),
    ];

    for class in errors.iter().map(classify_codex_task_execution_error) {
        assert!(matches!(
            class,
            FailureClass::Timeout | FailureClass::Unknown
        ));
    }
    assert_eq!(
        classify_codex_task_execution_result(&completed_result(0, b"", b"")),
        None
    );
    assert_eq!(
        classify_codex_task_execution_result(&completed_result(1, b"", b"")),
        Some(FailureClass::Unknown)
    );
}

#[test]
fn unknown_remains_first_class() {
    assert_ne!(FailureClass::Unknown, FailureClass::RateLimited);
    assert_ne!(FailureClass::Unknown, FailureClass::ProviderDown);
    assert_ne!(FailureClass::Unknown, FailureClass::PolicyBlocked);
}
