use crate::CandidateEligibilityRejection::*;
use crate::CandidateIdentityConstraintOutcome::{Rejected, Satisfied};
use crate::CandidateIdentityConstraintRejection::AvoidedProvider;
use crate::intelligence::{LifecycleState, ModelIntelligenceService};
use crate::policy_eligibility::{
    ModelRoutingDateTimeV1, PolicyEvidenceLabel, PolicyStatus, ProviderPolicyEligibility,
    TechnicalStatus,
};
use crate::registry::{CapabilityId, ModelId, Observation, ProviderId, RuntimeId};
use crate::*;

fn timestamp() -> ModelRoutingDateTimeV1 {
    ModelRoutingDateTimeV1::try_new("2026-09-20T00:00:00Z".into()).unwrap()
}

fn observation() -> Observation {
    Observation {
        confidence: EvidenceConfidence::Unverified,
        source_ref: Some(
            EvidenceSourceRef::try_new(
                EvidenceSourceRefType::ArtifactId,
                "synthetic-set-composition-evidence".into(),
                None,
                None,
            )
            .unwrap(),
        ),
        observed_at: timestamp(),
    }
}

fn registry(rows: &[(&str, &str, &str)]) -> ModelIntelligenceService {
    let mut service = ModelIntelligenceService::new();
    for &(provider, model, runtime) in rows {
        let p = ProviderId::try_new(provider.into()).unwrap();
        let m = ModelId::try_new(model.into()).unwrap();
        let r = RuntimeId::try_new(runtime.into()).unwrap();
        if service.registry().provider(provider).is_none() {
            service.insert_provider(p.clone(), observation()).unwrap();
        }
        if service.registry().model(provider, model).is_none() {
            service
                .discover_model(p.clone(), m.clone(), observation())
                .unwrap();
        }
        if service.registry().runtime(provider, runtime).is_none() {
            service
                .insert_runtime(p.clone(), r.clone(), observation())
                .unwrap();
        }
        service.associate_runtime(p, m, r, observation()).unwrap();
    }
    service
}

fn request(
    required: &[&str],
    avoid: Option<&str>,
    max_cost: Option<f64>,
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
        Some(
            RoutingRequestConstraints::try_new(
                None,
                avoid.map(|value| vec![value.into()]),
                None,
                None,
                max_cost,
            )
            .unwrap(),
        ),
        None,
    )
    .unwrap()
}

fn availability(
    candidate: &RegistryCandidateIdentity,
    state: AvailabilityStateKind,
) -> AvailabilityState {
    AvailabilityState::new(
        AvailabilityStateNonTemporalCore::try_new(
            candidate.provider_id().as_str().into(),
            Some(candidate.model_id().as_str().into()),
            Some(candidate.runtime_id().as_str().into()),
            state,
            None,
            None,
            None,
        )
        .unwrap(),
        timestamp(),
    )
}

fn policy(candidate: &RegistryCandidateIdentity) -> ProviderPolicyEligibility {
    policy_with_technical(candidate, TechnicalStatus::Connected)
}

fn policy_with_technical(
    candidate: &RegistryCandidateIdentity,
    technical: TechnicalStatus,
) -> ProviderPolicyEligibility {
    ProviderPolicyEligibility::try_new(
        candidate.provider_id().as_str().into(),
        candidate.runtime_id().as_str().into(),
        "synthetic-credential".into(),
        technical,
        PolicyStatus::VerifiedAllowed,
        Some(vec!["worker".into(), " Worker ".into(), "é".into()]),
        timestamp(),
        None,
        PolicyEvidenceLabel::PolicyNeedsReview,
        None,
        None,
        Some(Some(timestamp())),
        None,
    )
    .unwrap()
}

#[test]
fn recorded_technical_status_propagates_in_canonical_candidate_order() {
    let service = registry(&[
        ("C", "M", "R"),
        ("A", "M", "R2"),
        ("B", "M", "R"),
        ("A", "M", "R1"),
        ("D", "M", "R"),
    ]);
    let candidates = enumerate_registry_candidates(service.registry());
    let available: Vec<_> = candidates
        .iter()
        .map(|candidate| availability(candidate, AvailabilityStateKind::Available))
        .collect();
    let policies: Vec<_> = candidates
        .iter()
        .zip(TechnicalStatus::ALL)
        .map(|(candidate, status)| policy_with_technical(candidate, status))
        .collect();
    let evidence = bundles(&candidates, &available, &policies, &[Some(false); 5]);
    let result = assert_direct_delegation(
        &service,
        &request(&["coding"], None, None),
        &evidence,
        Some("worker"),
    );
    assert_eq!(
        result
            .iter()
            .map(|item| item.candidate())
            .collect::<Vec<_>>(),
        candidates.iter().collect::<Vec<_>>()
    );
    for (assessment, status) in result.iter().zip(TechnicalStatus::ALL) {
        let mut expected = vec![
            LifecycleNotNormallyRoutable(LifecycleState::Discovered),
            RequiredCapabilityUnknown(CapabilityId::try_new("coding".into()).unwrap()),
        ];
        if status != TechnicalStatus::Connected {
            expected.push(RecordedTechnicalStatusIneligible(status));
        }
        assert_eq!(
            assessment.bounded_eligibility_outcome().rejections(),
            expected
        );
    }
}

