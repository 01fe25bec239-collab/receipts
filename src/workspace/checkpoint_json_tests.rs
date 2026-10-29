use crate::checkpoint_json::write_workspace_checkpoint_json;
use crate::*;
use serde_json::{Value, json};

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const BASE: &str = "fedcba9876543210fedcba9876543210fedcba98";
const TIME: &str = "2026-09-08T12:00:00Z";

fn time(value: &str) -> WorkspaceDateTimeV1 {
    WorkspaceDateTimeV1::try_new(value).unwrap()
}

fn sha(value: &str) -> CommitSha {
    CommitSha::parse(value).unwrap()
}

fn reference(
    ref_type: WorkspaceCheckpointRefType,
    target: &str,
    digest: Option<&str>,
    section: Option<&str>,
) -> WorkspaceCheckpointRef {
    WorkspaceCheckpointRef::new(
        ref_type,
        target,
        digest.map(str::to_owned),
        section.map(str::to_owned),
    )
    .unwrap()
}

fn check(exit_code: i64) -> WorkspaceCheckpointExecutedCheckCore {
    WorkspaceCheckpointExecutedCheckCore::new(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        vec!["cargo".into(), "test".into()],
        exit_code,
        sha(HEAD),
        None,
        None,
    )
    .unwrap()
}

fn minimal() -> WorkspaceCheckpointTemporalCaptureCore {
    let core = WorkspaceCheckpointCaptureCore::new(
        "c",
        "w",
        None,
        None,
        WorkspaceCheckpointKind::Progress,
        sha(HEAD),
        None,
        None,
        vec![],
        vec![],
        vec![],
        None,
        None,
    )
    .unwrap();
    WorkspaceCheckpointTemporalCaptureCore::new(core, time(TIME))
}

#[allow(clippy::too_many_arguments)]
fn with(
    kind: WorkspaceCheckpointKind,
    task_id: Option<&str>,
    attempt_id: Option<&str>,
    modified: Vec<String>,
    untracked: Vec<String>,
    checks: Vec<WorkspaceCheckpointExecutedCheckCore>,
    dirty: Option<WorkspaceCheckpointRef>,
    decision: Option<WorkspaceRecoveryDecision>,
    rationale: Option<&str>,
    crash: Option<WorkspaceCheckpointCrashClassification>,
    captured_at: &str,
) -> WorkspaceCheckpointTemporalCaptureCore {
    let core = WorkspaceCheckpointCaptureCore::new(
        "checkpoint-1",
        "workspace-1",
        task_id.map(str::to_owned),
        attempt_id.map(str::to_owned),
        kind,
        sha(HEAD),
        Some(sha(BASE)),
        dirty,
        modified,
        untracked,
        checks,
        decision,
        rationale.map(str::to_owned),
    )
    .unwrap()
    .with_crash_classification(crash);
    WorkspaceCheckpointTemporalCaptureCore::new(core, time(captured_at))
}

fn export(checkpoint: &WorkspaceCheckpointTemporalCaptureCore) -> (String, Value) {
    let before = checkpoint.clone();
    let text = workspace_checkpoint_to_json(checkpoint).unwrap();
    assert_eq!(*checkpoint, before, "export must not modify its input");
    assert_eq!(workspace_checkpoint_to_json(checkpoint).unwrap(), text);
    let value = serde_json::from_str(&text).unwrap();
    (text, value)
}

fn full_check() -> WorkspaceCheckpointExecutedCheckCore {
    WorkspaceCheckpointExecutedCheckCore::new(
        WorkspaceCheckpointCheckSource::BrokerExecution,
        vec!["git".into(), "diff".into(), "--".into(), String::new()],
        1,
        sha(BASE),
        Some(false),
        Some(reference(
            WorkspaceCheckpointRefType::Url,
            "https://example.invalid/out",
            Some(""),
            Some(""),
        )),
    )
    .unwrap()
    .with_result(Some(WorkspaceCheckpointExecutedCheckResult::Fail))
    .with_started_at(Some(time("2026-09-08T11:00:00Z")))
    .with_finished_at(Some(time("2026-09-08T11:00:01.5-00:00")))
}

