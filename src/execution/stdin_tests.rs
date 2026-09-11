//! Behavioral stdin proofs through the production launchers, with private gates
//! only for claim ordering. Probe processes never invoke a shell.
use super::live_attempt::{
    AFTER_CLAIM, AFTER_START_FAILURE, BEFORE_MONITOR, BEFORE_READY, TestGate,
};
use super::unix_signal::{caller_process_group, process_alive, recorded_group_is_empty};
use super::*;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc::{Receiver, sync_channel},
    },
    time::{Duration, Instant},
};

const LIMIT: Duration = Duration::from_secs(10);
const SECRET: &[u8] = b"stdin-private-context\0\xff\r\n";
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "receipts-stdin-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, value: impl AsRef<[u8]>) {
        fs::write(self.0.join(name), value).unwrap();
    }
    fn request(&self, stdin: ProcessStdin) -> ProcessRunRequest {
        ProcessRunRequest::new(
            std::env::current_exe().unwrap(),
            [
                "execution::stdin_tests::stdin_probe",
                "--exact",
                "--ignored",
                "--nocapture",
            ],
            &self.0,
            &self.0,
        )
        .unwrap()
        .with_stdin(stdin)
    }
    fn bytes(&self, bytes: &[u8]) -> ProcessRunRequest {
        self.request(ProcessStdin::bytes(bytes).unwrap())
    }
    fn wait(&self, marker: &str) {
        until(|| self.0.join(marker).exists());
    }
    fn value(&self, marker: &str) -> u32 {
        fs::read_to_string(self.0.join(marker))
            .unwrap()
            .parse()
            .unwrap()
    }
    fn empty(&self) {
        let pid = self.value("pid");
        assert_ne!(pid, caller_process_group() as u32);
        assert!(!process_alive(pid));
        assert_eq!(recorded_group_is_empty(pid), Some(true));
        if self.0.join("descendant-pid").exists() {
            assert!(!process_alive(self.value("descendant-pid")));
        }
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn until(mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < LIMIT, "bounded test condition");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn policy(timeout: Duration) -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(timeout, Duration::from_millis(100)).unwrap()
}
fn arrived(r: &Receiver<()>) {
    r.recv_timeout(LIMIT).unwrap();
}
fn track() -> Arc<AtomicUsize> {
    let counter = Arc::new(AtomicUsize::new(0));
    super::stdin::TRACKER.set(Some(counter.clone()));
    counter
}
fn binary() -> Vec<u8> {
    (0..MAX_STDIN_BYTES).map(|i| (i % 256) as u8).collect()
}
fn safe(value: impl std::fmt::Debug + std::fmt::Display) {
    for text in [format!("{value:?}"), value.to_string()] {
        assert!(!text.contains("stdin-private-context"));
        assert!(!text.contains("stdout-secret"));
        assert!(!text.contains("stderr-secret"));
        assert!(!text.contains("115, 116, 100, 105, 110"));
    }
}
unsafe extern "C" {
    fn close(fd: i32) -> i32;
    fn signal(sig: i32, handler: usize) -> usize;
}
#[test]
#[ignore]
fn stdin_descendant() {
    unsafe {
        signal(15, 1);
        close(0);
    }
    fs::write("descendant-pid", std::process::id().to_string()).unwrap();
    fs::write("descendant-ready", b"1").unwrap();
    std::thread::sleep(Duration::from_secs(30));
}
#[test]
#[ignore]
#[allow(clippy::zombie_processes)] // Production cleanup must contain this inherited descendant.
fn stdin_probe() {
    assert!(std::env::vars_os().next().is_none());
    if Path::new("ignore").exists() {
        unsafe {
            signal(15, 1);
        }
    }
    fs::write("pid", std::process::id().to_string()).unwrap();
    if Path::new("descendant").exists() {
        let _child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "execution::stdin_tests::stdin_descendant",
                "--exact",
                "--ignored",
                "--nocapture",
            ])
            .spawn()
            .unwrap();
        until(|| Path::new("descendant-ready").exists());
    }
    if Path::new("broken").exists() {
        unsafe {
            close(0);
        }
    }
    fs::write("ready", b"1").unwrap();
    for (name, sink, byte) in [
        ("stdout", &mut std::io::stdout() as &mut dyn Write, b'O'),
        ("stderr", &mut std::io::stderr() as &mut dyn Write, b'E'),
    ] {
        if Path::new(name).exists() {
            let n: usize = fs::read_to_string(name).unwrap().parse().unwrap();
            sink.write_all(&vec![byte; n]).unwrap();
        }
    }
    if Path::new("secrets").exists() {
        std::io::stdout().write_all(b"stdout-secret").unwrap();
        std::io::stderr().write_all(b"stderr-secret").unwrap();
        std::io::stdout().flush().unwrap();
        std::io::stderr().flush().unwrap();
    }
    if !Path::new("broken").exists() && !Path::new("refuse").exists() {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes).unwrap();
        fs::write("received", &bytes).unwrap();
        fs::write("eof", b"1").unwrap();
    }
    if Path::new("hold").exists() {
        until(|| {
            if Path::new("close-request").exists() {
                unsafe {
                    close(0);
                }
                fs::write("closed", b"1").unwrap();
            }
            Path::new("release").exists()
        });
    }
    let code = fs::read_to_string("exit")
        .ok()
        .map(|v| v.parse().unwrap())
        .unwrap_or(0);
    std::process::exit(code);
}

