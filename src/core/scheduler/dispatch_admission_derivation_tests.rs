use super::dispatch_admission_derivation::*;
use super::{
    DispatchAdmissionAxisResult as Res, DispatchAdmissionDenialReason as Reason,
    DispatchAdmissionFailingAxis as Axis, DispatchAdmissionOutcome as Outcome,
    DispatchAdmissionProviderPolicyStatus as Policy,
};
use crate::orchestration::FeatureAdmissionOutcome as Feature;

fn binding() -> AdmissionBindingV1 {
    AdmissionBindingV1 {
        node_id: "n-a3".into(),
        graph_id: Some("G-PRO".into()),
        context_epoch: "7".into(),
    }
}
const F: EvidenceFreshnessV1 = EvidenceFreshnessV1::Fresh;
const S: EvidenceFreshnessV1 = EvidenceFreshnessV1::Stale;

/// All six axes PASS (frozen fixture E's shape).
fn passing() -> DispatchAdmissionRequestV1 {
    DispatchAdmissionRequestV1 {
        decision_id: "da-3".into(),
        capability_id: Some("orchestration.multi_runtime".into()),
        binding: binding(),
        selected_provider_id: Some("openai".into()),
        entitlement: Some(EntitlementEvidenceV1 {
            binding: binding(),
            freshness: F,
            feature_admission_decision_id: "fa-ok".into(),
            outcome: Feature::Allow,
        }),
        provider_auth: Some(ProviderAuthEvidenceV1 {
            binding: binding(),
            freshness: F,
            provider_id: "openai".into(),
            connection: ProviderConnectionV1::Authenticated,
            technical_status: "CONNECTED".into(),
        }),
        provider_policy: Some(ProviderPolicyEvidenceV1 {
            binding: binding(),
            freshness: F,
            provider_policy_eligibility_id: "ppe-3".into(),
            provider_id: "openai".into(),
            status: Policy::VerifiedAllowed,
            execution_context: "PROGRAMMATIC_WORKER".into(),
        }),
        provider_availability: Some(AvailabilityEvidenceV1 {
            binding: binding(),
            freshness: F,
            availability_state_ref: "as-1".into(),
            observation: AvailabilityObservationV1::Available,
            runtime_id: Some("codex-cli".into()),
        }),
        safety: Some(SafetyEvidenceV1 {
            binding: binding(),
            freshness: F,
            state: SafetyStateV1::Clear,
            safety_interruption_id: None,
        }),
        quality_floor: Some(QualityFloorEvidenceV1 {
            binding: binding(),
            freshness: F,
            routing_decision_id: "rd-1".into(),
            result: QualityFloorResultV1::Satisfied,
        }),
    }
}

/// One way to make each axis fail, with its expected reason.
fn break_axis(axis: Axis, r: &mut DispatchAdmissionRequestV1) -> Reason {
    match axis {
        Axis::Safety => {
            let s = r.safety.as_mut().unwrap();
            s.state = SafetyStateV1::Pending;
            s.safety_interruption_id = Some("si-1".into());
            Reason::SafetyCheckPending
        }
        Axis::Entitlement => {
            r.entitlement.as_mut().unwrap().outcome = Feature::LockedRequiresPro;
            Reason::LockedRequiresPro
        }
        Axis::ProviderPolicy => {
            r.provider_policy.as_mut().unwrap().status = Policy::VerifiedDisallowed;
            Reason::ProviderPolicyDisallowed
        }
        Axis::ProviderAuth => {
            r.provider_auth.as_mut().unwrap().connection = ProviderConnectionV1::Unauthenticated;
            Reason::AuthRequired
        }
        Axis::ProviderAvailability => {
            r.provider_availability.as_mut().unwrap().observation =
                AvailabilityObservationV1::RateLimited;
            Reason::ProviderRateLimited
        }
        Axis::QualityFloor => {
            r.quality_floor.as_mut().unwrap().result = QualityFloorResultV1::Unsatisfied;
            Reason::QualityFloorUnsatisfied
        }
        Axis::None => unreachable!(),
    }
}

fn derive(r: &DispatchAdmissionRequestV1) -> DispatchAdmissionDerivationV1 {
    derive_dispatch_admission(r).unwrap()
}

fn denied(r: &DispatchAdmissionRequestV1) -> (Axis, Option<Reason>) {
    let d = derive(r);
    assert!(!d.dispatch_permitted(), "every denial means zero dispatch");
    let core = d.decision().core();
    assert_eq!(core.outcome(), Outcome::Deny);
    assert_eq!(core.selected_provider(), None);
    assert_eq!(core.selected_runtime(), None);
    (core.failing_axis(), core.denial_reason())
}

