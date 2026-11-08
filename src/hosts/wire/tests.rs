use super::*;

fn fixture_dir(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn fixtures(name: &str) -> Vec<(String, String)> {
    let mut files: Vec<_> = std::fs::read_dir(fixture_dir(name))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect()
}

#[test]
fn host_capability_fixtures_01_to_08_roundtrip() {
    let all = fixtures("host_capability");
    assert_eq!(all.len(), 8);
    for (name, text) in all {
        let report =
            host_capability_report_from_json(&text).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        let encoded = host_capability_report_to_json(&report);
        let again = host_capability_report_from_json(&encoded).unwrap();
        assert_eq!(again, report, "{name}");
        assert_eq!(host_capability_report_to_json(&again), encoded, "{name}");
        // Every encoded property is a schema property with the original value.
        let original = V::parse_json(&text).unwrap();
        let V::Object(original) = original else {
            panic!()
        };
        let V::Object(out) = V::parse_json(&encoded).unwrap() else {
            panic!()
        };
        for (key, value) in out.as_map() {
            match original.as_map().get(key) {
                Some(v) => assert_eq!(v, value, "{name}.{key}"),
                // Only required nullable fields may be materialized as null.
                None => assert_eq!(value, &V::Null, "{name}.{key}"),
            }
        }
        for (key, value) in original.as_map() {
            if !out.as_map().contains_key(key) {
                assert_eq!(value, &V::Null, "{name}.{key} dropped");
            }
        }
    }
}

#[test]
fn host_capability_negative_fixtures_01_to_13_rejected() {
    let all = fixtures("host_capability-negative");
    assert_eq!(all.len(), 13);
    for (name, text) in all {
        let error = host_capability_report_from_json(&text).expect_err(&name);
        assert!(
            matches!(
                error,
                HostWireError::Invalid(_) | HostWireError::SelectionMismatch
            ),
            "{name}: {error:?}"
        );
    }
}

fn healthy() -> String {
    fixtures("host_capability")
        .into_iter()
        .find(|(n, _)| n.starts_with("02"))
        .unwrap()
        .1
}

fn edit(text: &str, apply: impl FnOnce(&mut Map)) -> String {
    let V::Object(o) = V::parse_json(text).unwrap() else {
        panic!()
    };
    let mut m = o.as_map().clone();
    apply(&mut m);
    V::Object(OrchestrationJsonObjectV1::new(m)).to_json()
}

#[test]
fn report_structural_negatives_name_the_field_only() {
    let base = healthy();
    assert!(host_capability_report_from_json(&base).is_ok());
    let cases: Vec<(String, HostWireError)> = vec![
        ("[]".into(), HostWireError::NotAnObject),
        ("{".into(), HostWireError::InvalidJson),
        (
            r#"{"host_id":"a","host_id":"b"}"#.into(),
            HostWireError::InvalidJson,
        ),
        (
            edit(&base, |m| {
                m.insert("SECRET_EXTRA".into(), V::Boolean(true));
            }),
            HostWireError::UnknownField,
        ),
        (
            edit(&base, |m| {
                m.remove("hooks_trusted");
            }),
            HostWireError::MissingField("hooks_trusted"),
        ),
        (
            edit(&base, |m| {
                m.insert("probe_status".into(), s("complete"));
            }),
            HostWireError::InvalidField("probe_status"),
        ),
        (
            edit(&base, |m| {
                m.insert("probed_at".into(), s("2026-08-16 04:00:00Z"));
            }),
            HostWireError::InvalidField("probed_at"),
        ),
        (
            edit(&base, |m| {
                m.insert("host_id".into(), s(""));
            }),
            HostWireError::InvalidField("host_id"),
        ),
        (
            edit(&base, |m| {
                m.insert("validity_fingerprint".into(), s(""));
            }),
            HostWireError::InvalidField("validity_fingerprint"),
        ),
        (
            edit(&base, |m| {
                m.insert("hooks_enabled".into(), s("true"));
            }),
            HostWireError::InvalidField("hooks_enabled"),
        ),
        (
            edit(&base, |m| {
                m.insert("hook_events".into(), V::Array(vec![V::Null]));
            }),
            HostWireError::InvalidField("hook_events"),
        ),
        (
            edit(&base, |m| {
                let mut o = Map::new();
                o.insert("source".into(), s("DEBUG"));
                o.insert("reason".into(), s(""));
                m.insert(
                    "mode_override".into(),
                    V::Object(OrchestrationJsonObjectV1::new(o)),
                );
            }),
            HostWireError::InvalidField("mode_override.reason"),
        ),
        ("x".repeat(HOST_WIRE_MAX_BYTES + 1), HostWireError::TooLarge),
        // Facts say healthy native path but the file asserts SUPERVISED.
        (
            edit(&base, |m| {
                m.insert("selected_mode".into(), s("SUPERVISED"));
            }),
            HostWireError::SelectionMismatch,
        ),
        // A stale report can never carry EMBEDDED.
        (
            edit(&base, |m| {
                m.insert("stale_reason".into(), s("VALIDITY_FINGERPRINT_CHANGED"));
            }),
            HostWireError::SelectionMismatch,
        ),
    ];
    for (text, expected) in cases {
        let error = host_capability_report_from_json(&text).unwrap_err();
        assert_eq!(error, expected);
        assert!(!format!("{error:?}{error}").contains("SECRET"));
    }
}

fn event_json() -> String {
    r#"{"event_id":"01J9ZZZZZZZZZZZZZZZZZZZZZZ","event_type":"TOOL_EXECUTED","host":"claude-code","host_session_id":"s-1","project_id":"p","occurred_at":"2026-11-08T00:00:00.123Z","payload":{"tool_name":"Bash","n":1.50},"raw_ref":{"ref_type":"REPO_PATH","target":"a/b","digest":"sha256:x"},"confidence":"OBSERVED"}"#.into()
}

#[test]
fn normalized_host_event_roundtrip() {
    let event = normalized_host_event_from_json(&event_json()).unwrap();
    let encoded = normalized_host_event_to_json(&event);
    assert_eq!(
        encoded,
        r#"{"confidence":"OBSERVED","event_id":"01J9ZZZZZZZZZZZZZZZZZZZZZZ","event_type":"TOOL_EXECUTED","host":"claude-code","host_session_id":"s-1","occurred_at":"2026-11-08T00:00:00.123Z","payload":{"n":1.50,"tool_name":"Bash"},"project_id":"p","raw_ref":{"digest":"sha256:x","ref_type":"REPO_PATH","target":"a/b"}}"#,
        "sorted keys and exact lexemes"
    );
    assert_eq!(normalized_host_event_from_json(&encoded).unwrap(), event);
    // Minimal event: optional members absent; payload materializes as {}.
    let minimal = r#"{"confidence":"INFERRED","event_id":"01J9ZZZZZZZZZZZZZZZZZZZZZZ","event_type":"CONTEXT_COMPACTED","host":"x","host_session_id":"s","occurred_at":"2026-11-08T00:00:00Z"}"#;
    let event = normalized_host_event_from_json(minimal).unwrap();
    assert!(event.payload().as_map().is_empty());
    assert_eq!(event.raw_ref(), None);
    assert_eq!(event.confidence(), NormalizedHostEventConfidence::Inferred);
    let again = normalized_host_event_to_json(&event);
    assert!(again.contains(r#""payload":{}"#));
    assert_eq!(normalized_host_event_from_json(&again).unwrap(), event);
    // Every event type spelling round-trips.
    for t in NormalizedHostEventType::ALL {
        let text = minimal.replace("CONTEXT_COMPACTED", t.as_str());
        assert_eq!(
            normalized_host_event_from_json(&text).unwrap().event_type(),
            t
        );
    }
}

#[test]
fn normalized_host_event_negatives() {
    let base = event_json();
    let cases: Vec<(String, HostWireError)> = vec![
        (
            base.replace("01J9ZZZZZZZZZZZZZZZZZZZZZZ", "01J9ZZZZZZZZZZZZZZZZZZZZZI"),
            HostWireError::InvalidField("event_id"),
        ),
        (
            base.replace("TOOL_EXECUTED", "SUBAGENT_STOPPED"),
            HostWireError::InvalidField("event_type"),
        ),
        (
            base.replace(r#""host":"claude-code""#, r#""host":"""#),
            HostWireError::InvalidField("host"),
        ),
        (
            base.replace(r#""s-1""#, &format!("\"{}\"", "s".repeat(201))),
            HostWireError::InvalidField("host_session_id"),
        ),
        (
            base.replace(r#""project_id":"p""#, r#""project_id":"""#),
            HostWireError::InvalidField("project_id"),
        ),
        (
            base.replace(r#""project_id":"p""#, r#""project_id":null"#),
            HostWireError::InvalidField("project_id"),
        ),
        (
            base.replace("2026-11-08T00:00:00.123Z", "2026-02-30T00:00:00Z"),
            HostWireError::InvalidField("occurred_at"),
        ),
        (
            base.replace(
                r#""payload":{"tool_name":"Bash","n":1.50}"#,
                r#""payload":[]"#,
            ),
            HostWireError::InvalidField("payload"),
        ),
        (
            base.replace(r#""target":"a/b""#, r#""target":"""#),
            HostWireError::InvalidField("raw_ref.target"),
        ),
        (
            base.replace(r#""digest":"sha256:x""#, r#""digest":"x","extra":1"#),
            HostWireError::UnknownField,
        ),
        (
            base.replace("REPO_PATH", "FILE"),
            HostWireError::InvalidField("raw_ref.ref_type"),
        ),
        (
            base.replace("OBSERVED", "observed"),
            HostWireError::InvalidField("confidence"),
        ),
        (
            base.replace(r#","confidence":"OBSERVED""#, ""),
            HostWireError::MissingField("confidence"),
        ),
        (
            base.replace(
                r#""host":"claude-code","#,
                r#""host":"claude-code","secret":"SECRET","#,
            ),
            HostWireError::UnknownField,
        ),
        (
            base.replace(
                r#""tool_name":"Bash""#,
                r#""tool_name":"Bash","tool_name":"x""#,
            ),
            HostWireError::InvalidJson,
        ),
    ];
    for (text, expected) in cases {
        let error = normalized_host_event_from_json(&text).unwrap_err();
        assert_eq!(error, expected, "{text}");
        assert!(!format!("{error:?}{error}").contains("SECRET"));
    }
}
