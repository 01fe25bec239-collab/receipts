use std::{path::Path, time::Duration};

use receipts_workspace_execution::execution::{ProcessTermination, ProcessTimeoutPolicy};

use crate::codex_probe_execution::{ProbeCaptureSnapshot, execute_with_runner};
use crate::{
    CodexCapability, CodexProbeChannel, CodexProbeError, CodexProbeExecutionError, CodexProbeKind,
    FailureClass, classify_codex_probe_execution_error,
};

const PROBES: [CodexProbeKind; 2] = [CodexProbeKind::Version, CodexProbeKind::ExecHelp];
const CHANNELS: [CodexProbeChannel; 2] = [CodexProbeChannel::Stdout, CodexProbeChannel::Stderr];
const TERMINATIONS: [ProcessTermination; 2] = [
    ProcessTermination::TimedOutGracefullyTerminated,
    ProcessTermination::TimedOutForceKilled,
];
const OUTPUTS: [&[u8]; 6] = [
    b"ordinary unrelated text",
    b"rate limit exceeded; quota exhausted; too many requests; 429",
    b"authentication required / please login",
    b"provider unavailable / network down",
    b"policy blocked",
    b"rate limited; session exhausted; auth required; provider down; sandbox denied; safety check pending; policy blocked; runtime crash; invalid output; user cancelled",
];

fn policy() -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(Duration::from_secs(10), Duration::from_secs(2))
        .expect("valid test policy")
}

fn workspace_error() -> CodexProbeExecutionError {
    let error = execute_with_runner(
        Path::new("relative/codex"),
        Path::new("/tmp"),
        Path::new("/tmp"),
        &policy(),
        |_, _, _| unreachable!("relative executable must fail before execution"),
    )
    .expect_err("relative executable must fail request validation");
    assert!(matches!(
        error,
        CodexProbeExecutionError::WorkspaceExecution {
            probe: CodexProbeKind::Version,
            ..
        }
    ));
    error
}

fn output_error(text: &[u8], channel: CodexProbeChannel) -> CodexProbeExecutionError {
    let mut calls = Vec::new();
    let error = execute_with_runner(
        Path::new("/tmp/fake-codex-bin"),
        Path::new("/tmp"),
        Path::new("/tmp"),
        &policy(),
        |probe, _, _| {
            calls.push(probe);
            let (stdout, stderr) = match channel {
                CodexProbeChannel::Stdout => (text.to_vec(), Vec::new()),
                CodexProbeChannel::Stderr => (Vec::new(), text.to_vec()),
            };
            Ok(ProbeCaptureSnapshot::new(
                ProcessTermination::Completed,
                Some(0),
                stdout,
                Vec::new(),
                false,
                stderr,
                Vec::new(),
                false,
            ))
        },
    )
    .expect_err("complete nonempty output lacks help shape");
    assert_eq!(calls, PROBES);
    assert!(matches!(
        error,
        CodexProbeExecutionError::Parse(CodexProbeError::InvalidHelpShape)
    ));
    error
}

fn parse_errors() -> Vec<CodexProbeError> {
    let mut errors = vec![
        CodexProbeError::MissingVersionEvidence,
        CodexProbeError::MissingHelpEvidence,
        CodexProbeError::InvalidHelpShape,
    ];
    for probe in PROBES {
        errors.push(CodexProbeError::MissingStatus(probe));
        errors.push(CodexProbeError::IncompleteCapture(probe));
        for status in [1, 124, 126, 127, 137, 429] {
            errors.push(CodexProbeError::NonSuccessStatus(probe, status));
        }
        for channel in CHANNELS {
            errors.push(CodexProbeError::InvalidEncoding(probe, channel));
        }
    }
    for capability in [
        CodexCapability::Json,
        CodexCapability::OutputSchema,
        CodexCapability::Sandbox,
    ] {
        errors.push(CodexProbeError::AmbiguousCapabilityEvidence(capability));
    }
    errors
}