fn axis_result(d: &DispatchAdmissionDerivationV1, axis: Axis) -> Res {
    let a = d.decision().axis_results();
    match axis {
        Axis::Entitlement => a.entitlement.result(),
        Axis::ProviderAuth => a.provider_auth.result(),
        Axis::ProviderPolicy => a.provider_policy.result(),
        Axis::ProviderAvailability => a.provider_availability.result(),
        Axis::Safety => a.safety.result(),
        Axis::QualityFloor => a.quality_floor.result(),
        Axis::None => unreachable!(),
    }
}

#[test]
fn all_six_pass_allows_and_names_selected_provider_and_runtime() {
    let d = derive(&passing());
    assert!(d.dispatch_permitted());
    let core = d.decision().core();
    assert_eq!(core.outcome(), Outcome::Allow);
    assert_eq!(core.failing_axis(), Axis::None);
    assert_eq!(core.denial_reason(), Some(Reason::None));
    assert_eq!(core.selected_provider(), Some("openai"));
    assert_eq!(core.selected_runtime(), Some("codex-cli"));
    assert!(d.failures().is_empty());
    for axis in PRIMARY_FAILURE_PRECEDENCE {
        assert_eq!(axis_result(&d, axis), Res::Pass);
    }
}

#[test]
fn every_single_axis_failure_names_its_axis_and_reason() {
    for axis in PRIMARY_FAILURE_PRECEDENCE {
        let mut r = passing();
        let reason = break_axis(axis, &mut r);
        assert_eq!(denied(&r), (axis, Some(reason)), "{axis:?}");
        let d = derive(&r);
        assert_eq!(d.failures(), &[(axis, reason)]);
        for other in PRIMARY_FAILURE_PRECEDENCE {
            let expected = if other == axis { Res::Fail } else { Res::Pass };
            assert_eq!(axis_result(&d, other), expected);
        }
    }
}

#[test]
fn every_reason_maps_to_its_adopted_axis() {
    type Case = (fn(&mut DispatchAdmissionRequestV1), Axis, Reason);
    let cases: [Case; 13] = [
        (
            |r| r.entitlement.as_mut().unwrap().outcome = Feature::LockedRequiresPro,
            Axis::Entitlement,
            Reason::LockedRequiresPro,
        ),
        (
            |r| r.entitlement.as_mut().unwrap().outcome = Feature::EntitlementUnknown,
            Axis::Entitlement,
            Reason::EntitlementUnknown,
        ),
        (
            |r| r.entitlement.as_mut().unwrap().outcome = Feature::EntitlementExpired,
            Axis::Entitlement,
            Reason::EntitlementExpired,
        ),
        (
            |r| r.provider_auth.as_mut().unwrap().connection = ProviderConnectionV1::Unknown,
            Axis::ProviderAuth,
            Reason::AuthRequired,
        ),
        (
            |r| r.provider_policy.as_mut().unwrap().status = Policy::VerifiedDisallowed,
            Axis::ProviderPolicy,
            Reason::ProviderPolicyDisallowed,
        ),
        (
            |r| r.provider_policy.as_mut().unwrap().status = Policy::NeedsReview,
            Axis::ProviderPolicy,
            Reason::ProviderPolicyUnknown,
        ),
        (
            |r| r.provider_policy.as_mut().unwrap().status = Policy::Unknown,
            Axis::ProviderPolicy,
            Reason::ProviderPolicyUnknown,
        ),
        (
            |r| {
                r.provider_availability.as_mut().unwrap().observation =
                    AvailabilityObservationV1::RateLimited
            },
            Axis::ProviderAvailability,
            Reason::ProviderRateLimited,
        ),
        (
            |r| {
                r.provider_availability.as_mut().unwrap().observation =
                    AvailabilityObservationV1::ProvenDown
            },
            Axis::ProviderAvailability,
            Reason::ProviderDown,
        ),
        (
            |r| {
                r.provider_availability.as_mut().unwrap().observation =
                    AvailabilityObservationV1::Unknown
            },
            Axis::ProviderAvailability,
            Reason::NoEligibleRuntime,
        ),
        (
            |r| {
                let s = r.safety.as_mut().unwrap();
                s.state = SafetyStateV1::Blocked;
                s.safety_interruption_id = Some("si".into());
            },
            Axis::Safety,
            Reason::PolicyBlocked,
        ),
        (
            |r| {
                let s = r.safety.as_mut().unwrap();
                s.state = SafetyStateV1::HumanRequired;
                s.safety_interruption_id = Some("si".into());
            },
            Axis::Safety,
            Reason::HumanRequired,
        ),
        (
            |r| r.quality_floor.as_mut().unwrap().result = QualityFloorResultV1::Unsatisfied,
            Axis::QualityFloor,
            Reason::QualityFloorUnsatisfied,
        ),
    ];
    for (mutate, axis, reason) in cases {
        let mut r = passing();
        mutate(&mut r);
        assert_eq!(denied(&r), (axis, Some(reason)), "{reason:?}");
    }
    // SAFETY_CHECK_PENDING is covered by break_axis; NO_ELIGIBLE_RUNTIME also
    // covers an available observation with no admitted runtime.
    let mut r = passing();
    r.provider_availability.as_mut().unwrap().runtime_id = None;
    assert_eq!(
        denied(&r),
        (Axis::ProviderAvailability, Some(Reason::NoEligibleRuntime))
    );
}