fn bundles<'a>(
    candidates: &[RegistryCandidateIdentity],
    availability: &'a [AvailabilityState],
    policy: &'a [ProviderPolicyEligibility],
    deadlines: &[Option<bool>],
) -> Vec<BoundedPreScoreCandidateEvidence<'a>> {
    candidates
        .iter()
        .zip(availability)
        .zip(policy)
        .zip(deadlines)
        .map(|(((candidate, availability), policy), deadline)| {
            BoundedPreScoreCandidateEvidence::new(
                candidate.clone(),
                availability,
                policy,
                *deadline,
            )
        })
        .collect()
}

fn assert_direct_delegation(
    service: &ModelIntelligenceService,
    request: &RoutingRequestNonTemporalCore,
    evidence: &[BoundedPreScoreCandidateEvidence<'_>],
    context: Option<&str>,
) -> Vec<BoundedPreScoreCandidateAssessment> {
    let candidates = enumerate_registry_candidates(service.registry());
    let result =
        evaluate_bounded_pre_score_candidate_set(request, service.registry(), evidence, context)
            .unwrap();
    assert_eq!(result.len(), candidates.len());
    for ((assessment, candidate), bundle) in result.iter().zip(&candidates).zip(evidence) {
        assert_eq!(assessment.candidate(), candidate);
        assert_eq!(
            assessment,
            &evaluate_bounded_pre_score_candidate(
                candidate,
                request,
                service.registry(),
                bundle.availability(),
                bundle.policy(),
                context,
                bundle.reverification_deadline_passed(),
            )
        );
    }
    result
}

#[test]
fn empty_and_count_mismatch() {
    let service = registry(&[]);
    let req = request(&["coding"], None, None);
    assert_eq!(
        evaluate_bounded_pre_score_candidate_set(&req, service.registry(), &[], None),
        Ok(vec![])
    );
    let other = registry(&[("P", "M", "R")]);
    let candidate = enumerate_registry_candidates(other.registry())
        .pop()
        .unwrap();
    let available = availability(&candidate, AvailabilityStateKind::Available);
    let allowed = policy(&candidate);
    let evidence = [BoundedPreScoreCandidateEvidence::new(
        candidate, &available, &allowed, None,
    )];
    assert_eq!(
        evaluate_bounded_pre_score_candidate_set(&req, service.registry(), &evidence, None),
        Err(BoundedPreScoreEvidenceBindingError::EvidenceCountMismatch)
    );
    assert_eq!(
        evaluate_bounded_pre_score_candidate_set(&req, other.registry(), &[], None),
        Err(BoundedPreScoreEvidenceBindingError::EvidenceCountMismatch)
    );
}

#[test]
fn one_candidate_and_more_or_fewer_evidence() {
    let service = registry(&[("P", "M", "R")]);
    let candidates = enumerate_registry_candidates(service.registry());
    let available = [availability(
        &candidates[0],
        AvailabilityStateKind::Available,
    )];
    let allowed = [policy(&candidates[0])];
    let evidence = bundles(&candidates, &available, &allowed, &[Some(false)]);
    assert_eq!(
        assert_direct_delegation(
            &service,
            &request(&["coding"], None, None),
            &evidence,
            Some("worker")
        )
        .len(),
        1
    );
    assert_eq!(
        evaluate_bounded_pre_score_candidate_set(
            &request(&["coding"], None, None),
            service.registry(),
            &[evidence[0].clone(), evidence[0].clone()],
            Some("worker")
        ),
        Err(BoundedPreScoreEvidenceBindingError::EvidenceCountMismatch)
    );
}

#[test]
fn positional_binding_rejects_first_late_and_reversed_mismatch_without_partial_results() {
    let service = registry(&[("B", "M", "R"), ("A", "M", "R"), ("C", "M", "R")]);
    let candidates = enumerate_registry_candidates(service.registry());
    let available: Vec<_> = candidates
        .iter()
        .map(|c| availability(c, AvailabilityStateKind::Available))
        .collect();
    let allowed: Vec<_> = candidates.iter().map(policy).collect();
    let mut evidence = bundles(&candidates, &available, &allowed, &[None; 3]);
    let req = request(&["coding"], None, None);
    evidence[0] = BoundedPreScoreCandidateEvidence::new(
        candidates[1].clone(),
        &available[0],
        &allowed[0],
        None,
    );
    assert_eq!(
        evaluate_bounded_pre_score_candidate_set(&req, service.registry(), &evidence, None),
        Err(BoundedPreScoreEvidenceBindingError::CandidateIdentityMismatch { index: 0 })
    );
    evidence[0] = BoundedPreScoreCandidateEvidence::new(
        candidates[0].clone(),
        &available[0],
        &allowed[0],
        None,
    );
    evidence[2] = BoundedPreScoreCandidateEvidence::new(
        candidates[1].clone(),
        &available[2],
        &allowed[2],
        None,
    );
    assert_eq!(
        evaluate_bounded_pre_score_candidate_set(&req, service.registry(), &evidence, None),
        Err(BoundedPreScoreEvidenceBindingError::CandidateIdentityMismatch { index: 2 })
    );
    evidence[2] = BoundedPreScoreCandidateEvidence::new(
        candidates[2].clone(),
        &available[2],
        &allowed[2],
        None,
    );
    evidence.reverse();
    assert_eq!(
        evaluate_bounded_pre_score_candidate_set(&req, service.registry(), &evidence, None),
        Err(BoundedPreScoreEvidenceBindingError::CandidateIdentityMismatch { index: 0 })
    );
}

#[test]
fn canonical_order_rejections_duplicates_and_max_cost_are_preserved() {
    let service = registry(&[("B", "M", "R"), ("A", "M", "R2"), ("A", "M", "R1")]);
    let candidates = enumerate_registry_candidates(service.registry());
    let available: Vec<_> = candidates
        .iter()
        .map(|c| availability(c, AvailabilityStateKind::Unknown))
        .collect();
    let allowed: Vec<_> = candidates.iter().map(policy).collect();
    let evidence = bundles(&candidates, &available, &allowed, &[Some(false); 3]);
    let req = request(&["missing", "missing"], Some("A"), None);
    let result = assert_direct_delegation(&service, &req, &evidence, Some("worker"));
    assert_eq!(
        result.iter().map(|a| a.candidate()).collect::<Vec<_>>(),
        candidates.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        result
            .iter()
            .map(|a| a.identity_constraint_outcome())
            .collect::<Vec<_>>(),
        [
            Rejected(AvoidedProvider),
            Rejected(AvoidedProvider),
            Satisfied
        ]
    );
    for assessment in &result {
        assert_eq!(
            assessment.bounded_eligibility_outcome().rejections(),
            [
                LifecycleNotNormallyRoutable(LifecycleState::Discovered),
                RequiredCapabilityUnknown(CapabilityId::try_new("missing".into()).unwrap()),
                RequiredCapabilityUnknown(CapabilityId::try_new("missing".into()).unwrap()),
                AvailabilityIneligible(AvailabilityStateKind::Unknown),
            ]
        );
    }
    for cost in [Some(0.0), Some(-17.25), Some(f64::MAX)] {
        assert_eq!(
            result,
            assert_direct_delegation(
                &service,
                &request(&["missing", "missing"], Some("A"), cost),
                &evidence,
                Some("worker")
            )
        );
    }
    assert_eq!(
        result,
        evaluate_bounded_pre_score_candidate_set(
            &req,
            service.registry(),
            &evidence,
            Some("worker")
        )
        .unwrap()
    );
}

#[test]
fn evidence_internal_identities_and_scope_remain_subordinate_rejections() {
    let service = registry(&[("P", "M", "R")]);
    let candidates = enumerate_registry_candidates(service.registry());
    let candidate = &candidates[0];
    let req = request(&["coding"], None, None);
    let allowed = policy(candidate);
    for (p, m, r, expected) in [
        ("p", Some("M"), Some("R"), AvailabilityIdentityMismatch),
        ("P", Some("m"), Some("R"), AvailabilityIdentityMismatch),
        ("P", Some("M"), Some("r"), AvailabilityIdentityMismatch),
        ("P", None, Some("R"), AvailabilityScopeInsufficient),
    ] {
        let available = AvailabilityState::new(
            AvailabilityStateNonTemporalCore::try_new(
                p.into(),
                m.map(String::from),
                r.map(String::from),
                AvailabilityStateKind::Available,
                None,
                None,
                None,
            )
            .unwrap(),
            timestamp(),
        );
        let evidence = [BoundedPreScoreCandidateEvidence::new(
            candidate.clone(),
            &available,
            &allowed,
            Some(false),
        )];
        let result = assert_direct_delegation(&service, &req, &evidence, Some("worker"));
        assert!(
            result[0]
                .bounded_eligibility_outcome()
                .rejections()
                .contains(&expected)
        );
    }
    let available = availability(candidate, AvailabilityStateKind::Available);
    let allowed = ProviderPolicyEligibility::try_new(
        "p".into(),
        "R".into(),
        "credential".into(),
        TechnicalStatus::Connected,
        PolicyStatus::VerifiedAllowed,
        Some(vec!["worker".into()]),
        timestamp(),
        None,
        PolicyEvidenceLabel::PolicyNeedsReview,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let evidence = [BoundedPreScoreCandidateEvidence::new(
        candidate.clone(),
        &available,
        &allowed,
        Some(false),
    )];
    assert!(
        assert_direct_delegation(&service, &req, &evidence, Some("worker"))[0]
            .bounded_eligibility_outcome()
            .rejections()
            .contains(&ProviderPolicyIdentityMismatch)
    );
}

#[test]
fn availability_context_and_deadline_are_delegated_per_candidate() {
    let service = registry(&[("P", "M", "R1"), ("P", "M", "R2"), ("P", "M", "R3")]);
    let candidates = enumerate_registry_candidates(service.registry());
    let available = [
        availability(&candidates[0], AvailabilityStateKind::Available),
        availability(&candidates[1], AvailabilityStateKind::Degraded),
        availability(&candidates[2], AvailabilityStateKind::Unknown),
    ];
    let allowed: Vec<_> = candidates.iter().map(policy).collect();
    let evidence = bundles(
        &candidates,
        &available,
        &allowed,
        &[Some(false), Some(true), None],
    );
    let req = request(&["coding"], None, None);
    for context in [
        Some("worker"),
        Some(" Worker "),
        Some("é"),
        Some("Worker"),
        Some("e\u{301}"),
        None,
    ] {
        let result = assert_direct_delegation(&service, &req, &evidence, context);
        assert!(
            !result[0]
                .bounded_eligibility_outcome()
                .rejections()
                .iter()
                .any(|r| matches!(r, AvailabilityIneligible(_)))
        );
        assert!(
            !result[1]
                .bounded_eligibility_outcome()
                .rejections()
                .iter()
                .any(|r| matches!(r, AvailabilityIneligible(_)))
        );
        assert!(
            result[2]
                .bounded_eligibility_outcome()
                .rejections()
                .contains(&AvailabilityIneligible(AvailabilityStateKind::Unknown))
        );
        assert!(
            !result[0]
                .bounded_eligibility_outcome()
                .rejections()
                .contains(&ReverificationDeadlinePassed)
        );
        assert!(
            result[1]
                .bounded_eligibility_outcome()
                .rejections()
                .contains(&ReverificationDeadlinePassed)
        );
        assert!(
            result[2]
                .bounded_eligibility_outcome()
                .rejections()
                .contains(&ReverificationEvidenceMissing)
        );
        let context_rejected = !matches!(context, Some("worker" | " Worker " | "é"));
        assert!(result.iter().all(|assessment| {
            assessment
                .bounded_eligibility_outcome()
                .rejections()
                .contains(&ExecutionContextNotProvenAllowed)
                == context_rejected
        }));
    }
}

#[test]
fn exact_case_whitespace_and_unicode_candidate_binding() {
    let service = registry(&[
        ("P", "M", "R"),
        ("p", "M", "R"),
        (" P ", "M", "R"),
        ("é", "M", "R"),
        ("e\u{301}", "M", "R"),
    ]);
    let candidates = enumerate_registry_candidates(service.registry());
    let available: Vec<_> = candidates
        .iter()
        .map(|c| availability(c, AvailabilityStateKind::Available))
        .collect();
    let allowed: Vec<_> = candidates.iter().map(policy).collect();
    let evidence = bundles(&candidates, &available, &allowed, &[Some(false); 5]);
    assert_direct_delegation(
        &service,
        &request(&["coding"], None, None),
        &evidence,
        Some("worker"),
    );
    let mut reversed = evidence.clone();
    reversed.reverse();
    assert_eq!(
        evaluate_bounded_pre_score_candidate_set(
            &request(&["coding"], None, None),
            service.registry(),
            &reversed,
            Some("worker")
        ),
        Err(BoundedPreScoreEvidenceBindingError::CandidateIdentityMismatch { index: 0 })
    );
}
