use crate::CandidateEligibilityRejection::*;
use crate::intelligence::*;
use crate::policy_eligibility::*;
use crate::registry::*;
use crate::*;

fn p(value: &str) -> ProviderId {
    ProviderId::try_new(value.into()).unwrap()
}
fn m(value: &str) -> ModelId {
    ModelId::try_new(value.into()).unwrap()
}
fn r(value: &str) -> RuntimeId {
    RuntimeId::try_new(value.into()).unwrap()
}
fn c(value: &str) -> CapabilityId {
    CapabilityId::try_new(value.into()).unwrap()
}
fn timestamp(value: &str) -> ModelRoutingDateTimeV1 {
    ModelRoutingDateTimeV1::try_new(value.into()).unwrap()
}
fn observation(confidence: EvidenceConfidence) -> Observation {
    Observation {
        confidence,
        source_ref: Some(
            EvidenceSourceRef::try_new(
                EvidenceSourceRefType::ArtifactId,
                "synthetic-evidence".into(),
                None,
                None,
            )
            .unwrap(),
        ),
        observed_at: timestamp("2026-09-08T00:00:00Z"),
    }
}
fn pair(provider: &str) -> CapabilitySubject {
    CapabilitySubject::ModelRuntimePair {
        provider_id: p(provider),
        model_id: m("model-a"),
        runtime_id: r("runtime-a"),
    }
}
fn evidence(
    subject: CapabilitySubject,
    capability: &str,
    confidence: EvidenceConfidence,
    value: Option<CapabilityValue>,
) -> CapabilityEvidence {
    CapabilityEvidence {
        subject,
        capability: c(capability),
        value,
        observation: observation(confidence),
        sample_size: None,
    }
}
fn insert_candidate(service: &mut ModelIntelligenceService, provider: &str, model: &str) {
    service
        .discover_model(
            p(provider),
            m(model),
            observation(EvidenceConfidence::Unverified),
        )
        .unwrap();
    service
        .associate_runtime(
            p(provider),
            m(model),
            r("runtime-a"),
            observation(EvidenceConfidence::Unverified),
        )
        .unwrap();
}
fn service(provider: &str, target: LifecycleState) -> ModelIntelligenceService {
    let mut service = ModelIntelligenceService::new();
    service
        .insert_provider(p(provider), observation(EvidenceConfidence::Unverified))
        .unwrap();
    service
        .insert_runtime(
            p(provider),
            r("runtime-a"),
            observation(EvidenceConfidence::Unverified),
        )
        .unwrap();
    insert_candidate(&mut service, provider, "model-a");
    for (subject, confidence) in [
        (
            CapabilitySubject::Model {
                provider_id: p(provider),
                model_id: m("model-a"),
            },
            EvidenceConfidence::OfficialVerified,
        ),
        (pair(provider), EvidenceConfidence::OfficialVerified),
        (pair(provider), EvidenceConfidence::LocalEmpirical),
    ] {
        service
            .record_capability(evidence(
                subject,
                "coding",
                confidence,
                Some(CapabilityValue::Boolean(true)),
            ))
            .unwrap();
    }
    reach(&mut service, provider, target);
    service
}
fn reach(service: &mut ModelIntelligenceService, provider: &str, target: LifecycleState) {
    if target == LifecycleState::Discovered {
        return;
    }
    let transitions = [
        (
            LifecycleState::Unassessed,
            LifecycleEvidence::ReadyForAssessment,
        ),
        (
            LifecycleState::CapabilityVerified,
            LifecycleEvidence::CapabilityVerification(VerificationPath {
                runtime_id: r("runtime-a"),
                required_capabilities: vec![c("coding")],
            }),
        ),
        (
            LifecycleState::Calibrating,
            LifecycleEvidence::BoundedCalibrationAdmission { authorized: true },
        ),
        (
            LifecycleState::Routable,
            LifecycleEvidence::RoutablePromotion(RoutablePromotionEvidence::LocalCalibration {
                sufficient_acceptable_observations: true,
            }),
        ),
    ];
    for (next, evidence) in transitions {
        service
            .try_transition(
                provider,
                "model-a",
                LifecycleTransition {
                    target: next,
                    evidence,
                    observation: observation(EvidenceConfidence::LocalEmpirical),
                },
            )
            .unwrap();
        if next == target {
            return;
        }
    }
    let reason = match target {
        LifecycleState::Deprecated => DemotionReason::VendorRetirement,
        LifecycleState::Disabled => DemotionReason::RepeatedRuntimeFailures,
        LifecycleState::Unavailable => DemotionReason::TemporarilyUnreachable,
        LifecycleState::UserBlocked => DemotionReason::UserBlock,
        _ => unreachable!(),
    };
    service
        .try_transition(
            provider,
            "model-a",
            LifecycleTransition {
                target,
                evidence: LifecycleEvidence::Demotion(reason),
                observation: observation(EvidenceConfidence::LocalEmpirical),
            },
        )
        .unwrap();
}
fn request(required: &[&str]) -> RoutingRequestNonTemporalCore {
    RoutingRequestNonTemporalCore::try_new(
        "request-a".into(),
        None,
        RoutingRequestRole::Implementer,
        RoutingTaskClass::FrontierImplementation,
        RoutingQualityFloor::Frontier,
        required.iter().map(|value| String::from(*value)).collect(),
        Some(vec!["missing-preferred".into()]),
        Some(RoutingPriority::Highest),
        Some(RoutingPriority::Low),
        Some(
            RoutingRequestConstraints::try_new(
                Some("provider-a".into()),
                Some(vec!["provider-a".into()]),
                Some("different-model".into()),
                Some("different-provider".into()),
                Some(-1.0),
            )
            .unwrap(),
        ),
        Some(u64::MAX),
    )
    .unwrap()
}
fn availability(
    provider: &str,
    model: Option<&str>,
    runtime: Option<&str>,
    state: AvailabilityStateKind,
) -> AvailabilityStateNonTemporalCore {
    AvailabilityStateNonTemporalCore::try_new(
        provider.into(),
        model.map(String::from),
        runtime.map(String::from),
        state,
        Some(u64::MAX),
        Some(AvailabilitySignalSource::Unknown),
        None,
    )
    .unwrap()
}
fn policy(
    provider: &str,
    runtime: &str,
    status: PolicyStatus,
    technical: TechnicalStatus,
    contexts: Option<&[&str]>,
    deadline: Option<Option<&str>>,
) -> ProviderPolicyEligibility {
    ProviderPolicyEligibility::try_new(
        provider.into(),
        runtime.into(),
        "synthetic-credential".into(),
        technical,
        status,
        contexts.map(|values| values.iter().map(|value| String::from(*value)).collect()),
        timestamp("2026-09-08T00:00:00Z"),
        None,
        PolicyEvidenceLabel::PolicyNeedsReview,
        None,
        None,
        deadline.map(|value| value.map(timestamp)),
        None,
    )
    .unwrap()
}
fn allowed(provider: &str) -> ProviderPolicyEligibility {
    policy(
        provider,
        "runtime-a",
        PolicyStatus::VerifiedAllowed,
        TechnicalStatus::Connected,
        Some(&["worker"]),
        Some(Some("2026-09-09T00:00:00Z")),
    )
}
fn evaluate(
    registry: &Registry,
    request: &RoutingRequestNonTemporalCore,
    availability: &AvailabilityStateNonTemporalCore,
    policy: &ProviderPolicyEligibility,
    context: Option<&str>,
    deadline: Option<bool>,
) -> CandidateEligibilityOutcome {
    evaluate_candidate_eligibility(
        request,
        registry,
        &p("provider-a"),
        &m("model-a"),
        &r("runtime-a"),
        availability,
        policy,
        context,
        deadline,
    )
}
fn ordinary(registry: &Registry, required: &[&str]) -> CandidateEligibilityOutcome {
    evaluate(
        registry,
        &request(required),
        &availability(
            "provider-a",
            Some("model-a"),
            Some("runtime-a"),
            AvailabilityStateKind::Available,
        ),
        &allowed("provider-a"),
        Some("worker"),
        Some(false),
    )
}

