use super::*;
use crate::policy_eligibility::{ModelRoutingDateTimeComparisonError, ModelRoutingDateTimeV1};
use crate::{EvidenceConfidence, EvidenceSourceRef, EvidenceSourceRefType};

fn id<T>(f: impl Fn(String) -> Result<T, EmptyIdentifier>, s: &str) -> T {
    f(s.into()).unwrap()
}
fn t(s: &str) -> ModelRoutingDateTimeV1 {
    ModelRoutingDateTimeV1::try_new(s.into()).unwrap()
}
fn source(tag: &str) -> Option<EvidenceSourceRef> {
    Some(
        EvidenceSourceRef::try_new(EvidenceSourceRefType::ArtifactId, tag.into(), None, None)
            .unwrap(),
    )
}
fn model(name: &str) -> CapabilitySubject {
    CapabilitySubject::Model {
        provider_id: id(ProviderId::try_new, "p"),
        model_id: id(ModelId::try_new, name),
    }
}
fn pair() -> CapabilitySubject {
    CapabilitySubject::ModelRuntimePair {
        provider_id: id(ProviderId::try_new, "p"),
        model_id: id(ModelId::try_new, "m"),
        runtime_id: id(RuntimeId::try_new, "r"),
    }
}
fn cap(name: &str) -> CapabilityId {
    id(CapabilityId::try_new, name)
}
fn ev(
    subject: CapabilitySubject,
    capability: &str,
    value: Option<CapabilityValue>,
    at: &str,
    src: Option<&str>,
) -> CapabilityEvidence {
    CapabilityEvidence {
        subject,
        capability: cap(capability),
        value,
        observation: Observation {
            confidence: EvidenceConfidence::Unverified,
            source_ref: src.and_then(source),
            observed_at: t(at),
        },
        sample_size: Some(Some(3)),
    }
}
fn yes() -> Option<CapabilityValue> {
    Some(CapabilityValue::Boolean(true))
}
fn no() -> Option<CapabilityValue> {
    Some(CapabilityValue::Boolean(false))
}

const NOW: &str = "2026-09-08T12:00:00Z";
const CUTOFF: &str = "2026-09-01T00:00:00Z";

fn service_with(records: Vec<CapabilityEvidence>) -> ModelIntelligenceService {
    let obs = |at| Observation {
        confidence: EvidenceConfidence::Unverified,
        source_ref: None,
        observed_at: t(at),
    };
    let mut s = ModelIntelligenceService::new();
    s.insert_provider(id(ProviderId::try_new, "p"), obs(NOW))
        .unwrap();
    for name in ["m", "other"] {
        s.discover_model(
            id(ProviderId::try_new, "p"),
            id(ModelId::try_new, name),
            obs(NOW),
        )
        .unwrap();
    }
    s.insert_runtime(
        id(ProviderId::try_new, "p"),
        id(RuntimeId::try_new, "r"),
        obs(NOW),
    )
    .unwrap();
    s.associate_runtime(
        id(ProviderId::try_new, "p"),
        id(ModelId::try_new, "m"),
        id(RuntimeId::try_new, "r"),
        obs(NOW),
    )
    .unwrap();
    for r in records {
        s.record_capability(r).unwrap();
    }
    s
}
fn resolve_at<'a>(
    s: &'a ModelIntelligenceService,
    subject: &CapabilitySubject,
    capability: &str,
    now: &str,
    cutoff: &str,
) -> Result<CapabilityResolution<'a>, ResolveError> {
    s.registry()
        .resolve_capability(subject, &cap(capability), &t(now), &t(cutoff))
}
fn resolve<'a>(
    s: &'a ModelIntelligenceService,
    subject: &CapabilitySubject,
) -> CapabilityResolution<'a> {
    resolve_at(s, subject, "c", NOW, CUTOFF).unwrap()
}

#[test]
fn exact_subject_and_capability_with_no_fallback() {
    let s = service_with(vec![
        ev(model("m"), "c", yes(), "2026-09-05T00:00:00Z", Some("a")),
        ev(
            pair(),
            "other-cap",
            yes(),
            "2026-09-05T00:00:00Z",
            Some("a"),
        ),
    ]);
    assert!(matches!(
        resolve(&s, &model("m")).outcome,
        CapabilityOutcome::Established { value: true, .. }
    ));
    // pair has no "c" evidence: model evidence is not borrowed; nor other model/capability
    for subject in [pair(), model("other")] {
        assert_eq!(
            resolve(&s, &subject).outcome,
            CapabilityOutcome::NoUsableEvidence
        );
    }
    assert_eq!(
        resolve_at(&s, &model("m"), "other-cap", NOW, CUTOFF)
            .unwrap()
            .outcome,
        CapabilityOutcome::NoUsableEvidence
    );
}

