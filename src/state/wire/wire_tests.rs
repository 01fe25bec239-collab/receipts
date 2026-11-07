//! Owned conformance cases for the six State codecs. The frozen corpus has
//! no State fixtures; these cases are generated here and checked against
//! the pinned offline `jsonschema` oracle over the immutable root schemas.
use super::*;
use crate::error::StateError;

fn schema(name: &str) -> serde_json::Value {
    serde_json::from_str(match name {
        "LogicalRole" => include_str!("../../../schemas/LogicalRole.schema.json"),
        "ExecutorBinding" => include_str!("../../../schemas/ExecutorBinding.schema.json"),
        "ContextManifest" => include_str!("../../../schemas/ContextManifest.schema.json"),
        "ContextEpoch" => include_str!("../../../schemas/ContextEpoch.schema.json"),
        "ProductEntitlement" => include_str!("../../../schemas/ProductEntitlement.schema.json"),
        "ActivationState" => include_str!("../../../schemas/ActivationState.schema.json"),
        _ => unreachable!(),
    })
    .unwrap()
}

type Codec = fn(&str) -> Result<String, StateError>;

fn codec(name: &str) -> Codec {
    match name {
        "LogicalRole" => |t| LogicalRoleWireV1::parse_json(t)?.to_json(),
        "ExecutorBinding" => |t| ExecutorBindingWireV1::parse_json(t)?.to_json(),
        "ContextManifest" => |t| ContextManifestWireV1::parse_json(t)?.to_json(),
        "ContextEpoch" => |t| ContextEpochWireV1::parse_json(t)?.to_json(),
        "ProductEntitlement" => |t| ProductEntitlementWireV1::parse_json(t)?.to_json(),
        "ActivationState" => |t| ActivationStateWireV1::parse_json(t)?.to_json(),
        _ => unreachable!(),
    }
}

fn value(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap()
}

/// Valid cases round-trip to the same JSON value and a stable encoding;
/// invalid cases are refused by both the codec and the oracle.
fn conformance(name: &str, valid: &[String], invalid: &[String]) {
    let validator = jsonschema::validator_for(&schema(name)).unwrap();
    let codec = codec(name);
    for text in valid {
        assert!(
            validator.is_valid(&value(text)),
            "{name} oracle refused {text}"
        );
        let encoded = codec(text).unwrap_or_else(|e| panic!("{name} refused {text}: {e}"));
        assert_eq!(value(&encoded), value(text), "{name} changed {text}");
        assert_eq!(
            codec(&encoded).unwrap(),
            encoded,
            "{name} encoding unstable"
        );
    }
    for text in invalid {
        let oracle_accepts =
            serde_json::from_str::<serde_json::Value>(text).is_ok_and(|v| validator.is_valid(&v));
        assert!(!oracle_accepts, "{name} oracle accepted negative {text}");
        assert!(
            codec(text).is_err(),
            "{name} codec accepted negative {text}"
        );
    }
}

const ROLE: &str = r#"{"role_id":"r","project_id":"p","role_type":"RUNTIME_A2","status":"ACTIVE","current_context_epoch":0}"#;

#[test]
fn logical_role_valid_fixture_roundtrips() {
    conformance(
        "LogicalRole",
        &[
            ROLE.to_string(),
            r#"{"role_id":"r","project_id":"p","role_type":"RUNTIME_A1","name":"","workstream_id":"w","ownership_paths":[],"integration_branch":"b","status":"RETIRED","context_manifest_id":"m","current_context_epoch":7,"active_binding_id":null,"created_at":"not a date"}"#.into(),
            ROLE.replace(":0}", ":1.0}"),
            ROLE.replace(":0}", ":1e3}"),
            ROLE.replace(":0}", ":-0}"),
            ROLE.replace("\"r\"", &format!("\"{}\"", "é".repeat(200))),
        ],
        &[],
    );
}

