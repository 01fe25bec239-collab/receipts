//! Bounded Runtime projections of accepted failure sources, without raw payloads.

use std::{
    error::Error,
    fmt,
    mem::{Discriminant, discriminant},
};

use receipts_workspace_execution::execution::{
    ExecutionError, LiveProcessAttemptError, ProcessTermination,
};

use crate::{
    CodexLiveProtocolError, CodexLiveStartError, CodexProbeChannel, CodexProbeError,
    CodexProbeExecutionError, CodexProbeKind, CodexTaskExecutionError, CodexTaskOutputChannel,
    FailureClass, classify_codex_probe_execution_error, classify_codex_task_execution_error,
};

/// Runtime origin of one error observation, not a lifecycle cause or policy decision.
/// Fixed-size tags carry no payload and grant no authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawFailureSource {
    /// One-shot task execution.
    CodexTask,
    /// Capability probing, with the original probe kind where available.
    CodexProbe,
    /// Live request validation or Workspace start failure.
    CodexLiveStart,
    /// Workspace live control/collection failure, never a terminal outcome.
    WorkspaceLive,
    /// Workspace execution error, including live snapshot allocation failure.
    WorkspaceExecution,
    /// Fail-closed Codex protocol interpretation.
    CodexLiveProtocol,
}

/// Fixed-size typed evidence retained by Runtime for classification and inspection.
/// This is source evidence, not another FailureClass vocabulary. Workspace error
/// discriminants preserve exact variant identity without retaining paths, prose,
/// stream strings or error chains. Discriminants are process-local, not wire IDs.
/// No variant authorizes execution, routing or acceptance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawFailureEvidence {
    /// Shared task request rejected an empty prompt.
    EmptyPrompt,
    /// Workspace error variant only; all potentially secret-bearing fields omitted.
    WorkspaceExecution(Discriminant<ExecutionError>),
    /// Existing explicit timeout error, preserving graceful versus forced cleanup.
    TimedOut(ProcessTermination),
    /// Existing task capture failure, with its original stream identity.
    TaskTruncatedStream(CodexTaskOutputChannel),
    /// Existing probe capture failure, with its original stream identity.
    ProbeTruncatedStream(CodexProbeChannel),
    /// Existing payload-free capability parser evidence, including unknown statuses.
    ProbeParse(CodexProbeError),
    /// Completed one-shot process did not report a numeric status.
    MissingExitStatus,
    /// Workspace's single collection was already consumed or in progress.
    AlreadyCollectedOrCollecting,
    /// Workspace controller failed; does not imply RuntimeCrash.
    ControllerFailed,
    /// Original payload-free protocol availability or parse error.
    Protocol(CodexLiveProtocolError),
}

/// One immutable, allocation-free Runtime failure projection for future trait binding.
/// Constructors accept only existing typed errors. Original strings, output, paths,
/// credentials and error chains are neither retained nor formatted. Evidence and the
/// existing conservative FailureClass are independent: no lifecycle outcome, stderr,
/// exit code or unknown provider event is accepted as an error by this API.
/// No admission, routing, credential acquisition or recovery action is implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawFailure {
    source: RawFailureSource,
    evidence: RawFailureEvidence,
    probe: Option<CodexProbeKind>,
    class: FailureClass,
}

impl RawFailure {
    /// Original typed source boundary; no provider semantic inference.
    pub fn origin(&self) -> RawFailureSource {
        self.source
    }
    /// Bounded typed facts; omitted source payloads cannot be recovered here.
    pub fn evidence(&self) -> RawFailureEvidence {
        self.evidence
    }
    /// Probe identity where supplied by the outer execution error. Parser errors
    /// retain their own probe identity, if any, in `evidence()`.
    pub fn probe(&self) -> Option<CodexProbeKind> {
        self.probe
    }
    /// Reuse the accepted task/probe classification, otherwise preserve UNKNOWN.
    /// No new live-lifecycle or protocol mapping is introduced.
    pub fn classify_failure(&self) -> FailureClass {
        self.class
    }

    fn unknown(source: RawFailureSource, evidence: RawFailureEvidence) -> Self {
        Self {
            source,
            evidence,
            probe: None,
            class: FailureClass::Unknown,
        }
    }
}

impl fmt::Display for RawFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Runtime {:?}: {:?}", self.source, self.evidence)
    }
}
// Deliberately no source(): Workspace error chains can contain caller/provider data.
impl Error for RawFailure {}

impl From<&CodexTaskExecutionError> for RawFailure {
    fn from(error: &CodexTaskExecutionError) -> Self {
        use CodexTaskExecutionError::*;
        let evidence = match error {
            EmptyPrompt => RawFailureEvidence::EmptyPrompt,
            WorkspaceExecution { source } => {
                RawFailureEvidence::WorkspaceExecution(discriminant(source))
            }
            TimedOut { termination } => RawFailureEvidence::TimedOut(*termination),
            TruncatedStream { channel } => RawFailureEvidence::TaskTruncatedStream(*channel),
            MissingExitStatus => RawFailureEvidence::MissingExitStatus,
        };
        Self {
            class: classify_codex_task_execution_error(error),
            ..Self::unknown(RawFailureSource::CodexTask, evidence)
        }
    }
}

impl From<&CodexProbeExecutionError> for RawFailure {
    fn from(error: &CodexProbeExecutionError) -> Self {
        use CodexProbeExecutionError::*;
        let (probe, evidence) = match error {
            WorkspaceExecution { probe, source } => (
                Some(*probe),
                RawFailureEvidence::WorkspaceExecution(discriminant(source)),
            ),
            TimedOut { probe, termination } => {
                (Some(*probe), RawFailureEvidence::TimedOut(*termination))
            }
            TruncatedStream { probe, channel } => (
                Some(*probe),
                RawFailureEvidence::ProbeTruncatedStream(*channel),
            ),
            Parse(error) => (None, RawFailureEvidence::ProbeParse(*error)),
        };
        Self {
            probe,
            class: classify_codex_probe_execution_error(error),
            ..Self::unknown(RawFailureSource::CodexProbe, evidence)
        }
    }
}

impl From<&ExecutionError> for RawFailure {
    fn from(error: &ExecutionError) -> Self {
        Self::unknown(
            RawFailureSource::WorkspaceExecution,
            RawFailureEvidence::WorkspaceExecution(discriminant(error)),
        )
    }
}

impl From<&LiveProcessAttemptError> for RawFailure {
    fn from(error: &LiveProcessAttemptError) -> Self {
        let evidence = match error {
            LiveProcessAttemptError::Execution(error) => {
                RawFailureEvidence::WorkspaceExecution(discriminant(error))
            }
            LiveProcessAttemptError::AlreadyCollectedOrCollecting => {
                RawFailureEvidence::AlreadyCollectedOrCollecting
            }
            LiveProcessAttemptError::ControllerFailed => RawFailureEvidence::ControllerFailed,
        };
        Self::unknown(RawFailureSource::WorkspaceLive, evidence)
    }
}

impl From<&CodexLiveStartError> for RawFailure {
    fn from(error: &CodexLiveStartError) -> Self {
        let mut failure = match error {
            CodexLiveStartError::Request(error) => Self::from(error),
            CodexLiveStartError::Workspace(error) => Self::from(error),
        };
        failure.source = RawFailureSource::CodexLiveStart;
        failure
    }
}

impl From<&CodexLiveProtocolError> for RawFailure {
    fn from(error: &CodexLiveProtocolError) -> Self {
        Self::unknown(
            RawFailureSource::CodexLiveProtocol,
            RawFailureEvidence::Protocol(*error),
        )
    }
}