#[test]
fn stdin_admission_is_immutable_bounded_distinct_and_redacted() {
    let mut original = SECRET.to_vec();
    let stdin = ProcessStdin::bytes(&original).unwrap();
    original.fill(0);
    let ProcessStdin::Bytes(bytes) = &stdin else {
        panic!("bytes variant")
    };
    assert_eq!(bytes.as_bytes(), SECRET);
    assert_ne!(ProcessStdin::Closed, ProcessStdin::bytes([]).unwrap());
    assert!(ProcessStdin::bytes(binary()).is_ok());
    let ws = Workspace::new();
    let attempt = || -> Result<ProcessRunOutcome, ExecutionError> {
        let input = ProcessStdin::bytes(vec![0; MAX_STDIN_BYTES + 1])?;
        run(&ws.request(input))
    };
    let error = attempt().unwrap_err();
    assert!(matches!(
        error,
        ExecutionError::StdinPayloadTooLarge {
            len: 1_048_577,
            max: 1_048_576
        }
    ));
    assert!(!ws.0.join("pid").exists());
    safe(error);
    for text in [format!("{stdin:?}"), format!("{:?}", ws.request(stdin))] {
        assert!(text.contains("redacted: true"));
        assert!(!text.contains("stdin-private-context"));
        assert!(!text.contains("115, 116, 100"));
    }
}

#[test]
fn stdin_all_one_shot_paths_preserve_raw_bytes_and_nonzero_status() {
    let cases = [
        vec![],
        vec![42],
        vec![0, 1, 0],
        vec![0xff, 0xfe],
        b"\r".to_vec(),
        b"\n".to_vec(),
        b"\r\n".to_vec(),
        b"no-newline".to_vec(),
        b"two\n\n".to_vec(),
        binary(),
    ];
    for mode in 0..3 {
        for bytes in &cases {
            for code in [0, 7] {
                let ws = Workspace::new();
                ws.write("exit", code.to_string());
                let counter = track();
                let request = ws.bytes(bytes);
                let result = match mode {
                    0 => run(&request).unwrap(),
                    1 => run_with_timeout(&request, &policy(LIMIT)).unwrap(),
                    _ => *run_with_timeout_and_capture(&request, &policy(LIMIT))
                        .unwrap()
                        .outcome(),
                };
                assert_eq!(result.exit_code(), Some(code));
                assert_eq!(result.success(), code == 0);
                assert_eq!(fs::read(ws.0.join("received")).unwrap(), *bytes);
                assert!(ws.0.join("eof").exists());
                assert_eq!(counter.load(Ordering::SeqCst), 0);
                ws.empty();
            }
        }
    }
}

#[test]
fn stdin_closed_and_empty_bytes_both_cause_immediate_eof() {
    for stdin in [ProcessStdin::Closed, ProcessStdin::bytes([]).unwrap()] {
        for mode in 0..4 {
            let ws = Workspace::new();
            let request = ws.request(stdin.clone());
            match mode {
                0 => {
                    assert!(run(&request).unwrap().success());
                }
                1 => {
                    assert!(
                        run_with_timeout(&request, &policy(LIMIT))
                            .unwrap()
                            .success()
                    );
                }
                2 => {
                    assert!(
                        run_with_timeout_and_capture(&request, &policy(LIMIT))
                            .unwrap()
                            .outcome()
                            .success()
                    );
                }
                _ => {
                    assert!(
                        start_live_process_attempt(&request, &policy(LIMIT))
                            .unwrap()
                            .wait_collect()
                            .unwrap()
                            .success()
                    );
                }
            }
            assert!(fs::read(ws.0.join("received")).unwrap().is_empty());
        }
    }
}

