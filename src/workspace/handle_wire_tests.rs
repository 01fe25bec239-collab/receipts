use crate::test_support::{TestRepo, branch_exists, worktree_registered};
use crate::*;
use serde_json::{Value, json};

fn minimal() -> Value {
    json!({"workspace_id":"w", "branch":"b", "worktree_path":"p",
        "base_sha":"0123456789abcdef0123456789abcdef01234567", "state":"PROVISIONED"})
}

fn roundtrip(value: &Value) -> WorkspaceHandleWire {
    let wire = WorkspaceHandleWire::from_json(&value.to_string()).unwrap();
    let encoded = wire.to_json().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&encoded).unwrap(), *value);
    assert_eq!(WorkspaceHandleWire::from_json(&encoded).unwrap(), wire);
    wire
}

// Third member is structural-schema acceptance, excluding the disposed format
// and Python final-LF regex divergences from Rust admission parity claims.
fn cases() -> Vec<(Value, bool, bool)> {
    let mut cases = vec![(minimal(), true, true)];
    for state in [
        "PROVISIONED",
        "ACTIVE",
        "CHECKPOINTED",
        "ORPHANED",
        "RECOVERED",
        "TORN_DOWN",
    ] {
        for policy in ["LOCAL_ONLY", "PUSH_ON_ACCEPT", "PUSH_ALWAYS"] {
            let mut v = minimal();
            v["state"] = json!(state);
            v["remote_publish_policy"] = json!(policy);
            v["task_id"] = json!("task\n e\u{301}🦀");
            v["head_sha"] = json!("f".repeat(40));
            v["created_at"] = json!("2026-09-08t00:00:00.100-00:00");
            cases.push((v, true, true));
        }
    }
    for isolation in [
        json!({}),
        json!({"kind":"WORKSPACE_ISOLATION"}),
        json!({"sandbox_mode":null}),
        json!({"network_access":null}),
        json!({"sandbox_mode":""}),
        json!({"network_access":false}),
        json!({"kind":"WORKSPACE_ISOLATION","sandbox_mode":"lexical🦀\n","network_access":true}),
        json!({"sandbox_mode":null,"network_access":null}),
    ] {
        let mut v = minimal();
        v["isolation"] = isolation;
        cases.push((v, true, true));
    }
    for id in [
        " ".into(),
        "\0\n".into(),
        "🦀".repeat(200),
        "e\u{301}".repeat(100),
    ] {
        let mut v = minimal();
        v["workspace_id"] = json!(id);
        v["task_id"] = v["workspace_id"].clone();
        cases.push((v, true, true));
    }
    let mut lexical = minimal();
    lexical["branch"] = json!(" --not/a/git ref\n");
    lexical["worktree_path"] = json!("relative/../🦀\0");
    cases.push((lexical, true, true));
    for field in [
        "workspace_id",
        "branch",
        "worktree_path",
        "base_sha",
        "state",
    ] {
        let mut v = minimal();
        v.as_object_mut().unwrap().remove(field);
        cases.push((v, false, false));
    }
    for field in [
        "workspace_id",
        "task_id",
        "branch",
        "worktree_path",
        "base_sha",
        "head_sha",
        "state",
        "isolation",
        "remote_publish_policy",
        "created_at",
    ] {
        for value in [Value::Null, json!(42), json!(true), json!([]), json!({})] {
            if field == "isolation" && value == json!({}) {
                continue;
            }
            let mut v = minimal();
            v[field] = value;
            cases.push((v, false, false));
        }
    }
    for field in ["workspace_id", "task_id", "branch", "worktree_path"] {
        let mut v = minimal();
        v[field] = json!("");
        cases.push((v, false, false));
    }
    for field in ["workspace_id", "task_id"] {
        for id in ["x".repeat(201), "🦀".repeat(201), "e\u{301}".repeat(101)] {
            let mut v = minimal();
            v[field] = json!(id);
            cases.push((v, false, false));
        }
    }
    for (field, values) in [
        (
            "state",
            vec![
                json!("PROVISIONING"),
                json!("FINALIZING"),
                json!("RECOVERING"),
                json!("active"),
                json!(""),
            ],
        ),
        (
            "remote_publish_policy",
            vec![json!("local_only"), json!("")],
        ),
        ("extra", vec![Value::Null]),
    ] {
        for value in values {
            let mut v = minimal();
            v[field] = value;
            cases.push((v, false, false));
        }
    }
    for isolation in [
        json!({"kind":null}),
        json!({"kind":"SANDBOX"}),
        json!({"kind":true}),
        json!({"sandbox_mode":false}),
        json!({"network_access":"true"}),
        json!({"sandbox_mode":{}}),
        json!({"network_access":0}),
        json!({"unknown":null}),
        json!([]),
        json!(["WORKSPACE_ISOLATION"]),
    ] {
        let mut v = minimal();
        v["isolation"] = isolation;
        cases.push((v, false, false));
    }
    for field in ["base_sha", "head_sha"] {
        for sha in [
            "a".repeat(39),
            "a".repeat(41),
            "A".repeat(40),
            "g".repeat(40),
            "é".repeat(20),
            format!("{}\r", "a".repeat(40)),
            format!("{}\n", "a".repeat(40)),
        ] {
            let python_accepts = sha.ends_with('\n');
            let mut v = minimal();
            v[field] = json!(sha);
            cases.push((v, false, python_accepts));
        }
    }
    for time in [
        "",
        "2026-02-29T00:00:00Z",
        "1900-02-29T00:00:00Z",
        "2026-09-08T24:00:00Z",
        "2026-09-08T00:00:61Z",
        "2026-09-08T00:00:00.Z",
        "2026-09-08T00:00:00+24:00",
        "2026-09-08T00:00:00+00:60",
        "2026-09-08T00:00:00",
        "2026-09-08T00:00:00Z\n",
        "２０２６-09-08T00:00:00Z",
    ] {
        let mut v = minimal();
        v["created_at"] = json!(time);
        cases.push((v, false, true));
    }
    cases
}

