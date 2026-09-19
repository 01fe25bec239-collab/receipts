use super::*;
use crate::orchestration::OrchestrationDateTimeV1;

const SPELLINGS: [&str; 3] = [
    "2026-09-08T00:00:00.100Z",
    "2026-09-08t00:00:00.100z",
    "2026-09-08T00:00:00.1Z",
];

fn core(
    outcome: DispatchAdmissionOutcome,
    failing_axis: DispatchAdmissionFailingAxis,
) -> DispatchAdmissionDecisionCore {
    DispatchAdmissionDecisionCore::try_new(
        " decision é ".into(),
        " node e\u{301} ".into(),
        Some(" graph ".into()),
        Some(String::new()),
        outcome,
        failing_axis,
        Some(DispatchAdmissionDenialReason::ProviderPolicyUnknown),
        Some(" Provider/未知\0 ".into()),
        Some(" Runtime/e\u{301} ".into()),
    )
    .unwrap()
}

fn all_pass() -> DispatchAdmissionAxisResults {
    use DispatchAdmissionAxisResult::Pass;
    let reference = Some(" Reference/e\u{301} 未知 ".to_owned());
    DispatchAdmissionAxisResults {
        entitlement: DispatchAdmissionEntitlementAxisResult::try_new(Pass, reference.clone())
            .unwrap(),
        provider_auth: DispatchAdmissionProviderAuthAxisResult::new(
            Pass,
            Some(" Provider/未知\0 ".into()),
            Some("CONNECTED".into()),
        ),
        provider_policy: DispatchAdmissionProviderPolicyAxisResult::try_new(
            Pass,
            reference.clone(),
            Some(DispatchAdmissionProviderPolicyStatus::Unknown),
            Some(" Context/e\u{301}\0 ".into()),
        )
        .unwrap(),
        provider_availability: DispatchAdmissionProviderAvailabilityAxisResult::try_new(
            Pass,
            reference.clone(),
        )
        .unwrap(),
        safety: DispatchAdmissionSafetyAxisResult::try_new(Pass, reference.clone()).unwrap(),
        quality_floor: DispatchAdmissionQualityFloorAxisResult::try_new(Pass, reference).unwrap(),
    }
}

#[test]
fn required_temporal_composition_preserves_values_and_is_deterministic() {
    let core = DispatchAdmissionDecisionNonTemporalCore::try_new(
        core(
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
        ),
        all_pass(),
    )
    .unwrap();
    let decided_at = OrchestrationDateTimeV1::try_new(SPELLINGS[1]).unwrap();
    let construct: fn(
        DispatchAdmissionDecisionNonTemporalCore,
        OrchestrationDateTimeV1,
    ) -> DispatchAdmissionDecision = DispatchAdmissionDecision::new;
    let decision = construct(core.clone(), decided_at.clone());
    let retained_core: &DispatchAdmissionDecisionNonTemporalCore = decision.core();
    let retained_time: &OrchestrationDateTimeV1 = decision.decided_at();
    assert_eq!(retained_core, &core);
    assert_eq!(retained_time, &decided_at);
    assert_eq!(retained_time.as_str(), SPELLINGS[1]);
    assert_eq!(decision, construct(core, decided_at));
}

#[test]
fn lexical_forms_remain_distinct_and_allow_is_independent_of_decided_at() {
    let axes = all_pass();
    let core = DispatchAdmissionDecisionNonTemporalCore::try_new(
        core(
            DispatchAdmissionOutcome::Allow,
            DispatchAdmissionFailingAxis::None,
        ),
        axes.clone(),
    )
    .unwrap();
    let decisions = SPELLINGS.map(|text| {
        let decision = DispatchAdmissionDecision::new(
            core.clone(),
            OrchestrationDateTimeV1::try_new(text).unwrap(),
        );
        assert_eq!(decision.decided_at().as_str(), text);
        assert_eq!(decision.core(), &core);
        assert_eq!(decision.core().axis_results(), &axes);
        assert_eq!(
            decision.core().core().outcome(),
            DispatchAdmissionOutcome::Allow
        );
        assert_eq!(
            decision.core().core().failing_axis(),
            DispatchAdmissionFailingAxis::None
        );
        decision
    });
    assert_ne!(decisions[0], decisions[1]);
    assert_ne!(decisions[0], decisions[2]);
    assert_ne!(decisions[1], decisions[2]);
}

