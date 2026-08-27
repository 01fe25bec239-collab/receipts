//! Tests for bounded, separately retained stdout/stderr capture.
//!
//! Every behavioral proof runs a real child through the public capturing
//! runner. Controlled output generators are this very test binary acting as
//! ignored probe helpers, driven by small spec files in the attempt's
//! working directory and coordinated by marker files — no shells anywhere,
//! no shell scripts, no arbitrary-pid signaling.
//!
//! One deliberate wrinkle shapes every expectation: a probe is a libtest
//! invocation, so the harness itself writes a short fixed banner to the
//! child's stdout before the probe body runs. That banner is real child
//! output and is therefore genuinely part of the captured stdout stream.
//! Rather than hardcode it, the suite measures it once from a probe that
//! emits nothing, and every stdout expectation is `banner ++ pattern`.
//! stderr receives no banner, so stderr expectations are the raw pattern
//! alone. Both channels stay byte-exact either way.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::capture::{BoundedStreamRetention, CaptureFault, drain_to_eof};
use super::runner::{capture_fault_failed, frozen_retention, retention_allocation_failed};
use crate::execution::unix_signal::{caller_process_group, process_alive, recorded_group_is_empty};
use crate::execution::{
    CapturedProcessRun, CapturedStream, ExecutionError, ProcessRunRequest, ProcessTermination,
    ProcessTimeoutPolicy, STREAM_CAPTURE_LIMIT_BYTES, STREAM_HEAD_RETENTION_BYTES,
    STREAM_TAIL_RETENTION_BYTES, run_with_timeout, run_with_timeout_and_capture,
};

/// How long every controlled probe would keep emitting if the timeout
/// machinery failed to act.
const PROBE_SLEEP: Duration = Duration::from_secs(60);

/// Chunk size used by probes when emitting; small and fixed, never derived
/// from the total they are asked to produce.
const EMIT_CHUNK: usize = 32 * 1024;

/// Comfortably above ordinary OS pipe capacity (typically 16–64 KiB) on
/// both streams at once, so a non-concurrent drainer deadlocks instead of
/// passing.
const LARGE_STREAM_BYTES: usize = 6 * 1024 * 1024;

/// Generous ceiling for any bounded run in this suite, far below
/// [`PROBE_SLEEP`] so a leak could never pass unnoticed.
const RUN_UPPER_BOUND: Duration = Duration::from_secs(45);

// --- Deterministic byte pattern -----------------------------------------

/// The deterministic emission pattern shared by probes and expectations.
///
/// A 251-byte cycle: coprime with every power of two, so no chunk boundary
/// ever aligns with it, and it contains bytes above `0x7F` in sequences
/// that are not valid UTF-8 — capture must stay byte-exact regardless.
fn pattern_byte(index: usize) -> u8 {
    (index % 251) as u8
}

fn pattern_bytes(offset: usize, len: usize) -> Vec<u8> {
    (offset..offset + len).map(pattern_byte).collect()
}

/// The complete expected stream of `total` bytes: the harness banner (empty
/// for stderr) followed by the deterministic pattern.
fn expected_stream(banner: &[u8], total: usize) -> Vec<u8> {
    let mut expected = Vec::with_capacity(total);
    let banner_take = banner.len().min(total);
    expected.extend_from_slice(&banner[..banner_take]);
    expected.extend(pattern_bytes(0, total - banner_take));
    expected
}

// --- Capture assertions --------------------------------------------------

/// Asserts every frozen capture invariant against the complete expected
/// stream, including that retention contains process bytes only.
fn assert_captured(actual: &CapturedStream, expected: &[u8], what: &str) {
    let total = expected.len() as u64;
    assert_eq!(actual.total_bytes(), total, "{what}: total_bytes");
    assert_eq!(
        actual.truncated(),
        total > STREAM_CAPTURE_LIMIT_BYTES,
        "{what}: truncated must be exactly total_bytes > the limit"
    );
    assert!(
        actual.captured_bytes() <= STREAM_CAPTURE_LIMIT_BYTES,
        "{what}: captured_bytes must never exceed the retention limit"
    );
    assert_eq!(
        actual.captured_bytes(),
        (actual.head().len() + actual.tail().len()) as u64,
        "{what}: captured_bytes must equal the retained segments"
    );

    if total <= STREAM_CAPTURE_LIMIT_BYTES {
        assert_eq!(actual.captured_bytes(), total, "{what}: captured_bytes");
        let mut joined = actual.head().to_vec();
        joined.extend_from_slice(actual.tail());
        assert!(
            joined == expected,
            "{what}: head++tail must reproduce the complete stream byte-exactly with no \
             synthetic marker (retained {} bytes)",
            joined.len()
        );
    } else {
        assert_eq!(
            actual.captured_bytes(),
            STREAM_CAPTURE_LIMIT_BYTES,
            "{what}: captured_bytes"
        );
        assert_eq!(
            actual.head().len(),
            STREAM_HEAD_RETENTION_BYTES,
            "{what}: head length"
        );
        assert_eq!(
            actual.tail().len(),
            STREAM_TAIL_RETENTION_BYTES,
            "{what}: tail length"
        );
        assert!(
            actual.head() == &expected[..STREAM_HEAD_RETENTION_BYTES],
            "{what}: head must be the exact first {STREAM_HEAD_RETENTION_BYTES} raw bytes"
        );
        assert!(
            actual.tail() == &expected[expected.len() - STREAM_TAIL_RETENTION_BYTES..],
            "{what}: tail must be the exact last {STREAM_TAIL_RETENTION_BYTES} raw bytes"
        );
    }
}