#[test]
fn happy_path_available_and_degraded_pass_only_the_bounded_subset_without_mutation() {
    let service = service("provider-a", LifecycleState::Routable);
    let registry = service.registry();
    let request = request(&["coding"]);
    let policy = allowed("provider-a");
    for state in [
        AvailabilityStateKind::Available,
        AvailabilityStateKind::Degraded,
    ] {
        let availability = availability("provider-a", Some("model-a"), Some("runtime-a"), state);
        let before = (
            registry.clone(),
            request.clone(),
            availability.clone(),
            policy.clone(),
        );
        let result = evaluate(
            registry,
            &request,
            &availability,
            &policy,
            Some("worker"),
            Some(false),
        );
        assert!(result.passes_bounded_pre_score_eligibility());
        assert!(result.rejections().is_empty());
        assert_eq!(
            result,
            evaluate(
                registry,
                &request,
                &availability,
                &policy,
                Some("worker"),
                Some(false)
            )
        );
        assert_eq!(
            before,
            (
                registry.clone(),
                request.clone(),
                availability,
                policy.clone()
            )
        );
    }
    // Missing preferred support, conflicting pins/avoidance, floor, priorities,
    // max_cost and context size are intentionally outside this bounded subset.
}

#[test]
fn every_lifecycle_state_uses_the_normal_gate() {
    for state in LifecycleState::ALL {
        let service = service("provider-a", state);
        let result = ordinary(service.registry(), &["coding"]);
        let expected = if state == LifecycleState::Routable {
            vec![]
        } else {
            vec![LifecycleNotNormallyRoutable(state)]
        };
        assert_eq!(result.rejections(), expected, "{state:?}");
        assert_eq!(
            result.passes_bounded_pre_score_eligibility(),
            state == LifecycleState::Routable
        );
    }
}

