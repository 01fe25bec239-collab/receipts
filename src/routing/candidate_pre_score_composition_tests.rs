use crate::CandidateEligibilityRejection::*;
use crate::CandidateIdentityConstraintOutcome::{Rejected, Satisfied};
use crate::CandidateIdentityConstraintRejection::*;
use crate::intelligence::*;
use crate::policy_eligibility::*;
use crate::registry::*;
use crate::*;

const PROVIDER: &str = " Provider-α ";
const MODEL: &str = " Model-β ";
const RUNTIME: &str = " Runtime-γ ";

fn timestamp(value: &str) -> ModelRoutingDateTimeV1 {
    ModelRoutingDateTimeV1::try_new(value.into()).unwrap()
}

fn observation(confidence: EvidenceConfidence) -> Observation {
    Observation {
        confidence,
        source_ref: Some(
            EvidenceSourceRef::try_new(
                EvidenceSourceRefType::ArtifactId,
                "synthetic-pre-score-composition-evidence".into(),
                None,
                None,
            )
            .unwrap(),
        ),
        observed_at: timestamp("2026-09-20T00:00:00Z"),
    }
}

fn capability(value: &str) -> CapabilityId {
    CapabilityId::try_new(value.into()).unwrap()
}

fn request(
    required: &[&str],
    constraints: Option<RoutingRequestConstraints>,
) -> RoutingRequestNonTemporalCore {
    RoutingRequestNonTemporalCore::try_new(
        "request-a".into(),
        None,
        RoutingRequestRole::Implementer,
        RoutingTaskClass::FrontierImplementation,
        RoutingQualityFloor::Frontier,
        required.iter().map(|value| String::from(*value)).collect(),
        None,
        None,
        None,
        constraints,
        None,
    )
    .unwrap()
}

fn constraints(
    distinct: Option<&str>,
    avoid: Option<&str>,
    provider: Option<&str>,
    model: Option<&str>,
    cost: Option<f64>,
) -> RoutingRequestConstraints {
    RoutingRequestConstraints::try_new(
        distinct.map(String::from),
        avoid.map(|value| vec![value.into()]),
        model.map(String::from),
        provider.map(String::from),
        cost,
    )
    .unwrap()
}

fn availability(
    provider: &str,
    model: Option<&str>,
    runtime: Option<&str>,
    state: AvailabilityStateKind,
) -> AvailabilityState {
    AvailabilityState::new(
        AvailabilityStateNonTemporalCore::try_new(
            provider.into(),
            model.map(String::from),
            runtime.map(String::from),
            state,
            None,
            None,
            None,
        )
        .unwrap(),
        timestamp("2026-09-20T00:00:00Z"),
    )
}

fn policy(status: PolicyStatus) -> ProviderPolicyEligibility {
    ProviderPolicyEligibility::try_new(
        PROVIDER.into(),
        RUNTIME.into(),
        "synthetic-credential".into(),
        TechnicalStatus::Connected,
        status,
        Some(vec!["worker".into()]),
        timestamp("2026-09-20T00:00:00Z"),
        None,
        PolicyEvidenceLabel::PolicyNeedsReview,
        None,
        None,
        Some(Some(timestamp("2026-09-21T00:00:00Z"))),
        None,
    )
    .unwrap()
}

#[derive(Debug, Clone, PartialEq)]
struct Fixture {
    candidate: RegistryCandidateIdentity,
    request: RoutingRequestNonTemporalCore,
    registry: Registry,
    availability: AvailabilityState,
    policy: ProviderPolicyEligibility,
    context: Option<String>,
    deadline_passed: Option<bool>,
}

