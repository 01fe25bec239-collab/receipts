//! Prepared live-proof cases (docs/runtime-adapters/live-proof-plan.md).
//!
//! The two `live_*` tests are NOT RUN by any suite: each is `#[ignore]`d AND
//! returns unless its explicit opt-in variable is exactly `1`. Running one
//! executes a real provider CLI with the caller's existing login, so it
//! requires A1's separate context disposition first. These use the ORDINARY,
//! non-authoritative adapter path (protected launch is unavailable on Darwin);
//! a pass is compatibility evidence for that CLI version only, never
//! protected-launch or universal provider evidence. Raw output goes only to
//! the absolute private `RECEIPTS_LIVE_EVIDENCE_DIR`, which must lie outside
//! the repository. No credential file, token or environment value is read,
//! printed or copied.
//!
//! Each case returns a verdict and the live test panics on any failure, so a
//! silent exit, an unknown/invalid result, a missing first record, an
//! unaccepted cancellation or a failed/dirty Git inspection is never a pass.
//! The `synthetic_*` controls drive the same cases against owned synthetic
//! executables; they are harness evidence only, never provider evidence.
use super::*;
use crate::protected_launch_tests::{Fixture, block_on};
use crate::registry::{DynAdapter, DynAttemptEvent, DynAttemptResult};
use receipts_workspace_execution::execution::{
    CapturedStream, ChildEnvAuthMode, ChildEnvPolicy, ChildEnvProvider, LiveProcessOutcome,
    LiveProcessTerminalCause, ProcessTimeoutPolicy,
};
use std::{
    fs,
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const SUCCESS_OBJECTIVE: &str = "Reply with the single word OK. Do not create, modify or delete any file and do not run any command.";
const CANCEL_OBJECTIVE: &str = "Count slowly from 1 to 200, one number per line. Do not create, modify or delete any file and do not run any command.";
const GIT: &str = "/usr/bin/git";

struct Live {
    runtime: &'static str,
    executable: PathBuf,
    evidence: PathBuf,
}
fn opted_in(runtime: &'static str, flag: &str, exe: &str) -> Option<Live> {
    if std::env::var(flag).ok().as_deref() != Some("1") {
        return None;
    }
    let executable = PathBuf::from(std::env::var_os(exe).expect("explicit executable path"));
    let evidence =
        PathBuf::from(std::env::var_os("RECEIPTS_LIVE_EVIDENCE_DIR").expect("evidence dir"));
    assert!(executable.is_absolute() && evidence.is_absolute());
    let repo = fs::canonicalize(env!("CARGO_MANIFEST_DIR")).unwrap();
    let evidence_real = fs::canonicalize(&evidence).expect("existing evidence dir");
    assert!(
        !evidence_real.starts_with(repo.parent().unwrap().parent().unwrap()),
        "evidence must stay outside the repository"
    );
    Some(Live {
        runtime,
        executable,
        evidence: evidence_real,
    })
}
fn private_write(path: PathBuf, bytes: &[u8]) {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .unwrap()
        .write_all(bytes)
        .unwrap();
}

/// N11 evidence from one captured stream. Production JSONL strictness is not
/// affected; this only describes what a capture shows.
#[derive(Debug, PartialEq, Eq)]
enum BlankRecords {
    /// Complete capture. `records` counts newline-delimited records (a final
    /// unterminated record counts; the trailing delimiter is not a record);
    /// `blank` of them are empty. Empty output has zero records.
    Complete {
        records: usize,
        blank: usize,
        trailing_delimiter: bool,
    },
    /// Truncated capture: the middle is missing, so record boundaries across
    /// head and tail are unknown and nothing is counted.
    Unavailable,
}
fn blank_records(head: &[u8], tail: &[u8], truncated: bool) -> BlankRecords {
    if truncated {
        return BlankRecords::Unavailable;
    }
    // Capture contract: untruncated head followed by tail is the exact stream.
    let bytes = [head, tail].concat();
    let (body, trailing_delimiter) = match bytes.strip_suffix(b"\n") {
        Some(body) => (body, true),
        None => (&bytes[..], false),
    };
    if bytes.is_empty() {
        return BlankRecords::Complete {
            records: 0,
            blank: 0,
            trailing_delimiter,
        };
    }
    let records: Vec<_> = body.split(|b| *b == b'\n').collect();
    BlankRecords::Complete {
        records: records.len(),
        blank: records.iter().filter(|r| r.is_empty()).count(),
        trailing_delimiter,
    }
}

/// Raw bytes privately: the complete stream when untruncated, otherwise the
/// retained head and tail as SEPARATE fragments (never joined). Returns the
/// payload-free summary fields.
fn retain(evidence: &Path, stem: &str, stream: &str, s: &CapturedStream) -> String {
    if s.truncated() {
        private_write(evidence.join(format!("{stem}.{stream}.head")), s.head());
        private_write(evidence.join(format!("{stem}.{stream}.tail")), s.tail());
    } else {
        private_write(
            evidence.join(format!("{stem}.{stream}")),
            &[s.head(), s.tail()].concat(),
        );
    }
    format!(
        "{stream}_truncated={} {stream}_total_bytes={} {stream}_retained_bytes={}",
        s.truncated(),
        s.total_bytes(),
        s.captured_bytes()
    )
}

/// Payload-free facts of one collected attempt.
#[derive(Debug)]
struct Observed {
    cause: LiveProcessTerminalCause,
    exit: Option<i32>,
    forced_kill: bool,
    failure: Option<FailureClass>,
    /// Claude `Success` result / Codex `Completed` protocol termination.
    provider_success: bool,
    cancel: Option<CancelReason>,
    label: String,
}
fn observe(result: &DynAttemptResult) -> Result<(Observed, &LiveProcessOutcome), String> {
    let failure = result.failure();
    let obs = |p: &LiveProcessOutcome, provider_success, cancel, label| Observed {
        cause: p.terminal_cause(),
        exit: p.exit_code(),
        forced_kill: p.forced_kill_required(),
        failure,
        provider_success,
        cancel,
        label,
    };
    if let Some(r) = result.downcast_ref::<Result<ClaudeAttemptResult, FailureClass>>() {
        let r = r
            .as_ref()
            .map_err(|c| format!("collection failed: {c:?}"))?;
        let p = r.outcome.process();
        let provider = r.provider_result();
        let label = format!("provider={provider:?} protected={}", r.protected());
        let success = provider == ClaudeProviderResult::Success;
        return Ok((obs(p, success, r.cancel_reason(), label), p));
    }
    let r = result
        .downcast_ref::<Result<CodexAttemptResult, FailureClass>>()
        .ok_or("unexpected result type")?
        .as_ref()
        .map_err(|c| format!("collection failed: {c:?}"))?;
    let p = r.outcome.process();
    let protocol = r.protocol_termination();
    let label = format!("protocol={protocol:?} protected={}", r.protected());
    let success = protocol == CodexProtocolTermination::Completed;
    Ok((obs(p, success, r.cancel_reason(), label), p))
}

/// Git inspection must itself succeed, and the scratch worktree must be clean
/// at its base revision.
fn worktree_unchanged(git: &Path, f: &Fixture) -> Result<(), String> {
    let run = |args: &[&str]| {
        let out = Command::new(git)
            .current_dir(f.workspace.worktree_path())
            .args(args)
            .output()
            .map_err(|e| format!("git {args:?} did not run: {e}"))?;
        if !out.status.success() {
            return Err(format!("git {args:?} exited {:?}", out.status.code()));
        }
        Ok(out.stdout)
    };
    if !run(&["status", "--porcelain", "--untracked-files=all"])?.is_empty() {
        return Err("worktree changed".into());
    }
    let head = run(&["rev-parse", "HEAD"])?;
    if head.trim_ascii() != f.workspace.base_sha().as_str().as_bytes() {
        return Err("worktree HEAD moved".into());
    }
    Ok(())
}

fn l1_verdict(o: &Observed) -> Result<(), String> {
    let ok = o.cause == LiveProcessTerminalCause::Completed
        && o.exit == Some(0)
        && !o.forced_kill
        && o.failure.is_none()
        && o.provider_success
        && o.cancel.is_none();
    ok.then_some(())
        .ok_or_else(|| format!("L1 not a success: {o:?}"))
}
/// The provider's own termination is NOT asserted for an intentional cancel.
fn l2_verdict(first_record: bool, o: &Observed) -> Result<(), String> {
    let ok = first_record
        && o.cancel == Some(CancelReason::UserRequested)
        && o.cause == LiveProcessTerminalCause::Cancelled
        && o.failure == Some(FailureClass::UserCancelled);
    ok.then_some(()).ok_or_else(|| {
        format!(
            "L2 not an accepted cancel after a first record (first_record={first_record}): {o:?}"
        )
    })
}

struct Case<'a> {
    adapter: &'a DynAdapter,
    runtime: &'a str,
    evidence: &'a Path,
    git: &'a Path,
}
impl Case<'_> {
    /// Retains evidence and the summary, then returns the combined verdict.
    fn finish(
        &self,
        case: &str,
        f: &Fixture,
        result: &DynAttemptResult,
        verdict: impl FnOnce(&Observed) -> Result<(), String>,
    ) -> Result<(), String> {
        let stem = format!("{}-{case}", self.runtime);
        let (o, process) = observe(result)?;
        let stdout = retain(self.evidence, &stem, "stdout", process.stdout());
        let stderr = retain(self.evidence, &stem, "stderr", process.stderr());
        let s = process.stdout();
        let blank = match blank_records(s.head(), s.tail(), s.truncated()) {
            BlankRecords::Complete {
                records,
                blank,
                trailing_delimiter,
            } => format!(
                "stdout_records={records} stdout_blank_records={blank} stdout_trailing_delimiter={trailing_delimiter}"
            ),
            BlankRecords::Unavailable => "stdout_blank_records=unavailable(truncated)".into(),
        };
        let git = worktree_unchanged(self.git, f);
        let verdict = verdict(&o).and(git.clone());
        let summary = format!(
            "runtime={} case={case} cause={:?} exit={:?} forced_kill={} failure={:?} cancel={:?} {} {stdout} {stderr} {blank} worktree={} verdict={}\n",
            self.runtime,
            o.cause,
            o.exit,
            o.forced_kill,
            o.failure,
            o.cancel,
            o.label,
            git.map_or_else(|e| e, |()| "unchanged".into()),
            if verdict.is_ok() { "PASS" } else { "FAIL" },
        );
        private_write(
            self.evidence.join(format!("{stem}.summary")),
            summary.as_bytes(),
        );
        print!("{summary}");
        verdict
    }
    /// L1: one bounded read-only success attempt.
    fn l1(&self, policy: &RuntimeExecutionPolicy) -> Result<(), String> {
        let f = Fixture::new(Some("task"));
        let task = f.task_with_objective(SUCCESS_OBJECTIVE);
        let h = self
            .adapter
            .start(&task, &f.workspace, policy)
            .map_err(|c| format!("L1 start refused: {c:?}"))?;
        let result = self.adapter.collect_result(&h).unwrap();
        self.finish("L1-success", &f, &result, l1_verdict)
    }
    /// L2: cancel with `UserRequested` after the first event (30 s wakeup cap).
    /// `before_cancel` is only for synthetic controls.
    fn l2(
        &self,
        policy: &RuntimeExecutionPolicy,
        before_cancel: impl FnOnce(&mut crate::registry::DynEventStream<'_>),
    ) -> Result<(), String> {
        let f = Fixture::new(Some("task"));
        let task = f.task_with_objective(CANCEL_OBJECTIVE);
        let h = self
            .adapter
            .start(&task, &f.workspace, policy)
            .map_err(|c| format!("L2 start refused: {c:?}"))?;
        let first_record = {
            let mut stream = self.adapter.stream_events(&h).unwrap();
            let first = matches!(
                block_on(stream.next_event()),
                Some(DynAttemptEvent::Record(_))
            );
            before_cancel(&mut stream);
            first
        };
        self.adapter
            .cancel(&h, &CancelReason::UserRequested)
            .unwrap();
        let result = self.adapter.collect_result(&h).unwrap();
        self.finish("L2-cancel", &f, &result, |o| l2_verdict(first_record, o))
    }
}