/// Asserts the stream is exactly `banner ++ pattern` of the given total.
fn assert_pattern_stream(actual: &CapturedStream, banner: &[u8], total: usize, what: &str) {
    assert_captured(actual, &expected_stream(banner, total), what);
}

/// Asserts a stream of unpredictable length (a child cut short by timeout)
/// is nonetheless exactly a prefix of `banner ++ pattern` — proving capture
/// stayed byte-exact and inserted nothing.
fn assert_pattern_prefix_stream(actual: &CapturedStream, banner: &[u8], what: &str) {
    let total = usize::try_from(actual.total_bytes()).expect("test totals fit in usize");
    assert_captured(actual, &expected_stream(banner, total), what);
}

fn assert_empty_stream(actual: &CapturedStream, what: &str) {
    assert_eq!(actual.total_bytes(), 0, "{what}: total_bytes");
    assert_eq!(actual.captured_bytes(), 0, "{what}: captured_bytes");
    assert!(!actual.truncated(), "{what}: truncated");
    assert!(actual.head().is_empty(), "{what}: head");
    assert!(actual.tail().is_empty(), "{what}: tail");
}

// --- Controlled workspace ------------------------------------------------

/// A temporary directory removed on drop.
struct TempDir {
    root: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "receipts-capture-test-{tag}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temporary test directory creation");
        Self { root }
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn first_existing(candidates: &[&str]) -> PathBuf {
    candidates
        .iter()
        .map(Path::new)
        .find(|candidate| candidate.is_file())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| panic!("expected one of {candidates:?} to exist on this platform"))
}

fn policy(run_timeout: Duration, termination_grace: Duration) -> ProcessTimeoutPolicy {
    ProcessTimeoutPolicy::new(run_timeout, termination_grace).expect("valid timeout policy")
}

/// The generous policy used by every test whose child is expected to finish
/// on its own.
fn completing_policy() -> ProcessTimeoutPolicy {
    policy(RUN_UPPER_BOUND, Duration::from_secs(5))
}

fn request(
    executable: impl Into<PathBuf>,
    workspace: &Path,
    arguments: &[&str],
) -> ProcessRunRequest {
    ProcessRunRequest::new(executable, arguments.iter().copied(), workspace, workspace)
        .expect("structural request construction")
}

/// Writes one probe's emission spec into the attempt's working directory.
fn write_spec(workspace: &Path, stdout_bytes: usize, stderr_bytes: usize, exit_code: i32) {
    fs::write(workspace.join("stdout-bytes"), stdout_bytes.to_string()).expect("stdout spec");
    fs::write(workspace.join("stderr-bytes"), stderr_bytes.to_string()).expect("stderr spec");
    fs::write(workspace.join("exit-code"), exit_code.to_string()).expect("exit-code spec");
}

/// Runs one probe of this binary through the public capturing runner.
fn run_probe_captured(
    probe: &str,
    workspace: &Path,
    policy: &ProcessTimeoutPolicy,
) -> (CapturedProcessRun, Duration) {
    let started = Instant::now();
    let captured = run_with_timeout_and_capture(
        &request(
            std::env::current_exe().expect("current test executable path"),
            workspace,
            &[probe, "--ignored"],
        ),
        policy,
    )
    .expect("captured run should produce typed metadata, not an error");
    (captured, started.elapsed())
}

/// Runs the emitting probe to completion with the given spec.
fn emit_and_capture(
    tag: &str,
    stdout_bytes: usize,
    stderr_bytes: usize,
    exit_code: i32,
) -> (TempDir, CapturedProcessRun) {
    let workspace = TempDir::new(tag);
    write_spec(workspace.path(), stdout_bytes, stderr_bytes, exit_code);
    let (captured, elapsed) = run_probe_captured(
        "execution_capture_probe_emit_then_exit",
        workspace.path(),
        &completing_policy(),
    );
    assert!(
        elapsed < RUN_UPPER_BOUND,
        "{tag}: the captured run must complete well inside its bound; a pipe deadlock would \
         instead be cut short only by the run deadline (elapsed {elapsed:?})"
    );
    (workspace, captured)
}