impl Fixture {
    fn new(promote: bool) -> Self {
        let provider = ProviderId::try_new(PROVIDER.into()).unwrap();
        let model = ModelId::try_new(MODEL.into()).unwrap();
        let runtime = RuntimeId::try_new(RUNTIME.into()).unwrap();
        let mut service = ModelIntelligenceService::new();
        let observation = observation(EvidenceConfidence::OfficialVerified);
        service
            .insert_provider(provider.clone(), observation.clone())
            .unwrap();
        service
            .discover_model(provider.clone(), model.clone(), observation.clone())
            .unwrap();
        service
            .insert_runtime(provider.clone(), runtime.clone(), observation.clone())
            .unwrap();
        service
            .associate_runtime(
                provider.clone(),
                model.clone(),
                runtime.clone(),
                observation.clone(),
            )
            .unwrap();
        let pair = CapabilitySubject::ModelRuntimePair {
            provider_id: provider.clone(),
            model_id: model.clone(),
            runtime_id: runtime.clone(),
        };
        for (subject, name, supported, confidence) in [
            (
                CapabilitySubject::Model {
                    provider_id: provider,
                    model_id: model,
                },
                "coding",
                true,
                EvidenceConfidence::OfficialVerified,
            ),
            (
                pair.clone(),
                "coding",
                true,
                EvidenceConfidence::OfficialVerified,
            ),
            (
                pair.clone(),
                "coding",
                true,
                EvidenceConfidence::LocalEmpirical,
            ),
            (
                pair,
                "unsupported",
                false,
                EvidenceConfidence::OfficialVerified,
            ),
        ] {
            service
                .record_capability(CapabilityEvidence {
                    subject,
                    capability: capability(name),
                    value: Some(CapabilityValue::Boolean(supported)),
                    observation: self::observation(confidence),
                    sample_size: None,
                })
                .unwrap();
        }
        if promote {
            for (target, evidence) in [
                (
                    LifecycleState::Unassessed,
                    LifecycleEvidence::ReadyForAssessment,
                ),
                (
                    LifecycleState::CapabilityVerified,
                    LifecycleEvidence::CapabilityVerification(VerificationPath {
                        runtime_id: runtime,
                        required_capabilities: vec![capability("coding")],
                    }),
                ),
                (
                    LifecycleState::Calibrating,
                    LifecycleEvidence::BoundedCalibrationAdmission { authorized: true },
                ),
                (
                    LifecycleState::Routable,
                    LifecycleEvidence::RoutablePromotion(
                        RoutablePromotionEvidence::LocalCalibration {
                            sufficient_acceptable_observations: true,
                        },
                    ),
                ),
            ] {
                service
                    .try_transition(
                        PROVIDER,
                        MODEL,
                        LifecycleTransition {
                            target,
                            evidence,
                            observation: self::observation(EvidenceConfidence::LocalEmpirical),
                        },
                    )
                    .unwrap();
            }
        }
        let mut candidates = enumerate_registry_candidates(service.registry());
        assert_eq!(candidates.len(), 1);
        Self {
            candidate: candidates.pop().unwrap(),
            request: request(&["coding"], None),
            registry: service.registry().clone(),
            availability: availability(
                PROVIDER,
                Some(MODEL),
                Some(RUNTIME),
                AvailabilityStateKind::Available,
            ),
            policy: policy(PolicyStatus::VerifiedAllowed),
            context: Some("worker".into()),
            deadline_passed: Some(false),
        }
    }

    // Every scenario materially compares both results with direct subordinate calls.
    fn evaluate(&self) -> BoundedPreScoreCandidateAssessment {
        let result = evaluate_bounded_pre_score_candidate(
            &self.candidate,
            &self.request,
            &self.registry,
            &self.availability,
            &self.policy,
            self.context.as_deref(),
            self.deadline_passed,
        );
        assert_eq!(result.candidate(), &self.candidate);
        assert_eq!(
            result.identity_constraint_outcome(),
            evaluate_candidate_identity_constraints(&self.candidate, self.request.constraints(),)
        );
        assert_eq!(
            result.bounded_eligibility_outcome(),
            &evaluate_candidate_eligibility(
                &self.request,
                &self.registry,
                self.candidate.provider_id(),
                self.candidate.model_id(),
                self.candidate.runtime_id(),
                self.availability.core(),
                &self.policy,
                self.context.as_deref(),
                self.deadline_passed,
            )
        );
        result
    }

