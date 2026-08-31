//! Contract tests for the closed `HostCapabilityReport` vocabularies.

use std::collections::HashSet;
use std::{fmt::Debug, hash::Hash};

use super::*;

fn assert_closed<T: Copy + Debug + Eq + Hash, const N: usize>(
    all: [T; N],
    expected: [(T, &'static str); N],
    as_str: impl Fn(T) -> &'static str,
    contract: impl Fn(T) -> &'static str,
) {
    assert_eq!(all.len(), N);
    assert_eq!(all.iter().copied().collect::<HashSet<_>>().len(), N);
    assert_eq!(
        all.iter()
            .copied()
            .map(&as_str)
            .collect::<HashSet<_>>()
            .len(),
        N
    );

    for (index, (value, canonical)) in expected.into_iter().enumerate() {
        assert_eq!(all[index], value);
        assert_eq!(as_str(value), canonical);
        assert_eq!(contract(value), canonical);
    }
}

#[test]
fn probe_status_is_exactly_frozen() {
    let expected = [
        (HostCapabilityProbeStatus::Complete, "COMPLETE"),
        (HostCapabilityProbeStatus::Partial, "PARTIAL"),
        (HostCapabilityProbeStatus::Failed, "FAILED"),
    ];
    assert_closed(
        HostCapabilityProbeStatus::ALL,
        expected,
        HostCapabilityProbeStatus::as_str,
        |value| match value {
            HostCapabilityProbeStatus::Complete => "COMPLETE",
            HostCapabilityProbeStatus::Partial => "PARTIAL",
            HostCapabilityProbeStatus::Failed => "FAILED",
        },
    );
}

#[test]
fn stale_reason_is_exactly_frozen() {
    let expected = [
        (HostCapabilityStaleReason::None, "NONE"),
        (
            HostCapabilityStaleReason::ValidityFingerprintChanged,
            "VALIDITY_FINGERPRINT_CHANGED",
        ),
        (
            HostCapabilityStaleReason::ValidityFingerprintUnproven,
            "VALIDITY_FINGERPRINT_UNPROVEN",
        ),
        (HostCapabilityStaleReason::ProbeFailed, "PROBE_FAILED"),
    ];
    assert_closed(
        HostCapabilityStaleReason::ALL,
        expected,
        HostCapabilityStaleReason::as_str,
        |value| match value {
            HostCapabilityStaleReason::None => "NONE",
            HostCapabilityStaleReason::ValidityFingerprintChanged => "VALIDITY_FINGERPRINT_CHANGED",
            HostCapabilityStaleReason::ValidityFingerprintUnproven => {
                "VALIDITY_FINGERPRINT_UNPROVEN"
            }
            HostCapabilityStaleReason::ProbeFailed => "PROBE_FAILED",
        },
    );
}

#[test]
fn hook_coverage_class_is_exactly_frozen() {
    let expected = [
        (HostCapabilityHookCoverageClass::Full, "FULL"),
        (HostCapabilityHookCoverageClass::Partial, "PARTIAL"),
        (HostCapabilityHookCoverageClass::None, "NONE"),
        (HostCapabilityHookCoverageClass::Unknown, "UNKNOWN"),
    ];
    assert_closed(
        HostCapabilityHookCoverageClass::ALL,
        expected,
        HostCapabilityHookCoverageClass::as_str,
        |value| match value {
            HostCapabilityHookCoverageClass::Full => "FULL",
            HostCapabilityHookCoverageClass::Partial => "PARTIAL",
            HostCapabilityHookCoverageClass::None => "NONE",
            HostCapabilityHookCoverageClass::Unknown => "UNKNOWN",
        },
    );
}

#[test]
fn selected_mode_is_exactly_frozen() {
    let expected = [
        (HostCapabilitySelectedMode::Embedded, "EMBEDDED"),
        (HostCapabilitySelectedMode::Hybrid, "HYBRID"),
        (HostCapabilitySelectedMode::Supervised, "SUPERVISED"),
    ];
    assert_closed(
        HostCapabilitySelectedMode::ALL,
        expected,
        HostCapabilitySelectedMode::as_str,
        |value| match value {
            HostCapabilitySelectedMode::Embedded => "EMBEDDED",
            HostCapabilitySelectedMode::Hybrid => "HYBRID",
            HostCapabilitySelectedMode::Supervised => "SUPERVISED",
        },
    );
}

#[test]
fn mode_override_source_is_exactly_frozen() {
    let expected = [
        (HostCapabilityModeOverrideSource::User, "USER"),
        (HostCapabilityModeOverrideSource::Admin, "ADMIN"),
        (HostCapabilityModeOverrideSource::Debug, "DEBUG"),
    ];
    assert_closed(
        HostCapabilityModeOverrideSource::ALL,
        expected,
        HostCapabilityModeOverrideSource::as_str,
        |value| match value {
            HostCapabilityModeOverrideSource::User => "USER",
            HostCapabilityModeOverrideSource::Admin => "ADMIN",
            HostCapabilityModeOverrideSource::Debug => "DEBUG",
        },
    );
}

#[test]
fn inactive_reason_is_exactly_frozen() {
    let expected = [
        (HostCapabilityInactiveReason::None, "NONE"),
        (
            HostCapabilityInactiveReason::PluginNotInstalled,
            "PLUGIN_NOT_INSTALLED",
        ),
        (
            HostCapabilityInactiveReason::HooksUnsupported,
            "HOOKS_UNSUPPORTED",
        ),
        (
            HostCapabilityInactiveReason::HooksNotConfigured,
            "HOOKS_NOT_CONFIGURED",
        ),
        (
            HostCapabilityInactiveReason::HooksUntrusted,
            "HOOKS_UNTRUSTED",
        ),
        (
            HostCapabilityInactiveReason::HooksDisabled,
            "HOOKS_DISABLED",
        ),
        (
            HostCapabilityInactiveReason::HooksExcludedByAdminPolicy,
            "HOOKS_EXCLUDED_BY_ADMIN_POLICY",
        ),
        (
            HostCapabilityInactiveReason::InsufficientCoverage,
            "INSUFFICIENT_COVERAGE",
        ),
        (HostCapabilityInactiveReason::ModeOverride, "MODE_OVERRIDE"),
        (HostCapabilityInactiveReason::Unknown, "UNKNOWN"),
    ];
    assert_closed(
        HostCapabilityInactiveReason::ALL,
        expected,
        HostCapabilityInactiveReason::as_str,
        |value| match value {
            HostCapabilityInactiveReason::None => "NONE",
            HostCapabilityInactiveReason::PluginNotInstalled => "PLUGIN_NOT_INSTALLED",
            HostCapabilityInactiveReason::HooksUnsupported => "HOOKS_UNSUPPORTED",
            HostCapabilityInactiveReason::HooksNotConfigured => "HOOKS_NOT_CONFIGURED",
            HostCapabilityInactiveReason::HooksUntrusted => "HOOKS_UNTRUSTED",
            HostCapabilityInactiveReason::HooksDisabled => "HOOKS_DISABLED",
            HostCapabilityInactiveReason::HooksExcludedByAdminPolicy => {
                "HOOKS_EXCLUDED_BY_ADMIN_POLICY"
            }
            HostCapabilityInactiveReason::InsufficientCoverage => "INSUFFICIENT_COVERAGE",
            HostCapabilityInactiveReason::ModeOverride => "MODE_OVERRIDE",
            HostCapabilityInactiveReason::Unknown => "UNKNOWN",
        },
    );
}

#[test]
fn evidence_label_is_exactly_frozen() {
    let expected = [
        (
            HostCapabilityEvidenceLabel::VerifiedCurrentSelfFetched,
            "VERIFIED_CURRENT_SELF_FETCHED",
        ),
        (
            HostCapabilityEvidenceLabel::ReviewerSuppliedCurrentPrimarySource,
            "REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE",
        ),
        (
            HostCapabilityEvidenceLabel::VerifiedHistorical,
            "VERIFIED_HISTORICAL",
        ),
        (
            HostCapabilityEvidenceLabel::IndependentVerified,
            "INDEPENDENT_VERIFIED",
        ),
        (HostCapabilityEvidenceLabel::UserDeclared, "USER_DECLARED"),
        (
            HostCapabilityEvidenceLabel::DesignDecision,
            "DESIGN_DECISION",
        ),
        (HostCapabilityEvidenceLabel::Assumption, "ASSUMPTION"),
        (HostCapabilityEvidenceLabel::Unverified, "UNVERIFIED"),
        (
            HostCapabilityEvidenceLabel::PolicyNeedsReview,
            "POLICY_NEEDS_REVIEW",
        ),
    ];
    assert_closed(
        HostCapabilityEvidenceLabel::ALL,
        expected,
        HostCapabilityEvidenceLabel::as_str,
        |value| match value {
            HostCapabilityEvidenceLabel::VerifiedCurrentSelfFetched => {
                "VERIFIED_CURRENT_SELF_FETCHED"
            }
            HostCapabilityEvidenceLabel::ReviewerSuppliedCurrentPrimarySource => {
                "REVIEWER_SUPPLIED_CURRENT_PRIMARY_SOURCE"
            }
            HostCapabilityEvidenceLabel::VerifiedHistorical => "VERIFIED_HISTORICAL",
            HostCapabilityEvidenceLabel::IndependentVerified => "INDEPENDENT_VERIFIED",
            HostCapabilityEvidenceLabel::UserDeclared => "USER_DECLARED",
            HostCapabilityEvidenceLabel::DesignDecision => "DESIGN_DECISION",
            HostCapabilityEvidenceLabel::Assumption => "ASSUMPTION",
            HostCapabilityEvidenceLabel::Unverified => "UNVERIFIED",
            HostCapabilityEvidenceLabel::PolicyNeedsReview => "POLICY_NEEDS_REVIEW",
        },
    );
}

#[test]
fn total_closed_vocabulary_entry_count_is_36() {
    assert_eq!(
        HostCapabilityProbeStatus::ALL.len()
            + HostCapabilityStaleReason::ALL.len()
            + HostCapabilityHookCoverageClass::ALL.len()
            + HostCapabilitySelectedMode::ALL.len()
            + HostCapabilityModeOverrideSource::ALL.len()
            + HostCapabilityInactiveReason::ALL.len()
            + HostCapabilityEvidenceLabel::ALL.len(),
        36
    );
}
