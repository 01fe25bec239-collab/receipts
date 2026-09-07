//! Real-process lifecycle and ownership tests. Gates control claim ordering;
//! file readiness barriers prove child behavior is installed before cancellation.
use super::live_attempt::{AFTER_CLAIM, BEFORE_MONITOR, TestGate};
use super::unix_signal::{caller_process_group, process_alive, recorded_group_is_empty};
use super::*;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::time::{Duration, Instant};

const LIMIT: Duration = Duration::from_secs(10);
const GRACE: Duration = Duration::from_millis(100);

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("receipts-live-{}-{n}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
    fn write(&self, name: &str, value: impl AsRef<[u8]>) {
        fs::write(self.0.join(name), value).unwrap();
    }
    fn ready(&self) {
        until(|| self.0.join("ready").exists());
    }
    fn value(&self, name: &str) -> u32 {
        fs::read_to_string(self.0.join(name))
            .unwrap()
            .parse()
            .unwrap()
    }
    fn request(&self) -> ProcessRunRequest {
        ProcessRunRequest::new(
            std::env::current_exe().unwrap(),
            [
                "execution::live_attempt_tests::live_probe",
                "--exact",
                "--ignored",
                "--nocapture",
            ],
            &self.0,
            &self.0,
        )
        .unwrap()
    }
    fn start(&self, timeout: Duration) -> LiveProcessAttempt {
        start_live_process_attempt(
            &self.request(),
            &ProcessTimeoutPolicy::new(timeout, GRACE).unwrap(),
        )
        .unwrap()
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn until(mut predicate: impl FnMut() -> bool) {
    let start = Instant::now();
    while !predicate() {
        assert!(start.elapsed() < LIMIT, "bounded condition was not reached");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn gate(
    slot: &'static std::thread::LocalKey<std::cell::RefCell<Option<TestGate>>>,
) -> (Receiver<()>, SyncSender<()>) {
    let (reached, arrival) = sync_channel(1);
    let (release, permit) = sync_channel(1);
    slot.set(Some(TestGate {
        reached,
        release: permit,
    }));
    (arrival, release)
}
fn arrive(arrival: &Receiver<()>) {
    arrival.recv_timeout(LIMIT).unwrap();
}
fn outcome(
    attempt: &LiveProcessAttempt,
    expected: LiveProcessTerminalCause,
    forced: bool,
) -> LiveProcessOutcome {
    let result = attempt.wait_collect().unwrap();
    assert_eq!(result.terminal_cause(), expected);
    assert_eq!(attempt.terminal_cause(), Some(expected));
    assert_eq!(result.forced_kill_required(), forced);
    if expected != LiveProcessTerminalCause::Completed {
        assert!(!result.success());
    }
    assert_eq!(
        attempt.cancel(),
        LiveProcessCancelAcceptance::AlreadyTerminalOrTerminating
    );
    assert!(matches!(
        attempt.wait_collect(),
        Err(LiveProcessAttemptError::AlreadyCollectedOrCollecting)
    ));
    let snapshot = attempt.snapshot_output().unwrap();
    assert_eq!(snapshot.stdout(), result.stdout());
    assert_eq!(snapshot.stderr(), result.stderr());
    result
}
fn prove_empty(ws: &Workspace, caller: i32) {
    let pid = ws.value("pid");
    assert_eq!(pid, ws.value("pgid"));
    assert_ne!(pid, caller as u32);
    assert!(!process_alive(pid), "direct child must already be reaped");
    assert_eq!(recorded_group_is_empty(pid), Some(true));
    if ws.0.join("descendant-pid").exists() {
        let descendant = ws.value("descendant-pid");
        assert_ne!(descendant, pid);
        assert_eq!(ws.value("descendant-pgid"), pid);
        assert!(!process_alive(descendant));
    }
    assert_eq!(caller_process_group(), caller);
    assert!(process_alive(std::process::id()));
}

#[test]
#[ignore]
#[allow(clippy::zombie_processes)] // The parent is terminated; the test independently proves descendant death/reap.
fn live_probe() {
    assert!(std::env::vars_os().next().is_none());
    assert_eq!(std::io::stdin().read(&mut [0u8]).unwrap(), 0);
    let descendant =
        Path::new("spawn-descendant").exists() && !Path::new("parent-spawned").exists();
    let is_descendant = Path::new("parent-spawned").exists();
    let ignore =
        Path::new("ignore").exists() || (is_descendant && Path::new("descendant-ignore").exists());
    if ignore {
        unsafe extern "C" {
            fn signal(signal: i32, handler: usize) -> usize;
        }
        unsafe {
            signal(15, 1);
        }
    }
    let prefix = if is_descendant { "descendant-" } else { "" };
    fs::write(format!("{prefix}pid"), std::process::id().to_string()).unwrap();
    fs::write(format!("{prefix}pgid"), caller_process_group().to_string()).unwrap();
    if descendant {
        fs::write("parent-spawned", b"1").unwrap();
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "execution::live_attempt_tests::live_probe",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .spawn()
            .unwrap();
        until(|| Path::new("descendant-ready").exists());
        assert_eq!(
            child.id(),
            fs::read_to_string("descendant-pid")
                .unwrap()
                .parse::<u32>()
                .unwrap()
        );
    }
    let emit = |stream: &str, sink: &mut dyn Write, byte: u8| {
        let n = fs::read_to_string(format!("{stream}-bytes"))
            .ok()
            .map(|n| n.parse::<usize>().unwrap())
            .unwrap_or(0);
        let chunk = [byte; 8192];
        for _ in 0..n / chunk.len() {
            sink.write_all(&chunk).unwrap();
        }
        sink.write_all(&chunk[..n % chunk.len()]).unwrap();
        sink.flush().unwrap();
    };
    std::thread::scope(|scope| {
        scope.spawn(|| emit("stdout", &mut std::io::stdout(), b'O'));
        scope.spawn(|| emit("stderr", &mut std::io::stderr(), b'E'));
    });
    fs::write(format!("{prefix}ready"), b"1").unwrap();
    if Path::new("exit-now").exists() {
        let code = fs::read_to_string("exit-now").unwrap().parse().unwrap();
        std::process::exit(code);
    }
    until(|| Path::new("release-child").exists());
    std::process::exit(0);
}

#[test]
fn live_natural_success_nonzero_and_exit_before_first_poll() {
    for code in [0, 7] {
        let ws = Workspace::new();
        ws.write("exit-now", code.to_string());
        ws.write("stdout-bytes", b"17");
        ws.write("stderr-bytes", b"23");
        let (arrived, release) = gate(&BEFORE_MONITOR);
        let attempt = ws.start(LIMIT);
        arrive(&arrived);
        ws.ready();
        // Establish real child exit non-reapingly before the first output poll.
        until(|| {
            super::unix_signal::observe_leader_without_reaping(ws.value("pid")).unwrap()
                == super::unix_signal::LeaderState::Exited
        });
        release.send(()).unwrap();
        let result = outcome(&attempt, LiveProcessTerminalCause::Completed, false);
        assert_eq!(result.success(), code == 0);
        assert_eq!(result.exit_code(), Some(code));
        assert!(result.stdout().head().ends_with(&[b'O'; 17]));
        assert_eq!(result.stderr().head(), &[b'E'; 23]);
        prove_empty(&ws, caller_process_group());
    }
}

#[test]
fn live_cancel_graceful_force_double_cancel_and_poll_running() {
    for forced in [false, true] {
        let ws = Workspace::new();
        if forced {
            ws.write("ignore", b"1");
        }
        ws.write("stderr-bytes", b"23");
        let attempt = ws.start(LIMIT);
        ws.ready();
        until(|| attempt.snapshot_output().unwrap().stderr().total_bytes() == 23);
        assert_eq!(attempt.terminal_cause(), None);
        assert_eq!(
            attempt.cancel(),
            LiveProcessCancelAcceptance::CancelAccepted
        );
        assert_eq!(
            attempt.cancel(),
            LiveProcessCancelAcceptance::AlreadyTerminalOrTerminating
        );
        outcome(&attempt, LiveProcessTerminalCause::Cancelled, forced);
        prove_empty(&ws, caller_process_group());
    }
}

#[test]
fn live_timeout_graceful_force_and_cancel_after_timeout_ownership() {
    for forced in [false, true] {
        let ws = Workspace::new();
        if forced {
            ws.write("ignore", b"1");
        }
        let (arrived, release) = gate(&BEFORE_MONITOR);
        let (claimed, finish) = gate(&AFTER_CLAIM);
        let attempt = ws.start(Duration::from_millis(150));
        let started = Instant::now();
        arrive(&arrived);
        ws.ready();
        until(|| started.elapsed() >= Duration::from_millis(200));
        release.send(()).unwrap();
        arrive(&claimed);
        assert_eq!(
            attempt.terminal_cause(),
            Some(LiveProcessTerminalCause::TimedOut)
        );
        assert_eq!(
            attempt.cancel(),
            LiveProcessCancelAcceptance::AlreadyTerminalOrTerminating
        );
        finish.send(()).unwrap();
        outcome(&attempt, LiveProcessTerminalCause::TimedOut, forced);
        prove_empty(&ws, caller_process_group());
    }
}

#[test]
fn live_cancel_wins_over_completion_and_expired_deadline() {
    let ws = Workspace::new();
    let (arrived, release) = gate(&BEFORE_MONITOR);
    let attempt = ws.start(Duration::from_millis(150));
    let started = Instant::now();
    arrive(&arrived);
    ws.ready();
    assert_eq!(
        attempt.cancel(),
        LiveProcessCancelAcceptance::CancelAccepted
    );
    ws.write("release-child", b"1");
    until(|| {
        super::unix_signal::observe_leader_without_reaping(ws.value("pid")).unwrap()
            == super::unix_signal::LeaderState::Exited
    });
    until(|| started.elapsed() >= Duration::from_millis(200));
    release.send(()).unwrap();
    let result = outcome(&attempt, LiveProcessTerminalCause::Cancelled, false);
    assert_eq!(result.exit_code(), Some(0));
    assert!(!result.success());
}

#[test]
fn live_completion_wins_over_cancel_and_later_deadline() {
    let ws = Workspace::new();
    ws.write("exit-now", b"0");
    let (claimed, release) = gate(&AFTER_CLAIM);
    let attempt = ws.start(Duration::from_secs(2));
    let started = Instant::now();
    arrive(&claimed);
    assert_eq!(
        attempt.terminal_cause(),
        Some(LiveProcessTerminalCause::Completed)
    );
    assert_eq!(
        attempt.cancel(),
        LiveProcessCancelAcceptance::AlreadyTerminalOrTerminating
    );
    until(|| started.elapsed() >= Duration::from_millis(2100));
    release.send(()).unwrap();
    assert!(outcome(&attempt, LiveProcessTerminalCause::Completed, false).success());
}

#[test]
fn live_timeout_wins_over_already_exited_but_unclaimed_completion() {
    let ws = Workspace::new();
    ws.write("exit-now", b"0");
    let (arrived, release) = gate(&BEFORE_MONITOR);
    let attempt = ws.start(Duration::from_millis(150));
    let started = Instant::now();
    arrive(&arrived);
    ws.ready();
    until(|| {
        super::unix_signal::observe_leader_without_reaping(ws.value("pid")).unwrap()
            == super::unix_signal::LeaderState::Exited
    });
    until(|| started.elapsed() >= Duration::from_millis(200));
    release.send(()).unwrap();
    let result = outcome(&attempt, LiveProcessTerminalCause::TimedOut, false);
    assert_eq!(result.exit_code(), Some(0));
    assert!(!result.success());
}

#[test]
fn live_stdout_heavy_stderr_heavy_and_simultaneous_bounded_retention() {
    let big = STREAM_CAPTURE_LIMIT_BYTES as usize * 3 + 19;
    for (out, err) in [(big, 0), (0, big), (big, big)] {
        let ws = Workspace::new();
        ws.write("stdout-bytes", out.to_string());
        ws.write("stderr-bytes", err.to_string());
        let attempt = ws.start(LIMIT);
        ws.ready();
        until(|| attempt.snapshot_output().unwrap().stderr().total_bytes() == err as u64);
        let snapshot = attempt.snapshot_output().unwrap();
        for stream in [snapshot.stdout(), snapshot.stderr()] {
            assert!(stream.captured_bytes() <= STREAM_CAPTURE_LIMIT_BYTES);
        }
        ws.write("release-child", b"1");
        let result = outcome(&attempt, LiveProcessTerminalCause::Completed, false);
        assert_eq!(result.stderr().total_bytes(), err as u64);
        // Libtest adds a small stdout banner before the probe's raw bytes.
        let banner = result.stdout().total_bytes() - out as u64;
        assert!(banner > 0 && banner < 512);
        for (stream, count, byte) in [(result.stdout(), out, b'O'), (result.stderr(), err, b'E')] {
            assert!(stream.captured_bytes() <= STREAM_CAPTURE_LIMIT_BYTES);
            assert_eq!(
                stream.truncated(),
                count > STREAM_CAPTURE_LIMIT_BYTES as usize
            );
            if count == big {
                assert_eq!(stream.captured_bytes(), STREAM_CAPTURE_LIMIT_BYTES);
                assert_eq!(stream.tail(), vec![byte; STREAM_TAIL_RETENTION_BYTES]);
                assert!(stream.head().ends_with(&[byte; 8192]));
            }
        }
    }
}

#[test]
fn live_owned_descendants_cancel_and_timeout_leave_group_empty() {
    for timeout in [false, true] {
        for forced in [false, true] {
            let caller = caller_process_group();
            let ws = Workspace::new();
            ws.write("spawn-descendant", b"1");
            if forced {
                ws.write("descendant-ignore", b"1");
            }
            let (arrived, release) = gate(&BEFORE_MONITOR);
            let attempt = ws.start(if timeout {
                Duration::from_millis(200)
            } else {
                LIMIT
            });
            let started = Instant::now();
            arrive(&arrived);
            ws.ready();
            assert!(process_alive(ws.value("descendant-pid")));
            assert_eq!(ws.value("descendant-pgid"), ws.value("pgid"));
            if timeout {
                until(|| started.elapsed() >= Duration::from_millis(250));
            } else {
                assert_eq!(
                    attempt.cancel(),
                    LiveProcessCancelAcceptance::CancelAccepted
                );
            }
            release.send(()).unwrap();
            outcome(
                &attempt,
                if timeout {
                    LiveProcessTerminalCause::TimedOut
                } else {
                    LiveProcessTerminalCause::Cancelled
                },
                forced,
            );
            prove_empty(&ws, caller);
        }
    }
}

#[test]
fn live_concurrent_collect_is_typed_and_cancel_remains_available() {
    let ws = Workspace::new();
    let (claimed, release) = gate(&AFTER_CLAIM);
    let attempt = ws.start(LIMIT);
    ws.ready();
    assert_eq!(
        attempt.cancel(),
        LiveProcessCancelAcceptance::CancelAccepted
    );
    arrive(&claimed);
    std::thread::scope(|scope| {
        let collecting = scope.spawn(|| attempt.wait_collect());
        // The first collector's consumption is independently observable through the
        // explicit AlreadyCollecting result; the controller is held at a barrier.
        until(|| super::live_attempt::collection_started_for_test(&attempt));
        assert!(matches!(
            attempt.wait_collect(),
            Err(LiveProcessAttemptError::AlreadyCollectedOrCollecting)
        ));
        assert_eq!(
            attempt.cancel(),
            LiveProcessCancelAcceptance::AlreadyTerminalOrTerminating
        );
        release.send(()).unwrap();
        assert_eq!(
            collecting.join().unwrap().unwrap().terminal_cause(),
            LiveProcessTerminalCause::Cancelled
        );
    });
}

#[test]
fn live_drop_cancels_and_reaps_owned_group() {
    let ws = Workspace::new();
    ws.write("spawn-descendant", b"1");
    ws.write("descendant-ignore", b"1");
    let caller = caller_process_group();
    let attempt = ws.start(LIMIT);
    ws.ready();
    drop(attempt);
    prove_empty(&ws, caller);
}

#[test]
fn live_shell_cwd_and_unsupported_platform_fail_closed() {
    let ws = Workspace::new();
    let policy = ProcessTimeoutPolicy::new(LIMIT, GRACE).unwrap();
    let shell = ProcessRunRequest::new("/bin/sh", ["-c", "exit 0"], &ws.0, &ws.0).unwrap();
    assert!(matches!(
        start_live_process_attempt(&shell, &policy),
        Err(LiveProcessAttemptError::Execution(
            ExecutionError::ShellExecutableRejected { .. }
        ))
    ));
    let outside = Workspace::new();
    std::os::unix::fs::symlink(&outside.0, ws.0.join("escape")).unwrap();
    for cwd in [ws.0.join("escape"), ws.0.join("..")] {
        let req = ProcessRunRequest::new(std::env::current_exe().unwrap(), ["--list"], &ws.0, cwd)
            .unwrap();
        assert!(matches!(
            start_live_process_attempt(&req, &policy),
            Err(LiveProcessAttemptError::Execution(
                ExecutionError::CwdOutsideWorkspace { .. }
            ))
        ));
    }
    let _unsupported = super::runner::inject_unsupported_timeout_platform();
    assert!(matches!(
        start_live_process_attempt(&ws.request(), &policy),
        Err(LiveProcessAttemptError::Execution(
            ExecutionError::UnsupportedTimeoutPlatform
        ))
    ));
    assert!(!ws.0.join("ready").exists());
}

#[test]
fn live_control_failures_are_typed_consumed_and_do_not_report_success() {
    for fault in [
        super::runner::CAPTURE_TEST_FAIL_FORCE_KILL,
        super::runner::TEST_FAIL_POST_REAP_GROUP_EMPTY,
    ] {
        let ws = Workspace::new();
        ws.write("ignore", b"1");
        super::live_attempt::CONTROLLER_FAULTS.set(fault);
        let attempt = ws.start(LIMIT);
        ws.ready();
        assert_eq!(
            attempt.cancel(),
            LiveProcessCancelAcceptance::CancelAccepted
        );
        let error = attempt.wait_collect().unwrap_err();
        if fault == super::runner::CAPTURE_TEST_FAIL_FORCE_KILL {
            assert!(matches!(
                error,
                LiveProcessAttemptError::Execution(ExecutionError::ForceKillFailed { .. })
            ));
        } else {
            assert!(matches!(
                error,
                LiveProcessAttemptError::Execution(
                    ExecutionError::ProcessGroupControlFailed { .. }
                )
            ));
        }
        assert_eq!(
            attempt.terminal_cause(),
            Some(LiveProcessTerminalCause::Cancelled)
        );
        assert_eq!(
            attempt.cancel(),
            LiveProcessCancelAcceptance::AlreadyTerminalOrTerminating
        );
        assert!(matches!(
            attempt.wait_collect(),
            Err(LiveProcessAttemptError::AlreadyCollectedOrCollecting)
        ));
        prove_empty(&ws, caller_process_group());
    }
}

#[test]
fn live_incremental_snapshot_matches_retention_across_ring_wraps() {
    for (head, tail) in [(7, 11), (0, 11), (7, 0), (0, 0)] {
        let mut retention = super::capture::BoundedStreamRetention::new(head, tail).unwrap();
        let bytes: Vec<u8> = (0..200).collect();
        let mut count = 0;
        for chunk in bytes.chunks(13) {
            retention.push(chunk).unwrap();
            count += chunk.len();
            let snapshot = retention.snapshot().unwrap();
            assert_eq!(snapshot.total_bytes(), count as u64);
            assert_eq!(snapshot.captured_bytes(), count.min(head + tail) as u64);
            assert_eq!(snapshot.truncated(), count > head + tail);
            assert_eq!(snapshot.head(), &bytes[..head.min(count)]);
            let tail_start = head.min(count).max(count.saturating_sub(tail));
            assert_eq!(snapshot.tail(), &bytes[tail_start..count]);
        }
        assert_eq!(retention.snapshot().unwrap(), retention.finish());
    }
}

// Real reader threads drain real child pipes, then wait at EOF. The observation
// hook runs only after join_readers has observed an unfinished reader. Moving
// its private clock makes expiry ordering deterministic without timing sleeps.
fn eof_boundary_case(finish_at_boundary: &[&'static str], expired: bool) {
    use super::live_attempt::{EOF_OBSERVATION, READER_EOF_GATES};
    let ws = Workspace::new();
    ws.write("stderr-bytes", b"23");
    ws.write("spawn-descendant", b"1");
    let mut arrivals = Vec::new();
    let mut releases = Vec::new();
    for stream in ["stdout", "stderr"] {
        let (reached, arrival) = sync_channel(1);
        let (release, permit) = sync_channel(1);
        READER_EOF_GATES.with_borrow_mut(|gates| {
            gates.push((
                stream,
                TestGate {
                    reached,
                    release: permit,
                },
            ))
        });
        arrivals.push(arrival);
        releases.push((stream, release));
    }
    let finished = finish_at_boundary.to_vec();
    let (observed, observation) = sync_channel(1);
    EOF_OBSERVATION.set(Some(Box::new(move |readers, started| {
        for arrival in arrivals {
            arrive(&arrival);
        }
        assert_eq!(readers.len(), 2);
        assert!(readers.iter().all(|(_, reader)| !reader.is_finished()));
        for (stream, release) in &releases {
            if finished.contains(stream) {
                release.send(()).unwrap();
            }
        }
        until(|| {
            readers
                .iter()
                .all(|(stream, reader)| reader.is_finished() == finished.contains(stream))
        });
        *started = Instant::now();
        if expired {
            *started -= Duration::from_secs(2);
        }
        observed.send(()).unwrap();
        if finished.len() != readers.len() {
            // Keep the wedge through the error decision. Resources::drop then
            // releases and joins the test readers, retaining the original error.
            EOF_OBSERVATION.set(Some(Box::new(move |readers, _| {
                assert!(readers.iter().any(|(_, reader)| !reader.is_finished()));
                for (stream, release) in releases {
                    if !finished.contains(&stream) {
                        release.send(()).unwrap();
                    }
                }
                until(|| readers.iter().all(|(_, reader)| reader.is_finished()));
            })));
        }
    })));
    let (arrived, release) = gate(&BEFORE_MONITOR);
    let attempt = ws.start(LIMIT);
    arrive(&arrived);
    ws.ready();
    assert_eq!(
        attempt.cancel(),
        LiveProcessCancelAcceptance::CancelAccepted
    );
    release.send(()).unwrap();
    let result = attempt.wait_collect();
    arrive(&observation);
    if finish_at_boundary.len() == 2 {
        let result = result.expect("EOF completion must collect without ControllerFailed");
        assert_eq!(result.terminal_cause(), LiveProcessTerminalCause::Cancelled);
        // Both the leader and its descendant emit 23 bytes.
        assert_eq!(result.stderr().total_bytes(), 46);
        assert_eq!(attempt.snapshot_output().unwrap().stderr(), result.stderr());
    } else {
        let expected = ["stdout", "stderr"]
            .into_iter()
            .find(|stream| !finish_at_boundary.contains(stream))
            .unwrap();
        match result {
            Err(LiveProcessAttemptError::Execution(ExecutionError::CaptureReaderFailed {
                stream,
                detail,
            })) => {
                assert_eq!(stream, expected);
                assert_eq!(detail, "live reader EOF verification exceeded two seconds");
            }
            other => panic!("expected typed failure for {expected}, got {other:?}"),
        }
    }
    prove_empty(&ws, caller_process_group());
}

#[test]
fn live_eof_boundary_readers_finish_before_deadline() {
    eof_boundary_case(&["stdout", "stderr"], false);
}

#[test]
fn live_eof_boundary_all_readers_finish_between_observations() {
    eof_boundary_case(&["stdout", "stderr"], true);
}

#[test]
fn live_eof_boundary_wedged_reader_is_typed_through_wait_collect() {
    for finished in ["stdout", "stderr"] {
        eof_boundary_case(&[finished], true);
    }
}
