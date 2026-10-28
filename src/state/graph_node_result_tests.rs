use crate::{graph_history::StateGraphMutationV1, graph_node_result::StateGraphNodeResultV1};
fn result(exit: &str) -> String {
    format!(
        r#"{{"graph_id":"g","node_id":"n","outcome":"PASS","produced_at":"2026-10-28T01:02:03+05:30","checks":[{{"command":[""],"exit_code":{exit}}}]}}"#
    )
}
fn mutation(detail: &str) -> String {
    format!(
        r#"{{"mutation_id":"m","graph_id":"g","parent_version":1,"resulting_version":2,"actor":{{"role":""}},"reason":"r","created_at":"2026-10-28t01:02:03z","operations":[{{"op":"ADD_NODE","detail":{detail}}}]}}"#
    )
}
#[test]
fn actual_integer_boundary_and_non_echoing_refusals() {
    for (s, valid) in [
        ("1e1000001", true),
        ("-1e1000001", true),
        ("1e-1000001", false),
        ("-1e-1000001", false),
        ("1e9223372036854775808", true),
        ("1e-9223372036854775808", false),
        ("-0e-999999999999999999999", true),
        ("18446744073709551617", true),
        ("-18446744073709551617", true),
        ("18446744073709551617.1", false),
        ("1.5e1", true),
        ("100e-2", true),
        ("100e-3", false),
        ("null", false),
        ("[]", false),
        ("\"1\"", false),
        (r#"{"$serde_json::private::Number":"1"}"#, false),
    ] {
        let parsed = StateGraphNodeResultV1::parse_json(&result(s));
        assert_eq!(parsed.is_ok(), valid, "{s}");
        if let Ok(v) = parsed {
            let stored = v.to_json().unwrap();
            assert!(stored.contains(s));
            StateGraphNodeResultV1::parse_json(&stored).unwrap();
        }
    }
    for s in [
        result("0").replace(
            "\"outcome\":\"PASS\"",
            "\"outcome\":\"PASS\",\"outcome\":\"FAIL\"",
        ),
        result("0").replace(
            "\"outcome\":\"PASS\"",
            "\"outcome\":\"PASS\",\"secret\":\"DO_NOT_ECHO\"",
        ),
        result("0").replace("\"checks\":[", "\"checks\":null,\"checks\":["),
    ] {
        let e = StateGraphNodeResultV1::parse_json(&s).unwrap_err();
        assert!(!format!("{e:?} {e}").contains("DO_NOT_ECHO"));
    }
}
#[test]
fn actual_raw_object_identity_null_unknown_and_duplicates() {
    for raw in [
        r#"{"$serde_json::private::Number":"1"}"#,
        r#"{"$serde_json::private::RawValue":"null","x":1,"x":2,"\u0024serde_json::private::Number":[null,"1",1,1e999999999999999999,-1e-99999999999999999]}"#,
        r#"{"first":0,"nested":{"$serde_json::private::Number":"18446744073709551617"}}"#,
    ] {
        let parsed = StateGraphMutationV1::parse_json(&mutation(raw)).unwrap();
        let stored = parsed.to_json().unwrap();
        assert!(stored.contains(raw));
        assert_eq!(
            StateGraphMutationV1::parse_json(&stored)
                .unwrap()
                .to_json()
                .unwrap(),
            stored
        );
    }
    for raw in ["null", "[]", "1", "\"object\"", "true"] {
        assert!(StateGraphMutationV1::parse_json(&mutation(raw)).is_err());
    }
    for s in [
        mutation("{}").replace("\"detail\":{}", "\"detail\":{},\"detail\":{}"),
        mutation("{}").replace("\"detail\":{}", "\"detail\":{},\"unknown\":1"),
        mutation("{}").replace(
            "\"actor\":{\"role\":\"\"}",
            "\"actor\":{\"role\":\"\",\"role_id\":null}",
        ),
    ] {
        assert!(StateGraphMutationV1::parse_json(&s).is_err());
    }
}
#[test]
fn schema_field_presence_and_date_time() {
    let raw = r#"{"graph_id":"g","node_id":"n","outcome":"PASS","produced_at":"2000-02-29T23:59:60Z","checks":[],"evidence_refs":[""],"review_id":"r"}"#;
    StateGraphNodeResultV1::parse_json(raw).unwrap();
    for (from, to) in [
        ("2000-02-29", "1900-02-29"),
        ("\"checks\":[]", "\"checks\":null"),
        ("\"review_id\":\"r\"", "\"review_id\":\"\""),
        ("\"evidence_refs\":[\"\"]", "\"evidence_refs\":null"),
    ] {
        assert!(StateGraphNodeResultV1::parse_json(&raw.replace(from, to)).is_err());
    }
}

#[test]
fn evidence_dates_share_schema_format_cases_without_canonical_clock_restriction() {
    let schema = serde_json::json!({"type":"string","format":"date-time"});
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)
        .unwrap();
    for date in [
        "2026-10-28T03:00:00Z",
        "2026-10-28t03:00:00z",
        "2000-02-29T00:00:00.123456789123+05:30",
        "2026-10-28T03:00:00-00:00",
        "1900-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T00:00:00+24:00",
        "2026-01-01T00:00:00.Z",
        "2026-01-01",
        "2026-01-01T00:00:00+05:60",
    ] {
        let value = serde_json::Value::String(date.into());
        let raw =
            format!(r#"{{"node_id":"n","graph_id":"g","outcome":"PASS","produced_at":"{date}"}}"#);
        assert_eq!(
            StateGraphNodeResultV1::parse_json(&raw).is_ok(),
            validator.is_valid(&value),
            "{date}"
        );
    }
}