fn full() -> WorkspaceCheckpointTemporalCaptureCore {
    with(
        WorkspaceCheckpointKind::RecoveryCapture,
        Some("task-1"),
        Some("attempt-1"),
        vec!["src/a.rs".into(), "src/b.rs".into()],
        vec!["new.txt".into()],
        vec![full_check(), check(0)],
        Some(reference(
            WorkspaceCheckpointRefType::RepoPath,
            "diff.patch",
            Some("sha256:abc"),
            Some("body"),
        )),
        Some(WorkspaceRecoveryDecision::InspectAndSalvage),
        Some("keep the half-done refactor"),
        Some(WorkspaceCheckpointCrashClassification::RuntimeCrash),
        "2026-09-08T12:00:00.100+05:30",
    )
}

#[test]
fn minimal_checkpoint_exports_required_fields_empty_arrays_and_no_nulls() {
    let (text, value) = export(&minimal());
    assert_eq!(
        text,
        format!(
            r#"{{"checkpoint_id":"c","workspace_id":"w","kind":"PROGRESS","head_sha":"{HEAD}","modified_files":[],"untracked_files":[],"executed_checks":[],"captured_at":"{TIME}"}}"#
        )
    );
    assert!(!text.contains("null"));
    assert_eq!(value.as_object().unwrap().len(), 8);
}

#[test]
fn full_checkpoint_exports_every_captured_field() {
    let (_, value) = export(&full());
    assert_eq!(
        value,
        json!({
            "checkpoint_id": "checkpoint-1",
            "workspace_id": "workspace-1",
            "task_id": "task-1",
            "attempt_id": "attempt-1",
            "kind": "RECOVERY_CAPTURE",
            "head_sha": HEAD,
            "base_sha": BASE,
            "dirty_diff_ref": {
                "ref_type": "REPO_PATH", "target": "diff.patch",
                "digest": "sha256:abc", "section": "body"
            },
            "modified_files": ["src/a.rs", "src/b.rs"],
            "untracked_files": ["new.txt"],
            "executed_checks": [
                {
                    "source": "BROKER_EXECUTION",
                    "command": ["git", "diff", "--", ""],
                    "exit_code": 1,
                    "code_sha": BASE,
                    "timed_out": false,
                    "started_at": "2026-09-08T11:00:00Z",
                    "finished_at": "2026-09-08T11:00:01.5-00:00",
                    "output_ref": {
                        "ref_type": "URL", "target": "https://example.invalid/out",
                        "digest": "", "section": ""
                    },
                    "result": "FAIL"
                },
                {
                    "source": "WORKER_EXECUTION",
                    "command": ["cargo", "test"],
                    "exit_code": 0,
                    "code_sha": HEAD
                }
            ],
            "crash_classification": "RUNTIME_CRASH",
            "recovery_decision": "INSPECT_AND_SALVAGE",
            "recovery_rationale": "keep the half-done refactor",
            "captured_at": "2026-09-08T12:00:00.100+05:30"
        })
    );
}