#[test]
fn stdin_large_bidirectional_capture_never_deadlocks() {
    for live in [false, true] {
        for (stdout, stderr) in [(3_000_000, 0), (0, 3_000_000), (3_000_000, 3_000_000)] {
            let ws = Workspace::new();
            ws.write("stdout", stdout.to_string());
            ws.write("stderr", stderr.to_string());
            let payload = binary();
            let request = ws.bytes(&payload);
            let start = Instant::now();
            let (out, err) = if live {
                let result = start_live_process_attempt(&request, &policy(LIMIT))
                    .unwrap()
                    .wait_collect()
                    .unwrap();
                assert!(result.success());
                (result.stdout().clone(), result.stderr().clone())
            } else {
                let result = run_with_timeout_and_capture(&request, &policy(LIMIT)).unwrap();
                assert!(result.outcome().success());
                (result.stdout().clone(), result.stderr().clone())
            };
            assert!(start.elapsed() < LIMIT);
            assert_eq!(fs::read(ws.0.join("received")).unwrap(), payload);
            assert_eq!(err.total_bytes(), stderr);
            assert!((stdout..stdout + 512).contains(&out.total_bytes()));
            for (stream, n, byte) in [(&out, stdout, b'O'), (&err, stderr, b'E')] {
                assert_eq!(stream.truncated(), n > STREAM_CAPTURE_LIMIT_BYTES);
                if n > 0 {
                    assert_eq!(stream.tail(), vec![byte; STREAM_TAIL_RETENTION_BYTES]);
                }
            }
            ws.empty();
        }
    }
}

#[test]
fn stdin_early_close_is_typed_never_success_and_cleans_descendants() {
    for mode in 0..4 {
        let ws = Workspace::new();
        ws.write("broken", b"1");
        ws.write("hold", b"1");
        ws.write("descendant", b"1");
        let request = ws.bytes(&binary());
        let counter = track();
        let start = Instant::now();
        let error = match mode {
            0 => run(&request).unwrap_err(),
            1 => run_with_timeout(&request, &policy(LIMIT)).unwrap_err(),
            2 => run_with_timeout_and_capture(&request, &policy(LIMIT)).unwrap_err(),
            _ => match start_live_process_attempt(&request, &policy(LIMIT)).unwrap_err() {
                LiveProcessStartError::Execution(error) => error,
                other => panic!("expected stdin failure: {other}"),
            },
        };
        assert!(matches!(
            error,
            ExecutionError::StdinWriteFailed {
                kind: std::io::ErrorKind::BrokenPipe
            }
        ));
        safe(error);
        assert!(start.elapsed() < LIMIT);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        ws.empty();
    }
}

#[test]
fn stdin_setup_and_delivery_failures_are_typed_payload_safe() {
    for fault in 1..=4 {
        let ws = Workspace::new();
        ws.write("hold", b"1");
        super::stdin::FAULT.set(fault);
        let counter = track();
        let error = start_live_process_attempt(&ws.bytes(SECRET), &policy(LIMIT)).unwrap_err();
        match &error {
            LiveProcessStartError::Execution(e) => {
                assert!(match fault {
                    1 => matches!(e, ExecutionError::StdinPipeCreationFailed { .. }),
                    2 => matches!(e, ExecutionError::StdinConfigurationFailed { .. }),
                    3 => matches!(e, ExecutionError::StdinWriteFailed { .. }),
                    _ => matches!(e, ExecutionError::StdinDeliveryFailed { .. }),
                });
                safe(e);
            }
            other => panic!("typed delivery error required: {other}"),
        }
        safe(error);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        if fault < 3 {
            assert!(!ws.0.join("pid").exists());
        }
    }
}

