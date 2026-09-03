//! Tests for [`DispatchAdmissionDecisionCore`]: owned constraint validation,
//! read-only accessor preservation, storage-only denial reasons,
//! provider/runtime independence, and A3-012 plus graph regressions.

use super::dispatch_admission_core::{
    DispatchAdmissionDecisionCore, DispatchAdmissionDecisionCoreError,
};
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

/// Every non-`NONE` failing axis in frozen schema order.
const NON_NONE_AXES: [DispatchAdmissionFailingAxis; 6] = [
    DispatchAdmissionFailingAxis::Entitlement,
    DispatchAdmissionFailingAxis::ProviderAuth,
    DispatchAdmissionFailingAxis::ProviderPolicy,
    DispatchAdmissionFailingAxis::ProviderAvailability,
    DispatchAdmissionFailingAxis::Safety,
    DispatchAdmissionFailingAxis::QualityFloor,
];

/// Builds a representative valid core decision through the public constructor.
fn valid_core() -> DispatchAdmissionDecisionCore {
    DispatchAdmissionDecisionCore::try_new(
        "decision-1".to_owned(),
        "node-1".to_owned(),
        Some("graph-1".to_owned()),
        Some("capability-1".to_owned()),
        DispatchAdmissionOutcome::Deny,
        DispatchAdmissionFailingAxis::Safety,
        Some(DispatchAdmissionDenialReason::SafetyCheckPending),
        Some("provider-1".to_owned()),
        Some("runtime-1".to_owned()),
    )
    .expect("representative valid core decision must construct")
}

/// Builds a valid `ALLOW` core decision with the given bounded identifiers.
///
/// Kept to three arguments so test scaffolding introduces no lint
/// suppressions of its own; every other test supplies remaining fields
/// through direct [`DispatchAdmissionDecisionCore::try_new`] calls.
fn allow_none_with_ids(
    decision_id: &str,
    node_id: &str,
    graph_id: Option<&str>,
) -> DispatchAdmissionDecisionCore {
    DispatchAdmissionDecisionCore::try_new(
        decision_id.to_owned(),
        node_id.to_owned(),
        graph_id.map(str::to_owned),
        None,
        DispatchAdmissionOutcome::Allow,
        DispatchAdmissionFailingAxis::None,
        None,
        None,
        None,
    )
    .expect("ALLOW + NONE with valid identifiers must construct")
}

#[test]
fn construction_exposes_all_nine_fields_through_read_only_accessors() {
    let core = valid_core();

    assert_eq!(core.decision_id(), "decision-1");
    assert_eq!(core.node_id(), "node-1");
    assert_eq!(core.graph_id(), Some("graph-1"));
    assert_eq!(core.capability_id(), Some("capability-1"));
    assert_eq!(core.outcome(), DispatchAdmissionOutcome::Deny);
    assert_eq!(core.failing_axis(), DispatchAdmissionFailingAxis::Safety);
    assert_eq!(
        core.denial_reason(),
        Some(DispatchAdmissionDenialReason::SafetyCheckPending)
    );
    assert_eq!(core.selected_provider(), Some("provider-1"));
    assert_eq!(core.selected_runtime(), Some("runtime-1"));
}

#[test]
fn decision_id_length_is_validated_in_characters_not_bytes() {
    assert_eq!(
        DispatchAdmissionDecisionCore::try_new(
            String::new(),
            "node-1".to_owned(),
            None,
            None,
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
            None,
            None,
            None,
        ),
        Err(DispatchAdmissionDecisionCoreError::DecisionIdLengthOutOfRange { character_count: 0 })
    );

    let one = allow_none_with_ids("d", "node-1", None);
    assert_eq!(one.decision_id(), "d");

    let boundary = "🦀".repeat(200);
    assert_eq!(boundary.chars().count(), 200);
    assert!(boundary.len() > 200);
    let accepted = allow_none_with_ids(&boundary, "node-1", None);
    assert_eq!(accepted.decision_id(), boundary);

    let over = "🦀".repeat(201);
    assert_eq!(over.chars().count(), 201);
    assert_eq!(
        DispatchAdmissionDecisionCore::try_new(
            over,
            "node-1".to_owned(),
            None,
            None,
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
            None,
            None,
            None,
        ),
        Err(
            DispatchAdmissionDecisionCoreError::DecisionIdLengthOutOfRange {
                character_count: 201
            }
        )
    );
}