#[test]
fn required_capabilities_use_exact_registry_evidence_and_preserve_unknown_vs_unsupported() {
    for (confidence, value, sourced, expected) in [
        (
            EvidenceConfidence::OfficialVerified,
            Some(CapabilityValue::Boolean(true)),
            true,
            Compatibility::Confirmed,
        ),
        (
            EvidenceConfidence::OfficialVerified,
            Some(CapabilityValue::Boolean(false)),
            true,
            Compatibility::Unsupported,
        ),
        (
            EvidenceConfidence::OfficialVerified,
            Some(CapabilityValue::Unknown),
            true,
            Compatibility::Unknown,
        ),
        (
            EvidenceConfidence::OfficialVerified,
            None,
            true,
            Compatibility::Unknown,
        ),
        (
            EvidenceConfidence::Unverified,
            Some(CapabilityValue::Boolean(true)),
            true,
            Compatibility::Unknown,
        ),
        (
            EvidenceConfidence::UserDeclared,
            Some(CapabilityValue::Boolean(true)),
            true,
            Compatibility::Unknown,
        ),
        (
            EvidenceConfidence::IndependentVerified,
            Some(CapabilityValue::Boolean(true)),
            true,
            Compatibility::Unknown,
        ),
        (
            EvidenceConfidence::OfficialVerified,
            Some(CapabilityValue::Boolean(true)),
            false,
            Compatibility::Unknown,
        ),
    ] {
        let mut service = service("provider-a", LifecycleState::Routable);
        let mut item = evidence(pair("provider-a"), "future-capability-☃", confidence, value);
        if !sourced {
            item.observation.source_ref = None;
        }
        service.record_capability(item).unwrap();
        assert_eq!(
            service.registry().compatibility(
                &p("provider-a"),
                &m("model-a"),
                &r("runtime-a"),
                &c("future-capability-☃")
            ),
            expected
        );
        let result = ordinary(service.registry(), &["coding", "future-capability-☃"]);
        let expected = match expected {
            Compatibility::Confirmed => vec![],
            Compatibility::Unsupported => {
                vec![RequiredCapabilityUnsupported(c("future-capability-☃"))]
            }
            Compatibility::Unknown => vec![RequiredCapabilityUnknown(c("future-capability-☃"))],
        };
        assert_eq!(result.rejections(), expected);
    }
    let service = service("provider-a", LifecycleState::Routable);
    assert_eq!(
        ordinary(service.registry(), &["coding", "missing"]).rejections(),
        [RequiredCapabilityUnknown(c("missing"))]
    );
}

#[test]
fn capabilities_preserve_whitespace_case_unicode_punctuation_and_request_order() {
    let mut service = service("provider-a", LifecycleState::Routable);
    for cap in [" ", "future-capability-☃", "Case/α:+", "é"] {
        service
            .record_capability(evidence(
                pair("provider-a"),
                cap,
                EvidenceConfidence::OfficialVerified,
                Some(CapabilityValue::Boolean(true)),
            ))
            .unwrap();
    }
    assert!(
        ordinary(
            service.registry(),
            &["coding", " ", "future-capability-☃", "Case/α:+", "é"]
        )
        .passes_bounded_pre_score_eligibility()
    );
    let missing = ["coding ", "Coding", "  ", "case/α:+", "e\u{301}", "coding "];
    assert_eq!(
        ordinary(service.registry(), &missing).rejections(),
        missing.map(|cap| RequiredCapabilityUnknown(c(cap)))
    );
}