#[test]
fn graceful_and_force_killed_timeouts_are_timeout_for_both_probes() {
    for probe in PROBES {
        for termination in TERMINATIONS {
            assert_eq!(
                classify_codex_probe_execution_error(&CodexProbeExecutionError::TimedOut {
                    probe,
                    termination,
                }),
                FailureClass::Timeout
            );
        }
    }
}

#[test]
fn local_workspace_execution_error_is_unknown_not_provider_failure() {
    let class = classify_codex_probe_execution_error(&workspace_error());
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
fn stdout_and_stderr_truncation_are_unknown_for_both_probes() {
    for probe in PROBES {
        for channel in CHANNELS {
            assert_eq!(
                classify_codex_probe_execution_error(&CodexProbeExecutionError::TruncatedStream {
                    probe,
                    channel,
                }),
                FailureClass::Unknown
            );
        }
    }
}

#[test]
fn all_parse_variants_including_adversarial_statuses_are_unknown() {
    for error in parse_errors() {
        assert_eq!(
            classify_codex_probe_execution_error(&CodexProbeExecutionError::Parse(error)),
            FailureClass::Unknown,
            "{error:?}"
        );
    }
}

#[test]
fn misleading_output_in_either_channel_is_unknown_and_output_independent() {
    let ordinary = output_error(OUTPUTS[0], CodexProbeChannel::Stdout);
    assert_eq!(
        classify_codex_probe_execution_error(&ordinary),
        FailureClass::Unknown
    );
    for text in OUTPUTS {
        for channel in CHANNELS {
            let error = output_error(text, channel);
            let class = classify_codex_probe_execution_error(&error);
            assert_eq!(class, FailureClass::Unknown, "{text:?} on {channel:?}");
            assert_eq!(class, classify_codex_probe_execution_error(&ordinary));
        }
    }
}

#[test]
fn all_exercised_inputs_return_only_timeout_or_unknown_with_timeout_only_for_outer_timed_out() {
    let mut errors = vec![workspace_error()];
    errors.extend(
        parse_errors()
            .into_iter()
            .map(CodexProbeExecutionError::Parse),
    );
    for probe in PROBES {
        for termination in TERMINATIONS {
            errors.push(CodexProbeExecutionError::TimedOut { probe, termination });
        }
        for channel in CHANNELS {
            errors.push(CodexProbeExecutionError::TruncatedStream { probe, channel });
        }
    }
    for text in OUTPUTS {
        for channel in CHANNELS {
            errors.push(output_error(text, channel));
        }
    }
    for error in errors {
        let class = classify_codex_probe_execution_error(&error);
        assert!(matches!(
            class,
            FailureClass::Timeout | FailureClass::Unknown
        ));
        assert_eq!(
            class == FailureClass::Timeout,
            matches!(error, CodexProbeExecutionError::TimedOut { .. }),
            "{error:?}"
        );
    }
}

#[test]
fn failure_class_has_twelve_distinct_values_and_unknown_is_first_class() {
    let classes = [
        FailureClass::RateLimited,
        FailureClass::SessionExhausted,
        FailureClass::AuthRequired,
        FailureClass::ProviderDown,
        FailureClass::Timeout,
        FailureClass::SandboxDenied,
        FailureClass::SafetyCheckPending,
        FailureClass::PolicyBlocked,
        FailureClass::RuntimeCrash,
        FailureClass::InvalidOutput,
        FailureClass::UserCancelled,
        FailureClass::Unknown,
    ];
    assert_eq!(classes.len(), 12);
    for (index, class) in classes.iter().enumerate() {
        for other in &classes[index + 1..] {
            assert_ne!(class, other);
        }
    }
    assert_ne!(FailureClass::Unknown, FailureClass::RateLimited);
    assert_ne!(FailureClass::Unknown, FailureClass::AuthRequired);
    assert_ne!(FailureClass::Unknown, FailureClass::ProviderDown);
    assert_ne!(FailureClass::Unknown, FailureClass::PolicyBlocked);
    assert_ne!(FailureClass::Unknown, FailureClass::Timeout);
}