#[test]
fn logical_role_negative_fixture_rejected() {
    conformance(
        "LogicalRole",
        &[],
        &[
            ROLE.replace("RUNTIME_A2", "RUNTIME_A3"),
            ROLE.replace("ACTIVE", "active"),
            ROLE.replace(":0}", ":-1}"),
            ROLE.replace(":0}", ":0.5}"),
            ROLE.replace(":0}", ":\"0\"}"),
            ROLE.replace(":0}", ":0,\"extra\":1}"),
            ROLE.replace(":0}", ":0,\"name\":null}"),
            ROLE.replace(":0}", ":0,\"ownership_paths\":null}"),
            ROLE.replace(":0}", ":0,\"workstream_id\":\"\"}"),
            ROLE.replace("\"r\"", &format!("\"{}\"", "é".repeat(201))),
            ROLE.replace(",\"status\":\"ACTIVE\"", ""),
            format!("{ROLE} {{}}"),
            "[]".into(),
        ],
    );
    // Duplicate keys (escaped alias included) are refused by the codec.
    assert!(
        LogicalRoleWireV1::parse_json(
            &ROLE.replace("\"status\"", "\"status\":\"ACTIVE\",\"st\\u0061tus\"")
        )
        .is_err()
    );
}

#[test]
fn logical_role_presence_numbers_and_conversion() {
    let huge = format!("1{}", "0".repeat(400));
    for (input, kept) in [
        (
            ROLE.replace(":0}", ":0,\"active_binding_id\":null}"),
            "\"active_binding_id\":null",
        ),
        (
            ROLE.replace(":0}", ":0,\"ownership_paths\":[]}"),
            "\"ownership_paths\":[]",
        ),
        (ROLE.replace(":0}", ":1e1000}"), "1e1000"),
        (ROLE.replace(":0}", &format!(":{huge}}}")), huge.as_str()),
    ] {
        let wire = LogicalRoleWireV1::parse_json(&input).unwrap();
        assert!(wire.to_json().unwrap().contains(kept), "{kept}");
    }
    assert!(
        !LogicalRoleWireV1::parse_json(ROLE)
            .unwrap()
            .to_json()
            .unwrap()
            .contains("active_binding_id")
    );
    // Shared domain: wire -> operational -> wire is equivalent.
    let full = r#"{"role_id":"r","project_id":"p","role_type":"RUNTIME_A1","name":"n","workstream_id":"w","ownership_paths":["a","a"],"integration_branch":"b","status":"SUSPENDED","context_manifest_id":"m","current_context_epoch":12345678901234567890123,"active_binding_id":"b1","created_at":"c"}"#;
    let role = LogicalRoleWireV1::parse_json(full)
        .unwrap()
        .to_logical_role()
        .unwrap();
    assert_eq!(
        role.current_context_epoch.as_str(),
        "12345678901234567890123"
    );
    let back = LogicalRoleWireV1::from_logical_role(&role)
        .unwrap()
        .to_json()
        .unwrap();
    assert_eq!(value(&back), value(full));
    // Explicit unsupported operational conversions keep the wire value.
    for token in ["1.0", "1e3", "-0"] {
        let text = ROLE.replace(":0}", &format!(":{token}}}"));
        let wire = LogicalRoleWireV1::parse_json(&text).unwrap();
        assert!(matches!(
            wire.to_logical_role(),
            Err(StateError::WireConversionUnsupported {
                field: "current_context_epoch",
                ..
            })
        ));
        assert!(wire.to_json().unwrap().contains(token));
    }
    let mut bad = role.clone();
    bad.role_id = String::new();
    assert!(LogicalRoleWireV1::from_logical_role(&bad).is_err());
}

const BINDING: &str = r#"{"binding_id":"b","role_id":"r","provider_id":"x","model_id":"m","runtime_id":"rt","bound_at":"t0","lease_expires_at":"t1"}"#;

fn binding_with(extra: &str) -> String {
    BINDING.replace("\"t1\"}", &format!("\"t1\",{extra}}}"))
}

