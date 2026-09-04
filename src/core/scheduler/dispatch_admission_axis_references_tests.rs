use super::{
    DispatchAdmissionAxisReferenceError, DispatchAdmissionAxisResult,
    DispatchAdmissionDecisionCore, DispatchAdmissionDenialReason,
    DispatchAdmissionEntitlementAxisResult, DispatchAdmissionFailingAxis, DispatchAdmissionOutcome,
    DispatchAdmissionProviderAvailabilityAxisResult, DispatchAdmissionQualityFloorAxisResult,
    DispatchAdmissionSafetyAxisResult,
};
use crate::{
    AUTHORIZED_ACCEPTED_INTEGRATION_TRANSITIONS, AUTHORIZED_PASSED_ACCEPTANCE_TRANSITIONS,
    AUTHORIZED_PREFIX_TRANSITIONS, AUTHORIZED_REJECTED_REPAIR_TRANSITIONS,
    AUTHORIZED_REPAIR_COMPLETION_TRANSITIONS, AUTHORIZED_REVIEW_VERDICT_TRANSITIONS,
    GraphMutationOperationKind, GraphNodeCheckResult, GraphNodeResultOutcome, GraphNodeState,
};

macro_rules! reference_contract_test {
    ($test_name:ident, $record:ty, $accessor:ident, $error_variant:ident) => {
        #[test]
        fn $test_name() {
            let absent = <$record>::try_new(DispatchAdmissionAxisResult::Pass, None)
                .expect("an absent reference must be accepted");
            assert_eq!(absent.result(), DispatchAdmissionAxisResult::Pass);
            assert_eq!(absent.$accessor(), None);

            assert_eq!(
                <$record>::try_new(DispatchAdmissionAxisResult::Fail, Some(String::new())),
                Err(DispatchAdmissionAxisReferenceError::$error_variant { character_count: 0 })
            );

            let one = <$record>::try_new(DispatchAdmissionAxisResult::Fail, Some("x".to_owned()))
                .expect("one character must be accepted");
            assert_eq!(one.result(), DispatchAdmissionAxisResult::Fail);
            assert_eq!(one.$accessor(), Some("x"));

            let boundary = "🦀".repeat(200);
            assert_eq!(boundary.chars().count(), 200);
            assert!(boundary.len() > 200);
            let accepted = <$record>::try_new(
                DispatchAdmissionAxisResult::NotApplicable,
                Some(boundary.clone()),
            )
            .expect("200 Unicode characters must be accepted");
            assert_eq!(
                accepted.result(),
                DispatchAdmissionAxisResult::NotApplicable
            );
            assert_eq!(accepted.$accessor(), Some(boundary.as_str()));

            let over = "🦀".repeat(201);
            assert_eq!(over.chars().count(), 201);
            assert!(over.len() > 201);
            assert_eq!(
                <$record>::try_new(DispatchAdmissionAxisResult::Pass, Some(over)),
                Err(DispatchAdmissionAxisReferenceError::$error_variant {
                    character_count: 201,
                })
            );

            let whitespace =
                <$record>::try_new(DispatchAdmissionAxisResult::Pass, Some(" ".to_owned()))
                    .expect("one space is one valid character");
            assert_eq!(whitespace.$accessor(), Some(" "));

            let exact = "  e\u{301} É  ".to_owned();
            let preserved =
                <$record>::try_new(DispatchAdmissionAxisResult::Pass, Some(exact.clone()))
                    .expect("surrounding and decomposed Unicode content must be accepted");
            assert_eq!(preserved.$accessor(), Some(exact.as_str()));
        }
    };
}

reference_contract_test!(
    entitlement_reference_contract,
    DispatchAdmissionEntitlementAxisResult,
    feature_admission_decision_id,
    FeatureAdmissionDecisionIdLengthOutOfRange
);
reference_contract_test!(
    provider_availability_reference_contract,
    DispatchAdmissionProviderAvailabilityAxisResult,
    availability_state_ref,
    AvailabilityStateRefLengthOutOfRange
);
reference_contract_test!(
    safety_reference_contract,
    DispatchAdmissionSafetyAxisResult,
    safety_interruption_id,
    SafetyInterruptionIdLengthOutOfRange
);
reference_contract_test!(
    quality_floor_reference_contract,
    DispatchAdmissionQualityFloorAxisResult,
    routing_decision_id,
    RoutingDecisionIdLengthOutOfRange
);