#[test]
fn all_fifteen_pairs_resolve_by_explicit_precedence_and_keep_both_failures() {
    let mut pairs = 0;
    for (i, &high) in PRIMARY_FAILURE_PRECEDENCE.iter().enumerate() {
        for &low in &PRIMARY_FAILURE_PRECEDENCE[i + 1..] {
            for order in [[high, low], [low, high]] {
                let mut r = passing();
                let reasons: Vec<_> = order.iter().map(|&a| (a, break_axis(a, &mut r))).collect();
                let high_reason = reasons.iter().find(|(a, _)| *a == high).unwrap().1;
                let low_reason = reasons.iter().find(|(a, _)| *a == low).unwrap().1;
                assert_eq!(denied(&r), (high, Some(high_reason)));
                assert_eq!(
                    derive(&r).failures(),
                    &[(high, high_reason), (low, low_reason)]
                );
            }
            pairs += 1;
        }
    }
    assert_eq!(pairs, 15);
}

#[test]
fn all_fail_reports_safety_first_and_records_every_axis() {
    let mut r = passing();
    for axis in PRIMARY_FAILURE_PRECEDENCE {
        break_axis(axis, &mut r);
    }
    assert_eq!(denied(&r), (Axis::Safety, Some(Reason::SafetyCheckPending)));
    let d = derive(&r);
    let axes: Vec<_> = d.failures().iter().map(|(a, _)| *a).collect();
    assert_eq!(axes, PRIMARY_FAILURE_PRECEDENCE);
    for axis in PRIMARY_FAILURE_PRECEDENCE {
        assert_eq!(axis_result(&d, axis), Res::Fail);
    }
}

#[test]
fn missing_evidence_never_passes_and_fails_conservatively() {
    let expected = [
        (Axis::Safety, Reason::SafetyCheckPending),
        (Axis::Entitlement, Reason::EntitlementUnknown),
        (Axis::ProviderPolicy, Reason::ProviderPolicyUnknown),
        (Axis::ProviderAuth, Reason::AuthRequired),
        (Axis::ProviderAvailability, Reason::NoEligibleRuntime),
        (Axis::QualityFloor, Reason::QualityFloorUnsatisfied),
    ];
    for (axis, reason) in expected {
        let mut r = passing();
        match axis {
            Axis::Safety => r.safety = None,
            Axis::Entitlement => r.entitlement = None,
            Axis::ProviderPolicy => r.provider_policy = None,
            Axis::ProviderAuth => {
                r.provider_auth = None;
            }
            Axis::ProviderAvailability => r.provider_availability = None,
            Axis::QualityFloor => r.quality_floor = None,
            Axis::None => unreachable!(),
        }
        // Each missing input fails only its own axis; in particular missing
        // authentication is AUTH_REQUIRED, not invented policy uncertainty.
        assert_eq!(denied(&r), (axis, Some(reason)), "{axis:?}");
        assert_eq!(derive(&r).failures(), &[(axis, reason)]);
    }
}

#[test]
fn missing_auth_with_valid_selected_provider_policy_is_auth_required() {
    let mut r = passing();
    r.provider_auth = None;
    let d = derive(&r);
    assert_eq!(axis_result(&d, Axis::ProviderPolicy), Res::Pass);
    assert_eq!(denied(&r), (Axis::ProviderAuth, Some(Reason::AuthRequired)));
    // Unknown connection state is the same conservative auth failure.
    let mut r = passing();
    r.provider_auth.as_mut().unwrap().connection = ProviderConnectionV1::Unknown;
    assert_eq!(denied(&r), (Axis::ProviderAuth, Some(Reason::AuthRequired)));
}