#[test]
fn newer_supersedes_independent_of_insertion_order_and_offsets() {
    let old = ev(model("m"), "c", yes(), "2026-09-03T00:00:00Z", Some("old"));
    // 2026-09-04T00:00Z written with offset and fraction
    let new = ev(
        model("m"),
        "c",
        no(),
        "2026-09-03T20:00:00.50-04:00",
        Some("new"),
    );
    for order in [
        vec![old.clone(), new.clone()],
        vec![new.clone(), old.clone()],
    ] {
        let s = service_with(order);
        let r = resolve(&s, &model("m"));
        assert_eq!(
            r.outcome,
            CapabilityOutcome::Established {
                value: false,
                evidence: vec![&new]
            }
        );
        assert!(r.rejected.is_empty());
    }
}

#[test]
fn equal_instant_agreement_keeps_all_provenance() {
    let a = ev(model("m"), "c", yes(), "2026-09-04T00:00:00Z", Some("a"));
    let b = ev(
        model("m"),
        "c",
        yes(),
        "2026-09-03T20:00:00.000-04:00",
        Some("b"),
    );
    let s = service_with(vec![a.clone(), b.clone()]);
    assert_eq!(
        resolve(&s, &model("m")).outcome,
        CapabilityOutcome::Established {
            value: true,
            evidence: vec![&a, &b]
        }
    );
}

#[test]
fn equal_instant_conflict_in_both_orders() {
    let a = ev(model("m"), "c", yes(), "2026-09-04T00:00:00Z", Some("a"));
    let b = ev(
        model("m"),
        "c",
        no(),
        "2026-09-04T00:00:00+00:00",
        Some("b"),
    );
    for (first, second) in [(&a, &b), (&b, &a)] {
        let s = service_with(vec![first.clone(), second.clone()]);
        assert_eq!(
            resolve(&s, &model("m")).outcome,
            CapabilityOutcome::Conflicted {
                evidence: vec![first, second]
            }
        );
    }
}

#[test]
fn unknown_missing_and_unsourced_never_succeed() {
    let unknown = ev(
        model("m"),
        "c",
        Some(CapabilityValue::Unknown),
        "2026-09-05T00:00:00Z",
        Some("a"),
    );
    let omitted = ev(model("m"), "c", None, "2026-09-05T00:00:00Z", Some("b"));
    let older_true = ev(model("m"), "c", yes(), "2026-09-04T00:00:00Z", Some("c"));
    // newer UNKNOWN supersedes older true
    for u in [&unknown, &omitted] {
        let s = service_with(vec![older_true.clone(), u.clone()]);
        assert_eq!(
            resolve(&s, &model("m")).outcome,
            CapabilityOutcome::Unknown { evidence: vec![u] }
        );
    }
    // UNKNOWN beside true at the same instant is a conflict, not success
    let same = ev(model("m"), "c", yes(), "2026-09-05T00:00:00Z", Some("d"));
    let s = service_with(vec![same, unknown]);
    assert!(matches!(
        resolve(&s, &model("m")).outcome,
        CapabilityOutcome::Conflicted { .. }
    ));
    // unsourced is rejected, leaving nothing usable
    let unsourced = ev(model("m"), "c", yes(), "2026-09-05T00:00:00Z", None);
    let s = service_with(vec![unsourced.clone()]);
    let r = resolve(&s, &model("m"));
    assert_eq!(r.outcome, CapabilityOutcome::NoUsableEvidence);
    assert_eq!(r.rejected[0].reason, RejectionReason::Unsourced);
    // no records at all
    assert_eq!(
        resolve(&service_with(vec![]), &model("m")).outcome,
        CapabilityOutcome::NoUsableEvidence
    );
}

