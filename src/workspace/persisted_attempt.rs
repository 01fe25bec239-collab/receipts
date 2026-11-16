//! Opt-in lifecycle driver. Persistence runs outside the process controller;
//! cleanup waits at most the bounded acknowledgement window for the sink.
use crate::{
    AttemptEvidenceSinkV1, WorkspaceCheckpointEvidenceCaptureRequest, WorkspaceCheckpointKind,
    WorkspaceDateTimeV1, WorkspaceHandle,
    checkpoint_writer::{CheckpointWriteError, capture_and_persist},
    execution::{
        CleanupEvidenceBarrier, CleanupEvidenceStatus, ExecutionError, LiveProcessOutcome,
        LiveProcessPreparedCancelAcceptance, ProcessRunRequest, ProcessTimeoutPolicy,
    },
    lifecycle::{
        WorkspaceCollectionFailure, WorkspaceLifecycleError,
        start_workspace_attempt_with_cleanup_evidence,
    },
    output_store::store_output,
};
use receipts_state::{OutputRef, StateError, TrustedClockV1};
use std::{
    path::Path,
    sync::mpsc::{Receiver, TryRecvError},
    time::{Duration, Instant},
};

pub struct PersistedAttemptRequest<'a> {
    pub handle: &'a WorkspaceHandle,
    pub process: &'a ProcessRunRequest,
    pub timeout: &'a ProcessTimeoutPolicy,
    pub project_id: &'a str,
    pub attempt_id: &'a str,
    pub output_directory: &'a Path,
    pub interval: Duration,
    pub clock: &'a dyn TrustedClockV1,
    pub cancellation: &'a Receiver<()>,
}
#[derive(Debug)]
pub enum PersistedAttemptError {
    InvalidInterval,
    Lifecycle {
        error: WorkspaceLifecycleError,
        cleanup_evidence: CleanupEvidenceStatus,
    },
    /// Evidence failed independently of the actual owned-process cleanup result.
    EvidenceFailure {
        source: Box<PersistedAttemptError>,
        cleanup: Result<Box<LiveProcessOutcome>, WorkspaceCollectionFailure>,
    },
    CleanupAcknowledgementFailed,
    Checkpoint(CheckpointWriteError),
    State(StateError),
    Output(std::io::Error),
    Execution(ExecutionError),
    Collection(WorkspaceCollectionFailure),
    InvalidTime,
    Git(crate::WorkspaceError),
}
pub struct PersistedAttemptOutcome<R> {
    pub terminal: LiveProcessOutcome,
    pub finish_receipt: R,
    pub cancellation: Option<LiveProcessPreparedCancelAcceptance>,
}
impl PersistedAttemptRequest<'_> {
    fn persist<S: AttemptEvidenceSinkV1>(
        &self,
        sink: &mut S,
        sequence: u64,
        kind: WorkspaceCheckpointKind,
        streams: &[(&str, &crate::execution::CapturedStream)],
    ) -> Result<S::Receipt, PersistedAttemptError> {
        crate::git::refuse_untrusted_repository_config(
            self.handle.worktree_path(),
            "checkpoint dirty diff",
        )
        .map_err(PersistedAttemptError::Git)?;
        let diff = crate::git::capture(
            self.handle.worktree_path(),
            "checkpoint dirty diff",
            &[
                std::ffi::OsStr::new("diff"),
                std::ffi::OsStr::new("--binary"),
                std::ffi::OsStr::new("--no-ext-diff"),
                std::ffi::OsStr::new("--no-textconv"),
                std::ffi::OsStr::new("HEAD"),
                std::ffi::OsStr::new("--"),
            ],
        )
        .map_err(PersistedAttemptError::Git)?
        .require_success("checkpoint dirty diff")
        .map_err(PersistedAttemptError::Git)?;
        let (diff_digest, _) = store_output(self.output_directory, diff.raw_stdout())
            .map_err(PersistedAttemptError::Output)?;
        let diff_id = format!("{}:{sequence}:dirty-diff", self.attempt_id);
        let diff_output = OutputRef::new(
            self.project_id,
            &diff_id,
            &diff_digest,
            self.output_directory
                .join(&diff_digest)
                .to_str()
                .ok_or(PersistedAttemptError::InvalidTime)?,
            &diff.raw_stdout().len().to_string(),
        )
        .map_err(PersistedAttemptError::State)?;
        let mut outputs = vec![diff_output];
        let dirty_diff_ref = crate::WorkspaceCheckpointRef::new(
            crate::WorkspaceCheckpointRefType::ArtifactId,
            diff_id,
            Some(diff_digest),
            None,
        )
        .map_err(|_| PersistedAttemptError::InvalidTime)?;
        for (name, stream) in streams {
            let mut bytes = stream.head().to_vec();
            bytes.extend_from_slice(stream.tail());
            let (digest, path) = store_output(self.output_directory, &bytes)
                .map_err(PersistedAttemptError::Output)?;
            let reference = format!("{}:{sequence}:{name}", self.attempt_id);
            outputs.push(
                OutputRef::new(
                    self.project_id,
                    &reference,
                    &digest,
                    path.to_str().ok_or(PersistedAttemptError::InvalidTime)?,
                    &bytes.len().to_string(),
                )
                .map_err(PersistedAttemptError::State)?,
            );
        }
        let sample = self.clock.sample().map_err(PersistedAttemptError::State)?;
        let timestamp = WorkspaceDateTimeV1::try_new(sample.canonical_utc_timestamp)
            .map_err(|_| PersistedAttemptError::InvalidTime)?;
        capture_and_persist(
            sink,
            self.project_id,
            WorkspaceCheckpointEvidenceCaptureRequest {
                directory: self.handle.worktree_path(),
                checkpoint_id: format!("{}-{sequence}", self.attempt_id),
                workspace_id: self.handle.workspace_id().into(),
                task_id: self.handle.task_id().map(str::to_owned),
                attempt_id: Some(self.attempt_id.into()),
                kind,
                base_sha: Some(self.handle.base_sha().as_str()),
                dirty_diff_ref: Some(dirty_diff_ref),
                executed_checks: vec![],
            },
            timestamp,
            &outputs,
        )
        .map_err(PersistedAttemptError::Checkpoint)
    }
}

