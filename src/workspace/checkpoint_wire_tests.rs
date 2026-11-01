use serde_json::{Value, json};

use super::*;
use crate::test_support::TestRepo;
use crate::{
    WorkspaceCheckpointEvidenceCaptureRequest, capture_workspace_checkpoint_temporal_evidence,
};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn minimal() -> Value {
    json!({
        "checkpoint_id": "cp",
        "workspace_id": "ws",
        "head_sha": SHA,
        "captured_at": "2026-10-30T00:00:00Z",
        "kind": "PROGRESS"
    })
}

fn full() -> Value {
    json!({
        "checkpoint_id": "cp",
        "workspace_id": "ws",
        "task_id": "task",
        "attempt_id": "attempt",
        "kind": "RECOVERY_CAPTURE",
        "head_sha": SHA,
        "base_sha": SHA,
        "dirty_diff_ref": {"ref_type": "ARTIFACT_ID", "target": "a", "digest": "", "section": ""},
        "modified_files": ["m"],
        "untracked_files": [],
        "executed_checks": [{
            "source": "WORKER_EXECUTION",
            "command": ["cargo", "test"],
            "exit_code": 0,
            "code_sha": SHA,
            "timed_out": false,
            "started_at": "2026-10-30T00:00:00Z",
            "finished_at": "2026-10-30T00:00:01Z",
            "output_ref": {"ref_type": "URL", "target": "u"},
            "result": "PASS"
        }],
        "crash_classification": "UNKNOWN",
        "recovery_decision": "INSPECT_AND_SALVAGE",
        "recovery_rationale": "",
        "captured_at": "2026-10-30T00:00:02Z"
    })
}

fn decode(value: &Value) -> Result<WorkspaceCheckpointWire, serde_json::Error> {
    WorkspaceCheckpointWire::from_json(&value.to_string())
}

#[test]
fn checkpoint_json_roundtrip_matches_schema_shapes() {
    for value in [minimal(), full()] {
        let wire = decode(&value).unwrap();
        let encoded: Value = serde_json::from_str(&wire.to_json().unwrap()).unwrap();
        assert_eq!(encoded, value);
        assert_eq!(
            WorkspaceCheckpointWire::from_json(&wire.to_json().unwrap()).unwrap(),
            wire
        );
    }
}

#[test]
fn absence_and_present_empty_arrays_stay_distinct() {
    let absent = decode(&minimal()).unwrap();
    assert_eq!(absent.modified_files(), None);
    assert_eq!(absent.executed_check_count(), None);
    let mut value = minimal();
    value["modified_files"] = json!([]);
    value["executed_checks"] = json!([]);
    let present = decode(&value).unwrap();
    assert_eq!(present.modified_files(), Some(&[][..]));
    assert_eq!(present.executed_check_count(), Some(0));
    assert!(!absent.to_json().unwrap().contains("modified_files"));
    assert!(present.to_json().unwrap().contains("\"modified_files\":[]"));
    assert_ne!(absent, present);
}

#[test]
fn exact_integer_tokens_are_preserved_beyond_i64() {
    for (token, operational) in [
        ("0", Some(0)),
        ("-0", Some(0)),
        ("1e2", Some(100)),
        ("1.0", Some(1)),
        ("12.50e1", Some(125)),
        ("-9223372036854775808", Some(i64::MIN)),
        ("9223372036854775807", Some(i64::MAX)),
        ("9223372036854775808", None),
        ("123456789012345678901234567890", None),
        ("1e400", None),
        ("0e999999999999999999999999", Some(0)),
    ] {
        let mut value = full();
        value["executed_checks"][0]["exit_code"] = json!(0);
        let json = value
            .to_string()
            .replace("\"exit_code\":0", &format!("\"exit_code\":{token}"));
        let wire = WorkspaceCheckpointWire::from_json(&json).unwrap();
        let code = wire.executed_check_exit_code(0).unwrap();
        assert_eq!(code.as_json(), token);
        assert_eq!(code.as_i64(), operational, "{token}");
        assert!(
            wire.to_json()
                .unwrap()
                .contains(&format!("\"exit_code\":{token}"))
        );
        match operational {
            Some(expected) => assert_eq!(
                wire.to_temporal_capture_core()
                    .unwrap()
                    .core()
                    .executed_checks()[0]
                    .exit_code(),
                expected
            ),
            None => assert_eq!(
                wire.to_temporal_capture_core().unwrap_err(),
                WorkspaceCheckpointWireConversionError::ExitCodeOutOfRange { index: 0 }
            ),
        }
    }
    for token in [
        "1.5",
        "1e-1",
        "\"1\"",
        "true",
        "null",
        "[0]",
        "1.0000000001",
    ] {
        let json = full()
            .to_string()
            .replace("\"exit_code\":0", &format!("\"exit_code\":{token}"));
        assert!(
            WorkspaceCheckpointWire::from_json(&json).is_err(),
            "{token}"
        );
    }
}

