//! Deterministic helpers only: no installed Claude/model/subscription is invoked.
//! All execution and lifecycle evidence comes from the real Workspace process APIs.
use crate::claude_execution::build_claude_task_request;
use crate::*;
use receipts_workspace_execution::execution::{
    ExecutionError, LiveProcessAttemptError, LiveProcessCancelAcceptance as Acceptance,
    LiveProcessStartError, LiveProcessTerminalCause as Cause, MAX_STDIN_BYTES, ProcessStdin,
    ProcessTermination, ProcessTimeoutPolicy, STREAM_CAPTURE_LIMIT_BYTES,
    start_live_process_attempt,
};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

const MARKER: &str = "sk-test-NOT-A-REAL-CREDENTIAL-A3-013";
const RECORDS: &str = "{\"type\":\"system\",\"subtype\":\"init\",\"future\":17}\n{\"type\":\"future.kind\",\"content\":\"世界 😀 é\",\"extra\":{\"x\":true}}\n{\"type\":\"result\",\"is_error\":true,\"future\":[1,2]}\n";
const HELPER: &str = r#"
use std::{fs,io::{Read,Write},path::Path,time::{Duration,Instant}};
unsafe extern "C" { fn signal(sig:i32,handler:usize)->usize; fn getpgrp()->i32; }
fn until(mut f:impl FnMut()->bool) { let t=Instant::now(); while !f() { assert!(t.elapsed()<Duration::from_secs(30)); std::thread::sleep(Duration::from_millis(2)); } }
fn main() {
 let args:Vec<_>=std::env::args().collect();
 let leaf=args.get(1).map(String::as_str)==Some("leaf");
 if leaf || Path::new("ignore").exists() { unsafe {signal(15,1);} }
 let prefix=if leaf {"leaf-"} else {""};
 fs::write(format!("{prefix}pid"),std::process::id().to_string()).unwrap();
 fs::write(format!("{prefix}pgid"),unsafe {getpgrp()}.to_string()).unwrap();
 if leaf { fs::write("leaf-ready",b"1").unwrap(); until(||Path::new("leaf-release").exists()); return; }
 assert!(std::env::vars_os().next().is_none());
 if args.get(1).map(String::as_str)==Some("auth") {
   assert_eq!(&args[1..], &["auth","status"]);
 } else {
   let mode=fs::read_to_string("mode").unwrap();
   assert_eq!(&args[1..], &["--print","--input-format","stream-json","--output-format","stream-json","--no-session-persistence","--permission-mode",mode.as_str(),"--verbose"]);
 }
 if Path::new("no-read").exists() { fs::write("ready",b"1").unwrap(); until(||Path::new("release").exists()); return; }
 let mut input=Vec::new(); std::io::stdin().read_to_end(&mut input).unwrap();
 fs::write("received",&input).unwrap();
 if args[1]=="auth" { assert!(input.is_empty()); }
 else { assert_eq!(input,fs::read("expected").unwrap()); }
 if Path::new("spawn-leaf").exists() {
   let _child=std::process::Command::new(std::env::current_exe().unwrap()).arg("leaf").spawn().unwrap();
   until(||Path::new("leaf-ready").exists());
 }
 std::io::stdout().write_all(&fs::read("stdout").unwrap()).unwrap(); std::io::stdout().flush().unwrap();
 std::io::stderr().write_all(&fs::read("stderr").unwrap()).unwrap(); std::io::stderr().flush().unwrap();
 fs::write("ready",b"1").unwrap();
 until(||Path::new("release").exists());
 std::io::stdout().write_all(&fs::read("suffix").unwrap()).unwrap(); std::io::stdout().flush().unwrap();
 fs::write("suffix-ready",b"1").unwrap();
 if Path::new("hold-suffix").exists() { until(||Path::new("finish").exists()); }
 std::process::exit(fs::read_to_string("exit").unwrap().parse().unwrap());
}
"#;
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("receipts-claude-a3-013-{}-{n}", std::process::id()));
        fs::create_dir(&path).unwrap();
        let ws = Self(fs::canonicalize(path).unwrap());
        for (name, bytes) in [
            ("stdout", ""),
            ("stderr", ""),
            ("suffix", ""),
            ("exit", "0"),
            ("mode", "plan"),
        ] {
            ws.write(name, bytes);
        }
        ws
    }
    fn write(&self, name: &str, bytes: impl AsRef<[u8]>) {
        fs::write(self.0.join(name), bytes).unwrap();
    }
    fn request(
        &self,
        prompt: &str,
        mode: ClaudePermissionMode,
        timeout: Duration,
    ) -> ClaudeTaskExecutionRequest {
        self.write(
            "mode",
            match mode {
                ClaudePermissionMode::Plan => "plan",
                ClaudePermissionMode::AcceptEdits => "acceptEdits",
            },
        );
        // Independent semantic oracle, not the Runtime encoder.
        self.write("expected",format!("{}\n",serde_json::json!({"type":"user","session_id":"","message":{"role":"user","content":prompt},"parent_tool_use_id":null})));
        ClaudeTaskExecutionRequest::new(helper(), &self.0, &self.0, policy(timeout), mode, prompt)
    }
    fn ready(&self) {
        until(|| self.0.join("ready").exists());
    }
    fn prove_empty(&self) {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
            fn getpgrp() -> i32;
        }
        let number = |name| {
            fs::read_to_string(self.0.join(name))
                .unwrap()
                .parse::<i32>()
                .unwrap()
        };
        let pid = number("pid");
        assert!(pid > 1);
        assert_eq!(pid, number("pgid"));
        assert_ne!(pid, unsafe { getpgrp() });
        for target in [pid, -pid]
            .into_iter()
            .chain(self.0.join("leaf-pid").exists().then(|| number("leaf-pid")))
        {
            // Observation only; Runtime tests never signal or reap the helper.
            assert_eq!(unsafe { kill(target, 0) }, -1);
            assert_eq!(std::io::Error::last_os_error().raw_os_error(), Some(3));
        }
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn helper() -> &'static Path {
    static BINARY: OnceLock<PathBuf> = OnceLock::new();
    BINARY.get_or_init(|| {
        let ws = Workspace::new();
        let source = ws.0.join("helper.rs");
        fs::write(&source, HELPER).unwrap();
        let binary = std::env::temp_dir().join(format!(
            "receipts-claude-a3-013-helper-{}",
            std::process::id()
        ));
        let output = std::process::Command::new("rustc")
            .args(["--edition=2024"])
            .arg(source)
            .arg("-o")
            .arg(&binary)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        binary
    })
}
fn policy(timeout: Duration) -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(timeout, Duration::from_millis(100)).unwrap()
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
fn input(request: &receipts_workspace_execution::execution::ProcessRunRequest) -> &[u8] {
    match request.stdin() {
        ProcessStdin::Bytes(bytes) => bytes.as_bytes(),
        ProcessStdin::Closed => panic!("missing stdin"),
    }
}
fn complete(stdout: &[u8], stderr: &[u8], code: i32) -> ClaudeTaskExecutionResult {
    let ws = Workspace::new();
    ws.write("stdout", stdout);
    ws.write("stderr", stderr);
    ws.write("exit", code.to_string());
    ws.write("release", "1");
    let result = execute_claude_task_once(&ws.request(
        "fixture",
        ClaudePermissionMode::Plan,
        Duration::from_secs(10),
    ))
    .unwrap();
    assert_eq!(
        result.process().outcome().termination(),
        ProcessTermination::Completed
    );
    assert_eq!(result.process().outcome().exit_code(), Some(code));
    ws.prove_empty();
    result
}
fn safe(error: &(dyn Error + 'static)) {
    assert!(!format!("{error:?} {error}").contains(MARKER));
    if let Some(source) = error.source() {
        safe(source);
    }
}

#[test]
fn fixed_argv_and_exact_structured_stdin_preserve_all_prompt_semantics() {
    for mode in [
        ClaudePermissionMode::Plan,
        ClaudePermissionMode::AcceptEdits,
    ] {
        for prompt in [
            "--version",
            "--help",
            "-x",
            "--",
            "--permission-mode bypassPermissions",
            "--model x",
            "quotes \" ' backslash \\",
            "\n\r\n\r\t",
            "世界 😀 e\u{301}",
            "  fn main() {\n\tprintln!(\"hello\\n\");\n}\n  ",
            " \t\r\n ",
            "",
            MARKER,
        ] {
            let ws = Workspace::new();
            ws.write("release", "1");
            let request = ws.request(prompt, mode, Duration::from_secs(10));
            let built = build_claude_task_request(&request).unwrap();
            assert!(built.executable().is_absolute());
            assert_eq!(built.executable(), helper());
            let expected = [
                "--print",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--no-session-persistence",
                "--permission-mode",
                if mode == ClaudePermissionMode::Plan {
                    "plan"
                } else {
                    "acceptEdits"
                },
                "--verbose",
            ];
            assert_eq!(built.arguments(), expected.map(std::ffi::OsString::from));
            assert!(!built.arguments().iter().any(|arg| arg == prompt));
            let bytes = input(&built);
            assert_eq!(bytes.last(), Some(&b'\n'));
            assert_eq!(bytes.iter().filter(|b| **b == b'\n').count(), 1);
            let decoded: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(
                decoded,
                serde_json::json!({"type":"user","session_id":"","message":{"role":"user","content":prompt},"parent_tool_use_id":null})
            );
            let result = execute_claude_task_once(&request).unwrap();
            assert!(result.process().outcome().success());
            assert_eq!(fs::read(ws.0.join("received")).unwrap(), bytes);
            ws.prove_empty();
        }
    }
}

#[test]
fn encoded_bound_includes_json_escaping_and_newline_and_prevents_spawn() {
    let ws = Workspace::new();
    ws.write("release", "1");
    let empty = ws.request("", ClaudePermissionMode::Plan, Duration::from_secs(10));
    let overhead = input(&build_claude_task_request(&empty).unwrap()).len();
    let room = MAX_STDIN_BYTES - overhead;
    let prompt = "a".repeat(room);
    let exact = ws.request(&prompt, ClaudePermissionMode::Plan, Duration::from_secs(10));
    assert_eq!(
        input(&build_claude_task_request(&exact).unwrap()).len(),
        MAX_STDIN_BYTES
    );
    assert!(
        execute_claude_task_once(&exact)
            .unwrap()
            .process()
            .outcome()
            .success()
    );
    assert_eq!(
        fs::read(ws.0.join("received")).unwrap().len(),
        MAX_STDIN_BYTES
    );
    ws.prove_empty();
    for prompt in [
        "a".repeat(room + 1),
        "\"".repeat(room / 2 + 1),
        "\0".repeat(room / 6 + 1),
    ] {
        let fresh = Workspace::new();
        let request = fresh.request(&prompt, ClaudePermissionMode::Plan, Duration::from_secs(10));
        let error = execute_claude_task_once(&request).err().unwrap();
        assert!(
            matches!(error.0,ExecutionError::StdinPayloadTooLarge{len,max} if len>max && max==MAX_STDIN_BYTES)
        );
        assert!(matches!(
            start_claude_live_attempt(&request),
            Err(ClaudeLiveStartError::Request(ClaudeTaskExecutionError(
                ExecutionError::StdinPayloadTooLarge { .. }
            )))
        ));
        assert!(!fresh.0.join("pid").exists());
    }
}

#[test]
fn relative_executable_and_shell_are_rejected_by_workspace_without_spawn() {
    let ws = Workspace::new();
    for executable in [Path::new("relative/claude"), Path::new("/bin/sh")] {
        let request = ClaudeTaskExecutionRequest::new(
            executable,
            &ws.0,
            &ws.0,
            policy(Duration::from_secs(10)),
            ClaudePermissionMode::Plan,
            MARKER,
        );
        let error = execute_claude_task_once(&request).err().unwrap();
        assert!(matches!(
            error.0,
            ExecutionError::ExecutablePathNotAbsolute { .. }
                | ExecutionError::ShellExecutableRejected { .. }
        ));
        safe(&error);
        safe(&RawFailure::from(&error));
        assert!(!ws.0.join("pid").exists());
    }
}

#[test]
fn one_shot_preserves_order_unknown_objects_fields_unicode_and_independent_exit_truth() {
    for code in [0, 7] {
        let result = complete(RECORDS.as_bytes(), b"{\"type\":\"stderr-only\"}\n", code);
        assert_eq!(result.process().outcome().success(), code == 0);
        assert_eq!(
            classify_claude_task_execution_result(&result),
            if code == 0 {
                None
            } else {
                Some(FailureClass::Unknown)
            }
        );
        let protocol = result.protocol().unwrap();
        assert_eq!(protocol.records().len(), 3);
        for (record, line) in protocol.records().iter().zip(RECORDS.split_inclusive('\n')) {
            assert_eq!(record.raw_record(), line.as_bytes());
            assert_eq!(
                record.value(),
                &serde_json::from_str::<serde_json::Value>(line).unwrap()
            );
        }
        // Even exit 0 + is_error:true has no invented worker-success projection.
        assert_eq!(protocol.records()[2].value()["is_error"], true);
        assert_eq!(
            result.process().stderr().head(),
            b"{\"type\":\"stderr-only\"}\n"
        );
    }
}

#[test]
fn malformed_utf8_incomplete_garbage_and_nonobjects_fail_closed_stderr_cannot_rescue() {
    use ClaudeStreamJsonError::*;
    for (bytes, expected) in [
        (&b"{bad}\n"[..], InvalidJson { line: 1 }),
        (&b"\xff\n"[..], InvalidUtf8 { line: 1 }),
        (&b"{\"type\":"[..], IncompleteJson { line: 1 }),
        (&b"garbage\n"[..], InvalidJson { line: 1 }),
        (&b"[]\n"[..], ExpectedObject { line: 1 }),
        (&b"{}\n\n"[..], IncompleteJson { line: 2 }),
    ] {
        let result = complete(bytes, RECORDS.as_bytes(), 0);
        assert_eq!(result.protocol().err(), Some(expected));
        assert!(result.process().outcome().success());
        let raw = RawFailure::from(&expected);
        assert_eq!(raw.classify_failure(), FailureClass::Unknown);
        safe(&raw);
    }
    let empty = complete(b"", RECORDS.as_bytes(), 0);
    assert!(empty.protocol().unwrap().records().is_empty());
    let final_without_newline = complete(b"{}", b"", 0);
    assert_eq!(final_without_newline.protocol().unwrap().records().len(), 1);
}

#[test]
fn stdout_truncation_fails_closed_and_stderr_truncation_never_changes_protocol() {
    let large = "{}\n".repeat(usize::try_from(STREAM_CAPTURE_LIMIT_BYTES).unwrap() / 3 + 20);
    let result = complete(large.as_bytes(), RECORDS.as_bytes(), 0);
    assert!(result.process().stdout().truncated());
    assert_eq!(
        result.protocol().err(),
        Some(ClaudeStreamJsonError::StdoutTruncated)
    );
    assert!(result.process().stdout().captured_bytes() <= STREAM_CAPTURE_LIMIT_BYTES);
    let result = complete(RECORDS.as_bytes(), large.as_bytes(), 7);
    assert!(result.process().stderr().truncated());
    assert_eq!(result.protocol().unwrap().records().len(), 3);
}

#[test]
fn live_uses_same_input_for_both_modes_and_preserves_natural_zero_and_nonzero_completion() {
    for mode in [
        ClaudePermissionMode::Plan,
        ClaudePermissionMode::AcceptEdits,
    ] {
        for code in [0, 9] {
            let ws = Workspace::new();
            ws.write("stdout", RECORDS);
            ws.write("stderr", MARKER);
            ws.write("exit", code.to_string());
            let attempt =
                start_claude_live_attempt(&ws.request("--version", mode, Duration::from_secs(10)))
                    .unwrap();
            ws.ready();
            ws.write("release", "1");
            until(|| attempt.terminal_cause() == Some(Cause::Completed));
            assert_eq!(attempt.cancel(), Acceptance::AlreadyTerminalOrTerminating);
            let result = attempt.wait_collect().unwrap();
            assert_eq!(result.process().terminal_cause(), Cause::Completed);
            assert_eq!(result.process().exit_code(), Some(code));
            assert_eq!(result.process().success(), code == 0);
            assert!(!result.process().forced_kill_required());
            assert_eq!(result.protocol().unwrap().records().len(), 3);
            assert_eq!(
                classify_claude_live_outcome(&result),
                if code == 0 {
                    None
                } else {
                    Some(FailureClass::Unknown)
                }
            );
            let error = attempt.wait_collect().err().unwrap();
            assert!(matches!(
                error,
                LiveProcessAttemptError::AlreadyCollectedOrCollecting
            ));
            safe(&RawFailure::from(&error));
            ws.prove_empty();
        }
    }
}

#[test]
fn live_cancel_timeout_and_forced_kill_are_independent_workspace_facts() {
    for timed_out in [false, true] {
        for forced in [false, true] {
            let ws = Workspace::new();
            ws.write("stdout", "{\"type\":");
            ws.write("stderr", "rate limit login required UserCancelled");
            if forced {
                ws.write("ignore", "1");
            }
            let timeout = if timed_out {
                Duration::from_millis(700)
            } else {
                Duration::from_secs(10)
            };
            let attempt = start_claude_live_attempt(&ws.request(
                "fixture",
                ClaudePermissionMode::Plan,
                timeout,
            ))
            .unwrap();
            ws.ready();
            if timed_out {
                until(|| attempt.terminal_cause() == Some(Cause::TimedOut));
                assert_eq!(attempt.cancel(), Acceptance::AlreadyTerminalOrTerminating);
            } else {
                assert_eq!(attempt.cancel(), Acceptance::CancelAccepted);
                assert_eq!(attempt.cancel(), Acceptance::AlreadyTerminalOrTerminating);
            }
            let result = attempt.wait_collect().unwrap();
            assert_eq!(
                result.process().terminal_cause(),
                if timed_out {
                    Cause::TimedOut
                } else {
                    Cause::Cancelled
                }
            );
            assert_eq!(result.process().forced_kill_required(), forced);
            assert!(!result.process().success());
            assert_eq!(
                result.protocol().err(),
                Some(ClaudeStreamJsonError::IncompleteJson { line: 1 })
            );
            assert_eq!(
                classify_claude_live_outcome(&result),
                Some(if timed_out {
                    FailureClass::Timeout
                } else {
                    FailureClass::Unknown
                })
            );
            ws.prove_empty();
        }
    }
}

#[test]
fn live_partial_snapshots_can_later_parse_without_accumulation_or_lifecycle_inference() {
    let ws = Workspace::new();
    ws.write("stdout", "{\"type\":");
    ws.write("suffix", "\"future\",\"data\":\"世界\"}\n");
    ws.write("hold-suffix", "1");
    ws.write("stderr", RECORDS);
    let attempt = start_claude_live_attempt(&ws.request(
        "fixture",
        ClaudePermissionMode::Plan,
        Duration::from_secs(10),
    ))
    .unwrap();
    ws.ready();
    until(|| {
        attempt
            .snapshot_output()
            .unwrap()
            .output()
            .stdout()
            .total_bytes()
            == 8
    });
    for _ in 0..20 {
        let snapshot = attempt.snapshot_output().unwrap();
        assert_eq!(
            snapshot.protocol().err(),
            Some(ClaudeStreamJsonError::IncompleteJson { line: 1 })
        );
        assert_eq!(attempt.terminal_cause(), None);
        assert!(snapshot.output().stdout().captured_bytes() <= STREAM_CAPTURE_LIMIT_BYTES);
    }
    ws.write("release", "1");
    until(|| attempt.snapshot_output().unwrap().protocol().is_ok());
    assert_eq!(attempt.terminal_cause(), None);
    assert_eq!(
        attempt
            .snapshot_output()
            .unwrap()
            .protocol()
            .unwrap()
            .records()
            .len(),
        1
    );
    ws.write("finish", "1");
    assert!(attempt.wait_collect().unwrap().protocol().is_ok());
    ws.prove_empty();
}

#[test]
fn repeated_truncated_live_snapshots_never_join_head_and_tail() {
    let ws = Workspace::new();
    ws.write(
        "stdout",
        "{}\n".repeat(usize::try_from(STREAM_CAPTURE_LIMIT_BYTES).unwrap() / 3 + 20),
    );
    let attempt = start_claude_live_attempt(&ws.request(
        "fixture",
        ClaudePermissionMode::Plan,
        Duration::from_secs(10),
    ))
    .unwrap();
    ws.ready();
    until(|| {
        attempt
            .snapshot_output()
            .unwrap()
            .output()
            .stdout()
            .truncated()
    });
    for _ in 0..20 {
        let snapshot = attempt.snapshot_output().unwrap();
        assert_eq!(
            snapshot.protocol().err(),
            Some(ClaudeStreamJsonError::StdoutTruncated)
        );
        assert_eq!(
            snapshot.output().stdout().captured_bytes(),
            STREAM_CAPTURE_LIMIT_BYTES
        );
    }
    ws.write("release", "1");
    let result = attempt.wait_collect().unwrap();
    assert_eq!(
        result.protocol().err(),
        Some(ClaudeStreamJsonError::StdoutTruncated)
    );
    ws.prove_empty();
}

#[test]
fn drop_before_collect_and_orphan_cleanup_remain_workspace_owned() {
    for drop_early in [false, true] {
        let ws = Workspace::new();
        ws.write("spawn-leaf", "1");
        let attempt = start_claude_live_attempt(&ws.request(
            "fixture",
            ClaudePermissionMode::Plan,
            Duration::from_millis(700),
        ))
        .unwrap();
        ws.ready();
        if drop_early {
            drop(attempt);
        } else {
            ws.write("release", "1");
            let result = attempt.wait_collect().unwrap();
            // The orphan keeps both output pipes open and ignores SIGTERM.
            // Workspace requires reader EOF and group cleanup for completion.
            assert_eq!(result.process().terminal_cause(), Cause::TimedOut);
            assert!(result.process().forced_kill_required());
        }
        ws.prove_empty();
    }
}

#[test]
fn terminal_before_ready_timeout_is_real_and_payload_safe() {
    let ws = Workspace::new();
    ws.write("no-read", "1");
    ws.write("ignore", "1");
    let prompt = MARKER.repeat(16000); // Below bound, above pipe capacity; helper never reads.
    let request = ws.request(
        &prompt,
        ClaudePermissionMode::Plan,
        Duration::from_millis(700),
    );
    let error = start_claude_live_attempt(&request).err().unwrap();
    let ClaudeLiveStartError::Workspace(LiveProcessStartError::TerminalBeforeReady(ref outcome)) =
        error
    else {
        panic!("{error:?}");
    };
    assert_eq!(outcome.terminal_cause(), Cause::TimedOut);
    assert!(outcome.forced_kill_required());
    let raw = RawFailure::from(&error);
    assert_eq!(raw.origin(), RawFailureSource::ClaudeLiveStart);
    assert_eq!(raw.classify_failure(), FailureClass::Timeout);
    assert!(matches!(
        raw.evidence(),
        RawFailureEvidence::LiveTerminalBeforeReady {
            terminal_cause: Cause::TimedOut,
            forced_kill_required: true,
            ..
        }
    ));
    safe(&error);
    safe(&raw);
    ws.prove_empty();
}

#[test]
fn terminal_before_ready_completed_projection_preserves_real_workspace_outcome() {
    // Workspace's own stdin suite controls the private READY arbitration barrier.
    // Here the projection is exercised with a real collected Completed outcome,
    // not a fabricated lifecycle. The placement before READY is supplied explicitly.
    let ws = Workspace::new();
    ws.write("stdout", MARKER);
    ws.write("stderr", MARKER);
    ws.write("exit", "7");
    ws.write("release", "1");
    let request = ws.request(
        "fixture",
        ClaudePermissionMode::Plan,
        Duration::from_secs(10),
    );
    let process = build_claude_task_request(&request).unwrap();
    let attempt = start_live_process_attempt(&process, request.timeout_policy()).unwrap();
    let outcome = attempt.wait_collect().unwrap();
    assert_eq!(outcome.terminal_cause(), Cause::Completed);
    let error = ClaudeLiveStartError::Workspace(LiveProcessStartError::TerminalBeforeReady(
        Box::new(outcome),
    ));
    let raw = RawFailure::from(&error);
    assert_eq!(raw.classify_failure(), FailureClass::Unknown);
    assert_eq!(
        raw.evidence(),
        RawFailureEvidence::LiveTerminalBeforeReady {
            terminal_cause: Cause::Completed,
            forced_kill_required: false,
            process_success: false,
            exit_code: Some(7)
        }
    );
    safe(&error);
    safe(&raw);
    ws.prove_empty();
}

#[test]
fn auth_exact_argv_exit_mapping_discards_payload_without_policy_or_expiry_inference() {
    for (code, expected) in [
        (0, RuntimeAuthStatus::Connected),
        (1, RuntimeAuthStatus::AuthRequired),
        (2, RuntimeAuthStatus::Unknown),
        (7, RuntimeAuthStatus::Unknown),
    ] {
        let ws = Workspace::new();
        ws.write(
            "stdout",
            format!("{{\"token\":\"{MARKER}\",\"expired\":true,\"loggedIn\":false}}"),
        );
        ws.write(
            "stderr",
            format!("{MARKER} login required rate limit expired"),
        );
        ws.write("exit", code.to_string());
        ws.write("release", "1");
        let status =
            observe_claude_auth_status(helper(), &ws.0, &ws.0, &policy(Duration::from_secs(10)))
                .unwrap();
        assert_eq!(status, expected);
        assert!(!format!("{status:?}").contains(MARKER));
        assert!(fs::read(ws.0.join("received")).unwrap().is_empty());
        ws.prove_empty();
        // The return type contains only technical status, no eligibility/routing grant.
    }
}

#[test]
fn auth_execution_and_timeout_errors_are_typed_conservative_and_secret_safe() {
    let ws = Workspace::new();
    let error = observe_claude_auth_status(
        ws.0.join(MARKER),
        &ws.0,
        &ws.0,
        &policy(Duration::from_secs(10)),
    )
    .unwrap_err();
    assert!(matches!(error, ClaudeAuthStatusError::Workspace(_)));
    assert_eq!(
        RawFailure::from(&error).classify_failure(),
        FailureClass::Unknown
    );
    safe(&error);
    safe(&RawFailure::from(&error));
    ws.write("stdout", MARKER);
    ws.write("stderr", MARKER);
    let error =
        observe_claude_auth_status(helper(), &ws.0, &ws.0, &policy(Duration::from_millis(700)))
            .unwrap_err();
    assert!(matches!(error, ClaudeAuthStatusError::TimedOut(_)));
    assert_eq!(
        RawFailure::from(&error).classify_failure(),
        FailureClass::Timeout
    );
    safe(&error);
    safe(&RawFailure::from(&error));
    ws.prove_empty();
    // Invalid caller construction cannot turn Completed into typed timeout evidence.
    assert_eq!(
        RawFailure::from(&ClaudeAuthStatusError::TimedOut(
            ProcessTermination::Completed
        ))
        .classify_failure(),
        FailureClass::Unknown
    );
}

#[test]
fn task_timeout_only_uses_typed_evidence_and_diagnostic_prose_never_classifies() {
    for text in [
        "rate limit",
        "login required",
        "UserCancelled",
        "runtime crash",
        "invalid output",
        MARKER,
    ] {
        let result = complete(b"not json", text.as_bytes(), 9);
        assert_eq!(
            classify_claude_task_execution_result(&result),
            Some(FailureClass::Unknown)
        );
        assert_eq!(
            RawFailure::from(&result.protocol().err().unwrap()).classify_failure(),
            FailureClass::Unknown
        );
    }
    let ws = Workspace::new();
    ws.write("stdout", RECORDS);
    ws.write("stderr", MARKER);
    let result = execute_claude_task_once(&ws.request(
        "fixture",
        ClaudePermissionMode::Plan,
        Duration::from_millis(700),
    ))
    .unwrap();
    assert!(result.process().outcome().timed_out());
    assert!(result.protocol().is_ok());
    assert_eq!(
        classify_claude_task_execution_result(&result),
        Some(FailureClass::Timeout)
    );
    ws.prove_empty();
    let error = ClaudeTaskExecutionError(ExecutionError::TimeoutFinalWaitFailed {
        detail: MARKER.into(),
    });
    assert_eq!(
        classify_claude_task_execution_error(&error),
        FailureClass::Unknown
    );
    safe(&error);
    safe(&RawFailure::from(&error));
}

#[test]
fn raw_failure_remains_fixed_size_payload_free_and_has_no_class_injection_constructor() {
    fn copy<T: Copy>() {}
    copy::<RawFailure>();
    assert!(!std::mem::needs_drop::<RawFailure>());
    assert!(std::mem::size_of::<RawFailure>() <= 64);
    let error = ClaudeLiveStartError::Workspace(LiveProcessStartError::Execution(
        ExecutionError::ProcessSpawnFailed {
            detail: MARKER.into(),
        },
    ));
    safe(&error);
    let raw = RawFailure::from(&error);
    safe(&raw);
    assert!(raw.source().is_none());
    assert_eq!(raw.classify_failure(), FailureClass::Unknown);
    assert_eq!(raw.origin(), RawFailureSource::ClaudeLiveStart);
}

#[test]
fn shared_jsonl_retains_codex_shape_error_priority_and_framing_contract() {
    let error = crate::codex_jsonl::interpret_stdout(b"[]\ngarbage\n")
        .err()
        .unwrap();
    assert_eq!(
        error,
        CodexJsonlError {
            line: 1,
            kind: CodexJsonlErrorKind::InvalidEventShape
        }
    );
    let error = crate::codex_jsonl::interpret_stdout(b"{\"type\":\"turn.started\"}\n\xff")
        .err()
        .unwrap();
    assert_eq!(
        error,
        CodexJsonlError {
            line: 2,
            kind: CodexJsonlErrorKind::InvalidUtf8
        }
    );
}
