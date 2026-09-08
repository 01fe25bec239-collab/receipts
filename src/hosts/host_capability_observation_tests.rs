use super::*;
use crate::{
    HostCapabilityReportSelectionCompositionOutcome as Outcome, HostCapabilitySelectedMode as Mode,
    HostCapabilityStaleReason as Stale, compose_host_capability_report_selection,
};
use std::{path::Path, time::Duration};

fn config(source: Source, text: &str) -> Result<HostObservedConfiguration, Error> {
    decode::configuration(source, text.as_bytes())
}
fn plugins(text: &str) -> Result<Vec<HostObservedPlugin>, Error> {
    decode::claude_plugins(text.as_bytes())
}

#[test]
fn documented_claude_list_retains_only_receipts_facts_verbatim() {
    let list = plugins(r#"[{"id":"other@market","version":"unrelated"},{"id":"receipts@market","version":" 1.2.3 ","enabled":true,"installPath":"SECRET_PATH","mcpServers":{"token":"SECRET_TOKEN"}}]"#).unwrap();
    assert_eq!(
        list,
        vec![HostObservedPlugin {
            id: "receipts@market".into(),
            version: Value::Known(" 1.2.3 ".into()),
            enabled: Value::Known(true)
        }]
    );
    assert!(!format!("{list:?}").contains("SECRET"));
}

#[test]
fn absent_plugin_is_not_failed_plugin_probe() {
    assert!(plugins("[]").unwrap().is_empty());
    assert!(
        plugins(r#"[{"id":"not-receipts@market"}]"#)
            .unwrap()
            .is_empty()
    );
    assert_eq!(plugins("{}"), Err(Error::InvalidJson));
}

#[test]
fn missing_null_and_false_remain_distinct() {
    let list = plugins(
        r#"[{"id":"receipts"},{"id":"receipts","enabled":null},{"id":"receipts","enabled":false}]"#,
    )
    .unwrap();
    assert_eq!(list[0].enabled, Value::Absent);
    assert_eq!(list[1].enabled, Value::Unknown);
    assert_eq!(list[2].enabled, Value::Known(false));
    assert_eq!(list[0].version, Value::Absent);
    for (input, expected) in [
        ("{}", Value::Absent),
        (r#"{"disableAllHooks":null}"#, Value::Unknown),
        (r#"{"disableAllHooks":false}"#, Value::Known(false)),
    ] {
        assert_eq!(
            config(Source::ClaudeUserSettings, input)
                .unwrap()
                .disable_all_hooks,
            expected
        );
    }
}

#[test]
fn required_shape_and_known_json_types_fail_the_whole_source() {
    for input in [
        "null",
        "{}",
        "[{}]",
        r#"[["receipts"]]"#,
        r#"[{"id":null}]"#,
        r#"[{"id":4}]"#,
        r#"[{"id":""}]"#,
        r#"[{"id":"receipts","enabled":"true"}]"#,
        r#"[{"id":"receipts","version":3}]"#,
        r#"[{"id":"receipts","enabled":true},{}]"#,
    ] {
        assert_eq!(plugins(input), Err(Error::InvalidJson), "{input}");
    }
    for input in [
        "{}",
        "null",
        r#"{"hooks":null}"#,
        r#"{"hooks":[]}"#,
        r#"{"hooks":{"SessionStart":false}}"#,
        r#"{"hooks":{"SessionStart":[{}]}}"#,
        r#"{"hooks":{"SessionStart":[[]]}}"#,
        r#"{"hooks":{"SessionStart":[{"hooks":[null]}]}}"#,
        r#"{"hooks":{"SessionStart":[{"hooks":[[]]}]}}"#,
    ] {
        assert_eq!(
            config(Source::CodexUserHooks, input),
            Err(Error::InvalidJson)
        );
    }
    for input in [
        "[]",
        r#"{"disableAllHooks":"false"}"#,
        r#"{"enabledPlugins":[]}"#,
        r#"{"enabledPlugins":{"receipts@market":"yes"}}"#,
    ] {
        assert_eq!(
            config(Source::ClaudeUserSettings, input),
            Err(Error::InvalidJson)
        );
    }
}

#[test]
fn duplicate_known_json_fields_including_null_are_rejected() {
    for input in [
        r#"[{"id":"receipts","id":"receipts"}]"#,
        r#"[{"id":"receipts","enabled":null,"enabled":false}]"#,
        r#"[{"id":"receipts","version":null,"version":"v"}]"#,
    ] {
        assert_eq!(plugins(input), Err(Error::InvalidJson));
    }
    for input in [
        r#"{"disableAllHooks":null,"disableAllHooks":false}"#,
        r#"{"allowManagedHooksOnly":false,"allowManagedHooksOnly":true}"#,
        r#"{"enabledPlugins":{"receipts@market":null,"receipts@market":true}}"#,
        r#"{"hooks":{"SessionStart":null,"SessionStart":[]}}"#,
    ] {
        assert_eq!(
            config(Source::ClaudeManagedSettings, input),
            Err(Error::InvalidJson)
        );
    }
}

#[test]
fn unknown_json_fields_are_ignored_without_capturing_secrets_or_facts() {
    let value = config(Source::ClaudeUserSettings, r#"{"env":{"API_KEY":"SECRET_KEY"},"permissions":{"allow":["SECRET_PERMISSION"]},"messages":["SECRET_MESSAGE"],"history":"SECRET_HISTORY","cookies":"SECRET_COOKIE","hooks_trusted":true,"enabledPlugins":{"unrelated":"SECRET_PLUGIN"},"hooks":{"SECRET_EVENT":[{"command":"SECRET_COMMAND"}]}}"#).unwrap();
    assert!(!format!("{value:?}").contains("SECRET"));
    assert_eq!(value.disable_all_hooks, Value::Absent);
    assert_eq!(value.receipts_enabled, Value::Known(vec![]));
    assert_eq!(value.configured_recognized_events, Value::Known(vec![]));
}

#[test]
fn configured_event_names_exclude_all_handler_contents() {
    let value = config(Source::CodexProjectHooks, r#"{"hooks":{"SessionStart":[{"matcher":"SECRET_MATCHER","hooks":[{"type":"command","command":"SECRET_COMMAND","prompt":"SECRET_PROMPT"}]}],"Stop":[]}}"#).unwrap();
    assert_eq!(
        value.configured_recognized_events,
        Value::Known(vec!["SessionStart".into()])
    );
    assert!(!format!("{value:?}").contains("SECRET"));
    assert_eq!(
        config(Source::CodexUserHooks, r#"{"hooks":{}}"#)
            .unwrap()
            .configured_recognized_events,
        Value::Known(vec![])
    );
    assert_eq!(
        config(Source::CodexUserHooks, r#"{"hooks":{"Stop":null}}"#)
            .unwrap()
            .configured_recognized_events,
        Value::Unknown
    );
}

#[test]
fn source_settings_record_both_enablement_and_admin_values_without_merging() {
    for flag in [true, false] {
        let value = config(Source::ClaudeManagedSettings, &format!(r#"{{"disableAllHooks":{flag},"allowManagedHooksOnly":{flag},"enabledPlugins":{{"receipts@market":{flag}}}}}"#)).unwrap();
        assert_eq!(value.disable_all_hooks, Value::Known(flag));
        assert_eq!(value.allow_managed_hooks_only, Value::Known(flag));
        assert_eq!(
            value.receipts_enabled,
            Value::Known(vec![HostObservedPluginSetting {
                id: "receipts@market".into(),
                enabled: Value::Known(flag)
            }])
        );
    }
}

#[test]
fn codex_toml_decodes_only_authorized_source_fields() {
    for flag in [true, false] {
        let value = config(
            Source::CodexLocalRequirements,
            &format!("allow_managed_hooks_only={flag}\n[features]\nhooks={flag}\n"),
        )
        .unwrap();
        assert_eq!(value.hooks_feature, Value::Known(flag));
        assert_eq!(value.allow_managed_hooks_only, Value::Known(flag));
    }
    let value = config(Source::CodexUserConfig, "[features]\nhooks=true\n[provider]\napi_key='SECRET_KEY'\n[env]\ncookie='SECRET_COOKIE'\n[hooks]\nunknown='SECRET_COMMAND'\n").unwrap();
    assert_eq!(value.hooks_feature, Value::Known(true));
    assert!(!format!("{value:?}").contains("SECRET"));
    assert_eq!(value.configured_recognized_events, Value::Unknown);
    assert_eq!(
        config(Source::CodexUserConfig, "").unwrap().hooks_feature,
        Value::Absent
    );
}

#[test]
fn duplicate_wrong_type_and_malformed_toml_fail_closed() {
    for input in [
        "[features]\nhooks=true\nhooks=false",
        "[features]\n[features]",
        "[features]\nhooks='false'",
        "features=false",
        "features=[]",
        "allow_managed_hooks_only='false'",
        "invalid=[",
        "x={a=1,a=2}",
    ] {
        assert_eq!(
            config(Source::CodexLocalRequirements, input),
            Err(Error::InvalidToml)
        );
    }
}

#[test]
fn malformed_json_has_no_partial_facts_or_secret_diagnostics() {
    for input in [
        r#"{"disableAllHooks":false,"env":"SECRET_TOKEN""#,
        r#"{"disableAllHooks":"SECRET_TOKEN"}"#,
        r#"{"hooks":{"SessionStart":[{"hooks":[]}]},"x":}"#,
    ] {
        let error = config(Source::ClaudeUserSettings, input).unwrap_err();
        assert_eq!(error, Error::InvalidJson);
        assert!(!format!("{error:?}").contains("SECRET"));
    }
    let error = config(Source::CodexUserConfig, "[features]\nhooks='SECRET_TOKEN'").unwrap_err();
    assert_eq!(error, Error::InvalidToml);
    assert!(!format!("{error:?}").contains("SECRET"));
}

#[test]
fn strict_utf8_for_both_formats() {
    for source in [Source::ClaudeUserSettings, Source::CodexUserConfig] {
        assert_eq!(
            decode::configuration(source, &[0xff]),
            Err(Error::InvalidUtf8)
        );
    }
    assert_eq!(decode::claude_plugins(&[0xff]), Err(Error::InvalidUtf8));
}

#[test]
fn exact_json_and_toml_document_bounds_are_enforced_on_complete_bytes() {
    for (source, prefix) in [
        (Source::ClaudeUserSettings, "{}"),
        (Source::CodexUserConfig, "[features]\nhooks=false\n"),
    ] {
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.resize(HOST_STRUCTURED_OBSERVATION_MAX_BYTES, b' ');
        assert!(decode::configuration(source, &bytes).is_ok());
        bytes.push(b' ');
        assert_eq!(
            decode::configuration(source, &bytes),
            Err(Error::StructuredObservationTooLarge)
        );
    }
    let mut bytes = b"[]".to_vec();
    bytes.resize(HOST_PROBE_STDOUT_MAX_BYTES, b' ');
    assert!(decode::claude_plugins(&bytes).is_ok());
    bytes.push(b' ');
    assert_eq!(
        decode::claude_plugins(&bytes),
        Err(Error::StructuredObservationTooLarge)
    );
}

#[test]
fn json_recursion_protection_is_active() {
    let text = format!("{{\"unknown\":{}0{}}}", "[".repeat(256), "]".repeat(256));
    // Normal serde_json protection must remain enabled, including for ignored data.
    // IgnoredAny may skip iteratively; either bounded success or safe rejection
    // is valid for ignored fields, while a known nested type must fail closed.
    let result = config(Source::ClaudeUserSettings, &text);
    if let Ok(value) = result {
        assert_eq!(value.disable_all_hooks, Value::Absent);
    }
    assert_eq!(
        plugins(&format!("{}0{}", "[".repeat(256), "]".repeat(256))),
        Err(Error::InvalidJson)
    );
}

#[derive(Clone)]
struct Fixture {
    plugins: Result<Vec<u8>, Error>,
    files: Vec<(Source, Result<Vec<u8>, Error>)>,
}
impl Sources for Fixture {
    fn plugins(&self, _: &Path) -> Result<Vec<u8>, Error> {
        self.plugins.clone()
    }
    fn configurations(
        &self,
        _: &HostCapabilityObservationRequest,
    ) -> Vec<(Source, Result<Vec<u8>, Error>)> {
        self.files.clone()
    }
}
fn fixture() -> Fixture {
    Fixture {
        plugins: Ok(b"[]".to_vec()),
        files: vec![],
    }
}
fn request(host: HostId) -> HostCapabilityObservationRequest {
    HostCapabilityObservationRequest {
        host,
        project_root: "/synthetic-project".into(),
    }
}

#[test]
fn real_source_semantics_distinguish_present_absent_and_failed_plugins() {
    for (bytes, expected) in [
        (b"[]".to_vec(), Value::Known(false)),
        (
            br#"[{"id":"receipts@market","enabled":false}]"#.to_vec(),
            Value::Known(true),
        ),
    ] {
        let observation = observe(
            &request(HostId::ClaudeCode),
            &Fixture {
                plugins: Ok(bytes),
                ..fixture()
            },
        );
        assert_eq!(observation.plugin_supported, Value::Known(true));
        assert_eq!(observation.plugin_installed, expected);
        assert_eq!(observation.hooks_enabled, Value::Unknown);
    }
    for error in [
        Error::ProbeFailed,
        Error::ProbeTimeout,
        Error::ProbeStdoutTooLarge,
        Error::ProbeStderrTooLarge,
        Error::Unsupported,
    ] {
        let observation = observe(
            &request(HostId::ClaudeCode),
            &Fixture {
                plugins: Err(error),
                ..fixture()
            },
        );
        assert_eq!(observation.plugin_supported, Value::Unknown);
        assert_eq!(observation.plugin_installed, Value::Unknown);
        assert_eq!(observation.plugins, Err(error));
    }
}

#[test]
fn codex_and_headless_do_not_invoke_the_claude_probe() {
    struct NoProbe;
    impl Sources for NoProbe {
        fn plugins(&self, _: &Path) -> Result<Vec<u8>, Error> {
            panic!("wrong Host command")
        }
        fn configurations(
            &self,
            _: &HostCapabilityObservationRequest,
        ) -> Vec<(Source, Result<Vec<u8>, Error>)> {
            vec![]
        }
    }
    for host in [HostId::Codex, HostId::Headless] {
        let value = observe(&request(host), &NoProbe);
        assert_eq!(value.plugins, Err(Error::Unsupported));
        assert_eq!(value.plugin_supported, Value::Unknown);
        assert_eq!(value.hooks_trusted, Value::Unknown);
    }
}

#[test]
fn partial_source_failure_preserves_independent_facts() {
    let state = Fixture {
        files: vec![
            (
                Source::ClaudeUserSettings,
                Ok(br#"{"disableAllHooks":true}"#.to_vec()),
            ),
            (Source::ClaudeProjectSettings, Err(Error::PermissionDenied)),
            (
                Source::ClaudeLocalSettings,
                Ok(br#"{"disableAllHooks":false,"bad":}"#.to_vec()),
            ),
            (Source::ClaudeManagedSettings, Err(Error::Missing)),
        ],
        ..fixture()
    };
    let value = observe(&request(HostId::ClaudeCode), &state);
    assert_eq!(value.plugin_installed, Value::Known(false));
    assert_eq!(
        value.configurations[0]
            .result
            .as_ref()
            .unwrap()
            .disable_all_hooks,
        Value::Known(true)
    );
    assert_eq!(value.configurations[1].result, Err(Error::PermissionDenied));
    assert_eq!(value.configurations[2].result, Err(Error::InvalidJson));
    assert_eq!(value.configurations[3].result, Err(Error::Missing));
    assert_eq!(value.hooks_enabled, Value::Unknown);
    assert_eq!(value, observe(&request(HostId::ClaudeCode), &state));
}

#[test]
fn file_policy_and_project_config_never_prove_effective_authority() {
    for flag in [true, false] {
        for (host, source, input) in [
            (
                HostId::ClaudeCode,
                Source::ClaudeManagedSettings,
                format!(r#"{{"allowManagedHooksOnly":{flag},"disableAllHooks":{flag}}}"#),
            ),
            (
                HostId::Codex,
                Source::CodexLocalRequirements,
                format!("allow_managed_hooks_only={flag}\n[features]\nhooks={flag}"),
            ),
            (
                HostId::Codex,
                Source::CodexProjectConfig,
                format!("[features]\nhooks={flag}"),
            ),
        ] {
            let value = observe(
                &request(host),
                &Fixture {
                    files: vec![(source, Ok(input.into_bytes()))],
                    ..fixture()
                },
            );
            assert_eq!(value.hooks_allowed_by_admin_policy, Value::Unknown);
            assert_eq!(value.hooks_trusted, Value::Unknown);
            assert_eq!(value.hooks_enabled, Value::Unknown);
            assert_eq!(value.hooks_supported, Value::Unknown);
        }
    }
}

#[test]
fn configured_events_do_not_invent_required_coverage_or_receipts_ownership() {
    for hooks in [
        "{}",
        r#"{"SessionStart":[{"hooks":[{}]}]}"#,
        r#"{"SessionStart":[{"hooks":[{}]}],"Stop":[{"hooks":[{}]}]}"#,
    ] {
        let value = observe(
            &request(HostId::Codex),
            &Fixture {
                files: vec![(
                    Source::CodexUserHooks,
                    Ok(format!(r#"{{"hooks":{hooks}}}"#).into_bytes()),
                )],
                ..fixture()
            },
        );
        assert_eq!(value.hooks_configured, Value::Unknown);
        assert_eq!(value.required_hook_coverage_satisfied, Value::Unknown);
        assert_eq!(
            value.hook_coverage_class,
            HostCapabilityHookCoverageClass::Unknown
        );
    }
}

#[test]
fn existing_composition_accepts_none_fingerprint_and_denies_unknown_embedded() {
    for host in [HostId::ClaudeCode, HostId::Codex, HostId::Headless] {
        let value = observe(&request(host), &fixture());
        for current in [true, false] {
            let inputs = value.report_selection_inputs(current, Stale::None, None);
            assert_eq!(inputs.validity_fingerprint, None);
            assert_eq!(inputs.hook_definition_digest, None);
            assert_eq!(inputs.relevant_config_digest, None);
            assert_eq!(inputs.hooks_trusted, None);
            match compose_host_capability_report_selection(&inputs).unwrap() {
                Outcome::Selected { report } => {
                    assert!(current);
                    assert_eq!(report.selected_mode(), Mode::Supervised);
                    assert_eq!(report.validity_fingerprint(), None);
                }
                Outcome::ReprobeRequired => assert!(!current),
                other => panic!("unexpected {other:?}"),
            }
        }
    }
}

#[test]
fn adaptation_preserves_known_true_false_unknown_without_policy() {
    // Synthetic already-observed facts test mechanical adaptation only. No
    // production source is claimed to establish effective trust/admin/coverage.
    for fact in [
        Value::Known(true),
        Value::Known(false),
        Value::Unknown,
        Value::Absent,
    ] {
        let mut value = observe(&request(HostId::Codex), &fixture());
        value.plugin_supported = fact.clone();
        value.plugin_installed = fact.clone();
        value.hooks_supported = fact.clone();
        value.hooks_configured = fact.clone();
        value.hook_trust_required = fact.clone();
        value.hooks_trusted = fact.clone();
        value.hooks_enabled = fact.clone();
        value.hooks_allowed_by_admin_policy = fact.clone();
        value.required_hook_coverage_satisfied = fact.clone();
        let input = value.report_selection_inputs(true, Stale::None, None);
        for projected in [
            input.plugin_supported,
            input.plugin_installed,
            input.hooks_supported,
            input.hooks_configured,
            input.hook_trust_required,
            input.hooks_trusted,
            input.hooks_enabled,
            input.hooks_allowed_by_admin_policy,
            input.required_hook_coverage_satisfied,
        ] {
            assert_eq!(projected, fact.policy_value());
        }
        assert_eq!(input.probe_status, HostCapabilityProbeStatus::Partial);
    }
    for coverage in HostCapabilityHookCoverageClass::ALL {
        let mut value = observe(&request(HostId::Codex), &fixture());
        value.hook_coverage_class = coverage;
        assert_eq!(
            value
                .report_selection_inputs(true, Stale::None, None)
                .hook_coverage_class,
            coverage
        );
    }
}

#[test]
fn identities_are_opaque_and_never_reinterpreted_as_digests() {
    let mut value = observe(&request(HostId::Codex), &fixture());
    value.hook_definition_identity = Value::Known(" opaque:host-definition-v2 ".into());
    value.relevant_config_identity = Value::Known(" opaque:host-config-v3 ".into());
    let input = value.report_selection_inputs(true, Stale::None, None);
    assert_eq!(
        value.hook_definition_identity,
        Value::Known(" opaque:host-definition-v2 ".into())
    );
    assert_eq!(input.hook_definition_digest, None);
    assert_eq!(input.relevant_config_digest, None);
    assert_eq!(input.validity_fingerprint, None);
}

struct Directory(std::path::PathBuf);
impl Directory {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "host-observation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn actual_file_reads_are_bounded_and_do_not_mutate_contents() {
    let directory = Directory::new();
    let path = directory.0.join("settings.json");
    assert_eq!(io::read_document(&path), Err(Error::Missing));
    let mut bytes = b"{}".to_vec();
    bytes.resize(HOST_STRUCTURED_OBSERVATION_MAX_BYTES, b' ');
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(io::read_document(&path).unwrap(), bytes);
    bytes.push(b' ');
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        io::read_document(&path),
        Err(Error::StructuredObservationTooLarge)
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(io::read_document(&directory.0), Err(Error::NotRegularFile));
}

#[cfg(unix)]
#[test]
fn symlink_redirects_are_not_followed() {
    let directory = Directory::new();
    let target = directory.0.join("secret");
    std::fs::write(&target, b"SECRET").unwrap();
    let path = directory.0.join("settings.json");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert_eq!(io::read_document(&path), Err(Error::NotRegularFile));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn managed_drop_in_enumeration_has_a_file_count_bound() {
    let directory = Directory::new();
    for index in 0..HOST_MANAGED_DROP_IN_MAX_FILES {
        std::fs::write(directory.0.join(format!("{index:02}.json")), b"{}").unwrap();
    }
    let paths = io::drop_ins(&directory.0).unwrap();
    assert_eq!(paths.len(), HOST_MANAGED_DROP_IN_MAX_FILES);
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]));
    std::fs::write(directory.0.join("extra.unrelated"), b"{}").unwrap();
    assert_eq!(io::drop_ins(&directory.0), Err(Error::TooManyFiles));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn child(mode: &str, timeout: Duration) -> Result<Vec<u8>, Error> {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "host_capability_observation::tests::probe_child",
            "--nocapture",
        ])
        .env("RECEIPTS_HOST_TEST_PROBE", mode);
    io::process::capture(command, timeout)
}

#[test]
#[ignore = "invoked in an isolated subprocess by transport tests"]
fn probe_child() {
    use std::io::Write;
    let Ok(mode) = std::env::var("RECEIPTS_HOST_TEST_PROBE") else {
        return;
    };
    match mode.as_str() {
        "stdout-overflow" => std::io::stdout()
            .write_all(&vec![b'x'; HOST_PROBE_STDOUT_MAX_BYTES + 1])
            .unwrap(),
        "stderr-overflow" => std::io::stderr()
            .write_all(&vec![b'x'; HOST_PROBE_STDERR_MAX_BYTES + 1])
            .unwrap(),
        "both" => {
            let writer =
                std::thread::spawn(|| std::io::stderr().write_all(&vec![b'e'; 524288]).unwrap());
            std::io::stdout().write_all(&vec![b'o'; 524288]).unwrap();
            writer.join().unwrap();
        }
        "timeout" => {
            std::io::stdout().write_all(b"[]").unwrap();
            std::thread::sleep(Duration::from_secs(60));
        }
        "failure" => {
            std::io::stdout().write_all(b"[]").unwrap();
            std::process::exit(7);
        }
        "stderr-json" => {
            std::io::stdout().write_all(b"INVALID").unwrap();
            std::io::stderr()
                .write_all(br#"[{"id":"receipts"}]"#)
                .unwrap();
        }
        _ => panic!("unknown synthetic probe"),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn process_drains_both_pipes_and_bounds_each_stream() {
    assert_eq!(
        child("stdout-overflow", HOST_PROBE_TIMEOUT),
        Err(Error::ProbeStdoutTooLarge)
    );
    assert_eq!(
        child("stderr-overflow", HOST_PROBE_TIMEOUT),
        Err(Error::ProbeStderrTooLarge)
    );
    let bytes = child("both", HOST_PROBE_TIMEOUT).unwrap();
    assert!(bytes.len() >= 524288 && bytes.len() <= HOST_PROBE_STDOUT_MAX_BYTES);
    assert!(!bytes.windows(16).any(|part| part == [b'e'; 16]));
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn timeout_and_nonzero_exit_discard_partial_positive_stdout() {
    let start = std::time::Instant::now();
    assert_eq!(
        child("timeout", Duration::from_millis(200)),
        Err(Error::ProbeTimeout)
    );
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(
        child("failure", HOST_PROBE_TIMEOUT),
        Err(Error::ProbeFailed)
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn json_stderr_cannot_rescue_invalid_stdout() {
    let bytes = child("stderr-json", HOST_PROBE_TIMEOUT).unwrap();
    assert!(!bytes.windows(8).any(|part| part == b"receipts"));
    assert_eq!(decode::claude_plugins(&bytes), Err(Error::InvalidJson));
}

#[test]
fn observation_has_no_fingerprint_serialization_hash_or_mutation_path() {
    let implementation = [
        include_str!("host_capability_observation.rs"),
        include_str!("host_capability_observation_decode.rs"),
        include_str!("host_capability_observation_io.rs"),
    ]
    .join("\n");
    for forbidden in [
        "serde(flatten)",
        "serde_json::Value",
        "toml::Value",
        "from_utf8_lossy",
        "Serialize",
        "to_string(",
        "to_vec(",
        "Hasher",
        "sha2",
        "Blake",
        ".output()",
        "File::create",
        "fs::write",
        ".write(true)",
        "vars()",
        "vars_os()",
        "codex exec",
        "claude -p",
    ] {
        assert!(
            !implementation.contains(forbidden),
            "unexpected implementation token {forbidden}"
        );
    }
    assert!(implementation.contains("[\"plugin\", \"list\", \"--json\"]"));
    for shell in ["sh", "bash", "zsh", "fish", "cmd.exe", "powershell"] {
        assert!(!implementation.contains(&format!("Command::new(\"{shell}\")")));
    }
    assert_eq!(HOST_STRUCTURED_OBSERVATION_MAX_BYTES, 1048576);
    assert_eq!(HOST_PROBE_TIMEOUT, Duration::from_secs(10));
    assert_eq!(crate::HostCapabilityValidityInput::ALL.len(), 7);
}