#[test]
fn integrality_matches_independent_cases() {
    for yes in [
        "0", "-0", "0.0", "10e-1", "100e-2", "1E+2", "5e0", "0.5e1", "1.50e1",
    ] {
        assert!(is_json_integer(yes), "{yes}");
    }
    for no in ["0.5", "1e-1", "15e-1", "1.05e1", "\"1\"", "t", ""] {
        assert!(!is_json_integer(no), "{no}");
    }
}

#[test]
fn invalid_null_unknown_duplicate_and_domain_values_refuse() {
    let mut cases = Vec::new();
    for (field, bad) in [
        ("checkpoint_id", json!("")),
        ("workspace_id", json!("x".repeat(201))),
        ("task_id", Value::Null),
        ("kind", json!("PROGRES")),
        ("head_sha", json!(SHA.to_uppercase())),
        ("base_sha", json!(format!("{SHA}\n"))),
        ("modified_files", Value::Null),
        ("modified_files", json!([1])),
        ("crash_classification", json!("TIMEOUTS")),
        ("recovery_decision", Value::Null),
        ("unknown", json!(1)),
        ("dirty_diff_ref", json!({"ref_type": "URL", "target": ""})),
        (
            "dirty_diff_ref",
            json!({"ref_type": "URL", "target": "x", "extra": 1}),
        ),
    ] {
        let mut value = full();
        value[field] = bad;
        cases.push(value.to_string());
    }
    let mut empty_command = full();
    empty_command["executed_checks"][0]["command"] = json!([]);
    cases.push(empty_command.to_string());
    let mut bad_result = full();
    bad_result["executed_checks"][0]["result"] = json!("OK");
    cases.push(bad_result.to_string());
    let mut missing = minimal();
    missing.as_object_mut().unwrap().remove("captured_at");
    cases.push(missing.to_string());
    cases.push(r#"{"checkpoint_id":"a","checkpoint_id":"b","workspace_id":"w","head_sha":"0123456789abcdef0123456789abcdef01234567","captured_at":"t","kind":"PROGRESS"}"#.into());
    cases.push("[]".into());
    for case in cases {
        assert!(WorkspaceCheckpointWire::from_json(&case).is_err(), "{case}");
    }
    // 200 multibyte characters are admitted.
    let mut value = minimal();
    value["workspace_id"] = json!("🦀".repeat(200));
    decode(&value).unwrap();
}

/// F3: every nested check and reference is a JSON object. A serde struct derive
/// also admits sequences, so a positional array must refuse instead of being
/// silently re-encoded as an object.
#[test]
fn nested_checks_and_references_require_json_objects_not_positional_arrays() {
    // The exact retained regression input and its object counterpart.
    let mut retained = minimal();
    retained["dirty_diff_ref"] = json!(["ARTIFACT_ID", "synthetic", "", ""]);
    let error = decode(&retained).unwrap_err().to_string();
    assert!(error.contains("a JSON object"), "{error}");
    let mut object = minimal();
    object["dirty_diff_ref"] =
        json!({"ref_type": "ARTIFACT_ID", "target": "synthetic", "digest": "", "section": ""});
    let accepted = decode(&object).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&accepted.to_json().unwrap()).unwrap(),
        object,
        "the object form keeps present-empty digest/section"
    );

    let positional_check = json!([
        "WORKER_EXECUTION",
        ["cargo", "test"],
        0,
        SHA,
        false,
        "2026-10-30T00:00:00Z",
        "2026-10-30T00:00:01Z",
        {"ref_type": "URL", "target": "u"},
        "PASS"
    ]);
    let mut check_array = full();
    check_array["executed_checks"] = json!([positional_check]);
    let mut empty_check = full();
    empty_check["executed_checks"] = json!([[]]);
    let mut second_check_array = full();
    let first = second_check_array["executed_checks"][0].clone();
    second_check_array["executed_checks"] = json!([first, ["WORKER_EXECUTION"]]);
    let mut output_array = full();
    output_array["executed_checks"][0]["output_ref"] = json!(["URL", "u", "", ""]);
    let mut dirty_scalar = full();
    dirty_scalar["dirty_diff_ref"] = json!(7);
    let mut check_null = full();
    check_null["executed_checks"][0]["output_ref"] = Value::Null;
    for (name, case) in [
        ("executed_checks[0] positional array", check_array),
        ("executed_checks[0] empty array", empty_check),
        ("executed_checks[1] positional array", second_check_array),
        ("output_ref positional array", output_array),
        ("dirty_diff_ref scalar", dirty_scalar),
        ("output_ref null", check_null),
    ] {
        assert!(decode(&case).is_err(), "{name} must refuse");
    }

    // Valid object-only documents still round-trip exactly, including optional
    // absence versus present-empty and an exact large integer exit token.
    let mut valid = full();
    valid["executed_checks"][0]["exit_code"] = json!(0);
    let exact = valid
        .to_string()
        .replace("\"exit_code\":0", "\"exit_code\":1e2");
    let wire = WorkspaceCheckpointWire::from_json(&exact).unwrap();
    assert_eq!(wire.executed_check_exit_code(0).unwrap().as_json(), "1e2");
    assert_eq!(
        WorkspaceCheckpointWire::from_json(&wire.to_json().unwrap()).unwrap(),
        wire
    );
    assert_eq!(wire.executed_check_count(), Some(1));
}

