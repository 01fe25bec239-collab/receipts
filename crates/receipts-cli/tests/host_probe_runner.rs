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

#[test]
fn timeout_terminates_the_probe_and_its_descendants() {
    let d = Dir::new();
    let exe = d.script("/bin/sleep 30 & echo $! > child.pid; echo '[]'; wait");
    let runner = WorkspaceHostProbeRunner::new(&exe, env()).unwrap();
    assert_eq!(
        runner.run(&command(&d.0, &[], Duration::from_millis(300), 1 << 20)),
        Err(Error::ProbeTimeout)
    );
    let pid = std::fs::read_to_string(d.0.join("child.pid")).unwrap();
    let alive = std::process::Command::new("/bin/kill")
        .args(["-0", pid.trim()])
        .status()
        .unwrap()
        .success();
    assert!(!alive, "descendant {pid} survived the probe deadline");
}