    fn assert_rejections(&self, expected: &[CandidateEligibilityRejection]) {
        assert_eq!(
            self.evaluate().bounded_eligibility_outcome().rejections(),
            expected
        );
    }
}

#[test]
fn t01_exact_candidate_identity_preserved() {
    let fixture = Fixture::new(true);
    let result = fixture.evaluate();
    assert_eq!(result.candidate(), &fixture.candidate);
    assert_eq!(result.candidate().provider_id().as_str(), PROVIDER);
    assert_eq!(result.candidate().model_id().as_str(), MODEL);
    assert_eq!(result.candidate().runtime_id().as_str(), RUNTIME);
}

#[test]
fn t02_satisfied_identity_delegation() {
    let mut fixture = Fixture::new(true);
    for constraints in [
        None,
        Some(constraints(
            Some("other"),
            Some("other"),
            Some(PROVIDER),
            Some(MODEL),
            None,
        )),
    ] {
        fixture.request = request(&["coding"], constraints);
        assert_eq!(fixture.evaluate().identity_constraint_outcome(), Satisfied);
    }
}

#[test]
fn t03_passing_eligibility_delegation() {
    let fixture = Fixture::new(true);
    assert!(
        fixture
            .evaluate()
            .bounded_eligibility_outcome()
            .passes_bounded_pre_score_eligibility()
    );
}

#[test]
fn t04_distinct_provider_rejection_and_precedence() {
    let mut fixture = Fixture::new(true);
    fixture.request = request(
        &["coding"],
        Some(constraints(
            Some(PROVIDER),
            Some(PROVIDER),
            Some("other"),
            Some("other"),
            None,
        )),
    );
    assert_eq!(
        fixture.evaluate().identity_constraint_outcome(),
        Rejected(DistinctProviderFromViolation)
    );
    fixture.assert_rejections(&[]);
}

#[test]
fn t05_avoided_provider_rejection_and_precedence() {
    let mut fixture = Fixture::new(true);
    fixture.request = request(
        &["coding"],
        Some(constraints(
            None,
            Some(PROVIDER),
            Some("other"),
            Some("other"),
            None,
        )),
    );
    assert_eq!(
        fixture.evaluate().identity_constraint_outcome(),
        Rejected(AvoidedProvider)
    );
    fixture.assert_rejections(&[]);
}

#[test]
fn t06_pinned_provider_rejection_and_precedence() {
    let mut fixture = Fixture::new(true);
    fixture.request = request(
        &["coding"],
        Some(constraints(None, None, Some("other"), Some("other"), None)),
    );
    assert_eq!(
        fixture.evaluate().identity_constraint_outcome(),
        Rejected(PinnedProviderMismatch)
    );
}

#[test]
fn t07_pinned_model_rejection() {
    let mut fixture = Fixture::new(true);
    fixture.request = request(
        &["coding"],
        Some(constraints(None, None, Some(PROVIDER), Some("other"), None)),
    );
    assert_eq!(
        fixture.evaluate().identity_constraint_outcome(),
        Rejected(PinnedModelMismatch)
    );
}

#[test]
fn t08_lifecycle_rejection() {
    Fixture::new(false)
        .assert_rejections(&[LifecycleNotNormallyRoutable(LifecycleState::Discovered)]);
}

#[test]
fn t09_unsupported_capability() {
    let mut fixture = Fixture::new(true);
    fixture.request = request(&["unsupported"], None);
    fixture.assert_rejections(&[RequiredCapabilityUnsupported(capability("unsupported"))]);
}

#[test]
fn t10_unknown_capability() {
    let mut fixture = Fixture::new(true);
    fixture.request = request(&["unknown"], None);
    fixture.assert_rejections(&[RequiredCapabilityUnknown(capability("unknown"))]);
}