#[test]
fn date_time_is_annotation_on_wire_and_validated_on_conversion() {
    let mut value = full();
    value["captured_at"] = json!("not a timestamp");
    let wire = decode(&value).unwrap();
    assert_eq!(wire.captured_at(), "not a timestamp");
    assert_eq!(
        wire.to_temporal_capture_core().unwrap_err(),
        WorkspaceCheckpointWireConversionError::TimestampUnsupported {
            field: "captured_at"
        }
    );
    assert_eq!(
        decode(&minimal())
            .unwrap()
            .to_temporal_capture_core()
            .unwrap_err(),
        WorkspaceCheckpointWireConversionError::ArrayAbsent {
            field: "executed_checks"
        }
    );
}

#[test]
fn actual_capture_exports_decodes_and_converts_losslessly() {
    let repo = TestRepo::new("checkpoint-wire");
    std::fs::write(repo.path().join("new"), "x").unwrap();
    let base = repo.head_sha();
    let captured = capture_workspace_checkpoint_temporal_evidence(
        WorkspaceCheckpointEvidenceCaptureRequest {
            directory: repo.path(),
            checkpoint_id: "cp".into(),
            workspace_id: "ws".into(),
            task_id: None,
            attempt_id: Some("a".into()),
            kind: WorkspaceCheckpointKind::PreTermination,
            base_sha: Some(&base),
            dirty_diff_ref: None,
            executed_checks: Vec::new(),
        },
        WorkspaceDateTimeV1::try_new("0000-01-01T23:59:60Z").unwrap(),
    )
    .unwrap();
    let exported = crate::workspace_checkpoint_to_json(&captured).unwrap();
    let wire = WorkspaceCheckpointWire::try_from(&captured).unwrap();
    assert_eq!(wire.to_json().unwrap(), exported, "byte-exact re-encoding");
    assert_eq!(wire.untracked_files(), Some(&["new".to_string()][..]));
    assert_eq!(wire.to_temporal_capture_core().unwrap(), captured);
}

