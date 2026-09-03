//! Tests proving the four dispatch-admission vocabularies match the frozen
//! `DispatchAdmissionDecision` schema exactly, and that the pre-existing
//! graph contracts are unchanged.

use std::fmt::Debug;

use super::{
    DispatchAdmissionAxisResult, DispatchAdmissionDenialReason, DispatchAdmissionFailingAxis,
    DispatchAdmissionOutcome,
};
use crate::{
    AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS, AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS,
    AUTHORIZED_PREFIX_TRANSITIONS, AUTHORIZED_REJECTED_REPAIR_TRANSITIONS,
    AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS, AUTHORIZED_REVIEW_VERDICT_TRANSITIONS,
    GraphMutationOperationKind, GraphNodeCheckResult, GraphNodeResultOutcome, GraphNodeState,
};

const OUTCOMES: [DispatchAdmissionOutcome; 2] = [
    DispatchAdmissionOutcome::Allow,
    DispatchAdmissionOutcome::Deny,
];
const OUTCOME_STRINGS: [&str; 2] = ["ALLOW", "DENY"];

const FAILING_AXES: [DispatchAdmissionFailingAxis; 7] = [
    DispatchAdmissionFailingAxis::None,
    DispatchAdmissionFailingAxis::Entitlement,
    DispatchAdmissionFailingAxis::ProviderAuth,
    DispatchAdmissionFailingAxis::ProviderPolicy,
    DispatchAdmissionFailingAxis::ProviderAvailability,
    DispatchAdmissionFailingAxis::Safety,
    DispatchAdmissionFailingAxis::QualityFloor,
];
const FAILING_AXIS_STRINGS: [&str; 7] = [
    "NONE",
    "ENTITLEMENT",
    "PROVIDER_AUTH",
    "PROVIDER_POLICY",
    "PROVIDER_AVAILABILITY",
    "SAFETY",
    "QUALITY_FLOOR",
];

const DENIAL_REASONS: [DispatchAdmissionDenialReason; 14] = [
    DispatchAdmissionDenialReason::None,
    DispatchAdmissionDenialReason::LockedRequiresPro,
    DispatchAdmissionDenialReason::EntitlementUnknown,
    DispatchAdmissionDenialReason::EntitlementExpired,
    DispatchAdmissionDenialReason::AuthRequired,
    DispatchAdmissionDenialReason::ProviderPolicyDisallowed,
    DispatchAdmissionDenialReason::ProviderPolicyUnknown,
    DispatchAdmissionDenialReason::ProviderRateLimited,
    DispatchAdmissionDenialReason::ProviderDown,
    DispatchAdmissionDenialReason::NoEligibleRuntime,
    DispatchAdmissionDenialReason::SafetyCheckPending,
    DispatchAdmissionDenialReason::PolicyBlocked,
    DispatchAdmissionDenialReason::HumanRequired,
    DispatchAdmissionDenialReason::QualityFloorUnsatisfied,
];
const DENIAL_REASON_STRINGS: [&str; 14] = [
    "NONE",
    "LOCKED_REQUIRES_PRO",
    "ENTITLEMENT_UNKNOWN",
    "ENTITLEMENT_EXPIRED",
    "AUTH_REQUIRED",
    "PROVIDER_POLICY_DISALLOWED",
    "PROVIDER_POLICY_UNKNOWN",
    "PROVIDER_RATE_LIMITED",
    "PROVIDER_DOWN",
    "NO_ELIGIBLE_RUNTIME",
    "SAFETY_CHECK_PENDING",
    "POLICY_BLOCKED",
    "HUMAN_REQUIRED",
    "QUALITY_FLOOR_UNSATISFIED",
];

const AXIS_RESULTS: [DispatchAdmissionAxisResult; 3] = [
    DispatchAdmissionAxisResult::Pass,
    DispatchAdmissionAxisResult::Fail,
    DispatchAdmissionAxisResult::NotApplicable,
];
const AXIS_RESULT_STRINGS: [&str; 3] = ["PASS", "FAIL", "NOT_APPLICABLE"];

