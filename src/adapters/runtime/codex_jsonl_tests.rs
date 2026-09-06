use std::{error::Error, time::Duration};

use receipts_workspace_execution::execution::{
    ProcessTermination, ProcessTimeoutPolicy, STREAM_CAPTURE_LIMIT_BYTES,
};

use crate::codex_task_execution::{TaskCaptureSnapshot, execute_with_runner};
use crate::{
    CodexJsonlErrorKind, CodexJsonlEventKind as Kind, CodexProtocolTermination as Termination,
    CodexTaskExecutionRequest, CodexTaskExecutionResult, CodexTaskSandboxMode, FailureClass,
    classify_codex_task_execution_result, interpret_codex_jsonl,
};

const COMPLETED: &str = r#"{"type":"turn.completed","usage":{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1}}"#;
const FAILED: &str = r#"{"type":"turn.failed","error":{"message":"synthetic failure"}}"#;
const MESSAGE: &str = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"CODEX_JSONL_PROBE_OK"}}"#;

// Use the existing execution seam to construct a real immutable production
// result. The public interpreter is exercised without an alternate parser API.
fn execution(stdout: &[u8], stderr: &[u8], exit_code: i32) -> CodexTaskExecutionResult {
    assert!(stdout.len() as u64 <= STREAM_CAPTURE_LIMIT_BYTES);
    assert!(stderr.len() as u64 <= STREAM_CAPTURE_LIMIT_BYTES);
    let request = CodexTaskExecutionRequest::new(
        "/tmp/fake-codex-bin",
        "/tmp",
        "/tmp",
        ProcessTimeoutPolicy::new(Duration::from_secs(30), Duration::from_secs(3)).unwrap(),
        CodexTaskSandboxMode::ReadOnly,
        "fixture",
    );
    execute_with_runner(&request, |_, _| {
        // Split inside arbitrary bytes, as head/tail may split a JSON record.
        let split = stdout.len() / 2;
        Ok(TaskCaptureSnapshot::new(
            ProcessTermination::Completed,
            Some(exit_code),
            stdout[..split].to_vec(),
            stdout[split..].to_vec(),
            false,
            stderr.to_vec(),
            Vec::new(),
            false,
        ))
    })
    .unwrap()
}

#[test]
fn one_line_with_or_without_final_newline() {
    for stdout in [
        "{\"type\":\"turn.started\"}",
        "{\"type\":\"turn.started\"}\n",
    ] {
        let result = execution(stdout.as_bytes(), b"", 0);
        let parsed = interpret_codex_jsonl(&result).unwrap();
        assert_eq!(parsed.events().len(), 1);
        assert_eq!(parsed.events()[0].kind(), Kind::TurnStarted);
        assert_eq!(parsed.events()[0].raw_record(), stdout.as_bytes());
        assert_eq!(parsed.termination(), Termination::Indeterminate);
    }
}

#[test]
fn live_probe_shape_order_and_exact_raw_evidence() {
    let records = [
        " {\"thread_id\":\"synthetic-thread\", \"type\":\"thread.started\"} \r\n".to_string(),
        "{\"type\":\"turn.started\"}\n".to_string(),
        format!("{MESSAGE}\n"),
        format!("{COMPLETED}\n"),
    ];
    let stdout = records.concat();
    let result = execution(
        stdout.as_bytes(),
        b"Reading additional input from stdin...\n",
        0,
    );
    let parsed = interpret_codex_jsonl(&result).unwrap();
    assert_eq!(
        parsed.events().iter().map(|e| e.kind()).collect::<Vec<_>>(),
        [
            Kind::ThreadStarted,
            Kind::TurnStarted,
            Kind::ItemCompleted,
            Kind::TurnCompleted
        ]
    );
    for (event, record) in parsed.events().iter().zip(&records) {
        assert_eq!(event.raw_record(), record.as_bytes());
    }
    assert_eq!(
        parsed
            .events()
            .iter()
            .flat_map(|e| e.raw_record().iter().copied())
            .collect::<Vec<_>>(),
        stdout.as_bytes()
    );
    assert_eq!(
        parsed.events()[2].completed_agent_message(),
        Some("CODEX_JSONL_PROBE_OK")
    );
    assert!(parsed.turn_completed_observed());
    assert!(!parsed.turn_failed_observed());
    assert_eq!(parsed.termination(), Termination::Completed);
    assert_eq!(parsed.final_agent_message(), Some("CODEX_JSONL_PROBE_OK"));
}