/// Starts only after the start checkpoint is durable. Cancellation competes for
/// a terminal claim; the controller then requests evidence before signalling.
/// Every automatic cleanup also requests a durable checkpoint before signalling.
/// Missing/late acknowledgement triggers bounded emergency cleanup; errors retain
/// the independent cleanup result and never claim durable pre-action evidence.
pub fn run_persisted_attempt<S: AttemptEvidenceSinkV1>(
    request: PersistedAttemptRequest<'_>,
    sink: &mut S,
) -> Result<PersistedAttemptOutcome<S::Receipt>, PersistedAttemptError> {
    if request.interval.is_zero() {
        return Err(PersistedAttemptError::InvalidInterval);
    }
    request.persist(sink, 0, WorkspaceCheckpointKind::Progress, &[])?;
    let (barrier, cleanup_requests) = CleanupEvidenceBarrier::channel();
    let report = barrier.report();
    let (_, attempt) = start_workspace_attempt_with_cleanup_evidence(
        request.handle,
        request.process,
        request.timeout,
        barrier,
    )
    .map_err(|error| PersistedAttemptError::Lifecycle {
        error,
        cleanup_evidence: report.status(),
    })?;
    let mut sequence = 1;
    let mut last = Instant::now();
    let mut cancellation = None;
    let mut cancellation_open = true;
    let driven = (|| {
        while !attempt.is_terminating_or_closed()
            || report.status() == CleanupEvidenceStatus::Pending
        {
            if let Ok(cleanup) = cleanup_requests.try_recv() {
                request.persist(
                    sink,
                    sequence,
                    WorkspaceCheckpointKind::PreTermination,
                    &[
                        ("stdout", cleanup.output.stdout()),
                        ("stderr", cleanup.output.stderr()),
                    ],
                )?;
                sequence += 1;
                if !cleanup.acknowledge() {
                    return Err(PersistedAttemptError::CleanupAcknowledgementFailed);
                }
            }
            if cancellation_open {
                match request.cancellation.try_recv() {
                    Ok(()) => {
                        cancellation_open = false;
                        if let Some(prepared) = attempt
                            .prepare_cancellation()
                            .map_err(PersistedAttemptError::Execution)?
                        {
                            cancellation = Some(attempt.cancel_prepared(&prepared));
                        }
                    }
                    Err(TryRecvError::Disconnected) => cancellation_open = false,
                    Err(TryRecvError::Empty) => {}
                }
            }
            if last.elapsed() >= request.interval && !attempt.is_terminating_or_closed() {
                let output = attempt
                    .snapshot_output()
                    .map_err(PersistedAttemptError::Execution)?;
                request.persist(
                    sink,
                    sequence,
                    WorkspaceCheckpointKind::Progress,
                    &[("stdout", output.stdout()), ("stderr", output.stderr())],
                )?;
                sequence += 1;
                last = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if report.status() != CleanupEvidenceStatus::Acknowledged {
            return Err(PersistedAttemptError::CleanupAcknowledgementFailed);
        }
        Ok(())
    })();
    // Disconnect first: a failed sink cannot add another acknowledgement wait.
    drop(cleanup_requests);
    if let Err(source) = driven {
        attempt.cancel();
        return Err(PersistedAttemptError::EvidenceFailure {
            source: Box::new(source),
            cleanup: attempt.wait_collect().map(Box::new),
        });
    }
    let terminal = attempt
        .wait_collect()
        .map_err(PersistedAttemptError::Collection)?;
    let finish_receipt = match request.persist(
        sink,
        sequence,
        WorkspaceCheckpointKind::Progress,
        &[("stdout", terminal.stdout()), ("stderr", terminal.stderr())],
    ) {
        Ok(receipt) => receipt,
        Err(source) => {
            return Err(PersistedAttemptError::EvidenceFailure {
                source: Box::new(source),
                cleanup: Ok(Box::new(terminal)),
            });
        }
    };
    Ok(PersistedAttemptOutcome {
        terminal,
        finish_receipt,
        cancellation,
    })
}