fn adapter(codex: bool, executable: &Path) -> DynAdapter {
    let provider = if codex {
        ChildEnvProvider::Codex
    } else {
        ChildEnvProvider::Claude
    };
    let env = ChildEnvPolicy::new(provider, ChildEnvAuthMode::Subscription).unwrap();
    println!(
        "admitted environment NAMES: {:?}",
        env.names().collect::<Vec<_>>()
    );
    if codex {
        DynAdapter::new(CodexAdapter::new(executable).with_env_policy(env))
    } else {
        DynAdapter::new(ClaudeAdapter::new(executable).with_env_policy(env))
    }
}
fn policy(timeout: Duration) -> RuntimeExecutionPolicy {
    RuntimeExecutionPolicy::new(
        ProcessTimeoutPolicy::new(timeout, Duration::from_secs(5)).unwrap(),
        true,
    )
}

fn run(live: Live) {
    let adapter = adapter(live.runtime == "codex", &live.executable);
    let case = Case {
        adapter: &adapter,
        runtime: live.runtime,
        evidence: &live.evidence,
        git: Path::new(GIT),
    };
    let policy = policy(Duration::from_secs(180));
    // Both cases run (each at most once); either failure fails the test.
    let l1 = case.l1(&policy);
    let l2 = case.l2(&policy, |_| {});
    assert!(l1.is_ok() && l2.is_ok(), "L1={l1:?} L2={l2:?}");
}

