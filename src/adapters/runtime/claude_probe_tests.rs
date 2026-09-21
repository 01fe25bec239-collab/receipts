use crate::*;
use receipts_workspace_execution::execution::{ProcessTermination, STREAM_CAPTURE_LIMIT_BYTES};

pub(crate) const VERSION: &[u8] = b"2.1.246 (Claude Code)\n";
pub(crate) const HELP: &[u8] = b"Usage: claude [options] [command] [prompt]\n\nOptions:\n  --input-format <format>               Input format (only works with --print):\n                                        text or stream-json\n  --no-session-persistence              Disable persistence\n  --output-format <format>              Output format\n  --permission-mode <mode>              Permission mode\n  -p, --print                           Print response and exit\n  --verbose                            Override verbose mode\n\nCommands:\n  auth                                 Authentication\n";

fn observation(stdout: &[u8]) -> ClaudeProbeObservation<'_> {
    ClaudeProbeObservation {
        stdout,
        stderr: b"",
        exit_code: Some(0),
        capture_complete: true,
        termination: ProcessTermination::Completed,
    }
}
fn parse(help: &[u8]) -> ClaudeCapabilityProbeReport {
    parse_claude_probe(observation(VERSION), observation(help)).unwrap()
}
fn evidence(report: &ClaudeCapabilityProbeReport) -> [ClaudeCapabilityEvidence; 6] {
    [
        report.print,
        report.input_format,
        report.output_format,
        report.no_session_persistence,
        report.permission_mode,
        report.verbose,
    ]
}

#[test]
fn current_style_help_supports_all_six_exact_declarations() {
    let report = parse(HELP);
    assert_eq!(report.version, "2.1.246 (Claude Code)");
    assert_eq!(evidence(&report), [ClaudeCapabilityEvidence::Supported; 6]);
}

#[test]
fn each_exact_declaration_is_independently_recognized_and_all_absences_are_unknown() {
    for (i, declaration) in [
        "-p, --print",
        "--input-format <format>",
        "--output-format <format>",
        "--no-session-persistence",
        "--permission-mode <mode>",
        "--verbose",
    ]
    .iter()
    .enumerate()
    {
        let help = format!("Usage: claude [options]\nOptions:\n  {declaration}  Description\n");
        let mut expected = [ClaudeCapabilityEvidence::Unknown; 6];
        expected[i] = ClaudeCapabilityEvidence::Supported;
        assert_eq!(evidence(&parse(help.as_bytes())), expected);
    }
    assert_eq!(
        evidence(&parse(
            b"Usage: claude [options]\nOptions:\n  --other  Other option\n"
        )),
        [ClaudeCapabilityEvidence::Unknown; 6]
    );
}

#[test]
fn missing_one_declaration_is_unknown() {
    let help = str::from_utf8(HELP).unwrap().replace(
        "  --verbose                            Override verbose mode\n",
        "",
    );
    let mut expected = [ClaudeCapabilityEvidence::Supported; 6];
    expected[5] = ClaudeCapabilityEvidence::Unknown;
    assert_eq!(evidence(&parse(help.as_bytes())), expected);
}

#[test]
fn prose_longer_tokens_wrapped_text_and_other_sections_do_not_declare_flags() {
    let report = parse(b"Usage: claude [options]\nThis mode behaves like --print\nOptions:\n  This mode behaves like --print\n  --verbose-example  Example\n  --other  Description mentions --input-format\n  --another behaves like --permission-mode\n                                        --no-session-persistence\n\n                                        --output-format\nCommands:\n  --print  Not an option\n");
    assert_eq!(evidence(&report), [ClaudeCapabilityEvidence::Unknown; 6]);
}

