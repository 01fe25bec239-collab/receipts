//! Hermetic consumer: Host refresh through the admitted runner binds the
//! exact fingerprint, mints a coupled cache entry only for a complete
//! observation of one unchanged executable, and never formats private
//! settings/probe payloads. Single test in its own process, so the
//! config-dir override cannot race.

use std::cell::Cell;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use receipts_cli::WorkspaceHostProbeRunner;
use receipts_host_integration::host_capability_observation::{
    HostCapabilityObservationError as Error, HostCapabilityObservationRequest,
};
use receipts_host_integration::{
    HostCapabilityReportRefreshRequest, HostExecutableIdentityV1, HostId, HostProbeCommandV1,
    HostProbeProcessPort, refresh_host_capability_report,
};
use receipts_workspace_execution::execution::{ChildEnvAuthMode, ChildEnvPolicy, ChildEnvProvider};

const MARKER: &str = "PRIVATE-MARKER-c41d";

fn write_script(exe: &Path, plugins: &str, comment: &str) {
    std::fs::write(
        exe,
        format!("#!/bin/sh\n# {comment}\nif [ \"$1\" = --version ]; then echo '9.9.9 (fake)'; else echo '{plugins}'; fi\n"),
    )
    .unwrap();
    std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o700)).unwrap();
}

fn runner(exe: &Path) -> WorkspaceHostProbeRunner {
    WorkspaceHostProbeRunner::new(
        exe,
        ChildEnvPolicy::new(ChildEnvProvider::Claude, ChildEnvAuthMode::Subscription).unwrap(),
    )
    .unwrap()
}

/// Delegates to the real runner and replaces the executable (identical
/// probe output) right after the first probe completes.
struct ReplaceAfterFirstProbe<'a> {
    inner: &'a WorkspaceHostProbeRunner,
    exe: &'a Path,
    runs: Cell<u32>,
}
impl HostProbeProcessPort for ReplaceAfterFirstProbe<'_> {
    fn executable(&self) -> Result<HostExecutableIdentityV1, Error> {
        self.inner.executable()
    }
    fn run(&self, command: &HostProbeCommandV1) -> Result<Vec<u8>, Error> {
        let out = self.inner.run(command);
        self.runs.set(self.runs.get() + 1);
        if self.runs.get() == 1 {
            write_script(self.exe, "[]", "replaced between probes");
        }
        out
    }
}

#[test]
fn refresh_binds_one_unchanged_executable_and_complete_sources() {
    let root = std::env::temp_dir().join(format!("receipts-cli-consumer-{}", std::process::id()));
    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::create_dir_all(root.join("project")).unwrap();
    let root = std::fs::canonicalize(&root).unwrap();
    // SAFETY: the only test in this binary; set before any thread reads it.
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", root.join("config")) };
    let exe = root.join("claude");
    let request = HostCapabilityReportRefreshRequest {
        observation: HostCapabilityObservationRequest {
            host: HostId::ClaudeCode,
            project_root: root.join("project"),
            hook_execution: None,
        },
        mode_override: None,
    };
    write_script(&exe, "[]", "v1");
    let original = runner(&exe);
    let first = refresh_host_capability_report(&request, &original);
    let second = refresh_host_capability_report(&request, &original);
    assert_eq!(first.fingerprint, second.fingerprint);
    // Whether an entry is minted depends on host-managed sources this test
    // does not control; when minted, unchanged reuse holds.
    if let (Some(a), Some(b)) = (&first.cache_entry, &second.cache_entry) {
        assert!(a.permits_reuse_by(b));
    }

    // Settings drift, with a private marker that must never be formatted.
    std::fs::create_dir_all(root.join("project/.claude")).unwrap();
    std::fs::write(
        root.join("project/.claude/settings.json"),
        format!("{{\"env\":{{\"TOKEN\":\"{MARKER}\"}}}}"),
    )
    .unwrap();
    let drifted = refresh_host_capability_report(&request, &original);
    assert_ne!(drifted.fingerprint, first.fingerprint);
    if let (Some(a), Some(b)) = (&first.cache_entry, &drifted.cache_entry) {
        assert!(!a.permits_reuse_by(b));
    }
    let rendered = format!("{drifted:?}");
    assert!(!rendered.contains(MARKER), "private marker formatted");

    // Replacement after construction: every probe of the old runner refuses.
    write_script(&exe, "[]", "v2 identical output");
    let stale = refresh_host_capability_report(&request, &original);
    let fingerprint = stale.fingerprint.as_ref().unwrap();
    assert!(!fingerprint.is_complete());
    assert_eq!(stale.observation.plugins, Err(Error::ExecutableChanged));
    assert!(stale.cache_entry.is_none());

    // A new validated runner admits the new bytes, with a new identity.
    let upgraded = runner(&exe);
    let rebuilt = refresh_host_capability_report(&request, &upgraded);
    assert_ne!(rebuilt.fingerprint, drifted.fingerprint);

    // Replacement between the version and plugin probes: no mixed
    // observation becomes evidence.
    let between = ReplaceAfterFirstProbe {
        inner: &upgraded,
        exe: &exe,
        runs: Cell::new(0),
    };
    let mixed = refresh_host_capability_report(&request, &between);
    assert_eq!(mixed.observation.plugins, Err(Error::ExecutableChanged));
    assert!(!mixed.fingerprint.as_ref().unwrap().is_complete());
    assert!(mixed.cache_entry.is_none());
    assert!(!format!("{mixed:?}").contains(MARKER));
    let _ = std::fs::remove_dir_all(&root);
}
