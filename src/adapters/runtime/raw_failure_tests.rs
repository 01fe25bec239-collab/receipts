use std::{error::Error, mem::discriminant};

use receipts_workspace_execution::execution::{
    ExecutionError, LiveProcessAttemptError, ProcessTermination,
};

use crate::{
    CodexCapability, CodexJsonlError, CodexJsonlErrorKind, CodexLiveProtocolError,
    CodexLiveStartError, CodexProbeChannel, CodexProbeError, CodexProbeExecutionError,
    CodexProbeKind, CodexTaskExecutionError, CodexTaskOutputChannel, FailureClass, RawFailure,
    RawFailureEvidence as Evidence, RawFailureSource as Source,
    classify_codex_probe_execution_error, classify_codex_task_execution_error,
};

const SECRET: &str = "sk-test-NOT-A-REAL-CREDENTIAL-A3-012";

fn safe(failure: RawFailure) {
    assert!(!format!("{failure:?}").contains(SECRET));
    assert!(!failure.to_string().contains(SECRET));
    assert!(failure.source().is_none());
    fn fixed<T: Copy + Send + Sync + 'static>(_: T) {}
    fixed(failure); // No borrowed source, heap payload, or error chain is retained.
}

fn workspace_error() -> ExecutionError {
    ExecutionError::ProcessSpawnFailed {
        detail: SECRET.repeat(100_000),
    }
}

#[test]
fn raw_task_sources_preserve_typed_evidence_and_existing_classifier() {
    let mut cases = vec![
        (CodexTaskExecutionError::EmptyPrompt, Evidence::EmptyPrompt),
        (
            CodexTaskExecutionError::MissingExitStatus,
            Evidence::MissingExitStatus,
        ),
    ];
    let error = workspace_error();
    let kind = discriminant(&error);
    cases.push((
        CodexTaskExecutionError::WorkspaceExecution { source: error },
        Evidence::WorkspaceExecution(kind),
    ));
    for termination in [
        ProcessTermination::TimedOutGracefullyTerminated,
        ProcessTermination::TimedOutForceKilled,
    ] {
        cases.push((
            CodexTaskExecutionError::TimedOut { termination },
            Evidence::TimedOut(termination),
        ));
    }
    for channel in [
        CodexTaskOutputChannel::Stdout,
        CodexTaskOutputChannel::Stderr,
    ] {
        cases.push((
            CodexTaskExecutionError::TruncatedStream { channel },
            Evidence::TaskTruncatedStream(channel),
        ));
    }
    for (error, evidence) in cases {
        let failure = RawFailure::from(&error);
        assert_eq!(failure.origin(), Source::CodexTask);
        assert_eq!(failure.evidence(), evidence);
        assert_eq!(failure.probe(), None);
        assert_eq!(
            failure.classify_failure(),
            classify_codex_task_execution_error(&error)
        );
        safe(failure);
        let start = RawFailure::from(&CodexLiveStartError::Request(error));
        assert_eq!(start.origin(), Source::CodexLiveStart);
        assert_eq!(start.evidence(), evidence);
        assert_eq!(start.classify_failure(), failure.classify_failure());
        safe(start);
    }
}

#[test]
fn raw_probe_sources_and_all_parser_variants_keep_existing_classification() {
    let mut cases = vec![];
    for probe in [CodexProbeKind::Version, CodexProbeKind::ExecHelp] {
        let error = workspace_error();
        let kind = discriminant(&error);
        cases.push((
            CodexProbeExecutionError::WorkspaceExecution {
                probe,
                source: error,
            },
            Some(probe),
            Evidence::WorkspaceExecution(kind),
        ));
        for termination in [
            ProcessTermination::TimedOutGracefullyTerminated,
            ProcessTermination::TimedOutForceKilled,
        ] {
            cases.push((
                CodexProbeExecutionError::TimedOut { probe, termination },
                Some(probe),
                Evidence::TimedOut(termination),
            ));
        }
        for channel in [CodexProbeChannel::Stdout, CodexProbeChannel::Stderr] {
            cases.push((
                CodexProbeExecutionError::TruncatedStream { probe, channel },
                Some(probe),
                Evidence::ProbeTruncatedStream(channel),
            ));
        }
        let mut parse_errors = vec![
            CodexProbeError::MissingStatus(probe),
            CodexProbeError::IncompleteCapture(probe),
        ];
        for code in [1, 124, 137, 401, 403, 429, 500] {
            parse_errors.push(CodexProbeError::NonSuccessStatus(probe, code));
        }
        for channel in [CodexProbeChannel::Stdout, CodexProbeChannel::Stderr] {
            parse_errors.push(CodexProbeError::InvalidEncoding(probe, channel));
        }
        for error in parse_errors {
            cases.push((
                CodexProbeExecutionError::Parse(error),
                None,
                Evidence::ProbeParse(error),
            ));
        }
    }
    let mut parse_errors = vec![
        CodexProbeError::MissingVersionEvidence,
        CodexProbeError::MissingHelpEvidence,
        CodexProbeError::InvalidHelpShape,
    ];
    for capability in [
        CodexCapability::Json,
        CodexCapability::OutputSchema,
        CodexCapability::Sandbox,
    ] {
        parse_errors.push(CodexProbeError::AmbiguousCapabilityEvidence(capability));
    }
    for error in parse_errors {
        cases.push((
            CodexProbeExecutionError::Parse(error),
            None,
            Evidence::ProbeParse(error),
        ));
    }
    for (error, probe, evidence) in cases {
        let failure = RawFailure::from(&error);
        assert_eq!(failure.origin(), Source::CodexProbe);
        assert_eq!(failure.probe(), probe);
        assert_eq!(failure.evidence(), evidence);
        assert_eq!(
            failure.classify_failure(),
            classify_codex_probe_execution_error(&error)
        );
        assert_eq!(
            failure.classify_failure() == FailureClass::Timeout,
            matches!(error, CodexProbeExecutionError::TimedOut { .. })
        );
        safe(failure);
    }
}