#[test]
fn all_four_records_accept_the_complete_result_reference_presence_matrix() {
    let mut success_count = 0;

    for result in DispatchAdmissionAxisResult::ALL {
        let entitlement = DispatchAdmissionEntitlementAxisResult::try_new(result, None)
            .expect("entitlement must accept every result without a reference");
        assert_eq!(entitlement.result(), result);
        assert_eq!(entitlement.feature_admission_decision_id(), None);
        success_count += 1;
        let entitlement =
            DispatchAdmissionEntitlementAxisResult::try_new(result, Some("x".to_owned()))
                .expect("entitlement must accept every result with a valid reference");
        assert_eq!(entitlement.result(), result);
        assert_eq!(entitlement.feature_admission_decision_id(), Some("x"));
        success_count += 1;

        let availability = DispatchAdmissionProviderAvailabilityAxisResult::try_new(result, None)
            .expect("availability must accept every result without a reference");
        assert_eq!(availability.result(), result);
        assert_eq!(availability.availability_state_ref(), None);
        success_count += 1;
        let availability =
            DispatchAdmissionProviderAvailabilityAxisResult::try_new(result, Some("x".to_owned()))
                .expect("availability must accept every result with a valid reference");
        assert_eq!(availability.result(), result);
        assert_eq!(availability.availability_state_ref(), Some("x"));
        success_count += 1;

        let safety = DispatchAdmissionSafetyAxisResult::try_new(result, None)
            .expect("safety must accept every result without a reference");
        assert_eq!(safety.result(), result);
        assert_eq!(safety.safety_interruption_id(), None);
        success_count += 1;
        let safety = DispatchAdmissionSafetyAxisResult::try_new(result, Some("x".to_owned()))
            .expect("safety must accept every result with a valid reference");
        assert_eq!(safety.result(), result);
        assert_eq!(safety.safety_interruption_id(), Some("x"));
        success_count += 1;

        let quality = DispatchAdmissionQualityFloorAxisResult::try_new(result, None)
            .expect("quality floor must accept every result without a reference");
        assert_eq!(quality.result(), result);
        assert_eq!(quality.routing_decision_id(), None);
        success_count += 1;
        let quality =
            DispatchAdmissionQualityFloorAxisResult::try_new(result, Some("x".to_owned()))
                .expect("quality floor must accept every result with a valid reference");
        assert_eq!(quality.result(), result);
        assert_eq!(quality.routing_decision_id(), Some("x"));
        success_count += 1;
    }

    assert_eq!(success_count, 24);
}

#[test]
fn a3_012_vocabularies_are_preserved() {
    assert_eq!(
        DispatchAdmissionAxisResult::ALL,
        [
            DispatchAdmissionAxisResult::Pass,
            DispatchAdmissionAxisResult::Fail,
            DispatchAdmissionAxisResult::NotApplicable,
        ]
    );
    assert_eq!(
        DispatchAdmissionAxisResult::ALL.map(|result| result.as_str()),
        ["PASS", "FAIL", "NOT_APPLICABLE"]
    );
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
}

#[test]
fn a3_013_core_still_constructs_through_its_public_api() {
    let core = DispatchAdmissionDecisionCore::try_new(
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
    .expect("the accepted A3-013 core must still construct");

    assert_eq!(core.decision_id(), "decision-1");
    assert_eq!(core.node_id(), "node-1");
    assert_eq!(core.graph_id(), Some("graph-1"));
    assert_eq!(core.outcome(), DispatchAdmissionOutcome::Deny);
    assert_eq!(core.failing_axis(), DispatchAdmissionFailingAxis::Safety);
}

#[test]
fn graph_contracts_are_preserved() {
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
