use super::*;
use crate::intelligence::*;
use crate::{EvidenceSourceRef, EvidenceSourceRefType, policy_eligibility::ModelRoutingDateTimeV1};

fn p(s: &str) -> ProviderId {
    ProviderId::try_new(s.into()).unwrap()
}
fn m(s: &str) -> ModelId {
    ModelId::try_new(s.into()).unwrap()
}
fn r(s: &str) -> RuntimeId {
    RuntimeId::try_new(s.into()).unwrap()
}
fn c(s: &str) -> CapabilityId {
    CapabilityId::try_new(s.into()).unwrap()
}
fn observation(confidence: EvidenceConfidence) -> Observation {
    Observation {
        confidence,
        source_ref: Some(
            EvidenceSourceRef::try_new(
                EvidenceSourceRefType::ArtifactId,
                "evidence-☃".into(),
                Some("".into()),
                None,
            )
            .unwrap(),
        ),
        observed_at: ModelRoutingDateTimeV1::try_new("2026-09-08t00:00:00.000z".into()).unwrap(),
    }
}
fn subject(kind: SubjectKind) -> CapabilitySubject {
    match kind {
        SubjectKind::Model => CapabilitySubject::Model {
            provider_id: p("provider-a"),
            model_id: m("model-a"),
        },
        SubjectKind::Runtime => CapabilitySubject::Runtime {
            provider_id: p("provider-a"),
            runtime_id: r("runtime-a"),
        },
        SubjectKind::ModelRuntimePair => CapabilitySubject::ModelRuntimePair {
            provider_id: p("provider-a"),
            model_id: m("model-a"),
            runtime_id: r("runtime-a"),
        },
    }
}
fn assertion(
    kind: SubjectKind,
    confidence: EvidenceConfidence,
    value: Option<CapabilityValue>,
) -> CapabilityEvidence {
    CapabilityEvidence {
        subject: subject(kind),
        capability: c("coding"),
        value,
        observation: observation(confidence),
        sample_size: None,
    }
}
fn service() -> ModelIntelligenceService {
    let mut s = ModelIntelligenceService::new();
    s.insert_provider(p("provider-a"), observation(EvidenceConfidence::Unverified))
        .unwrap();
    s.discover_model(
        p("provider-a"),
        m("model-a"),
        observation(EvidenceConfidence::Unverified),
    )
    .unwrap();
    s.insert_runtime(
        p("provider-a"),
        r("runtime-a"),
        observation(EvidenceConfidence::Unverified),
    )
    .unwrap();
    s.associate_runtime(
        p("provider-a"),
        m("model-a"),
        r("runtime-a"),
        observation(EvidenceConfidence::Unverified),
    )
    .unwrap();
    s
}
fn path() -> VerificationPath {
    VerificationPath {
        runtime_id: r("runtime-a"),
        required_capabilities: vec![c("coding")],
    }
}
fn transition(target: LifecycleState) -> LifecycleTransition {
    use LifecycleState::*;
    let evidence = match target {
        Discovered | Unassessed => LifecycleEvidence::ReadyForAssessment,
        CapabilityVerified => LifecycleEvidence::CapabilityVerification(path()),
        Calibrating => LifecycleEvidence::BoundedCalibrationAdmission { authorized: true },
        Routable => {
            LifecycleEvidence::RoutablePromotion(RoutablePromotionEvidence::LocalCalibration {
                sufficient_acceptable_observations: true,
            })
        }
        Deprecated => LifecycleEvidence::Demotion(DemotionReason::VendorRetirement),
        Disabled => LifecycleEvidence::Demotion(DemotionReason::RepeatedRuntimeFailures),
        Unavailable => LifecycleEvidence::Demotion(DemotionReason::AuthLoss),
        UserBlocked => LifecycleEvidence::Demotion(DemotionReason::UserBlock),
    };
    LifecycleTransition {
        target,
        evidence,
        observation: observation(EvidenceConfidence::LocalEmpirical),
    }
}
fn apply(s: &mut ModelIntelligenceService, t: LifecycleTransition) -> Result<(), RegistryError> {
    s.try_transition("provider-a", "model-a", t)
}
fn state(s: &ModelIntelligenceService) -> LifecycleState {
    s.registry()
        .model("provider-a", "model-a")
        .unwrap()
        .lifecycle_state()
}
fn add(s: &mut ModelIntelligenceService, kind: SubjectKind, confidence: EvidenceConfidence) {
    s.record_capability(assertion(
        kind,
        confidence,
        Some(CapabilityValue::Boolean(true)),
    ))
    .unwrap();
}
fn evidenced() -> ModelIntelligenceService {
    let mut s = service();
    add(
        &mut s,
        SubjectKind::Model,
        EvidenceConfidence::OfficialVerified,
    );
    for confidence in [
        EvidenceConfidence::OfficialVerified,
        EvidenceConfidence::IndependentVerified,
        EvidenceConfidence::LocalEmpirical,
    ] {
        add(&mut s, SubjectKind::ModelRuntimePair, confidence);
    }
    s
}
fn reach(s: &mut ModelIntelligenceService, target: LifecycleState) {
    if target == LifecycleState::Discovered {
        return;
    }
    for next in [
        LifecycleState::Unassessed,
        LifecycleState::CapabilityVerified,
        LifecycleState::Calibrating,
        LifecycleState::Routable,
    ] {
        apply(s, transition(next)).unwrap();
        if next == target {
            return;
        }
    }
    apply(s, transition(target)).unwrap();
}