#[test]
fn raw_live_control_and_start_errors_never_invent_lifecycle_classification() {
    let error = workspace_error();
    let kind = discriminant(&error);
    for (error, evidence) in [
        (
            LiveProcessAttemptError::Execution(error),
            Evidence::WorkspaceExecution(kind),
        ),
        (
            LiveProcessAttemptError::AlreadyCollectedOrCollecting,
            Evidence::AlreadyCollectedOrCollecting,
        ),
        (
            LiveProcessAttemptError::ControllerFailed,
            Evidence::ControllerFailed,
        ),
    ] {
        let failure = RawFailure::from(&error);
        assert_eq!(failure.origin(), Source::WorkspaceLive);
        assert_eq!(failure.evidence(), evidence);
        assert_eq!(failure.classify_failure(), FailureClass::Unknown);
        safe(failure);
    }
}

#[test]
fn raw_protocol_errors_preserve_availability_without_invalid_output_or_timeout_inference() {
    let mut errors = vec![CodexLiveProtocolError::StdoutTruncated];
    for kind in [
        CodexJsonlErrorKind::InvalidUtf8,
        CodexJsonlErrorKind::InvalidJson,
        CodexJsonlErrorKind::IncompleteJson,
        CodexJsonlErrorKind::InvalidEventShape,
    ] {
        errors.push(CodexLiveProtocolError::Jsonl(CodexJsonlError {
            line: 17,
            kind,
        }));
    }
    for error in errors {
        let failure = RawFailure::from(&error);
        assert_eq!(failure.origin(), Source::CodexLiveProtocol);
        assert_eq!(failure.evidence(), Evidence::Protocol(error));
        assert_eq!(failure.classify_failure(), FailureClass::Unknown);
        safe(failure);
    }
}

#[test]
fn workspace_payloads_are_not_retained_and_variant_identity_survives() {
    for text in [
        SECRET.to_string(),
        format!("{SECRET} 429 401 403 timeout rate limited auth required policy blocked"),
        SECRET.repeat(100_000),
    ] {
        for error in [
            ExecutionError::ExecutablePathNotAbsolute {
                value: text.clone(),
            },
            ExecutionError::CwdOutsideWorkspace {
                requested: text.clone(),
                canonical_cwd: text.clone(),
                canonical_workspace_root: text.clone(),
            },
            ExecutionError::CaptureReaderFailed {
                stream: SECRET,
                detail: text.clone(),
            },
            ExecutionError::ForceKillFailed {
                detail: text.clone(),
            },
            ExecutionError::TimeoutFinalWaitFailed {
                detail: text.clone(),
            },
            ExecutionError::ProcessSpawnFailed { detail: text },
        ] {
            let kind = discriminant(&error);
            let failure = RawFailure::from(&error);
            drop(error);
            assert_eq!(failure.origin(), Source::WorkspaceExecution);
            assert_eq!(failure.evidence(), Evidence::WorkspaceExecution(kind));
            assert_eq!(failure.classify_failure(), FailureClass::Unknown);
            safe(failure);
        }
    }
    assert_ne!(
        discriminant(&ExecutionError::UnsupportedTimeoutPlatform),
        discriminant(&workspace_error())
    );
}

#[test]
fn raw_start_execution_and_controller_errors_keep_distinct_domains() {
    use receipts_workspace_execution::execution::LiveProcessStartError;
    let error = workspace_error();
    let kind = discriminant(&error);
    for (error, evidence) in [
        (
            LiveProcessStartError::Execution(error),
            Evidence::WorkspaceExecution(kind),
        ),
        (
            LiveProcessStartError::ControllerFailed,
            Evidence::ControllerFailed,
        ),
    ] {
        let start = RawFailure::from(&CodexLiveStartError::Workspace(error));
        assert_eq!(start.origin(), Source::CodexLiveStart);
        assert_eq!(start.evidence(), evidence);
        assert_eq!(start.classify_failure(), FailureClass::Unknown);
        safe(start);
    }
}