#[test]
fn handle_wire_roundtrips_and_refuses_field_errors() {
    for isolation in [r#"{"kind":"SANDBOX"}"#, "[]", r#"["WORKSPACE_ISOLATION"]"#] {
        assert!(serde_json::from_str::<WorkspaceHandleWireIsolation>(isolation).is_err());
    }
    for (value, valid, _) in cases() {
        if valid {
            roundtrip(&value);
        } else {
            assert!(
                WorkspaceHandleWire::from_json(&value.to_string()).is_err(),
                "{value}"
            );
        }
    }
    let mut v = minimal();
    v["isolation"] = json!({"sandbox_mode":null,"network_access":false});
    let wire = roundtrip(&v);
    assert_eq!(wire.isolation().unwrap().kind(), None);
    assert_eq!(wire.isolation().unwrap().sandbox_mode(), Some(None));
    assert_eq!(
        wire.isolation().unwrap().network_access(),
        Some(Some(false))
    );
    assert_eq!(roundtrip(&minimal()).isolation(), None);
    v["isolation"] = json!({});
    let wire = roundtrip(&v);
    assert_eq!(wire.isolation().unwrap().sandbox_mode(), None);
    assert_eq!(wire.isolation().unwrap().network_access(), None);
}

#[test]
fn handle_wire_refuses_duplicate_fields_and_malformed_json() {
    for text in [
        "",
        "null",
        "[]",
        "{}",
        "true",
        r#"["w","b","p","aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","PROVISIONED",null,null,null,null,null]"#,
        "{",
        "{\"workspace_id\":\"\\ud800\"}",
        "{\"workspace_id\":NaN}",
    ] {
        assert!(WorkspaceHandleWire::from_json(text).is_err(), "{text}");
    }
    let mut complete = minimal();
    for (key, value) in [
        ("task_id", json!("t")),
        ("head_sha", json!("b".repeat(40))),
        ("isolation", json!({})),
        ("remote_publish_policy", json!("LOCAL_ONLY")),
        ("created_at", json!("2026-01-01T00:00:00Z")),
    ] {
        complete[key] = value;
    }
    for (key, value) in complete.as_object().unwrap() {
        let text = complete.to_string();
        let duplicate = format!("{},{}:{value}}}", &text[..text.len() - 1], json!(key));
        assert!(
            WorkspaceHandleWire::from_json(&duplicate).is_err(),
            "{duplicate}"
        );
    }
    for pair in [
        "\"kind\":\"WORKSPACE_ISOLATION\",\"kind\":\"WORKSPACE_ISOLATION\"",
        "\"sandbox_mode\":null,\"sandbox_mode\":null",
        "\"network_access\":null,\"network_access\":false",
    ] {
        let mut text = minimal().to_string();
        text.pop();
        text.push_str(&format!(",\"isolation\":{{{pair}}}}}"));
        assert!(WorkspaceHandleWire::from_json(&text).is_err(), "{text}");
    }
    for suffix in ["{}", "x", ",", "\0"] {
        assert!(WorkspaceHandleWire::from_json(&(minimal().to_string() + suffix)).is_err());
    }
    let text = minimal().to_string();
    for end in 0..text.len() {
        assert!(WorkspaceHandleWire::from_json(&text[..end]).is_err());
    }
    roundtrip(&serde_json::from_str::<Value>(&(text + " \n\t")).unwrap());
}