#[test]
fn registry_identities_are_nonempty_open_and_exact() {
    assert_eq!(ProviderId::try_new("".into()), Err(EmptyIdentifier));
    assert_eq!(ModelId::try_new("".into()), Err(EmptyIdentifier));
    assert_eq!(RuntimeId::try_new("".into()), Err(EmptyIdentifier));
    let mut s = ModelIntelligenceService::new();
    let ids = [
        " ",
        "future-provider-☃",
        "future-model",
        "future-runtime-☃",
        "Mixed.Case/α:+",
        "é",
        "e\u{301}",
    ];
    for id in ids {
        s.insert_provider(p(id), observation(EvidenceConfidence::Unverified))
            .unwrap();
        s.discover_model(p(id), m(id), observation(EvidenceConfidence::Unverified))
            .unwrap();
        s.insert_runtime(p(id), r(id), observation(EvidenceConfidence::Unverified))
            .unwrap();
        s.associate_runtime(
            p(id),
            m(id),
            r(id),
            observation(EvidenceConfidence::Unverified),
        )
        .unwrap();
        assert_eq!(
            s.registry().provider(id).unwrap().provider_id().as_str(),
            id
        );
        assert_eq!(s.registry().model(id, id).unwrap().model_id().as_str(), id);
        assert_eq!(
            s.registry().runtime(id, id).unwrap().runtime_id().as_str(),
            id
        );
        assert_eq!(
            s.registry()
                .association(id, id, id)
                .unwrap()
                .runtime_id()
                .as_str(),
            id
        );
    }
    let before = s.registry().clone();
    for wrong in [
        "",
        "  ",
        "Future-model",
        "future-model ",
        "future",
        "mixed.case/α:+",
    ] {
        assert!(s.registry().provider(wrong).is_none());
        assert!(s.registry().model("future-model", wrong).is_none());
        assert!(s.registry().runtime("future-model", wrong).is_none());
        assert!(
            s.registry()
                .association("future-model", "future-model", wrong)
                .is_none()
        );
    }
    assert_ne!(
        s.registry().provider("é"),
        s.registry().provider("e\u{301}")
    );
    assert_eq!(&before, s.registry());
}