#[test]
fn node_id_length_is_validated_in_characters_not_bytes() {
    assert_eq!(
        DispatchAdmissionDecisionCore::try_new(
            "decision-1".to_owned(),
            String::new(),
            None,
            None,
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
            None,
            None,
            None,
        ),
        Err(DispatchAdmissionDecisionCoreError::NodeIdLengthOutOfRange { character_count: 0 })
    );

    let one = allow_none_with_ids("decision-1", "n", None);
    assert_eq!(one.node_id(), "n");

    let boundary = "🦀".repeat(200);
    assert_eq!(boundary.chars().count(), 200);
    assert!(boundary.len() > 200);
    let accepted = allow_none_with_ids("decision-1", &boundary, None);
    assert_eq!(accepted.node_id(), boundary);

    let over = "🦀".repeat(201);
    assert_eq!(
        DispatchAdmissionDecisionCore::try_new(
            "decision-1".to_owned(),
            over,
            None,
            None,
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
            None,
            None,
            None,
        ),
        Err(DispatchAdmissionDecisionCoreError::NodeIdLengthOutOfRange {
            character_count: 201
        })
    );
}

#[test]
fn graph_id_absence_is_valid_and_presence_is_bounded_in_characters() {
    let absent = allow_none_with_ids("decision-1", "node-1", None);
    assert_eq!(absent.graph_id(), None);

    assert_eq!(
        DispatchAdmissionDecisionCore::try_new(
            "decision-1".to_owned(),
            "node-1".to_owned(),
            Some(String::new()),
            None,
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
            None,
            None,
            None,
        ),
        Err(DispatchAdmissionDecisionCoreError::GraphIdLengthOutOfRange { character_count: 0 })
    );

    let one = allow_none_with_ids("decision-1", "node-1", Some("g"));
    assert_eq!(one.graph_id(), Some("g"));

    let boundary = "🦀".repeat(200);
    assert_eq!(boundary.chars().count(), 200);
    assert!(boundary.len() > 200);
    let accepted = allow_none_with_ids("decision-1", "node-1", Some(&boundary));
    assert_eq!(accepted.graph_id(), Some(boundary.as_str()));

    let over = "🦀".repeat(201);
    assert_eq!(
        DispatchAdmissionDecisionCore::try_new(
            "decision-1".to_owned(),
            "node-1".to_owned(),
            Some(over),
            None,
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
            None,
            None,
            None,
        ),
        Err(
            DispatchAdmissionDecisionCoreError::GraphIdLengthOutOfRange {
                character_count: 201
            }
        )
    );
}

#[test]
fn unconstrained_optional_strings_accept_empty_and_over_200_characters() {
    for value in [
        Some(""),
        Some("x"),
        Some(&"y".repeat(201)),
        Some(&"z".repeat(1000)),
    ] {
        let owned = value.map(|v| v.to_owned());
        let core = DispatchAdmissionDecisionCore::try_new(
            "decision-1".to_owned(),
            "node-1".to_owned(),
            None,
            owned.clone(),
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
            None,
            None,
            None,
        )
        .expect("capability_id is unconstrained and must be accepted");
        assert_eq!(core.capability_id(), owned.as_deref());
    }

    for value in [
        Some(""),
        Some("p"),
        Some(&"q".repeat(201)),
        Some(&"r".repeat(1000)),
    ] {
        let owned = value.map(|v| v.to_owned());
        let core = DispatchAdmissionDecisionCore::try_new(
            "decision-1".to_owned(),
            "node-1".to_owned(),
            None,
            None,
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
            None,
            owned.clone(),
            None,
        )
        .expect("selected_provider is unconstrained and must be accepted");
        assert_eq!(core.selected_provider(), owned.as_deref());
    }

    for value in [
        Some(""),
        Some("s"),
        Some(&"t".repeat(201)),
        Some(&"u".repeat(1000)),
    ] {
        let owned = value.map(|v| v.to_owned());
        let core = DispatchAdmissionDecisionCore::try_new(
            "decision-1".to_owned(),
            "node-1".to_owned(),
            None,
            None,
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
            None,
            None,
            owned.clone(),
        )
        .expect("selected_runtime is unconstrained and must be accepted");
        assert_eq!(core.selected_runtime(), owned.as_deref());
    }

    let multibyte_over = "🦀".repeat(201);
    assert!(multibyte_over.len() > 201);
    let core = DispatchAdmissionDecisionCore::try_new(
        "decision-1".to_owned(),
        "node-1".to_owned(),
        None,
        Some(multibyte_over.clone()),
        DispatchAdmissionOutcome::Allow,
        DispatchAdmissionFailingAxis::None,
        None,
        Some(multibyte_over.clone()),
        Some(multibyte_over.clone()),
    )
    .expect("multibyte strings over 200 bytes but unconstrained must be accepted");
    assert_eq!(core.capability_id(), Some(multibyte_over.as_str()));
    assert_eq!(core.selected_provider(), Some(multibyte_over.as_str()));
    assert_eq!(core.selected_runtime(), Some(multibyte_over.as_str()));
}