#[test]
fn provider_mismatch_and_missing_selection_never_grant_permission() {
    // Policy about another provider than the selected one.
    let mut r = passing();
    r.provider_policy.as_mut().unwrap().provider_id = "anthropic".into();
    assert_eq!(
        denied(&r),
        (Axis::ProviderPolicy, Some(Reason::ProviderPolicyUnknown))
    );
    // Authentication of another provider.
    let mut r = passing();
    r.provider_auth.as_mut().unwrap().provider_id = "anthropic".into();
    assert_eq!(denied(&r), (Axis::ProviderAuth, Some(Reason::AuthRequired)));
    // No selected provider: neither policy nor auth can pass.
    let mut r = passing();
    r.selected_provider_id = None;
    let d = derive(&r);
    assert_eq!(axis_result(&d, Axis::ProviderPolicy), Res::Fail);
    assert_eq!(axis_result(&d, Axis::ProviderAuth), Res::Fail);
    assert_eq!(
        denied(&r),
        (Axis::ProviderPolicy, Some(Reason::ProviderPolicyUnknown))
    );
}

#[test]
fn only_fresh_availability_asserts_a_current_limit_or_outage() {
    use AvailabilityObservationV1 as A;
    let cases = [
        (F, A::Available, None),
        (S, A::Available, Some(Reason::NoEligibleRuntime)),
        (F, A::RateLimited, Some(Reason::ProviderRateLimited)),
        (S, A::RateLimited, Some(Reason::NoEligibleRuntime)),
        (F, A::ProvenDown, Some(Reason::ProviderDown)),
        (S, A::ProvenDown, Some(Reason::NoEligibleRuntime)),
        (F, A::Unknown, Some(Reason::NoEligibleRuntime)),
        (S, A::Unknown, Some(Reason::NoEligibleRuntime)),
    ];
    for (freshness, observation, expected) in cases {
        let mut r = passing();
        let a = r.provider_availability.as_mut().unwrap();
        a.freshness = freshness;
        a.observation = observation;
        let d = derive(&r);
        match expected {
            None => assert!(d.dispatch_permitted()),
            Some(reason) => assert_eq!(
                denied(&r),
                (Axis::ProviderAvailability, Some(reason)),
                "{freshness:?} {observation:?}"
            ),
        }
    }
    // Precedence pairs with stale outage: higher axes still win, and the
    // availability failure is recorded truthfully as NO_ELIGIBLE_RUNTIME.
    for higher in [Axis::ProviderAuth, Axis::ProviderPolicy, Axis::Safety] {
        let mut r = passing();
        let primary = break_axis(higher, &mut r);
        let a = r.provider_availability.as_mut().unwrap();
        a.freshness = S;
        a.observation = A::ProvenDown;
        assert_eq!(denied(&r), (higher, Some(primary)));
        assert!(
            derive(&r)
                .failures()
                .contains(&(Axis::ProviderAvailability, Reason::NoEligibleRuntime))
        );
    }
    // Missing auth outranks a fresh outage; the outage stays recorded.
    let mut r = passing();
    r.provider_auth = None;
    r.provider_availability.as_mut().unwrap().observation = A::ProvenDown;
    assert_eq!(denied(&r), (Axis::ProviderAuth, Some(Reason::AuthRequired)));
    assert!(
        derive(&r)
            .failures()
            .contains(&(Axis::ProviderAvailability, Reason::ProviderDown))
    );
}

#[test]
fn stale_evidence_fails_its_axis_and_unknown_is_never_down() {
    let cases = [
        (Axis::Safety, Reason::SafetyCheckPending),
        (Axis::Entitlement, Reason::EntitlementUnknown),
        (Axis::ProviderPolicy, Reason::ProviderPolicyUnknown),
        (Axis::ProviderAuth, Reason::AuthRequired),
        (Axis::ProviderAvailability, Reason::NoEligibleRuntime),
        (Axis::QualityFloor, Reason::QualityFloorUnsatisfied),
    ];
    for (axis, reason) in cases {
        let mut r = passing();
        match axis {
            Axis::Safety => r.safety.as_mut().unwrap().freshness = S,
            Axis::Entitlement => r.entitlement.as_mut().unwrap().freshness = S,
            Axis::ProviderPolicy => r.provider_policy.as_mut().unwrap().freshness = S,
            Axis::ProviderAuth => r.provider_auth.as_mut().unwrap().freshness = S,
            Axis::ProviderAvailability => r.provider_availability.as_mut().unwrap().freshness = S,
            Axis::QualityFloor => r.quality_floor.as_mut().unwrap().freshness = S,
            Axis::None => unreachable!(),
        }
        assert_eq!(denied(&r), (axis, Some(reason)), "{axis:?}");
    }
    // A stale but known disallow or outage keeps its specific reason.
    let mut r = passing();
    let p = r.provider_policy.as_mut().unwrap();
    p.status = Policy::VerifiedDisallowed;
    p.freshness = S;
    assert_eq!(
        denied(&r),
        (Axis::ProviderPolicy, Some(Reason::ProviderPolicyDisallowed))
    );
}

