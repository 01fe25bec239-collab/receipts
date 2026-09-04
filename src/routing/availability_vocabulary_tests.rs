use crate::{AvailabilitySignalSource, AvailabilityStateKind};

fn assert_state_exhaustive(value: AvailabilityStateKind) {
    match value {
        AvailabilityStateKind::Available
        | AvailabilityStateKind::Degraded
        | AvailabilityStateKind::RateLimited
        | AvailabilityStateKind::SessionExhausted
        | AvailabilityStateKind::AuthRequired
        | AvailabilityStateKind::ProviderDown
        | AvailabilityStateKind::SafetyCheckPending
        | AvailabilityStateKind::PolicyBlocked
        | AvailabilityStateKind::Unknown => {}
    }
}

fn assert_signal_source_exhaustive(value: AvailabilitySignalSource) {
    match value {
        AvailabilitySignalSource::RateLimitHeader
        | AvailabilitySignalSource::ExitCode
        | AvailabilitySignalSource::StderrClassification
        | AvailabilitySignalSource::LocalUsageView
        | AvailabilitySignalSource::LatencyTrend
        | AvailabilitySignalSource::Probe
        | AvailabilitySignalSource::Unknown => {}
    }
}

#[test]
fn availability_state_vocabulary_is_exact_ordered_and_distinct() {
    use AvailabilityStateKind::{
        AuthRequired, Available, Degraded, PolicyBlocked, ProviderDown, RateLimited,
        SafetyCheckPending, SessionExhausted, Unknown,
    };

    const VALUES: [AvailabilityStateKind; 9] = [
        Available,
        Degraded,
        RateLimited,
        SessionExhausted,
        AuthRequired,
        ProviderDown,
        SafetyCheckPending,
        PolicyBlocked,
        Unknown,
    ];
    const STRINGS: [&str; 9] = [
        "AVAILABLE",
        "DEGRADED",
        "RATE_LIMITED",
        "SESSION_EXHAUSTED",
        "AUTH_REQUIRED",
        "PROVIDER_DOWN",
        "SAFETY_CHECK_PENDING",
        "POLICY_BLOCKED",
        "UNKNOWN",
    ];

    assert_eq!(AvailabilityStateKind::ALL.len(), 9);
    assert_eq!(AvailabilityStateKind::ALL, VALUES);
    assert_eq!(VALUES.map(AvailabilityStateKind::as_str), STRINGS);
    for (index, value) in VALUES.iter().enumerate() {
        assert_state_exhaustive(*value);
        for other in &VALUES[index + 1..] {
            assert_ne!(value, other);
            assert_ne!(value.as_str(), other.as_str());
        }
    }
    assert_eq!(Unknown.as_str(), "UNKNOWN");
    assert_ne!(Unknown, Available);
    assert_ne!(RateLimited, PolicyBlocked);
    assert_ne!(SafetyCheckPending, PolicyBlocked);
    assert_ne!(AuthRequired, ProviderDown);
}

#[test]
fn availability_signal_source_vocabulary_is_exact_ordered_and_distinct() {
    use AvailabilitySignalSource::{
        ExitCode, LatencyTrend, LocalUsageView, Probe, RateLimitHeader, StderrClassification,
        Unknown,
    };

    const VALUES: [AvailabilitySignalSource; 7] = [
        RateLimitHeader,
        ExitCode,
        StderrClassification,
        LocalUsageView,
        LatencyTrend,
        Probe,
        Unknown,
    ];
    const STRINGS: [&str; 7] = [
        "RATE_LIMIT_HEADER",
        "EXIT_CODE",
        "STDERR_CLASSIFICATION",
        "LOCAL_USAGE_VIEW",
        "LATENCY_TREND",
        "PROBE",
        "UNKNOWN",
    ];

    assert_eq!(AvailabilitySignalSource::ALL.len(), 7);
    assert_eq!(AvailabilitySignalSource::ALL, VALUES);
    assert_eq!(VALUES.map(AvailabilitySignalSource::as_str), STRINGS);
    for (index, value) in VALUES.iter().enumerate() {
        assert_signal_source_exhaustive(*value);
        for other in &VALUES[index + 1..] {
            assert_ne!(value, other);
            assert_ne!(value.as_str(), other.as_str());
        }
    }
    assert_eq!(Unknown.as_str(), "UNKNOWN");
    assert_ne!(Unknown, Probe);
}