#[test]
fn every_vocabulary_variant_exports_its_schema_spelling() {
    for kind in WorkspaceCheckpointKind::ALL {
        let c = with(
            kind,
            None,
            None,
            vec![],
            vec![],
            vec![],
            None,
            None,
            None,
            None,
            TIME,
        );
        assert_eq!(export(&c).1["kind"], kind.as_str());
    }
    for decision in WorkspaceRecoveryDecision::ALL {
        let c = with(
            WorkspaceCheckpointKind::Progress,
            None,
            None,
            vec![],
            vec![],
            vec![],
            None,
            Some(decision),
            None,
            None,
            TIME,
        );
        assert_eq!(export(&c).1["recovery_decision"], decision.as_str());
    }
    for crash in WorkspaceCheckpointCrashClassification::ALL {
        let c = with(
            WorkspaceCheckpointKind::Progress,
            None,
            None,
            vec![],
            vec![],
            vec![],
            None,
            None,
            None,
            Some(crash),
            TIME,
        );
        assert_eq!(export(&c).1["crash_classification"], crash.as_str());
    }
    for ref_type in WorkspaceCheckpointRefType::ALL {
        let r = || reference(ref_type, "t", Some("d"), Some("s"));
        let checks = vec![
            WorkspaceCheckpointExecutedCheckCore::new(
                WorkspaceCheckpointCheckSource::GitProvenance,
                vec!["x".into()],
                0,
                sha(HEAD),
                None,
                Some(r()),
            )
            .unwrap(),
        ];
        let c = with(
            WorkspaceCheckpointKind::Progress,
            None,
            None,
            vec![],
            vec![],
            checks,
            Some(r()),
            None,
            None,
            None,
            TIME,
        );
        let v = export(&c).1;
        assert_eq!(v["dirty_diff_ref"]["ref_type"], ref_type.as_str());
        assert_eq!(
            v["executed_checks"][0]["output_ref"]["ref_type"],
            ref_type.as_str()
        );
    }
    for source in WorkspaceCheckpointCheckSource::ALL {
        for result in WorkspaceCheckpointExecutedCheckResult::ALL {
            let one = WorkspaceCheckpointExecutedCheckCore::new(
                source,
                vec!["x".into()],
                0,
                sha(HEAD),
                Some(true),
                None,
            )
            .unwrap()
            .with_result(Some(result));
            let c = with(
                WorkspaceCheckpointKind::Progress,
                None,
                None,
                vec![],
                vec![],
                vec![one],
                None,
                None,
                None,
                None,
                TIME,
            );
            let v = export(&c).1;
            assert_eq!(v["executed_checks"][0]["source"], source.as_str());
            assert_eq!(v["executed_checks"][0]["result"], result.as_str());
            assert_eq!(v["executed_checks"][0]["timed_out"], true);
        }
    }
}

#[test]
fn absent_optionals_are_omitted_and_present_empty_values_are_kept() {
    let absent = with(
        WorkspaceCheckpointKind::Progress,
        None,
        None,
        vec![],
        vec![],
        vec![check(0)],
        None,
        None,
        None,
        None,
        TIME,
    );
    let mut value = export(&absent).1;
    let object = value.as_object_mut().unwrap();
    for key in [
        "task_id",
        "attempt_id",
        "dirty_diff_ref",
        "crash_classification",
        "recovery_decision",
        "recovery_rationale",
    ] {
        assert!(!object.contains_key(key), "{key}");
    }
    assert_eq!(value["base_sha"], BASE);
    let check = value["executed_checks"][0].as_object().unwrap();
    assert_eq!(check.len(), 4);

    let present = with(
        WorkspaceCheckpointKind::Progress,
        None,
        None,
        vec![],
        vec![],
        vec![],
        Some(reference(
            WorkspaceCheckpointRefType::ArtifactId,
            "a",
            Some(""),
            Some(""),
        )),
        None,
        Some(""),
        None,
        TIME,
    );
    let v = export(&present).1;
    assert_eq!(v["recovery_rationale"], "");
    assert_eq!(v["dirty_diff_ref"]["digest"], "");
    assert_eq!(v["dirty_diff_ref"]["section"], "");

    let bare = reference(WorkspaceCheckpointRefType::StateQuery, "q", None, None);
    let c = with(
        WorkspaceCheckpointKind::Progress,
        None,
        None,
        vec![],
        vec![],
        vec![],
        Some(bare),
        None,
        None,
        None,
        TIME,
    );
    assert_eq!(
        export(&c).1["dirty_diff_ref"],
        json!({"ref_type": "STATE_QUERY", "target": "q"})
    );

    // Recovery is exported, never decided: a classification alone yields no decision.
    let classified = with(
        WorkspaceCheckpointKind::PreTermination,
        None,
        None,
        vec![],
        vec![],
        vec![],
        None,
        None,
        None,
        Some(WorkspaceCheckpointCrashClassification::Unknown),
        TIME,
    );
    let v = export(&classified).1;
    assert_eq!(v["crash_classification"], "UNKNOWN");
    assert!(v.get("recovery_decision").is_none());
    assert!(v.get("recovery_rationale").is_none());
}