// Real terminal output is deliberately secret-bearing; the projection below
// must preserve only scalars even for Completed + process_success=true.
#[test]
#[ignore]
fn raw_start_terminal_probe() {
    use std::io::{Read, Write};
    if std::path::Path::new("ignore").exists() {
        unsafe extern "C" {
            fn signal(sig: i32, handler: usize) -> usize;
        }
        unsafe {
            signal(15, 1);
        }
    }
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input).unwrap();
    assert_eq!(input, SECRET.as_bytes());
    std::io::stdout().write_all(SECRET.as_bytes()).unwrap();
    std::io::stdout().flush().unwrap();
    std::io::stderr().write_all(SECRET.as_bytes()).unwrap();
    std::io::stderr().flush().unwrap();
    std::fs::write("ready", b"1").unwrap();
    let start = std::time::Instant::now();
    while !std::path::Path::new("release").exists() {
        assert!(start.elapsed() < std::time::Duration::from_secs(20));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    std::process::exit(std::fs::read_to_string("exit").unwrap().parse().unwrap());
}

#[test]
fn raw_start_terminal_projection_retains_only_scalar_evidence() {
    use receipts_workspace_execution::execution::{
        LiveProcessStartError, LiveProcessTerminalCause as Cause, ProcessRunRequest, ProcessStdin,
        ProcessTimeoutPolicy, start_live_process_attempt,
    };
    use std::{
        fs,
        time::{Duration, Instant},
    };
    for (index, (cause, code, forced)) in [
        (Cause::Completed, 0, false),
        (Cause::Completed, 7, false),
        (Cause::TimedOut, 0, false),
        (Cause::TimedOut, 0, true),
        (Cause::Cancelled, 0, false),
        (Cause::Cancelled, 0, true),
    ]
    .into_iter()
    .enumerate()
    {
        let dir =
            std::env::temp_dir().join(format!("receipts-raw-start-{}-{index}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("exit"), code.to_string()).unwrap();
        if forced {
            fs::write(dir.join("ignore"), b"1").unwrap();
        }
        let request = ProcessRunRequest::new(
            std::env::current_exe().unwrap(),
            [
                "raw_failure_tests::raw_start_terminal_probe",
                "--exact",
                "--ignored",
                "--nocapture",
            ],
            &dir,
            &dir,
        )
        .unwrap()
        .with_stdin(ProcessStdin::bytes(SECRET).unwrap());
        let policy =
            ProcessTimeoutPolicy::new(Duration::from_secs(2), Duration::from_millis(100)).unwrap();
        let attempt = start_live_process_attempt(&request, &policy).unwrap();
        let start = Instant::now();
        while !dir.join("ready").exists() {
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(2));
        }
        match cause {
            Cause::Completed => fs::write(dir.join("release"), b"1").unwrap(),
            Cause::Cancelled => {
                attempt.cancel();
            }
            Cause::TimedOut => {}
        }
        let outcome = attempt.wait_collect().unwrap();
        assert_eq!(outcome.terminal_cause(), cause);
        assert_eq!(outcome.forced_kill_required(), forced);
        assert!(
            outcome
                .stdout()
                .head()
                .windows(SECRET.len())
                .any(|part| part == SECRET.as_bytes())
        );
        assert_eq!(outcome.stderr().head(), SECRET.as_bytes());
        let expected = Evidence::LiveTerminalBeforeReady {
            terminal_cause: cause,
            forced_kill_required: outcome.forced_kill_required(),
            process_success: outcome.success(),
            exit_code: outcome.exit_code(),
        };
        // The public error variant accepts real terminal evidence. This tests
        // every projection, including Cancelled which public pre-READY control
        // cannot generate. It does not claim the collected attempt failed start.
        let error = CodexLiveStartError::Workspace(LiveProcessStartError::TerminalBeforeReady(
            Box::new(outcome),
        ));
        assert!(!format!("{error:?}").contains(SECRET));
        assert!(!error.to_string().contains(SECRET));
        let raw = RawFailure::from(&error);
        assert_eq!(raw.origin(), Source::CodexLiveStart);
        assert_eq!(raw.evidence(), expected);
        assert_eq!(
            raw.classify_failure(),
            if cause == Cause::TimedOut {
                FailureClass::Timeout
            } else {
                FailureClass::Unknown
            }
        );
        assert_ne!(raw.classify_failure(), FailureClass::UserCancelled);
        safe(raw);
        drop(error);
        assert_eq!(raw.evidence(), expected);
        fs::remove_dir_all(dir).unwrap();
    }
}
