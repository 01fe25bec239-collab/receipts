//! Synthetic semantic fixtures, not recorded real-provider evidence.
use crate::claude_stream_json::protocol;
use crate::*;
use serde_json::{Value, json};
use std::error::Error;

fn success() -> Value {
    json!({"type":"result", "subtype":"success", "is_error":false,
        "result":"世界 😀", "session_id":"opaque-session", "future":{"extra":17}})
}
fn error(subtype: &str) -> Value {
    json!({"type":"result", "subtype":subtype, "is_error":true,
        "errors":["rate limit login required safety pending cancelled"]})
}
fn bytes(records: &[Value]) -> Option<Vec<u8>> {
    Some(
        records
            .iter()
            .map(|v| format!("{v}\n"))
            .collect::<String>()
            .into_bytes(),
    )
}

#[test]
fn supported_success_usage_and_unknown_evidence_are_preserved() {
    let mut value = success();
    value["usage"] = json!({"input_tokens":0,"output_tokens":42,"future":91});
    let input = bytes(&[
        json!({"type":"future","payload":19}),
        value.clone(),
        json!({"type":"system","subtype":"prompt_suggestion"}),
    ]);
    let protocol = protocol(&input).unwrap();
    let result = protocol.result().unwrap();
    assert_eq!(result.subtype(), ClaudeResultSubtype::Success);
    assert_eq!(result.provider_result(), ClaudeProviderResult::Success);
    assert!(!result.is_error());
    assert_eq!(result.text(), Some("世界 😀"));
    assert_eq!(result.session_id(), Some("opaque-session"));
    assert_eq!(
        result.usage(),
        ClaudeResultUsage::Known {
            input_tokens: 0,
            output_tokens: 42
        }
    );
    assert_eq!(protocol.records()[1].value(), &value);
    assert_eq!(protocol.records()[0].value()["payload"], 19);
    assert_eq!(
        protocol.records()[1].raw_record(),
        format!("{value}\n").as_bytes()
    );
    assert!(result.errors().is_none());
}

#[test]
fn documented_error_subtypes_and_api_error_never_become_success() {
    for (name, subtype) in [
        (
            "error_during_execution",
            ClaudeResultSubtype::ErrorDuringExecution,
        ),
        ("error_max_turns", ClaudeResultSubtype::ErrorMaxTurns),
        (
            "error_max_budget_usd",
            ClaudeResultSubtype::ErrorMaxBudgetUsd,
        ),
        (
            "error_max_structured_output_retries",
            ClaudeResultSubtype::ErrorMaxStructuredOutputRetries,
        ),
    ] {
        // The SDK types is_error as boolean on both variants. The error subtype
        // still reports an error when that flag is false; no guessed cause.
        for flag in [true, false] {
            let mut value = error(name);
            value["is_error"] = json!(flag);
            let input = bytes(&[value]);
            let protocol = protocol(&input).unwrap();
            let result = protocol.result().unwrap();
            assert_eq!(result.subtype(), subtype);
            assert_eq!(result.is_error(), flag);
            assert_eq!(result.provider_result(), ClaudeProviderResult::Error);
            assert!(result.text().is_none());
            assert_eq!(result.errors().unwrap().len(), 1);
        }
    }
    let mut value = success();
    value["is_error"] = json!(true);
    let input = bytes(&[value]);
    let protocol = protocol(&input).unwrap();
    let result = protocol.result().unwrap();
    assert_eq!(result.provider_result(), ClaudeProviderResult::Error);
    assert_eq!(result.text(), Some("世界 😀"));
}