#[test]
fn registry_duplicates_reject_without_overwriting_and_keys_remain_distinct() {
    let mut s = service();
    let before = s.registry().clone();
    for _ in 0..2 {
        assert_eq!(
            s.insert_provider(
                p("provider-a"),
                observation(EvidenceConfidence::OfficialVerified)
            ),
            Err(RegistryError::DuplicateProvider)
        );
        assert_eq!(
            s.discover_model(
                p("provider-a"),
                m("model-a"),
                observation(EvidenceConfidence::OfficialVerified)
            ),
            Err(RegistryError::DuplicateModel)
        );
        assert_eq!(
            s.insert_runtime(
                p("provider-a"),
                r("runtime-a"),
                observation(EvidenceConfidence::OfficialVerified)
            ),
            Err(RegistryError::DuplicateRuntime)
        );
        assert_eq!(
            s.associate_runtime(
                p("provider-a"),
                m("model-a"),
                r("runtime-a"),
                observation(EvidenceConfidence::OfficialVerified)
            ),
            Err(RegistryError::DuplicateAssociation)
        );
        assert_eq!(&before, s.registry());
    }
    s.insert_provider(p("provider-b"), observation(EvidenceConfidence::Unverified))
        .unwrap();
    s.discover_model(
        p("provider-b"),
        m("model-a"),
        observation(EvidenceConfidence::Unverified),
    )
    .unwrap();
    s.discover_model(
        p("provider-a"),
        m("model-b"),
        observation(EvidenceConfidence::Unverified),
    )
    .unwrap();
    s.associate_runtime(
        p("provider-a"),
        m("model-b"),
        r("runtime-a"),
        observation(EvidenceConfidence::Unverified),
    )
    .unwrap();
    s.insert_runtime(
        p("provider-a"),
        r("runtime-b"),
        observation(EvidenceConfidence::Unverified),
    )
    .unwrap();
    s.associate_runtime(
        p("provider-a"),
        m("model-a"),
        r("runtime-b"),
        observation(EvidenceConfidence::Unverified),
    )
    .unwrap();
    assert_eq!(s.registry().models().count(), 3);
    assert_eq!(
        s.registry()
            .model("provider-a", "model-a")
            .unwrap()
            .runtime_associations()
            .count(),
        2
    );
    assert!(
        s.registry()
            .association("provider-a", "model-b", "runtime-a")
            .is_some()
    );
    assert!(
        s.registry()
            .association("provider-b", "model-a", "runtime-a")
            .is_none()
    );
}

#[test]
fn registry_missing_parents_and_subjects_reject_atomically() {
    let mut s = ModelIntelligenceService::new();
    let before = s.registry().clone();
    assert_eq!(
        s.discover_model(
            p("provider-a"),
            m("model-a"),
            observation(EvidenceConfidence::Unverified)
        ),
        Err(RegistryError::ProviderNotFound)
    );
    assert_eq!(
        s.insert_runtime(
            p("provider-a"),
            r("runtime-a"),
            observation(EvidenceConfidence::Unverified)
        ),
        Err(RegistryError::ProviderNotFound)
    );
    assert_eq!(
        s.associate_runtime(
            p("provider-a"),
            m("model-a"),
            r("runtime-a"),
            observation(EvidenceConfidence::Unverified)
        ),
        Err(RegistryError::ModelNotFound)
    );
    for kind in SubjectKind::ALL {
        assert_eq!(
            s.record_capability(assertion(kind, EvidenceConfidence::OfficialVerified, None)),
            Err(RegistryError::SubjectNotFound)
        );
    }
    assert_eq!(
        apply(&mut s, transition(LifecycleState::Routable)),
        Err(RegistryError::ModelNotFound)
    );
    assert_eq!(&before, s.registry());
}

#[test]
fn registry_enumeration_is_exact_lexical_identity_order_independent_of_insertion() {
    let mut snapshots = Vec::new();
    for ids in [["z", " ", "☃", "A"], ["☃", "A", " ", "z"]] {
        let mut s = ModelIntelligenceService::new();
        for provider in ids {
            s.insert_provider(p(provider), observation(EvidenceConfidence::Unverified))
                .unwrap();
            for id in ids {
                s.discover_model(
                    p(provider),
                    m(id),
                    observation(EvidenceConfidence::Unverified),
                )
                .unwrap();
                s.insert_runtime(
                    p(provider),
                    r(id),
                    observation(EvidenceConfidence::Unverified),
                )
                .unwrap();
            }
            for model in ids {
                for runtime in ids {
                    s.associate_runtime(
                        p(provider),
                        m(model),
                        r(runtime),
                        observation(EvidenceConfidence::Unverified),
                    )
                    .unwrap();
                }
            }
        }
        assert_eq!(
            s.registry()
                .providers()
                .map(|p| p.provider_id().as_str())
                .collect::<Vec<_>>(),
            [" ", "A", "z", "☃"]
        );
        let models = s
            .registry()
            .models()
            .map(|m| (m.provider_id().as_str(), m.model_id().as_str()))
            .collect::<Vec<_>>();
        assert!(models.windows(2).all(|w| w[0] < w[1]));
        let runtimes = s
            .registry()
            .runtimes()
            .map(|r| (r.provider_id().as_str(), r.runtime_id().as_str()))
            .collect::<Vec<_>>();
        assert!(runtimes.windows(2).all(|w| w[0] < w[1]));
        for model in s.registry().models() {
            assert_eq!(
                model
                    .runtime_associations()
                    .map(|a| a.runtime_id().as_str())
                    .collect::<Vec<_>>(),
                [" ", "A", "z", "☃"]
            );
        }
        snapshots.push(s.registry().clone());
    }
    assert_eq!(snapshots[0], snapshots[1]);
}

