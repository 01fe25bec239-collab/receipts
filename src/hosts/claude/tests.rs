use super::*;
use crate::{
    NormalizedHostEventConfidence as Confidence, NormalizedHostEventId,
    NormalizedHostEventType as T,
};
use receipts_orchestration::orchestration::{
    OrchestrationDateTimeV1, OrchestrationJsonValueV1 as V,
};

fn id() -> NormalizedHostEventId {
    NormalizedHostEventId::try_new("01J9ZZZZZZZZZZZZZZZZZZZZZZ").unwrap()
}
fn at() -> OrchestrationDateTimeV1 {
    OrchestrationDateTimeV1::try_new("2026-11-08T00:00:00.000Z").unwrap()
}
fn input(event: &str, extra: &str) -> Vec<u8> {
    format!(
        r#"{{"session_id":"abc-123","transcript_path":"/SECRET/transcript.jsonl","cwd":"/repo","hook_event_name":"{event}"{extra}}}"#
    )
    .into_bytes()
}

#[test]
#[allow(clippy::type_complexity)]
fn each_required_claude_hook_maps_to_expected_event() {
    let cases: [(&str, &str, T, Vec<(&str, &str)>); 6] = [
        (
            "SessionStart",
            r#","source":"resume""#,
            T::HostSessionStarted,
            vec![("start_source", "resume")],
        ),
        (
            "SessionEnd",
            r#","reason":"logout""#,
            T::HostSessionEnding,
            vec![("end_reason", "logout")],
        ),
        (
            "PostToolUse",
            r#","tool_name":"Bash","tool_input":{"command":"SECRET"},"tool_response":"SECRET""#,
            T::ToolExecuted,
            vec![("tool_name", "Bash")],
        ),
        (
            "PreCompact",
            r#","trigger":"auto","custom_instructions":"SECRET""#,
            T::ContextCompacted,
            vec![("phase", "PRE"), ("trigger", "auto")],
        ),
        (
            "PostCompact",
            r#","trigger":"manual""#,
            T::ContextCompacted,
            vec![("phase", "POST"), ("trigger", "manual")],
        ),
        // Unknown closed-set values are dropped, never echoed.
        (
            "SessionStart",
            r#","source":"SECRET""#,
            T::HostSessionStarted,
            vec![],
        ),
    ];
    for (hook, extra, expected, payload) in cases {
        let invocation = parse_claude_hook(hook, &input(hook, extra)).unwrap();
        let event = invocation.observed_event(id(), at()).unwrap();
        assert_eq!(event.event_type(), expected, "{hook}");
        assert_eq!(event.confidence(), Confidence::Observed);
        assert_eq!(event.host().as_str(), "claude-code");
        assert_eq!(event.host_session_id(), "abc-123");
        assert_eq!(event.project_id(), None);
        assert_eq!(event.raw_ref(), None);
        let got: Vec<_> = event
            .payload()
            .as_map()
            .iter()
            .map(|(k, v)| match v {
                V::String(s) => (k.as_str(), s.as_str()),
                _ => panic!(),
            })
            .collect();
        let mut want = payload.clone();
        want.sort();
        assert_eq!(got, want, "{hook}");
        assert!(!crate::wire::normalized_host_event_to_json(&event).contains("SECRET"));
    }
    // Worktree hooks are parsed but produce events only from outcomes.
    let create = parse_claude_hook(
        "WorktreeCreate",
        &input("WorktreeCreate", r#","name":"feat-1""#),
    )
    .unwrap();
    assert_eq!(create.observed_event(id(), at()), None);
    assert_eq!(
        create.detail(),
        &ClaudeHookDetail::WorktreeCreate {
            name: "feat-1".into()
        }
    );
    let plain = create
        .workspace_created_event(
            Path::new("/repo/.claude/worktrees/feat-1"),
            "PLAIN_CREATED",
            false,
            id(),
            at(),
        )
        .unwrap();
    assert_eq!(plain.event_type(), T::WorkspaceCreated);
    assert_eq!(plain.confidence(), Confidence::Inferred);
    let task = create
        .workspace_created_event(Path::new("/w"), "TASK_PROVISIONED", true, id(), at())
        .unwrap();
    assert_eq!(task.confidence(), Confidence::Observed);
    let remove = parse_claude_hook(
        "WorktreeRemove",
        &input(
            "WorktreeRemove",
            r#","worktree_path":"/repo/.claude/worktrees/feat-1""#,
        ),
    )
    .unwrap();
    assert_eq!(remove.observed_event(id(), at()), None);
    assert_eq!(
        remove.workspace_created_event(Path::new("/w"), "x", false, id(), at()),
        None
    );
}

#[test]
fn every_registered_hook_has_a_required_event_and_the_plugin_registers_exactly_them() {
    let names: Vec<_> = ClaudeHookEvent::ALL.iter().map(|e| e.as_str()).collect();
    assert_eq!(
        names,
        crate::host_capability_observation::CLAUDE_REQUIRED_HOOK_EVENTS
    );
    let manifest = include_str!("../../../plugins/claude/hooks/hooks.json");
    let V::Object(root) = V::parse_json(manifest).unwrap() else {
        panic!()
    };
    let V::Object(hooks) = &root.as_map()["hooks"] else {
        panic!()
    };
    let registered: Vec<_> = hooks.as_map().keys().map(String::as_str).collect();
    let mut expected = names.clone();
    expected.sort();
    assert_eq!(registered, expected);
    for (name, groups) in hooks.as_map() {
        let V::Array(groups) = groups else { panic!() };
        for group in groups {
            let V::Object(group) = group else { panic!() };
            let V::Array(handlers) = &group.as_map()["hooks"] else {
                panic!()
            };
            for handler in handlers {
                let V::Object(h) = handler else { panic!() };
                assert_eq!(h.as_map()["type"], V::String("command".into()));
                // argv only: the fixed launcher plus the fixed event literal.
                assert_eq!(
                    h.as_map()["command"],
                    V::String(format!(
                        "\"${{CLAUDE_PLUGIN_ROOT}}/bin/receipts-hook\" {name}"
                    ))
                );
            }
        }
    }
    assert!(ClaudeHookEvent::WorktreeCreate.is_blocking());
    assert_eq!(
        ClaudeHookEvent::ALL
            .iter()
            .filter(|e| e.is_blocking())
            .count(),
        1
    );
}

#[test]
fn oversized_hook_payload_rejected() {
    let padded = |n: usize| input("SessionStart", &format!(r#","pad":"{}""#, "x".repeat(n)));
    let base = padded(0).len();
    let big = padded(CLAUDE_HOOK_PAYLOAD_MAX_BYTES + 1 - base);
    assert_eq!(big.len(), CLAUDE_HOOK_PAYLOAD_MAX_BYTES + 1);
    assert_eq!(
        parse_claude_hook("SessionStart", &big),
        Err(ClaudeHookError::Oversized)
    );
    // Exactly at the bound is still parsed (and then judged on content).
    let edge = padded(CLAUDE_HOOK_PAYLOAD_MAX_BYTES - base);
    assert_eq!(edge.len(), CLAUDE_HOOK_PAYLOAD_MAX_BYTES);
    assert_eq!(parse_claude_hook("SessionStart", &edge).map(|_| ()), Ok(()));
}

#[test]
fn malformed_path_and_mismatched_hook_input_is_refused_without_echo() {
    use ClaudeHookError as E;
    let deep = format!(
        r#"{{"session_id":"s","cwd":"/r","hook_event_name":"SessionStart","x":{}0{}}}"#,
        "[".repeat(200),
        "]".repeat(200)
    );
    let cases: Vec<(&str, Vec<u8>, E)> = vec![
        ("Stop", input("Stop", ""), E::UnknownEvent),
        ("SessionStart", b"\xff".to_vec(), E::InvalidUtf8),
        ("SessionStart", b"not json".to_vec(), E::InvalidJson),
        ("SessionStart", b"[1]".to_vec(), E::NotAnObject),
        ("SessionStart", deep.into_bytes(), E::InvalidJson),
        (
            "SessionStart",
            input("SessionStart", r#","session_id":"dup""#),
            E::InvalidJson,
        ),
        ("SessionStart", input("SessionEnd", ""), E::EventMismatch),
        (
            "SessionStart",
            br#"{"session_id":"s","cwd":"/r"}"#.to_vec(),
            E::EventMismatch,
        ),
        (
            "SessionStart",
            br#"{"session_id":"a b","cwd":"/r","hook_event_name":"SessionStart"}"#.to_vec(),
            E::InvalidField("session_id"),
        ),
        (
            "SessionStart",
            format!(
                r#"{{"session_id":"{}","cwd":"/r","hook_event_name":"SessionStart"}}"#,
                "s".repeat(201)
            )
            .into_bytes(),
            E::InvalidField("session_id"),
        ),
        (
            "SessionStart",
            br#"{"session_id":"s","cwd":"relative/SECRET","hook_event_name":"SessionStart"}"#
                .to_vec(),
            E::InvalidField("cwd"),
        ),
        (
            "SessionStart",
            br#"{"session_id":"s","cwd":"/a\u0000b","hook_event_name":"SessionStart"}"#.to_vec(),
            E::InvalidField("cwd"),
        ),
        (
            "PostToolUse",
            input("PostToolUse", r#","tool_name":"rm -rf /""#),
            E::InvalidField("tool_name"),
        ),
        (
            "PostToolUse",
            input("PostToolUse", ""),
            E::InvalidField("tool_name"),
        ),
        (
            "WorktreeCreate",
            input("WorktreeCreate", r#","name":"../SECRET""#),
            E::InvalidField("name"),
        ),
        (
            "WorktreeCreate",
            input("WorktreeCreate", r#","name":"a/b""#),
            E::InvalidField("name"),
        ),
        (
            "WorktreeCreate",
            input("WorktreeCreate", r#","name":".git""#),
            E::InvalidField("name"),
        ),
        (
            "WorktreeCreate",
            input("WorktreeCreate", r#","name":"-f""#),
            E::InvalidField("name"),
        ),
        (
            "WorktreeCreate",
            input("WorktreeCreate", r#","name":"""#),
            E::InvalidField("name"),
        ),
        (
            "WorktreeCreate",
            input("WorktreeCreate", r#","name":7"#),
            E::InvalidField("name"),
        ),
        (
            "WorktreeRemove",
            input("WorktreeRemove", r#","worktree_path":"rel""#),
            E::InvalidField("worktree_path"),
        ),
    ];
    for (hook, bytes, expected) in cases {
        let error = parse_claude_hook(hook, &bytes).unwrap_err();
        assert_eq!(
            error,
            expected,
            "{hook} {}",
            String::from_utf8_lossy(&bytes)
        );
        assert!(!format!("{error}{error:?}").contains("SECRET"));
    }
}

#[test]
fn worktree_create_response_format_exact() {
    assert_eq!(
        worktree_create_stdout(Path::new("/repo/.claude/worktrees/feat-1")).as_deref(),
        Some("/repo/.claude/worktrees/feat-1\n")
    );
    assert_eq!(worktree_create_stdout(Path::new("relative")), None);
    assert_eq!(worktree_create_stdout(Path::new("/a\nb")), None);
    assert_eq!(
        worktree_target(Path::new("/repo"), "feat-1"),
        (
            PathBuf::from("/repo/.claude/worktrees/feat-1"),
            "worktree-feat-1".to_owned()
        )
    );
}

#[test]
fn execution_evidence_requires_an_absolute_host_plugin_root() {
    let invocation = parse_claude_hook("SessionStart", &input("SessionStart", "")).unwrap();
    assert!(
        invocation
            .execution_evidence(Path::new("relative"))
            .is_none()
    );
    assert!(
        invocation
            .execution_evidence(Path::new("/plugins/receipts"))
            .is_none()
    );
}

#[test]
fn claude_adapter_binds_spool_and_never_installs() {
    let root = std::fs::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("receipts-claude-adapter-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let spool = HostEventSpool::for_project(&root).unwrap();
    let mut adapter = ClaudeHostAdapter::new(
        ClaudeHostEnvironment { claudecode: true },
        spool.clone(),
        crate::adapter_tests::canonical_report(),
    );
    assert_eq!(adapter.id(), HostId::ClaudeCode);
    assert!(adapter.detect());
    assert_eq!(adapter.start(), spool);
    let steps = adapter.install(&ClaudeInstallPlan {
        plugin_dir: "/p".into(),
        receipts_bin: None,
    });
    assert!(steps[0].contains("--plugin-dir /p"));
    assert!(steps.iter().any(|s| s.contains("/p/bin/receipts")));
    assert!(!root.join(".receipts").exists(), "install performs nothing");
    let event = parse_claude_hook("SessionStart", &input("SessionStart", ""))
        .unwrap()
        .observed_event(id(), at())
        .unwrap();
    crate::emit_validated_normalized_host_event(
        &adapter,
        crate::NormalizedHostEventSourceClass::HostHook,
        &event,
    )
    .unwrap()
    .unwrap();
    let line = std::fs::read_to_string(spool.path()).unwrap();
    assert_eq!(
        crate::wire::normalized_host_event_from_json(line.trim_end()).unwrap(),
        event
    );
    // A disallowed source class never reaches the spool.
    assert!(
        crate::emit_validated_normalized_host_event(
            &adapter,
            crate::NormalizedHostEventSourceClass::WorkerDispatch,
            &event,
        )
        .is_err()
    );
    assert_eq!(std::fs::read_to_string(spool.path()).unwrap(), line);
    let view = ClaudeCoreView {
        view_type: ClaudeCoreViewType::Error,
        content: Default::default(),
    };
    assert_eq!(adapter.present(&view), "receipts error: {}");
    let pending = adapter.request_user_input("q".into());
    assert_eq!(pending.into_inner(), ClaudeUserResponse::Unavailable);
    assert_eq!(
        adapter.capabilities(),
        crate::adapter_tests::canonical_report()
    );
    adapter.shutdown(Some("logout"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn trusted_translation_requires_matching_type_session_host_and_source() {
    use crate::{
        NormalizedHostEvent, NormalizedHostEventInputs, NormalizedHostEventSourceClass as S,
    };
    for (hook, event_type, class) in [
        ("FileChanged", T::WorkspaceChanged, S::WorkerDispatch),
        ("SubagentStart", T::RoleExecutorStarted, S::CoreDriven),
        ("SubagentStop", T::RoleExecutorStopped, S::CoreDriven),
        ("TaskCompleted", T::TaskCompleted, S::WorkerDispatch),
    ] {
        let invocation = parse_claude_hook(hook, &input(hook, "")).unwrap();
        assert!(invocation.observed_event(id(), at()).is_none());
        let event = |kind, host: &str, session: &str| {
            NormalizedHostEvent::try_new(NormalizedHostEventInputs {
                event_id: id(),
                event_type: kind,
                host: crate::NormalizedHostEventHost::try_new(host).unwrap(),
                host_session_id: session.into(),
                project_id: None,
                occurred_at: at(),
                payload: Default::default(),
                raw_ref: None,
                confidence: Confidence::Observed,
            })
            .unwrap()
        };
        assert!(invocation.admits_translation(class, &event(event_type, "claude-code", "abc-123")));
        assert!(
            !invocation
                .admits_translation(S::HostHook, &event(event_type, "claude-code", "abc-123"))
        );
        assert!(
            !invocation.admits_translation(class, &event(event_type, "claude-code", "another"))
        );
        assert!(!invocation.admits_translation(class, &event(event_type, "codex", "abc-123")));
        assert!(
            !invocation.admits_translation(class, &event(T::HostError, "claude-code", "abc-123"))
        );
    }
}