#[test]
fn handle_wire_sha_exact_ascii_membership_and_length_for_both_fields() {
    for field in ["base_sha", "head_sha"] {
        for length in 0..=81 {
            let mut v = minimal();
            v[field] = json!("a".repeat(length));
            assert_eq!(
                WorkspaceHandleWire::from_json(&v.to_string()).is_ok(),
                length == 40
            );
        }
        for index in 0..40 {
            for byte in 0..=127u8 {
                let mut bytes = vec![b'a'; 40];
                bytes[index] = byte;
                let mut v = minimal();
                v[field] = json!(String::from_utf8(bytes).unwrap());
                assert_eq!(
                    WorkspaceHandleWire::from_json(&v.to_string()).is_ok(),
                    matches!(byte, b'0'..=b'9' | b'a'..=b'f'),
                    "{field} {index} {byte}"
                );
            }
        }
        for invalid in [
            format!("{}\n", "a".repeat(40)),
            format!("{}\r", "a".repeat(40)),
            format!(" {}", "a".repeat(40)),
            "ａ".repeat(40),
            "é".repeat(20),
        ] {
            let mut v = minimal();
            v[field] = json!(invalid);
            assert!(WorkspaceHandleWire::from_json(&v.to_string()).is_err());
        }
    }
}

#[test]
fn handle_wire_v1_timestamps_preserve_lexical_grammar_and_calendar() {
    for time in [
        "0000-02-29T00:00:00Z",
        "0000-01-01T00:00:00Z",
        "1990-12-31T23:59:60Z",
        "2026-09-08T12:00:60Z",
        "9999-12-31t23:59:60.0z",
        "2000-02-29t00:00:00.100-00:00",
        "2026-09-08T00:00:00+23:59",
        "2026-09-08T00:00:00-23:59",
    ] {
        let mut v = minimal();
        v["created_at"] = json!(time);
        assert_eq!(roundtrip(&v).created_at(), Some(time));
    }
    let time = format!("2026-09-08t00:00:00.{}z", "7".repeat(10_000));
    let mut v = minimal();
    v["created_at"] = json!(time);
    assert_eq!(roundtrip(&v).created_at(), Some(time.as_str()));
    for (year, leap) in [(0, true), (1900, false), (2000, true), (2100, false)] {
        for day in 28..=30 {
            let time = format!("{year:04}-02-{day:02}T00:00:00Z");
            let mut v = minimal();
            v["created_at"] = json!(time);
            assert_eq!(
                WorkspaceHandleWire::from_json(&v.to_string()).is_ok(),
                day <= if leap { 29 } else { 28 }
            );
        }
    }
}

#[test]
#[ignore = "independent oracle: explicitly set WORKSPACE_SCHEMA_PYTHON to a Python with jsonschema and run with --ignored --nocapture"]
fn handle_wire_structural_schema_conformance_with_documented_oracle_limits() {
    let mut samples: Vec<_> = cases()
        .into_iter()
        .map(|(value, rust_valid, schema_valid)| {
            let value = if rust_valid {
                serde_json::from_str(&roundtrip(&value).to_json().unwrap()).unwrap()
            } else {
                value
            };
            json!([value, schema_valid])
        })
        .collect();
    for time in [
        "0000-01-01T00:00:00Z",
        "1990-12-31T23:59:60Z",
        "2026-09-08T12:00:60Z",
    ] {
        let mut v = minimal();
        v["created_at"] = json!(time);
        roundtrip(&v);
        samples.push(json!([v, true]));
    }
    samples.extend(actual_export_samples());
    structural_schema(samples);
}