#[test]
fn malformed_records_fail_without_skipping_or_joining() {
    let bad: &[(&[u8], CodexJsonlErrorKind)] = &[
        (b"{not json}", CodexJsonlErrorKind::InvalidJson),
        (
            b"{\"type\":\"turn.started\"",
            CodexJsonlErrorKind::IncompleteJson,
        ),
        (b"garbage", CodexJsonlErrorKind::InvalidJson),
        (b"\n", CodexJsonlErrorKind::IncompleteJson),
        (b"  \t\n", CodexJsonlErrorKind::IncompleteJson),
        (
            b"{\n\"type\":\"turn.started\"}",
            CodexJsonlErrorKind::IncompleteJson,
        ),
        (
            b"{\"type\":\"turn.started\"} {}",
            CodexJsonlErrorKind::InvalidJson,
        ),
        (b"{\"type\":\"\xff\"}", CodexJsonlErrorKind::InvalidUtf8),
        (b"{\"type\":\"\\uD800\"}", CodexJsonlErrorKind::InvalidJson),
    ];
    for (record, kind) in bad {
        let mut stdout = b"{\"type\":\"turn.started\"}\n".to_vec();
        stdout.extend_from_slice(record);
        let result = execution(&stdout, b"", 0);
        let error = interpret_codex_jsonl(&result)
            .err()
            .expect("must fail closed");
        assert_eq!(error.kind, *kind);
        assert_eq!(error.line, 2);
        assert_eq!(result.exit_code(), 0);
    }
}

#[test]
fn invalid_envelopes_and_consumed_fields_fail_explicitly() {
    for record in [
        "null",
        "[]",
        "42",
        "{}",
        r#"{"type":7}"#,
        r#"{"type":"thread.started"}"#,
        r#"{"type":"turn.completed"}"#,
        r#"{"type":"turn.completed","usage":{"input_tokens":"1","cached_input_tokens":0,"output_tokens":1}}"#,
        r#"{"type":"turn.failed","error":"failure"}"#,
        r#"{"type":"error","message":42}"#,
        r#"{"type":"item.completed","item":null}"#,
        r#"{"type":"item.completed","item":{"id":"i","type":"agent_message","text":42}}"#,
        r#"{"type":"item.completed","item":{"id":"i","type":"future","status":false}}"#,
    ] {
        let result = execution(record.as_bytes(), b"", 0);
        assert_eq!(
            interpret_codex_jsonl(&result).err().unwrap().kind,
            CodexJsonlErrorKind::InvalidEventShape
        );
    }
}

#[test]
fn unknown_event_after_completed_is_preserved_and_indeterminate() {
    let future = r#"{"type":"future.終端","status":"almost_completed","new":{"data":[1,2]}}"#;
    let result = execution(format!("{COMPLETED}\n{future}").as_bytes(), b"", 0);
    let parsed = interpret_codex_jsonl(&result).unwrap();
    assert_eq!(parsed.events()[1].kind(), Kind::Unknown);
    assert_eq!(parsed.events()[1].event_type(), "future.終端");
    assert_eq!(parsed.events()[1].raw_record(), future.as_bytes());
    assert_eq!(parsed.events()[1].value()["status"], "almost_completed");
    assert!(parsed.turn_completed_observed());
    assert_eq!(parsed.termination(), Termination::Indeterminate);
}