#[test]
fn accepted_strings_are_preserved_byte_for_byte_without_trimming() {
    let core = DispatchAdmissionDecisionCore::try_new(
        " decision ".to_owned(),
        " node ".to_owned(),
        Some(" graph ".to_owned()),
        Some(" capability ".to_owned()),
        DispatchAdmissionOutcome::Deny,
        DispatchAdmissionFailingAxis::Entitlement,
        Some(DispatchAdmissionDenialReason::EntitlementExpired),
        Some(" provider ".to_owned()),
        Some(" runtime ".to_owned()),
    )
    .expect("padded strings with valid lengths must construct");

    assert_eq!(core.decision_id(), " decision ");
    assert_eq!(core.node_id(), " node ");
    assert_eq!(core.graph_id(), Some(" graph "));
    assert_eq!(core.capability_id(), Some(" capability "));
    assert_eq!(core.selected_provider(), Some(" provider "));
    assert_eq!(core.selected_runtime(), Some(" runtime "));

    let single_space = allow_none_with_ids(" ", " ", None);
    assert_eq!(single_space.decision_id(), " ");
    assert_eq!(single_space.node_id(), " ");
}

#[test]
fn outcome_failing_axis_invariant_holds_for_every_axis() {
    allow_none_with_ids("decision-1", "node-1", None);

    for axis in NON_NONE_AXES {
        assert_eq!(
            DispatchAdmissionDecisionCore::try_new(
                "decision-1".to_owned(),
                "node-1".to_owned(),
                None,
                None,
                DispatchAdmissionOutcome::Allow,
                axis,
                None,
                None,
                None,
            ),
            Err(
                DispatchAdmissionDecisionCoreError::AllowRequiresNoFailingAxis {
                    failing_axis: axis
                }
            ),
            "ALLOW with {axis:?} must be rejected"
        );
    }

    assert_eq!(
        DispatchAdmissionDecisionCore::try_new(
            "decision-1".to_owned(),
            "node-1".to_owned(),
            None,
            None,
            DispatchAdmissionOutcome::Deny,
            DispatchAdmissionFailingAxis::None,
            None,
            None,
            None,
        ),
        Err(DispatchAdmissionDecisionCoreError::DenyRequiresFailingAxis)
    );

    for axis in NON_NONE_AXES {
        let core = DispatchAdmissionDecisionCore::try_new(
            "decision-1".to_owned(),
            "node-1".to_owned(),
            None,
            None,
            DispatchAdmissionOutcome::Deny,
            axis,
            None,
            None,
            None,
        )
        .expect("DENY with a non-NONE axis must construct");
        assert_eq!(core.outcome(), DispatchAdmissionOutcome::Deny);
        assert_eq!(core.failing_axis(), axis);
    }

    assert_eq!(DispatchAdmissionFailingAxis::ALL.len(), 7);
    assert_eq!(NON_NONE_AXES.len(), 6);
}

