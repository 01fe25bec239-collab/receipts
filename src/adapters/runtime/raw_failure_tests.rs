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
        let start = RawFailure::from(&CodexLiveStartError::Workspace(error));
        assert_eq!(start.origin(), Source::CodexLiveStart);
        assert_eq!(start.evidence(), evidence);
        assert_eq!(start.classify_failure(), FailureClass::Unknown);
        safe(start);
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