#[test]
fn t11_availability_identity_mismatch() {
    let mut fixture = Fixture::new(true);
    for (provider, model, runtime) in [
        ("other", MODEL, RUNTIME),
        (PROVIDER, "other", RUNTIME),
        (PROVIDER, MODEL, "other"),
    ] {
        fixture.availability = availability(
            provider,
            Some(model),
            Some(runtime),
            AvailabilityStateKind::Available,
        );
        fixture.assert_rejections(&[AvailabilityIdentityMismatch]);
    }
}

#[test]
fn t12_availability_scope_insufficient() {
    let mut fixture = Fixture::new(true);
    for (model, runtime) in [(None, Some(RUNTIME)), (Some(MODEL), None), (None, None)] {
        fixture.availability =
            availability(PROVIDER, model, runtime, AvailabilityStateKind::Available);
        fixture.assert_rejections(&[AvailabilityScopeInsufficient]);
    }
}

#[test]
fn t13_all_availability_states_preserved() {
    let mut fixture = Fixture::new(true);
    for state in AvailabilityStateKind::ALL {
        fixture.availability = availability(PROVIDER, Some(MODEL), Some(RUNTIME), state);
        match state {
            AvailabilityStateKind::Available | AvailabilityStateKind::Degraded => {
                fixture.assert_rejections(&[])
            }
            _ => fixture.assert_rejections(&[AvailabilityIneligible(state)]),
        }
    }
}

#[test]
fn t14_policy_disallowed() {
    let mut fixture = Fixture::new(true);
    fixture.policy = policy(PolicyStatus::VerifiedDisallowed);
    fixture.assert_rejections(&[ProviderPolicyStatusIneligible(
        PolicyStatus::VerifiedDisallowed,
    )]);
}

#[test]
fn t15_policy_needs_review_q_v13_04_fail_closed() {
    let mut fixture = Fixture::new(true);
    fixture.policy = policy(PolicyStatus::NeedsReview);
    fixture.assert_rejections(&[ProviderPolicyStatusIneligible(PolicyStatus::NeedsReview)]);
}

#[test]
fn t16_policy_unknown_q_v13_04_fail_closed() {
    let mut fixture = Fixture::new(true);
    fixture.policy = policy(PolicyStatus::Unknown);
    fixture.assert_rejections(&[ProviderPolicyStatusIneligible(PolicyStatus::Unknown)]);
}

#[test]
fn t17_execution_context_not_proven() {
    let mut fixture = Fixture::new(true);
    for context in [None, Some("Worker"), Some("worker ")] {
        fixture.context = context.map(String::from);
        fixture.assert_rejections(&[ExecutionContextNotProvenAllowed]);
    }
}

#[test]
fn t18_reverification_missing() {
    let mut fixture = Fixture::new(true);
    fixture.deadline_passed = None;
    fixture.assert_rejections(&[ReverificationEvidenceMissing]);
}

#[test]
fn t19_reverification_deadline_passed() {
    let mut fixture = Fixture::new(true);
    fixture.deadline_passed = Some(true);
    fixture.assert_rejections(&[ReverificationDeadlinePassed]);
}

#[test]
fn t20_identity_rejection_does_not_suppress_eligibility() {
    let mut fixture = Fixture::new(false);
    fixture.request = request(
        &["coding"],
        Some(constraints(None, Some(PROVIDER), None, None, None)),
    );
    fixture.availability = availability(
        PROVIDER,
        Some(MODEL),
        Some(RUNTIME),
        AvailabilityStateKind::Unknown,
    );
    fixture.policy = policy(PolicyStatus::NeedsReview);
    let result = fixture.evaluate();
    assert_eq!(
        result.identity_constraint_outcome(),
        Rejected(AvoidedProvider)
    );
    assert_eq!(
        result.bounded_eligibility_outcome().rejections(),
        [
            LifecycleNotNormallyRoutable(LifecycleState::Discovered),
            AvailabilityIneligible(AvailabilityStateKind::Unknown),
            ProviderPolicyStatusIneligible(PolicyStatus::NeedsReview),
        ]
    );
}

