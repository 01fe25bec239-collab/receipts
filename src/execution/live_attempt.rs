//! Live, locally owned attempts. The lifecycle mutex is the only terminal-claim authority.
//! Completion includes reader EOF and the accepted owned-group completion proof.
//! Dropping an uncollected handle requests cancellation and waits for bounded cleanup.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use super::{CapturedStream, ExecutionError, ProcessRunRequest, ProcessTimeoutPolicy};

/// Why the attempt ended, independently of how its group was cleaned up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveProcessTerminalCause {
    Completed,
    Cancelled,
    TimedOut,
}

/// Only the first cancellation of a running attempt is accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveProcessCancelAcceptance {
    CancelAccepted,
    AlreadyTerminalOrTerminating,
}

/// A failed terminal collection is also consumed; cleanup is never retried by collecting.
#[derive(Debug)]
pub enum LiveProcessAttemptError {
    Execution(ExecutionError),
    AlreadyCollectedOrCollecting,
    ControllerFailed,
}

impl fmt::Display for LiveProcessAttemptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Execution(error) => error.fmt(f),
            Self::AlreadyCollectedOrCollecting => write!(f, "attempt collection already started"),
            Self::ControllerFailed => write!(f, "attempt controller did not finish normally"),
        }
    }
}
impl std::error::Error for LiveProcessAttemptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Execution(error) => Some(error),
            _ => None,
        }
    }
}
impl From<ExecutionError> for LiveProcessAttemptError {
    fn from(error: ExecutionError) -> Self {
        Self::Execution(error)
    }
}

/// Separate bounded stream snapshots. Counts describe bytes drained at snapshot time;
/// the two pipes have no shared byte ordering. Polling remains legal after collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveProcessOutput {
    stdout: CapturedStream,
    stderr: CapturedStream,
}
impl LiveProcessOutput {
    pub fn stdout(&self) -> &CapturedStream {
        &self.stdout
    }
    pub fn stderr(&self) -> &CapturedStream {
        &self.stderr
    }
}

/// Immutable terminal evidence, constructible only after verified reap and reader joins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveProcessOutcome {
    // Completed cannot carry a forced-kill bit, and stopped outcomes cannot carry success.
    termination: TerminalEvidence,
    exit_code: Option<i32>,
    output: LiveProcessOutput,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum TerminalEvidence {
    Completed { success: bool },
    Cancelled { forced: bool },
    TimedOut { forced: bool },
}
impl LiveProcessOutcome {
    pub fn terminal_cause(&self) -> LiveProcessTerminalCause {
        match self.termination {
            TerminalEvidence::Completed { .. } => LiveProcessTerminalCause::Completed,
            TerminalEvidence::Cancelled { .. } => LiveProcessTerminalCause::Cancelled,
            TerminalEvidence::TimedOut { .. } => LiveProcessTerminalCause::TimedOut,
        }
    }
    pub fn forced_kill_required(&self) -> bool {
        match self.termination {
            TerminalEvidence::Completed { .. } => false,
            TerminalEvidence::Cancelled { forced } | TerminalEvidence::TimedOut { forced } => {
                forced
            }
        }
    }
    pub fn success(&self) -> bool {
        matches!(
            self.termination,
            TerminalEvidence::Completed { success: true }
        )
    }
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
    pub fn stdout(&self) -> &CapturedStream {
        self.output.stdout()
    }
    pub fn stderr(&self) -> &CapturedStream {
        self.output.stderr()
    }
}

#[derive(Debug, Default)]
struct Lifecycle {
    cause: Option<LiveProcessTerminalCause>,
    closed: bool,
}
impl Lifecycle {
    fn claim(&mut self, cause: LiveProcessTerminalCause) -> bool {
        if self.closed || self.cause.is_some() {
            return false;
        }
        self.cause = Some(cause);
        true
    }
}

// No caller-provided code runs under these locks. Recovering poisoning permits
// controller-unwind cleanup and preserves any cause already committed.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

type Retention = Arc<Mutex<super::capture::BoundedStreamRetention>>;
type Controller = JoinHandle<Result<LiveProcessOutcome, ExecutionError>>;