#[test]
fn capability_identifiers_and_subject_lookups_preserve_exact_values() {
    assert_eq!(CapabilityId::try_new("".into()), Err(EmptyIdentifier));
    let mut s = service();
    for value in [
        " ",
        "coding",
        "structured_output",
        "future_capability",
        "☃",
        "foo.bar",
        "Coding",
        " coding",
        "é",
        "e\u{301}",
    ] {
        assert_eq!(c(value).as_str(), value);
        for kind in SubjectKind::ALL {
            let mut e = assertion(
                kind,
                EvidenceConfidence::Unverified,
                Some(CapabilityValue::Unknown),
            );
            e.capability = c(value);
            s.record_capability(e.clone()).unwrap();
            assert_eq!(
                s.record_capability(e.clone()),
                Err(RegistryError::DuplicateEvidence)
            );
            assert_eq!(
                s.registry().capability_evidence(&subject(kind), value),
                Some([e].as_slice())
            );
        }
    }
    for missing in ["", "CODING", "coding ", "foo"] {
        assert!(
            s.registry()
                .capability_evidence(&subject(SubjectKind::Model), missing)
                .is_none()
        );
    }
}

#[test]
fn capability_bounded_values_sources_timestamps_and_sample_sizes_are_preserved() {
    for kind in SubjectKind::ALL {
        for confidence in EvidenceConfidence::ALL {
            for value in [
                None,
                Some(CapabilityValue::Unknown),
                Some(CapabilityValue::Boolean(false)),
                Some(CapabilityValue::Boolean(true)),
            ] {
                for sample_size in [None, Some(None), Some(Some(0)), Some(Some(u64::MAX))] {
                    for ref_type in EvidenceSourceRefType::ALL {
                        let mut s = service();
                        let mut e = assertion(kind, confidence, value);
                        e.sample_size = sample_size;
                        e.observation.source_ref = Some(
                            EvidenceSourceRef::try_new(
                                ref_type,
                                " nonexistent/opaque ☃ ".into(),
                                Some("".into()),
                                Some(" section ".into()),
                            )
                            .unwrap(),
                        );
                        s.record_capability(e.clone()).unwrap();
                        let stored = &s
                            .registry()
                            .capability_evidence(&subject(kind), "coding")
                            .unwrap()[0];
                        assert_eq!(stored, &e);
                        assert_eq!(
                            stored.observation.observed_at.as_str(),
                            "2026-09-08t00:00:00.000z"
                        );
                        assert_eq!(stored.subject.kind(), kind);
                    }
                }
            }
        }
    }
}

#[test]
fn lifecycle_vocabularies_are_exact_and_only_routable_passes_normal_gate() {
    use LifecycleState::*;
    assert_eq!(
        LifecycleState::ALL.map(LifecycleState::as_str),
        [
            "DISCOVERED",
            "UNASSESSED",
            "CAPABILITY_VERIFIED",
            "CALIBRATING",
            "ROUTABLE",
            "DEPRECATED",
            "DISABLED",
            "UNAVAILABLE",
            "USER_BLOCKED"
        ]
    );
    for state in LifecycleState::ALL {
        match state {
            Discovered | Unassessed | CapabilityVerified | Calibrating | Routable | Deprecated
            | Disabled | Unavailable | UserBlocked => {}
        }
        assert_eq!(state.passes_normal_lifecycle_gate(), state == Routable);
    }
    assert_eq!(
        SubjectKind::ALL.map(SubjectKind::as_str),
        ["MODEL", "RUNTIME", "MODEL_RUNTIME_PAIR"]
    );
    assert_eq!(
        EvidenceConfidence::ALL.map(EvidenceConfidence::as_str),
        [
            "OFFICIAL_VERIFIED",
            "INDEPENDENT_VERIFIED",
            "LOCAL_EMPIRICAL",
            "USER_DECLARED",
            "UNVERIFIED"
        ]
    );
}

