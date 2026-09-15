use crate::{
    HostCapabilityEvidenceLabel, HostCapabilityHookCoverageClass, HostCapabilityInactiveReason,
    HostCapabilityModeOverride, HostCapabilityModeOverrideSource, HostCapabilityProbeStatus,
    HostCapabilityReport, HostCapabilityReportNonTemporalCore,
    HostCapabilityReportNonTemporalCoreInputs, HostCapabilitySelectedMode,
    HostCapabilityStaleReason,
};
use receipts_orchestration::orchestration::OrchestrationDateTimeV1;

fn valid_inputs() -> HostCapabilityReportNonTemporalCoreInputs {
    HostCapabilityReportNonTemporalCoreInputs {
        host_id: " synthetic host 💾 ".to_owned(),
        host_version: Some(" 1.2.3 ".to_owned()),
        probe_status: HostCapabilityProbeStatus::Partial,
        validity_fingerprint: Some(" opaque fingerprint ".to_owned()),
        hook_definition_digest: Some("different digest".to_owned()),
        relevant_config_digest: Some(String::new()),
        stale_reason: HostCapabilityStaleReason::ValidityFingerprintChanged,
        plugin_supported: Some(false),
        plugin_installed: Some(true),
        manifest_path: Some("synthetic/manifest".to_owned()),
        supports_skills: None,
        supports_commands: Some(false),
        supports_subagents: Some(true),
        supports_mcp: None,
        hooks_supported: Some(false),
        hooks_configured: Some(true),
        hook_trust_required: Some(true),
        hooks_trusted: None,
        hooks_enabled: Some(false),
        hooks_allowed_by_admin_policy: Some(false),
        hook_events: Some(vec!["b".to_owned(), String::new(), "b".to_owned()]),
        blocking_hook_events: Some(Vec::new()),
        hook_coverage_class: HostCapabilityHookCoverageClass::Unknown,
        required_hook_coverage_satisfied: None,
        selected_mode: HostCapabilitySelectedMode::Hybrid,
        mode_override: Some(
            HostCapabilityModeOverride::new(
                HostCapabilityModeOverrideSource::Admin,
                " caller reason ".to_owned(),
            )
            .unwrap(),
        ),
        inactive_reason: Some(HostCapabilityInactiveReason::ModeOverride),
        plugin_data_path: Some("synthetic/data".to_owned()),
        sandbox_modes: None,
        evidence_label: Some(HostCapabilityEvidenceLabel::Assumption),
        source_claim_id: Some(String::new()),
    }
}

fn date_time(value: &str) -> OrchestrationDateTimeV1 {
    OrchestrationDateTimeV1::try_new(value).unwrap()
}

#[test]
fn composition_surface_has_only_owned_evidence_and_no_clock_or_io() {
    // Compile-time source inclusion performs no runtime filesystem access.
    // Keep this boundary guard alongside the behavioral tests; the constructor
    // and accessors are also directly inspectable, with no calls to other code.
    let source = include_str!("host_capability_report.rs");
    let production = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(production.contains(
        "pub struct HostCapabilityReport {\n    core: HostCapabilityReportNonTemporalCore,\n    probed_at: OrchestrationDateTimeV1,\n    last_verified_at: OrchestrationDateTimeV1,\n}"
    ));
    for forbidden in [
        "SystemTime",
        "Instant",
        "std::",
        "chrono",
        "time::",
        "env::",
        "fs::",
        "process::",
        "net::",
        "unsafe",
        "extern",
        "include!",
        "macro_rules!",
        "try_new",
        "parse",
        "now(",
        "probe(",
        "observe",
        "refresh",
        "validate",
        "&mut",
        "Default",
        "From<",
        "TryFrom<",
        "serde",
    ] {
        assert!(
            !production.contains(forbidden),
            "unexpected composition boundary token: {forbidden}"
        );
    }
}

