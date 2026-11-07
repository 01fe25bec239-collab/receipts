//! Prepared live-proof cases (docs/runtime-adapters/live-proof-plan.md).
//!
//! NOT RUN by any suite: each test is `#[ignore]`d AND returns unless its
//! explicit opt-in variable is exactly `1`. Running one executes a real
//! provider CLI with the caller's existing login, so it requires A1's separate
//! context disposition first. These use the ORDINARY, non-authoritative adapter
//! path (protected launch is unavailable on Darwin); a pass is compatibility
//! evidence for that CLI version only, never protected-launch or universal
//! provider evidence. Raw output goes only to the absolute private
//! `RECEIPTS_LIVE_EVIDENCE_DIR`, which must lie outside the repository. No
//! credential file, token or environment value is read, printed or copied.
use super::*;
use crate::protected_launch_tests::{Fixture, block_on};
use crate::registry::{DynAdapter, DynAttemptEvent};
use receipts_workspace_execution::execution::{
    ChildEnvAuthMode, ChildEnvPolicy, ChildEnvProvider, LiveProcessOutcome, ProcessTimeoutPolicy,
};
use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, path::PathBuf, time::Duration};

const SUCCESS_OBJECTIVE: &str = "Reply with the single word OK. Do not create, modify or delete any file and do not run any command.";
const CANCEL_OBJECTIVE: &str = "Count slowly from 1 to 200, one number per line. Do not create, modify or delete any file and do not run any command.";

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
/// Raw streams privately; one summary line without payload.
fn record(
    live: &Live,
    case: &str,
    outcome: &LiveProcessOutcome,
    failure: Option<FailureClass>,
    extra: &str,
) {
    let stdout = [outcome.stdout().head(), outcome.stdout().tail()].concat();
    let stderr = [outcome.stderr().head(), outcome.stderr().tail()].concat();
    private_write(
        live.evidence
            .join(format!("{}-{case}.stdout", live.runtime)),
        &stdout,
    );
    private_write(
        live.evidence
            .join(format!("{}-{case}.stderr", live.runtime)),
        &stderr,
    );
    let blank = stdout
        .split(|b| *b == b'\n')
        .filter(|l| l.is_empty())
        .count();
    let summary = format!(
        "runtime={} case={case} cause={:?} exit={:?} forced_kill={} stdout_truncated={} stderr_truncated={} stdout_bytes={} blank_segments={blank} failure={failure:?} {extra}\n",
        live.runtime,
        outcome.terminal_cause(),
        outcome.exit_code(),
        outcome.forced_kill_required(),
        outcome.stdout().truncated(),
        outcome.stderr().truncated(),
        stdout.len(),
    );
    private_write(
        live.evidence
            .join(format!("{}-{case}.summary", live.runtime)),
        summary.as_bytes(),
    );
    print!("{summary}");
}

/// Collected Workspace outcome and a payload-free provider/protocol label.
fn outcome(result: &crate::registry::DynAttemptResult) -> Option<(String, &LiveProcessOutcome)> {
    if let Some(Ok(r)) = result.downcast_ref::<Result<ClaudeAttemptResult, FailureClass>>() {
        let label = format!(
            "provider={:?} protected={}",
            r.provider_result(),
            r.protected()
        );
        return Some((label, r.outcome.process()));
    }
    let r = result
        .downcast_ref::<Result<CodexAttemptResult, FailureClass>>()?
        .as_ref()
        .ok()?;
    let label = format!(
        "protocol={:?} protected={}",
        r.protocol_termination(),
        r.protected()
    );
    Some((label, r.outcome.process()))
}

fn run(live: Live) {
    let codex = live.runtime == "codex";
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
    let adapter = if codex {
        DynAdapter::new(CodexAdapter::new(&live.executable).with_env_policy(env))
    } else {
        DynAdapter::new(ClaudeAdapter::new(&live.executable).with_env_policy(env))
    };
    let policy = RuntimeExecutionPolicy::new(
        ProcessTimeoutPolicy::new(Duration::from_secs(180), Duration::from_secs(5)).unwrap(),
        true,
    );
    let changed = |f: &Fixture| {
        let out = std::process::Command::new("/usr/bin/git")
            .current_dir(f.workspace.worktree_path())
            .args(["status", "--porcelain", "--untracked-files=all"])
            .output()
            .unwrap();
        !out.stdout.is_empty()
    };

    // L1: one bounded read-only success attempt.
    let f = Fixture::new(Some("task"));
    let h = adapter
        .start(
            &f.task_with_objective(SUCCESS_OBJECTIVE),
            &f.workspace,
            &policy,
        )
        .unwrap_or_else(|c| panic!("L1 start refused: {c:?}"));
    let result = adapter.collect_result(&h).unwrap();
    let (extra, process) = outcome(&result).expect("L1 collection failed");
    record(
        &live,
        "L1-success",
        process,
        result.failure(),
        &format!("{extra} worktree_changed={}", changed(&f)),
    );
    drop(h);

    // L2: one bounded cancellation after the first event (30 s cap, then Drop cancels).
    let f = Fixture::new(Some("task"));
    let h = adapter
        .start(
            &f.task_with_objective(CANCEL_OBJECTIVE),
            &f.workspace,
            &policy,
        )
        .unwrap_or_else(|c| panic!("L2 start refused: {c:?}"));
    {
        let mut stream = adapter.stream_events(&h).unwrap();
        let first = block_on(stream.next_event());
        println!(
            "L2 first event record={}",
            matches!(first, Some(DynAttemptEvent::Record(_)))
        );
    }
    adapter.cancel(&h, &CancelReason::UserRequested).unwrap();
    let result = adapter.collect_result(&h).unwrap();
    let (extra, process) = outcome(&result).expect("L2 collection failed");
    record(
        &live,
        "L2-cancel",
        process,
        result.failure(),
        &format!("{extra} worktree_changed={}", changed(&f)),
    );
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