/// One non-cloneable typed ownership handle; no raw child or signal capability escapes.
/// Methods take shared references so scoped threads can cancel while another collects.
/// Collection is globally single-use, including errors and concurrent collection attempts.
/// Drop cancels and joins an uncollected attempt; use `wait_collect` to observe failures.
#[derive(Debug)]
pub struct LiveProcessAttempt {
    lifecycle: Arc<Mutex<Lifecycle>>,
    stdout: Retention,
    stderr: Retention,
    controller: Mutex<Option<Controller>>,
}
impl LiveProcessAttempt {
    /// Bounded copies (at most the existing per-stream capture limit each).
    /// Valid while running, terminating, terminal, and after collection.
    pub fn snapshot_output(&self) -> Result<LiveProcessOutput, ExecutionError> {
        snapshot(&self.stdout, &self.stderr)
    }
    /// Claim cancellation; cleanup runs exactly once on the owning controller.
    pub fn cancel(&self) -> LiveProcessCancelAcceptance {
        if lock(&self.lifecycle).claim(LiveProcessTerminalCause::Cancelled) {
            LiveProcessCancelAcceptance::CancelAccepted
        } else {
            LiveProcessCancelAcceptance::AlreadyTerminalOrTerminating
        }
    }
    /// The immutable winning cause, once claimed. A cause is not a cleanup-success proof.
    /// Control failure before a cause is claimed may leave this `None`.
    pub fn terminal_cause(&self) -> Option<LiveProcessTerminalCause> {
        lock(&self.lifecycle).cause
    }
    /// Wait for execution/cleanup and verified reader joins. The explicit policy bounds
    /// execution; cleanup uses the accepted grace and private verification windows.
    pub fn wait_collect(&self) -> Result<LiveProcessOutcome, LiveProcessAttemptError> {
        let controller = lock(&self.controller)
            .take()
            .ok_or(LiveProcessAttemptError::AlreadyCollectedOrCollecting)?;
        controller
            .join()
            .map_err(|_| LiveProcessAttemptError::ControllerFailed)?
            .map_err(Into::into)
    }
}
impl Drop for LiveProcessAttempt {
    fn drop(&mut self) {
        self.cancel();
        if let Some(controller) = lock(&self.controller).take() {
            let _ = controller.join();
        }
    }
}

fn snapshot(stdout: &Retention, stderr: &Retention) -> Result<LiveProcessOutput, ExecutionError> {
    let copy = |retention: &Retention, stream| {
        lock(retention)
            .snapshot()
            .map_err(|error| super::runner::retention_allocation_failed(stream, error))
    };
    Ok(LiveProcessOutput {
        stdout: copy(stdout, "stdout")?,
        stderr: copy(stderr, "stderr")?,
    })
}

/// Validates and starts a real attempt with explicit orchestrator-owned timeout and grace.
/// No default timeout, shell, inherited environment, or process-control authority is added.
pub fn start_live_process_attempt(
    request: &ProcessRunRequest,
    policy: &ProcessTimeoutPolicy,
) -> Result<LiveProcessAttempt, LiveProcessAttemptError> {
    #[cfg(unix)]
    {
        platform::start(request, policy)
    }
    #[cfg(not(unix))]
    {
        let _ = (request, policy);
        Err(ExecutionError::UnsupportedTimeoutPlatform.into())
    }
}

#[cfg(test)]
pub(super) struct TestGate {
    pub reached: std::sync::mpsc::SyncSender<()>,
    pub release: std::sync::mpsc::Receiver<()>,
}
#[cfg(test)]
impl TestGate {
    fn wait(self) {
        self.reached.send(()).unwrap();
        self.release
            .recv_timeout(std::time::Duration::from_secs(15))
            .unwrap();
    }
}
#[cfg(test)]
thread_local! {
    pub(super) static CONTROLLER_FAULTS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    pub(super) static BEFORE_MONITOR: std::cell::RefCell<Option<TestGate>> = const { std::cell::RefCell::new(None) };
    pub(super) static AFTER_CLAIM: std::cell::RefCell<Option<TestGate>> = const { std::cell::RefCell::new(None) };
}

#[cfg(unix)]
mod platform {
    use super::super::runner;
    use super::super::unix_signal::OwnedProcessGroup;
    use super::*;
    use std::io::{ErrorKind, Read};
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    const POLL: Duration = Duration::from_millis(10);
    const READER_VERIFY: Duration = Duration::from_secs(2);
    type Reader = JoinHandle<Result<(), ExecutionError>>;