#[test]
fn invalid_allow_is_rejected_by_the_underlying_core() {
    for result in [
        DispatchAdmissionAxisResult::Fail,
        DispatchAdmissionAxisResult::NotApplicable,
    ] {
        let mut axes = all_pass();
        axes.safety = DispatchAdmissionSafetyAxisResult::try_new(result, None).unwrap();
        assert_eq!(
            DispatchAdmissionDecisionNonTemporalCore::try_new(
                core(
                    DispatchAdmissionOutcome::Allow,
                    DispatchAdmissionFailingAxis::None
                ),
                axes,
            ),
            Err(DispatchAdmissionDecisionNonTemporalCoreError::AllowRequiresAllSixPass)
        );
    }
}

#[test]
fn deny_and_policy_evidence_are_independent_of_decided_at() {
    for status in [
        DispatchAdmissionProviderPolicyStatus::Unknown,
        DispatchAdmissionProviderPolicyStatus::NeedsReview,
    ] {
        let mut axes = all_pass();
        axes.provider_policy = DispatchAdmissionProviderPolicyAxisResult::try_new(
            DispatchAdmissionAxisResult::Fail,
            Some(" policy é ".into()),
            Some(status),
            Some(" Context/e\u{301}\0 ".into()),
        )
        .unwrap();
        axes.entitlement = DispatchAdmissionEntitlementAxisResult::try_new(
            DispatchAdmissionAxisResult::Fail,
            None,
        )
        .unwrap();
        // Multiple failures retain the caller's selected axis, with no precedence.
        let core = DispatchAdmissionDecisionNonTemporalCore::try_new(
            core(
                DispatchAdmissionOutcome::Deny,
                DispatchAdmissionFailingAxis::ProviderPolicy,
            ),
            axes.clone(),
        )
        .unwrap();
        for text in [SPELLINGS[1], "2000-01-01T12:34:56+05:30"] {
            let decision = DispatchAdmissionDecision::new(
                core.clone(),
                OrchestrationDateTimeV1::try_new(text).unwrap(),
            );
            assert_eq!(decision.decided_at().as_str(), text);
            assert_eq!(decision.core(), &core);
            assert_eq!(decision.core().axis_results(), &axes);
            assert_eq!(
                decision.core().core().outcome(),
                DispatchAdmissionOutcome::Deny
            );
            assert_eq!(
                decision.core().core().failing_axis(),
                DispatchAdmissionFailingAxis::ProviderPolicy
            );
            let policy = &decision.core().axis_results().provider_policy;
            assert_eq!(policy, &axes.provider_policy);
            assert_eq!(policy.policy_status(), Some(status));
            assert_eq!(policy.result(), DispatchAdmissionAxisResult::Fail);
        }
    }
}

#[test]
fn invalid_deny_is_rejected_by_the_underlying_core() {
    let mut axes = all_pass();
    axes.entitlement =
        DispatchAdmissionEntitlementAxisResult::try_new(DispatchAdmissionAxisResult::Fail, None)
            .unwrap();
    assert_eq!(
        DispatchAdmissionDecisionNonTemporalCore::try_new(
            core(
                DispatchAdmissionOutcome::Deny,
                DispatchAdmissionFailingAxis::ProviderPolicy
            ),
            axes,
        ),
        Err(
            DispatchAdmissionDecisionNonTemporalCoreError::FailingAxisMustReferenceFail {
                failing_axis: DispatchAdmissionFailingAxis::ProviderPolicy,
            }
        )
    );
}