#[test]
fn composition_moves_the_existing_core_and_shared_carriers_unchanged() {
    let core = HostCapabilityReportNonTemporalCore::new(valid_inputs()).unwrap();
    let expected = core.clone();
    let host_id_storage = core.host_id().as_ptr();
    let probed = date_time("2026-09-08t00:00:00z");
    let verified = date_time("2026-09-07T00:00:00-00:00");
    let probed_storage = probed.as_str().as_ptr();
    let verified_storage = verified.as_str().as_ptr();
    let constructor: fn(
        HostCapabilityReportNonTemporalCore,
        OrchestrationDateTimeV1,
        OrchestrationDateTimeV1,
    ) -> HostCapabilityReport = HostCapabilityReport::new;

    let report = constructor(core, probed, verified);
    let stored_core: &HostCapabilityReportNonTemporalCore = report.core();
    let stored_probed: &OrchestrationDateTimeV1 = report.probed_at();
    let stored_verified: &OrchestrationDateTimeV1 = report.last_verified_at();
    assert_eq!(stored_core, &expected);
    assert_eq!(stored_core.host_id().as_ptr(), host_id_storage);
    assert_eq!(stored_probed.as_str().as_ptr(), probed_storage);
    assert_eq!(stored_verified.as_str().as_ptr(), verified_storage);
    assert_eq!(report, report.clone());
}

#[test]
fn both_timestamps_preserve_every_lexical_form_without_normalization() {
    let core = HostCapabilityReportNonTemporalCore::new(valid_inputs()).unwrap();
    let long_fraction = format!("2026-09-08T00:00:00.{}Z", "1234567890".repeat(100));
    let forms = [
        "2026-09-08t00:00:00z",
        "2026-09-08T00:00:00+05:30",
        "2026-09-08T00:00:00-05:30",
        "2026-09-08T00:00:00-00:00",
        "2026-09-08T00:00:00+00:00",
        "2026-09-08T00:00:00.123456789123456789Z",
        "2026-09-08T00:00:00.1000Z",
        "0000-01-01t00:00:60z",
        "9999-12-31T23:59:59Z",
        long_fraction.as_str(),
    ];
    for probed in forms {
        for verified in forms {
            let report =
                HostCapabilityReport::new(core.clone(), date_time(probed), date_time(verified));
            assert_eq!(report.probed_at().as_str().as_bytes(), probed.as_bytes());
            assert_eq!(
                report.last_verified_at().as_str().as_bytes(),
                verified.as_bytes()
            );
            assert_eq!(report.core(), &core);
        }
    }
}

#[test]
fn earlier_equal_and_later_verification_are_all_accepted() {
    let core = HostCapabilityReportNonTemporalCore::new(valid_inputs()).unwrap();
    let earlier = date_time("2026-09-07T00:00:00Z");
    let later = date_time("2026-09-08T00:00:00Z");
    for (probed, verified) in [(&later, &earlier), (&earlier, &later), (&earlier, &earlier)] {
        let report = HostCapabilityReport::new(core.clone(), probed.clone(), verified.clone());
        assert_eq!(report.probed_at(), probed);
        assert_eq!(report.last_verified_at(), verified);
        assert_eq!(report.core(), &core);
    }
}

#[test]
fn same_timestamps_with_distinct_cores_remain_independent() {
    let first_core = HostCapabilityReportNonTemporalCore::new(valid_inputs()).unwrap();
    let mut second_inputs = valid_inputs();
    second_inputs.host_id = "another host".to_owned();
    second_inputs.probe_status = HostCapabilityProbeStatus::Failed;
    second_inputs.selected_mode = HostCapabilitySelectedMode::Supervised;
    second_inputs.hook_events = None;
    let second_core = HostCapabilityReportNonTemporalCore::new(second_inputs).unwrap();
    let probed = date_time("2026-09-08t00:00:00z");
    let verified = date_time("2026-09-07T00:00:00+05:30");
    let first = HostCapabilityReport::new(first_core.clone(), probed.clone(), verified.clone());
    let second = HostCapabilityReport::new(second_core.clone(), probed, verified);
    assert_eq!(first.core(), &first_core);
    assert_eq!(second.core(), &second_core);
    assert_ne!(first.core(), second.core());
    assert_eq!(first.probed_at(), second.probed_at());
    assert_eq!(first.last_verified_at(), second.last_verified_at());
    drop(first);
    assert_eq!(second.core(), &second_core);
}