#[test]
fn model_or_runtime_evidence_cannot_replace_exact_pair_evidence() {
    let mut service = service("provider-a", LifecycleState::Routable);
    for subject in [
        CapabilitySubject::Model {
            provider_id: p("provider-a"),
            model_id: m("model-a"),
        },
        CapabilitySubject::Runtime {
            provider_id: p("provider-a"),
            runtime_id: r("runtime-a"),
        },
    ] {
        service
            .record_capability(evidence(
                subject,
                "shell",
                EvidenceConfidence::OfficialVerified,
                Some(CapabilityValue::Boolean(true)),
            ))
            .unwrap();
    }
    assert_eq!(
        ordinary(service.registry(), &["shell"]).rejections(),
        [RequiredCapabilityUnknown(c("shell"))]
    );
}

#[test]
fn all_availability_states_preserve_their_exact_rejection_cause() {
    let service = service("provider-a", LifecycleState::Routable);
    for state in AvailabilityStateKind::ALL {
        let result = evaluate(
            service.registry(),
            &request(&["coding"]),
            &availability("provider-a", Some("model-a"), Some("runtime-a"), state),
            &allowed("provider-a"),
            Some("worker"),
            Some(false),
        );
        let expected = match state {
            AvailabilityStateKind::Available | AvailabilityStateKind::Degraded => vec![],
            _ => vec![AvailabilityIneligible(state)],
        };
        assert_eq!(result.rejections(), expected, "{state:?}");
    }
    // SafetyCheckPending and PolicyBlocked are preserved terminal candidate
    // rejections. This API returns no alternatives or recovery actions.
}

#[test]
fn availability_requires_exact_complete_scope_before_trusting_state() {
    let service = service("provider-a", LifecycleState::Routable);
    for (provider, model, runtime, expected) in [
        (
            "Provider-A",
            Some("model-a"),
            Some("runtime-a"),
            AvailabilityIdentityMismatch,
        ),
        (
            "provider-a ",
            Some("model-a"),
            Some("runtime-a"),
            AvailabilityIdentityMismatch,
        ),
        (
            "provider-a",
            Some("model-a "),
            Some("runtime-a"),
            AvailabilityIdentityMismatch,
        ),
        (
            "provider-a",
            Some("model-a"),
            Some("runtime-A"),
            AvailabilityIdentityMismatch,
        ),
        (
            "provider-a",
            None,
            Some("runtime-a"),
            AvailabilityScopeInsufficient,
        ),
        (
            "provider-a",
            Some("model-a"),
            None,
            AvailabilityScopeInsufficient,
        ),
        ("provider-a", None, None, AvailabilityScopeInsufficient),
        ("different", None, None, AvailabilityIdentityMismatch),
        (
            "provider-a",
            None,
            Some("different"),
            AvailabilityIdentityMismatch,
        ),
    ] {
        for state in [
            AvailabilityStateKind::Available,
            AvailabilityStateKind::PolicyBlocked,
        ] {
            assert_eq!(
                evaluate(
                    service.registry(),
                    &request(&["coding"]),
                    &availability(provider, model, runtime, state),
                    &allowed("provider-a"),
                    Some("worker"),
                    Some(false)
                )
                .rejections(),
                std::slice::from_ref(&expected)
            );
        }
    }
    // Schema optionality supplies storage, not inheritance authority. Neither
    // broader positive evidence nor another identity's rejection is applied.
}