#[test]
fn stdin_refusal_cannot_defeat_one_shot_or_live_timeout() {
    for mode in 0..3 {
        for forced in [false, true] {
            let ws = Workspace::new();
            ws.write("refuse", b"1");
            ws.write("hold", b"1");
            ws.write("secrets", b"1");
            if forced {
                ws.write("ignore", b"1");
                ws.write("descendant", b"1");
            }
            let counter = track();
            let request = ws.bytes(&binary());
            let start = Instant::now();
            let timeout = policy(Duration::from_millis(500));
            match mode {
                0 => {
                    let result = run_with_timeout(&request, &timeout).unwrap();
                    assert!(result.timed_out());
                    assert_eq!(result.forced_kill_required(), forced);
                }
                1 => {
                    let result = run_with_timeout_and_capture(&request, &timeout).unwrap();
                    assert!(result.outcome().timed_out());
                    assert_eq!(result.outcome().forced_kill_required(), forced);
                    assert_eq!(result.stderr().head(), b"stderr-secret");
                }
                _ => {
                    let error = start_live_process_attempt(&request, &timeout).unwrap_err();
                    safe(&error);
                    let LiveProcessStartError::TerminalBeforeReady(result) = error else {
                        panic!("terminal evidence required")
                    };
                    assert_eq!(result.terminal_cause(), LiveProcessTerminalCause::TimedOut);
                    assert!(!result.success());
                    assert_eq!(result.forced_kill_required(), forced);
                    assert_eq!(result.exit_code(), None);
                    assert_eq!(result.stderr().head(), b"stderr-secret");
                    assert!(result.stdout().head().ends_with(b"stdout-secret"));
                }
            }
            assert!(start.elapsed() < LIMIT);
            assert_eq!(counter.load(Ordering::SeqCst), 0);
            ws.empty();
        }
    }
}

#[test]
fn stdin_ready_commits_only_after_delivery_and_close() {
    let ws = Workspace::new();
    ws.write("hold", b"1");
    let request = ws.bytes(SECRET);
    std::thread::scope(|scope| {
        let (observed, observe) = sync_channel(1);
        let (release, permit) = sync_channel(1);
        let starting = scope.spawn(|| {
            BEFORE_READY.set(Some(TestGate {
                reached: observed,
                release: permit,
            }));
            start_live_process_attempt(&request, &policy(LIMIT))
        });
        arrived(&observe);
        ws.wait("eof");
        assert!(
            !starting.is_finished(),
            "no public handle before READY commit"
        );
        assert_eq!(fs::read(ws.0.join("received")).unwrap(), SECRET);
        release.send(()).unwrap();
        let attempt = starting.join().unwrap().unwrap();
        assert_eq!(attempt.terminal_cause(), None);
        ws.write("release", b"1");
        assert!(attempt.wait_collect().unwrap().success());
        ws.empty();
    });
}

#[test]
fn stdin_completed_and_timeout_before_ready_preserve_terminal_evidence() {
    for timed_out in [false, true] {
        for code in [0, 7] {
            let ws = Workspace::new();
            ws.write("hold", b"1");
            ws.write("secrets", b"1");
            ws.write("exit", code.to_string());
            let request = ws.bytes(SECRET);
            std::thread::scope(|scope| {
                let (observed, observe) = sync_channel(1);
                let (_release, permit) = sync_channel(1);
                let starting = scope.spawn(|| {
                    BEFORE_READY.set(Some(TestGate {
                        reached: observed,
                        release: permit,
                    }));
                    start_live_process_attempt(
                        &request,
                        &policy(if timed_out {
                            Duration::from_millis(500)
                        } else {
                            LIMIT
                        }),
                    )
                });
                arrived(&observe);
                ws.wait("eof");
                assert!(!starting.is_finished());
                if !timed_out {
                    ws.write("release", b"1");
                }
                let error = starting.join().unwrap().unwrap_err();
                safe(&error);
                let LiveProcessStartError::TerminalBeforeReady(result) = error else {
                    panic!("terminal evidence")
                };
                assert_eq!(
                    result.terminal_cause(),
                    if timed_out {
                        LiveProcessTerminalCause::TimedOut
                    } else {
                        LiveProcessTerminalCause::Completed
                    }
                );
                assert_eq!(result.success(), !timed_out && code == 0);
                assert_eq!(
                    result.exit_code(),
                    if timed_out { None } else { Some(code) }
                );
                assert_eq!(result.stderr().head(), b"stderr-secret");
                ws.empty();
            });
        }
    }
}

#[test]
fn stdin_ready_before_timeout_completion_cancel_and_drop_use_existing_lifecycle() {
    for mode in 0..4 {
        let ws = Workspace::new();
        ws.write("hold", b"1");
        let attempt =
            start_live_process_attempt(&ws.bytes(SECRET), &policy(Duration::from_millis(500)))
                .unwrap();
        ws.wait("eof");
        match mode {
            0 => {
                assert_eq!(
                    attempt.wait_collect().unwrap().terminal_cause(),
                    LiveProcessTerminalCause::TimedOut
                );
            }
            1 => {
                ws.write("release", b"1");
                assert!(attempt.wait_collect().unwrap().success());
            }
            2 => {
                assert_eq!(
                    attempt.cancel(),
                    LiveProcessCancelAcceptance::CancelAccepted
                );
                assert_eq!(
                    attempt.wait_collect().unwrap().terminal_cause(),
                    LiveProcessTerminalCause::Cancelled
                );
            }
            _ => {
                drop(attempt);
                ws.empty();
                continue;
            }
        }
        assert_eq!(
            attempt.cancel(),
            LiveProcessCancelAcceptance::AlreadyTerminalOrTerminating
        );
        let collected: Result<LiveProcessOutcome, LiveProcessAttemptError> = attempt.wait_collect();
        assert!(matches!(
            collected,
            Err(LiveProcessAttemptError::AlreadyCollectedOrCollecting)
        ));
        ws.empty();
    }
}