fn assert_pairwise_distinct<T: Debug + PartialEq>(values: &[T]) {
    for (index, value) in values.iter().enumerate() {
        for other in &values[index + 1..] {
            assert_ne!(value, other);
        }
    }
}

#[test]
fn outcome_vocabulary_matches_the_frozen_schema() {
    assert_eq!(DispatchAdmissionOutcome::ALL.len(), 2);
    assert_eq!(DispatchAdmissionOutcome::ALL, OUTCOMES);

    for (outcome, expected) in OUTCOMES.iter().zip(OUTCOME_STRINGS) {
        let exhaustive = match outcome {
            DispatchAdmissionOutcome::Allow => "ALLOW",
            DispatchAdmissionOutcome::Deny => "DENY",
        };
        assert_eq!(outcome.as_str(), expected);
        assert_eq!(exhaustive, expected);
    }

    assert_pairwise_distinct(&DispatchAdmissionOutcome::ALL);
    assert_pairwise_distinct(&OUTCOME_STRINGS);
}

#[test]
fn failing_axis_vocabulary_matches_the_frozen_schema() {
    assert_eq!(DispatchAdmissionFailingAxis::ALL.len(), 7);
    assert_eq!(DispatchAdmissionFailingAxis::ALL, FAILING_AXES);

    for (axis, expected) in FAILING_AXES.iter().zip(FAILING_AXIS_STRINGS) {
        let exhaustive = match axis {
            DispatchAdmissionFailingAxis::None => "NONE",
            DispatchAdmissionFailingAxis::Entitlement => "ENTITLEMENT",
            DispatchAdmissionFailingAxis::ProviderAuth => "PROVIDER_AUTH",
            DispatchAdmissionFailingAxis::ProviderPolicy => "PROVIDER_POLICY",
            DispatchAdmissionFailingAxis::ProviderAvailability => "PROVIDER_AVAILABILITY",
            DispatchAdmissionFailingAxis::Safety => "SAFETY",
            DispatchAdmissionFailingAxis::QualityFloor => "QUALITY_FLOOR",
        };
        assert_eq!(axis.as_str(), expected);
        assert_eq!(exhaustive, expected);
    }

    assert_pairwise_distinct(&DispatchAdmissionFailingAxis::ALL);
    assert_pairwise_distinct(&FAILING_AXIS_STRINGS);
}

#[test]
fn denial_reason_vocabulary_matches_the_frozen_schema() {
    assert_eq!(DispatchAdmissionDenialReason::ALL.len(), 14);
    assert_eq!(DispatchAdmissionDenialReason::ALL, DENIAL_REASONS);

    for (reason, expected) in DENIAL_REASONS.iter().zip(DENIAL_REASON_STRINGS) {
        let exhaustive = match reason {
            DispatchAdmissionDenialReason::None => "NONE",
            DispatchAdmissionDenialReason::LockedRequiresPro => "LOCKED_REQUIRES_PRO",
            DispatchAdmissionDenialReason::EntitlementUnknown => "ENTITLEMENT_UNKNOWN",
            DispatchAdmissionDenialReason::EntitlementExpired => "ENTITLEMENT_EXPIRED",
            DispatchAdmissionDenialReason::AuthRequired => "AUTH_REQUIRED",
            DispatchAdmissionDenialReason::ProviderPolicyDisallowed => "PROVIDER_POLICY_DISALLOWED",
            DispatchAdmissionDenialReason::ProviderPolicyUnknown => "PROVIDER_POLICY_UNKNOWN",
            DispatchAdmissionDenialReason::ProviderRateLimited => "PROVIDER_RATE_LIMITED",
            DispatchAdmissionDenialReason::ProviderDown => "PROVIDER_DOWN",
            DispatchAdmissionDenialReason::NoEligibleRuntime => "NO_ELIGIBLE_RUNTIME",
            DispatchAdmissionDenialReason::SafetyCheckPending => "SAFETY_CHECK_PENDING",
            DispatchAdmissionDenialReason::PolicyBlocked => "POLICY_BLOCKED",
            DispatchAdmissionDenialReason::HumanRequired => "HUMAN_REQUIRED",
            DispatchAdmissionDenialReason::QualityFloorUnsatisfied => "QUALITY_FLOOR_UNSATISFIED",
        };
        assert_eq!(reason.as_str(), expected);
        assert_eq!(exhaustive, expected);
    }

    assert_pairwise_distinct(&DispatchAdmissionDenialReason::ALL);
    assert_pairwise_distinct(&DENIAL_REASON_STRINGS);
}