#[test]
fn optional_check_fields_are_independent() {
    let timed_out = |value| {
        WorkspaceCheckpointExecutedCheckCore::new(
            WorkspaceCheckpointCheckSource::ReviewExecution,
            vec!["x".into()],
            2,
            sha(HEAD),
            value,
            None,
        )
        .unwrap()
    };
    for (check, keys) in [
        (timed_out(Some(false)), vec!["timed_out"]),
        (
            timed_out(None).with_started_at(Some(time(TIME))),
            vec!["started_at"],
        ),
        (
            timed_out(None).with_finished_at(Some(time(TIME))),
            vec!["finished_at"],
        ),
        (
            timed_out(None).with_result(Some(WorkspaceCheckpointExecutedCheckResult::Unknown)),
            vec!["result"],
        ),
    ] {
        let c = with(
            WorkspaceCheckpointKind::Progress,
            None,
            None,
            vec![],
            vec![],
            vec![check],
            None,
            None,
            None,
            None,
            TIME,
        );
        let v = export(&c).1;
        let object = v["executed_checks"][0].as_object().unwrap();
        assert_eq!(object.len(), 4 + keys.len(), "{object:?}");
        for key in keys {
            assert!(object.contains_key(key));
        }
    }
}

#[test]
fn exit_codes_are_exact_i64_integers() {
    for exit_code in [
        i64::MIN,
        i64::MIN + 1,
        -1,
        0,
        1,
        (1 << 53) + 1,
        i64::MAX - 1,
        i64::MAX,
    ] {
        let c = with(
            WorkspaceCheckpointKind::Progress,
            None,
            None,
            vec![],
            vec![],
            vec![check(exit_code)],
            None,
            None,
            None,
            None,
            TIME,
        );
        let (text, value) = export(&c);
        assert!(
            text.contains(&format!(r#""exit_code":{exit_code},"#)),
            "{text}"
        );
        let number = &value["executed_checks"][0]["exit_code"];
        assert_eq!(number.as_i64(), Some(exit_code));
        assert!(!number.is_f64());
    }
}

#[test]
fn strings_argv_and_paths_are_preserved_without_normalization() {
    let nasty = [
        "",
        " ",
        "  lead and trail  ",
        "a\nb\r\nc\td",
        "nul\0byte",
        "ctl\u{1}\u{1f}\u{7f}\u{85}\u{2028}\u{2029}",
        "quote\"back\\slash/",
        "e\u{301}",
        "é",
        "🦀日本語",
        "\u{feff}bom",
        "--option-shaped",
        "$(not a shell)",
    ];
    let strings: Vec<String> = nasty.iter().map(|s| (*s).to_owned()).collect();
    let one = WorkspaceCheckpointExecutedCheckCore::new(
        WorkspaceCheckpointCheckSource::WorkerExecution,
        strings.clone(),
        0,
        sha(HEAD),
        None,
        Some(reference(
            WorkspaceCheckpointRefType::RepoPath,
            "  target\n",
            Some("  d\u{0} "),
            Some("\n"),
        )),
    )
    .unwrap();
    let c = with(
        WorkspaceCheckpointKind::Progress,
        Some(" task\n"),
        Some("🦀"),
        strings.clone(),
        strings.clone(),
        vec![one],
        None,
        None,
        Some("why\n\u{0}🦀  "),
        None,
        TIME,
    );
    let v = export(&c).1;
    let expected = json!(strings);
    assert_eq!(v["modified_files"], expected);
    assert_eq!(v["untracked_files"], expected);
    assert_eq!(v["executed_checks"][0]["command"], expected);
    assert_eq!(v["task_id"], " task\n");
    assert_eq!(v["attempt_id"], "🦀");
    assert_eq!(v["recovery_rationale"], "why\n\u{0}🦀  ");
    let output = &v["executed_checks"][0]["output_ref"];
    assert_eq!(output["target"], "  target\n");
    assert_eq!(output["digest"], "  d\u{0} ");
    assert_eq!(output["section"], "\n");
}

#[test]
fn identifier_limits_are_the_constructors_unicode_character_limits() {
    let id = "🦀".repeat(200);
    assert_eq!(id.len(), 800);
    let core = WorkspaceCheckpointCaptureCore::new(
        id.clone(),
        id.clone(),
        Some(id.clone()),
        Some(id.clone()),
        WorkspaceCheckpointKind::Progress,
        sha(HEAD),
        None,
        None,
        vec![],
        vec![],
        vec![],
        None,
        None,
    )
    .unwrap();
    let v = export(&WorkspaceCheckpointTemporalCaptureCore::new(
        core,
        time(TIME),
    ))
    .1;
    for key in ["checkpoint_id", "workspace_id", "task_id", "attempt_id"] {
        assert_eq!(v[key], id.as_str());
    }
    // Export neither widens nor re-checks the constructor domain.
    assert!(
        WorkspaceCheckpointCaptureCore::new(
            "🦀".repeat(201),
            "w",
            None,
            None,
            WorkspaceCheckpointKind::Progress,
            sha(HEAD),
            None,
            None,
            vec![],
            vec![],
            vec![],
            None,
            None,
        )
        .is_err()
    );
}

#[test]
fn timestamps_keep_their_exact_lexical_spelling() {
    let long = format!("2026-09-08t00:00:00.{}z", "7".repeat(10_000));
    let spellings = [
        "0000-01-01T00:00:00Z",
        "0000-02-29T00:00:00Z",
        "1990-12-31T23:59:60Z",
        "2026-09-08T12:00:60Z",
        "9999-12-31t23:59:60.0z",
        "2000-02-29t00:00:00.100-00:00",
        "2026-09-08T00:00:00+00:00",
        "2026-09-08T00:00:00-23:59",
        "2026-09-08T00:00:00.1Z",
        "2026-09-08T00:00:00.100Z",
        long.as_str(),
    ];
    for spelling in spellings {
        let one = check(0)
            .with_started_at(Some(time(spelling)))
            .with_finished_at(Some(time(spelling)));
        let c = with(
            WorkspaceCheckpointKind::Progress,
            None,
            None,
            vec![],
            vec![],
            vec![one],
            None,
            None,
            None,
            None,
            spelling,
        );
        let (text, v) = export(&c);
        assert_eq!(v["captured_at"], spelling);
        assert_eq!(v["executed_checks"][0]["started_at"], spelling);
        assert_eq!(v["executed_checks"][0]["finished_at"], spelling);
        assert!(text.contains(&format!(r#""captured_at":"{spelling}""#)));
    }
}

#[test]
fn serialization_failure_is_a_fixed_message_that_does_not_echo_captured_data() {
    struct Failing;
    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("SECRET-PATH-/home/user/private"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let c = with(
        WorkspaceCheckpointKind::Progress,
        None,
        None,
        vec!["SECRET-PATH-/home/user/private".into()],
        vec![],
        vec![],
        None,
        None,
        None,
        None,
        TIME,
    );
    let error = write_workspace_checkpoint_json(&c, Failing).unwrap_err();
    assert_eq!(error, WorkspaceCheckpointJsonExportError);
    for text in [format!("{error}"), format!("{error:?}")] {
        assert!(!text.contains("SECRET"), "{text}");
    }
    assert_eq!(error.to_string(), "workspace checkpoint JSON export failed");
    fn assert_error<T: std::error::Error>() {}
    assert_error::<WorkspaceCheckpointJsonExportError>();
}

fn oracle_samples() -> Vec<Value> {
    let mut samples = vec![];
    let mut variants = vec![minimal(), full()];
    for exit_code in [i64::MIN, i64::MAX] {
        variants.push(with(
            WorkspaceCheckpointKind::PreTermination,
            None,
            None,
            vec!["m\n🦀".into()],
            vec![],
            vec![check(exit_code)],
            None,
            Some(WorkspaceRecoveryDecision::ResetToLastAccepted),
            Some(""),
            Some(WorkspaceCheckpointCrashClassification::RateLimited),
            "0000-01-01T00:00:00Z",
        ));
    }
    variants.push(with(
        WorkspaceCheckpointKind::Progress,
        None,
        None,
        vec![],
        vec![],
        vec![check(0).with_started_at(Some(time("1990-12-31T23:59:60Z")))],
        None,
        None,
        None,
        None,
        "2026-09-08T12:00:60Z",
    ));
    // Every closed-vocabulary spelling, each checked by the schema's own enums.
    let mut vocab = |kind, decision, crash, source, result, ref_type| {
        let one = WorkspaceCheckpointExecutedCheckCore::new(
            source,
            vec!["x".into()],
            0,
            sha(HEAD),
            None,
            Some(reference(ref_type, "t", None, None)),
        )
        .unwrap()
        .with_result(Some(result));
        variants.push(with(
            kind,
            None,
            None,
            vec![],
            vec![],
            vec![one],
            Some(reference(ref_type, "t", None, None)),
            decision,
            None,
            crash,
            TIME,
        ));
    };
    let (source, result, ref_type) = (
        WorkspaceCheckpointCheckSource::ALL[0],
        WorkspaceCheckpointExecutedCheckResult::ALL[0],
        WorkspaceCheckpointRefType::ALL[0],
    );
    for kind in WorkspaceCheckpointKind::ALL {
        vocab(kind, None, None, source, result, ref_type);
    }
    for decision in WorkspaceRecoveryDecision::ALL {
        let kind = WorkspaceCheckpointKind::Progress;
        vocab(kind, Some(decision), None, source, result, ref_type);
    }
    for crash in WorkspaceCheckpointCrashClassification::ALL {
        let kind = WorkspaceCheckpointKind::Progress;
        vocab(kind, None, Some(crash), source, result, ref_type);
    }
    for source in WorkspaceCheckpointCheckSource::ALL {
        let kind = WorkspaceCheckpointKind::Progress;
        vocab(kind, None, None, source, result, ref_type);
    }
    for result in WorkspaceCheckpointExecutedCheckResult::ALL {
        let kind = WorkspaceCheckpointKind::Progress;
        vocab(kind, None, None, source, result, ref_type);
    }
    for ref_type in WorkspaceCheckpointRefType::ALL {
        let kind = WorkspaceCheckpointKind::Progress;
        vocab(kind, None, None, source, result, ref_type);
    }
    for c in &variants {
        samples.push(json!([export(c).1, true]));
    }
    // Not vacuous: the oracle must refuse these.
    let mut null_optional = export(&minimal()).1;
    null_optional["task_id"] = Value::Null;
    let mut extra = export(&minimal()).1;
    extra["unknown"] = json!(1);
    let mut float_exit = export(&full()).1;
    float_exit["executed_checks"][0]["exit_code"] = json!(1.5);
    let mut missing = export(&minimal()).1;
    missing.as_object_mut().unwrap().remove("captured_at");
    samples.extend([null_optional, extra, float_exit, missing].map(|v| json!([v, false])));
    samples
}

#[test]
fn oracle_samples_are_well_formed_without_python() {
    // Ordinary tests never spawn an interpreter; they only build the samples.
    // minimal, full and 3 edge cases; (3+3+12+4+5+4) vocabulary; 4 must-refuse.
    assert_eq!(oracle_samples().len(), 5 + 31 + 4);
}

#[test]
#[ignore = "independent oracle: explicitly set WORKSPACE_SCHEMA_PYTHON to a Python with jsonschema and run with --ignored --nocapture"]
fn checkpoint_export_structural_schema_conformance() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let samples = oracle_samples();
    println!(
        "SCHEMA_SAMPLES={}",
        serde_json::to_string(&samples).unwrap()
    );
    let input = json!({
        "schema": serde_json::from_str::<Value>(include_str!("../../schemas/WorkspaceCheckpoint.schema.json")).unwrap(),
        "samples": samples,
    });
    // Structural validation only: no FormatChecker, so `date-time` stays an annotation.
    let script = r#"
import sys, json, importlib.metadata
from jsonschema import Draft202012Validator
x=json.load(sys.stdin)
Draft202012Validator.check_schema(x['schema'])
v=Draft202012Validator(x['schema'])
for i, (value, expected) in enumerate(x['samples']):
    assert v.is_valid(value) == expected, (i, value, list(v.iter_errors(value)))
print('jsonschema', importlib.metadata.version('jsonschema'), 'Draft202012 structural cases', len(x['samples']), 'PASS')
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
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