fn structural_schema(samples: Vec<Value>) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    println!(
        "SCHEMA_SAMPLES={}",
        serde_json::to_string(&samples).unwrap()
    );
    let input = json!({"schema": serde_json::from_str::<Value>(include_str!("../../schemas/WorkspaceHandle.schema.json")).unwrap(), "samples": samples});
    let script = r#"
import sys, json, importlib.metadata
from jsonschema import Draft202012Validator, FormatChecker
x=json.load(sys.stdin)
Draft202012Validator.check_schema(x['schema'])
v=Draft202012Validator(x['schema']) # A1 disposition: structural validation only
for i, (value, expected) in enumerate(x['samples']):
    assert v.is_valid(value) == expected, (i, value, list(v.iter_errors(value)))
print('jsonschema', importlib.metadata.version('jsonschema'), 'Draft202012 structural cases', len(x['samples']), 'PASS')
f=Draft202012Validator(x['schema'], format_checker=FormatChecker())
for time in ['0000-01-01T00:00:00Z', '1990-12-31T23:59:60Z', '2026-09-08T12:00:60Z']:
    value=dict(x['samples'][0][0], created_at=time)
    print('FormatChecker compatibility', time, f.is_valid(value))
print('SHA final-LF dialect divergence retained; Rust exact ASCII tests are decisive')
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

fn provision(repo: &TestRepo, workspace: &str, task: Option<&str>, name: &str) -> WorkspaceHandle {
    WorkspaceProvisionRequest::new(
        repo.path(),
        workspace,
        task,
        name,
        repo.root.path().join(name),
        &repo.head_sha(),
    )
    .unwrap()
    .provision()
    .unwrap()
}

fn assert_export(handle: &WorkspaceHandle) -> WorkspaceHandleWire {
    let wire = WorkspaceHandleWire::try_from(handle).unwrap();
    assert_eq!(wire.workspace_id(), handle.workspace_id());
    assert_eq!(wire.task_id(), handle.task_id());
    assert_eq!(wire.branch(), handle.branch());
    assert_eq!(
        wire.worktree_path(),
        handle.worktree_path().to_str().unwrap()
    );
    assert_eq!(wire.base_sha(), handle.base_sha().as_str());
    assert_eq!(wire.head_sha(), handle.head_sha().map(CommitSha::as_str));
    assert_eq!(wire.state(), handle.state().as_str());
    assert_eq!(
        wire.isolation().unwrap().kind(),
        Some(handle.isolation().as_str())
    );
    assert_eq!(wire.isolation().unwrap().sandbox_mode(), None);
    assert_eq!(wire.isolation().unwrap().network_access(), None);
    assert_eq!(
        wire.remote_publish_policy(),
        handle
            .remote_publish_policy()
            .map(WorkspaceRemotePublishPolicy::as_str)
    );
    assert_eq!(
        wire.created_at(),
        handle.created_at().map(WorkspaceDateTimeV1::as_str)
    );
    assert_eq!(
        WorkspaceHandleWire::from_json(&wire.to_json().unwrap()).unwrap(),
        wire
    );
    wire
}

#[test]
fn handle_wire_exports_real_provisioned_and_verified_torn_down_evidence() {
    assert_eq!(actual_export_samples().len(), 10);
}

fn actual_export_samples() -> Vec<Value> {
    let repo = TestRepo::new_nested("wire-export", "repo");
    let mut samples = Vec::new();
    for (index, time) in [
        None,
        Some("0000-01-01T00:00:00Z"),
        Some("1990-12-31T23:59:60Z"),
        Some("2026-09-08T12:00:60Z"),
        Some("2026-09-08t00:00:00.100-00:00"),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("task-{index}");
        let mut request = WorkspaceProvisionRequest::new(
            repo.path(),
            "w-é",
            if index == 0 { None } else { Some("t🦀") },
            &name,
            repo.root.path().join(&name),
            &repo.head_sha(),
        )
        .unwrap();
        if let Some(time) = time {
            request = request
                .with_created_at(WorkspaceDateTimeV1::try_new(time).unwrap())
                .with_remote_publish_policy(WorkspaceRemotePublishPolicy::ALL[index % 3]);
        }
        let handle = request.provision().unwrap();
        let before = handle.clone();
        let provisioned = assert_export(&handle);
        assert_eq!(handle, before);
        assert!(worktree_registered(&repo, handle.worktree_path()));
        let torn = WorkspaceTeardownRequest::new(repo.path())
            .teardown(&handle)
            .unwrap();
        let torn_wire = assert_export(&torn);
        for wire in [&provisioned, &torn_wire] {
            samples.push(json!([
                serde_json::from_str::<Value>(&wire.to_json().unwrap()).unwrap(),
                true
            ]));
        }
        assert_eq!(provisioned.state(), "PROVISIONED");
        assert_eq!(torn.state(), WorkspaceState::TornDown);
        assert!(!torn.worktree_path().exists());
        assert!(branch_exists(&repo, torn.branch()));
    }
    samples
}

