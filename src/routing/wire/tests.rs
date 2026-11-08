//! MR-WIRE conformance: every valid fixture round-trips exactly, every
//! negative fixture is refused with its expected class, and each codec is
//! bound to the actual frozen schema file (property set, required set and
//! every enum value).

use std::path::PathBuf;

use serde_json::Value;

use super::*;

type Codec = fn(&str) -> Result<String, RoutingWireError>;

fn codec(schema: &str) -> Codec {
    match schema {
        "RoutingRequest" => |j| decode_routing_request(j).map(|v| encode_routing_request(&v)),
        "RoutingDecision" => |j| decode_routing_decision(j).map(|v| encode_routing_decision(&v)),
        "AvailabilityState" => {
            |j| decode_availability_state(j).map(|v| encode_availability_state(&v))
        }
        "QuotaState" => |j| decode_quota_state(j).map(|v| encode_quota_state(&v)),
        "ModelCapability" => |j| decode_model_capability(j).map(|v| encode_model_capability(&v)),
        "ModelObservation" => |j| decode_model_observation(j).map(|v| encode_model_observation(&v)),
        "ProviderPolicyEligibility" => |j| {
            decode_provider_policy_eligibility(j).map(|v| encode_provider_policy_eligibility(&v))
        },
        other => panic!("no codec for {other}"),
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn fixtures(schema: &str) -> Vec<(String, String)> {
    let dir = root().join("src/routing/wire/fixtures");
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.starts_with(&format!("{schema}.valid.")))
        .map(|n| (n.clone(), read(dir.join(&n))))
        .collect();
    out.sort();
    assert!(out.len() >= 3, "{schema}: full, minimal and nulls fixtures");
    out
}

fn canonical(json: &str) -> String {
    serde_json::from_str::<Value>(json).unwrap().to_string()
}

fn roundtrip(schema: &str) {
    let c = codec(schema);
    for (name, json) in fixtures(schema) {
        let encoded = c(&json).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(encoded, canonical(&json), "{name}: exact round trip");
        assert_eq!(c(&encoded).unwrap(), encoded, "{name}: idempotent");
    }
}

fn negatives(schema: &str) {
    let c = codec(schema);
    let cases: Vec<Value> = serde_json::from_str(&read(
        root().join(format!("src/routing/wire/fixtures/{schema}.negative.json")),
    ))
    .unwrap();
    assert!(cases.len() >= 8, "{schema}: negative coverage");
    for case in cases {
        let name = case["case"].as_str().unwrap();
        let input = match case.get("raw") {
            Some(raw) => raw.as_str().unwrap().to_owned(),
            None => case["document"].to_string(),
        };
        let expected = case["error"].as_str().unwrap();
        let actual = match c(&input) {
            Ok(_) => panic!("{schema}/{name}: accepted"),
            Err(RoutingWireError::Json) => "Json",
            Err(RoutingWireError::Shape(_)) => "Shape",
            Err(RoutingWireError::Refused(_)) => "Refused",
        };
        assert_eq!(actual, expected, "{schema}/{name}");
    }
}

/// Fields the operational carrier requires although the schema does not.
fn carrier_required(schema: &str) -> &'static [&'static str] {
    match schema {
        "RoutingDecision" => &["occurred_at"],
        _ => &[],
    }
}

fn schema_binding(schema: &str) {
    let c = codec(schema);
    let file: Value =
        serde_json::from_str(&read(root().join(format!("schemas/{schema}.schema.json")))).unwrap();
    let properties = file["properties"].as_object().unwrap();
    let required: Vec<&str> = file["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let full: Value = serde_json::from_str(&read(root().join(format!(
        "src/routing/wire/fixtures/{schema}.valid.full.json"
    ))))
    .unwrap();
    let full = full.as_object().unwrap();
    let mut keys: Vec<_> = full.keys().collect();
    let mut schema_keys: Vec<_> = properties.keys().collect();
    keys.sort();
    schema_keys.sort();
    assert_eq!(
        keys, schema_keys,
        "{schema}: full fixture covers every property"
    );
    for key in properties.keys() {
        let mut without = full.clone();
        without.remove(key);
        let result = c(&Value::Object(without).to_string());
        if required.contains(&key.as_str()) {
            assert_eq!(
                result,
                Err(RoutingWireError::Shape(schema_name(schema))),
                "{schema}.{key}"
            );
        } else if carrier_required(schema).contains(&key.as_str()) {
            assert!(
                matches!(result, Err(RoutingWireError::Refused(_))),
                "{schema}.{key}"
            );
        } else {
            assert!(
                result.is_ok(),
                "{schema}: optional {key} may be absent: {result:?}"
            );
        }
        if let Some(values) = properties[key].get("enum").and_then(Value::as_array) {
            for value in values {
                let mut with = full.clone();
                with.insert(key.clone(), value.clone());
                assert!(
                    c(&Value::Object(with).to_string()).is_ok(),
                    "{schema}.{key}={value}"
                );
            }
            let mut bad = full.clone();
            bad.insert(key.clone(), Value::String("NOT_IN_SCHEMA_ENUM".into()));
            assert!(
                matches!(
                    c(&Value::Object(bad).to_string()),
                    Err(RoutingWireError::Shape(_))
                ),
                "{schema}.{key}"
            );
        }
    }
    let mut unknown = full.clone();
    unknown.insert("not_in_schema".into(), Value::Bool(true));
    assert_eq!(
        c(&Value::Object(unknown).to_string()),
        Err(RoutingWireError::Shape(schema_name(schema)))
    );
}