#[test]
fn lifecycle_exhaustive_9_by_9_transition_matrix() {
    // Rows/columns are LifecycleState::ALL. P=adjacent evidenced promotion;
    // S=reason-backed demotion from a primary state. All other pairs reject.
    let allowed = [
        [false, true, false, false, false, true, true, true, true],
        [false, false, true, false, false, true, true, true, true],
        [false, false, false, true, false, true, true, true, true],
        [false, false, false, false, true, true, true, true, true],
        [false, false, false, false, false, true, true, true, true],
        [false; 9],
        [false; 9],
        [false; 9],
        [false; 9],
    ];
    let mut checked = 0;
    for (i, from) in LifecycleState::ALL.into_iter().enumerate() {
        for (j, to) in LifecycleState::ALL.into_iter().enumerate() {
            let mut s = evidenced();
            reach(&mut s, from);
            let before = s.registry().clone();
            let result = apply(&mut s, transition(to));
            assert_eq!(
                result.is_ok(),
                allowed[i][j],
                "{from:?} -> {to:?}: {result:?}"
            );
            if allowed[i][j] {
                assert_eq!(state(&s), to);
            } else {
                assert_eq!(
                    result,
                    Err(RegistryError::Lifecycle(LifecycleError::IllegalTransition))
                );
                assert_eq!(&before, s.registry());
            }
            checked += 1;
        }
    }
    assert_eq!(checked, 81);
}

#[test]
fn lifecycle_marketing_discovery_cannot_promote_or_reinsert_over_state() {
    let mut s = service();
    for kind in SubjectKind::ALL {
        add(&mut s, kind, EvidenceConfidence::Unverified);
    }
    assert_eq!(state(&s), LifecycleState::Discovered);
    for to in [
        LifecycleState::CapabilityVerified,
        LifecycleState::Calibrating,
        LifecycleState::Routable,
    ] {
        assert!(apply(&mut s, transition(to)).is_err());
    }
    apply(&mut s, transition(LifecycleState::Unassessed)).unwrap();
    for to in [
        LifecycleState::CapabilityVerified,
        LifecycleState::Calibrating,
        LifecycleState::Routable,
    ] {
        assert!(apply(&mut s, transition(to)).is_err());
    }
    assert_eq!(
        s.discover_model(
            p("provider-a"),
            m("model-a"),
            observation(EvidenceConfidence::OfficialVerified)
        ),
        Err(RegistryError::DuplicateModel)
    );
    assert_eq!(state(&s), LifecycleState::Unassessed);
}

#[test]
fn lifecycle_verification_requires_both_official_capability_and_exact_compatible_path() {
    for model_confidence in EvidenceConfidence::ALL {
        for pair_confidence in EvidenceConfidence::ALL {
            for value in [
                None,
                Some(CapabilityValue::Unknown),
                Some(CapabilityValue::Boolean(false)),
                Some(CapabilityValue::Boolean(true)),
            ] {
                let mut s = service();
                add(&mut s, SubjectKind::Model, model_confidence);
                s.record_capability(assertion(
                    SubjectKind::ModelRuntimePair,
                    pair_confidence,
                    value,
                ))
                .unwrap();
                apply(&mut s, transition(LifecycleState::Unassessed)).unwrap();
                let result = apply(&mut s, transition(LifecycleState::CapabilityVerified));
                assert_eq!(
                    result.is_ok(),
                    model_confidence == EvidenceConfidence::OfficialVerified
                        && pair_confidence == EvidenceConfidence::OfficialVerified
                        && value == Some(CapabilityValue::Boolean(true))
                );
            }
        }
    }
    for missing_kind in [SubjectKind::Model, SubjectKind::ModelRuntimePair] {
        let mut s = service();
        for kind in [SubjectKind::Model, SubjectKind::ModelRuntimePair] {
            if kind != missing_kind {
                add(&mut s, kind, EvidenceConfidence::OfficialVerified);
            }
        }
        apply(&mut s, transition(LifecycleState::Unassessed)).unwrap();
        assert!(apply(&mut s, transition(LifecycleState::CapabilityVerified)).is_err());
    }
    for bad_path in [
        VerificationPath {
            runtime_id: r("Runtime-a"),
            required_capabilities: vec![c("coding")],
        },
        VerificationPath {
            runtime_id: r("runtime-a"),
            required_capabilities: vec![],
        },
        VerificationPath {
            runtime_id: r("runtime-a"),
            required_capabilities: vec![c("coding"), c("shell")],
        },
    ] {
        let mut s = evidenced();
        reach(&mut s, LifecycleState::Unassessed);
        let mut t = transition(LifecycleState::CapabilityVerified);
        t.evidence = LifecycleEvidence::CapabilityVerification(bad_path);
        assert!(apply(&mut s, t).is_err());
    }
}

