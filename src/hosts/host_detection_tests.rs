//! Tests for the pure host detection/selection policy.

use std::collections::HashSet;

use super::*;

/// The full automatic-signal matrix, no override:
/// `(false, false) => Headless`, `(true, false) => ClaudeCode`,
/// `(false, true) => Codex`, `(true, true) => ambiguity error`.
///
/// Covers TEST-01, TEST-02, TEST-03, TEST-04, and TEST-09.
#[test]
fn automatic_signal_matrix_resolves_exactly() {
    let expected: [(HostDetectionSignals, Result<HostId, HostDetectionError>); 4] = [
        (HostDetectionSignals::NONE, Ok(HostId::Headless)),
        (HostDetectionSignals::CLAUDE_ONLY, Ok(HostId::ClaudeCode)),
        (HostDetectionSignals::CODEX_ONLY, Ok(HostId::Codex)),
        (
            HostDetectionSignals::BOTH,
            Err(HostDetectionError::AmbiguousAutomaticDetection {
                detected: [HostId::ClaudeCode, HostId::Codex],
            }),
        ),
    ];

    for (signals, expected) in expected {
        assert_eq!(
            resolve_host(signals, None),
            expected,
            "automatic resolution must match the frozen matrix"
        );
    }
}

/// Every signal combination is representable by the exhaustive boolean
/// pair, so iterating all four pairs proves the matrix above is complete
/// rather than sampled.
#[test]
fn signal_matrix_covers_every_boolean_combination() {
    let mut seen = HashSet::new();
    for claude in [false, true] {
        for codex in [false, true] {
            let signals = HostDetectionSignals {
                claude_detected: claude,
                codex_detected: codex,
            };
            assert!(seen.insert(signals), "each combination must be distinct");
            // Resolution never panics and always terminates with a verdict.
            let _ = resolve_host(signals, None);
        }
    }
    assert_eq!(seen.len(), 4);
}

/// An explicit Claude override is authoritative over every automatic-signal
/// combination. Covers the Claude half of TEST-05.
#[test]
fn explicit_claude_override_beats_every_signal_combination() {
    for signals in ALL_SIGNALS {
        assert_eq!(
            resolve_host(signals, Some(HostId::ClaudeCode)),
            Ok(HostId::ClaudeCode),
            "explicit Claude override must win for {signals:?}"
        );
    }
}

/// An explicit Codex override is authoritative over every automatic-signal
/// combination. Covers the Codex half of TEST-06.
#[test]
fn explicit_codex_override_beats_every_signal_combination() {
    for signals in ALL_SIGNALS {
        assert_eq!(
            resolve_host(signals, Some(HostId::Codex)),
            Ok(HostId::Codex),
            "explicit Codex override must win for {signals:?}"
        );
    }
}

/// An explicit Headless override is authoritative over every
/// automatic-signal combination, including both-hosts-detected. Covers the
/// Headless half of TEST-07.
#[test]
fn explicit_headless_override_beats_every_signal_combination() {
    for signals in ALL_SIGNALS {
        assert_eq!(
            resolve_host(signals, Some(HostId::Headless)),
            Ok(HostId::Headless),
            "explicit Headless override must win for {signals:?}"
        );
    }
}

/// The complete override × signals decision table: every override value
/// (`None`, `Some(ClaudeCode)`, `Some(Codex)`, `Some(Headless)`) against
/// every automatic-signal combination. Overrides are returned verbatim;
/// only `None` consults signals. Covers TEST-05, TEST-06, TEST-07, and
/// TEST-10 as one exhaustive matrix.
#[test]
fn complete_override_matrix_is_authoritative_over_signals() {
    let overrides = [
        None,
        Some(HostId::ClaudeCode),
        Some(HostId::Codex),
        Some(HostId::Headless),
    ];

    for override_id in overrides {
        for signals in ALL_SIGNALS {
            let resolved = resolve_host(signals, override_id);

            if let Some(expected) = override_id {
                // Rule 1: the explicit override is authoritative, even when
                // it conflicts with or is absent from automatic detection —
                // including BOTH signals, which would otherwise be an
                // ambiguity error.
                assert_eq!(
                    resolved,
                    Ok(expected),
                    "override {expected:?} must beat {signals:?}"
                );
            } else {
                match (signals.claude_detected, signals.codex_detected) {
                    (false, false) => assert_eq!(resolved, Ok(HostId::Headless)),
                    (true, false) => assert_eq!(resolved, Ok(HostId::ClaudeCode)),
                    (false, true) => assert_eq!(resolved, Ok(HostId::Codex)),
                    (true, true) => assert_eq!(
                        resolved,
                        Err(HostDetectionError::AmbiguousAutomaticDetection {
                            detected: [HostId::ClaudeCode, HostId::Codex],
                        })
                    ),
                }
            }
        }
    }
}

/// Resolution is deterministic: evaluating the entire matrix repeatedly
/// yields identical results on every pass. Covers TEST-08.
#[test]
fn resolution_is_deterministic_under_repeated_evaluation() {
    let overrides = [
        None,
        Some(HostId::ClaudeCode),
        Some(HostId::Codex),
        Some(HostId::Headless),
    ];

    let first_pass: Vec<_> = overrides
        .iter()
        .flat_map(|override_id| {
            ALL_SIGNALS
                .iter()
                .map(move |signals| (*override_id, *signals, resolve_host(*signals, *override_id)))
        })
        .collect();

    for _ in 0..32 {
        let pass: Vec<_> = overrides
            .iter()
            .flat_map(|override_id| {
                ALL_SIGNALS.iter().map(move |signals| {
                    (*override_id, *signals, resolve_host(*signals, *override_id))
                })
            })
            .collect();
        assert_eq!(pass, first_pass, "resolution must be deterministic");
    }
}

/// Ambiguity fails closed as a typed, diagnosable error: it preserves the
/// conflicting identities, renders them through `Display` and `Debug`, and
/// implements `std::error::Error`. It is never a silently chosen host.
#[test]
fn ambiguity_error_is_explicit_and_diagnosable() {
    let error = resolve_host(HostDetectionSignals::BOTH, None)
        .expect_err("simultaneous detection without override must fail");

    assert_eq!(
        error,
        HostDetectionError::AmbiguousAutomaticDetection {
            detected: [HostId::ClaudeCode, HostId::Codex],
        }
    );

    let rendered = error.to_string();
    assert!(rendered.contains("ambiguous"));
    assert!(rendered.contains("CLAUDE_CODE"));
    assert!(rendered.contains("CODEX"));

    let debugged = format!("{error:?}");
    assert!(debugged.contains("AmbiguousAutomaticDetection"));
    // The derived `Debug` of `HostId` renders Rust variant names.
    assert!(debugged.contains("ClaudeCode"));
    assert!(debugged.contains("Codex"));

    // Trait-object usability without dependencies.
    let boxed: &dyn std::error::Error = &error;
    assert!(!boxed.to_string().is_empty());
}

const ALL_SIGNALS: [HostDetectionSignals; 4] = [
    HostDetectionSignals::NONE,
    HostDetectionSignals::CLAUDE_ONLY,
    HostDetectionSignals::CODEX_ONLY,
    HostDetectionSignals::BOTH,
];
