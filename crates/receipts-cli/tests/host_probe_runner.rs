//! Synthetic controls for the admitted Host probe runner (no installed host,
//! no provider). Each script is a fake `claude` written by the test.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use receipts_cli::{HostProbeRunnerError, WorkspaceHostProbeRunner};
use receipts_host_integration::host_capability_observation::HostCapabilityObservationError as Error;
use receipts_host_integration::{HostProbeCommandV1, HostProbeProcessPort};
use receipts_workspace_execution::execution::{ChildEnvAuthMode, ChildEnvPolicy, ChildEnvProvider};

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "receipts-cli-probe-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(std::fs::canonicalize(p).unwrap())
    }
    fn script(&self, body: &str) -> PathBuf {
        let p = self.0.join("claude");
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn env() -> ChildEnvPolicy {
    ChildEnvPolicy::new(ChildEnvProvider::Claude, ChildEnvAuthMode::Subscription).unwrap()
}

fn command(dir: &Path, args: &[&str], timeout: Duration, max: usize) -> HostProbeCommandV1 {
    HostProbeCommandV1 {
        args: args.iter().map(|a| a.to_string()).collect(),
        current_dir: dir.to_path_buf(),
        timeout,
        stdout_max_bytes: max,
        stderr_max_bytes: max,
    }
}

#[test]
fn only_an_absolute_regular_executable_is_admitted() {
    let d = Dir::new();
    assert_eq!(
        WorkspaceHostProbeRunner::new(Path::new("claude"), env()).unwrap_err(),
        HostProbeRunnerError::NotAbsolute
    );
    assert_eq!(
        WorkspaceHostProbeRunner::new(&d.0.join("missing"), env()).unwrap_err(),
        HostProbeRunnerError::Unresolvable
    );
    assert_eq!(
        WorkspaceHostProbeRunner::new(&d.0, env()).unwrap_err(),
        HostProbeRunnerError::NotRegularFile
    );
    let plain = d.0.join("plain");
    std::fs::write(&plain, "x").unwrap();
    assert_eq!(
        WorkspaceHostProbeRunner::new(&plain, env()).unwrap_err(),
        HostProbeRunnerError::NotExecutable
    );
    // A symlink alias is resolved to, and identified by, its canonical target.
    let exe = d.script("echo ok");
    let link = d.0.join("alias");
    std::os::unix::fs::symlink(&exe, &link).unwrap();
    let runner = WorkspaceHostProbeRunner::new(&link, env()).unwrap();
    let id = runner.executable().unwrap();
    assert_eq!(id.canonical_path, exe);
    assert_eq!(id.content_sha256.len(), 64);
}

#[test]
fn successful_probe_returns_exact_stdout_with_only_the_policy_environment() {
    let d = Dir::new();
    let exe = d.script("printf '%s ' \"$@\"; env > env.out");
    let runner = WorkspaceHostProbeRunner::new(&exe, env()).unwrap();
    let out = runner
        .run(&command(
            &d.0,
            &["plugin", "list", "--json"],
            Duration::from_secs(10),
            1 << 20,
        ))
        .unwrap();
    assert_eq!(out, b"plugin list --json ");
    let child_env = std::fs::read_to_string(d.0.join("env.out")).unwrap();
    // The test process has CARGO_* variables; none may reach the probe.
    assert!(
        !child_env.contains("CARGO_"),
        "ambient environment leaked: {child_env}"
    );
}

#[test]
fn failures_timeouts_and_oversize_output_never_become_evidence() {
    let d = Dir::new();
    let fail = WorkspaceHostProbeRunner::new(&d.script("echo '[]'; exit 3"), env()).unwrap();
    assert_eq!(
        fail.run(&command(&d.0, &[], Duration::from_secs(10), 1 << 20)),
        Err(Error::ProbeFailed)
    );
    let big = WorkspaceHostProbeRunner::new(&d.script("printf '%0200d' 0"), env()).unwrap();
    assert_eq!(
        big.run(&command(&d.0, &[], Duration::from_secs(10), 100)),
        Err(Error::ProbeStdoutTooLarge)
    );
    let noisy = WorkspaceHostProbeRunner::new(&d.script("printf '%0200d' 0 >&2"), env()).unwrap();
    assert_eq!(
        noisy.run(&command(&d.0, &[], Duration::from_secs(10), 100)),
        Err(Error::ProbeStderrTooLarge)
    );
}

/// Property: when the deadline passes, the probe is reported as a timeout
/// (never as evidence) and its whole process group, including a backgrounded
/// descendant, is terminated. The deadline must exceed the time the script
/// needs to start that descendant: the first exec of a freshly written script
/// on macOS measured ~50 ms median and up to ~164 ms here, and a 300 ms
/// deadline occasionally expired before `child.pid` existed, so the
/// descendant check never ran. 2 s keeps the precondition reliable; the
/// script itself would run for 30 s, so the bounds below still prove the
/// deadline (not natural exit) ended it.
#[test]
fn timeout_terminates_the_probe_and_its_descendants() {
    let d = Dir::new();
    let exe = d.script("/bin/sleep 30 & echo $! > child.pid; echo '[]'; wait");
    let runner = WorkspaceHostProbeRunner::new(&exe, env()).unwrap();
    let deadline = Duration::from_millis(2000);
    let started = std::time::Instant::now();
    assert_eq!(
        runner.run(&command(&d.0, &[], deadline, 1 << 20)),
        Err(Error::ProbeTimeout)
    );
    let elapsed = started.elapsed();
    assert!(
        elapsed >= deadline,
        "returned before the deadline: {elapsed:?}"
    );
    assert!(
        elapsed < deadline + receipts_cli::HOST_PROBE_TERMINATION_GRACE + Duration::from_secs(5),
        "the deadline did not bound the 30 s probe: {elapsed:?}"
    );
    let pid = std::fs::read_to_string(d.0.join("child.pid"))
        .expect("precondition: the descendant started before the deadline");
    let alive = std::process::Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .status()
        .unwrap()
        .success();
    assert!(!alive, "descendant {pid} survived the probe deadline");
}

#[test]
fn unchanged_executable_keeps_its_admitted_identity_across_probes() {
    let d = Dir::new();
    let runner = WorkspaceHostProbeRunner::new(&d.script("printf ok"), env()).unwrap();
    let id = runner.executable().unwrap();
    for _ in 0..3 {
        assert_eq!(
            runner.run(&command(&d.0, &[], Duration::from_secs(10), 1 << 20)),
            Ok(b"ok".to_vec())
        );
    }
    assert_eq!(runner.executable().unwrap(), id);
    assert!(id.probe_policy.contains("Claude"));
}

#[test]
fn replacement_after_construction_is_refused_even_with_identical_output() {
    let d = Dir::new();
    let exe = d.script("printf original");
    let runner = WorkspaceHostProbeRunner::new(&exe, env()).unwrap();
    // Identical output, different bytes: output equality cannot hide drift.
    d.script("printf original # upgraded");
    let probe = command(&d.0, &[], Duration::from_secs(10), 1 << 20);
    assert_eq!(runner.run(&probe), Err(Error::ExecutableChanged));
    assert_eq!(runner.executable(), Err(Error::ExecutableChanged));
    // Restoring the admitted bytes restores the admitted identity.
    d.script("printf original");
    assert_eq!(runner.run(&probe), Ok(b"original".to_vec()));
    // A newly validated runner explicitly admits the new bytes.
    d.script("printf original # upgraded");
    let upgraded = WorkspaceHostProbeRunner::new(&exe, env()).unwrap();
    assert_eq!(upgraded.run(&probe), Ok(b"original".to_vec()));
    assert_eq!(runner.executable(), Err(Error::ExecutableChanged));
}

#[test]
fn replacement_during_a_probe_discards_its_output() {
    let d = Dir::new();
    // The probe rewrites its own file (same reported output) before exiting.
    let exe = d.0.join("claude");
    let body = format!(
        "#!/bin/sh\nprintf '#!/bin/sh\\nprintf same\\n' > {}\nprintf same\n",
        exe.display()
    );
    std::fs::write(&exe, body).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
    let runner = WorkspaceHostProbeRunner::new(&exe, env()).unwrap();
    assert_eq!(
        runner.run(&command(&d.0, &[], Duration::from_secs(10), 1 << 20)),
        Err(Error::ExecutableChanged)
    );
}

#[test]
fn replacing_the_path_with_a_symlink_is_refused() {
    let d = Dir::new();
    let exe = d.script("printf ok");
    let runner = WorkspaceHostProbeRunner::new(&exe, env()).unwrap();
    let other = d.0.join("other");
    std::fs::copy(&exe, &other).unwrap();
    std::fs::remove_file(&exe).unwrap();
    std::os::unix::fs::symlink(&other, &exe).unwrap();
    assert_eq!(runner.executable(), Err(Error::ExecutableChanged));
    assert_eq!(
        runner.run(&command(&d.0, &[], Duration::from_secs(10), 1 << 20)),
        Err(Error::ExecutableChanged)
    );
}