/// Independent decoder oracle. Each case is `(JSON text, python-jsonschema
/// verdict, decoder verdict)`; the enum values come from the frozen schema file
/// itself, not from the Rust vocabularies. Cases where the two verdicts differ
/// are the documented, deliberate divergences:
/// * Python's `$` also matches before a trailing newline, so a SHA with `\n`
///   passes the pattern while the decoder (strict SHA40) refuses it;
/// * `json.loads` keeps the last duplicate key, the decoder refuses duplicates;
/// * `json.loads` turns `1e400` into infinity (not an integer) while the
///   decoder preserves the exact integer token.
fn decoder_oracle_cases() -> Vec<(String, bool, bool)> {
    let schema: Value = serde_json::from_str(include_str!(
        "../../schemas/WorkspaceCheckpoint.schema.json"
    ))
    .unwrap();
    let enum_values = |field: &str| -> Vec<String> {
        schema["properties"][field]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect()
    };
    let mut cases: Vec<(String, bool, bool)> = Vec::new();
    let mut agree = |value: Value, valid: bool| cases.push((value.to_string(), valid, valid));
    agree(minimal(), true);
    agree(full(), true);
    let mut present_empty = minimal();
    present_empty["modified_files"] = json!([]);
    present_empty["untracked_files"] = json!([]);
    present_empty["executed_checks"] = json!([]);
    present_empty["recovery_rationale"] = json!("");
    agree(present_empty, true);
    let mut crabs = minimal();
    crabs["workspace_id"] = json!("🦀".repeat(200));
    agree(crabs, true);
    for kind in enum_values("kind") {
        let mut value = minimal();
        value["kind"] = json!(kind);
        agree(value, true);
    }
    for crash in enum_values("crash_classification") {
        let mut value = minimal();
        value["crash_classification"] = json!(crash);
        agree(value, true);
    }
    for decision in enum_values("recovery_decision") {
        let mut value = minimal();
        value["recovery_decision"] = json!(decision);
        agree(value, true);
    }
    // Negative controls: schema-invalid input the decoder must also refuse.
    for (field, bad) in [
        ("checkpoint_id", json!("")),
        ("workspace_id", json!("x".repeat(201))),
        ("task_id", Value::Null),
        ("kind", json!("PROGRES")),
        ("head_sha", json!(SHA.to_uppercase())),
        ("head_sha", json!(&SHA[1..])),
        ("modified_files", Value::Null),
        ("modified_files", json!([1])),
        ("crash_classification", json!("TIMEOUTS")),
        ("recovery_decision", Value::Null),
        ("recovery_rationale", Value::Null),
        ("unknown", json!(1)),
        ("dirty_diff_ref", json!({"ref_type": "URL"})),
        (
            "dirty_diff_ref",
            json!({"ref_type": "URL", "target": "x", "extra": 1}),
        ),
        // Positional arrays are not objects (F3).
        (
            "dirty_diff_ref",
            json!(["ARTIFACT_ID", "synthetic", "", ""]),
        ),
        ("dirty_diff_ref", json!(["URL", "x"])),
        ("dirty_diff_ref", json!([])),
        ("dirty_diff_ref", json!("URL")),
        (
            "executed_checks",
            json!([["WORKER_EXECUTION", ["cargo"], 0, SHA]]),
        ),
        ("executed_checks", json!([[]])),
        ("executed_checks", json!([null])),
    ] {
        let mut value = full();
        value[field] = bad;
        agree(value, false);
    }
    let mut positional_output_ref = full();
    positional_output_ref["executed_checks"][0]["output_ref"] = json!(["URL", "u"]);
    agree(positional_output_ref, false);
    let mut empty_command = full();
    empty_command["executed_checks"][0]["command"] = json!([]);
    agree(empty_command, false);
    let mut bad_result = full();
    bad_result["executed_checks"][0]["result"] = json!("OK");
    agree(bad_result, false);
    let mut missing = minimal();
    missing.as_object_mut().unwrap().remove("captured_at");
    agree(missing, false);
    cases.push(("[]".into(), false, false));
    // Exit codes are exact integer tokens.
    let with_exit = |token: &str| {
        full()
            .to_string()
            .replace("\"exit_code\":0", &format!("\"exit_code\":{token}"))
    };
    for token in [
        "1e2",
        "1.0",
        "-0",
        "9223372036854775808",
        "123456789012345678901234567890",
    ] {
        cases.push((with_exit(token), true, true));
    }
    for token in [
        "1.5",
        "1e-1",
        "\"1\"",
        "true",
        "null",
        "[0]",
        "1.0000000001",
    ] {
        cases.push((with_exit(token), false, false));
    }
    // Documented divergences.
    let mut newline = full();
    newline["base_sha"] = json!(format!("{SHA}\n"));
    cases.push((newline.to_string(), true, false));
    cases.push((
        format!(
            r#"{{"checkpoint_id":"a","checkpoint_id":"b","workspace_id":"w","head_sha":"{SHA}","captured_at":"2026-10-30T00:00:00Z","kind":"PROGRESS"}}"#
        ),
        true,
        false,
    ));
    cases.push((with_exit("1e400"), false, true));
    cases
}