#[test]
fn stdin_timeout_claim_precedes_later_pipe_close() {
    let ws = Workspace::new();
    ws.write("refuse", b"1");
    ws.write("hold", b"1");
    let request = ws.bytes(&binary());
    std::thread::scope(|scope| {
        let (claimed, claim) = sync_channel(1);
        let (finish, permit) = sync_channel(1);
        let starting = scope.spawn(|| {
            AFTER_CLAIM.set(Some(TestGate {
                reached: claimed,
                release: permit,
            }));
            start_live_process_attempt(&request, &policy(Duration::from_millis(500)))
        });
        arrived(&claim);
        ws.write("close-request", b"1");
        ws.wait("closed");
        finish.send(()).unwrap();
        let LiveProcessStartError::TerminalBeforeReady(outcome) =
            starting.join().unwrap().unwrap_err()
        else {
            panic!("timeout remains authoritative")
        };
        assert_eq!(outcome.terminal_cause(), LiveProcessTerminalCause::TimedOut);
        ws.empty();
    });
}

#[test]
fn stdin_failure_claim_cannot_be_overwritten_by_later_timeout() {
    let ws = Workspace::new();
    ws.write("broken", b"1");
    ws.write("hold", b"1");
    let request = ws.bytes(&binary());
    std::thread::scope(|scope| {
        let (reached, arrival) = sync_channel(1);
        let (release, permit) = sync_channel(1);
        let (failed, failure) = sync_channel(1);
        let (finish, cleanup) = sync_channel(1);
        let starting = scope.spawn(|| {
            BEFORE_MONITOR.set(Some(TestGate {
                reached,
                release: permit,
            }));
            AFTER_START_FAILURE.set(Some(TestGate {
                reached: failed,
                release: cleanup,
            }));
            start_live_process_attempt(&request, &policy(Duration::from_millis(500)))
        });
        let started = Instant::now();
        arrived(&arrival);
        ws.wait("ready");
        release.send(()).unwrap();
        arrived(&failure);
        until(|| started.elapsed() > Duration::from_millis(600));
        finish.send(()).unwrap();
        assert!(matches!(
            starting.join().unwrap(),
            Err(LiveProcessStartError::Execution(
                ExecutionError::StdinWriteFailed { .. }
            ))
        ));
        ws.empty();
    });
}

#[test]
fn stdin_controller_failure_before_ready_cleans_all_resources() {
    let ws = Workspace::new();
    ws.write("refuse", b"1");
    ws.write("hold", b"1");
    ws.write("descendant", b"1");
    let request = ws.bytes(SECRET);
    let counter = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        let (reached, arrival) = sync_channel(1);
        let (release, permit) = sync_channel(1);
        let starting = scope.spawn(|| {
            super::stdin::TRACKER.set(Some(counter.clone()));
            super::live_attempt::START_CONTROLLER_PANIC.set(true);
            BEFORE_MONITOR.set(Some(TestGate {
                reached,
                release: permit,
            }));
            start_live_process_attempt(&request, &policy(LIMIT))
        });
        arrived(&arrival);
        ws.wait("ready");
        release.send(()).unwrap();
        let error = starting.join().unwrap().unwrap_err();
        assert!(matches!(error, LiveProcessStartError::ControllerFailed));
        safe(error);
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        ws.empty();
    });
}

#[test]
fn stdin_zero_exit_without_consuming_payload_is_not_one_shot_success() {
    for mode in 0..3 {
        let ws = Workspace::new();
        ws.write("refuse", b"1");
        let request = ws.bytes(&binary());
        let error = match mode {
            0 => run(&request).unwrap_err(),
            1 => run_with_timeout(&request, &policy(LIMIT)).unwrap_err(),
            _ => run_with_timeout_and_capture(&request, &policy(LIMIT)).unwrap_err(),
        };
        assert!(
            matches!(error, ExecutionError::StdinWriteFailed { .. }),
            "{error:?}"
        );
        ws.empty();
    }
}