#[test]
fn lifecycle_unknown_and_unsourced_evidence_never_become_compatible_or_verified() {
    for value in [
        None,
        Some(CapabilityValue::Unknown),
        Some(CapabilityValue::Boolean(false)),
    ] {
        let mut s = service();
        s.record_capability(assertion(
            SubjectKind::Model,
            EvidenceConfidence::OfficialVerified,
            value,
        ))
        .unwrap();
        add(
            &mut s,
            SubjectKind::ModelRuntimePair,
            EvidenceConfidence::OfficialVerified,
        );
        reach(&mut s, LifecycleState::Unassessed);
        assert!(apply(&mut s, transition(LifecycleState::CapabilityVerified)).is_err());
        assert_eq!(
            s.registry()
                .capability_evidence(&subject(SubjectKind::Model), "coding")
                .unwrap()[0]
                .value,
            value
        );
    }
    for kind in [SubjectKind::Model, SubjectKind::ModelRuntimePair] {
        let mut s = service();
        for k in [SubjectKind::Model, SubjectKind::ModelRuntimePair] {
            let mut e = assertion(
                k,
                EvidenceConfidence::OfficialVerified,
                Some(CapabilityValue::Boolean(true)),
            );
            if k == kind {
                e.observation.source_ref = None;
            }
            s.record_capability(e).unwrap();
        }
        reach(&mut s, LifecycleState::Unassessed);
        assert!(apply(&mut s, transition(LifecycleState::CapabilityVerified)).is_err());
    }
    let mut s = service();
    assert_eq!(
        s.registry().compatibility(
            &p("provider-a"),
            &m("model-a"),
            &r("runtime-a"),
            &c("coding")
        ),
        Compatibility::Unknown
    );
    s.record_capability(assertion(
        SubjectKind::ModelRuntimePair,
        EvidenceConfidence::OfficialVerified,
        Some(CapabilityValue::Unknown),
    ))
    .unwrap();
    assert_eq!(
        s.registry().compatibility(
            &p("provider-a"),
            &m("model-a"),
            &r("runtime-a"),
            &c("coding")
        ),
        Compatibility::Unknown
    );
}

#[test]
fn lifecycle_calibration_requires_explicit_bounded_admission_and_provenance() {
    let mut s = evidenced();
    reach(&mut s, LifecycleState::CapabilityVerified);
    assert_eq!(state(&s), LifecycleState::CapabilityVerified);
    let before = s.registry().clone();
    let mut t = transition(LifecycleState::Calibrating);
    t.evidence = LifecycleEvidence::BoundedCalibrationAdmission { authorized: false };
    assert_eq!(
        apply(&mut s, t),
        Err(RegistryError::Lifecycle(
            LifecycleError::BoundedAdmissionRequired
        ))
    );
    let mut t = transition(LifecycleState::Calibrating);
    t.observation.source_ref = None;
    assert_eq!(
        apply(&mut s, t),
        Err(RegistryError::Lifecycle(LifecycleError::MissingProvenance))
    );
    assert_eq!(&before, s.registry());
    apply(&mut s, transition(LifecycleState::Calibrating)).unwrap();
}

#[test]
fn lifecycle_local_routable_promotion_requires_observations_and_sufficiency_result() {
    for have_local in [false, true] {
        for sufficient in [false, true] {
            let mut s = service();
            add(
                &mut s,
                SubjectKind::Model,
                EvidenceConfidence::OfficialVerified,
            );
            add(
                &mut s,
                SubjectKind::ModelRuntimePair,
                EvidenceConfidence::OfficialVerified,
            );
            if have_local {
                add(
                    &mut s,
                    SubjectKind::ModelRuntimePair,
                    EvidenceConfidence::LocalEmpirical,
                );
            }
            reach(&mut s, LifecycleState::Calibrating);
            let mut t = transition(LifecycleState::Routable);
            t.evidence =
                LifecycleEvidence::RoutablePromotion(RoutablePromotionEvidence::LocalCalibration {
                    sufficient_acceptable_observations: sufficient,
                });
            assert_eq!(apply(&mut s, t).is_ok(), have_local && sufficient);
        }
    }
}