#[test]
fn duplicate_declarations_fail_closed_with_bounded_evidence() {
    for help in [
        "Usage: claude\nOptions:\n  --print  One\n  --print <value>  Two\n",
        "Usage: claude\nOptions:\n  --print, --print  Duplicate alias\n",
    ] {
        assert_eq!(
            parse_claude_probe(observation(VERSION), observation(help.as_bytes())),
            Err(ClaudeProbeError::AmbiguousCapabilityEvidence(
                ClaudeCapability::Print
            ))
        );
    }
    let mut help = observation(HELP);
    help.stderr = HELP;
    assert_eq!(
        parse_claude_probe(observation(VERSION), help),
        Err(ClaudeProbeError::AmbiguousCapabilityEvidence(
            ClaudeCapability::Print
        ))
    );
}

#[test]
fn missing_nonzero_status_incomplete_process_and_capture_fail_for_both_probes() {
    for kind in [ClaudeProbeKind::Version, ClaudeProbeKind::Help] {
        for case in 0..4 {
            let mut version = observation(VERSION);
            let mut help = observation(HELP);
            let obs = if kind == ClaudeProbeKind::Version {
                &mut version
            } else {
                &mut help
            };
            let expected = match case {
                0 => {
                    obs.exit_code = None;
                    ClaudeProbeError::MissingStatus(kind)
                }
                1 => {
                    obs.exit_code = Some(7);
                    ClaudeProbeError::NonSuccessStatus(kind, 7)
                }
                2 => {
                    obs.termination = ProcessTermination::TimedOutGracefullyTerminated;
                    ClaudeProbeError::IncompleteProcess(kind)
                }
                _ => {
                    obs.capture_complete = false;
                    ClaudeProbeError::IncompleteCapture(kind)
                }
            };
            assert_eq!(parse_claude_probe(version, help), Err(expected));
        }
    }
}

#[test]
fn missing_and_malformed_evidence_fail_closed() {
    assert_eq!(
        parse_claude_probe(observation(b" \n"), observation(HELP)),
        Err(ClaudeProbeError::MissingVersionEvidence)
    );
    assert_eq!(
        parse_claude_probe(observation(VERSION), observation(b" \n")),
        Err(ClaudeProbeError::MissingHelpEvidence)
    );
    for help in [
        "--print",
        "Usage: other\nOptions:\n  --print\n",
        "Usage: claude\n  --print\n",
    ] {
        assert_eq!(
            parse_claude_probe(observation(VERSION), observation(help.as_bytes())),
            Err(ClaudeProbeError::InvalidHelpShape)
        );
    }
}

#[test]
fn invalid_utf8_and_oversized_capture_fail_on_either_channel_of_either_probe() {
    let oversized = vec![b'x'; STREAM_CAPTURE_LIMIT_BYTES as usize + 1];
    for kind in [ClaudeProbeKind::Version, ClaudeProbeKind::Help] {
        for channel in [ClaudeProbeChannel::Stdout, ClaudeProbeChannel::Stderr] {
            for bytes in [&b"\xff"[..], oversized.as_slice()] {
                let mut version = observation(VERSION);
                let mut help = observation(HELP);
                let obs = if kind == ClaudeProbeKind::Version {
                    &mut version
                } else {
                    &mut help
                };
                match channel {
                    ClaudeProbeChannel::Stdout => obs.stdout = bytes,
                    ClaudeProbeChannel::Stderr => obs.stderr = bytes,
                }
                let expected = if bytes.len() == 1 {
                    ClaudeProbeError::InvalidEncoding(kind, channel)
                } else {
                    ClaudeProbeError::CaptureLimitExceeded(kind, channel)
                };
                assert_eq!(parse_claude_probe(version, help), Err(expected));
            }
        }
    }
}

#[test]
fn stdout_version_is_preferred_and_stderr_fallback_is_never_concatenated() {
    let mut version = observation(VERSION);
    version.stderr = b"diagnostic";
    assert_eq!(
        parse_claude_probe(version, observation(HELP))
            .unwrap()
            .version,
        "2.1.246 (Claude Code)"
    );
    version.stdout = b" \n";
    version.stderr = VERSION;
    let mut help = observation(b"");
    help.stderr = HELP;
    assert_eq!(parse_claude_probe(version, help).unwrap(), parse(HELP));
}
