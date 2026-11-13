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

/// Start failure is distinct from collection of an already-created attempt.
#[non_exhaustive]
pub enum LiveProcessStartError {
    Execution(ExecutionError),
    TerminalBeforeReady(Box<LiveProcessOutcome>),
    ControllerFailed,
}
impl fmt::Debug for LiveProcessStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Execution(error) => f
                .debug_tuple("Execution")
                .field(&std::mem::discriminant(error))
                .finish(),
            Self::ControllerFailed => f.write_str("ControllerFailed"),
            Self::TerminalBeforeReady(outcome) => f
                .debug_struct("TerminalBeforeReady")
                .field("terminal_cause", &outcome.terminal_cause())
                .field("forced_kill_required", &outcome.forced_kill_required())
                .field("process_success", &outcome.success())
                .field("exit_code", &outcome.exit_code())
                .field("stdout_bytes", &outcome.stdout().total_bytes())
                .field("stderr_bytes", &outcome.stderr().total_bytes())
                .finish(),
        }
    }
}
impl fmt::Display for LiveProcessStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for LiveProcessStartError {}
impl From<ExecutionError> for LiveProcessStartError {
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

/// An attempt-bound bounded snapshot taken before requesting explicit cancel.
/// Holding this value never delays timeout, Drop, completion or fault cleanup.
/// It proves no durable write and grants no workspace reset authority.
/// Snapshot each output artifact honestly: running captures have no full digest.
pub struct LiveProcessCancellationPreparation {
    lifecycle: Arc<Mutex<Lifecycle>>,
    output: LiveProcessOutput,
    deadline: std::time::Instant,
}
impl LiveProcessCancellationPreparation {
    pub fn output(&self) -> &LiveProcessOutput {
        &self.output
    }
    pub fn deadline(&self) -> std::time::Instant {
        self.deadline
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveProcessPreparedCancelAcceptance {
    CancelAccepted,
    AlreadyTerminalOrTerminating,
    PreparationExpired,
    ForeignAttempt,
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
    ready: bool,
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
    deadline: std::time::Instant,
}
impl LiveProcessAttempt {
    /// Bounded copies (at most the existing per-stream capture limit each).
    /// Valid while running, terminating, terminal, and after collection.
    pub fn snapshot_output(&self) -> Result<LiveProcessOutput, ExecutionError> {
        snapshot(&self.stdout, &self.stderr, false)
    }
    /// Opt-in split boundary: capture and persist outside the lifecycle lock,
    /// then call cancel_prepared. None means terminal/deadline already won.
    /// External persistence may fail or stall; the controller never waits on it.
    pub fn prepare_cancellation(
        &self,
    ) -> Result<Option<LiveProcessCancellationPreparation>, ExecutionError> {
        let available = || {
            let state = lock(&self.lifecycle);
            !state.closed && state.cause.is_none() && std::time::Instant::now() < self.deadline
        };
        if !available() {
            return Ok(None);
        }
        let output = self.snapshot_output()?;
        if !available() {
            return Ok(None);
        }
        Ok(Some(LiveProcessCancellationPreparation {
            lifecycle: self.lifecycle.clone(),
            output,
            deadline: self.deadline,
        }))
    }
    /// Requests explicit cancellation only for this attempt's unexpired capture.
    /// A lost race invalidates pre-cleanup ordering; retain the capture as evidence
    /// and reconcile. The caller reports persistence failure separately and may
    /// still request cancel; persistence never controls owned-process cleanup.
    pub fn cancel_prepared(
        &self,
        preparation: &LiveProcessCancellationPreparation,
    ) -> LiveProcessPreparedCancelAcceptance {
        use LiveProcessPreparedCancelAcceptance as A;
        if !Arc::ptr_eq(&self.lifecycle, &preparation.lifecycle) {
            return A::ForeignAttempt;
        }
        let mut state = lock(&self.lifecycle);
        if state.closed || state.cause.is_some() {
            return A::AlreadyTerminalOrTerminating;
        }
        if std::time::Instant::now() >= preparation.deadline {
            return A::PreparationExpired;
        }
        if state.claim(LiveProcessTerminalCause::Cancelled) {
            A::CancelAccepted
        } else {
            A::AlreadyTerminalOrTerminating
        }
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
    /// True once a terminal claim exists or the controller closed after a fault.
    /// This is an observation only; collection still verifies cleanup and readers.
    pub fn is_terminating_or_closed(&self) -> bool {
        let state = lock(&self.lifecycle);
        state.closed || state.cause.is_some()
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

fn snapshot(
    stdout: &Retention,
    stderr: &Retention,
    complete: bool,
) -> Result<LiveProcessOutput, ExecutionError> {
    let copy = |retention: &Retention, stream| {
        let retention = lock(retention);
        (if complete {
            retention.complete_snapshot()
        } else {
            retention.snapshot()
        })
        .map_err(|error| super::runner::retention_allocation_failed(stream, error))
    };
    Ok(LiveProcessOutput {
        stdout: copy(stdout, "stdout")?,
        stderr: copy(stderr, "stderr")?,
    })
}

/// Validates and starts a real attempt with explicit orchestrator-owned timeout and grace.
/// No default timeout, shell, inherited environment, or process-control authority is added.
/// Closed stdin retains immediate startup. Bytes require exact initial delivery,
/// successful close, and a READY commit under the terminal-claim mutex. If a
/// terminal cause wins first, bounded cleanup completes before returning
/// `LiveProcessStartError::TerminalBeforeReady`; no attempt handle escapes.
pub fn start_live_process_attempt(
    request: &ProcessRunRequest,
    policy: &ProcessTimeoutPolicy,
) -> Result<LiveProcessAttempt, LiveProcessStartError> {
    start_customized_attempt(request, policy, LaunchCustomization::default())
}

/// Crate-private containment for protected launches: kill every member and
/// prove emptiness. Runs during cleanup after process-group handling and
/// before reader verification; it is a cleanup step, never a second timer.
pub(crate) trait AttemptContainment: Send + Sync {
    fn kill_and_verify_empty(&self) -> Result<(), String>;
}

/// Async-signal-safe child setup run after fork, before exec.
pub(crate) type PreExecHook = Box<dyn FnMut() -> std::io::Result<()> + Send + Sync>;

/// Crate-private launch customization. The default is the public behavior.
#[derive(Default)]
pub(crate) struct LaunchCustomization {
    pub(crate) pre_exec: Option<PreExecHook>,
    pub(crate) containment: Option<Arc<dyn AttemptContainment>>,
}

pub(crate) fn start_customized_attempt(
    request: &ProcessRunRequest,
    policy: &ProcessTimeoutPolicy,
    customization: LaunchCustomization,
) -> Result<LiveProcessAttempt, LiveProcessStartError> {
    #[cfg(unix)]
    {
        platform::start(request, policy, customization)
    }
    #[cfg(not(unix))]
    {
        let _ = (request, policy, customization);
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
type ReaderObservation = Box<
    dyn FnOnce(&[(&'static str, JoinHandle<Result<(), ExecutionError>>)], &mut std::time::Instant)
        + Send,
>;
#[cfg(test)]
thread_local! {
    pub(super) static READER_EOF_GATES: std::cell::RefCell<Vec<(&'static str, TestGate)>> = const { std::cell::RefCell::new(Vec::new()) };
    pub(super) static EOF_OBSERVATION: std::cell::RefCell<Option<ReaderObservation>> = const { std::cell::RefCell::new(None) };
    pub(super) static CONTROLLER_FAULTS: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
    pub(super) static START_CONTROLLER_PANIC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    pub(super) static AFTER_START_FAILURE: std::cell::RefCell<Option<TestGate>> = const { std::cell::RefCell::new(None) };
    pub(super) static BEFORE_READY: std::cell::RefCell<Option<TestGate>> = const { std::cell::RefCell::new(None) };
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
        stdin: super::super::stdin::StdinDelivery,
        containment: Option<Arc<dyn AttemptContainment>>,
    }
    impl Resources {
        fn cleanup(&mut self, primary: Option<&ExecutionError>) -> Result<(), ExecutionError> {
            if let Some(child) = self.child.take() {
                runner::cleanup_owned_attempt(child, self.group, primary)?;
            }
            // Members that left the process group (setsid/double fork) are
            // still contained; kill them and prove emptiness before readers
            // are verified or any identity/cgroup can be reused.
            if let Some(containment) = self.containment.take() {
                containment.kill_and_verify_empty().map_err(|detail| {
                    ExecutionError::ProcessGroupControlFailed {
                        detail: format!("containment cleanup not proven: {detail}"),
                    }
                })?;
            }
            Ok(())
        }
        fn join_readers(&mut self) -> Result<(), ExecutionError> {
            let started = Instant::now();
            #[cfg(test)]
            let mut started = started;
            while self.readers.iter().any(|(_, reader)| !reader.is_finished()) {
                // Test gates force EOF between the preliminary and expiry observations.
                #[cfg(test)]
                if let Some(observe) = EOF_OBSERVATION.take() {
                    observe(&self.readers, &mut started);
                }
                if started.elapsed() >= READER_VERIFY {
                    let Some((stream, _)) = self.readers.iter().find(|(_, r)| !r.is_finished())
                    else {
                        break;
                    };
                    return Err(ExecutionError::CaptureReaderFailed {
                        stream,
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
            let _ = self.cleanup(None);
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
        #[cfg(test)]
        let eof_gate = READER_EOF_GATES.with_borrow_mut(|gates| {
            gates
                .iter()
                .position(|(name, _)| *name == stream)
                .map(|index| gates.swap_remove(index).1)
        });
        std::thread::Builder::new()
            .name(format!("receipts-live-{stream}"))
            .spawn(move || {
                let mut buffer = [0u8; 32 * 1024];
                loop {
                    match source.read(&mut buffer) {
                        Ok(0) => {
                            #[cfg(test)]
                            if let Some(gate) = eof_gate {
                                gate.wait();
                            }
                            return Ok(());
                        }
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
        stdin: super::super::stdin::StdinDelivery,
        containment: Option<Arc<dyn AttemptContainment>>,
    ) -> Result<Resources, ExecutionError> {
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                // A failed pre-exec setup may already have placed the forked
                // child in containment; prove it empty before reporting.
                if let Some(containment) = &containment {
                    containment.kill_and_verify_empty().map_err(|detail| {
                        ExecutionError::ProcessGroupControlFailed {
                            detail: format!("containment cleanup not proven: {detail}"),
                        }
                    })?;
                }
                return Err(runner::spawn_failed(error));
            }
        };
        command.stdin(Stdio::null()); // Release the parent's stdin read end.

        let Some(group) = runner::owned_process_group(child.id()) else {
            let error = runner::process_group_ownership_failed(child);
            if let Some(containment) = &containment {
                let _ = containment.kill_and_verify_empty();
            }
            return Err(error);
        };
        let mut resources = Resources {
            child: Some(child),
            group,
            readers: Vec::with_capacity(2),
            stdin,
            containment,
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
            resources.cleanup(Some(&error))?;
            resources.join_readers()?;
            return Err(error);
        }
        Ok(resources)
    }

    pub(super) fn start(
        request: &ProcessRunRequest,
        policy: &ProcessTimeoutPolicy,
        customization: LaunchCustomization,
    ) -> Result<LiveProcessAttempt, LiveProcessStartError> {
        runner::ensure_timeout_platform_supported()?;
        let deadline = runner::validated_run_deadline(policy)?;
        let executable = runner::validated_executable(request.executable())?;
        let cwd = runner::validated_workspace_cwd(request.workspace_root(), request.cwd())?;
        let stdout = Arc::new(Mutex::new(runner::frozen_retention("stdout")?));
        let stderr = Arc::new(Mutex::new(runner::frozen_retention("stderr")?));
        let mut command = runner::request_command(&executable, &cwd, request);
        command
            .args(request.arguments())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let LaunchCustomization {
            pre_exec,
            containment,
        } = customization;
        if let Some(hook) = pre_exec {
            // SAFETY: the hook is built by this crate from preformatted data
            // and performs only async-signal-safe raw system calls.
            unsafe { command.pre_exec(hook) };
        }
        let stdin = super::super::stdin::StdinDelivery::prepare(&mut command, request.stdin())?;
        let closed_stdin = matches!(request.stdin(), super::super::ProcessStdin::Closed);
        let lifecycle = Arc::new(Mutex::new(Lifecycle::default()));
        let (out, err, state, policy) =
            (stdout.clone(), stderr.clone(), lifecycle.clone(), *policy);
        let (ready, started) = std::sync::mpsc::sync_channel(1);
        #[cfg(test)]
        let before_monitor = BEFORE_MONITOR.take();
        #[cfg(test)]
        let before_ready = BEFORE_READY.take();
        #[cfg(test)]
        let after_start_failure = AFTER_START_FAILURE.take();
        #[cfg(test)]
        let startup_panic = START_CONTROLLER_PANIC.replace(false);
        #[cfg(test)]
        let faults = CONTROLLER_FAULTS.replace(0);
        #[cfg(test)]
        let after_claim = AFTER_CLAIM.take();
        #[cfg(test)]
        let eof_gates = READER_EOF_GATES.take();
        #[cfg(test)]
        let eof_observation = EOF_OBSERVATION.take();
        let controller = std::thread::Builder::new()
            .name("receipts-live-controller".into())
            .spawn(move || {
                #[cfg(test)]
                AFTER_CLAIM.set(after_claim);
                #[cfg(test)]
                BEFORE_READY.set(before_ready);
                #[cfg(test)]
                READER_EOF_GATES.set(eof_gates);
                #[cfg(test)]
                EOF_OBSERVATION.set(eof_observation);
                #[cfg(test)]
                let _faults = runner::inject_capture_test_faults(faults);
                let _closed = CloseLifecycle(state.clone());
                let mut resources =
                    match setup(&mut command, out.clone(), err.clone(), stdin, containment) {
                        Ok(resources) => resources,
                        Err(error) => {
                            let _ = ready.send(false);
                            return Err(error);
                        }
                    };
                if closed_stdin {
                    lock(&state).ready = true;
                    let _ = ready.send(true);
                }
                #[cfg(test)]
                if let Some(gate) = before_monitor {
                    gate.wait();
                }
                #[cfg(test)]
                assert!(!startup_panic, "injected startup controller failure");
                let result = monitor(&mut resources, &state, deadline, &policy, &ready);
                lock(&state).closed = true;
                #[cfg(test)]
                if result.is_err()
                    && let Some(gate) = after_start_failure
                {
                    gate.wait();
                }
                // Control errors outrank capture errors. Never signal after a completed reap.
                let cleanup = resources.cleanup(result.as_ref().err());
                let readers = resources.join_readers();
                cleanup?;
                let (termination, exit_code) = result?;
                readers?;
                Ok(LiveProcessOutcome {
                    termination,
                    exit_code,
                    output: snapshot(&out, &err, true)?,
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
                deadline,
            }),
            _ => match controller.join() {
                Ok(Err(error)) => Err(error.into()),
                Ok(Ok(outcome)) => Err(LiveProcessStartError::TerminalBeforeReady(Box::new(
                    outcome,
                ))),
                Err(_) => Err(LiveProcessStartError::ControllerFailed),
            },
        }
    }

    fn monitor(
        resources: &mut Resources,
        lifecycle: &Mutex<Lifecycle>,
        deadline: Instant,
        policy: &ProcessTimeoutPolicy,
        ready: &std::sync::mpsc::SyncSender<bool>,
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
                    Some(deadline),
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
            if !state.ready && resources.stdin.advance()? {
                // Exact write and successful close precede READY, committed under
                // the same mutex as every terminal claim. No handle exists yet.
                #[cfg(test)]
                if !ready_permitted_for_test() {
                    drop(state);
                    std::thread::sleep(POLL);
                    continue;
                }
                state.ready = true;
                let _ = ready.send(true);
            }
            drop(state);
            std::thread::sleep(POLL.min(deadline.saturating_duration_since(Instant::now())));
        }
    }
}

#[cfg(test)]
fn ready_permitted_for_test() -> bool {
    BEFORE_READY.with_borrow_mut(|slot| {
        let Some(gate) = slot else { return true };
        let _ = gate.reached.try_send(());
        match gate.release.try_recv() {
            Ok(()) => {
                *slot = None;
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                panic!("READY test gate disconnected")
            }
        }
    })
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

#[cfg(test)]
pub(crate) fn inject_collection_failure_for_test() {
    CONTROLLER_FAULTS.set(
        super::runner::CAPTURE_TEST_FORCE_COMPLETION_BARRIER
            | super::runner::TEST_FAIL_POST_REAP_GROUP_EMPTY,
    );
}
