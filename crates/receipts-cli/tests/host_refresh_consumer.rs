//! Hermetic consumer: Host refresh through the admitted runner binds the
//! exact fingerprint, which changes when probe output or settings change.
//! Single test in its own process, so the config-dir override cannot race.

use std::os::unix::fs::PermissionsExt;

use receipts_cli::WorkspaceHostProbeRunner;
use receipts_host_integration::host_capability_observation::HostCapabilityObservationRequest;
use receipts_host_integration::{
    HostCapabilityReportRefreshRequest, HostId, refresh_host_capability_report,
};
use receipts_workspace_execution::execution::{ChildEnvAuthMode, ChildEnvPolicy, ChildEnvProvider};

#[test]
fn refresh_fingerprint_is_stable_and_detects_drift() {
    let root = std::env::temp_dir().join(format!("receipts-cli-consumer-{}", std::process::id()));
    std::fs::create_dir_all(root.join("config")).unwrap();
    std::fs::create_dir_all(root.join("project")).unwrap();
    let root = std::fs::canonicalize(&root).unwrap();
    // SAFETY: the only test in this binary; set before any thread reads it.
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", root.join("config")) };
    let exe = root.join("claude");
    let script = |plugins: &str| {
        std::fs::write(&exe, format!("#!/bin/sh\nif [ \"$1\" = --version ]; then echo '9.9.9 (fake)'; else echo '{plugins}'; fi\n")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        WorkspaceHostProbeRunner::new(
            &exe,
            ChildEnvPolicy::new(ChildEnvProvider::Claude, ChildEnvAuthMode::Subscription).unwrap(),
        )
        .unwrap()
    };
    let request = HostCapabilityReportRefreshRequest {
        observation: HostCapabilityObservationRequest {
            host: HostId::ClaudeCode,
            project_root: root.join("project"),
        },
        mode_override: None,
    };
    let runner = script("[]");
    let first = refresh_host_capability_report(&request, &runner)
        .fingerprint
        .unwrap();
    let second = refresh_host_capability_report(&request, &runner)
        .fingerprint
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(second.permits_reuse_of(&first), second.is_complete());

    // Settings drift.
    std::fs::create_dir_all(root.join("project/.claude")).unwrap();
    std::fs::write(root.join("project/.claude/settings.json"), "{}").unwrap();
    let drifted = refresh_host_capability_report(&request, &runner)
        .fingerprint
        .unwrap();
    assert_ne!(drifted, first);
    assert!(!drifted.permits_reuse_of(&first));

    // Executable content drift (different bytes, same path).
    let changed = script("[ ]");
    let rebuilt = refresh_host_capability_report(&request, &changed)
        .fingerprint
        .unwrap();
    assert_ne!(rebuilt, drifted);
    let _ = std::fs::remove_dir_all(&root);
}