#[test]
fn freshness_boundaries_classify_selected_evidence() {
    for (at, freshness) in [
        ("2026-08-31T23:59:59.999Z", Freshness::Stale),
        ("2026-09-01T00:00:00Z", Freshness::Fresh),
        ("2026-08-31T20:00:00-04:00", Freshness::Fresh), // equals cutoff
        ("2026-09-08T12:00:00Z", Freshness::Fresh),      // equals now
    ] {
        let e = ev(model("m"), "c", yes(), at, Some("a"));
        let s = service_with(vec![e.clone()]);
        let r = resolve(&s, &model("m"));
        // stale evidence is still selected, never labeled fresh or dropped
        assert_eq!(
            r.outcome,
            CapabilityOutcome::Established {
                value: true,
                evidence: vec![&e]
            }
        );
        assert_eq!(r.freshness, Some(freshness), "{at}");
        assert!(r.rejected.is_empty());
    }
}

#[test]
fn stale_latest_keeps_agreement_and_conflict() {
    let a = ev(model("m"), "c", yes(), "2026-08-01T00:00:00Z", Some("a"));
    let b = ev(
        model("m"),
        "c",
        no(),
        "2026-07-31T20:00:00-04:00",
        Some("b"),
    );
    let older = ev(model("m"), "c", yes(), "2026-07-01T00:00:00Z", Some("o"));
    let s = service_with(vec![a.clone(), b.clone(), older]);
    let r = resolve(&s, &model("m"));
    assert_eq!(
        r.outcome,
        CapabilityOutcome::Conflicted {
            evidence: vec![&a, &b]
        }
    );
    assert_eq!(r.freshness, Some(Freshness::Stale));
    assert!(r.rejected.is_empty());
}

#[test]
fn cutoff_after_now_is_refused_even_without_evidence() {
    let s = service_with(vec![]);
    let later = "2026-09-08T12:00:00.001Z";
    assert_eq!(
        resolve_at(&s, &model("m"), "c", NOW, later).map(|_| ()),
        Err(ResolveError::CutoffAfterNow)
    );
    // cutoff == now (equivalent offset) is valid
    let r = resolve_at(&s, &model("m"), "c", NOW, "2026-09-08T08:00:00-04:00").unwrap();
    assert_eq!(
        (r.outcome, r.freshness),
        (CapabilityOutcome::NoUsableEvidence, None)
    );
}

#[test]
fn future_evidence_is_rejected_and_never_overrides_older() {
    let older = ev(model("m"), "c", yes(), "2026-09-05T00:00:00Z", Some("old"));
    let future = ev(
        model("m"),
        "c",
        no(),
        "2026-09-08T12:00:00.001Z",
        Some("fut"),
    );
    let s = service_with(vec![future.clone(), older.clone()]);
    let r = resolve(&s, &model("m"));
    assert_eq!(
        r.outcome,
        CapabilityOutcome::Established {
            value: true,
            evidence: vec![&older]
        }
    );
    assert_eq!(
        r.rejected,
        vec![RejectedEvidence {
            evidence: &future,
            reason: RejectionReason::Future
        }]
    );
    // all-future: no usable capability, reason kept
    let s = service_with(vec![future]);
    let r = resolve(&s, &model("m"));
    assert_eq!(r.outcome, CapabilityOutcome::NoUsableEvidence);
    assert_eq!(r.rejected[0].reason, RejectionReason::Future);
    // equal to now (different offset spelling) is eligible
    let s = service_with(vec![ev(
        model("m"),
        "c",
        yes(),
        "2026-09-08T08:00:00-04:00",
        Some("n"),
    )]);
    assert!(matches!(
        resolve(&s, &model("m")).outcome,
        CapabilityOutcome::Established { .. }
    ));
}

#[test]
fn leap_second_fails_closed_everywhere() {
    let leap = Err(ResolveError::Comparison(
        ModelRoutingDateTimeComparisonError::UnsupportedLeapSecond,
    ));
    let good = ev(model("m"), "c", yes(), "2026-09-05T00:00:00Z", Some("a"));
    let s = service_with(vec![good.clone()]);
    assert_eq!(
        resolve_at(&s, &model("m"), "c", "2026-09-08T23:59:60Z", CUTOFF).map(|_| ()),
        leap
    );
    assert_eq!(
        resolve_at(&s, &model("m"), "c", NOW, "2026-08-31T23:59:60Z").map(|_| ()),
        leap
    );
    let bad = ev(model("m"), "c", no(), "2026-09-06T23:59:60Z", Some("b"));
    let s = service_with(vec![good, bad]);
    assert_eq!(
        resolve_at(&s, &model("m"), "c", NOW, CUTOFF).map(|_| ()),
        leap
    );
}