    struct Resources {
        child: Option<Child>,
        group: OwnedProcessGroup,
        readers: Vec<(&'static str, Reader)>,
    }
    impl Resources {
        fn cleanup(&mut self) -> Result<(), ExecutionError> {
            if let Some(child) = self.child.take() {
                runner::cleanup_owned_attempt(child, self.group)?;
            }
            Ok(())
        }
        fn join_readers(&mut self) -> Result<(), ExecutionError> {
            let started = Instant::now();
            while self.readers.iter().any(|(_, reader)| !reader.is_finished()) {
                if started.elapsed() >= READER_VERIFY {
                    return Err(ExecutionError::CaptureReaderFailed {
                        stream: self
                            .readers
                            .iter()
                            .find(|(_, r)| !r.is_finished())
                            .unwrap()
                            .0,
                        detail: "live reader EOF verification exceeded two seconds".into(),
                    });
                }
                std::thread::sleep(POLL);
            }
            let mut failure = None;
            for (stream, reader) in self.readers.drain(..) {
                let result = reader.join().unwrap_or_else(|panic| {
                    Err(ExecutionError::CaptureReaderFailed {
                        stream,
                        detail: runner::reader_panic_detail(panic.as_ref()),
                    })
                });
                if failure.is_none() {
                    failure = result.err();
                }
            }
            failure.map_or(Ok(()), Err)
        }
    }
    impl Drop for Resources {
        fn drop(&mut self) {
            let _ = self.cleanup();
            let _ = self.join_readers();
        }
    }
    struct CloseLifecycle(Arc<Mutex<Lifecycle>>);
    impl Drop for CloseLifecycle {
        fn drop(&mut self) {
            lock(&self.0).closed = true;
        }
    }

    fn spawn_reader(
        stream: &'static str,
        mut source: impl Read + Send + 'static,
        retention: Retention,
    ) -> Result<Reader, ExecutionError> {
        std::thread::Builder::new()
            .name(format!("receipts-live-{stream}"))
            .spawn(move || {
                let mut buffer = [0u8; 32 * 1024];
                loop {
                    match source.read(&mut buffer) {
                        Ok(0) => return Ok(()),
                        Ok(n) => lock(&retention)
                            .push(&buffer[..n])
                            .map_err(|fault| runner::capture_fault_failed(stream, fault))?,
                        Err(error) if error.kind() == ErrorKind::Interrupted => {}
                        Err(error) => {
                            return Err(ExecutionError::CaptureReadFailed {
                                stream,
                                detail: error.to_string(),
                            });
                        }
                    }
                }
            })
            .map_err(|error| ExecutionError::CaptureReaderStartFailed {
                stream,
                detail: error.to_string(),
            })
    }

    fn setup(
        command: &mut Command,
        stdout: Retention,
        stderr: Retention,
    ) -> Result<Resources, ExecutionError> {
        let child = command.spawn().map_err(runner::spawn_failed)?;
        let Some(group) = runner::owned_process_group(child.id()) else {
            return Err(runner::process_group_ownership_failed(child));
        };
        let mut resources = Resources {
            child: Some(child),
            group,
            readers: Vec::with_capacity(2),
        };
        let setup = (|| {
            let child = resources.child.as_mut().unwrap();
            let out = child
                .stdout
                .take()
                .ok_or(ExecutionError::CaptureStreamUnavailable {
                    stream: "stdout",
                    detail: "missing pipe".into(),
                })?;
            let err = child
                .stderr
                .take()
                .ok_or(ExecutionError::CaptureStreamUnavailable {
                    stream: "stderr",
                    detail: "missing pipe".into(),
                })?;
            resources
                .readers
                .push(("stdout", spawn_reader("stdout", out, stdout)?));
            resources
                .readers
                .push(("stderr", spawn_reader("stderr", err, stderr)?));
            Ok(())
        })();
        if let Err(error) = setup {
            resources.cleanup()?;
            resources.join_readers()?;
            return Err(error);
        }
        Ok(resources)
    }

    pub(super) fn start(
        request: &ProcessRunRequest,
        policy: &ProcessTimeoutPolicy,
    ) -> Result<LiveProcessAttempt, LiveProcessAttemptError> {
        runner::ensure_timeout_platform_supported()?;
        let deadline = runner::validated_run_deadline(policy)?;
        let executable = runner::validated_executable(request.executable())?;
        let cwd = runner::validated_workspace_cwd(request.workspace_root(), request.cwd())?;
        let stdout = Arc::new(Mutex::new(runner::frozen_retention("stdout")?));
        let stderr = Arc::new(Mutex::new(runner::frozen_retention("stderr")?));
        let mut command = runner::prepared_command(&executable, &cwd);
        command
            .args(request.arguments())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let lifecycle = Arc::new(Mutex::new(Lifecycle::default()));
        let (out, err, state, policy) =
            (stdout.clone(), stderr.clone(), lifecycle.clone(), *policy);
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        #[cfg(test)]
        let before_monitor = BEFORE_MONITOR.take();
        #[cfg(test)]
        let faults = CONTROLLER_FAULTS.replace(0);
        #[cfg(test)]
        let after_claim = AFTER_CLAIM.take();
        let controller = std::thread::Builder::new()
            .name("receipts-live-controller".into())
            .spawn(move || {
                #[cfg(test)]
                AFTER_CLAIM.set(after_claim);
                #[cfg(test)]
                let _faults = runner::inject_capture_test_faults(faults);
                let _closed = CloseLifecycle(state.clone());
                let mut resources = match setup(&mut command, out.clone(), err.clone()) {
                    Ok(resources) => resources,
                    Err(error) => {
                        let _ = ready.send(false);
                        return Err(error);
                    }
                };
                let _ = ready.send(true);
                #[cfg(test)]
                if let Some(gate) = before_monitor {
                    gate.wait();
                }
                let result = monitor(&mut resources, &state, deadline, &policy);
                lock(&state).closed = true;
                // Control errors outrank capture errors. Never signal after a completed reap.
                let cleanup = resources.cleanup();
                let readers = resources.join_readers();
                cleanup?;
                let (termination, exit_code) = result?;
                readers?;
                Ok(LiveProcessOutcome {
                    termination,
                    exit_code,
                    output: snapshot(&out, &err)?,
                })
            })
            .map_err(|error| ExecutionError::ProcessSpawnFailed {
                detail: format!("live controller thread: {error}"),
            })?;
        match started.recv() {
            Ok(true) => Ok(LiveProcessAttempt {
                lifecycle,
                stdout,
                stderr,
                controller: Mutex::new(Some(controller)),
            }),
            _ => match controller.join() {
                Ok(Err(error)) => Err(error.into()),
                _ => Err(LiveProcessAttemptError::ControllerFailed),
            },
        }
    }