#[test]
fn denial_reason_positions_and_spellings_are_exact() {
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[1],
        DispatchAdmissionDenialReason::LockedRequiresPro
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[1].as_str(),
        "LOCKED_REQUIRES_PRO"
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[2],
        DispatchAdmissionDenialReason::EntitlementUnknown
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[2].as_str(),
        "ENTITLEMENT_UNKNOWN"
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[3],
        DispatchAdmissionDenialReason::EntitlementExpired
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[3].as_str(),
        "ENTITLEMENT_EXPIRED"
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[5],
        DispatchAdmissionDenialReason::ProviderPolicyDisallowed
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[5].as_str(),
        "PROVIDER_POLICY_DISALLOWED"
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[6],
        DispatchAdmissionDenialReason::ProviderPolicyUnknown
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[6].as_str(),
        "PROVIDER_POLICY_UNKNOWN"
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[7],
        DispatchAdmissionDenialReason::ProviderRateLimited
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[7].as_str(),
        "PROVIDER_RATE_LIMITED"
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[9],
        DispatchAdmissionDenialReason::NoEligibleRuntime
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[9].as_str(),
        "NO_ELIGIBLE_RUNTIME"
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[10],
        DispatchAdmissionDenialReason::SafetyCheckPending
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[10].as_str(),
        "SAFETY_CHECK_PENDING"
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[11],
        DispatchAdmissionDenialReason::PolicyBlocked
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[11].as_str(),
        "POLICY_BLOCKED"
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[12],
        DispatchAdmissionDenialReason::HumanRequired
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[12].as_str(),
        "HUMAN_REQUIRED"
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[13],
        DispatchAdmissionDenialReason::QualityFloorUnsatisfied
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL[13].as_str(),
        "QUALITY_FLOOR_UNSATISFIED"
    );
}

#[test]
fn axis_result_vocabulary_matches_the_frozen_schema() {
    assert_eq!(DispatchAdmissionAxisResult::ALL.len(), 3);
    assert_eq!(DispatchAdmissionAxisResult::ALL, AXIS_RESULTS);

    for (result, expected) in AXIS_RESULTS.iter().zip(AXIS_RESULT_STRINGS) {
        let exhaustive = match result {
            DispatchAdmissionAxisResult::Pass => "PASS",
            DispatchAdmissionAxisResult::Fail => "FAIL",
            DispatchAdmissionAxisResult::NotApplicable => "NOT_APPLICABLE",
        };
        assert_eq!(result.as_str(), expected);
        assert_eq!(exhaustive, expected);
    }

    assert_pairwise_distinct(&DispatchAdmissionAxisResult::ALL);
    assert_pairwise_distinct(&AXIS_RESULT_STRINGS);
}

#[test]
fn total_closed_value_count_is_twenty_six() {
    assert_eq!(
        DispatchAdmissionOutcome::ALL.len()
            + DispatchAdmissionFailingAxis::ALL.len()
            + DispatchAdmissionDenialReason::ALL.len()
            + DispatchAdmissionAxisResult::ALL.len(),
        26
    );
}

#[test]
fn graph_contracts_are_unchanged() {
    assert_eq!(GraphNodeState::ALL.len(), 15);
    assert_eq!(GraphNodeResultOutcome::ALL.len(), 7);
    assert_eq!(GraphNodeCheckResult::ALL.len(), 5);
    assert_eq!(GraphMutationOperationKind::ALL.len(), 6);

    assert_eq!(
        AUTHORIZED_PREFIX_TRANSITIONS.len()
            + AUTHORIZED_REVIEW_VERDICT_TRANSITIONS.len()
            + AUTHORIZED_REJECTED_REPAIR_TRANSITIONS.len()
            + AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS.len()
            + AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS.len()
            + AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS.len(),
        11
    );
}