#[test]
fn unknown_subtype_and_absent_usage_are_explicitly_unknown() {
    for flag in [true, false] {
        let mut value = success();
        value["subtype"] = json!("future_success");
        value["is_error"] = json!(flag);
        let input = bytes(&[value.clone()]);
        let protocol = protocol(&input).unwrap();
        let result = protocol.result().unwrap();
        assert_eq!(result.subtype(), ClaudeResultSubtype::Unknown);
        assert_eq!(result.provider_result(), ClaudeProviderResult::Unknown);
        assert_eq!(result.usage(), ClaudeResultUsage::Unknown);
        assert_eq!(protocol.records()[0].value(), &value);
        assert!(result.text().is_none());
    }
    for usage in [
        None,
        Some(Value::Null),
        Some(json!({})),
        Some(json!({"input_tokens":9})),
    ] {
        let mut value = success();
        value.as_object_mut().unwrap().remove("session_id");
        if let Some(usage) = usage {
            value["usage"] = usage;
        }
        let input = bytes(&[value]);
        let protocol = protocol(&input).unwrap();
        let result = protocol.result().unwrap();
        assert_eq!(result.usage(), ClaudeResultUsage::Unknown);
        assert_eq!(result.session_id(), None);
    }
}

#[test]
fn absent_repeated_unknown_and_contradictory_results_are_refused_in_both_orders() {
    for records in [vec![], vec![json!({"type":"assistant","is_error":false})]] {
        assert_eq!(
            protocol(&bytes(&records)).unwrap().result().err(),
            Some(ClaudeResultError::MissingResult)
        );
    }
    let mut unknown = success();
    unknown["subtype"] = json!("future");
    for other in [
        success(),
        error("error_during_execution"),
        unknown,
        json!({"type":"result"}),
    ] {
        for records in [
            vec![success(), other.clone()],
            vec![other.clone(), success()],
        ] {
            assert_eq!(
                protocol(&bytes(&records)).unwrap().result().err(),
                Some(ClaudeResultError::MultipleResults)
            );
        }
    }
}

#[test]
fn missing_wrong_type_and_contradictory_semantic_fields_refuse_interpretation_only() {
    for (base, key, values) in [
        (success(), "subtype", vec![Value::Null, json!(7)]),
        (
            success(),
            "is_error",
            vec![Value::Null, json!("false"), json!(0)],
        ),
        (success(), "result", vec![Value::Null, json!({})]),
        (
            error("error_max_turns"),
            "errors",
            vec![Value::Null, json!("error"), json!([1])],
        ),
    ] {
        for replacement in std::iter::once(None).chain(values.into_iter().map(Some)) {
            let mut value = base.clone();
            if let Some(replacement) = replacement {
                value[key] = replacement;
            } else {
                value.as_object_mut().unwrap().remove(key);
            }
            let input = bytes(&[value.clone()]);
            let protocol = protocol(&input).unwrap();
            assert_eq!(
                protocol.result().err(),
                Some(ClaudeResultError::InvalidResult)
            );
            assert_eq!(protocol.records()[0].value(), &value);
        }
    }
    let mut cases = Vec::new();
    let mut value = success();
    value["session_id"] = json!(12);
    cases.push(value);
    let mut value = success();
    value["errors"] = json!(["contradiction"]);
    cases.push(value);
    let mut value = error("error_max_turns");
    value["result"] = json!("contradiction");
    cases.push(value);
    for value in cases {
        assert_eq!(
            protocol(&bytes(&[value])).unwrap().result().err(),
            Some(ClaudeResultError::InvalidResult)
        );
    }
}

