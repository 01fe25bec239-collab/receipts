//! All nonempty records in this module and its JSON fixture are synthetic.
use super::*;
use crate::policy_eligibility::{ModelRoutingDateTimeComparisonError, ModelRoutingDateTimeV1};
use crate::{EvidenceConfidence, EvidenceSourceRefType, intelligence::LifecycleState};
use serde_json::{Value, json};

const FIXTURE: &str = include_str!("seed/synthetic-test-v1.json");
const P: &str = " synthetic-provider ☃ ";
const M: &str = "synthetic-model/α";
const R: &str = "synthetic-runtime";
const C: &str = "synthetic-capability";
fn seed() -> Value {
    serde_json::from_str(FIXTURE).unwrap()
}
fn load(v: &Value) -> Result<ModelIntelligenceService, SeedLoadError> {
    load_registry_seed(serde_json::to_vec(v).unwrap().as_slice())
}
fn subject() -> CapabilitySubject {
    CapabilitySubject::ModelRuntimePair {
        provider_id: ProviderId::try_new(P.into()).unwrap(),
        model_id: ModelId::try_new(M.into()).unwrap(),
        runtime_id: RuntimeId::try_new(R.into()).unwrap(),
    }
}
fn t(s: &str) -> ModelRoutingDateTimeV1 {
    ModelRoutingDateTimeV1::try_new(s.into()).unwrap()
}
fn resolve<'a>(
    s: &'a ModelIntelligenceService,
    now: &str,
    cutoff: &str,
) -> Result<CapabilityResolution<'a>, ResolveError> {
    s.registry().resolve_capability(
        &subject(),
        &CapabilityId::try_new(C.into()).unwrap(),
        &t(now),
        &t(cutoff),
    )
}
#[test]
fn file_to_registry_to_resolver_preserves_provenance_and_never_promotes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/registry/seed/synthetic-test-v1.json");
    let s = load_registry_seed(std::fs::File::open(path).unwrap()).unwrap();
    let registry = s.registry();
    let expected = registry.provider(P).unwrap().discovery();
    assert_eq!(expected.confidence, EvidenceConfidence::Unverified);
    assert_eq!(expected.observed_at.as_str(), "2026-09-01t00:00:00.000z");
    let source = expected.source_ref.as_ref().unwrap();
    assert_eq!(source.ref_type(), EvidenceSourceRefType::ArtifactId);
    assert_eq!(source.target(), "synthetic-test-only/☃");
    assert_eq!(source.digest(), Some(""));
    assert_eq!(source.section(), Some(" synthetic section "));
    let model = registry.model(P, M).unwrap();
    assert_eq!(model.lifecycle_state(), LifecycleState::Discovered);
    assert!(model.transitions().is_empty());
    assert!(model.verified_path().is_none());
    for obs in [
        model.discovery(),
        registry.runtime(P, R).unwrap().discovery(),
        registry.association(P, M, R).unwrap().observation(),
    ] {
        assert_eq!(obs, expected);
    }
    let evidence = &registry.capability_evidence(&subject(), C).unwrap()[0];
    assert_eq!(&evidence.observation, expected);
    assert_eq!(evidence.sample_size, Some(Some(u64::MAX)));
    let result = resolve(&s, "2026-09-01T00:00:00Z", "2026-08-31T20:00:00-04:00").unwrap();
    assert_eq!(result.freshness, Some(Freshness::Fresh));
    assert_eq!(
        result.outcome,
        CapabilityOutcome::Established {
            value: true,
            evidence: vec![evidence]
        }
    );
    assert!(registry.provider(P.trim()).is_none());
    assert!(registry.model(P, "Synthetic-model/α").is_none());
    assert_eq!(
        load_registry_seed(include_bytes!("seed/v1.json").as_slice())
            .unwrap()
            .registry()
            .providers()
            .count(),
        0
    );
}
#[test]
fn loaded_values_samples_and_all_provenance_vocabularies_roundtrip() {
    for confidence in EvidenceConfidence::ALL {
        for ref_type in EvidenceSourceRefType::ALL {
            for (value, expected) in [
                (None, None),
                (Some(Value::Null), Some(CapabilityValue::Unknown)),
                (Some(json!("UNKNOWN")), Some(CapabilityValue::Unknown)),
                (Some(json!(true)), Some(CapabilityValue::Boolean(true))),
                (Some(json!(false)), Some(CapabilityValue::Boolean(false))),
            ] {
                for (sample, expected_sample) in [
                    (None, None),
                    (Some(Value::Null), Some(None)),
                    (Some(json!(0)), Some(Some(0))),
                    (Some(json!(u64::MAX)), Some(Some(u64::MAX))),
                ] {
                    let mut v = seed();
                    let e = &mut v["capabilities"][0];
                    e.as_object_mut().unwrap().remove("value");
                    e.as_object_mut().unwrap().remove("sample_size");
                    if let Some(value) = value.clone() {
                        e["value"] = value;
                    }
                    if let Some(sample) = sample {
                        e["sample_size"] = sample;
                    }
                    e["observation"]["confidence"] = json!(confidence.as_str());
                    e["observation"]["source_ref"]["ref_type"] = json!(ref_type.as_str());
                    let s = load(&v).unwrap();
                    let e = &s.registry().capability_evidence(&subject(), C).unwrap()[0];
                    assert_eq!(e.value, expected);
                    assert_eq!(e.sample_size, expected_sample);
                    assert_eq!(e.observation.confidence, confidence);
                    assert_eq!(
                        e.observation.source_ref.as_ref().unwrap().ref_type(),
                        ref_type
                    );
                }
            }
        }
    }
}
#[test]
fn seed_without_provenance_rejected_on_every_record_kind() {
    for collection in [
        "providers",
        "models",
        "runtimes",
        "associations",
        "capabilities",
    ] {
        for field in ["confidence", "source_ref", "observed_at"] {
            for remove in [true, false] {
                let mut v = seed();
                let obs = v[collection][0]["observation"].as_object_mut().unwrap();
                if remove {
                    obs.remove(field);
                } else {
                    obs.insert(field.into(), Value::Null);
                }
                assert!(load(&v).is_err(), "{collection}.{field} remove={remove}");
            }
        }
    }
}
#[test]
fn malformed_values_and_numbers_are_refused_without_coercion() {
    for raw in [
        "-1",
        "-0",
        "1.0",
        "1e0",
        "1.5",
        "18446744073709551616",
        "184467440737095516150",
        "1e400",
        "true",
        "\"3\"",
        "[]",
        "{}",
    ] {
        let input = FIXTURE.replace("18446744073709551615", raw);
        assert!(
            load_registry_seed(input.as_bytes()).is_err(),
            "sample_size={raw}"
        );
    }
    for value in [
        json!(0),
        json!(1.0),
        json!("true"),
        json!("unknown"),
        json!([]),
        json!({}),
        json!({"UNKNOWN":null}),
    ] {
        let mut v = seed();
        v["capabilities"][0]["value"] = value;
        assert!(load(&v).is_err());
    }
    for (pointer, value) in [
        ("/version", json!(2)),
        ("/version", json!("1")),
        ("/models/0/model_id", json!("")),
        ("/providers/0/provider_id", json!("")),
        ("/runtimes/0/runtime_id", json!("")),
        ("/capabilities/0/capability", json!("")),
        ("/capabilities/0/observation/confidence", json!("verified")),
        ("/providers/0/observation/source_ref/target", json!("")),
        (
            "/providers/0/observation/source_ref/ref_type",
            json!("OTHER"),
        ),
        (
            "/providers/0/observation/observed_at",
            json!("2026-02-30T00:00:00Z"),
        ),
    ] {
        let mut v = seed();
        *v.pointer_mut(pointer).unwrap() = value;
        assert!(load(&v).is_err(), "{pointer}");
    }
    for raw in ["", "null", "{}", "[]", "{", "{} {}"] {
        assert!(load_registry_seed(raw.as_bytes()).is_err());
    }
    let mut v = seed();
    v["extra"] = json!(true);
    assert!(load(&v).is_err());
}
#[test]
fn duplicate_json_fields_including_nulls_are_refused() {
    for (old, new) in [
        ("\"version\": 1", "\"version\": 1, \"version\": 1"),
        (
            "\"kind\": \"MODEL_RUNTIME_PAIR\"",
            "\"kind\": \"MODEL_RUNTIME_PAIR\", \"kind\": \"MODEL_RUNTIME_PAIR\"",
        ),
        ("\"value\": true", "\"value\": null, \"value\": true"),
        (
            "\"sample_size\": 18446744073709551615",
            "\"sample_size\": null, \"sample_size\": 1",
        ),
        (
            "\"provider_id\":",
            "\"provider_id\": \"duplicate\", \"provider_id\":",
        ),
        (
            "\"confidence\":",
            "\"confidence\": \"UNVERIFIED\", \"confidence\":",
        ),
        ("\"target\":", "\"target\": \"synthetic\", \"target\":"),
        ("\"digest\": \"\"", "\"digest\": null, \"digest\": \"\""),
        ("\"providers\": [", "\"providers\": [], \"providers\": ["),
    ] {
        assert!(FIXTURE.contains(old));
        assert!(
            load_registry_seed(FIXTURE.replace(old, new).as_bytes()).is_err(),
            "{new}"
        );
    }
    // Duplicate subject fields must also refuse inside serde's tagged variant.
    let input = FIXTURE.replacen(
        "\"kind\": \"MODEL_RUNTIME_PAIR\",",
        "\"kind\": \"MODEL_RUNTIME_PAIR\", \"model_id\": \"synthetic-model/α\",",
        1,
    );
    assert!(load_registry_seed(input.as_bytes()).is_err());
}
#[test]
fn duplicate_records_and_dangling_or_cross_subject_references_refuse_atomically() {
    let existing = load(&seed()).unwrap();
    let before = existing.registry().clone();
    for collection in [
        "providers",
        "models",
        "runtimes",
        "associations",
        "capabilities",
    ] {
        let mut v = seed();
        let duplicate = v[collection][0].clone();
        v[collection].as_array_mut().unwrap().push(duplicate);
        assert!(
            matches!(load(&v), Err(SeedLoadError::Registry(_))),
            "{collection}"
        );
        assert_eq!(existing.registry(), &before);
    }
    for pointer in [
        "/models/0/provider_id",
        "/runtimes/0/provider_id",
        "/associations/0/provider_id",
        "/associations/0/model_id",
        "/associations/0/runtime_id",
        "/capabilities/0/subject/provider_id",
        "/capabilities/0/subject/model_id",
        "/capabilities/0/subject/runtime_id",
    ] {
        let mut v = seed();
        *v.pointer_mut(pointer).unwrap() = json!("missing");
        assert!(
            matches!(load(&v), Err(SeedLoadError::Registry(_))),
            "{pointer}"
        );
        assert_eq!(existing.registry(), &before);
    }
    let mut v = seed();
    let mut other = v["providers"][0].clone();
    other["provider_id"] = json!("other-synthetic-provider");
    v["providers"].as_array_mut().unwrap().push(other);
    v["capabilities"][0]["subject"]["provider_id"] = json!("other-synthetic-provider");
    assert!(load(&v).is_err());
    for kind in ["MODEL", "RUNTIME"] {
        let mut v = seed();
        v["capabilities"][0]["subject"]["kind"] = json!(kind);
        assert!(load(&v).is_err()); // mismatched extra subject identity
        let remove = if kind == "MODEL" {
            "runtime_id"
        } else {
            "model_id"
        };
        v["capabilities"][0]["subject"]
            .as_object_mut()
            .unwrap()
            .remove(remove);
        assert!(load(&v).is_ok());
    }
}
#[test]
fn loaded_conflicts_future_stale_offsets_and_temporal_refusals_use_existing_resolver() {
    let mut v = seed();
    let mut b = v["capabilities"][0].clone();
    b["value"] = json!(false);
    b["observation"]["observed_at"] = json!("2026-08-31T20:00:00-04:00");
    v["capabilities"].as_array_mut().unwrap().push(b.clone());
    b["observation"]["observed_at"] = json!("2026-10-01T00:00:00Z");
    v["capabilities"].as_array_mut().unwrap().push(b);
    let s = load(&v).unwrap();
    let result = resolve(&s, "2026-09-02T00:00:00Z", "2026-09-01T00:00:00Z").unwrap();
    assert!(
        matches!(result.outcome, CapabilityOutcome::Conflicted {ref evidence} if evidence.len()==2)
    );
    assert_eq!(result.freshness, Some(Freshness::Fresh));
    assert_eq!(result.rejected.len(), 1);
    assert_eq!(result.rejected[0].reason, RejectionReason::Future);
    assert_eq!(
        resolve(&s, "2026-09-03T00:00:00Z", "2026-09-02T00:00:00Z")
            .unwrap()
            .freshness,
        Some(Freshness::Stale)
    );
    assert_eq!(
        resolve(&s, "2026-09-01T00:00:00Z", "2026-09-02T00:00:00Z"),
        Err(ResolveError::CutoffAfterNow)
    );
    for (now, cutoff) in [
        ("2026-09-08T23:59:60Z", "2026-09-01T00:00:00Z"),
        ("2026-09-08T00:00:00Z", "2026-08-31T23:59:60Z"),
    ] {
        assert_eq!(
            resolve(&s, now, cutoff),
            Err(ResolveError::Comparison(
                ModelRoutingDateTimeComparisonError::UnsupportedLeapSecond
            ))
        );
    }
    v["capabilities"][0]["observation"]["observed_at"] = json!("2026-08-31T23:59:60Z");
    assert!(matches!(
        resolve(
            &load(&v).unwrap(),
            "2026-09-08T00:00:00Z",
            "2026-09-01T00:00:00Z"
        ),
        Err(ResolveError::Comparison(_))
    ));
}
#[test]
fn no_model_literal_in_routing_control_flow() {
    fn scan(dir: &std::path::Path) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path);
            } else if path.extension().is_some_and(|s| s == "rs") {
                let code = std::fs::read_to_string(&path).unwrap();
                for id in [P, M, R, C] {
                    assert!(
                        !code.contains(id),
                        "data identifier {id} in {}",
                        path.display()
                    );
                }
            }
        }
    }
    scan(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src/routing"));
}