#[test]
fn policy_permission_for_another_provider_is_unknown_not_allowed() {
    let mut r = passing();
    r.provider_policy.as_mut().unwrap().provider_id = "anthropic".into();
    assert_eq!(
        denied(&r),
        (Axis::ProviderPolicy, Some(Reason::ProviderPolicyUnknown))
    );
}

#[test]
fn wrong_binding_or_contradictory_safety_is_a_typed_pre_dispatch_refusal() {
    for axis in PRIMARY_FAILURE_PRECEDENCE {
        for field in 0..3 {
            let mut r = passing();
            let b = match axis {
                Axis::Safety => &mut r.safety.as_mut().unwrap().binding,
                Axis::Entitlement => &mut r.entitlement.as_mut().unwrap().binding,
                Axis::ProviderPolicy => &mut r.provider_policy.as_mut().unwrap().binding,
                Axis::ProviderAuth => &mut r.provider_auth.as_mut().unwrap().binding,
                Axis::ProviderAvailability => {
                    &mut r.provider_availability.as_mut().unwrap().binding
                }
                Axis::QualityFloor => &mut r.quality_floor.as_mut().unwrap().binding,
                Axis::None => unreachable!(),
            };
            match field {
                0 => b.node_id = "other-node".into(),
                1 => b.graph_id = None,
                _ => b.context_epoch = "6".into(),
            }
            assert_eq!(
                derive_dispatch_admission(&r),
                Err(DispatchAdmissionRefusalV1::EvidenceBindingMismatch { axis })
            );
        }
    }
    let mut r = passing();
    r.safety.as_mut().unwrap().safety_interruption_id = Some("si".into());
    assert_eq!(
        derive_dispatch_admission(&r),
        Err(DispatchAdmissionRefusalV1::ContradictorySafetyState)
    );
    let mut r = passing();
    r.safety.as_mut().unwrap().state = SafetyStateV1::Blocked;
    assert_eq!(
        derive_dispatch_admission(&r),
        Err(DispatchAdmissionRefusalV1::ContradictorySafetyState)
    );
    let mut r = passing();
    r.decision_id = String::new();
    assert!(matches!(
        derive_dispatch_admission(&r),
        Err(DispatchAdmissionRefusalV1::InvalidIdentity(_))
    ));
}

#[test]
fn frozen_fixtures_c_d_e_reproduce_their_outcome_axis_and_reason() {
    let fixture = |name: &str| -> serde_json::Value {
        let path = format!(
            "{}/../../fixtures/admission/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    };
    let check = |r: &DispatchAdmissionRequestV1, v: &serde_json::Value| {
        let d = derive(r);
        let core = d.decision().core();
        assert_eq!(core.outcome().as_str(), v["outcome"]);
        assert_eq!(core.failing_axis().as_str(), v["failing_axis"]);
        assert_eq!(core.denial_reason().unwrap().as_str(), v["denial_reason"]);
        assert_eq!(core.selected_provider(), v["selected_provider"].as_str());
        assert_eq!(core.selected_runtime(), v["selected_runtime"].as_str());
    };
    check(&passing(), &fixture("E_openai_user_api_allowed.json"));

    let mut c = passing();
    c.selected_provider_id = Some("anthropic".into());
    c.provider_auth.as_mut().unwrap().provider_id = "anthropic".into();
    let p = c.provider_policy.as_mut().unwrap();
    p.provider_id = "anthropic".into();
    p.status = Policy::VerifiedDisallowed;
    check(&c, &fixture("C_anthropic_subscription_worker_denied.json"));

    let mut d = passing();
    d.provider_policy.as_mut().unwrap().status = Policy::NeedsReview;
    check(&d, &fixture("D_openai_subscription_worker_unresolved.json"));
}
