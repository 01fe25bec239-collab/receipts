use super::hook_map::*;
use super::*;
use crate::{
    NormalizedHostEventId, NormalizedHostEventInputs, NormalizedHostEventSourceClass,
    NormalizedHostEventType,
};
use receipts_orchestration::orchestration::{OrchestrationDateTimeV1, OrchestrationJsonObjectV1};
fn id() -> NormalizedHostEventId {
    NormalizedHostEventId::try_new("01J9ZZZZZZZZZZZZZZZZZZZZZZ").unwrap()
}
fn at() -> OrchestrationDateTimeV1 {
    OrchestrationDateTimeV1::try_new("2026-11-11T00:00:00Z").unwrap()
}
fn input(name: &str) -> Vec<u8> {
    format!(r#"{{"hook_event_name":"{name}","session_id":"s","cwd":"/repo","tool_name":"Bash","source":"startup","reason":"other","trigger":"manual","tool_input":{{"secret":"SECRET"}}}}"#).into_bytes()
}
#[test]
fn codex_hook_map_matches_claude_semantics_for_shared_events() {
    for (name, expected) in [
        ("SessionStart", NormalizedHostEventType::HostSessionStarted),
        ("SessionEnd", NormalizedHostEventType::HostSessionEnding),
        ("PostToolUse", NormalizedHostEventType::ToolExecuted),
        ("PreCompact", NormalizedHostEventType::ContextCompacted),
        ("PostCompact", NormalizedHostEventType::ContextCompacted),
    ] {
        let codex = normalize_codex_hook(name, &input(name), id(), at()).unwrap();
        let claude = crate::claude::parse_claude_hook(name, &input(name))
            .unwrap()
            .observed_event(id(), at())
            .unwrap();
        assert_eq!(codex.host().as_str(), "codex");
        assert_eq!(codex.confidence(), NormalizedHostEventConfidence::Observed);
        assert_eq!(codex.event_type(), expected);
        assert_eq!(codex.event_type(), claude.event_type());
        assert_eq!(codex.payload(), claude.payload());
        assert_eq!(codex.host_session_id(), "s");
        assert!(codex.project_id().is_none());
        assert!(codex.raw_ref().is_none());
        assert!(!crate::wire::normalized_host_event_to_json(&codex).contains("SECRET"));
    }
    assert_eq!(
        normalize_codex_hook("SessionStart", &input("SessionStart"), id(), at())
            .unwrap()
            .event_type(),
        NormalizedHostEventType::HostSessionStarted
    );
    assert_eq!(
        normalize_codex_hook("PostToolUse", &input("PostToolUse"), id(), at())
            .unwrap()
            .event_type(),
        NormalizedHostEventType::ToolExecuted
    );
}
#[test]
fn malformed_or_authority_bearing_hook_claims_are_refused() {
    use crate::claude::ClaudeHookError as E;
    for name in [
        "WorktreeCreate",
        "SubagentStart",
        "SubagentStop",
        "TaskCompleted",
        "UserPromptSubmit",
        "Stop",
        "PreToolUse",
        "PermissionRequest",
    ] {
        assert_eq!(
            normalize_codex_hook(name, &input(name), id(), at()),
            Err(E::UnknownEvent)
        );
    }
    assert_eq!(
        normalize_codex_hook("SessionStart", &input("SessionEnd"), id(), at()),
        Err(E::EventMismatch)
    );
    for bytes in [
        b"[]".as_slice(),
        b"{",
        b"\xff",
        br#"{"hook_event_name":"SessionStart","hook_event_name":"SessionStart"}"#,
    ] {
        assert!(normalize_codex_hook("SessionStart", bytes, id(), at()).is_err());
    }
    assert_eq!(
        normalize_codex_hook(
            "SessionStart",
            &vec![b' '; crate::claude::CLAUDE_HOOK_PAYLOAD_MAX_BYTES + 1],
            id(),
            at()
        ),
        Err(E::Oversized)
    );
}
fn report() -> HostCapabilityReport {
    crate::wire::host_capability_report_from_json(include_str!(
        "../../../fixtures/host_capability/03_codex_supervised_untrusted.json"
    ))
    .unwrap()
}
#[test]
fn supervised_codex_delivery_and_three_host_core_view_parity() {
    let root = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("receipts-codex-adapter-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let spool = HostEventSpool::for_project(&root).unwrap();
    let adapter = CodexHostAdapter::new(true, spool.clone(), report()).unwrap();
    assert!(adapter.detect());
    assert_eq!(adapter.id(), HostId::Codex);
    assert!(adapter.install(&()).contains("pending"));
    assert!(!root.join(".receipts").exists());
    let native = normalize_codex_hook("SessionStart", &input("SessionStart"), id(), at()).unwrap();
    assert_eq!(
        adapter.emit(&native),
        Err(CodexAdapterError::NativeQualificationPending)
    );
    assert!(!root.join(".receipts").exists());
    let supervised = normalize_codex_supervised_event(NormalizedHostEventInputs {
        event_id: id(),
        event_type: NormalizedHostEventType::HostSessionStarted,
        host: HostId::Codex.into(),
        host_session_id: "s".into(),
        project_id: None,
        occurred_at: at(),
        payload: OrchestrationJsonObjectV1::new(Default::default()),
        raw_ref: None,
        confidence: NormalizedHostEventConfidence::Observed,
    })
    .unwrap();
    assert_eq!(
        supervised.confidence(),
        NormalizedHostEventConfidence::Inferred
    );
    assert_eq!(
        crate::emit_validated_normalized_host_event(
            &adapter,
            NormalizedHostEventSourceClass::HostHook,
            &supervised
        ),
        Ok(Ok(()))
    );
    let bytes = std::fs::read(spool.path()).unwrap();
    assert!(
        crate::emit_validated_normalized_host_event(
            &adapter,
            NormalizedHostEventSourceClass::CoreDriven,
            &supervised
        )
        .is_err()
    );
    assert_eq!(std::fs::read(spool.path()).unwrap(), bytes);
    let claude = crate::ClaudeHostAdapter::new(
        Default::default(),
        spool.clone(),
        crate::adapter_tests::canonical_report(),
    );
    let headless =
        crate::HeadlessAdapter::new(spool.clone(), crate::adapter_tests::canonical_report());
    let view = ClaudeCoreView {
        view_type: crate::claude::ClaudeCoreViewType::Dag,
        content: OrchestrationJsonObjectV1::new(Default::default()),
    };
    assert_eq!(
        adapter.present(&view).as_bytes(),
        claude.present(&view).as_bytes()
    );
    assert_eq!(
        adapter.present(&view).as_bytes(),
        headless.present(&view).as_bytes()
    );
    assert_eq!(adapter.capabilities(), report());
    let embedded = crate::wire::host_capability_report_from_json(include_str!(
        "../../../fixtures/host_capability/01_codex_embedded.json"
    ))
    .unwrap();
    assert!(matches!(
        CodexHostAdapter::new(true, spool.clone(), embedded),
        Err(CodexAdapterError::NativeQualificationPending)
    ));
    assert!(matches!(
        CodexHostAdapter::new(true, spool, crate::adapter_tests::canonical_report()),
        Err(CodexAdapterError::WrongHost)
    ));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn synthetic_native_observations_preserve_details_without_admitting_delivery() {
    for (name, input, expected) in [
        (
            "SessionStart",
            include_str!("fixtures/SessionStart.json"),
            r#""start_source":"startup""#,
        ),
        (
            "SessionEnd",
            include_str!("fixtures/SessionEnd.json"),
            r#""end_reason":"other""#,
        ),
        (
            "PostToolUse",
            include_str!("fixtures/PostToolUse.json"),
            r#""tool_name":"shell_command""#,
        ),
        (
            "PreCompact",
            include_str!("fixtures/PreCompact.json"),
            r#""phase":"PRE""#,
        ),
        (
            "PostCompact",
            include_str!("fixtures/PostCompact.json"),
            r#""phase":"POST""#,
        ),
    ] {
        let event = normalize_codex_hook(name, input.as_bytes(), id(), at()).unwrap();
        let json = crate::wire::normalized_host_event_to_json(&event);
        assert!(json.contains(expected), "{json}");
        if name.ends_with("Compact") {
            assert!(json.contains(r#""trigger":"manual""#));
        }
        assert!(event.raw_ref().is_none());
        assert!(
            normalize_codex_hook("SessionStart", input.as_bytes(), id(), at()).is_err()
                || name == "SessionStart"
        );
    }
}
