use crate::{
    CODEX_EXEC_HELP_PROBE, CODEX_VERSION_PROBE, CodexCapability, CodexCapabilityEvidence,
    CodexProbeChannel, CodexProbeError, CodexProbeKind, CodexProbeObservation, parse_codex_probe,
};

const CURRENT_HELP: &[u8] = br#"Codex execution

Usage: codex exec [OPTIONS] [PROMPT]

Options:
  -s, --sandbox <SANDBOX_MODE>
      Select the sandbox policy
  --output-schema <FILE>
      Path to a JSON Schema
  --json
      Print events as JSONL
"#;

const HELP_WITHOUT_TARGETS: &[u8] = br#"Codex execution

Usage: codex exec [OPTIONS] [PROMPT]

Options:
  --color <WHEN>
      Control terminal colors
"#;

fn observation(stdout: &'static [u8]) -> CodexProbeObservation<'static> {
    CodexProbeObservation {
        stdout,
        stderr: b"",
        exit_code: Some(0),
        capture_complete: true,
    }
}

fn parse_with_help(
    help: &'static [u8],
) -> Result<crate::CodexCapabilityProbeReport, CodexProbeError> {
    parse_codex_probe(observation(b"codex-cli 0.152.1\n"), observation(help))
}

#[test]
fn command_plan_is_exactly_argv_data() {
    assert_eq!(CODEX_VERSION_PROBE.program, "codex");
    assert_eq!(CODEX_VERSION_PROBE.args, ["--version"]);
    assert_eq!(CODEX_EXEC_HELP_PROBE.program, "codex");
    assert_eq!(CODEX_EXEC_HELP_PROBE.args, ["exec", "--help"]);
}

#[test]
fn current_evidence_supports_all_target_declarations() {
    let report = parse_with_help(CURRENT_HELP).unwrap();

    assert_eq!(report.version, "codex-cli 0.152.1");
    assert_eq!(report.json, CodexCapabilityEvidence::Supported);
    assert_eq!(report.output_schema, CodexCapabilityEvidence::Supported);
    assert_eq!(report.sandbox, CodexCapabilityEvidence::Supported);
}

#[test]
fn capability_detection_is_version_independent() {
    let report = parse_codex_probe(
        observation(b"codex-cli 999.0.0-test\n"),
        observation(CURRENT_HELP),
    )
    .unwrap();

    assert_eq!(report.version, "codex-cli 999.0.0-test");
    assert_eq!(report.json, CodexCapabilityEvidence::Supported);
    assert_eq!(report.output_schema, CodexCapabilityEvidence::Supported);
    assert_eq!(report.sandbox, CodexCapabilityEvidence::Supported);
}

#[test]
fn absent_target_declarations_are_unknown_independently() {
    for (help, expected) in [
        (
            br#"Usage: codex exec [OPTIONS]
Options:
  --output-schema <FILE>
  --sandbox <MODE>
"# as &[u8],
            (
                CodexCapabilityEvidence::Unknown,
                CodexCapabilityEvidence::Supported,
                CodexCapabilityEvidence::Supported,
            ),
        ),
        (
            br#"Usage: codex exec [OPTIONS]
Options:
  --json
  --sandbox <MODE>
"#,
            (
                CodexCapabilityEvidence::Supported,
                CodexCapabilityEvidence::Unknown,
                CodexCapabilityEvidence::Supported,
            ),
        ),
        (
            br#"Usage: codex exec [OPTIONS]
Options:
  --json
  --output-schema <FILE>
"#,
            (
                CodexCapabilityEvidence::Supported,
                CodexCapabilityEvidence::Supported,
                CodexCapabilityEvidence::Unknown,
            ),
        ),
    ] {
        let report = parse_with_help(help).unwrap();
        assert_eq!(
            (report.json, report.output_schema, report.sandbox),
            expected
        );
    }
}

#[test]
fn valid_help_without_targets_reports_all_unknown() {
    let report = parse_with_help(HELP_WITHOUT_TARGETS).unwrap();

    assert_eq!(report.json, CodexCapabilityEvidence::Unknown);
    assert_eq!(report.output_schema, CodexCapabilityEvidence::Unknown);
    assert_eq!(report.sandbox, CodexCapabilityEvidence::Unknown);
}

#[test]
fn substrings_and_prose_do_not_prove_capabilities() {
    let help = br#"Usage: codex exec [OPTIONS]
Options:
  --jsonish
      Use --json if available
  --json is mentioned in prose, not declared
  --color <WHEN>
      Mention --output-schema in prose
      Mention --sandbox in prose
"#;
    let report = parse_with_help(help).unwrap();

    assert_eq!(report.json, CodexCapabilityEvidence::Unknown);
    assert_eq!(report.output_schema, CodexCapabilityEvidence::Unknown);
    assert_eq!(report.sandbox, CodexCapabilityEvidence::Unknown);
}

#[test]
fn valid_help_on_stderr_is_parsed_but_unshaped_warnings_are_not() {
    let stderr_help = CodexProbeObservation {
        stdout: b"",
        stderr: CURRENT_HELP,
        exit_code: Some(0),
        capture_complete: true,
    };
    assert_eq!(
        parse_codex_probe(observation(b"codex-cli test"), stderr_help)
            .unwrap()
            .json,
        CodexCapabilityEvidence::Supported
    );

    let warning = CodexProbeObservation {
        stdout: HELP_WITHOUT_TARGETS,
        stderr: b"warning: use --json, --output-schema, and --sandbox if available",
        exit_code: Some(0),
        capture_complete: true,
    };
    let report = parse_codex_probe(observation(b"codex-cli test"), warning).unwrap();
    assert_eq!(report.json, CodexCapabilityEvidence::Unknown);
    assert_eq!(report.output_schema, CodexCapabilityEvidence::Unknown);
    assert_eq!(report.sandbox, CodexCapabilityEvidence::Unknown);
}

#[test]
fn missing_and_failed_statuses_fail_closed() {
    for (version_status, help_status, expected) in [
        (
            None,
            Some(0),
            CodexProbeError::MissingStatus(CodexProbeKind::Version),
        ),
        (
            Some(7),
            Some(0),
            CodexProbeError::NonSuccessStatus(CodexProbeKind::Version, 7),
        ),
        (
            Some(0),
            None,
            CodexProbeError::MissingStatus(CodexProbeKind::ExecHelp),
        ),
        (
            Some(0),
            Some(9),
            CodexProbeError::NonSuccessStatus(CodexProbeKind::ExecHelp, 9),
        ),
    ] {
        let mut version = observation(b"codex-cli test");
        let mut help = observation(CURRENT_HELP);
        version.exit_code = version_status;
        help.exit_code = help_status;
        assert_eq!(parse_codex_probe(version, help), Err(expected));
    }
}

#[test]
fn incomplete_captures_fail_closed() {
    let mut version = observation(b"codex-cli test");
    version.capture_complete = false;
    assert_eq!(
        parse_codex_probe(version, observation(CURRENT_HELP)),
        Err(CodexProbeError::IncompleteCapture(CodexProbeKind::Version))
    );

    let mut help = observation(CURRENT_HELP);
    help.capture_complete = false;
    assert_eq!(
        parse_codex_probe(observation(b"codex-cli test"), help),
        Err(CodexProbeError::IncompleteCapture(CodexProbeKind::ExecHelp))
    );
}

#[test]
fn missing_evidence_and_invalid_help_shape_fail_closed() {
    assert_eq!(
        parse_codex_probe(observation(b" \n"), observation(CURRENT_HELP)),
        Err(CodexProbeError::MissingVersionEvidence)
    );
    assert_eq!(
        parse_codex_probe(observation(b"codex-cli test"), observation(b" \n")),
        Err(CodexProbeError::MissingHelpEvidence)
    );
    assert_eq!(
        parse_codex_probe(
            observation(b"codex-cli test"),
            observation(b"arbitrary non-empty text")
        ),
        Err(CodexProbeError::InvalidHelpShape)
    );
}

#[test]
fn duplicate_target_declarations_fail_closed() {
    let help = br#"Usage: codex exec [OPTIONS]
Options:
  --json
  --json <MODE>
"#;
    assert_eq!(
        parse_with_help(help),
        Err(CodexProbeError::AmbiguousCapabilityEvidence(
            CodexCapability::Json
        ))
    );
}

#[test]
fn invalid_utf8_in_either_channel_fails_closed() {
    let invalid_version = CodexProbeObservation {
        stdout: b"codex-cli test",
        stderr: b"\xff",
        exit_code: Some(0),
        capture_complete: true,
    };
    assert_eq!(
        parse_codex_probe(invalid_version, observation(CURRENT_HELP)),
        Err(CodexProbeError::InvalidEncoding(
            CodexProbeKind::Version,
            CodexProbeChannel::Stderr
        ))
    );

    let invalid_help = CodexProbeObservation {
        stdout: b"\xff",
        stderr: b"",
        exit_code: Some(0),
        capture_complete: true,
    };
    assert_eq!(
        parse_codex_probe(observation(b"codex-cli test"), invalid_help),
        Err(CodexProbeError::InvalidEncoding(
            CodexProbeKind::ExecHelp,
            CodexProbeChannel::Stdout
        ))
    );
}

#[test]
fn duplicate_declarations_across_channels_fail_closed() {
    let help = CodexProbeObservation {
        stdout: CURRENT_HELP,
        stderr: CURRENT_HELP,
        exit_code: Some(0),
        capture_complete: true,
    };
    assert_eq!(
        parse_codex_probe(observation(b"codex-cli test"), help),
        Err(CodexProbeError::AmbiguousCapabilityEvidence(
            CodexCapability::Json
        ))
    );
}
