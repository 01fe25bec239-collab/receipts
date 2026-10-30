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