#[test]
fn executor_binding_valid_fixture_roundtrips() {
    conformance(
        "ExecutorBinding",
        &[
            BINDING.into(),
            binding_with(
                r#""session_ref":null,"routing_decision_id":"rd","released_at":null,"release_reason":"LEASE_EXPIRED","rehydration_completed":false"#,
            ),
            binding_with(r#""session_ref":"s","released_at":"2026-10-29T10:00:00Z""#),
            BINDING.replace("\"x\"", &format!("\"{}\"", "p".repeat(500))),
        ],
        &[],
    );
}

#[test]
fn executor_binding_negative_fixture_rejected() {
    conformance(
        "ExecutorBinding",
        &[],
        &[
            BINDING.replace("\"x\"", "\"\""),
            BINDING.replace(",\"lease_expires_at\":\"t1\"", ""),
            binding_with(r#""release_reason":"EXPIRED""#),
            binding_with(r#""release_reason":null"#),
            binding_with(r#""rehydration_completed":1"#),
            binding_with(r#""routing_decision_id":null"#),
            binding_with(r#""session_ref":5"#),
            binding_with(r#""unknown":true"#),
        ],
    );
}

#[test]
fn executor_binding_presence_and_conversion() {
    let text = binding_with(r#""session_ref":null,"released_at":null"#);
    let wire = ExecutorBindingWireV1::parse_json(&text).unwrap();
    assert_eq!(value(&wire.to_json().unwrap()), value(&text));
    let operational = wire.to_executor_binding().unwrap();
    assert_eq!(
        (operational.session_ref, operational.released_at),
        (None, None)
    );
    let full = binding_with(
        r#""session_ref":"s","routing_decision_id":"rd","released_at":"t2","release_reason":"COMPLETED","rehydration_completed":true"#,
    );
    let binding = ExecutorBindingWireV1::parse_json(&full)
        .unwrap()
        .to_executor_binding()
        .unwrap();
    let back = ExecutorBindingWireV1::from_executor_binding(&binding)
        .unwrap()
        .to_json()
        .unwrap();
    assert_eq!(value(&back), value(&full));
}

const SOURCE: &str = r#"{"ref":{"ref_type":"REPO_PATH","target":"a.md"},"source_class":"MANDATORY","digest":"d","required_for":["DISPATCH"]}"#;

fn manifest(sources: &str, extra: &str) -> String {
    format!(r#"{{"manifest_id":"m","role_id":"r","epoch":0,"sources":[{sources}]{extra}}}"#)
}

#[test]
fn context_manifest_valid_fixture_roundtrips() {
    conformance(
        "ContextManifest",
        &[
            manifest(SOURCE, ""),
            manifest(
                r#"{"ref":{"ref_type":"URL","target":"https://x","digest":"","section":"s"},"source_class":"REFERENCE","digest":null,"last_read_at":null,"required_for":[]}"#,
                r#","project_id":"p","derived_state":{"next":[1,2.50,{"z":null}],"a":{}},"last_rehydrated_at":null,"created_at":"c""#,
            ),
            manifest(&format!("{SOURCE},{SOURCE}"), ""),
        ],
        &[],
    );
}

#[test]
fn context_manifest_negative_fixture_rejected() {
    conformance(
        "ContextManifest",
        &[],
        &[
            manifest("", ""),
            manifest(&SOURCE.replace("REPO_PATH", "FILE"), ""),
            manifest(&SOURCE.replace("\"a.md\"", "\"\""), ""),
            manifest(&SOURCE.replace("MANDATORY", "OPTIONAL"), ""),
            manifest(&SOURCE.replace("DISPATCH", "MERGE"), ""),
            manifest(
                &SOURCE.replace("\"source_class\"", "\"extra\":1,\"source_class\""),
                "",
            ),
            manifest(SOURCE, r#","derived_state":[]"#),
            manifest(SOURCE, r#","derived_state":null"#),
            manifest(SOURCE, r#","project_id":null"#),
            manifest(SOURCE, "").replace("\"epoch\":0", "\"epoch\":-3"),
        ],
    );
}

#[test]
fn context_manifest_open_object_identity_and_conversion() {
    let derived = r#"{"z":1e400,"a":[{"k":"v"}],"n":123456789012345678901234567890}"#;
    let text = manifest(
        SOURCE,
        &format!(r#","project_id":"p","derived_state":{derived},"created_at":"c""#),
    );
    let wire = ContextManifestWireV1::parse_json(&text).unwrap();
    assert!(
        wire.to_json().unwrap().contains(derived),
        "open object bytes kept exactly"
    );
    assert!(matches!(
        wire.to_context_manifest(),
        Err(StateError::WireConversionUnsupported {
            field: "derived_state",
            ..
        })
    ));
    let plain = manifest(SOURCE, r#","project_id":"p","created_at":"c""#);
    let operational = ContextManifestWireV1::parse_json(&plain)
        .unwrap()
        .to_context_manifest()
        .unwrap();
    let back = ContextManifestWireV1::from_context_manifest(&operational)
        .unwrap()
        .to_json()
        .unwrap();
    assert_eq!(value(&back), value(&plain));
    for (input, field) in [
        (manifest(SOURCE, ""), "project_id"),
        (
            manifest(
                &SOURCE.replace("REPO_PATH", "URL"),
                r#","project_id":"p","created_at":"c""#,
            ),
            "sources.ref.ref_type",
        ),
        (
            manifest(
                &SOURCE.replace(r#""digest":"d""#, r#""digest":null"#),
                r#","project_id":"p","created_at":"c""#,
            ),
            "sources.digest",
        ),
        (
            manifest(
                &SOURCE.replace(r#""a.md""#, r#""a.md","section":"s""#),
                r#","project_id":"p","created_at":"c""#,
            ),
            "sources.ref",
        ),
    ] {
        assert!(
            matches!(
                ContextManifestWireV1::parse_json(&input).unwrap().to_context_manifest(),
                Err(StateError::WireConversionUnsupported { field: f, .. }) if f == field
            ),
            "{field}"
        );
    }
}

const EPOCH: &str = r#"{"epoch":3,"project_id":"p","advanced_at":"t","trigger":"NEW_WAVE"}"#;

#[test]
fn context_epoch_valid_fixture_roundtrips() {
    conformance(
        "ContextEpoch",
        &[
            EPOCH.into(),
            EPOCH.replace("}", r#","changed_sources":[{"ref_type":"URL","target":"u","digest":"","section":"x"}],"invalidated_role_ids":["r1","r2"]}"#),
            EPOCH.replace("}", r#","changed_sources":[],"invalidated_role_ids":[]}"#),
        ],
        &[],
    );
}

#[test]
fn context_epoch_negative_fixture_rejected() {
    conformance(
        "ContextEpoch",
        &[],
        &[
            EPOCH.replace("NEW_WAVE", "NEWWAVE"),
            EPOCH.replace("3", "-3"),
            EPOCH.replace("}", r#","changed_sources":null}"#),
            EPOCH.replace(
                "}",
                r#","changed_sources":[{"ref_type":"URL","target":""}]}"#,
            ),
            EPOCH.replace(
                "}",
                r#","changed_sources":[{"ref_type":"URL","target":"u","x":1}]}"#,
            ),
            EPOCH.replace("}", r#","invalidated_role_ids":[""]}"#),
            EPOCH.replace(",\"trigger\":\"NEW_WAVE\"", ""),
        ],
    );
}

#[test]
fn context_epoch_presence_and_conversion() {
    use crate::context_epoch::ChangedSources;
    let omitted = ContextEpochWireV1::parse_json(EPOCH)
        .unwrap()
        .to_parts()
        .unwrap();
    assert_eq!(
        (
            omitted.changed_sources.clone(),
            omitted.invalidated_role_ids.clone()
        ),
        (ChangedSources::Omitted, None)
    );
    let empty = EPOCH.replace("}", r#","changed_sources":[],"invalidated_role_ids":[]}"#);
    let parts = ContextEpochWireV1::parse_json(&empty)
        .unwrap()
        .to_parts()
        .unwrap();
    assert_eq!(parts.changed_sources, ChangedSources::Present(Vec::new()));
    assert_eq!(
        value(
            &ContextEpochWireV1::from_parts(&parts)
                .unwrap()
                .to_json()
                .unwrap()
        ),
        value(&empty)
    );
    assert_eq!(
        value(
            &ContextEpochWireV1::from_parts(&omitted)
                .unwrap()
                .to_json()
                .unwrap()
        ),
        value(EPOCH)
    );
    assert!(matches!(
        ContextEpochWireV1::parse_json(&EPOCH.replace("3", "3.0"))
            .unwrap()
            .to_parts(),
        Err(StateError::WireConversionUnsupported { field: "epoch", .. })
    ));
}

const ENT: &str = r#"{"subject_id":"sub","tier_id":"pro","capabilities":["graph.core","orchestration.multi_runtime"],"issued_at":"2026-10-01T00:00:00.000000000Z","expires_at":"2026-11-01T00:00:00.000000000Z","entitlement_version":3,"key_id":"k1","signature":"sig"}"#;

fn ent(from: &str, to: &str) -> String {
    assert!(ENT.contains(from));
    ENT.replace(from, to)
}

#[test]
fn product_entitlement_valid_fixture_roundtrips() {
    conformance(
        "ProductEntitlement",
        &[
            ENT.into(),
            ent(
                "\"key_id\"",
                "\"offline_grace_until\":null,\"device_binding\":null,\"key_id\"",
            ),
            ent(
                "\"key_id\"",
                "\"offline_grace_until\":\"2026-11-08T00:00:00Z\",\"device_binding\":\"opaque\",\"key_id\"",
            ),
            ent("[\"graph.core\",\"orchestration.multi_runtime\"]", "[]"),
            ent(":3,", ":1e2,"),
            ent(":3,", ":9223372036854775808,"),
            ent("2026-10-01T00:00:00.000000000Z", "2026-10-01T00:00:00Z"),
        ],
        &[],
    );
}

#[test]
fn product_entitlement_negative_fixture_rejected() {
    conformance(
        "ProductEntitlement",
        &[],
        &[
            ent(":3,", ":0,"),
            ent(":3,", ":1.5,"),
            ent(":3,", ":null,"),
            ent("\"graph.core\"", "\"Graph.core\""),
            ent("\"graph.core\"", "\"graph\""),
            ent("\"graph.core\"", "\"graph..core\""),
            ent("\"sig\"", "\"\""),
            ent("\"pro\"", "\"\""),
            ent("\"key_id\"", "\"extra\":1,\"key_id\""),
            ent(",\"signature\":\"sig\"", ""),
        ],
    );
}

#[test]
fn product_entitlement_wire_matches_verifier_parser() {
    use crate::entitlement::wire::parse;
    // Shared domain: identical bytes accepted by both; conversion equals the
    // authoritative parse of the original bytes.
    for text in [
        ENT.to_string(),
        ent(
            "\"key_id\"",
            "\"offline_grace_until\":\"2026-11-08T00:00:00.000000000Z\",\"key_id\"",
        ),
        ent("\"key_id\"", "\"device_binding\":\"opaque\",\"key_id\""),
    ] {
        let physical = parse(text.as_bytes()).unwrap();
        let wire = ProductEntitlementWireV1::parse_json(&text).unwrap();
        assert_eq!(wire.to_physical().unwrap(), physical);
    }
    // Schema-admitted spellings that must not silently become physical/verified values.
    for (text, field) in [
        (
            ent("\"key_id\"", "\"offline_grace_until\":null,\"key_id\""),
            "offline_grace_until",
        ),
        (
            ent("\"key_id\"", "\"device_binding\":null,\"key_id\""),
            "device_binding",
        ),
        (ent(":3,", ":9223372036854775808,"), "entitlement_version"),
        (ent(":3,", ":1e2,"), "entitlement_version"),
        (ent(":3,", ":3.0,"), "entitlement_version"),
        (
            ent("2026-10-01T00:00:00.000000000Z", "2026-10-01T00:00:00Z"),
            "issued_at",
        ),
    ] {
        assert!(
            parse(text.as_bytes()).is_err(),
            "verifier parser must refuse {text}"
        );
        let wire = ProductEntitlementWireV1::parse_json(&text).unwrap();
        assert!(
            matches!(
                wire.to_physical(),
                Err(StateError::WireConversionUnsupported { field: f, .. }) if f == field
            ),
            "{field}"
        );
        assert_eq!(
            value(&wire.to_json().unwrap()),
            value(&text),
            "wire value unchanged"
        );
    }
    let huge = ent(":3,", &format!(":{}0,", "9".repeat(300)));
    let wire = ProductEntitlementWireV1::parse_json(&huge).unwrap();
    assert_eq!(
        wire.entitlement_version_token(),
        format!("{}0", "9".repeat(300))
    );
    assert!(wire.to_physical().is_err());
}

const ACT: &str =
    r#"{"activation_state":"NEVER_ACTIVATED","recorded_at":"2026-10-29T10:00:00.000000000Z"}"#;

#[test]
fn activation_state_valid_fixture_roundtrips() {
    conformance(
        "ActivationState",
        &[
            ACT.into(),
            ACT.replace("{", r#"{"subject_id":null,"first_activated_at":null,"last_known_tier_id":null,"last_entitlement_seen_at":null,"last_observed_server_time":null,"logged_out_at":null,"#),
            r#"{"activation_state":"ACTIVATED_KNOWN","subject_id":"s","first_activated_at":"x","last_known_tier_id":"pro","recorded_at":"y"}"#.into(),
        ],
        &[],
    );
}

#[test]
fn activation_state_negative_fixture_rejected() {
    conformance(
        "ActivationState",
        &[],
        &[
            ACT.replace("NEVER_ACTIVATED", "NEVER"),
            ACT.replace(",\"recorded_at\":\"2026-10-29T10:00:00.000000000Z\"", ""),
            ACT.replace("{", r#"{"recorded_at_extra":1,"#),
            ACT.replace("{", r#"{"subject_id":7,"#),
            ACT.replace(
                "\"recorded_at\":\"2026-10-29T10:00:00.000000000Z\"",
                "\"recorded_at\":null",
            ),
        ],
    );
}

#[test]
fn activation_state_presence_and_conversion() {
    let nulls = ACT.replace("{", r#"{"subject_id":null,"logged_out_at":null,"#);
    let wire = ActivationStateWireV1::parse_json(&nulls).unwrap();
    assert!(wire.to_json().unwrap().contains("\"subject_id\":null"));
    let physical = wire.to_physical().unwrap();
    assert_eq!(
        (physical.subject_id(), physical.recorded_at().as_str()),
        (None, "2026-10-29T10:00:00.000000000Z")
    );
    let known = r#"{"activation_state":"ACTIVATED_KNOWN","subject_id":"s","first_activated_at":"2026-10-01T00:00:00.000000000Z","last_known_tier_id":"pro","recorded_at":"2026-10-29T10:00:00.000000000Z"}"#;
    assert!(
        ActivationStateWireV1::parse_json(known)
            .unwrap()
            .to_physical()
            .is_ok()
    );
    for (text, field) in [
        (ACT.replace(".000000000Z", "Z"), "recorded_at"),
        (
            known.replace("2026-10-01T00:00:00.000000000Z", "2026-10-01"),
            "first_activated_at",
        ),
        (
            ACT.replace("{", r#"{"subject_id":"s","#),
            "activation_state",
        ),
    ] {
        assert!(
            matches!(
                ActivationStateWireV1::parse_json(&text).unwrap().to_physical(),
                Err(StateError::WireConversionUnsupported { field: f, .. }) if f == field
            ),
            "{field}"
        );
    }
}

#[test]
fn derived_state_duplicate_decoded_keys_are_refused_at_every_depth() {
    // The oracle's Value model keeps the last duplicate, so it would accept
    // these; the codec refuses them because the object identity is ambiguous.
    // Same-named keys at different depths remain ordinary data.
    for derived in [
        r#"{"a":1,"a":2}"#,
        r#"{"a":1,"a":2}"#,
        r#"{"x":{"a":1,"a":1}}"#,
        r#"{"x":[{"a":null,"a":null}]}"#,
    ] {
        let text = manifest(SOURCE, &format!(r#","derived_state":{derived}"#));
        assert!(
            matches!(
                ContextManifestWireV1::parse_json(&text),
                Err(StateError::WireDecode {
                    schema: "ContextManifest"
                })
            ),
            "{derived}"
        );
    }
    let nested = r#"{"a":{"a":1},"b":[{"a":2},{"a":3}]}"#;
    let text = manifest(SOURCE, &format!(r#","derived_state":{nested}"#));
    let wire = ContextManifestWireV1::parse_json(&text).unwrap();
    assert!(wire.to_json().unwrap().contains(nested), "bytes kept");
}