#[test]
#[ignore = "live provider CLI; requires A1 context disposition"]
fn live_claude_ordinary_success_and_cancel() {
    if let Some(live) = opted_in("claude", "RECEIPTS_LIVE_CLAUDE", "RECEIPTS_LIVE_CLAUDE_EXE") {
        run(live);
    }
}

#[test]
#[ignore = "live provider CLI; requires A1 context disposition"]
fn live_codex_ordinary_success_and_cancel() {
    if let Some(live) = opted_in("codex", "RECEIPTS_LIVE_CODEX", "RECEIPTS_LIVE_CODEX_EXE") {
        run(live);
    }
}

// ---- synthetic harness controls (owned executables, never providers) ----

const CLAUDE_SUCCESS: &str =
    "{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"result\":\"ok\"}";
const CODEX_SUCCESS: &str = "{\"type\":\"thread.started\",\"thread_id\":\"t\"}\n{\"type\":\"turn.started\"}\n{\"type\":\"item.completed\",\"item\":{\"id\":\"i\",\"type\":\"agent_message\",\"text\":\"ok\"}}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1,\"cached_input_tokens\":0,\"output_tokens\":1}}";

/// Owned private scratch for one synthetic control: executable + evidence.
struct Synthetic {
    dir: PathBuf,
    evidence: PathBuf,
}
impl Synthetic {
    fn new(name: &str, body: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "receipts-ra-live-control-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("evidence")).unwrap();
        let exe = dir.join("cli");
        fs::write(&exe, format!("#!/bin/sh\n{body}")).unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        let evidence = dir.join("evidence");
        Self { dir, evidence }
    }
    fn exe(&self) -> PathBuf {
        self.dir.join("cli")
    }
    fn case<'a>(&'a self, adapter: &'a DynAdapter, git: &'a Path) -> Case<'a> {
        Case {
            adapter,
            runtime: "synthetic",
            evidence: &self.evidence,
            git,
        }
    }
}
impl Drop for Synthetic {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
fn drain(stream: &mut crate::registry::DynEventStream<'_>) {
    while block_on(stream.next_event()).is_some() {}
    // Stdout EOF precedes the process becoming terminal by a moment.
    std::thread::sleep(Duration::from_millis(500));
}

#[test]
fn synthetic_valid_success_and_cancel_pass_the_live_verdicts() {
    for codex in [false, true] {
        let line = if codex { CODEX_SUCCESS } else { CLAUDE_SUCCESS };
        let s = Synthetic::new(
            &format!("ok-{codex}"),
            &format!("/bin/cat >/dev/null\nprintf '%s\\n' '{line}'\n"),
        );
        let a = adapter(codex, &s.exe());
        let case = s.case(&a, Path::new(GIT));
        assert_eq!(case.l1(&policy(Duration::from_secs(30))), Ok(()));
        let c = Synthetic::new(
            &format!("cancel-{codex}"),
            "/bin/cat >/dev/null\nprintf '%s\\n' '{\"type\":\"system\"}'\nexec /bin/sleep 60\n",
        );
        let a = adapter(codex, &c.exe());
        let case = c.case(&a, Path::new(GIT));
        assert_eq!(case.l2(&policy(Duration::from_secs(30)), |_| {}), Ok(()));
    }
}

#[test]
fn synthetic_false_successes_never_pass_the_live_verdicts() {
    let p = policy(Duration::from_secs(30));
    for codex in [false, true] {
        let ok = if codex { CODEX_SUCCESS } else { CLAUDE_SUCCESS };
        // Silent exit 0: L1 has no success result; L2 has no first record.
        let s = Synthetic::new(&format!("silent-{codex}"), "/bin/cat >/dev/null\nexit 0\n");
        let a = adapter(codex, &s.exe());
        let case = s.case(&a, Path::new(GIT));
        assert!(case.l1(&p).is_err(), "silent exit passed L1");
        assert!(
            case.l2(&p, |_| {}).is_err(),
            "missing first event passed L2"
        );
        // Invalid / unknown results.
        for (n, body) in [
            "printf '%s\\n' 'not json'",
            "printf '%s\\n' '{\"type\":\"result\",\"subtype\":\"bogus\"}'",
            "printf '%s\\n' '{\"type\":\"turn.failed\",\"error\":{\"message\":\"x\"}}'",
        ]
        .iter()
        .enumerate()
        {
            let s = Synthetic::new(
                &format!("invalid-{codex}-{n}"),
                &format!("/bin/cat >/dev/null\n{body}\n"),
            );
            let a = adapter(codex, &s.exe());
            assert!(
                s.case(&a, Path::new(GIT)).l1(&p).is_err(),
                "{body} passed L1"
            );
        }
        // A success result with a nonzero exit is not L1 success.
        let s = Synthetic::new(
            &format!("nonzero-{codex}"),
            &format!("/bin/cat >/dev/null\nprintf '%s\\n' '{ok}'\nexit 3\n"),
        );
        let a = adapter(codex, &s.exe());
        assert!(
            s.case(&a, Path::new(GIT)).l1(&p).is_err(),
            "exit 3 passed L1"
        );
        // Cancellation after the worker already finished is not accepted.
        let s = Synthetic::new(
            &format!("late-cancel-{codex}"),
            "/bin/cat >/dev/null\nprintf '%s\\n' '{\"type\":\"system\"}'\nexit 0\n",
        );
        let a = adapter(codex, &s.exe());
        assert!(
            s.case(&a, Path::new(GIT)).l2(&p, drain).is_err(),
            "unaccepted cancellation passed L2"
        );
        // Git inspection failure, and a worktree the CLI changed.
        let s = Synthetic::new(
            &format!("git-{codex}"),
            &format!("/bin/cat >/dev/null\nprintf '%s\\n' '{ok}'\n"),
        );
        let a = adapter(codex, &s.exe());
        let failing_git = s.case(&a, Path::new("/usr/bin/false"));
        assert!(failing_git.l1(&p).unwrap_err().contains("exited"));
        let s = Synthetic::new(
            &format!("dirty-{codex}"),
            &format!("/bin/cat >/dev/null\n: > stray\nprintf '%s\\n' '{ok}'\n"),
        );
        let a = adapter(codex, &s.exe());
        assert!(
            s.case(&a, Path::new(GIT))
                .l1(&p)
                .unwrap_err()
                .contains("worktree changed")
        );
    }
}

#[test]
fn verdicts_require_the_promised_outcomes() {
    let good = || Observed {
        cause: LiveProcessTerminalCause::Completed,
        exit: Some(0),
        forced_kill: false,
        failure: None,
        provider_success: true,
        cancel: None,
        label: String::new(),
    };
    assert_eq!(l1_verdict(&good()), Ok(()));
    for broken in [
        Observed {
            exit: Some(1),
            ..good()
        },
        Observed {
            exit: None,
            ..good()
        },
        Observed {
            forced_kill: true,
            ..good()
        },
        Observed {
            failure: Some(FailureClass::Unknown),
            ..good()
        },
        Observed {
            provider_success: false,
            ..good()
        },
        Observed {
            cause: LiveProcessTerminalCause::TimedOut,
            ..good()
        },
        Observed {
            cancel: Some(CancelReason::UserRequested),
            ..good()
        },
    ] {
        assert!(l1_verdict(&broken).is_err(), "{broken:?}");
    }
    let cancelled = || Observed {
        cause: LiveProcessTerminalCause::Cancelled,
        exit: None,
        forced_kill: false,
        failure: Some(FailureClass::UserCancelled),
        // Provider termination of an intentional cancel is not asserted.
        provider_success: false,
        cancel: Some(CancelReason::UserRequested),
        label: String::new(),
    };
    assert_eq!(l2_verdict(true, &cancelled()), Ok(()));
    assert!(l2_verdict(false, &cancelled()).is_err());
    for broken in [
        Observed {
            cancel: None,
            ..cancelled()
        },
        Observed {
            cause: LiveProcessTerminalCause::Completed,
            ..cancelled()
        },
        Observed {
            failure: Some(FailureClass::Unknown),
            ..cancelled()
        },
        Observed {
            failure: None,
            ..cancelled()
        },
    ] {
        assert!(l2_verdict(true, &broken).is_err(), "{broken:?}");
    }
}

#[test]
fn blank_records_distinguish_delimiters_blank_lines_and_truncation() {
    use BlankRecords::{Complete, Unavailable};
    let c = |records, blank, trailing_delimiter| Complete {
        records,
        blank,
        trailing_delimiter,
    };
    assert_eq!(blank_records(b"", b"", false), c(0, 0, false));
    assert_eq!(blank_records(b"{}\n", b"", false), c(1, 0, true));
    assert_eq!(blank_records(b"{}", b"", false), c(1, 0, false));
    assert_eq!(blank_records(b"{}\n{}\n", b"", false), c(2, 0, true));
    assert_eq!(blank_records(b"\n", b"", false), c(1, 1, true));
    assert_eq!(blank_records(b"{}\n\n{}\n", b"", false), c(3, 1, true));
    assert_eq!(blank_records(b"{}\n\n\n", b"", false), c(3, 2, true));
    assert_eq!(blank_records(b"\n{}", b"", false), c(2, 1, false));
    // Untruncated head+tail is one exact stream.
    assert_eq!(blank_records(b"{}\n", b"\n{}\n", false), c(3, 1, true));
    // Truncated: head "...\n" + tail "\n..." would fake a blank record.
    assert_eq!(blank_records(b"{}\n", b"\n{}\n", true), Unavailable);
    assert_eq!(blank_records(b"", b"", true), Unavailable);
}

#[test]
fn truncated_capture_keeps_separate_fragments_and_no_blank_count() {
    // > STREAM_CAPTURE_LIMIT_BYTES of stdout from an owned synthetic CLI.
    let s = Synthetic::new(
        "truncated",
        "/bin/cat >/dev/null\n/usr/bin/head -c 1100000 /dev/zero | /usr/bin/tr '\\0' 'x'\nprintf '\\n'\n",
    );
    let a = adapter(false, &s.exe());
    let case = s.case(&a, Path::new(GIT));
    assert!(case.l1(&policy(Duration::from_secs(30))).is_err());
    let ev = &s.evidence;
    let head = fs::read(ev.join("synthetic-L1-success.stdout.head")).unwrap();
    let tail = fs::read(ev.join("synthetic-L1-success.stdout.tail")).unwrap();
    assert!(
        !ev.join("synthetic-L1-success.stdout").exists(),
        "fragments joined"
    );
    assert!(!head.is_empty() && tail.ends_with(b"x\n"));
    let summary = fs::read_to_string(ev.join("synthetic-L1-success.summary")).unwrap();
    assert!(summary.contains("stdout_truncated=true"));
    assert!(summary.contains("stdout_total_bytes=1100001"));
    assert!(summary.contains("stdout_blank_records=unavailable(truncated)"));
    assert!(!summary.contains("stdout_records="));
}