fn schema_name(schema: &str) -> &'static str {
    [REQ, DEC, AVA, QUO, CAP, OBS, POL]
        .into_iter()
        .find(|s| *s == schema)
        .unwrap()
}

macro_rules! conformance {
    ($($schema:literal => $valid:ident, $negative:ident, $binding:ident;)*) => {$(
        #[test]
        fn $valid() {
            roundtrip($schema);
        }
        #[test]
        fn $negative() {
            negatives($schema);
        }
        #[test]
        fn $binding() {
            schema_binding($schema);
        }
    )*};
}

conformance! {
    "RoutingRequest" => routing_request_valid_fixture_roundtrips, routing_request_negative_fixture_rejected, routing_request_codec_matches_schema_file;
    "RoutingDecision" => routing_decision_valid_fixture_roundtrips, routing_decision_negative_fixture_rejected, routing_decision_codec_matches_schema_file;
    "AvailabilityState" => availability_state_valid_fixture_roundtrips, availability_state_negative_fixture_rejected, availability_state_codec_matches_schema_file;
    "QuotaState" => quota_state_valid_fixture_roundtrips, quota_state_negative_fixture_rejected, quota_state_codec_matches_schema_file;
    "ModelCapability" => model_capability_valid_fixture_roundtrips, model_capability_negative_fixture_rejected, model_capability_codec_matches_schema_file;
    "ModelObservation" => model_observation_valid_fixture_roundtrips, model_observation_negative_fixture_rejected, model_observation_codec_matches_schema_file;
    "ProviderPolicyEligibility" => provider_policy_eligibility_valid_fixture_roundtrips, provider_policy_eligibility_negative_fixture_rejected, provider_policy_eligibility_codec_matches_schema_file;
}

#[test]
fn absent_and_null_stay_distinct_in_operational_values() {
    let full = decode_routing_request(&read(
        root().join("src/routing/wire/fixtures/RoutingRequest.valid.full.json"),
    ))
    .unwrap();
    let nulls = decode_routing_request(&read(
        root().join("src/routing/wire/fixtures/RoutingRequest.valid.nulls.json"),
    ))
    .unwrap();
    let minimal = decode_routing_request(&read(
        root().join("src/routing/wire/fixtures/RoutingRequest.valid.minimal.json"),
    ))
    .unwrap();
    assert_eq!(full.core().context_size_hint(), Some(Some(200_000)));
    assert_eq!(nulls.core().context_size_hint(), Some(None));
    assert_eq!(minimal.core().context_size_hint(), None);
    assert_eq!(nulls.deadline(), Some(None));
    assert_eq!(minimal.deadline(), None);
    assert_eq!(nulls.core().constraints().unwrap().max_cost(), Some(None));
    assert_eq!(
        full.core().constraints().unwrap().max_cost(),
        Some(Some(12.5))
    );
    assert_eq!(nulls.core().preferred_capabilities(), Some(&[][..]));

    let capability = decode_model_capability(&read(
        root().join("src/routing/wire/fixtures/ModelCapability.valid.nulls.json"),
    ))
    .unwrap();
    assert_eq!(capability.value(), Some(&Value::Null));
    assert_eq!(
        capability.to_evidence().unwrap().value,
        Some(crate::registry::CapabilityValue::Unknown),
        "null is UNKNOWN, never a default"
    );
    let structured = decode_model_capability(&read(
        root().join("src/routing/wire/fixtures/ModelCapability.valid.structured.json"),
    ))
    .unwrap();
    assert_eq!(
        structured.to_evidence(),
        Err(crate::registry::ModelCapabilityRecordError::LifecycleRow)
    );
}

#[test]
fn quota_scope_unknown_unless_verified() {
    let decode = |visibility: Option<&str>| {
        let mut doc: Value = serde_json::from_str(&read(
            root().join("src/routing/wire/fixtures/QuotaState.valid.full.json"),
        ))
        .unwrap();
        match visibility {
            Some(v) => doc["visibility"] = Value::String(v.into()),
            None => {
                doc.as_object_mut().unwrap().remove("visibility");
            }
        }
        decode_quota_state(&doc.to_string()).unwrap()
    };
    assert_eq!(
        decode(Some("HEADERS")).verified_scope(),
        QuotaScope::Provider
    );
    for unverified in [Some("LOCAL_APPROX"), Some("NONE"), Some("UNKNOWN"), None] {
        let q = decode(unverified);
        assert_eq!(
            q.core().quota_scope(),
            QuotaScope::Provider,
            "stored as supplied"
        );
        assert_eq!(q.verified_scope(), QuotaScope::Unknown, "{unverified:?}");
    }
}

#[test]
fn routing_codecs_carry_no_model_literals_or_numbers_not_in_input() {
    // Encoding a decoded minimal record never invents a field.
    for schema in [
        "RoutingRequest",
        "RoutingDecision",
        "AvailabilityState",
        "QuotaState",
        "ModelCapability",
        "ModelObservation",
        "ProviderPolicyEligibility",
    ] {
        let minimal = read(root().join(format!(
            "src/routing/wire/fixtures/{schema}.valid.minimal.json"
        )));
        let encoded: Value = serde_json::from_str(&codec(schema)(&minimal).unwrap()).unwrap();
        let input: Value = serde_json::from_str(&minimal).unwrap();
        assert_eq!(encoded, input, "{schema}");
    }
}