#[test]
fn policy_status_is_generic_and_technical_status_is_independent_including_q_v13_04() {
    for provider in ["provider-a", "another-synthetic-provider"] {
        let service = service(provider, LifecycleState::Routable);
        for status in PolicyStatus::ALL {
            for technical in TechnicalStatus::ALL {
                let policy = policy(
                    provider,
                    "runtime-a",
                    status,
                    technical,
                    Some(&["worker"]),
                    Some(Some("2026-09-09T00:00:00Z")),
                );
                let before = policy.clone();
                let result = evaluate_candidate_eligibility(
                    &request(&["coding"]),
                    service.registry(),
                    &p(provider),
                    &m("model-a"),
                    &r("runtime-a"),
                    &availability(
                        provider,
                        Some("model-a"),
                        Some("runtime-a"),
                        AvailabilityStateKind::Available,
                    ),
                    &policy,
                    Some("worker"),
                    Some(false),
                );
                let expected = if status == PolicyStatus::VerifiedAllowed {
                    vec![]
                } else {
                    vec![ProviderPolicyStatusIneligible(status)]
                };
                assert_eq!(
                    result.rejections(),
                    expected,
                    "{provider}, {status:?}, {technical:?}"
                );
                assert_eq!(policy, before);
            }
        }
    }
}

#[test]
fn policy_context_is_exact_open_and_required() {
    let service = service("provider-a", LifecycleState::Routable);
    for (contexts, context, passes) in [
        (Some(vec!["worker"]), Some("worker"), true),
        (Some(vec!["worker"]), Some("Worker"), false),
        (Some(vec!["worker"]), Some(" worker"), false),
        (Some(vec!["worker"]), Some("worker "), false),
        (Some(vec!["worker"]), None, false),
        (None, Some("worker"), false),
        (Some(vec![]), Some("worker"), false),
        (Some(vec!["worker"]), Some(""), false),
        (
            Some(vec!["future-context-☃", " "]),
            Some("future-context-☃"),
            true,
        ),
        (Some(vec!["future-context-☃", " "]), Some(" "), true),
        (Some(vec!["é"]), Some("e\u{301}"), false),
    ] {
        let policy = policy(
            "provider-a",
            "runtime-a",
            PolicyStatus::VerifiedAllowed,
            TechnicalStatus::Connected,
            contexts.as_deref(),
            None,
        );
        let result = evaluate(
            service.registry(),
            &request(&["coding"]),
            &availability(
                "provider-a",
                Some("model-a"),
                Some("runtime-a"),
                AvailabilityStateKind::Available,
            ),
            &policy,
            context,
            None,
        );
        assert_eq!(result.passes_bounded_pre_score_eligibility(), passes);
        assert_eq!(
            result.rejections(),
            if passes {
                vec![]
            } else {
                vec![ExecutionContextNotProvenAllowed]
            }
        );
    }
}

#[test]
fn policy_deadline_uses_only_caller_evidence_and_does_not_mutate_status() {
    let service = service("provider-a", LifecycleState::Routable);
    for deadline in [
        None,
        Some(None),
        Some(Some("0000-01-01T00:00:00-00:00")),
        Some(Some("9999-12-31T23:59:60+23:59")),
    ] {
        for passed in [None, Some(true), Some(false)] {
            let policy = policy(
                "provider-a",
                "runtime-a",
                PolicyStatus::VerifiedAllowed,
                TechnicalStatus::Connected,
                Some(&["worker"]),
                deadline,
            );
            let before = policy.clone();
            let result = evaluate(
                service.registry(),
                &request(&["coding"]),
                &availability(
                    "provider-a",
                    Some("model-a"),
                    Some("runtime-a"),
                    AvailabilityStateKind::Available,
                ),
                &policy,
                Some("worker"),
                passed,
            );
            let expected = match (deadline.flatten(), passed) {
                (Some(_), None) => vec![ReverificationEvidenceMissing],
                (Some(_), Some(true)) => vec![ReverificationDeadlinePassed],
                _ => vec![],
            };
            assert_eq!(result.rejections(), expected);
            assert_eq!(policy, before);
        }
    }
}

#[test]
fn policy_identity_mismatch_suppresses_unrelated_policy_conclusions() {
    let service = service("provider-a", LifecycleState::Routable);
    for (provider, runtime) in [
        ("Provider-A", "runtime-a"),
        ("provider-a ", "runtime-a"),
        ("provider-a", "runtime-A"),
        ("provider-a", "runtime-a "),
        ("other", "other"),
    ] {
        for status in PolicyStatus::ALL {
            let policy = policy(
                provider,
                runtime,
                status,
                TechnicalStatus::Connected,
                None,
                Some(Some("2026-09-09T00:00:00Z")),
            );
            assert_eq!(
                evaluate(
                    service.registry(),
                    &request(&["coding"]),
                    &availability(
                        "provider-a",
                        Some("model-a"),
                        Some("runtime-a"),
                        AvailabilityStateKind::Available
                    ),
                    &policy,
                    None,
                    None
                )
                .rejections(),
                [ProviderPolicyIdentityMismatch]
            );
        }
    }
}