#[test]
fn lifecycle_alternative_routable_path_requires_official_independent_and_consent() {
    for official in [false, true] {
        for independent in [false, true] {
            for consent in [false, true] {
                let mut s = service();
                if official {
                    add(
                        &mut s,
                        SubjectKind::Model,
                        EvidenceConfidence::OfficialVerified,
                    );
                    add(
                        &mut s,
                        SubjectKind::ModelRuntimePair,
                        EvidenceConfidence::OfficialVerified,
                    );
                }
                if independent {
                    add(
                        &mut s,
                        SubjectKind::ModelRuntimePair,
                        EvidenceConfidence::IndependentVerified,
                    );
                }
                apply(&mut s, transition(LifecycleState::Unassessed)).unwrap();
                let verified = apply(&mut s, transition(LifecycleState::CapabilityVerified));
                if official {
                    verified.unwrap();
                    apply(&mut s, transition(LifecycleState::Calibrating)).unwrap();
                } else {
                    assert!(verified.is_err());
                }
                let mut t = transition(LifecycleState::Routable);
                t.evidence = LifecycleEvidence::RoutablePromotion(
                    RoutablePromotionEvidence::OfficialAndIndependent {
                        ask_on_uncertainty_user_consent: consent,
                    },
                );
                t.observation = observation(EvidenceConfidence::UserDeclared);
                assert_eq!(apply(&mut s, t).is_ok(), official && independent && consent);
            }
        }
    }
}

#[test]
fn lifecycle_side_states_require_matching_reasons_record_observations_and_never_recover() {
    for reason in [
        DemotionReason::VendorRetirement,
        DemotionReason::RepeatedRuntimeFailures,
        DemotionReason::SustainedRejectionRate,
        DemotionReason::AuthLoss,
        DemotionReason::TemporarilyUnreachable,
        DemotionReason::RateLimited,
        DemotionReason::UserBlock,
    ] {
        let mut s = evidenced();
        reach(&mut s, LifecycleState::Routable);
        let t = LifecycleTransition {
            target: reason.target(),
            evidence: LifecycleEvidence::Demotion(reason),
            observation: observation(EvidenceConfidence::UserDeclared),
        };
        apply(&mut s, t.clone()).unwrap();
        let record = s.registry().model("provider-a", "model-a").unwrap();
        assert!(!record.lifecycle_state().passes_normal_lifecycle_gate());
        assert_eq!(
            record.transitions().last(),
            Some(&TransitionRecord {
                from: LifecycleState::Routable,
                transition: t
            })
        );
        assert_eq!(record.verified_path(), Some(&path()));
        assert!(apply(&mut s, transition(LifecycleState::Routable)).is_err());
    }
    let mut s = evidenced();
    let mut t = transition(LifecycleState::UserBlocked);
    t.evidence = LifecycleEvidence::Demotion(DemotionReason::AuthLoss);
    assert_eq!(
        apply(&mut s, t),
        Err(RegistryError::Lifecycle(LifecycleError::IllegalTransition))
    );
}

#[test]
fn lifecycle_conflicting_observations_fail_closed_and_promotions_recheck_stored_path() {
    let mut s = evidenced();
    reach(&mut s, LifecycleState::CapabilityVerified);
    s.record_capability(assertion(
        SubjectKind::ModelRuntimePair,
        EvidenceConfidence::OfficialVerified,
        Some(CapabilityValue::Boolean(false)),
    ))
    .unwrap();
    assert_eq!(
        s.registry().compatibility(
            &p("provider-a"),
            &m("model-a"),
            &r("runtime-a"),
            &c("coding")
        ),
        Compatibility::Unsupported
    );
    assert_eq!(
        apply(&mut s, transition(LifecycleState::Calibrating)),
        Err(RegistryError::Lifecycle(
            LifecycleError::MissingRuntimeCompatibility
        ))
    );
    assert_eq!(state(&s), LifecycleState::CapabilityVerified);
    assert_eq!(
        s.registry()
            .capability_evidence(&subject(SubjectKind::ModelRuntimePair), "coding")
            .unwrap()
            .len(),
        4
    );
}
