use super::*;

const AXES: [DispatchAdmissionFailingAxis; 6] = [
    DispatchAdmissionFailingAxis::Entitlement,
    DispatchAdmissionFailingAxis::ProviderAuth,
    DispatchAdmissionFailingAxis::ProviderPolicy,
    DispatchAdmissionFailingAxis::ProviderAvailability,
    DispatchAdmissionFailingAxis::Safety,
    DispatchAdmissionFailingAxis::QualityFloor,
];

fn core(
    outcome: DispatchAdmissionOutcome,
    failing_axis: DispatchAdmissionFailingAxis,
) -> Result<DispatchAdmissionDecisionCore, DispatchAdmissionDecisionCoreError> {
    DispatchAdmissionDecisionCore::try_new(
        " decision é ".into(),
        " node e\u{301} ".into(),
        Some(" graph ".into()),
        Some(String::new()),
        outcome,
        failing_axis,
        None,
        Some(" Provider/未知\0 ".into()),
        Some(" Runtime/e\u{301} ".into()),
    )
}

fn evidence(results: [DispatchAdmissionAxisResult; 6]) -> DispatchAdmissionAxisResults {
    let reference = Some(" Reference/e\u{301} 未知 ".to_owned());
    DispatchAdmissionAxisResults {
        entitlement: DispatchAdmissionEntitlementAxisResult::try_new(results[0], reference.clone())
            .unwrap(),
        provider_auth: DispatchAdmissionProviderAuthAxisResult::new(
            results[1],
            Some(" Provider/未知\0 ".into()),
            Some("CONNECTED".into()),
        ),
        provider_policy: DispatchAdmissionProviderPolicyAxisResult::try_new(
            results[2],
            reference.clone(),
            None,
            Some(" Context/e\u{301} ".into()),
        )
        .unwrap(),
        provider_availability: DispatchAdmissionProviderAvailabilityAxisResult::try_new(
            results[3],
            reference.clone(),
        )
        .unwrap(),
        safety: DispatchAdmissionSafetyAxisResult::try_new(results[4], reference.clone()).unwrap(),
        quality_floor: DispatchAdmissionQualityFloorAxisResult::try_new(results[5], reference)
            .unwrap(),
    }
}

fn results(evidence: &DispatchAdmissionAxisResults) -> [DispatchAdmissionAxisResult; 6] {
    // Destructuring names every field and forbids unnoticed extra/missing axes.
    let DispatchAdmissionAxisResults {
        entitlement,
        provider_auth,
        provider_policy,
        provider_availability,
        safety,
        quality_floor,
    } = evidence;
    [
        entitlement.result(),
        provider_auth.result(),
        provider_policy.result(),
        provider_availability.result(),
        safety.result(),
        quality_floor.result(),
    ]
}

#[test]
fn dispatch_admission_decision_exhaustive_six_axis_matrix() {
    use DispatchAdmissionAxisResult::{Fail, Pass};
    // All 3^6 result combinations, each with ALLOW and six caller-chosen DENYs.
    // Covers every single FAIL/NOT_APPLICABLE, mixed failures, and no precedence.
    for mut encoded in 0..729 {
        let supplied = std::array::from_fn(|_| {
            let result = DispatchAdmissionAxisResult::ALL[encoded % 3];
            encoded /= 3;
            result
        });
        let axes = evidence(supplied);
        let allow = DispatchAdmissionDecisionNonTemporalCore::try_new(
            core(
                DispatchAdmissionOutcome::Allow,
                DispatchAdmissionFailingAxis::None,
            )
            .unwrap(),
            axes.clone(),
        );
        if supplied == [Pass; 6] {
            assert_eq!(results(allow.unwrap().axis_results()), supplied);
        } else {
            assert_eq!(
                allow,
                Err(DispatchAdmissionDecisionNonTemporalCoreError::AllowRequiresAllSixPass)
            );
        }
        for (index, failing_axis) in AXES.into_iter().enumerate() {
            let deny = DispatchAdmissionDecisionNonTemporalCore::try_new(
                core(DispatchAdmissionOutcome::Deny, failing_axis).unwrap(),
                axes.clone(),
            );
            if supplied[index] == Fail {
                let decision = deny.unwrap();
                assert_eq!(decision.core().failing_axis(), failing_axis);
                assert_eq!(results(decision.axis_results()), supplied);
                assert_eq!(decision.axis_results(), &axes);
            } else {
                assert_eq!(
                    deny,
                    Err(DispatchAdmissionDecisionNonTemporalCoreError::FailingAxisMustReferenceFail {
                        failing_axis,
                    })
                );
            }
        }
    }
}

#[test]
fn dispatch_admission_decision_reuses_structural_core_gate() {
    for failing_axis in AXES {
        assert_eq!(
            core(DispatchAdmissionOutcome::Allow, failing_axis),
            Err(DispatchAdmissionDecisionCoreError::AllowRequiresNoFailingAxis { failing_axis })
        );
    }
    assert_eq!(
        core(
            DispatchAdmissionOutcome::Deny,
            DispatchAdmissionFailingAxis::None
        ),
        Err(DispatchAdmissionDecisionCoreError::DenyRequiresFailingAxis)
    );
}

