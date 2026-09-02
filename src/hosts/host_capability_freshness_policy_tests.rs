//! Contract tests for freshness disposition and EMBEDDED eligibility.

use std::collections::HashSet;

use super::*;

#[test]
fn disposition_vocabulary_is_exactly_authoritative() {
    let expected = [
        (
            HostCapabilityFreshnessDisposition::ReuseReport,
            "REUSE_REPORT",
        ),
        (
            HostCapabilityFreshnessDisposition::ReprobeThenSelect,
            "REPROBE_THEN_SELECT",
        ),
    ];

    assert_eq!(HostCapabilityFreshnessDisposition::ALL.len(), 2);
    assert_eq!(
        HostCapabilityFreshnessDisposition::ALL,
        expected.map(|(value, _)| value)
    );
    assert_eq!(
        expected
            .map(|(value, _)| value.as_str())
            .into_iter()
            .collect::<HashSet<_>>()
            .len(),
        2
    );

    for (value, canonical) in expected {
        assert_eq!(value.as_str(), canonical);
        assert_eq!(
            match value {
                HostCapabilityFreshnessDisposition::ReuseReport => "REUSE_REPORT",
                HostCapabilityFreshnessDisposition::ReprobeThenSelect => "REPROBE_THEN_SELECT",
            },
            canonical
        );
    }
}

#[test]
fn disposition_truth_table_is_exact_and_deterministic() {
    let expected = [
        (false, HostCapabilityFreshnessDisposition::ReprobeThenSelect),
        (true, HostCapabilityFreshnessDisposition::ReuseReport),
    ];

    for (current, disposition) in expected {
        assert_eq!(freshness_disposition(current), disposition);
        assert_eq!(
            freshness_disposition(current),
            freshness_disposition(current)
        );
    }
}

#[test]
fn all_48_eligibility_combinations_have_exactly_one_true_result() {
    let mut evaluated = 0;
    let mut eligible = Vec::new();

    for current in [false, true] {
        for probe_status in HostCapabilityProbeStatus::ALL {
            for stale_reason in HostCapabilityStaleReason::ALL {
                for native in [false, true] {
                    evaluated += 1;
                    let result = is_embedded_eligible(current, probe_status, stale_reason, native);
                    let expected = current
                        && probe_status == HostCapabilityProbeStatus::Complete
                        && stale_reason == HostCapabilityStaleReason::None
                        && native;
                    assert_eq!(result, expected);
                    if result {
                        eligible.push((current, probe_status, stale_reason, native));
                    }
                }
            }
        }
    }

    assert_eq!(evaluated, 48);
    assert_eq!(
        eligible,
        vec![(
            true,
            HostCapabilityProbeStatus::Complete,
            HostCapabilityStaleReason::None,
            true,
        )]
    );
}

#[test]
fn every_fail_closed_dimension_denies_eligibility() {
    let mut partial = 0;
    let mut failed = 0;
    let mut non_none_stale = 0;
    let mut unproven_current = 0;
    let mut missing_native = 0;

    for current in [false, true] {
        for probe_status in HostCapabilityProbeStatus::ALL {
            for stale_reason in HostCapabilityStaleReason::ALL {
                for native in [false, true] {
                    let result = is_embedded_eligible(current, probe_status, stale_reason, native);
                    if probe_status == HostCapabilityProbeStatus::Partial {
                        partial += 1;
                        assert!(!result);
                    }
                    if probe_status == HostCapabilityProbeStatus::Failed {
                        failed += 1;
                        assert!(!result);
                    }
                    if stale_reason != HostCapabilityStaleReason::None {
                        non_none_stale += 1;
                        assert!(!result);
                    }
                    if !current {
                        unproven_current += 1;
                        assert!(!result);
                    }
                    if !native {
                        missing_native += 1;
                        assert!(!result);
                    }
                }
            }
        }
    }

    assert_eq!(partial, 16);
    assert_eq!(failed, 16);
    assert_eq!(non_none_stale, 36);
    assert_eq!(unproven_current, 24);
    assert_eq!(missing_native, 24);
}

#[test]
fn healthy_looking_stale_or_native_incomplete_reports_fail_closed() {
    let result: bool = is_embedded_eligible(
        false,
        HostCapabilityProbeStatus::Complete,
        HostCapabilityStaleReason::None,
        true,
    );
    assert!(!result);
    assert!(!is_embedded_eligible(
        true,
        HostCapabilityProbeStatus::Complete,
        HostCapabilityStaleReason::None,
        false,
    ));
}