#[test]
fn decoder_oracle_cases_match_the_expected_decoder_verdicts_without_python() {
    let cases = decoder_oracle_cases();
    assert!(cases.len() > 50, "{}", cases.len());
    for (text, _, decoder_accepts) in &cases {
        assert_eq!(
            WorkspaceCheckpointWire::from_json(text).is_ok(),
            *decoder_accepts,
            "{text}"
        );
    }
    // Accepted input re-encodes to an equal document.
    for (text, _, accepts) in cases.iter().filter(|(_, _, accepts)| *accepts) {
        let wire = WorkspaceCheckpointWire::from_json(text).unwrap();
        let again = WorkspaceCheckpointWire::from_json(&wire.to_json().unwrap()).unwrap();
        assert_eq!(wire, again, "{text} {accepts}");
    }
}

#[test]
#[ignore = "independent oracle: explicitly set WORKSPACE_SCHEMA_PYTHON to a Python with jsonschema and run with --ignored --nocapture"]
fn decoder_structural_schema_conformance_with_documented_oracle_limits() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let cases = decoder_oracle_cases();
    let input = json!({
        "schema": serde_json::from_str::<Value>(include_str!("../../schemas/WorkspaceCheckpoint.schema.json")).unwrap(),
        "texts": cases.iter().map(|(text, _, _)| text).collect::<Vec<_>>(),
    });
    // Structural validation only (no FormatChecker): `date-time` stays an annotation.
    let script = r#"
import sys, json, importlib.metadata
from jsonschema import Draft202012Validator
x=json.load(sys.stdin)
Draft202012Validator.check_schema(x['schema'])
v=Draft202012Validator(x['schema'])
print(json.dumps([v.is_valid(json.loads(t)) for t in x['texts']]))
print('jsonschema', importlib.metadata.version('jsonschema'), 'Draft202012 decoder cases', len(x['texts']), file=sys.stderr)
"#;
    let python = std::env::var_os("WORKSPACE_SCHEMA_PYTHON")
        .expect("independent oracle requires WORKSPACE_SCHEMA_PYTHON and installed jsonschema");
    let mut child = Command::new(python)
        .args(["-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("required independent Draft202012 validator available");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    println!("{}", String::from_utf8_lossy(&output.stderr));
    let verdicts: Vec<bool> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(verdicts.len(), cases.len());
    for ((text, python_valid, _), verdict) in cases.iter().zip(verdicts) {
        assert_eq!(verdict, *python_valid, "{text}");
    }
}