#[test]
fn denial_reason_is_storage_only_with_no_axis_mapping() {
    let without_reason = DispatchAdmissionDecisionCore::try_new(
        "decision-1".to_owned(),
        "node-1".to_owned(),
        None,
        None,
        DispatchAdmissionOutcome::Deny,
        DispatchAdmissionFailingAxis::Entitlement,
        None,
        None,
        None,
    )
    .expect("DENY with a valid axis and no denial reason must construct");
    assert_eq!(without_reason.denial_reason(), None);

    let enum_none = DispatchAdmissionDecisionCore::try_new(
        "decision-1".to_owned(),
        "node-1".to_owned(),
        None,
        None,
        DispatchAdmissionOutcome::Deny,
        DispatchAdmissionFailingAxis::Entitlement,
        Some(DispatchAdmissionDenialReason::None),
        None,
        None,
    )
    .expect("DENY storing the canonical NONE reason must construct");
    assert_eq!(
        enum_none.denial_reason(),
        Some(DispatchAdmissionDenialReason::None)
    );

    let allow_with_reason = DispatchAdmissionDecisionCore::try_new(
        "decision-1".to_owned(),
        "node-1".to_owned(),
        None,
        None,
        DispatchAdmissionOutcome::Allow,
        DispatchAdmissionFailingAxis::None,
        Some(DispatchAdmissionDenialReason::PolicyBlocked),
        None,
        None,
    )
    .expect("ALLOW storing an arbitrary denial reason must construct");
    assert_eq!(
        allow_with_reason.denial_reason(),
        Some(DispatchAdmissionDenialReason::PolicyBlocked)
    );

    let deliberately_unmapped = DispatchAdmissionDecisionCore::try_new(
        "decision-1".to_owned(),
        "node-1".to_owned(),
        None,
        None,
        DispatchAdmissionOutcome::Deny,
        DispatchAdmissionFailingAxis::Safety,
        Some(DispatchAdmissionDenialReason::EntitlementExpired),
        None,
        None,
    )
    .expect("a deliberately non-mapped reason-to-axis pair must construct");
    assert_eq!(
        deliberately_unmapped.failing_axis(),
        DispatchAdmissionFailingAxis::Safety
    );
    assert_eq!(
        deliberately_unmapped.denial_reason(),
        Some(DispatchAdmissionDenialReason::EntitlementExpired)
    );
}

#[test]
fn provider_and_runtime_have_no_pair_or_presence_rule() {
    let provider_only = DispatchAdmissionDecisionCore::try_new(
        "decision-1".to_owned(),
        "node-1".to_owned(),
        None,
        None,
        DispatchAdmissionOutcome::Allow,
        DispatchAdmissionFailingAxis::None,
        None,
        Some(String::new()),
        None,
    )
    .expect("provider without runtime must construct");
    assert_eq!(provider_only.selected_provider(), Some(""));
    assert_eq!(provider_only.selected_runtime(), None);

    let runtime_only = DispatchAdmissionDecisionCore::try_new(
        "decision-1".to_owned(),
        "node-1".to_owned(),
        None,
        None,
        DispatchAdmissionOutcome::Allow,
        DispatchAdmissionFailingAxis::None,
        None,
        None,
        Some("runtime".to_owned()),
    )
    .expect("runtime without provider must construct");
    assert_eq!(runtime_only.selected_provider(), None);
    assert_eq!(runtime_only.selected_runtime(), Some("runtime"));

    let deny_split = DispatchAdmissionDecisionCore::try_new(
        "decision-1".to_owned(),
        "node-1".to_owned(),
        None,
        None,
        DispatchAdmissionOutcome::Deny,
        DispatchAdmissionFailingAxis::ProviderAuth,
        None,
        Some("provider".to_owned()),
        None,
    )
    .expect("DENY with provider but no runtime must construct");
    assert_eq!(deny_split.selected_provider(), Some("provider"));
    assert_eq!(deny_split.selected_runtime(), None);
}

#[test]
fn a3_012_vocabularies_are_preserved() {
    assert_eq!(DispatchAdmissionOutcome::ALL.len(), 2);
    assert_eq!(DispatchAdmissionFailingAxis::ALL.len(), 7);
    assert_eq!(DispatchAdmissionDenialReason::ALL.len(), 14);
    assert_eq!(DispatchAdmissionAxisResult::ALL.len(), 3);
    assert_eq!(
        DispatchAdmissionOutcome::ALL.len()
            + DispatchAdmissionFailingAxis::ALL.len()
            + DispatchAdmissionDenialReason::ALL.len()
            + DispatchAdmissionAxisResult::ALL.len(),
        26
    );

    assert_eq!(
        DispatchAdmissionOutcome::ALL.map(|outcome| outcome.as_str()),
        ["ALLOW", "DENY"]
    );
    assert_eq!(
        DispatchAdmissionFailingAxis::ALL.map(|axis| axis.as_str()),
        [
            "NONE",
            "ENTITLEMENT",
            "PROVIDER_AUTH",
            "PROVIDER_POLICY",
            "PROVIDER_AVAILABILITY",
            "SAFETY",
            "QUALITY_FLOOR",
        ]
    );
    assert_eq!(
        DispatchAdmissionDenialReason::ALL.map(|reason| reason.as_str()),
        [
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
        ]
    );
    assert_eq!(
        DispatchAdmissionAxisResult::ALL.map(|result| result.as_str()),
        ["PASS", "FAIL", "NOT_APPLICABLE"]
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