#[test]
fn t21_eligibility_rejection_does_not_alter_identity() {
    let fixture = Fixture::new(false);
    let result = fixture.evaluate();
    assert_eq!(result.identity_constraint_outcome(), Satisfied);
    assert_eq!(
        result.bounded_eligibility_outcome().rejections(),
        [LifecycleNotNormallyRoutable(LifecycleState::Discovered)]
    );
}

#[test]
fn t22_multiple_rejections_preserve_direct_outcome_and_order() {
    let mut fixture = Fixture::new(false);
    fixture.request = request(
        &["z-unknown", "unsupported", "a-unknown", "z-unknown"],
        None,
    );
    fixture.availability = availability(
        PROVIDER,
        Some(MODEL),
        Some(RUNTIME),
        AvailabilityStateKind::RateLimited,
    );
    fixture.policy = policy(PolicyStatus::NeedsReview);
    fixture.assert_rejections(&[
        LifecycleNotNormallyRoutable(LifecycleState::Discovered),
        RequiredCapabilityUnknown(capability("z-unknown")),
        RequiredCapabilityUnsupported(capability("unsupported")),
        RequiredCapabilityUnknown(capability("a-unknown")),
        RequiredCapabilityUnknown(capability("z-unknown")),
        AvailabilityIneligible(AvailabilityStateKind::RateLimited),
        ProviderPolicyStatusIneligible(PolicyStatus::NeedsReview),
    ]);
}

#[test]
fn t23_max_cost_has_no_effect() {
    for promote in [false, true] {
        let mut fixture = Fixture::new(promote);
        for avoid in [None, Some(PROVIDER)] {
            fixture.request = request(
                &["coding"],
                Some(constraints(None, avoid, None, None, None)),
            );
            let baseline = fixture.evaluate();
            for cost in [0.0, -0.0, -17.25, 17.25, f64::MIN, f64::MAX] {
                fixture.request = request(
                    &["coding"],
                    Some(constraints(None, avoid, None, None, Some(cost))),
                );
                assert_eq!(fixture.evaluate(), baseline);
            }
        }
    }
}

#[test]
fn t24_observed_at_has_no_effect() {
    let mut fixture = Fixture::new(true);
    for state in AvailabilityStateKind::ALL {
        fixture.availability = availability(PROVIDER, Some(MODEL), Some(RUNTIME), state);
        let baseline = fixture.evaluate();
        let original = fixture.availability.clone();
        for text in [
            "0000-01-01T00:00:00-00:00",
            "9999-12-31T23:59:60+23:59",
            "2026-09-20t00:00:00z",
        ] {
            fixture.availability = AvailabilityState::new(original.core().clone(), timestamp(text));
            assert_eq!(fixture.availability.core(), original.core());
            assert_ne!(fixture.availability.observed_at(), original.observed_at());
            assert_eq!(fixture.evaluate(), baseline);
        }
    }
}

#[test]
fn t25_determinism() {
    for promote in [false, true] {
        let mut fixture = Fixture::new(promote);
        for avoid in [None, Some(PROVIDER)] {
            fixture.request = request(
                &["coding"],
                Some(constraints(None, avoid, None, None, None)),
            );
            let first = fixture.evaluate();
            for _ in 0..100 {
                assert_eq!(fixture.evaluate(), first);
            }
        }
    }
}

#[test]
fn t26_inputs_unchanged() {
    for promote in [false, true] {
        let mut fixture = Fixture::new(promote);
        for avoid in [None, Some(PROVIDER)] {
            fixture.request = request(
                &["coding"],
                Some(constraints(None, avoid, None, None, Some(-17.25))),
            );
            let before = fixture.clone();
            fixture.evaluate();
            assert_eq!(fixture, before);
        }
    }
}
