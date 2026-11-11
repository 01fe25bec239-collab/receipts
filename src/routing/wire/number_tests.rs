//! F5/F6: accepted numeric wire values stay exact; schema-valid numbers the
//! carriers cannot hold exactly are `Refused`, never malformed `Shape`.
//!
//! The expectation tables below were produced by an independent oracle (CPython
//! exact decimal parsing and shortest `repr`, `/Users/omkar/Documents/A1_receipts/
//! model-routing/wave2-mr-w2-repair-2026-11-08/oracle.py`), not by this crate.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Class {
    Accept,
    Refused,
    Shape,
}

pub(super) const F64_CASES: &[(&str, Class)] = &[
    ("0", Class::Accept),
    ("0.0", Class::Accept),
    ("1", Class::Accept),
    ("1.0", Class::Accept),
    ("1e2", Class::Accept),
    ("1E2", Class::Accept),
    ("100e-2", Class::Accept),
    ("3.0e0", Class::Accept),
    ("0.1", Class::Accept),
    ("0.5", Class::Accept),
    ("12.5", Class::Accept),
    ("2e-1", Class::Accept),
    ("0.30000000000000004", Class::Accept),
    ("9007199254740991", Class::Accept),
    ("9007199254740992", Class::Accept),
    ("9007199254740993", Class::Refused),
    ("9007199254740994", Class::Accept),
    ("18446744073709551615", Class::Refused),
    ("18446744073709551616", Class::Refused),
    ("123456789012345678", Class::Refused),
    ("1e21", Class::Accept),
    ("1e22", Class::Accept),
    ("1e23", Class::Accept),
    ("1e308", Class::Accept),
    ("1.7976931348623157e308", Class::Accept),
    ("1.7976931348623159e308", Class::Refused),
    ("1e309", Class::Refused),
    ("1e400", Class::Refused),
    ("1e99999999999999999999", Class::Refused),
    ("1e-99999999999999999999", Class::Refused),
    ("5e-324", Class::Accept),
    ("4.9e-324", Class::Refused),
    ("1e-323", Class::Accept),
    ("1e-325", Class::Refused),
    ("1e-400", Class::Refused),
    ("0.10000000000000001", Class::Refused),
    ("0.1000000000000000055511151231257827", Class::Refused),
    ("0.0e-999999999999", Class::Accept),
    ("1E+2", Class::Accept),
    ("340282366920938463463374607431768211456", Class::Refused),
];
pub(super) const U64_CASES: &[(&str, Class)] = &[
    ("0", Class::Accept),
    ("1", Class::Accept),
    ("18446744073709551615", Class::Accept),
    ("18446744073709551616", Class::Refused),
    ("1e400", Class::Refused),
    ("1e-400", Class::Shape),
    ("1.5", Class::Shape),
    ("1.0", Class::Refused),
    ("1e2", Class::Refused),
    ("1E2", Class::Refused),
    ("100e-2", Class::Refused),
    ("-0", Class::Refused),
    ("-1", Class::Shape),
    ("-0.0", Class::Refused),
    ("0.0e-999999999999", Class::Refused),
    ("1e99999999999999999999", Class::Refused),
    ("1e-99999999999999999999", Class::Shape),
    ("9223372036854775807", Class::Accept),
    ("9223372036854775808", Class::Accept),
    ("-9223372036854775808", Class::Shape),
    ("-9223372036854775809", Class::Shape),
    ("1180591620717411303424", Class::Refused),
    ("0.5", Class::Shape),
    ("1.0e0", Class::Refused),
];
pub(super) const I64_CASES: &[(&str, Class)] = &[
    ("0", Class::Accept),
    ("1", Class::Accept),
    ("18446744073709551615", Class::Refused),
    ("18446744073709551616", Class::Refused),
    ("1e400", Class::Refused),
    ("1e-400", Class::Shape),
    ("1.5", Class::Shape),
    ("1.0", Class::Refused),
    ("1e2", Class::Refused),
    ("1E2", Class::Refused),
    ("100e-2", Class::Refused),
    ("-0", Class::Refused),
    ("-1", Class::Accept),
    ("-0.0", Class::Refused),
    ("0.0e-999999999999", Class::Refused),
    ("1e99999999999999999999", Class::Refused),
    ("1e-99999999999999999999", Class::Shape),
    ("9223372036854775807", Class::Accept),
    ("9223372036854775808", Class::Refused),
    ("-9223372036854775808", Class::Accept),
    ("-9223372036854775809", Class::Refused),
    ("1180591620717411303424", Class::Refused),
    ("0.5", Class::Shape),
    ("1.0e0", Class::Refused),
];