    fn monitor(
        resources: &mut Resources,
        lifecycle: &Mutex<Lifecycle>,
        deadline: Instant,
        policy: &ProcessTimeoutPolicy,
    ) -> Result<(TerminalEvidence, Option<i32>), ExecutionError> {
        loop {
            let mut state = lock(lifecycle);
            // Cancel, deadline, and natural completion all linearize under this mutex.
            // Deadline eligibility alone cannot override a prior claim.
            if Instant::now() >= deadline {
                state.claim(LiveProcessTerminalCause::TimedOut);
            }
            if let Some(cause) = state.cause {
                drop(state);
                #[cfg(test)]
                if let Some(gate) = AFTER_CLAIM.take() {
                    gate.wait();
                }
                let (status, forced) = runner::terminate_owned_attempt(
                    resources.child.take().unwrap(),
                    resources.group,
                    policy,
                )?;
                let evidence = match cause {
                    LiveProcessTerminalCause::Cancelled => TerminalEvidence::Cancelled { forced },
                    LiveProcessTerminalCause::TimedOut => TerminalEvidence::TimedOut { forced },
                    LiveProcessTerminalCause::Completed => {
                        unreachable!("completed returns directly")
                    }
                };
                return Ok((evidence, status.code()));
            }
            if resources
                .readers
                .iter()
                .all(|(_, reader)| reader.is_finished())
            {
                resources.join_readers()?;
                match runner::try_complete_owned_child(
                    resources.child.as_mut().unwrap(),
                    resources.group,
                    deadline,
                ) {
                    Ok(Some(status)) => {
                        resources.child.take(); // Already reaped; never signal this PGID again.
                        assert!(state.claim(LiveProcessTerminalCause::Completed));
                        drop(state);
                        #[cfg(test)]
                        if let Some(gate) = AFTER_CLAIM.take() {
                            gate.wait();
                        }
                        return Ok((
                            TerminalEvidence::Completed {
                                success: status.success(),
                            },
                            status.code(),
                        ));
                    }
                    Ok(None) => {}
                    Err(runner::OwnedWaitError::Owned(error)) => return Err(error),
                    Err(runner::OwnedWaitError::NoFurtherSignal(error)) => {
                        resources.child.take();
                        return Err(error);
                    }
                }
            } else if resources
                .readers
                .iter()
                .any(|(_, reader)| reader.is_finished())
            {
                // Inspect a finished reader promptly, so a read failure never waits for
                // an arbitrarily long user deadline while the other pipe is active.
                let index = resources
                    .readers
                    .iter()
                    .position(|(_, r)| r.is_finished())
                    .unwrap();
                let (stream, reader) = resources.readers.swap_remove(index);
                reader
                    .join()
                    .map_err(|panic| ExecutionError::CaptureReaderFailed {
                        stream,
                        detail: runner::reader_panic_detail(panic.as_ref()),
                    })??;
            }
            drop(state);
            std::thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
        }
    }
}

#[cfg(test)]
pub(super) fn collection_started_for_test(attempt: &LiveProcessAttempt) -> bool {
    lock(&attempt.controller).is_none()
}

#[cfg(test)]
#[test]
fn live_claim_authority_rejects_every_later_cause() {
    use LiveProcessTerminalCause::*;
    for first in [Completed, Cancelled, TimedOut] {
        let mut state = Lifecycle::default();
        assert!(state.claim(first));
        for later in [Completed, Cancelled, TimedOut] {
            assert!(!state.claim(later));
            assert_eq!(state.cause, Some(first));
        }
    }
}