#[test]
fn invalid_usage_does_not_fabricate_counts_or_provider_success() {
    for usage in [
        json!("unknown"),
        json!([]),
        json!({"input_tokens":-1,"output_tokens":1}),
        json!({"input_tokens":1,"output_tokens":1.5}),
        json!({"input_tokens":"1"}),
        json!({"output_tokens":null}),
        serde_json::from_str(r#"{"input_tokens":18446744073709551616}"#).unwrap(),
    ] {
        let mut value = success();
        value["usage"] = usage;
        assert_eq!(
            protocol(&bytes(&[value])).unwrap().result().err(),
            Some(ClaudeResultError::InvalidUsage)
        );
    }
}

#[test]
fn duplicate_raw_members_refuse_semantics_without_changing_protocol_evidence() {
    const SUCCESS: &str = r#"{"type":"result","subtype":"success","is_error":false,"result":"ok"}"#;
    for ambiguous in [
        r#"{"type":"result","subtype":"success","is_error":true,"is_error":false,"result":"ok"}"#,
        r#"{"type":"result","subtype":"success","is_error":false,"is_error":true,"result":"ok"}"#,
        r#"{"type":"result","subtype":"error_max_turns","subtype":"success","is_error":false,"result":"ok"}"#,
        r#"{"type":"result","subtype":"success","subtype":"error_max_turns","is_error":false,"result":"ok"}"#,
        r#"{"type":"result","subtype":"success","is_error":true,"is_\u0065rror":false,"result":"ok"}"#,
        r#"{"type":"result","subtype":"success","is_\u0065rror":false,"is_error":true,"result":"ok"}"#,
        r#"{"type":"result","subtype":"error_max_turns","is_error":true,"errors":["refused"],"type":"system"}"#,
        r#"{"type":"system","subtype":"error_max_turns","is_error":true,"errors":["refused"],"type":"result"}"#,
        r#"{"type":"result","subtype":"error_max_turns","is_error":true,"errors":["refused"],"ty\u0070e":"system"}"#,
        r#"{"ty\u0070e":"system","subtype":"error_max_turns","is_error":true,"errors":["refused"],"type":"result"}"#,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{"input_tokens":1,"input_tokens":2,"output_tokens":3}}"#,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{"input_tokens":2,"input_tokens":1,"output_tokens":3}}"#,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{"input_tokens":1,"output_tokens":2,"output_\u0074okens":3}}"#,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{"input_tokens":1,"output_\u0074okens":3,"output_tokens":2}}"#,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":null,"usage":{"input_tokens":1,"output_tokens":2}}"#,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{"input_tokens":1,"output_tokens":2},"usage":null}"#,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"first","result":"second"}"#,
        r#"{"type":"result","subtype":"success","is_error":false,"result":"ok","is_error":false}"#,
        r#"{"type":"future","payload":[{"nested":1,"nested":2}]}"#,
    ] {
        for observation in [
            ambiguous.to_owned(),
            format!("{ambiguous}\n{SUCCESS}\n"),
            format!("{SUCCESS}\n{ambiguous}\n"),
        ] {
            let input = Some(observation.as_bytes().to_vec());
            let protocol = protocol(&input).unwrap();
            assert_eq!(
                protocol.result().err(),
                Some(ClaudeResultError::InvalidResult)
            );
            assert_eq!(
                protocol
                    .records()
                    .iter()
                    .flat_map(|r| r.raw_record().iter().copied())
                    .collect::<Vec<_>>(),
                observation.as_bytes()
            );
            for record in protocol.records() {
                assert_eq!(
                    record.value(),
                    &serde_json::from_slice::<Value>(record.raw_record()).unwrap()
                );
            }
        }
    }
}

#[test]
fn unique_escaped_keys_and_key_looking_strings_keep_their_meaning() {
    let unknown = r#"{"type":"future","payload":[{"same":1},{"same":2}],"text":"\"type\":\"result\",{}[]:\\"}"#;
    for (result, expected) in [
        (
            r#"{"type":"result","subtype":"success","is_\u0065rror":false,"result":"\"is_error\":true,\"is_error\":false; \\ \u0022 {}[]:","usage":{"input_tokens":1,"output_tokens":2},"future":{"input_tokens":3}}"#,
            ClaudeProviderResult::Success,
        ),
        (
            r#"{"type":"result","subtype":"error_max_turns","is_error":true,"errors":["\"subtype\":\"success\"","\\\" {}[]:"]}"#,
            ClaudeProviderResult::Error,
        ),
        (
            r#"{"type":"result","subtype":"future","is_error":false}"#,
            ClaudeProviderResult::Unknown,
        ),
    ] {
        let input = Some(format!("{unknown}\n{result}\n{unknown}").into_bytes());
        let protocol = protocol(&input).unwrap();
        assert_eq!(protocol.result().unwrap().provider_result(), expected);
        assert_eq!(protocol.records().len(), 3);
        assert_eq!(
            protocol.records()[0].raw_record(),
            format!("{unknown}\n").as_bytes()
        );
        assert_eq!(protocol.records()[2].raw_record(), unknown.as_bytes());
        assert_eq!(protocol.records()[0].value(), protocol.records()[2].value());
    }
}

#[test]
fn duplicate_key_refusal_diagnostics_do_not_expose_keys_or_values() {
    const SECRET: &str = "sensitive-provider-session-prompt-marker";
    let input = Some(format!(r#"{{"type":"result","subtype":"success","is_error":false,"result":"{SECRET}","{SECRET}":1,"{SECRET}":2}}"#).into_bytes());
    let protocol = protocol(&input).unwrap();
    let error = protocol.result().err().unwrap();
    assert_eq!(error, ClaudeResultError::InvalidResult);
    assert!(!format!("{error:?} {error}").contains(SECRET));
    assert!(error.source().is_none());
}

#[test]
fn protocol_damage_including_after_success_prevents_any_interpretation() {
    for (damage, expected) in [
        (
            &b"{bad}\n"[..],
            ClaudeStreamJsonError::InvalidJson { line: 2 },
        ),
        (
            &b"\xff\n"[..],
            ClaudeStreamJsonError::InvalidUtf8 { line: 2 },
        ),
        (
            &b"{\"type\":"[..],
            ClaudeStreamJsonError::IncompleteJson { line: 2 },
        ),
        (
            &b"[]\n"[..],
            ClaudeStreamJsonError::ExpectedObject { line: 2 },
        ),
    ] {
        let mut input = bytes(&[success()]).unwrap();
        input.extend_from_slice(damage);
        assert_eq!(protocol(&Some(input)).err(), Some(expected));
        assert_eq!(
            protocol(&Some(damage.to_vec()))
                .err()
                .map(|e| std::mem::discriminant(&e)),
            Some(std::mem::discriminant(&expected))
        );
    }
    assert_eq!(
        protocol(&None).err(),
        Some(ClaudeStreamJsonError::StdoutTruncated)
    );
    let input = Some(success().to_string().into_bytes());
    assert_eq!(
        protocol(&input)
            .unwrap()
            .result()
            .unwrap()
            .provider_result(),
        ClaudeProviderResult::Success
    );
}

#[test]
fn semantic_and_protocol_diagnostics_have_no_payload_or_source() {
    const SECRET: &str = "sensitive-provider-session-prompt-marker";
    for error in [
        ClaudeResultError::MissingResult,
        ClaudeResultError::MultipleResults,
        ClaudeResultError::InvalidResult,
        ClaudeResultError::InvalidUsage,
    ] {
        assert!(!format!("{error:?} {error}").contains(SECRET));
        assert!(error.source().is_none());
    }
    let mut value = success();
    value["subtype"] = json!(SECRET);
    value["is_error"] = json!(SECRET);
    let input = bytes(&[value]);
    let protocol = protocol(&input).unwrap();
    let error = protocol.result().err().unwrap();
    assert!(!format!("{error:?} {error}").contains(SECRET));
    assert!(error.source().is_none());
    for input in [
        Some(format!("{{\"{SECRET}\":").into_bytes()),
        Some([format!("{{\"{SECRET}\":0}}\n").as_bytes(), &[0xff]].concat()),
    ] {
        let error = crate::claude_stream_json::protocol(&input).err().unwrap();
        assert!(!format!("{error:?} {error}").contains(SECRET));
        assert!(error.source().is_none());
    }
}