#[test]
fn item_types_and_statuses_are_open_unclassified_strings() {
    for item_type in [
        "agent_message",
        "reasoning",
        "command_execution",
        "file_change",
        "mcp_tool_call",
        "collab_tool_call",
        "web_search",
        "todo_list",
        "error",
        "未来_item",
    ] {
        for status in [
            "in_progress",
            "completed",
            "failed",
            "declined",
            "completed_未来",
            "",
        ] {
            let record = serde_json::json!({"type":"item.completed","item":{"id":"i","type":item_type,"status":status,"text":"hello"}}).to_string();
            let result = execution(record.as_bytes(), b"", 0);
            let parsed = interpret_codex_jsonl(&result).unwrap();
            let event = &parsed.events()[0];
            assert_eq!(event.item_type(), Some(item_type));
            assert_eq!(event.item_status(), Some(status));
            assert_eq!(event.raw_record(), record.as_bytes());
            assert_eq!(event.completed_agent_message(), None);
            assert_eq!(parsed.termination(), Termination::Indeterminate);
        }
    }
}

#[test]
fn all_known_event_envelopes_are_represented() {
    let records = [
        (
            r#"{"type":"thread.started","thread_id":"t"}"#,
            Kind::ThreadStarted,
        ),
        (r#"{"type":"turn.started"}"#, Kind::TurnStarted),
        (COMPLETED, Kind::TurnCompleted),
        (FAILED, Kind::TurnFailed),
        (
            r#"{"type":"item.started","item":{"id":"i","type":"future"}}"#,
            Kind::ItemStarted,
        ),
        (
            r#"{"type":"item.updated","item":{"id":"i","type":"future"}}"#,
            Kind::ItemUpdated,
        ),
        (MESSAGE, Kind::ItemCompleted),
        (
            r#"{"type":"error","message":"429 AUTH_REQUIRED rate limit policy blocked"}"#,
            Kind::Error,
        ),
    ];
    for (record, kind) in records {
        let result = execution(record.as_bytes(), b"", 7);
        let parsed = interpret_codex_jsonl(&result).unwrap();
        assert_eq!(parsed.events()[0].kind(), kind);
        assert_eq!(
            classify_codex_task_execution_result(&result),
            Some(FailureClass::Unknown)
        );
    }
}

#[test]
fn unicode_and_large_field_at_capture_limit_are_preserved() {
    let prefix = r#"{"type":"item.completed","item":{"id":"i","type":"agent_message","text":""#;
    let suffix = "\"}}\n";
    let budget = STREAM_CAPTURE_LIMIT_BYTES as usize - prefix.len() - suffix.len();
    let unit = "नमस्ते 🦀 e\u{301} 世界";
    let text = unit.repeat(budget / unit.len()) + &"x".repeat(budget % unit.len());
    let stdout = format!("{prefix}{text}{suffix}");
    assert_eq!(stdout.len() as u64, STREAM_CAPTURE_LIMIT_BYTES);
    let result = execution(stdout.as_bytes(), b"", 0);
    let parsed = interpret_codex_jsonl(&result).unwrap();
    assert_eq!(
        parsed.events()[0].completed_agent_message(),
        Some(text.as_str())
    );
    assert_eq!(parsed.events()[0].raw_record(), stdout.as_bytes());
}

#[test]
fn stderr_json_warnings_and_invalid_utf8_never_become_events() {
    let stderr = [
        COMPLETED.as_bytes(),
        b"\nWARNING: synthetic warning\n\xff",
        MESSAGE.as_bytes(),
    ]
    .concat();
    for stdout in [b"".as_slice(), b"{\"type\":\"turn.started\"}\n"] {
        let result = execution(stdout, &stderr, 0);
        let parsed = interpret_codex_jsonl(&result).unwrap();
        assert_eq!(parsed.events().len(), usize::from(!stdout.is_empty()));
        assert!(!parsed.turn_completed_observed());
        assert!(!parsed.turn_failed_observed());
        assert_eq!(parsed.termination(), Termination::Indeterminate);
        assert_eq!(result.stderr(), stderr);
    }
}

#[test]
fn process_and_protocol_cross_product_is_not_reconciled() {
    for exit in [0, 1, 7, 124, 137, 255] {
        for (record, termination, completed, failed) in [
            (COMPLETED, Termination::Completed, true, false),
            (FAILED, Termination::Failed, false, true),
            ("", Termination::Indeterminate, false, false),
        ] {
            let result = execution(record.as_bytes(), b"", exit);
            let parsed = interpret_codex_jsonl(&result).unwrap();
            assert_eq!(parsed.exit_code(), exit);
            assert_eq!(parsed.termination(), termination);
            assert_eq!(parsed.turn_completed_observed(), completed);
            assert_eq!(parsed.turn_failed_observed(), failed);
        }
        let result = execution(b"broken", b"", exit);
        assert!(interpret_codex_jsonl(&result).is_err());
        assert_eq!(result.exit_code(), exit);
    }
}

#[test]
fn conflicting_signals_and_nonterminal_suffix_are_indeterminate() {
    for stdout in [
        format!("{COMPLETED}\n{FAILED}"),
        format!("{FAILED}\n{COMPLETED}"),
    ] {
        let result = execution(stdout.as_bytes(), b"", 0);
        let parsed = interpret_codex_jsonl(&result).unwrap();
        assert!(parsed.turn_completed_observed());
        assert!(parsed.turn_failed_observed());
        assert_eq!(parsed.termination(), Termination::Indeterminate);
    }
    let result = execution(
        format!("{COMPLETED}\n{{\"type\":\"turn.started\"}}").as_bytes(),
        b"",
        0,
    );
    assert_eq!(
        interpret_codex_jsonl(&result).unwrap().termination(),
        Termination::Indeterminate
    );
}

#[test]
fn messages_are_preserved_without_selection_or_concatenation() {
    let started = MESSAGE.replace("item.completed", "item.started");
    let result = execution(
        format!("{started}\n{MESSAGE}\n{MESSAGE}\n{COMPLETED}").as_bytes(),
        b"",
        0,
    );
    let parsed = interpret_codex_jsonl(&result).unwrap();
    assert_eq!(parsed.events()[0].completed_agent_message(), None);
    assert_eq!(
        parsed
            .events()
            .iter()
            .filter_map(|e| e.completed_agent_message())
            .collect::<Vec<_>>(),
        ["CODEX_JSONL_PROBE_OK", "CODEX_JSONL_PROBE_OK"]
    );
}

#[test]
fn credential_like_malformed_payload_never_enters_error_surfaces() {
    const SECRET: &str = "sk-test-NOT-A-REAL-CREDENTIAL-a3-010";
    for record in [
        format!("{{\"type\":\"{SECRET}\",broken}}"),
        format!("{{\"type\":\"{SECRET}\""),
        format!("{{\"type\":\"item.completed\",\"item\":\"{SECRET}\"}}"),
    ] {
        let result = execution(record.as_bytes(), b"", 0);
        let error = interpret_codex_jsonl(&result).err().unwrap();
        assert!(!error.to_string().contains(SECRET));
        assert!(!format!("{error:?}").contains(SECRET));
        assert!(error.source().is_none());
        assert_eq!(error.line, 1);
    }
}

#[test]
fn final_message_requires_one_recognized_completed_turn_and_one_message() {
    for exit in [0, 7] {
        let result = execution(
            format!("{{\"type\":\"turn.started\"}}\n{MESSAGE}\n{COMPLETED}").as_bytes(),
            b"",
            exit,
        );
        assert_eq!(
            interpret_codex_jsonl(&result)
                .unwrap()
                .final_agent_message(),
            Some("CODEX_JSONL_PROBE_OK")
        );
    }
    let started = r#"{"type":"turn.started"}"#;
    let unknown = r#"{"type":"future"}"#;
    let error = r#"{"type":"error","message":"synthetic"}"#;
    let future_message = MESSAGE.replace("\"text\":", "\"status\":\"future\",\"text\":");
    for records in [
        vec![started, COMPLETED],
        vec![MESSAGE, started, COMPLETED],
        vec![started, MESSAGE, MESSAGE, COMPLETED],
        vec![started, MESSAGE, FAILED],
        vec![started, MESSAGE, unknown, COMPLETED],
        vec![started, MESSAGE, error, COMPLETED],
        vec![started, MESSAGE, COMPLETED, started, COMPLETED],
        vec![started, &future_message, COMPLETED],
        vec![started, MESSAGE, &future_message, COMPLETED],
    ] {
        let result = execution(records.join("\n").as_bytes(), b"", 0);
        assert_eq!(
            interpret_codex_jsonl(&result)
                .unwrap()
                .final_agent_message(),
            None
        );
    }
}