#[test]
fn simultaneous_rejections_have_stable_gate_and_request_order() {
    let mut service = service("provider-a", LifecycleState::Calibrating);
    service
        .record_capability(evidence(
            pair("provider-a"),
            "unsupported",
            EvidenceConfidence::OfficialVerified,
            Some(CapabilityValue::Boolean(false)),
        ))
        .unwrap();
    let policy = policy(
        "provider-a",
        "runtime-a",
        PolicyStatus::NeedsReview,
        TechnicalStatus::Connected,
        None,
        Some(Some("2026-09-09T00:00:00Z")),
    );
    let result = evaluate(
        service.registry(),
        &request(&["z-unknown", "unsupported", "a-unknown"]),
        &availability(
            "provider-a",
            Some("model-a"),
            Some("runtime-a"),
            AvailabilityStateKind::RateLimited,
        ),
        &policy,
        None,
        None,
    );
    assert_eq!(
        result.rejections(),
        [
            LifecycleNotNormallyRoutable(LifecycleState::Calibrating),
            RequiredCapabilityUnknown(c("z-unknown")),
            RequiredCapabilityUnsupported(c("unsupported")),
            RequiredCapabilityUnknown(c("a-unknown")),
            AvailabilityIneligible(AvailabilityStateKind::RateLimited),
            ProviderPolicyStatusIneligible(PolicyStatus::NeedsReview),
        ]
    );
    assert!(!result.passes_bounded_pre_score_eligibility());
}

#[test]
fn context_and_deadline_failures_accumulate_after_allowed_status() {
    let service = service("provider-a", LifecycleState::Routable);
    for (passed, expected) in [
        (None, ReverificationEvidenceMissing),
        (Some(true), ReverificationDeadlinePassed),
    ] {
        assert_eq!(
            evaluate(
                service.registry(),
                &request(&["coding"]),
                &availability(
                    "provider-a",
                    Some("model-a"),
                    Some("runtime-a"),
                    AvailabilityStateKind::Available
                ),
                &allowed("provider-a"),
                None,
                passed
            )
            .rejections(),
            [ExecutionContextNotProvenAllowed, expected]
        );
    }
}

#[test]
fn missing_registry_prerequisites_suppress_only_dependent_checks() {
    let unknown_policy = policy(
        "provider-a",
        "runtime-a",
        PolicyStatus::Unknown,
        TechnicalStatus::Connected,
        None,
        None,
    );
    let unknown_availability = availability(
        "provider-a",
        Some("model-a"),
        Some("runtime-a"),
        AvailabilityStateKind::Unknown,
    );
    assert_eq!(
        evaluate(
            &Registry::default(),
            &request(&["missing"]),
            &unknown_availability,
            &unknown_policy,
            None,
            None
        )
        .rejections(),
        [
            RegistryModelMissing,
            AvailabilityIneligible(AvailabilityStateKind::Unknown),
            ProviderPolicyStatusIneligible(PolicyStatus::Unknown),
        ]
    );
    let mut service = service("provider-a", LifecycleState::Discovered);
    service
        .insert_runtime(
            p("provider-a"),
            r("unassociated"),
            observation(EvidenceConfidence::Unverified),
        )
        .unwrap();
    for runtime in ["unassociated", "absent"] {
        let result = evaluate_candidate_eligibility(
            &request(&["missing"]),
            service.registry(),
            &p("provider-a"),
            &m("model-a"),
            &r(runtime),
            &availability(
                "provider-a",
                Some("model-a"),
                Some(runtime),
                AvailabilityStateKind::Unknown,
            ),
            &policy(
                "provider-a",
                runtime,
                PolicyStatus::Unknown,
                TechnicalStatus::Connected,
                None,
                None,
            ),
            None,
            None,
        );
        assert_eq!(
            result.rejections(),
            [
                RegistryRuntimeAssociationMissing,
                LifecycleNotNormallyRoutable(LifecycleState::Discovered),
                AvailabilityIneligible(AvailabilityStateKind::Unknown),
                ProviderPolicyStatusIneligible(PolicyStatus::Unknown)
            ]
        );
    }
}