const T0: &str = "2026-11-08T10:00:00Z";

/// Test-side exact decimal identity: (negative, significant digits, exponent).
fn decimal(lexeme: &str) -> Option<(bool, String, i128)> {
    let (negative, rest) = match lexeme.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, lexeme),
    };
    let (mantissa, exponent) = rest.split_once(['e', 'E']).unwrap_or((rest, "0"));
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = format!("{int}{frac}");
    let digits = digits.trim_start_matches('0');
    let stripped = digits.trim_end_matches('0');
    if stripped.is_empty() {
        return None; // zero, whatever its spelling or sign
    }
    let exponent: i128 = exponent.trim_start_matches('+').parse().unwrap();
    let exponent = exponent - frac.len() as i128 + (digits.len() - stripped.len()) as i128;
    Some((negative, stripped.to_owned(), exponent))
}

/// The number token that follows `"key":` in `json`.
fn token<'a>(json: &'a str, key: &str) -> &'a str {
    let at = json
        .find(&format!("\"{key}\":"))
        .unwrap_or_else(|| panic!("{key} missing in {json}"));
    let rest = &json[at + key.len() + 3..];
    let end = rest.find([',', '}', ']']).unwrap_or(rest.len());
    &rest[..end]
}

type Roundtrip = fn(&str) -> Result<String, RoutingWireError>;

struct Field {
    name: &'static str,
    key: &'static str,
    doc: fn(&str) -> String,
    run: Roundtrip,
}