/// The fixed stdout banner libtest writes before any probe body runs.
///
/// Measured once, from a probe asked to emit nothing at all, so the suite
/// never hardcodes a harness detail. It is deliberately measured through
/// the capture API itself: if capture were broken this would fail first.
fn stdout_banner() -> &'static [u8] {
    static BANNER: OnceLock<Vec<u8>> = OnceLock::new();
    BANNER.get_or_init(|| {
        let (_workspace, captured) = emit_and_capture("banner", 0, 0, 0);
        let stream = captured.stdout();
        assert!(
            !stream.truncated() && stream.total_bytes() > 0 && stream.total_bytes() < 4096,
            "the libtest stdout banner should be a small non-empty fixed prefix, observed {} bytes",
            stream.total_bytes()
        );
        assert_empty_stream(captured.stderr(), "banner probe stderr");
        stream.head().to_vec()
    })
}

// --- Marker coordination -------------------------------------------------

fn await_marker(dir: &Path, marker: &str) {
    let path = dir.join(marker);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "probe never wrote its {marker} marker; host is stalled beyond all test budgets"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn recorded_value(dir: &Path, marker: &str) -> u32 {
    await_marker(dir, marker);
    fs::read_to_string(dir.join(marker))
        .expect("marker readable")
        .trim()
        .parse()
        .expect("recorded marker value parses")
}

fn await_process_death(pid: u32, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while process_alive(pid) {
        assert!(
            Instant::now() < deadline,
            "{what} ({pid}) is still live or unreaped beyond every test budget"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn await_group_empty(pgid: u32, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if recorded_group_is_empty(pgid) == Some(true) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what}: owned process group -{pgid} still has members beyond every test budget"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

// --- Probe-side emission helpers -----------------------------------------

fn spec_value(name: &str, fallback: usize) -> usize {
    let cwd = std::env::current_dir().expect("probe working directory");
    fs::read_to_string(cwd.join(name))
        .ok()
        .and_then(|raw| raw.trim().parse().ok())
        .unwrap_or(fallback)
}

/// Writes `len` pattern bytes starting at `offset` to one raw stream.
///
/// `write_all` on the process-level handle goes straight to the inherited
/// file descriptor, so these are exactly the bytes the runner must capture.
fn emit(sink: &mut dyn Write, offset: usize, len: usize) {
    let mut written = 0;
    while written < len {
        let chunk = EMIT_CHUNK.min(len - written);
        let bytes = pattern_bytes(offset + written, chunk);
        if sink.write_all(&bytes).is_err() || sink.flush().is_err() {
            // The reader side is gone (the attempt was torn down); the
            // probe's job is over.
            return;
        }
        written += chunk;
    }
}

/// Ignores SIGTERM so only a forced group kill can end this process.
fn ignore_sigterm() {
    unsafe {
        // Test-side only: install SIG_IGN through the historic signal(2)
        // entry point. No shell, no dependency; SIG_IGN is fixed by POSIX.
        unsafe extern "C" {
            fn signal(signum: std::os::raw::c_int, handler: usize) -> usize;
        }
        const SIGTERM: std::os::raw::c_int = 15;
        const SIG_IGN: usize = 1;
        signal(SIGTERM, SIG_IGN);
    }
}

/// Spawns one of this binary's ignored probes as a direct no-shell child of
/// the current process, inheriting this process's stdout and stderr — which
/// is exactly how a descendant joins an attempt-owned group and an
/// attempt's captured pipes in production. Deliberately never reaped: the
/// descendant must outlive this helper.
#[allow(clippy::zombie_processes)]
fn spawn_and_await_emitting_descendant(probe: &str) {
    let executable = std::env::current_exe().expect("current test executable path");
    let descendant = std::process::Command::new(executable)
        .args([probe, "--ignored"])
        .spawn()
        .expect("descendant probe spawn");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !Path::new("descendant-ready").exists() {
        assert!(
            Instant::now() < deadline,
            "descendant probe {probe} never became ready"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = descendant.id();
}

// --- Controlled child probes ---------------------------------------------
//
// Never executed by the parent suite; the capturing runner re-invokes this
// same test binary filtered onto exactly one probe. Each reads its emission
// spec from the attempt's working directory.

/// Emits the requested stdout and stderr byte counts, then exits with the
/// requested code without letting libtest print its trailer.
///
/// The two streams are emitted **interleaved**, one chunk at a time. That
/// ordering is the point: a runner that drained stdout to completion before
/// touching stderr would block here as soon as the stderr pipe filled, and
/// a runner that waited on the child before reading anything would block as
/// soon as either pipe filled. Only genuinely concurrent draining finishes.
#[test]
#[ignore]
fn execution_capture_probe_emit_then_exit() {
    let stdout_bytes = spec_value("stdout-bytes", 0);
    let stderr_bytes = spec_value("stderr-bytes", 0);
    let exit_code = spec_value("exit-code", 0) as i32;

    let mut stdout = std::io::stdout();
    let mut stderr = std::io::stderr();
    let mut stdout_written = 0;
    let mut stderr_written = 0;
    while stdout_written < stdout_bytes || stderr_written < stderr_bytes {
        if stdout_written < stdout_bytes {
            let chunk = EMIT_CHUNK.min(stdout_bytes - stdout_written);
            emit(&mut stdout, stdout_written, chunk);
            stdout_written += chunk;
        }
        if stderr_written < stderr_bytes {
            let chunk = EMIT_CHUNK.min(stderr_bytes - stderr_written);
            emit(&mut stderr, stderr_written, chunk);
            stderr_written += chunk;
        }
    }
    std::process::exit(exit_code);
}

/// Emits an initial burst, publishes readiness, then keeps emitting the
/// continuing pattern until the attempt is torn down.
#[test]
#[ignore]
fn execution_capture_probe_emit_then_keep_emitting() {
    let stdout_bytes = spec_value("stdout-bytes", 0);
    let stderr_bytes = spec_value("stderr-bytes", 0);
    emit(&mut std::io::stdout(), 0, stdout_bytes);
    emit(&mut std::io::stderr(), 0, stderr_bytes);

    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("pid"), std::process::id().to_string()).expect("pid marker");
    fs::write(cwd.join("pgid"), caller_process_group().to_string()).expect("pgid marker");
    fs::write(cwd.join("ready"), b"ready").expect("ready marker");

    let mut stdout_offset = stdout_bytes;
    let mut stderr_offset = stderr_bytes;
    let deadline = Instant::now() + PROBE_SLEEP;
    while Instant::now() < deadline {
        if stdout_bytes > 0 {
            emit(&mut std::io::stdout(), stdout_offset, EMIT_CHUNK);
            stdout_offset += EMIT_CHUNK;
        }
        if stderr_bytes > 0 {
            emit(&mut std::io::stderr(), stderr_offset, EMIT_CHUNK);
            stderr_offset += EMIT_CHUNK;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Long-lived descendant emitting on both inherited streams, default
/// SIGTERM disposition.
#[test]
#[ignore]
fn execution_capture_probe_descendant_emitting_then_sleep_long() {
    descendant_emitting_body();
}

/// Long-lived descendant emitting on both inherited streams that ignores
/// SIGTERM, so only a forced process-group SIGKILL can end it.
#[test]
#[ignore]
fn execution_capture_probe_descendant_ignores_sigterm_emitting_then_sleep_long() {
    ignore_sigterm();
    descendant_emitting_body();
}

fn descendant_emitting_body() {
    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("descendant-pid"), std::process::id().to_string())
        .expect("descendant pid marker");
    fs::write(cwd.join("descendant-ready"), b"ready").expect("descendant ready marker");
    let deadline = Instant::now() + PROBE_SLEEP;
    let mut offset = 0;
    while Instant::now() < deadline {
        emit(&mut std::io::stdout(), offset, EMIT_CHUNK);
        emit(&mut std::io::stderr(), offset, EMIT_CHUNK);
        offset += EMIT_CHUNK;
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Timed parent that spawns one cooperative emitting descendant before
/// becoming ready; both share the attempt-owned group and both pipes.
#[test]
#[ignore]
fn execution_capture_probe_parent_of_graceful_emitting_descendant() {
    spawn_and_await_emitting_descendant(
        "execution_capture_probe_descendant_emitting_then_sleep_long",
    );
    emitting_parent_body();
}

/// Timed parent that spawns one SIGTERM-ignoring emitting descendant: the
/// parent dies on SIGTERM while the descendant survives into force kill.
#[test]
#[ignore]
fn execution_capture_probe_parent_of_sigterm_ignoring_emitting_descendant() {
    spawn_and_await_emitting_descendant(
        "execution_capture_probe_descendant_ignores_sigterm_emitting_then_sleep_long",
    );
    emitting_parent_body();
}

fn emitting_parent_body() {
    let cwd = std::env::current_dir().expect("probe working directory");
    fs::write(cwd.join("attempt-pid"), std::process::id().to_string()).expect("attempt pid marker");
    fs::write(cwd.join("attempt-pgid"), caller_process_group().to_string())
        .expect("attempt pgid marker");
    fs::write(cwd.join("ready"), b"ready").expect("ready marker");
    let deadline = Instant::now() + PROBE_SLEEP;
    let mut offset = 0;
    while Instant::now() < deadline {
        emit(&mut std::io::stdout(), offset, EMIT_CHUNK);
        emit(&mut std::io::stderr(), offset, EMIT_CHUNK);
        offset += EMIT_CHUNK;
        std::thread::sleep(Duration::from_millis(5));
    }
}

// --- C01 / C02: empty streams -------------------------------------------

#[test]
fn c01_c02_a_silent_child_captures_two_empty_streams() {
    // `true` writes nothing at all to either stream, so this is the only
    // fixture that proves genuinely empty stdout capture — a libtest probe
    // always emits the harness banner on stdout.
    let workspace = TempDir::new("silent");
    let captured = run_with_timeout_and_capture(
        &request(
            first_existing(&["/usr/bin/true", "/bin/true"]),
            workspace.path(),
            &[],
        ),
        &completing_policy(),
    )
    .expect("silent captured run");

    assert!(captured.outcome().success());
    assert_eq!(
        captured.outcome().termination(),
        ProcessTermination::Completed
    );
    assert_empty_stream(captured.stdout(), "C01 empty stdout");
    assert_empty_stream(captured.stderr(), "C02 empty stderr");
}

// --- C03 / C04: ordinary output ------------------------------------------

#[test]
fn c03_ordinary_stdout_is_captured_byte_exactly() {
    let banner = stdout_banner();
    let (_workspace, captured) = emit_and_capture("c03", 4_096, 0, 0);
    assert_pattern_stream(
        captured.stdout(),
        banner,
        banner.len() + 4_096,
        "C03 stdout",
    );
    assert_empty_stream(captured.stderr(), "C03 stderr");
    assert!(captured.outcome().success());
}

#[test]
fn c04_ordinary_stderr_is_captured_byte_exactly() {
    let banner = stdout_banner();
    let (_workspace, captured) = emit_and_capture("c04", 0, 4_096, 0);
    assert_pattern_stream(captured.stdout(), banner, banner.len(), "C04 stdout");
    assert_pattern_stream(captured.stderr(), &[], 4_096, "C04 stderr");
}

// --- C05 / C08: both streams, independently ------------------------------

#[test]
fn c05_c08_stdout_and_stderr_are_captured_simultaneously_and_independently() {
    let banner = stdout_banner();
    let (_workspace, captured) = emit_and_capture("c05", 3_001, 7_013, 0);
    assert_pattern_stream(
        captured.stdout(),
        banner,
        banner.len() + 3_001,
        "C05 stdout",
    );
    assert_pattern_stream(captured.stderr(), &[], 7_013, "C08 stderr");
    assert_ne!(
        captured.stdout().total_bytes(),
        captured.stderr().total_bytes(),
        "C08: the two streams must keep independent totals, never a merged one"
    );
    assert_ne!(
        captured.stdout().head(),
        captured.stderr().head(),
        "C08: neither stream may contain the other's bytes"
    );
}

// --- C06 / C07: raw binary, invalid UTF-8 --------------------------------

#[test]
fn c06_c07_binary_non_utf8_output_survives_both_streams_byte_exactly() {
    let banner = stdout_banner();
    let (_workspace, captured) = emit_and_capture("c06", 2_048, 2_048, 0);
    assert_pattern_stream(
        captured.stdout(),
        banner,
        banner.len() + 2_048,
        "C06 stdout",
    );
    assert_pattern_stream(captured.stderr(), &[], 2_048, "C07 stderr");
    assert!(
        std::str::from_utf8(captured.stdout().head()).is_err(),
        "C06: the fixture must contain bytes that are not valid UTF-8"
    );
    assert!(
        std::str::from_utf8(captured.stderr().head()).is_err(),
        "C07: the fixture must contain bytes that are not valid UTF-8"
    );
}

// --- C09 / C10: stdout boundary ------------------------------------------

#[test]
fn c09_stdout_exactly_at_the_limit_is_retained_whole_and_not_truncated() {
    let banner = stdout_banner();
    let limit = STREAM_CAPTURE_LIMIT_BYTES as usize;
    let (_workspace, captured) = emit_and_capture("c09", limit - banner.len(), 0, 0);
    let stdout = captured.stdout();
    assert_eq!(stdout.total_bytes(), STREAM_CAPTURE_LIMIT_BYTES);
    assert_eq!(stdout.captured_bytes(), STREAM_CAPTURE_LIMIT_BYTES);
    assert!(
        !stdout.truncated(),
        "C09: exactly the limit is NOT truncated"
    );
    assert_pattern_stream(stdout, banner, limit, "C09 stdout");
}

#[test]
fn c10_stdout_one_byte_over_the_limit_retains_exact_head_and_tail() {
    let banner = stdout_banner();
    let over = STREAM_CAPTURE_LIMIT_BYTES as usize + 1;
    let (_workspace, captured) = emit_and_capture("c10", over - banner.len(), 0, 0);
    let stdout = captured.stdout();
    assert_eq!(stdout.total_bytes(), STREAM_CAPTURE_LIMIT_BYTES + 1);
    assert_eq!(stdout.captured_bytes(), STREAM_CAPTURE_LIMIT_BYTES);
    assert!(stdout.truncated(), "C10: one byte over the limit truncates");
    assert_pattern_stream(stdout, banner, over, "C10 stdout");
}

// --- C11 / C12: stderr boundary ------------------------------------------

#[test]
fn c11_stderr_exactly_at_the_limit_is_retained_whole_and_not_truncated() {
    let limit = STREAM_CAPTURE_LIMIT_BYTES as usize;
    let (_workspace, captured) = emit_and_capture("c11", 0, limit, 0);
    let stderr = captured.stderr();
    assert_eq!(stderr.total_bytes(), STREAM_CAPTURE_LIMIT_BYTES);
    assert_eq!(stderr.captured_bytes(), STREAM_CAPTURE_LIMIT_BYTES);
    assert!(
        !stderr.truncated(),
        "C11: exactly the limit is NOT truncated"
    );
    assert_pattern_stream(stderr, &[], limit, "C11 stderr");
}

#[test]
fn c12_stderr_one_byte_over_the_limit_retains_exact_head_and_tail() {
    let over = STREAM_CAPTURE_LIMIT_BYTES as usize + 1;
    let (_workspace, captured) = emit_and_capture("c12", 0, over, 0);
    let stderr = captured.stderr();
    assert_eq!(stderr.total_bytes(), STREAM_CAPTURE_LIMIT_BYTES + 1);
    assert_eq!(stderr.captured_bytes(), STREAM_CAPTURE_LIMIT_BYTES);
    assert!(stderr.truncated(), "C12: one byte over the limit truncates");
    assert_pattern_stream(stderr, &[], over, "C12 stderr");
}

// --- Large simultaneous output (pipe-deadlock evidence) ------------------

#[test]
fn large_simultaneous_output_on_both_streams_never_deadlocks_and_stays_exact() {
    let banner = stdout_banner();
    // Both pipes are filled far past ordinary OS pipe capacity at the same
    // time. A runner that waited on the child first, or drained one stream
    // to completion before the other, would block here until the deadline
    // instead of completing.
    let (_workspace, captured) =
        emit_and_capture("large", LARGE_STREAM_BYTES, LARGE_STREAM_BYTES, 0);

    assert!(captured.outcome().success());
    assert_eq!(
        captured.outcome().termination(),
        ProcessTermination::Completed
    );
    assert_pattern_stream(
        captured.stdout(),
        banner,
        banner.len() + LARGE_STREAM_BYTES,
        "large stdout",
    );
    assert_pattern_stream(captured.stderr(), &[], LARGE_STREAM_BYTES, "large stderr");
    assert!(captured.stdout().truncated() && captured.stderr().truncated());
    assert_ne!(
        captured.stdout().total_bytes(),
        captured.stderr().total_bytes(),
        "the two large streams must stay distinct, never merged into one"
    );
}

// --- Nonzero exit --------------------------------------------------------

#[test]
fn a_nonzero_exit_is_an_ordinary_completed_capture_not_an_error() {
    let banner = stdout_banner();
    let (_workspace, captured) = emit_and_capture("nonzero", 512, 1_024, 7);
    let outcome = captured.outcome();
    assert!(!outcome.success(), "a non-zero exit is not success");
    assert!(!outcome.timed_out());
    assert_eq!(outcome.termination(), ProcessTermination::Completed);
    assert_eq!(outcome.exit_code(), Some(7));
    assert_pattern_stream(
        captured.stdout(),
        banner,
        banner.len() + 512,
        "nonzero stdout",
    );
    assert_pattern_stream(captured.stderr(), &[], 1_024, "nonzero stderr");
}

// --- C13 / C14 / C15: timeout while emitting -----------------------------

/// Runs the continuously emitting probe under a short deadline and returns
/// the captured result plus the workspace holding its markers.
fn timed_emitting_capture(
    tag: &str,
    stdout_bytes: usize,
    stderr_bytes: usize,
) -> (TempDir, CapturedProcessRun) {
    let workspace = TempDir::new(tag);
    write_spec(workspace.path(), stdout_bytes, stderr_bytes, 0);
    let (captured, elapsed) = run_probe_captured(
        "execution_capture_probe_emit_then_keep_emitting",
        workspace.path(),
        &policy(Duration::from_millis(400), Duration::from_secs(3)),
    );
    assert!(
        elapsed < RUN_UPPER_BOUND,
        "{tag}: a timed captured run must not hang on a full pipe (elapsed {elapsed:?})"
    );
    let outcome = captured.outcome();
    assert!(
        outcome.timed_out(),
        "{tag}: the run must classify as timed out"
    );
    assert!(
        !outcome.success(),
        "{tag}: a timed-out run is never success"
    );
    (workspace, captured)
}

#[test]
fn c13_timeout_while_emitting_stdout_captures_what_was_drained_to_eof() {
    let banner = stdout_banner();
    let (workspace, captured) = timed_emitting_capture("c13", 64 * 1024, 0);
    assert!(
        captured.stdout().total_bytes() >= (banner.len() + 64 * 1024) as u64,
        "C13: capture must include everything emitted before termination"
    );
    assert_pattern_prefix_stream(captured.stdout(), banner, "C13 stdout");
    assert_empty_stream(captured.stderr(), "C13 stderr");
    let pid = recorded_value(workspace.path(), "pid");
    let pgid = recorded_value(workspace.path(), "pgid");
    await_process_death(pid, "C13 attempt child");
    await_group_empty(pgid, "C13");
}

#[test]
fn c14_timeout_while_emitting_stderr_captures_what_was_drained_to_eof() {
    let banner = stdout_banner();
    let (workspace, captured) = timed_emitting_capture("c14", 0, 64 * 1024);
    assert!(
        captured.stderr().total_bytes() >= 64 * 1024,
        "C14: capture must include everything emitted before termination"
    );
    assert_pattern_prefix_stream(captured.stderr(), &[], "C14 stderr");
    // stdout still carries the harness banner and nothing else.
    assert_pattern_stream(captured.stdout(), banner, banner.len(), "C14 stdout");
    let pid = recorded_value(workspace.path(), "pid");
    await_process_death(pid, "C14 attempt child");
}

#[test]
fn c15_timeout_while_emitting_both_streams_keeps_them_separate_and_exact() {
    let banner = stdout_banner();
    let (workspace, captured) = timed_emitting_capture("c15", 96 * 1024, 128 * 1024);
    assert_pattern_prefix_stream(captured.stdout(), banner, "C15 stdout");
    assert_pattern_prefix_stream(captured.stderr(), &[], "C15 stderr");
    assert!(captured.stdout().total_bytes() >= (banner.len() + 96 * 1024) as u64);
    assert!(captured.stderr().total_bytes() >= 128 * 1024);
    let pid = recorded_value(workspace.path(), "pid");
    let pgid = recorded_value(workspace.path(), "pgid");
    await_process_death(pid, "C15 attempt child");
    await_group_empty(pgid, "C15");
    assert_ne!(
        caller_process_group(),
        pgid as std::os::raw::c_int,
        "the attempt must never share the caller's process group"
    );
}

// --- C16 / C17: descendants inheriting the captured pipes ----------------

fn run_descendant_capture(tag: &str, probe: &str) -> (TempDir, CapturedProcessRun) {
    let workspace = TempDir::new(tag);
    let caller_group_before = caller_process_group();
    let (captured, elapsed) = run_probe_captured(
        probe,
        workspace.path(),
        &policy(Duration::from_millis(500), Duration::from_secs(3)),
    );
    assert!(
        elapsed < RUN_UPPER_BOUND,
        "{tag}: descendants holding the captured pipes must not stall the run \
         (elapsed {elapsed:?})"
    );
    assert_eq!(
        caller_process_group(),
        caller_group_before,
        "{tag}: the caller's own process group must never be touched"
    );

    let attempt_pid = recorded_value(workspace.path(), "attempt-pid");
    let attempt_pgid = recorded_value(workspace.path(), "attempt-pgid");
    let descendant_pid = recorded_value(workspace.path(), "descendant-pid");
    assert_eq!(
        attempt_pid, attempt_pgid,
        "{tag}: the attempt child must lead its own dedicated process group"
    );
    assert_ne!(
        attempt_pgid as std::os::raw::c_int, caller_group_before,
        "{tag}: the attempt group must never be the caller's group"
    );
    await_process_death(attempt_pid, &format!("{tag} attempt child"));
    await_process_death(descendant_pid, &format!("{tag} inherited descendant"));
    await_group_empty(attempt_pgid, tag);

    // Both readers reached EOF — the call returned only after joining them —
    // and both streams carry real inherited descendant output.
    assert!(
        captured.stdout().total_bytes() > 0 && captured.stderr().total_bytes() > 0,
        "{tag}: both inherited streams must have been drained"
    );
    assert!(captured.stdout().captured_bytes() <= STREAM_CAPTURE_LIMIT_BYTES);
    assert!(captured.stderr().captured_bytes() <= STREAM_CAPTURE_LIMIT_BYTES);
    (workspace, captured)
}

#[test]
fn c16_cooperative_descendant_inheriting_the_captured_pipes_leaves_no_orphan() {
    let (_workspace, captured) = run_descendant_capture(
        "c16",
        "execution_capture_probe_parent_of_graceful_emitting_descendant",
    );
    assert_eq!(
        captured.outcome().termination(),
        ProcessTermination::TimedOutGracefullyTerminated,
        "C16: a cooperative group terminates within the grace, no force needed"
    );
}

#[test]
fn c17_sigterm_ignoring_descendant_inheriting_the_captured_pipes_is_force_killed() {
    let (_workspace, captured) = run_descendant_capture(
        "c17",
        "execution_capture_probe_parent_of_sigterm_ignoring_emitting_descendant",
    );
    assert_eq!(
        captured.outcome().termination(),
        ProcessTermination::TimedOutForceKilled,
        "C17: a SIGTERM-ignoring member forces the owned group kill"
    );
    assert!(captured.outcome().forced_kill_required());
}

// --- The uncaptured APIs keep their accepted behavior --------------------

#[test]
fn run_with_timeout_stays_uncaptured_for_a_loudly_emitting_child() {
    let workspace = TempDir::new("uncaptured");
    write_spec(workspace.path(), LARGE_STREAM_BYTES, LARGE_STREAM_BYTES, 0);
    let started = Instant::now();
    let outcome = run_with_timeout(
        &request(
            std::env::current_exe().expect("current test executable path"),
            workspace.path(),
            &["execution_capture_probe_emit_then_exit", "--ignored"],
        ),
        &completing_policy(),
    )
    .expect("the uncaptured timed runner must still succeed");
    // Null stdout/stderr means megabytes of output are discarded by the
    // kernel, never buffered, and no capture result exists to return.
    assert!(outcome.success());
    assert_eq!(outcome.termination(), ProcessTermination::Completed);
    assert!(started.elapsed() < RUN_UPPER_BOUND);
}

// --- Bounded-retention unit proofs ---------------------------------------

/// Drives the retention accumulator directly with the frozen limits.
fn retained(chunks: &[&[u8]]) -> CapturedStream {
    let mut retention = frozen_retention("stdout").expect("frozen retention");
    for chunk in chunks {
        retention.push(chunk).expect("push within budget");
    }
    retention.finish()
}

#[test]
fn retention_at_and_over_the_limit_matches_the_frozen_contract() {
    let limit = STREAM_CAPTURE_LIMIT_BYTES as usize;

    let exact = pattern_bytes(0, limit);
    assert_captured(&retained(&[&exact]), &exact, "unit exact limit");

    let over = pattern_bytes(0, limit + 1);
    assert_captured(&retained(&[&over]), &over, "unit one over");

    // Chunk boundaries must not change any retained byte.
    let chunked: Vec<&[u8]> = over.chunks(7_919).collect();
    assert_captured(&retained(&chunked), &over, "unit chunked one over");
}

#[test]
fn retention_never_inserts_synthetic_bytes_between_head_and_tail() {
    let head_only = pattern_bytes(0, STREAM_HEAD_RETENTION_BYTES - 1);
    let captured = retained(&[&head_only]);
    assert!(captured.tail().is_empty());
    assert_captured(&captured, &head_only, "head-only retention");

    let spanning = pattern_bytes(0, STREAM_HEAD_RETENTION_BYTES + 17);
    let captured = retained(&[&spanning]);
    assert_eq!(captured.head().len(), STREAM_HEAD_RETENTION_BYTES);
    assert_eq!(captured.tail().len(), 17);
    assert_captured(&captured, &spanning, "spanning retention");
}

#[test]
fn retention_allocation_failure_fails_closed_with_a_typed_error() {
    let error = BoundedStreamRetention::new(usize::MAX, STREAM_TAIL_RETENTION_BYTES)
        .expect_err("reserving usize::MAX bytes must fail rather than succeed");
    let mapped = retention_allocation_failed("stdout", error);
    assert!(
        matches!(
            mapped,
            ExecutionError::CaptureRetentionAllocationFailed { .. }
        ),
        "unexpected mapping: {mapped:?}"
    );
    assert!(mapped.to_string().contains("stdout"));
    // Nothing silently degrades into an unbounded buffer, and no valid
    // metadata is fabricated: no CapturedStream exists on this path at all.
}

#[test]
fn total_byte_counting_overflow_fails_closed_instead_of_wrapping() {
    let mut retention = frozen_retention("stderr").expect("frozen retention");
    retention.set_total_bytes_for_test(u64::MAX);
    let fault = retention
        .push(&[0u8])
        .expect_err("counting one more byte past u64::MAX must fail");
    assert_eq!(retention.total_bytes(), u64::MAX, "the count must not wrap");
    let mapped = capture_fault_failed("stderr", fault);
    assert!(
        matches!(
            mapped,
            ExecutionError::CaptureTotalByteOverflow {
                stream: "stderr",
                ..
            }
        ),
        "unexpected mapping: {mapped:?}"
    );
}

// --- Reader fail-closed proofs -------------------------------------------

/// A reader that yields some bytes, one `Interrupted`, then a hard failure.
struct FlakyThenFailingReader {
    served: bool,
    interrupted: bool,
}

impl Read for FlakyThenFailingReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if !self.served {
            self.served = true;
            buffer[..4].copy_from_slice(b"abcd");
            return Ok(4);
        }
        if !self.interrupted {
            self.interrupted = true;
            return Err(std::io::Error::from(ErrorKind::Interrupted));
        }
        Err(std::io::Error::new(ErrorKind::BrokenPipe, "pipe collapsed"))
    }
}

#[test]
fn a_read_failure_before_eof_fails_closed_with_a_stream_identified_error() {
    let mut retention = frozen_retention("stdout").expect("frozen retention");
    let fault = drain_to_eof(
        FlakyThenFailingReader {
            served: false,
            interrupted: false,
        },
        &mut retention,
    )
    .expect_err("a hard read failure must not be swallowed");
    assert!(
        matches!(fault, CaptureFault::Read(_)),
        "unexpected fault: {fault:?}"
    );
    let mapped = capture_fault_failed("stdout", fault);
    match mapped {
        ExecutionError::CaptureReadFailed { stream, ref detail } => {
            assert_eq!(stream, "stdout");
            assert!(detail.contains("pipe collapsed"));
        }
        other => panic!("unexpected mapping: {other:?}"),
    }
    // Interrupted was retried rather than treated as EOF, so the bytes read
    // before the failure were still counted — but no CapturedStream is
    // produced for a stream that never reached EOF.
    assert_eq!(retention.total_bytes(), 4);
}

#[test]
fn draining_reaches_eof_and_keeps_reading_past_the_retention_limit() {
    // A source larger than the retention limit must be consumed entirely:
    // the limit bounds retained memory, not bytes taken from the pipe.
    let source = pattern_bytes(0, STREAM_CAPTURE_LIMIT_BYTES as usize + 12_345);
    let mut retention = frozen_retention("stdout").expect("frozen retention");
    drain_to_eof(source.as_slice(), &mut retention).expect("draining to EOF");
    let captured = retention.finish();
    assert_eq!(captured.total_bytes(), source.len() as u64);
    assert_captured(&captured, &source, "drain past the limit");
}
