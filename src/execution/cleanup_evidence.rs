//! Bounded pre-signal evidence acknowledgement. Only trusted composition services
//! this channel. An acknowledgement asserts durability; it never authorizes reset.
use super::{LiveProcessOutput, live_attempt::snapshot};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::time::{Duration, Instant};

/// Maximum additional delay before owned-process emergency cleanup proceeds.
pub const CLEANUP_EVIDENCE_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupEvidenceStatus {
    NotRequested,
    Pending,
    Acknowledged,
    Failed,
}
/// Evidence success is independent of process cleanup, including failed starts
/// and Drop. Retain this report when moving the barrier into an attempt.
#[derive(Debug, Clone)]
pub struct CleanupEvidenceReport(Arc<AtomicU8>);
impl CleanupEvidenceReport {
    pub fn status(&self) -> CleanupEvidenceStatus {
        match self.0.load(Ordering::Acquire) {
            0 => CleanupEvidenceStatus::Pending,
            1 => CleanupEvidenceStatus::Acknowledged,
            _ => CleanupEvidenceStatus::Failed,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupEvidenceReason {
    Cancellation,
    Timeout,
    NaturalCompletion,
    SetupFailure,
    ControllerFault,
}
/// A single-use, attempt-bound acknowledgement. Dropping it, a full channel,
/// disconnection, or a late acknowledgement cannot postpone emergency cleanup.
pub struct CleanupEvidenceRequest {
    pub reason: CleanupEvidenceReason,
    pub output: LiveProcessOutput,
    deadline: Instant,
    acknowledgement: SyncSender<()>,
}
impl CleanupEvidenceRequest {
    /// Call only after the exact request's evidence is durable. False means the
    /// pre-action ordering guarantee was lost; retain evidence and reconcile.
    pub fn acknowledge(self) -> bool {
        Instant::now() < self.deadline && self.acknowledgement.try_send(()).is_ok()
    }
}
pub struct CleanupEvidenceBarrier {
    sender: SyncSender<CleanupEvidenceRequest>,
    report: CleanupEvidenceReport,
}
impl CleanupEvidenceBarrier {
    pub fn channel() -> (Self, Receiver<CleanupEvidenceRequest>) {
        let (sender, receiver) = mpsc::sync_channel(1);
        (
            Self {
                sender,
                report: CleanupEvidenceReport(Arc::new(AtomicU8::new(0))),
            },
            receiver,
        )
    }
    pub fn report(&self) -> CleanupEvidenceReport {
        self.report.clone()
    }
    pub(super) fn acknowledge(
        self,
        reason: CleanupEvidenceReason,
        stdout: &super::live_attempt::Retention,
        stderr: &super::live_attempt::Retention,
    ) {
        let deadline = Instant::now() + CLEANUP_EVIDENCE_TIMEOUT;
        let acknowledged = snapshot(stdout, stderr, false).ok().is_some_and(|output| {
            let (acknowledgement, receipt) = mpsc::sync_channel(1);
            self.sender
                .try_send(CleanupEvidenceRequest {
                    reason,
                    output,
                    deadline,
                    acknowledgement,
                })
                .is_ok()
                && receipt
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .is_ok()
                && Instant::now() < deadline
        });
        self.report
            .0
            .store(if acknowledged { 1 } else { 2 }, Ordering::Release);
    }
}

impl Drop for CleanupEvidenceBarrier {
    fn drop(&mut self) {
        let _ = self
            .report
            .0
            .compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire);
    }
}