fn quota(key: &'static str) -> impl Fn(&str) -> String {
    move |l| {
        format!(r#"{{"provider_id":"p","quota_scope":"UNKNOWN","observed_at":"{T0}","{key}":{l}}}"#)
    }
}

fn quota_run(j: &str) -> Result<String, RoutingWireError> {
    decode_quota_state(j).map(|v| encode_quota_state(&v))
}
fn request_run(j: &str) -> Result<String, RoutingWireError> {
    decode_routing_request(j).map(|v| encode_routing_request(&v))
}
fn avail_run(j: &str) -> Result<String, RoutingWireError> {
    decode_availability_state(j).map(|v| encode_availability_state(&v))
}
fn obs_run(j: &str) -> Result<String, RoutingWireError> {
    decode_model_observation(j).map(|v| encode_model_observation(&v))
}
fn cap_run(j: &str) -> Result<String, RoutingWireError> {
    decode_model_capability(j).map(|v| encode_model_capability(&v))
}
fn decision_run(j: &str) -> Result<String, RoutingWireError> {
    decode_routing_decision(j).map(|v| encode_routing_decision(&v))
}

const REQUEST: &str = r#""request_id":"r","role":"REVIEWER","task_class":"ECONOMY_DOCS","quality_floor":"ECONOMY","required_capabilities":["docs"]"#;
const OBS: &str = r#""observation_id":"o","provider_id":"p","model_id":"m","runtime_id":"r","task_class":"","outcome":"TIMEOUT""#;

fn decision(from: &str, to: &str) -> String {
    let full = include_str!("fixtures/RoutingDecision.valid.full.json");
    assert!(full.contains(from), "{from}");
    full.replace(from, to)
}

fn f64_fields() -> Vec<Field> {
    vec![
        Field {
            name: "QuotaState.limit",
            key: "limit",
            doc: |l| quota("limit")(l),
            run: quota_run,
        },
        Field {
            name: "QuotaState.remaining",
            key: "remaining",
            doc: |l| quota("remaining")(l),
            run: quota_run,
        },
        Field {
            name: "RoutingRequest.constraints.max_cost",
            key: "max_cost",
            doc: |l| format!(r#"{{{REQUEST},"constraints":{{"max_cost":{l}}}}}"#),
            run: request_run,
        },
        Field {
            name: "ModelObservation.cost",
            key: "cost",
            doc: |l| format!(r#"{{{OBS},"observed_at":"{T0}","cost":{l}}}"#),
            run: obs_run,
        },
        Field {
            name: "ModelObservation.latency_seconds",
            key: "latency_seconds",
            doc: |l| format!(r#"{{{OBS},"observed_at":"{T0}","latency_seconds":{l}}}"#),
            run: obs_run,
        },
        Field {
            name: "ModelObservation.cost_to_accepted_result",
            key: "cost_to_accepted_result",
            doc: |l| format!(r#"{{{OBS},"observed_at":"{T0}","cost_to_accepted_result":{l}}}"#),
            run: obs_run,
        },
        Field {
            name: "RoutingDecision.expected_implementation_cost",
            key: "expected_implementation_cost",
            doc: |l| {
                decision(
                    "\"expected_implementation_cost\": 1.5",
                    &format!("\"expected_implementation_cost\": {l}"),
                )
            },
            run: decision_run,
        },
        Field {
            name: "RoutingDecision.expected_repair_cost",
            key: "expected_repair_cost",
            doc: |l| {
                decision(
                    "\"expected_repair_cost\": 0.5",
                    &format!("\"expected_repair_cost\": {l}"),
                )
            },
            run: decision_run,
        },
        Field {
            name: "RoutingDecision.expected_review_cost",
            key: "expected_review_cost",
            doc: |l| {
                decision(
                    "\"expected_review_cost\": 0.75",
                    &format!("\"expected_review_cost\": {l}"),
                )
            },
            run: decision_run,
        },
        Field {
            name: "RoutingDecision.latency_estimate_seconds",
            key: "latency_estimate_seconds",
            doc: |l| {
                decision(
                    "\"latency_estimate_seconds\": 12.0",
                    &format!("\"latency_estimate_seconds\": {l}"),
                )
            },
            run: decision_run,
        },
        Field {
            name: "RoutingDecision.alternative.score",
            key: "score",
            doc: |l| decision("\"score\": 0.5", &format!("\"score\": {l}")),
            run: decision_run,
        },
    ]
}

fn u64_fields() -> Vec<Field> {
    vec![
        Field {
            name: "QuotaState.retry_after_seconds",
            key: "retry_after_seconds",
            doc: |l| quota("retry_after_seconds")(l),
            run: quota_run,
        },
        Field {
            name: "AvailabilityState.retry_after_seconds",
            key: "retry_after_seconds",
            doc: |l| {
                format!(
                    r#"{{"provider_id":"p","state":"UNKNOWN","observed_at":"{T0}","retry_after_seconds":{l}}}"#
                )
            },
            run: avail_run,
        },
        Field {
            name: "RoutingRequest.context_size_hint",
            key: "context_size_hint",
            doc: |l| format!(r#"{{{REQUEST},"context_size_hint":{l}}}"#),
            run: request_run,
        },
        Field {
            name: "ModelObservation.repair_count",
            key: "repair_count",
            doc: |l| format!(r#"{{{OBS},"observed_at":"{T0}","repair_count":{l}}}"#),
            run: obs_run,
        },
        Field {
            name: "ModelCapability.sample_size",
            key: "sample_size",
            doc: |l| {
                format!(
                    r#"{{"subject_kind":"RUNTIME","capability":"c","confidence":"LOCAL_EMPIRICAL","observed_at":"{T0}","sample_size":{l}}}"#
                )
            },
            run: cap_run,
        },
        Field {
            name: "RoutingDecision.evidence.sample_size",
            key: "sample_size",
            doc: |l| decision("\"sample_size\": 3", &format!("\"sample_size\": {l}")),
            run: decision_run,
        },
    ]
}

fn i64_fields() -> Vec<Field> {
    vec![
        Field {
            name: "ModelObservation.tokens",
            key: "tokens",
            doc: |l| format!(r#"{{{OBS},"observed_at":"{T0}","tokens":{l}}}"#),
            run: obs_run,
        },
        Field {
            name: "RoutingDecision.model_list_age_seconds",
            key: "model_list_age_seconds",
            doc: |l| {
                decision(
                    "\"model_list_age_seconds\": 60",
                    &format!("\"model_list_age_seconds\": {l}"),
                )
            },
            run: decision_run,
        },
        Field {
            name: "RoutingDecision.capability_age_seconds",
            key: "capability_age_seconds",
            doc: |l| {
                decision(
                    "\"capability_age_seconds\": -5",
                    &format!("\"capability_age_seconds\": {l}"),
                )
            },
            run: decision_run,
        },
        Field {
            name: "RoutingDecision.calibration_sample_size",
            key: "calibration_sample_size",
            doc: |l| {
                decision(
                    "\"calibration_sample_size\": 0",
                    &format!("\"calibration_sample_size\": {l}"),
                )
            },
            run: decision_run,
        },
    ]
}

fn check(fields: &[Field], cases: &[(&str, Class)]) {
    for field in fields {
        for (lexeme, expected) in cases {
            let result = (field.run)(&(field.doc)(lexeme));
            let actual = match &result {
                Ok(_) => Class::Accept,
                Err(RoutingWireError::Refused(_)) => Class::Refused,
                Err(RoutingWireError::Shape(_)) => Class::Shape,
                Err(RoutingWireError::Json) => panic!("{}: {lexeme} is JSON", field.name),
            };
            assert_eq!(actual, *expected, "{} = {lexeme}: {result:?}", field.name);
            if let Ok(encoded) = result {
                // Exactness: the wire value comes back mathematically unchanged.
                let out = token(&encoded, field.key);
                assert_eq!(
                    decimal(out),
                    decimal(lexeme),
                    "{}: {lexeme} -> {out}",
                    field.name
                );
                assert_eq!(
                    (field.run)(&encoded).unwrap(),
                    encoded,
                    "{}: idempotent",
                    field.name
                );
            }
        }
    }
}

#[test]
fn f64_fields_keep_accepted_values_exact_and_refuse_the_rest_as_unsupported() {
    check(&f64_fields(), F64_CASES);
}

#[test]
fn unsigned_integer_fields_never_misreport_overflow_as_malformed() {
    check(&u64_fields(), U64_CASES);
}

#[test]
fn signed_integer_fields_never_misreport_overflow_as_malformed() {
    check(&i64_fields(), I64_CASES);
}

#[test]
fn open_capability_values_keep_exact_numbers_or_are_refused() {
    let doc = |l: &str| {
        format!(
            r#"{{"subject_kind":"RUNTIME","capability":"c","value":{l},"confidence":"LOCAL_EMPIRICAL","observed_at":"{T0}"}}"#
        )
    };
    for (lexeme, expected) in [
        ("5", Class::Accept),
        ("-5", Class::Accept),
        ("0.1", Class::Accept),
        ("18446744073709551615", Class::Accept),
        ("-9223372036854775808", Class::Accept),
        ("9007199254740993", Class::Accept), // exact as an integer
        ("18446744073709551616", Class::Refused),
        ("1e400", Class::Refused),
        ("1e-400", Class::Refused),
        ("4.9e-324", Class::Refused),
    ] {
        let result = cap_run(&doc(lexeme));
        let actual = match &result {
            Ok(_) => Class::Accept,
            Err(RoutingWireError::Refused(_)) => Class::Refused,
            Err(e) => panic!("{lexeme}: {e:?}"),
        };
        assert_eq!(actual, expected, "{lexeme}: {result:?}");
        if let Ok(encoded) = result {
            assert_eq!(
                decimal(token(&encoded, "value")),
                decimal(lexeme),
                "{lexeme}"
            );
        }
    }
    // Nested numbers are held to the same rule.
    assert!(matches!(
        cap_run(&doc(r#"{"limits":[1,2,1e400]}"#)),
        Err(RoutingWireError::Refused(_))
    ));
    assert!(cap_run(&doc(r#"{"limits":[1,2.5,{"n":null}]}"#)).is_ok());
}

#[test]
fn unit_interval_is_judged_on_the_exact_value() {
    let doc = |l: &str| {
        decision(
            "\"expected_rejection_probability\": 0.25",
            &format!("\"expected_rejection_probability\": {l}"),
        )
    };
    for (lexeme, expected) in [
        ("0", Class::Accept),
        ("1", Class::Accept),
        ("0.25", Class::Accept),
        ("1e-400", Class::Refused), // inside [0,1] but not representable
        ("1e400", Class::Shape),    // outside the schema's range
        ("1.0000000000000000000001", Class::Shape),
        ("-1e-400", Class::Shape),
    ] {
        let result = decision_run(&doc(lexeme));
        let actual = match &result {
            Ok(_) => Class::Accept,
            Err(RoutingWireError::Refused(_)) => Class::Refused,
            Err(RoutingWireError::Shape(_)) => Class::Shape,
            Err(e) => panic!("{e:?}"),
        };
        assert_eq!(actual, expected, "{lexeme}: {result:?}");
    }
}