#[test]
fn handle_wire_export_domain_refusals_do_not_mutate_operational_handles() {
    let repo = TestRepo::new_nested("wire-domain", "repo");
    let unicode = "é".repeat(128);
    assert_eq!(unicode.len(), 256);
    let handle = provision(&repo, &unicode, Some(&unicode), "unicode");
    assert_export(&handle);
    WorkspaceTeardownRequest::new(repo.path())
        .teardown(&handle)
        .unwrap();
    for (index, field) in ["workspace_id", "task_id"].into_iter().enumerate() {
        let long = "x".repeat(201);
        let handle = provision(
            &repo,
            if field == "workspace_id" { &long } else { "w" },
            Some(if field == "task_id" { &long } else { "t" }),
            &format!("long-{index}"),
        );
        let before = handle.clone();
        assert_eq!(
            WorkspaceHandleWire::try_from(&handle),
            Err(WorkspaceHandleExportError::IdentifierOutOfRange {
                field,
                characters: 201
            })
        );
        assert_eq!(handle, before);
        assert!(worktree_registered(&repo, handle.worktree_path()));
        let torn = WorkspaceTeardownRequest::new(repo.path())
            .teardown(&handle)
            .unwrap();
        assert_eq!(
            WorkspaceHandleWire::try_from(&torn),
            Err(WorkspaceHandleExportError::IdentifierOutOfRange {
                field,
                characters: 201
            })
        );
    }
    let mut wire_only = minimal();
    wire_only["workspace_id"] = json!("🦀".repeat(200));
    assert_eq!(roundtrip(&wire_only).workspace_id().len(), 800);
}

#[cfg(unix)]
#[test]
fn handle_wire_refuses_native_non_utf8_path_without_loss() {
    use std::os::unix::ffi::OsStringExt;
    let path = std::path::PathBuf::from(std::ffi::OsString::from_vec(b"/native/\xff".to_vec()));
    // APFS cannot provision this name; exercise the export boundary with the
    // existing private constructor, without claiming successful provisioning.
    let handle = WorkspaceHandle::provisioned(
        "w".into(),
        None,
        "branch".into(),
        path.clone().into_boxed_path(),
        CommitSha::parse(&"a".repeat(40)).unwrap(),
        None,
        None,
    );
    assert_eq!(
        WorkspaceHandleWire::try_from(&handle),
        Err(WorkspaceHandleExportError::NonUtf8WorktreePath)
    );
    assert_eq!(handle.worktree_path(), path);
}

#[cfg(target_os = "linux")]
#[test]
fn handle_wire_actual_non_utf8_provisioned_path_refuses_export() {
    use std::os::unix::ffi::OsStringExt;
    let repo = TestRepo::new_nested("wire-native", "repo");
    let path = repo
        .root
        .path()
        .join(std::ffi::OsString::from_vec(vec![0xff]));
    let handle =
        WorkspaceProvisionRequest::new(repo.path(), "w", None, "native", path, &repo.head_sha())
            .unwrap()
            .provision()
            .unwrap();
    assert_eq!(
        WorkspaceHandleWire::try_from(&handle),
        Err(WorkspaceHandleExportError::NonUtf8WorktreePath)
    );
    let torn = WorkspaceTeardownRequest::new(repo.path())
        .teardown(&handle)
        .unwrap();
    assert_eq!(
        WorkspaceHandleWire::try_from(&torn),
        Err(WorkspaceHandleExportError::NonUtf8WorktreePath)
    );
}