#[test]
fn candidate_identity_attacks_cannot_select_nearby_registry_entries() {
    let service = service("provider-a", LifecycleState::Routable);
    for (provider, model, runtime, expected) in [
        ("Provider-A", "model-a", "runtime-a", RegistryModelMissing),
        ("provider-a ", "model-a", "runtime-a", RegistryModelMissing),
        ("provider-a", "model-a ", "runtime-a", RegistryModelMissing),
        ("provider-a", "model", "runtime-a", RegistryModelMissing),
        (
            "provider-a",
            "model-a",
            "runtime-A",
            RegistryRuntimeAssociationMissing,
        ),
        (
            "provider-a",
            "model-a",
            "runtime",
            RegistryRuntimeAssociationMissing,
        ),
    ] {
        let result = evaluate_candidate_eligibility(
            &request(&["coding"]),
            service.registry(),
            &p(provider),
            &m(model),
            &r(runtime),
            &availability(
                provider,
                Some(model),
                Some(runtime),
                AvailabilityStateKind::Available,
            ),
            &policy(
                provider,
                runtime,
                PolicyStatus::VerifiedAllowed,
                TechnicalStatus::Connected,
                Some(&["worker"]),
                None,
            ),
            Some("worker"),
            None,
        );
        assert_eq!(result.rejections(), [expected]);
    }
}

#[test]
fn registry_insertion_and_conflicting_observation_order_do_not_change_outcomes() {
    let mut outcomes = Vec::new();
    for reverse in [false, true] {
        let mut service = ModelIntelligenceService::new();
        let providers = if reverse {
            ["provider-b", "provider-a"]
        } else {
            ["provider-a", "provider-b"]
        };
        let models = if reverse {
            ["model-b", "model-a"]
        } else {
            ["model-a", "model-b"]
        };
        for provider in providers {
            service
                .insert_provider(p(provider), observation(EvidenceConfidence::Unverified))
                .unwrap();
            service
                .insert_runtime(
                    p(provider),
                    r("runtime-a"),
                    observation(EvidenceConfidence::Unverified),
                )
                .unwrap();
            for model in models {
                insert_candidate(&mut service, provider, model);
            }
        }
        let mut assertions = vec![
            evidence(
                CapabilitySubject::Model {
                    provider_id: p("provider-a"),
                    model_id: m("model-a"),
                },
                "coding",
                EvidenceConfidence::OfficialVerified,
                Some(CapabilityValue::Boolean(true)),
            ),
            evidence(
                pair("provider-a"),
                "coding",
                EvidenceConfidence::OfficialVerified,
                Some(CapabilityValue::Boolean(true)),
            ),
            evidence(
                pair("provider-a"),
                "coding",
                EvidenceConfidence::LocalEmpirical,
                Some(CapabilityValue::Boolean(true)),
            ),
            evidence(
                pair("provider-a"),
                "conflict-false",
                EvidenceConfidence::OfficialVerified,
                Some(CapabilityValue::Boolean(true)),
            ),
            evidence(
                pair("provider-a"),
                "conflict-false",
                EvidenceConfidence::OfficialVerified,
                Some(CapabilityValue::Boolean(false)),
            ),
            evidence(
                pair("provider-a"),
                "conflict-unknown",
                EvidenceConfidence::OfficialVerified,
                Some(CapabilityValue::Boolean(true)),
            ),
            evidence(
                pair("provider-a"),
                "conflict-unknown",
                EvidenceConfidence::OfficialVerified,
                Some(CapabilityValue::Unknown),
            ),
        ];
        if reverse {
            assertions.reverse();
        }
        for assertion in assertions {
            service.record_capability(assertion).unwrap();
        }
        reach(&mut service, "provider-a", LifecycleState::Routable);
        let pass = ordinary(service.registry(), &["coding"]);
        assert!(pass.passes_bounded_pre_score_eligibility());
        let rejected = ordinary(
            service.registry(),
            &["conflict-unknown", "conflict-false", "coding"],
        );
        assert_eq!(
            rejected.rejections(),
            [
                RequiredCapabilityUnknown(c("conflict-unknown")),
                RequiredCapabilityUnsupported(c("conflict-false"))
            ]
        );
        outcomes.push((pass, rejected));
    }
    assert_eq!(outcomes[0], outcomes[1]);
}