#[test]
fn dispatch_admission_decision_preserves_core_and_all_references() {
    let supplied_core = core(
        DispatchAdmissionOutcome::Allow,
        DispatchAdmissionFailingAxis::None,
    )
    .unwrap();
    let supplied_axes = evidence([DispatchAdmissionAxisResult::Pass; 6]);
    let decision = DispatchAdmissionDecisionNonTemporalCore::try_new(
        supplied_core.clone(),
        supplied_axes.clone(),
    )
    .unwrap();
    assert_eq!(decision.core(), &supplied_core);
    assert_eq!(decision.axis_results(), &supplied_axes);
    assert_eq!(
        decision.core().selected_provider(),
        Some(" Provider/未知\0 ")
    );
    assert_eq!(
        decision.core().selected_runtime(),
        Some(" Runtime/e\u{301} ")
    );

    // Editing a cloned bundle cannot invalidate the already-validated aggregate.
    let mut detached = decision.axis_results().clone();
    detached.provider_auth =
        DispatchAdmissionProviderAuthAxisResult::new(DispatchAdmissionAxisResult::Fail, None, None);
    assert_eq!(decision.axis_results(), &supplied_axes);
    assert!(DispatchAdmissionDecisionNonTemporalCore::try_new(supplied_core, detached).is_err());
}

#[test]
fn dispatch_admission_decision_preserves_identifier_boundaries_and_optional_selections() {
    for length in [0, 1, 200, 201] {
        for field in 0..3 {
            let mut ids = [" d ".to_owned(), " n ".to_owned(), " g ".to_owned()];
            ids[field] = "🦀".repeat(length);
            let supplied = DispatchAdmissionDecisionCore::try_new(
                ids[0].clone(),
                ids[1].clone(),
                Some(ids[2].clone()),
                None,
                DispatchAdmissionOutcome::Allow,
                DispatchAdmissionFailingAxis::None,
                None,
                None,
                None,
            );
            assert_eq!(supplied.is_ok(), (1..=200).contains(&length));
            if let Ok(core) = supplied {
                let decision = DispatchAdmissionDecisionNonTemporalCore::try_new(
                    core.clone(),
                    evidence([DispatchAdmissionAxisResult::Pass; 6]),
                )
                .unwrap();
                assert_eq!(decision.core(), &core);
            }
        }
    }
    for provider in [None, Some(""), Some(" Provider/未知 ")] {
        for runtime in [None, Some(""), Some(" Runtime/e\u{301} ")] {
            let core = DispatchAdmissionDecisionCore::try_new(
                "d".into(),
                "n".into(),
                None,
                None,
                DispatchAdmissionOutcome::Allow,
                DispatchAdmissionFailingAxis::None,
                None,
                provider.map(str::to_owned),
                runtime.map(str::to_owned),
            )
            .unwrap();
            let decision = DispatchAdmissionDecisionNonTemporalCore::try_new(
                core,
                evidence([DispatchAdmissionAxisResult::Pass; 6]),
            )
            .unwrap();
            assert_eq!(decision.core().selected_provider(), provider);
            assert_eq!(decision.core().selected_runtime(), runtime);
        }
    }
}

#[test]
fn dispatch_admission_decision_policy_evidence_never_derives_or_defaults_a_result() {
    use DispatchAdmissionProviderPolicyStatus as Status;
    assert_eq!(
        Status::ALL.map(|status| status.as_str()),
        [
            "VERIFIED_ALLOWED",
            "VERIFIED_DISALLOWED",
            "NEEDS_REVIEW",
            "UNKNOWN",
        ]
    );
    for status in [
        None,
        Some(Status::VerifiedAllowed),
        Some(Status::VerifiedDisallowed),
        Some(Status::NeedsReview),
        Some(Status::Unknown),
    ] {
        for result in DispatchAdmissionAxisResult::ALL {
            let mut axes = evidence([DispatchAdmissionAxisResult::Pass; 6]);
            axes.provider_policy =
                DispatchAdmissionProviderPolicyAxisResult::try_new(result, None, status, None)
                    .unwrap();
            assert_eq!(axes.provider_policy.result(), result);
            assert_eq!(axes.provider_policy.policy_status(), status);
            assert_eq!(axes.provider_policy.provider_policy_eligibility_id(), None);
            assert_eq!(axes.provider_policy.execution_context(), None);
            let allow = DispatchAdmissionDecisionNonTemporalCore::try_new(
                core(
                    DispatchAdmissionOutcome::Allow,
                    DispatchAdmissionFailingAxis::None,
                )
                .unwrap(),
                axes,
            );
            // Status is stored evidence, not an evaluator. Even UNKNOWN must
            // have an explicitly supplied PASS (and five other PASS results).
            assert_eq!(allow.is_ok(), result == DispatchAdmissionAxisResult::Pass);
        }
    }
}

#[test]
fn dispatch_admission_decision_policy_reference_and_open_context_are_exact() {
    use DispatchAdmissionProviderPolicyAxisError::ProviderPolicyEligibilityIdLengthOutOfRange;
    for length in [0, 1, 200, 201] {
        let reference = "🦀".repeat(length);
        let record = DispatchAdmissionProviderPolicyAxisResult::try_new(
            DispatchAdmissionAxisResult::NotApplicable,
            Some(reference.clone()),
            None,
            None,
        );
        if (1..=200).contains(&length) {
            assert_eq!(
                record.unwrap().provider_policy_eligibility_id(),
                Some(reference.as_str())
            );
        } else {
            assert_eq!(
                record,
                Err(ProviderPolicyEligibilityIdLengthOutOfRange {
                    character_count: length
                })
            );
        }
    }
    for reference in [" ", " e\u{301} É\0 "] {
        for context in [
            None,
            Some(String::new()),
            Some(" Context/e\u{301}\0 ".repeat(201)),
        ] {
            let record = DispatchAdmissionProviderPolicyAxisResult::try_new(
                DispatchAdmissionAxisResult::Fail,
                Some(reference.into()),
                None,
                context.clone(),
            )
            .unwrap();
            assert_eq!(record.provider_policy_eligibility_id(), Some(reference));
            assert_eq!(record.execution_context(), context.as_deref());
        }
    }
}
