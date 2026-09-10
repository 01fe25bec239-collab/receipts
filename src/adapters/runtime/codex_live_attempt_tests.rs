//! Real Workspace-boundary tests. File barriers follow Workspace's live suite;
//! a standalone Rust helper avoids libtest banners corrupting protocol stdout.
use crate::{
    CodexJsonlErrorKind as ErrorKind, CodexJsonlEventKind as Kind, CodexLiveAttempt,
    CodexLiveProtocolError, CodexLiveStartError, CodexProtocolTermination as Protocol,
    CodexTaskExecutionError, CodexTaskExecutionRequest, CodexTaskSandboxMode as Sandbox,
    start_codex_live_attempt,
};
use receipts_workspace_execution::execution::{
    LiveProcessAttemptError, LiveProcessCancelAcceptance as Acceptance, LiveProcessStartError,
    LiveProcessTerminalCause as Cause, ProcessTimeoutPolicy, STREAM_CAPTURE_LIMIT_BYTES,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

const COMPLETED: &str = "{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}\n";
const STARTED: &str = "{\"type\":\"turn.started\"}\n";
const FAILED: &str = "{\"type\":\"turn.failed\",\"error\":{\"message\":\"synthetic\"}}\n";
const HELPER: &str = r#"
use std::{fs, io::{Read, Write}, path::Path, time::{Duration, Instant}};
unsafe extern "C" { fn signal(sig: i32, handler: usize) -> usize; fn getpgrp() -> i32; }
fn until(mut f: impl FnMut() -> bool) {
    let start = Instant::now();
    while !f() { assert!(start.elapsed() < Duration::from_secs(20)); std::thread::sleep(Duration::from_millis(2)); }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let descendant = args.get(1).map(String::as_str) == Some("descendant");
    if descendant || Path::new("ignore").exists() { unsafe { signal(15, 1); } }
    let prefix = if descendant { "descendant-" } else { "" };
    fs::write(format!("{prefix}pid"), std::process::id().to_string()).unwrap();
    fs::write(format!("{prefix}pgid"), unsafe { getpgrp() }.to_string()).unwrap();
    if descendant {
        fs::write("descendant-ready", b"1").unwrap();
        until(|| Path::new("release-descendant").exists());
        return;
    }
    assert_eq!(args.len(), 6);
    assert_eq!(&args[1..4], &["exec", "--json", "--sandbox"]);
    assert_eq!(args[4].as_bytes(), fs::read("mode").unwrap());
    assert_eq!(args[5].as_bytes(), fs::read("prompt").unwrap());
    assert!(std::env::vars_os().next().is_none());
    assert_eq!(std::io::stdin().read(&mut [0]).unwrap(), 0);
    if Path::new("spawn-descendant").exists() {
        let _child = std::process::Command::new(std::env::current_exe().unwrap()).arg("descendant").spawn().unwrap();
        until(|| Path::new("descendant-ready").exists());
    }
    std::io::stdout().write_all(&fs::read("stdout").unwrap()).unwrap();
    std::io::stdout().flush().unwrap();
    std::io::stderr().write_all(&fs::read("stderr").unwrap()).unwrap();
    std::io::stderr().flush().unwrap();
    fs::write("ready", b"1").unwrap();
    until(|| Path::new("release").exists());
    std::io::stdout().write_all(&fs::read("suffix").unwrap()).unwrap();
    std::io::stdout().flush().unwrap();
    std::process::exit(fs::read_to_string("exit").unwrap().parse().unwrap());
}
"#;

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("receipts-codex-live-{}-{n}", std::process::id()));
        fs::create_dir(&path).unwrap();
        let ws = Self(fs::canonicalize(path).unwrap());
        for (name, bytes) in [
            ("stdout", ""),
            ("stderr", ""),
            ("suffix", ""),
            ("exit", "0"),
            ("mode", "read-only"),
            ("prompt", "fixture"),
        ] {
            ws.write(name, bytes);
        }
        ws
    }
    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.0.join(name), bytes).unwrap();
    }
    fn request(&self, timeout: Duration) -> CodexTaskExecutionRequest {
        CodexTaskExecutionRequest::new(
            helper(),
            &self.0,
            &self.0,
            ProcessTimeoutPolicy::new(timeout, Duration::from_millis(100)).unwrap(),
            Sandbox::ReadOnly,
            "fixture",
        )
    }
    fn start(&self, timeout: Duration) -> CodexLiveAttempt {
        start_codex_live_attempt(&self.request(timeout)).unwrap()
    }
    fn ready(&self) {
        until(|| self.0.join("ready").exists());
    }
    fn number(&self, name: &str) -> i32 {
        fs::read_to_string(self.0.join(name))
            .unwrap()
            .parse()
            .unwrap()
    }
    fn prove_empty(&self) {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
            fn getpgrp() -> i32;
        }
        let pid = self.number("pid");
        assert!(pid > 1);
        assert_eq!(pid, self.number("pgid"));
        assert_ne!(pid, unsafe { getpgrp() });
        for target in [pid, -pid].into_iter().chain(
            self.0
                .join("descendant-pid")
                .exists()
                .then(|| self.number("descendant-pid")),
        ) {
            // Observation only: signal zero never terminates or reaps anything.
            assert_eq!(unsafe { kill(target, 0) }, -1);
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(3)); // ESRCH
        }
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn helper() -> &'static Path {
    static HELPER_PATH: OnceLock<PathBuf> = OnceLock::new();
    HELPER_PATH.get_or_init(|| {
        let dir = Workspace::new();
        let source = dir.0.join("helper.rs");
        fs::write(&source, HELPER).unwrap();
        let binary =
            std::env::temp_dir().join(format!("receipts-codex-live-helper-{}", std::process::id()));
        let output = std::process::Command::new("rustc")
            .arg("--edition=2024")
            .arg(&source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "helper compile: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        binary
    })
}
fn until(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "condition not reached"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn complete(stdout: &[u8], stderr: &[u8], code: i32) -> crate::CodexLiveOutcome {
    let ws = Workspace::new();
    ws.write("stdout", stdout);
    ws.write("stderr", stderr);
    ws.write("exit", code.to_string());
    ws.write("release", "1");
    let attempt = ws.start(Duration::from_secs(10));
    let result = attempt.wait_collect().unwrap();
    assert_eq!(result.process().terminal_cause(), Cause::Completed);
    assert_eq!(result.process().exit_code(), Some(code));
    assert!(!result.process().forced_kill_required());
    ws.prove_empty();
    result
}

#[test]
fn natural_protocol_completion_and_nonzero_preserve_order_and_process_truth() {
    let stdout = format!(
        "{{\"type\":\"thread.started\",\"thread_id\":\"t\"}}\n{STARTED}{{\"type\":\"item.completed\",\"item\":{{\"id\":\"i\",\"type\":\"agent_message\",\"text\":\"hello 世界\"}}}}\n{COMPLETED}"
    );
    for code in [0, 7] {
        let result = complete(stdout.as_bytes(), COMPLETED.as_bytes(), code);
        let parsed = result.protocol().unwrap();
        assert_eq!(
            parsed.events().iter().map(|e| e.kind()).collect::<Vec<_>>(),
            [
                Kind::ThreadStarted,
                Kind::TurnStarted,
                Kind::ItemCompleted,
                Kind::TurnCompleted
            ]
        );
        assert_eq!(parsed.termination(), Protocol::Completed);
        assert!(parsed.turn_completed_observed());
        assert_eq!(parsed.final_agent_message(), Some("hello 世界"));
        assert_eq!(result.process().stderr().head(), COMPLETED.as_bytes());
        assert_eq!(result.sandbox_mode(), Sandbox::ReadOnly);
    }
}

#[test]
fn cancel_before_completion_double_cancel_and_forced_cleanup() {
    for forced in [false, true] {
        let ws = Workspace::new();
        ws.write("stdout", "{\"type\":");
        ws.write("stderr", COMPLETED);
        if forced {
            ws.write("ignore", "1");
        }
        let attempt = ws.start(Duration::from_secs(10));
        ws.ready();
        assert_eq!(attempt.terminal_cause(), None);
        assert_eq!(attempt.cancel(), Acceptance::CancelAccepted);
        assert_eq!(attempt.cancel(), Acceptance::AlreadyTerminalOrTerminating);
        let result = attempt.wait_collect().unwrap();
        assert_eq!(result.process().terminal_cause(), Cause::Cancelled);
        assert_eq!(result.process().forced_kill_required(), forced);
        assert_eq!(
            result.protocol().err(),
            Some(CodexLiveProtocolError::Jsonl(crate::CodexJsonlError {
                line: 1,
                kind: ErrorKind::IncompleteJson
            }))
        );
        ws.prove_empty();
    }
}

#[test]
fn timeout_before_cancel_preserves_cause_and_forced_fact() {
    for forced in [false, true] {
        let ws = Workspace::new();
        ws.write("stdout", "{\"type\":");
        ws.write("stderr", COMPLETED);
        if forced {
            ws.write("ignore", "1");
        }
        let attempt = ws.start(Duration::from_secs(2));
        ws.ready();
        until(|| attempt.terminal_cause() == Some(Cause::TimedOut));
        assert_eq!(attempt.cancel(), Acceptance::AlreadyTerminalOrTerminating);
        let result = attempt.wait_collect().unwrap();
        assert_eq!(result.process().terminal_cause(), Cause::TimedOut);
        assert_eq!(result.process().forced_kill_required(), forced);
        assert!(result.protocol().is_err());
        ws.prove_empty();
    }
}

#[test]
fn completion_before_cancel_and_snapshot_after_single_collection() {
    let ws = Workspace::new();
    ws.write("stdout", STARTED);
    ws.write("release", "1");
    let attempt = ws.start(Duration::from_secs(10));
    until(|| attempt.terminal_cause() == Some(Cause::Completed));
    assert_eq!(attempt.cancel(), Acceptance::AlreadyTerminalOrTerminating);
    let result = attempt.wait_collect().unwrap();
    assert_eq!(result.process().terminal_cause(), Cause::Completed);
    assert!(matches!(
        attempt.wait_collect(),
        Err(LiveProcessAttemptError::AlreadyCollectedOrCollecting)
    ));
    let snapshot = attempt.snapshot_output().unwrap();
    assert_eq!(snapshot.output().stdout(), result.process().stdout());
    assert_eq!(
        snapshot.protocol().unwrap().termination(),
        Protocol::Indeterminate
    );
}

#[test]
fn partial_records_and_repeated_snapshots_are_bounded_independent_observations() {
    for partial in [false, true] {
        let ws = Workspace::new();
        let initial = if partial {
            format!("{STARTED}{{\"type\":")
        } else {
            STARTED.into()
        };
        ws.write("stdout", &initial);
        ws.write("suffix", if partial { &COMPLETED[8..] } else { COMPLETED });
        let attempt = ws.start(Duration::from_secs(10));
        ws.ready();
        until(|| {
            attempt
                .snapshot_output()
                .unwrap()
                .output()
                .stdout()
                .total_bytes()
                == initial.len() as u64
        });
        let first = attempt.snapshot_output().unwrap();
        for _ in 0..20 {
            let snapshot = attempt.snapshot_output().unwrap();
            assert_eq!(snapshot.output(), first.output());
            assert!(snapshot.output().stdout().captured_bytes() <= STREAM_CAPTURE_LIMIT_BYTES);
            assert_eq!(attempt.terminal_cause(), None);
            if partial {
                assert_eq!(
                    snapshot.protocol().err().unwrap(),
                    CodexLiveProtocolError::Jsonl(crate::CodexJsonlError {
                        line: 2,
                        kind: ErrorKind::IncompleteJson
                    })
                );
            } else {
                assert_eq!(snapshot.protocol().unwrap().events().len(), 1);
            }
        }
        ws.write("release", "1");
        let result = attempt.wait_collect().unwrap();
        assert_eq!(result.protocol().unwrap().events().len(), 2);
        assert_eq!(
            result.protocol().unwrap().termination(),
            Protocol::Completed
        );
        assert_eq!(first.output().stdout().total_bytes(), initial.len() as u64);
    }
}

#[test]
fn malformed_invalid_utf8_and_incomplete_preserve_lifecycle_stderr_cannot_rescue() {
    for (stdout, kind) in [
        (b"{bad}".as_slice(), ErrorKind::InvalidJson),
        (b"\xff", ErrorKind::InvalidUtf8),
        (b"{\"type\":", ErrorKind::IncompleteJson),
    ] {
        let result = complete(stdout, COMPLETED.as_bytes(), 0);
        let error = result.protocol().err().unwrap();
        assert_eq!(
            error,
            CodexLiveProtocolError::Jsonl(crate::CodexJsonlError { line: 1, kind })
        );
        assert_eq!(result.process().stdout().head(), stdout);
    }
}

#[test]
fn unknown_event_item_and_status_are_exact_ordered_evidence() {
    let future = "{\"type\":\"future.世界\",\"value\":42}\n";
    let item = "{\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"future_item\",\"status\":\"future_status\"}}\n";
    let result = complete(format!("{STARTED}{future}{item}").as_bytes(), b"", 0);
    let parsed = result.protocol().unwrap();
    assert_eq!(parsed.events()[1].kind(), Kind::Unknown);
    assert_eq!(parsed.events()[1].raw_record(), future.as_bytes());
    assert_eq!(parsed.events()[2].raw_record(), item.as_bytes());
    assert_eq!(parsed.events()[2].item_type(), Some("future_item"));
    assert_eq!(parsed.events()[2].item_status(), Some("future_status"));
}

#[test]
fn truncation_never_parses_head_tail_and_stderr_truncation_is_separate() {
    for stdout_big in [false, true] {
        let ws = Workspace::new();
        let big = vec![b' '; STREAM_CAPTURE_LIMIT_BYTES as usize + 1];
        ws.write(
            "stdout",
            if stdout_big {
                &big
            } else {
                COMPLETED.as_bytes()
            },
        );
        ws.write("stderr", &big);
        let attempt = ws.start(Duration::from_secs(10));
        ws.ready();
        until(|| {
            attempt
                .snapshot_output()
                .unwrap()
                .output()
                .stderr()
                .total_bytes()
                == big.len() as u64
        });
        for _ in 0..5 {
            let snapshot = attempt.snapshot_output().unwrap();
            assert!(snapshot.output().stderr().truncated());
            assert!(snapshot.output().stdout().captured_bytes() <= STREAM_CAPTURE_LIMIT_BYTES);
            if stdout_big {
                assert_eq!(
                    snapshot.protocol().err(),
                    Some(CodexLiveProtocolError::StdoutTruncated)
                );
            } else {
                assert_eq!(
                    snapshot.protocol().unwrap().termination(),
                    Protocol::Completed
                );
            }
        }
        ws.write("release", "1");
        let result = attempt.wait_collect().unwrap();
        assert_eq!(result.process().stdout().truncated(), stdout_big);
        assert!(result.process().stderr().truncated());
        if stdout_big {
            assert_eq!(
                result.protocol().err(),
                Some(CodexLiveProtocolError::StdoutTruncated)
            );
        } else {
            assert_eq!(
                result.protocol().unwrap().termination(),
                Protocol::Completed
            );
        }
    }
}

#[test]
fn completion_without_protocol_terminal_and_contradictory_terminals_are_indeterminate() {
    for stdout in [
        String::new(),
        STARTED.into(),
        format!("{COMPLETED}{FAILED}"),
        format!("{FAILED}{COMPLETED}"),
    ] {
        let result = complete(stdout.as_bytes(), COMPLETED.as_bytes(), 0);
        assert_eq!(
            result.protocol().unwrap().termination(),
            Protocol::Indeterminate
        );
    }
}

#[test]
fn drop_before_collect_cleans_parent_and_descendant_via_workspace() {
    let ws = Workspace::new();
    ws.write("spawn-descendant", "1");
    let attempt = ws.start(Duration::from_secs(10));
    ws.ready();
    assert_eq!(ws.number("descendant-pgid"), ws.number("pid"));
    drop(attempt);
    ws.prove_empty();
}

#[test]
fn exact_shared_request_modes_prompts_and_validation_reach_real_boundary() {
    for mode in [Sandbox::ReadOnly, Sandbox::WorkspaceWrite] {
        for prompt in [
            "   ",
            "  hello\n世界 --sandbox danger-full-access $(false)  ",
        ] {
            let ws = Workspace::new();
            ws.write("prompt", prompt);
            ws.write("mode", mode.cli_value());
            ws.write("release", "1");
            let request = CodexTaskExecutionRequest::new(
                helper(),
                &ws.0,
                &ws.0,
                ProcessTimeoutPolicy::new(Duration::from_secs(10), Duration::from_millis(100))
                    .unwrap(),
                mode,
                prompt,
            );
            let outcome = start_codex_live_attempt(&request)
                .unwrap()
                .wait_collect()
                .unwrap();
            assert_eq!(outcome.process().exit_code(), Some(0));
            assert_eq!(outcome.sandbox_mode(), mode);
        }
    }
    let ws = Workspace::new();
    for (path, prompt) in [
        (helper().to_path_buf(), ""),
        (PathBuf::from("codex"), "fixture"),
        (PathBuf::from("/bin/sh"), "fixture"),
    ] {
        let request = CodexTaskExecutionRequest::new(
            path,
            &ws.0,
            &ws.0,
            *ws.request(Duration::from_secs(10)).timeout_policy(),
            Sandbox::ReadOnly,
            prompt,
        );
        let error = start_codex_live_attempt(&request).err().unwrap();
        if prompt.is_empty() {
            assert!(matches!(
                error,
                CodexLiveStartError::Request(CodexTaskExecutionError::EmptyPrompt)
            ));
        } else if matches!(&error, CodexLiveStartError::Workspace(_)) {
            assert!(matches!(
                error,
                CodexLiveStartError::Workspace(LiveProcessStartError::Execution(_))
            ));
        }
        assert!(!ws.0.join("ready").exists());
    }
}

#[test]
fn complete_stdout_across_head_tail_boundary_uses_same_parser() {
    let prefix = "{\"type\":\"future\",\"text\":\"";
    let suffix = "\"}\n";
    let stdout = format!(
        "{prefix}{}{suffix}",
        "x".repeat(STREAM_CAPTURE_LIMIT_BYTES as usize - prefix.len() - suffix.len())
    );
    let result = complete(stdout.as_bytes(), b"", 0);
    assert!(!result.process().stdout().truncated());
    assert!(!result.process().stdout().tail().is_empty());
    let parsed = result.protocol().unwrap();
    assert_eq!(parsed.events()[0].raw_record(), stdout.as_bytes());
}

#[test]
fn workspace_ownership_is_composed_without_runtime_control_authority() {
    fn shared<T: Send + Sync>() {}
    shared::<CodexLiveAttempt>();
    // Supplemental structural guard; real cancellation, timeout and Drop tests
    // above prove behavior through Workspace, not a mock lifecycle controller.
    let source = include_str!("codex_live_attempt.rs");
    for forbidden in [
        "std::process",
        "Command",
        "Child",
        "libc::",
        "nix::",
        "SIGTERM",
        "SIGKILL",
        "waitpid",
        "process_group",
        "setpgid",
        "killpg",
        "std::thread",
        "Instant",
        "AtomicBool",
        "Mutex",
        "impl Drop",
        "mem::forget",
        "ManuallyDrop",
    ] {
        assert!(
            !source.contains(forbidden),
            "Runtime control authority: {forbidden}"
        );
    }
    assert!(source.contains("attempt: LiveProcessAttempt"));
}

#[test]
fn bound_attempt_result_preserves_completed_process_and_protocol_cross_product() {
    use crate::{AttemptHandle, AttemptId};
    let unknown = "{\"type\":\"future.世界\"}\n";
    let item = "{\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"future_item\",\"status\":\"future_status\"}}\n";
    for code in [0, 7, 124, 137] {
        for stdout in [
            COMPLETED.into(),
            "{malformed}".into(),
            STARTED.into(),
            String::new(),
            format!("{COMPLETED}{FAILED}"),
            format!("{FAILED}{COMPLETED}"),
            format!("{STARTED}{unknown}{item}"),
        ] {
            let ws = Workspace::new();
            ws.write("stdout", &stdout);
            ws.write("stderr", COMPLETED);
            ws.write("exit", code.to_string());
            ws.write("release", "1");
            let id = AttemptId::new("caller/世界").unwrap();
            let handle = AttemptHandle::new(id.clone(), ws.start(Duration::from_secs(10)));
            assert_eq!(handle.id(), &id);
            let result = handle.collect_result().unwrap();
            assert_eq!(result.id(), &id);
            let codex = result.codex();
            assert_eq!(codex.process().terminal_cause(), Cause::Completed);
            assert_eq!(codex.process().exit_code(), Some(code));
            assert!(!codex.process().forced_kill_required());
            assert_eq!(codex.process().stdout().head(), stdout.as_bytes());
            assert_eq!(codex.process().stderr().head(), COMPLETED.as_bytes());
            assert_eq!(codex.sandbox_mode(), Sandbox::ReadOnly);
            if stdout == "{malformed}" {
                assert!(codex.protocol().is_err());
            } else {
                let protocol = codex.protocol().unwrap();
                assert_eq!(
                    protocol.termination(),
                    if stdout == COMPLETED {
                        Protocol::Completed
                    } else {
                        Protocol::Indeterminate
                    }
                );
                let records: Vec<_> = protocol
                    .events()
                    .iter()
                    .flat_map(|e| e.raw_record().iter().copied())
                    .collect();
                assert_eq!(records, stdout.as_bytes());
                if stdout.contains("future.世界") {
                    assert_eq!(protocol.events()[1].kind(), Kind::Unknown);
                    assert_eq!(protocol.events()[1].event_type(), "future.世界");
                    assert_eq!(protocol.events()[2].item_type(), Some("future_item"));
                    assert_eq!(protocol.events()[2].item_status(), Some("future_status"));
                }
                assert_eq!(protocol.final_agent_message(), None);
            }
            assert_eq!(handle.terminal_cause(), Some(Cause::Completed));
            assert_eq!(handle.cancel(), Acceptance::AlreadyTerminalOrTerminating);
            let error = handle.collect_result().err().unwrap();
            assert_eq!(
                error.evidence(),
                crate::RawFailureEvidence::AlreadyCollectedOrCollecting
            );
            assert_eq!(error.classify_failure(), crate::FailureClass::Unknown);
            ws.prove_empty();
        }
    }
}

#[test]
fn bound_attempt_cancellation_timeout_and_forced_cleanup_remain_workspace_truth() {
    use crate::{AttemptHandle, AttemptId};
    for timeout in [false, true] {
        for forced in [false, true] {
            let ws = Workspace::new();
            ws.write("stdout", "{\"type\":");
            ws.write("stderr", COMPLETED);
            if forced {
                ws.write("ignore", "1");
            }
            let id = AttemptId::new("same-caller-identity").unwrap();
            let handle = AttemptHandle::new(
                id.clone(),
                ws.start(Duration::from_secs(if timeout { 2 } else { 10 })),
            );
            ws.ready();
            let cause = if timeout {
                until(|| handle.terminal_cause() == Some(Cause::TimedOut));
                assert_eq!(handle.cancel(), Acceptance::AlreadyTerminalOrTerminating);
                Cause::TimedOut
            } else {
                assert_eq!(handle.cancel(), Acceptance::CancelAccepted);
                assert_eq!(handle.cancel(), Acceptance::AlreadyTerminalOrTerminating);
                Cause::Cancelled
            };
            let result = handle.collect_result().unwrap();
            assert_eq!(result.id(), &id);
            assert_eq!(handle.terminal_cause(), Some(cause));
            assert_eq!(result.codex().process().terminal_cause(), cause);
            assert_eq!(result.codex().process().forced_kill_required(), forced);
            assert_eq!(result.codex().process().stdout().head(), b"{\"type\":");
            assert_eq!(
                result.codex().process().stderr().head(),
                COMPLETED.as_bytes()
            );
            let error = result.codex().protocol().err().unwrap();
            assert_eq!(
                crate::RawFailure::from(&error).classify_failure(),
                crate::FailureClass::Unknown
            );
            ws.prove_empty();
        }
    }
}

#[test]
fn bound_attempt_snapshots_truncation_and_drop_reuse_the_real_live_attempt() {
    use crate::{AttemptHandle, AttemptId};
    fn shared<T: Send + Sync>() {}
    shared::<AttemptHandle>();
    for truncated in [false, true] {
        let ws = Workspace::new();
        let stdout = if truncated {
            vec![b' '; STREAM_CAPTURE_LIMIT_BYTES as usize + 1]
        } else {
            STARTED.as_bytes().to_vec()
        };
        ws.write("stdout", &stdout);
        ws.write("stderr", COMPLETED);
        let handle = AttemptHandle::new(
            AttemptId::new("bounded").unwrap(),
            ws.start(Duration::from_secs(10)),
        );
        ws.ready();
        until(|| {
            handle
                .snapshot_output()
                .unwrap()
                .output()
                .stdout()
                .total_bytes()
                == stdout.len() as u64
        });
        for _ in 0..3 {
            let snapshot = handle.snapshot_output().unwrap();
            assert!(snapshot.output().stdout().captured_bytes() <= STREAM_CAPTURE_LIMIT_BYTES);
            assert_eq!(snapshot.output().stdout().truncated(), truncated);
            assert_eq!(handle.terminal_cause(), None);
            if truncated {
                assert_eq!(
                    snapshot.protocol().err(),
                    Some(CodexLiveProtocolError::StdoutTruncated)
                );
            } else {
                assert_eq!(snapshot.protocol().unwrap().events().len(), 1);
            }
        }
        ws.write("release", "1");
        let result = handle.collect_result().unwrap();
        assert_eq!(result.codex().process().terminal_cause(), Cause::Completed);
        assert_eq!(result.codex().process().stdout().truncated(), truncated);
        if truncated {
            assert_eq!(
                result.codex().protocol().err(),
                Some(CodexLiveProtocolError::StdoutTruncated)
            );
        }
        ws.prove_empty();
    }
    let ws = Workspace::new();
    ws.write("spawn-descendant", "1");
    let handle = AttemptHandle::new(
        AttemptId::new("drop-owned").unwrap(),
        ws.start(Duration::from_secs(10)),
    );
    ws.ready();
    drop(handle);
    ws.prove_empty();
}

#[test]
fn codex_live_start_and_collection_error_types_remain_distinct() {
    let _: fn(&CodexLiveAttempt) -> Result<crate::CodexLiveOutcome, LiveProcessAttemptError> =
        CodexLiveAttempt::wait_collect;
    let start = CodexLiveStartError::Workspace(LiveProcessStartError::ControllerFailed);
    assert!(matches!(
        start,
        CodexLiveStartError::Workspace(LiveProcessStartError::ControllerFailed)
    ));
}
